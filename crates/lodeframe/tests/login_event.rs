// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Login hooks on a started server: who they can turn away, who they can turn a player into, and
//! that a player they refuse never reaches the world.

use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use lodeframe::{
    chunk::FlatGenerator,
    login::{LoginAttempt, LoginDecision},
    net::Connection,
    protocol::{
        Decode, Identifier, Packet, Uuid, VarInt,
        packets::{
            handshake::Intention,
            login::{
                Disconnect as LoginDisconnect, Hello, LoginCompression, LoginFinished,
                ProfileProperty,
            },
            play::ClientboundCustomPayload,
            status::{StatusRequest, StatusResponse},
        },
        split_packet_id,
    },
    registry::Registries,
    server::{Forwarding, RunningServer, Server},
    text::{Color, Component},
    world::{Ctx, PlayerJoinEvent, World},
};
use lodeframe_bot::{Bot, Bungee};
use tokio::net::TcpStream;

const T: Duration = Duration::from_secs(5);

fn skin() -> ProfileProperty {
    ProfileProperty {
        name: "textures".into(),
        value: "dGV4dHVyZXM=".into(),
        signature: Some("c2lnbmVk".into()),
    }
}

/// A server with `configure`d hooks, whose world counts the joins in `joins` and tells every
/// player who joins the name, UUID and number of properties it knows them by, on `test:who`.
async fn start(joins: Arc<AtomicUsize>, configure: impl FnOnce(Server) -> Server) -> RunningServer {
    configure(Server::new("127.0.0.1:0"))
        .start(move |registries: &Registries| {
            let mut world = World::new(registries, FlatGenerator::default());
            world.view_distance = 2;
            world
                .events_mut()
                .on(move |e: &mut PlayerJoinEvent, ctx: &mut Ctx| {
                    joins.fetch_add(1, Ordering::SeqCst);
                    let who = format!(
                        "{} {:x} {}",
                        ctx.name(e.player).unwrap_or("?"),
                        e.player.uuid().0,
                        ctx.profile_properties(e.player).len()
                    );
                    let channel = Identifier::new("test:who").unwrap();
                    ctx.send_plugin_message(e.player, &channel, who.as_bytes());
                });
            world
        })
        .await
        .unwrap()
}

async fn who(bot: &mut Bot<TcpStream>) -> String {
    let told = bot
        .recv_until(T, |frame| {
            let message = frame.decode::<ClientboundCustomPayload>().ok()?;
            (frame.is::<ClientboundCustomPayload>() && message.channel.as_str() == "test:who")
                .then_some(message.data)
        })
        .await
        .unwrap();
    String::from_utf8(told).unwrap()
}

