// SPDX-License-Identifier: Apache-2.0 OR MIT
//! BungeeCord legacy forwarding and BungeeGuard, over an in-memory pipe: the client plays the
//! proxy, and the address it forwards is handed to the login the way the server hands it.

use std::{net::IpAddr, time::Duration};

use lodeframe::{
    login::{self, Profile},
    net::Connection,
    protocol::{
        Result, State, Uuid,
        packets::login::{Disconnect, Hello, LoginAcknowledged, LoginFinished, ProfileProperty},
    },
};
use tokio::{io::duplex, task::JoinHandle};

const T: Duration = Duration::from_secs(5);
const UUID: &str = "000000000000000000000000000000ab";
const TOKEN: &str = "0123456789abcdef";

type Pipe = Connection<tokio::io::DuplexStream>;

fn ip(s: &str) -> IpAddr {
    s.parse().unwrap()
}

fn skin_json() -> &'static str {
    r#"{"name":"textures","value":"dGV4dHVyZXM=","signature":"c2lnbmVk"}"#
}

fn address(ip: &str, properties: &str) -> String {
    format!("localhost\0{ip}\0{UUID}\0[{properties}]")
}

/// Which login to run, and with what to believe the proxy.
#[derive(Clone)]
enum Mode {
    Peer(IpAddr),
    Tokens(Vec<String>),
}

fn start(
    address: String,
    mode: Mode,
    threshold: Option<usize>,
) -> (JoinHandle<Result<Profile>>, Pipe) {
    let (a, b) = duplex(1 << 16);
    let mut server = Connection::new(a, T);
    let mut client = Connection::new(b, T);
    server.set_state(State::Login);
    client.set_state(State::Login);
    let task = tokio::spawn(async move {
        let trusted = [ip("10.0.0.1"), ip("127.0.0.1")];
        let profile = match &mode {
            Mode::Peer(peer) => {
                login::bungeecord(&mut server, threshold, &address, *peer, &trusted).await
            }
            Mode::Tokens(tokens) => {
                login::bungeeguard(&mut server, threshold, &address, tokens).await
            }
        };
        assert_eq!(server.state() == State::Configuration, profile.is_ok());
        profile
    });
    (task, client)
}

async fn hello(client: &mut Pipe) {
    client
        .write_packet(&Hello {
            name: "Alex".into(),
            uuid: Uuid(0),
        })
        .await
        .unwrap();
}

/// Says hello and finishes the login the way a client does, returning what the server sent.
async fn join(client: &mut Pipe, compressed: bool) -> LoginFinished {
    hello(client).await;
    if compressed {
        let c: lodeframe::protocol::packets::login::LoginCompression =
            client.read_packet().await.unwrap();
        client.set_compression(Some(c.threshold.0 as usize));
    }
    let finished: LoginFinished = client.read_packet().await.unwrap();
    client.write_packet(&LoginAcknowledged).await.unwrap();
    finished
}

/// The reason of the `Disconnect` the server sends instead of the profile.
async fn refusal(client: &mut Pipe, says_hello: bool) -> String {
    if says_hello {
        hello(client).await;
    }
    client.read_packet::<Disconnect>().await.unwrap().reason
}

#[tokio::test]
async fn a_trusted_proxy_forwards_the_player() {
    let (task, mut client) = start(
        address("198.51.100.9", skin_json()),
        Mode::Peer(ip("10.0.0.1")),
        None,
    );

    let finished = join(&mut client, false).await;

    let profile = task.await.unwrap().unwrap();
    assert_eq!(profile.uuid, Uuid(0xab));
    assert_eq!(profile.name, "Alex");
    assert_eq!(profile.remote_addr, Some(ip("198.51.100.9")));
    assert_eq!(
        profile.properties,
        [ProfileProperty {
            name: "textures".into(),
            value: "dGV4dHVyZXM=".into(),
            signature: Some("c2lnbmVk".into()),
        }]
    );
    assert_eq!(finished.uuid, Uuid(0xab));
    assert_eq!(finished.properties, profile.properties);
}

#[tokio::test]
async fn a_forwarding_without_properties_is_accepted() {
    let (task, mut client) = start(
        format!("localhost\x00203.0.113.5\x00{UUID}"),
        Mode::Peer(ip("127.0.0.1")),
        None,
    );

    join(&mut client, false).await;

    let profile = task.await.unwrap().unwrap();
    assert!(profile.properties.is_empty());
    assert_eq!(profile.remote_addr, Some(ip("203.0.113.5")));
}

