// SPDX-License-Identifier: Apache-2.0 OR MIT
//! A started server answers the server list, lets bots in, and drops them when it stops.

use std::time::Duration;

use lodeframe::{
    chunk::FlatGenerator,
    protocol::{
        FrameDecoder, Identifier, VarInt, encode_frame, packet_body,
        packets::{
            handshake::Intention,
            play::{ClientboundCustomPayload, Disconnect},
            status::{StatusRequest, StatusResponse},
        },
        split_packet_id,
    },
    registry::Registries,
    server::{RunningServer, Server},
    world::{Ctx, PlayerJoinEvent, PluginMessageEvent, World},
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

/// Asks the server list of the server at `addr`, returning the JSON it answers with.
async fn server_list(addr: std::net::SocketAddr) -> String {
    let mut stream = TcpStream::connect(addr).await.unwrap();

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
    <StatusResponse as lodeframe::protocol::Decode>::decode(&mut payload)
        .unwrap()
        .json
}

#[tokio::test(flavor = "multi_thread")]
async fn the_server_list_shows_the_motd() {
    let server = start("a test server").await;

    let json = server_list(server.addr()).await;

    assert!(json.contains("a test server"), "{json}");
    server.stop();
}

/// Starts a world server with `configure` applied.
async fn start_with(configure: impl FnOnce(Server) -> Server) -> RunningServer {
    configure(Server::new("127.0.0.1:0"))
        .start(|registries: &Registries| {
            let mut world = World::new(registries, FlatGenerator::default());
            world.view_distance = 2;
            world
        })
        .await
        .unwrap()
}

/// Waits until `server` counts `n` players online.
async fn wait_for_online(server: &RunningServer, n: u32) {
    for _ in 0..200 {
        if server.online() == n {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("{} players online, not {n}", server.online());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_server_list_shows_who_is_online_and_the_most_there_can_be() {
    let server = start_with(|s| s.max_players(7)).await;
    assert!(
        server_list(server.addr())
            .await
            .contains(r#""players":{"max":7,"online":0}"#)
    );

    let _bot = Bot::connect(server.addr(), "Steve").await.unwrap();

    let json = server_list(server.addr()).await;
    assert!(json.contains(r#""players":{"max":7,"online":1}"#), "{json}");
    server.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_full_server_turns_the_next_player_away_until_someone_leaves() {
    let server = start_with(|s| s.max_players(1)).await;
    let first = Bot::connect(server.addr(), "Steve").await.unwrap();

    let refused = Bot::connect(server.addr(), "Alex").await;

    assert!(refused.is_err());
    assert_eq!(server.online(), 1);
    drop(first);
    wait_for_online(&server, 0).await;
    Bot::connect(server.addr(), "Alex").await.unwrap();
    server.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn settings_that_cannot_work_stop_the_server_from_starting() {
    let started = Server::new("127.0.0.1:0")
        .tick_rate(0)
        .start(|registries: &Registries| World::new(registries, FlatGenerator::default()))
        .await;

    assert_eq!(
        started.unwrap_err().kind(),
        std::io::ErrorKind::InvalidInput
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_tick_rate_and_the_compression_threshold_can_be_set() {
    let server = start_with(|s| s.tick_rate(40).compression_threshold(32).nodelay(true)).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let before = server.tick_stats();
    tokio::time::sleep(Duration::from_millis(500)).await;
    let ticks = server.tick_stats().ticks - before.ticks;

    // 40 a second: about 20 in half a second, with slack for a busy machine
    assert!((12..=28).contains(&ticks), "{ticks} ticks in 500 ms");
    // a player can still join with the other settings
    Bot::connect(server.addr(), "Steve").await.unwrap();
    server.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_player_who_never_answers_keep_alives_is_cut_off() {
    let server =
        start_with(|s| s.keep_alive(Duration::from_millis(50), Duration::from_millis(300))).await;
    // the bot answers keep alives only while it is reading, and here it does not read
    let _silent = Bot::connect(server.addr(), "Steve").await.unwrap();
    wait_for_online(&server, 1).await;

    wait_for_online(&server, 0).await;
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
async fn shutting_down_tells_the_players_why_before_it_returns() {
    let server = start_with(|s| s).await;
    let mut steve = Bot::connect(server.addr(), "Steve").await.unwrap();
    let mut alex = Bot::connect(server.addr(), "Alex").await.unwrap();
    wait_for_online(&server, 2).await;

    server.shutdown().await;

    // every connection has written what it was sent, so the players are gone
    assert_eq!(server.online(), 0);
    for bot in [&mut steve, &mut alex] {
        let mut told = false;
        let ended = bot
            .recv_until(Duration::from_secs(10), |frame| {
                told |= frame.is::<Disconnect>();
                None::<()>
            })
            .await;
        assert!(ended.is_err());
        assert!(told, "{} was not told why", bot.name());
    }
    server.wait().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn shutting_down_an_empty_server_returns_at_once() {
    let server = start_with(|s| s.shutdown_timeout(Duration::from_secs(60))).await;

    tokio::time::timeout(Duration::from_secs(10), server.shutdown())
        .await
        .expect("an empty server shuts down quickly");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_bot_and_the_server_trade_plugin_messages() {
    let server = Server::new("127.0.0.1:0")
        .start(|registries: &Registries| {
            let mut world = World::new(registries, FlatGenerator::default());
            world.view_distance = 2;
            world
                .events_mut()
                .on(|e: &mut PlayerJoinEvent, ctx: &mut Ctx| {
                    let brand = ctx.client_brand(e.player).unwrap_or("none").to_owned();
                    let channel = Identifier::new("test:brand").unwrap();
                    ctx.send_plugin_message(e.player, &channel, brand.as_bytes());
                })
                .on(|e: &mut PluginMessageEvent, ctx: &mut Ctx| {
                    if e.channel.as_str() == "test:ping" {
                        let channel = Identifier::new("test:pong").unwrap();
                        ctx.send_plugin_message(e.player, &channel, &e.data);
                    }
                });
            world
        })
        .await
        .unwrap();
    let mut bot = Bot::connect(server.addr(), "Steve").await.unwrap();

    // the brand the bot reported in configuration is known when it joins
    let limit = Duration::from_secs(10);
    let told = bot
        .recv_until(limit, |frame| {
            let message = frame.decode::<ClientboundCustomPayload>().ok()?;
            (frame.is::<ClientboundCustomPayload>() && message.channel.as_str() == "test:brand")
                .then_some(message.data)
        })
        .await
        .unwrap();
    assert_eq!(told, lodeframe_bot::BRAND.as_bytes());

    bot.plugin_message("test:ping", b"hello").await.unwrap();
    let echoed = bot
        .recv_until(limit, |frame| {
            let message = frame.decode::<ClientboundCustomPayload>().ok()?;
            (frame.is::<ClientboundCustomPayload>() && message.channel.as_str() == "test:pong")
                .then_some(message.data)
        })
        .await
        .unwrap();
    assert_eq!(echoed, b"hello");
    server.stop();
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
