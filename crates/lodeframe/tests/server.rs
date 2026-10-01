// SPDX-License-Identifier: Apache-2.0 OR MIT
//! A started server answers the server list, lets bots in, and drops them when it stops.

use std::time::Duration;

use lodeframe::{
    chunk::FlatGenerator,
    protocol::{
        FrameDecoder, VarInt, encode_frame, packet_body,
        packets::{
            handshake::Intention,
            status::{StatusRequest, StatusResponse},
        },
        split_packet_id,
    },
    registry::Registries,
    server::{RunningServer, Server},
    world::World,
};
use lodeframe_bot::Bot;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

async fn start(motd: &str) -> RunningServer {
    Server::new("127.0.0.1:0")
        .motd(motd)
        .start(|registries: &Registries| {
            let mut world = World::new(registries, FlatGenerator::default());
            world.view_distance = 2;
            world
        })
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_server_list_shows_the_motd() {
    let server = start("a test server").await;
    let mut stream = TcpStream::connect(server.addr()).await.unwrap();

    for body in [
        packet_body(&Intention {
            protocol_version: VarInt(0),
            server_address: "localhost".into(),
            server_port: 25565,
            next_state: VarInt(1),
        })
        .unwrap(),
        packet_body(&StatusRequest).unwrap(),
    ] {
        let mut wire = Vec::new();
        encode_frame(&body, None, &mut wire).unwrap();
        stream.write_all(&wire).await.unwrap();
    }
    let mut decoder = FrameDecoder::new();
    let mut chunk = [0u8; 4096];
    let body = loop {
        if let Some(body) = decoder.next_frame().unwrap() {
            break body;
        }
        let n = stream.read(&mut chunk).await.unwrap();
        assert_ne!(n, 0, "the server closed the connection");
        decoder.push(&chunk[..n]);
    };
    let (id, mut payload) = split_packet_id(&body).unwrap();
    assert_eq!(id, <StatusResponse as lodeframe::protocol::Packet>::ID);
    let response = <StatusResponse as lodeframe::protocol::Decode>::decode(&mut payload).unwrap();
    assert!(response.json.contains("a test server"), "{}", response.json);

    server.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn stopping_the_server_ends_the_connections_and_wait_returns() {
    let server = start("x").await;
    let mut bot = Bot::connect(server.addr(), "Steve").await.unwrap();

    server.stop();

    // the instance is gone, so the connection ends instead of staying quiet
    let ended = bot
        .recv_until(Duration::from_secs(10), |_| None::<()>)
        .await;
    assert!(ended.is_err());
    server.wait().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_tick_stats_follow_the_ticks() {
    let server = start("x").await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let before = server.tick_stats();

    tokio::time::sleep(Duration::from_millis(500)).await;
    let after = server.tick_stats();

    // 20 ticks a second: about 10 in half a second, with slack for a busy machine
    let ticks = after.ticks - before.ticks;
    assert!((5..=15).contains(&ticks), "{ticks} ticks in 500 ms");
    assert!(after.busy >= before.busy);
    server.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_server_calls_itself_lodeframe_unless_told_otherwise() {
    for (brand, expected) in [(None, "Lodeframe"), (Some("My Server"), "My Server")] {
        let mut server = Server::new("127.0.0.1:0");
        if let Some(brand) = brand {
            server = server.brand(brand);
        }
        let server = server
            .start(|registries: &Registries| {
                let mut world = World::new(registries, FlatGenerator::default());
                world.view_distance = 2;
                world
            })
            .await
            .unwrap();

        let bot = Bot::connect(server.addr(), "Steve").await.unwrap();

        assert_eq!(bot.server_brand(), Some(expected));
        server.stop();
    }
}