async fn wait_for_online(server: &RunningServer, n: u32) {
    for _ in 0..200 {
        if server.online() == n {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("{} players online, not {n}", server.online());
}

/// Logs in as `name` by hand, returning the reason the server turned it away with, or `None`
/// when it was let in.
async fn refusal(server: &RunningServer, name: &str) -> Option<String> {
    let stream = TcpStream::connect(server.addr()).await.unwrap();
    let mut conn = Connection::new(stream, T);
    conn.write_packet(&Intention {
        protocol_version: VarInt(lodeframe::protocol::PROTOCOL_VERSION),
        server_address: "localhost".into(),
        server_port: 25565,
        next_state: VarInt(2),
    })
    .await
    .unwrap();
    conn.write_packet(&Hello {
        name: name.into(),
        uuid: Uuid(0),
    })
    .await
    .unwrap();
    loop {
        let body = conn.read_frame().await.unwrap();
        let (id, mut payload) = split_packet_id(&body).unwrap();
        if id == LoginCompression::ID {
            let c = LoginCompression::decode(&mut payload).unwrap();
            conn.set_compression(Some(c.threshold.0 as usize));
        } else if id == LoginDisconnect::ID {
            return Some(LoginDisconnect::decode(&mut payload).unwrap().reason);
        } else if id == LoginFinished::ID {
            return None;
        } else {
            panic!("unexpected packet {id}");
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn without_a_hook_players_join_as_they_are() {
    let joins = Arc::new(AtomicUsize::new(0));
    let server = start(joins.clone(), |s| s).await;

    let mut steve = Bot::connect(server.addr(), "Steve").await.unwrap();

    assert_eq!(
        who(&mut steve).await,
        format!("Steve {:x} 0", Uuid::offline("Steve").0)
    );
    server.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_player_a_hook_denies_never_reaches_the_world() {
    let joins = Arc::new(AtomicUsize::new(0));
    let server = start(joins.clone(), |s| {
        s.on_login(|attempt: LoginAttempt| async move {
            if attempt.profile.name.starts_with("Bad") {
                attempt.deny("go away")
            } else {
                attempt.allow()
            }
        })
        // the server is full for one: a refusal must not take that place
        .max_players(1)
    })
    .await;

    assert!(Bot::connect(server.addr(), "BadGuy").await.is_err());
    assert!(Bot::connect(server.addr(), "BadGirl").await.is_err());
    assert_eq!(server.online(), 0);
    assert_eq!(joins.load(Ordering::SeqCst), 0);

    let mut good = Bot::connect(server.addr(), "Good").await.unwrap();
    who(&mut good).await;
    assert_eq!(joins.load(Ordering::SeqCst), 1);
    assert_eq!(server.online(), 1);
    server.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_reason_arrives_as_the_json_of_the_component() {
    let joins = Arc::new(AtomicUsize::new(0));
    let server = start(joins, |s| {
        s.on_login(|attempt: LoginAttempt| async move {
            attempt.deny(
                Component::text("You are \"banned\"")
                    .color(Color::Red)
                    .bold(),
            )
        })
    })
    .await;

    let reason = refusal(&server, "Steve").await.unwrap();

    assert_eq!(
        reason,
        r#"{"text":"You are \"banned\"","color":"red","bold":true}"#
    );
    server.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hook_can_let_a_player_in_as_someone_else() {
    let joins = Arc::new(AtomicUsize::new(0));
    let server = start(joins, |s| {
        s.on_login(|mut attempt: LoginAttempt| async move {
            // as a lookup that took its time would
            tokio::time::sleep(Duration::from_millis(20)).await;
            attempt.profile.name = "Renamed".into();
            attempt.profile.uuid = Uuid(0x77);
            attempt.profile.properties = vec![skin()];
            attempt.allow()
        })
    })
    .await;

    let mut steve = Bot::connect(server.addr(), "Steve").await.unwrap();

    assert_eq!(who(&mut steve).await, "Renamed 77 1");
    server.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn hooks_run_in_order_and_each_sees_the_change_before_it() {
    let joins = Arc::new(AtomicUsize::new(0));
    let server = start(joins, |s| {
        s.on_login(|mut a: LoginAttempt| async move {
            a.profile.name.push_str("_1");
            a.allow()
        })
        .on_login(|mut a: LoginAttempt| async move {
            assert_eq!(a.profile.name, "Steve_1");
            a.profile.name.push_str("_2");
            a.allow()
        })
    })
    .await;

    let mut steve = Bot::connect(server.addr(), "Steve").await.unwrap();

    assert!(who(&mut steve).await.starts_with("Steve_1_2 "));
    server.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_first_hook_to_deny_ends_the_login() {
    let joins = Arc::new(AtomicUsize::new(0));
    let asked_last = Arc::new(AtomicUsize::new(0));
    let counter = asked_last.clone();
    let server = start(joins, |s| {
        s.on_login(|a: LoginAttempt| async move { a.deny("first") })
            .on_login(move |a: LoginAttempt| {
                counter.fetch_add(1, Ordering::SeqCst);
                async move { a.allow() }
            })
    })
    .await;

    assert_eq!(
        refusal(&server, "Steve").await.unwrap(),
        r#"{"text":"first"}"#
    );
    assert_eq!(asked_last.load(Ordering::SeqCst), 0);
    server.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hook_that_waits_holds_up_nobody_else() {
    let joins = Arc::new(AtomicUsize::new(0));
    let server = start(joins, |s| {
        s.on_login(|a: LoginAttempt| async move {
            if a.profile.name == "Slow" {
                tokio::time::sleep(Duration::from_millis(800)).await;
            }
            a.allow()
        })
    })
    .await;
    let addr = server.addr();
    let slow = tokio::spawn(async move { Bot::connect(addr, "Slow").await });
    tokio::time::sleep(Duration::from_millis(100)).await;

    let mut fast = Bot::connect(server.addr(), "Fast").await.unwrap();
    who(&mut fast).await;

    assert!(!slow.is_finished());
    slow.await.unwrap().unwrap();
    server.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hook_that_takes_too_long_turns_the_player_away() {
    let joins = Arc::new(AtomicUsize::new(0));
    let server = start(joins.clone(), |s| {
        s.on_login(|_: LoginAttempt| std::future::pending::<LoginDecision>())
            .login_hook_timeout(Duration::from_millis(200))
    })
    .await;

    let reason = refusal(&server, "Steve").await.unwrap();

    assert!(reason.contains("timed out"), "{reason}");
    wait_for_online(&server, 0).await;
    assert_eq!(joins.load(Ordering::SeqCst), 0);
    server.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_profile_that_cannot_be_sent_is_turned_away_not_sent() {
    let joins = Arc::new(AtomicUsize::new(0));
    let server = start(joins.clone(), |s| {
        s.on_login(|mut a: LoginAttempt| async move {
            match a.profile.name.as_str() {
                "Empty" => a.profile.name.clear(),
                "Long" => a.profile.name = "x".repeat(17),
                "Many" => a.profile.properties = vec![skin(); 65],
                _ => {
                    a.profile.properties = vec![ProfileProperty {
                        value: "v".repeat(32768),
                        ..skin()
                    }];
                }
            }
            a.allow()
        })
    })
    .await;

    for name in ["Empty", "Long", "Many", "Big"] {
        let reason = refusal(&server, name).await.unwrap();

        assert!(reason.contains("Unable to verify"), "{name}: {reason}");
    }
    assert_eq!(joins.load(Ordering::SeqCst), 0);
    server.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_hook_sees_who_a_proxy_says_the_player_is() {
    let joins = Arc::new(AtomicUsize::new(0));
    let server = start(joins, |s| {
        s.forwarding(Forwarding::BungeeCord {
            trusted: vec!["127.0.0.1".parse().unwrap()],
        })
        .on_login(|a: LoginAttempt| async move {
            let forwarded = a.profile.uuid == Uuid(9)
                && a.profile.remote_addr == Some("203.0.113.9".parse().unwrap())
                && a.peer.ip().is_loopback();
            if forwarded {
                a.allow()
            } else {
                a.deny("not what the proxy said")
            }
        })
    })
    .await;
    let mut proxy = Bungee::new(Uuid(9));
    proxy.address = "203.0.113.9".into();

    let mut steve = Bot::connect_bungee(server.addr(), "Steve", proxy)
        .await
        .unwrap();

    assert!(who(&mut steve).await.starts_with("Steve 9 "));
    server.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_server_list_does_not_ask_the_hooks() {
    let joins = Arc::new(AtomicUsize::new(0));
    let asked = Arc::new(AtomicUsize::new(0));
    let counter = asked.clone();
    let server = start(joins, |s| {
        s.on_login(move |a: LoginAttempt| {
            counter.fetch_add(1, Ordering::SeqCst);
            async move { a.allow() }
        })
    })
    .await;
    let stream = TcpStream::connect(server.addr()).await.unwrap();
    let mut conn = Connection::new(stream, T);
    conn.write_packet(&Intention {
        protocol_version: VarInt(0),
        server_address: "localhost".into(),
        server_port: 25565,
        next_state: VarInt(1),
    })
    .await
    .unwrap();
    conn.write_packet(&StatusRequest).await.unwrap();

    let _: StatusResponse = conn.read_packet().await.unwrap();

    assert_eq!(asked.load(Ordering::SeqCst), 0);
    server.stop();
}
