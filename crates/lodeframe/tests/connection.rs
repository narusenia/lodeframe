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
    };
    tokio::spawn(serve(listener, config, |mut conn, _| async move {
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