#[tokio::test]
async fn compression_is_turned_on_before_the_profile() {
    let (task, mut client) = start(
        address("203.0.113.5", ""),
        Mode::Peer(ip("127.0.0.1")),
        Some(256),
    );

    join(&mut client, true).await;

    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn a_scope_id_on_the_address_is_dropped() {
    let (task, mut client) = start(
        address("fe80::1%eth0", ""),
        Mode::Peer(ip("127.0.0.1")),
        None,
    );

    join(&mut client, false).await;

    assert_eq!(
        task.await.unwrap().unwrap().remote_addr,
        Some(ip("fe80::1"))
    );
}

#[tokio::test]
async fn an_ipv4_peer_on_a_dual_stack_socket_is_still_trusted() {
    let (task, mut client) = start(
        address("203.0.113.5", ""),
        Mode::Peer(ip("::ffff:127.0.0.1")),
        None,
    );

    join(&mut client, false).await;

    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn a_peer_that_is_not_trusted_is_told_so_before_it_says_anything() {
    // the client never says hello: the refusal comes without reading it
    let (task, mut client) = start(
        address("10.0.0.1", ""),
        Mode::Peer(ip("198.51.100.1")),
        None,
    );

    let reason = refusal(&mut client, false).await;

    assert!(reason.contains("connect with BungeeCord"), "{reason}");
    assert!(task.await.unwrap().is_err());
}

#[tokio::test]
async fn an_address_written_wrong_is_refused_with_a_reason() {
    let wrong = [
        "localhost".to_owned(),
        format!("localhost\x00127.0.0.1\0{UUID}\0[]\0more"),
        format!("localhost\x00127.0.0.1\0{UUID}\0[]\0more\0and more"),
        format!("localhost\0nowhere\0{UUID}\0[]"),
        "localhost\x00127.0.0.1\0abcd\0[]".to_owned(),
        format!("localhost\x00127.0.0.1\0-{}\0[]", &UUID[1..]),
        format!("localhost\x00127.0.0.1\0{UUID}\0"),
        format!("localhost\x00127.0.0.1\0{UUID}\0{{}}"),
        format!("localhost\x00127.0.0.1\0{UUID}\0[1]"),
        format!("localhost\x00127.0.0.1\0{UUID}\0[{{\"name\":\"textures\"}}]"),
        format!("localhost\x00127.0.0.1\0{UUID}\0[{{\"name\":1,\"value\":\"v\"}}]"),
        address(
            "127.0.0.1",
            &format!(r#"{{"name":"n","value":"{}"}}"#, "v".repeat(32768)),
        ),
        address(
            "127.0.0.1",
            &vec![r#"{"name":"n","value":"v"}"#; 65].join(","),
        ),
    ];
    for wrong in wrong {
        let (task, mut client) = start(wrong.clone(), Mode::Peer(ip("127.0.0.1")), None);

        let reason = refusal(&mut client, true).await;

        assert!(reason.contains("Unable to verify"), "{wrong:?}: {reason}");
        assert!(task.await.unwrap().is_err());
    }
}

#[tokio::test]
async fn a_signature_that_is_empty_means_there_is_none() {
    let (task, mut client) = start(
        address("127.0.0.1", r#"{"name":"n","value":"v","signature":""}"#),
        Mode::Peer(ip("127.0.0.1")),
        None,
    );

    join(&mut client, false).await;

    assert_eq!(task.await.unwrap().unwrap().properties[0].signature, None);
}

fn guarded(token: Option<&str>) -> String {
    let token = token.map_or(String::new(), |t| {
        format!(r#",{{"name":"bungeeguard-token","value":"{t}","signature":""}}"#)
    });
    address("198.51.100.9", &format!("{}{token}", skin_json()))
}

#[tokio::test]
async fn bungeeguard_accepts_any_of_its_tokens_and_keeps_the_token_out_of_the_profile() {
    for tokens in [vec![TOKEN], vec!["another token", TOKEN]] {
        let tokens = tokens.into_iter().map(String::from).collect();
        let (task, mut client) = start(guarded(Some(TOKEN)), Mode::Tokens(tokens), None);

        let finished = join(&mut client, false).await;

        let profile = task.await.unwrap().unwrap();
        assert_eq!(profile.properties.len(), 1);
        assert_eq!(profile.properties[0].name, "textures");
        // nor does the client of the server get it back
        assert_eq!(finished.properties, profile.properties);
    }
}

#[tokio::test]
async fn bungeeguard_refuses_a_token_that_is_missing_or_wrong_the_same_way() {
    for given in [
        None,
        Some("fedcba9876543210"),
        Some("0123456789abcde"),
        Some("0123456789abcdef0"),
        Some(""),
    ] {
        let (task, mut client) = start(guarded(given), Mode::Tokens(vec![TOKEN.into()]), None);

        let reason = refusal(&mut client, true).await;

        assert!(reason.contains("Unable to verify"), "{given:?}: {reason}");
        assert!(task.await.unwrap().is_err());
    }
}

#[tokio::test]
async fn bungeeguard_does_not_look_at_where_the_connection_came_from() {
    // no peer is given at all: only the token counts
    let (task, mut client) = start(guarded(Some(TOKEN)), Mode::Tokens(vec![TOKEN.into()]), None);

    join(&mut client, false).await;

    task.await.unwrap().unwrap();
}
