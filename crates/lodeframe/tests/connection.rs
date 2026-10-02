// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Connection layer over in-memory pipes and a real TCP listener.
use std::{io, time::Duration};

use lodeframe::{
    net::{Config, Connection, serve},
    protocol::{Error, State, VarInt, packets::handshake::Intention},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, duplex},
    net::{TcpListener, TcpStream},
    time::timeout,
};

const T: Duration = Duration::from_secs(5);

fn intention(next_state: i32) -> Intention {
    Intention {
        protocol_version: VarInt(lodeframe::protocol::PROTOCOL_VERSION),
        server_address: "localhost".into(),
        server_port: 25565,
        next_state: VarInt(next_state),
    }
}

#[tokio::test]
async fn frames_roundtrip_with_and_without_compression() {
    let (a, b) = duplex(1 << 16);
    let (mut a, mut b) = (Connection::new(a, T), Connection::new(b, T));
    let big = vec![3u8; 10_000];
    for threshold in [None, Some(64)] {
        a.set_compression(threshold);
        b.set_compression(threshold);
        a.write_frame(&[1, 2, 3]).await.unwrap();
        a.write_frame(&big).await.unwrap();
        assert_eq!(b.read_frame().await.unwrap(), [1, 2, 3]);
        assert_eq!(b.read_frame().await.unwrap(), big);
    }
}

#[tokio::test]
async fn frames_written_together_are_read_back_one_by_one() {
    let (a, b) = duplex(1 << 16);
    let (mut a, mut b) = (Connection::new(a, T), Connection::new(b, T));
    let bodies = vec![vec![1, 2, 3], vec![3u8; 10_000], vec![9], vec![4u8; 100]];
    for threshold in [None, Some(64)] {
        a.set_compression(threshold);
        b.set_compression(threshold);
        a.write_frames(&bodies).await.unwrap();
        for body in &bodies {
            assert_eq!(&b.read_frame().await.unwrap(), body);
        }
    }
}

#[tokio::test]
async fn frames_written_together_are_the_same_bytes_as_written_one_by_one() {
    let bodies = vec![vec![1, 2, 3], vec![3u8; 10_000], vec![9]];
    for threshold in [None, Some(64)] {
        let mut wires = Vec::new();
        for together in [true, false] {
            let (a, mut b) = duplex(1 << 16);
            let mut a = Connection::new(a, T);
            a.set_compression(threshold);
            if together {
                a.write_frames(&bodies).await.unwrap();
            } else {
                for body in &bodies {
                    a.write_frame(body).await.unwrap();
                }
            }
            // the connection owns its end; dropping it lets the other side read to the end
            drop(a);
            let mut wire = Vec::new();
            b.read_to_end(&mut wire).await.unwrap();
            wires.push(wire);
        }
        assert_eq!(wires[0], wires[1]);
    }
}

#[tokio::test]
async fn the_handshake_picks_the_next_state() {
    for (next, state) in [(1, State::Status), (2, State::Login), (3, State::Login)] {
        let (a, b) = duplex(1024);
        let (mut a, mut b) = (Connection::new(a, T), Connection::new(b, T));
        a.write_packet(&intention(next)).await.unwrap();
        assert_eq!(b.read_handshake().await.unwrap(), intention(next));
        assert_eq!(b.state(), state);
    }
    let (a, b) = duplex(1024);
    let (mut a, mut b) = (Connection::new(a, T), Connection::new(b, T));
    a.write_packet(&intention(9)).await.unwrap();
    assert!(b.read_handshake().await.is_err());
}

#[tokio::test]
async fn a_silent_peer_times_out() {
    let (_a, b) = duplex(1024);
    let mut b = Connection::new(b, Duration::from_millis(50));
    match b.read_frame().await {
        Err(Error::Io(e)) => assert_eq!(e.kind(), io::ErrorKind::TimedOut),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn a_bad_connection_is_dropped_and_the_server_keeps_serving() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let config = Config {
        read_timeout: Duration::from_millis(200),
        ..Config::default()
    };
    tokio::spawn(serve(listener, config, |mut conn, _, _| async move {
        conn.write_frame(&[0x00, b'o', b'k']).await
    }));

    // Garbage length prefix: closed, not hung.
    let mut bad = TcpStream::connect(addr).await.unwrap();
    bad.write_all(&[0xff, 0xff, 0xff, 0xff, 0xff])
        .await
        .unwrap();
    let mut rest = Vec::new();
    timeout(T, bad.read_to_end(&mut rest)).await.unwrap().ok();
    assert!(rest.is_empty());

    // Silent client: closed by the read timeout.
    let mut idle = TcpStream::connect(addr).await.unwrap();
    let mut rest = Vec::new();
    timeout(T, idle.read_to_end(&mut rest)).await.unwrap().ok();
    assert!(rest.is_empty());

    // A good client is still served.
    let mut good = Connection::new(TcpStream::connect(addr).await.unwrap(), T);
    good.write_packet(&intention(1)).await.unwrap();
    assert_eq!(good.read_frame().await.unwrap(), [0x00, b'o', b'k']);
}

#[tokio::test]
async fn the_server_list_ping_is_answered() {
    use lodeframe::{
        protocol::packets::status::{PingRequest, PongResponse, StatusRequest, StatusResponse},
        status::{StatusInfo, respond},
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(serve(
        listener,
        Config::default(),
        |mut conn, _, _| async move {
            let mut info = StatusInfo::new("hello");
            info.online = 3;
            respond(&mut conn, &info).await
        },
    ));

    let mut c = Connection::new(TcpStream::connect(addr).await.unwrap(), T);
    c.write_packet(&intention(1)).await.unwrap();
    c.write_packet(&StatusRequest).await.unwrap();
    let StatusResponse { json } = c.read_packet().await.unwrap();
    assert!(
        json.contains(r#""text":"hello""#) && json.contains(r#""online":3"#),
        "{json}"
    );
    c.write_packet(&PingRequest { payload: 42 }).await.unwrap();
    assert_eq!(c.read_packet::<PongResponse>().await.unwrap().payload, 42);
}

#[tokio::test]
async fn the_handler_is_given_the_address_the_connection_came_from() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let tx = std::sync::Mutex::new(Some(tx));
    tokio::spawn(serve(listener, Config::default(), move |_conn, _, peer| {
        if let Some(tx) = tx.lock().unwrap().take() {
            let _ = tx.send(peer);
        }
        async { Ok(()) }
    }));

    let stream = TcpStream::connect(addr).await.unwrap();
    let local = stream.local_addr().unwrap();
    let mut c = Connection::new(stream, T);
    c.write_packet(&intention(1)).await.unwrap();
    assert_eq!(rx.await.unwrap(), local);
}
