// SPDX-License-Identifier: Apache-2.0 OR MIT
//! The PROXY protocol header on a real TCP listener: what `serve` hands to the handler, and
//! which connections it turns away before the handler is asked.

use std::{
    net::{IpAddr, SocketAddr},
    time::Duration,
};

use lodeframe::{
    net::{Config, Connection, ProxyProtocol, serve},
    protocol::{
        VarInt, encode_frame, packet_body,
        packets::{handshake::Intention, status::StatusRequest},
    },
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::mpsc,
    time::{sleep, timeout},
};

const T: Duration = Duration::from_secs(5);

/// What the handler saw of a connection: the peer it was given, and the proxied client.
type Seen = (SocketAddr, Option<SocketAddr>);

fn loopback() -> IpAddr {
    "127.0.0.1".parse().unwrap()
}

fn optional() -> ProxyProtocol {
    ProxyProtocol::Optional {
        trusted: vec![loopback()],
    }
}

fn required() -> ProxyProtocol {
    ProxyProtocol::Required {
        trusted: vec![loopback()],
    }
}

fn elsewhere(required: bool) -> ProxyProtocol {
    let trusted = vec!["203.0.113.1".parse().unwrap()];
    if required {
        ProxyProtocol::Required { trusted }
    } else {
        ProxyProtocol::Optional { trusted }
    }
}

/// A listener with `mode`, whose handler reports what it saw.
async fn listen(mode: ProxyProtocol) -> (SocketAddr, mpsc::UnboundedReceiver<Seen>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::unbounded_channel();
    let config = Config {
        read_timeout: Duration::from_millis(500),
        proxy_protocol: mode,
        ..Config::default()
    };
    tokio::spawn(serve(listener, config, move |conn, _, peer| {
        let _ = tx.send((peer, conn.proxied_addr()));
        async { Ok(()) }
    }));
    (addr, rx)
}

fn handshake() -> Vec<u8> {
    let intention = Intention {
        protocol_version: VarInt(0),
        server_address: "localhost".into(),
        server_port: 25565,
        next_state: VarInt(1),
    };
    let mut wire = Vec::new();
    encode_frame(&packet_body(&intention).unwrap(), None, &mut wire).unwrap();
    wire
}

/// Sends `bytes` as one write, then waits for the server to end the connection, and returns
/// what the handler saw, or `None` if it was never asked.
async fn send(mode: ProxyProtocol, bytes: &[u8]) -> Option<Seen> {
    let (addr, mut rx) = listen(mode).await;
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(bytes).await.unwrap();
    let mut rest = Vec::new();
    let _ = timeout(T, stream.read_to_end(&mut rest)).await.unwrap();
    rx.try_recv().ok()
}

fn v1(line: &str) -> Vec<u8> {
    [line.as_bytes(), &handshake()].concat()
}

fn v2(command: u8, family: u8, body: &[u8]) -> Vec<u8> {
    let mut h = b"\r\n\r\n\0\r\nQUIT\n".to_vec();
    h.extend([0x20 | command, family]);
    h.extend((body.len() as u16).to_be_bytes());
    h.extend(body);
    h.extend(handshake());
    h
}

fn v4_body(ip: [u8; 4], port: u16) -> Vec<u8> {
    let mut b = ip.to_vec();
    b.extend([10, 0, 0, 1]);
    b.extend(port.to_be_bytes());
    b.extend(25565u16.to_be_bytes());
    b
}

#[tokio::test]
async fn the_client_in_a_v1_header_is_the_peer() {
    let bytes = v1("PROXY TCP4 203.0.113.7 10.0.0.1 40000 25565\r\n");

    let (peer, proxied) = send(optional(), &bytes).await.unwrap();

    assert_eq!(peer, "203.0.113.7:40000".parse().unwrap());
    assert_eq!(proxied, Some(peer));
}

#[tokio::test]
async fn the_client_in_a_v2_header_is_the_peer_with_tlvs_and_all() {
    let mut body = v4_body([203, 0, 113, 8], 41000);
    body.extend([0x04, 0x00, 0x01, 0xff]);
    let (peer, proxied) = send(required(), &v2(1, 0x11, &body)).await.unwrap();
    assert_eq!(peer, "203.0.113.8:41000".parse().unwrap());
    assert_eq!(proxied, Some(peer));

    let mut body = vec![0u8; 36];
    body[0] = 0x20;
    body[1] = 0x01;
    body[15] = 9;
    body[32..34].copy_from_slice(&42u16.to_be_bytes());
    let (peer, _) = send(required(), &v2(1, 0x21, &body)).await.unwrap();
    assert_eq!(peer, "[2001::9]:42".parse().unwrap());
}

#[tokio::test]
async fn a_header_that_names_no_client_leaves_the_sockets_peer() {
    for bytes in [
        v1("PROXY UNKNOWN\r\n"),
        v2(0, 0x00, &[]),
        v2(0, 0x11, &v4_body([1, 2, 3, 4], 5)),
    ] {
        let (peer, proxied) = send(optional(), &bytes).await.unwrap();

        assert_eq!(peer.ip(), loopback());
        assert_eq!(proxied, None);
    }
}

#[tokio::test]
async fn a_header_that_comes_in_pieces_is_read_whole() {
    let (addr, mut rx) = listen(optional()).await;
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.set_nodelay(true).unwrap();
    let bytes = v1("PROXY TCP4 203.0.113.7 10.0.0.1 40000 25565\r\n");
    for chunk in bytes.chunks(7) {
        stream.write_all(chunk).await.unwrap();
        sleep(Duration::from_millis(5)).await;
    }
    let mut rest = Vec::new();
    let _ = timeout(T, stream.read_to_end(&mut rest)).await.unwrap();

    let (peer, _) = rx.try_recv().unwrap();
    assert_eq!(peer, "203.0.113.7:40000".parse().unwrap());
}

#[tokio::test]
async fn a_handshake_in_the_same_write_as_the_header_is_not_lost() {
    // the handler gets a connection that reads the handshake it did not see arrive
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let config = Config {
        proxy_protocol: optional(),
        ..Config::default()
    };
    let (tx, mut rx) = mpsc::unbounded_channel();
    tokio::spawn(serve(listener, config, move |mut conn, intention, _| {
        let tx = tx.clone();
        async move {
            let _ = tx.send(intention.next_state.0);
            let _: StatusRequest = conn.read_packet().await?;
            let _ = tx.send(-1);
            Ok(())
        }
    }));
    let mut bytes = v1("PROXY TCP4 203.0.113.7 10.0.0.1 40000 25565\r\n");
    encode_frame(&packet_body(&StatusRequest).unwrap(), None, &mut bytes).unwrap();
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(&bytes).await.unwrap();

    assert_eq!(timeout(T, rx.recv()).await.unwrap(), Some(1));
    assert_eq!(timeout(T, rx.recv()).await.unwrap(), Some(-1));
}

#[tokio::test]
async fn a_connection_without_a_header_is_let_in_when_one_is_optional() {
    for mode in [optional(), elsewhere(false)] {
        let (peer, proxied) = send(mode, &handshake()).await.unwrap();

        assert_eq!(peer.ip(), loopback());
        assert_eq!(proxied, None);
    }
}

#[tokio::test]
async fn a_header_from_an_address_that_is_not_trusted_is_refused() {
    let v1 = v1("PROXY TCP4 203.0.113.7 10.0.0.1 40000 25565\r\n");
    let v2 = v2(1, 0x11, &v4_body([203, 0, 113, 7], 40000));
    for bytes in [v1, v2] {
        assert_eq!(send(elsewhere(false), &bytes).await, None);
        assert_eq!(send(elsewhere(true), &bytes).await, None);
    }
}

#[tokio::test]
async fn a_required_header_is_asked_of_every_connection() {
    // trusted but without one
    assert_eq!(send(required(), &handshake()).await, None);
    // not trusted, with or without
    assert_eq!(send(elsewhere(true), &handshake()).await, None);
}

#[tokio::test]
async fn a_header_is_not_a_handshake_when_none_is_expected() {
    let bytes = v1("PROXY TCP4 203.0.113.7 10.0.0.1 40000 25565\r\n");

    assert_eq!(send(ProxyProtocol::Off, &bytes).await, None);
    assert!(send(ProxyProtocol::Off, &handshake()).await.is_some());
}

#[tokio::test]
async fn a_header_written_wrong_is_refused() {
    let mut version = v2(1, 0x11, &v4_body([1, 2, 3, 4], 5));
    version[12] = 0x11;
    let too_long = [b"PROXY TCP4 ".as_slice(), &[b'1'; 200]].concat();
    for bytes in [
        v1("PROXY TCP4 203.0.113.7 10.0.0.1 40000\r\n"),
        v1("PROXY TCP4 not-an-ip 10.0.0.1 1 2\r\n"),
        too_long,
        version,
        v2(1, 0x11, &[0; 4]),
        // half of a signature, then a handshake that is not
        [b"PROX".as_slice(), &handshake()].concat(),
    ] {
        assert_eq!(send(required(), &bytes).await, None, "{bytes:?}");
    }
}

#[tokio::test]
async fn a_header_that_never_ends_times_out() {
    let (addr, mut rx) = listen(optional()).await;
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(b"PROXY TCP4 203.0.113.7").await.unwrap();

    let mut rest = Vec::new();
    let _ = timeout(T, stream.read_to_end(&mut rest)).await.unwrap();

    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn the_handler_of_a_plain_connection_still_gets_a_connection_that_reads() {
    let (addr, mut rx) = listen(optional()).await;
    let stream = TcpStream::connect(addr).await.unwrap();
    let mut conn = Connection::new(stream, T);
    conn.write_frame(&handshake()[1..]).await.unwrap();

    assert!(timeout(T, rx.recv()).await.unwrap().is_some());
}
