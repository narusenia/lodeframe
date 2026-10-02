// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Velocity modern forwarding, over an in-memory pipe: the client plays the proxy.

use std::{net::IpAddr, time::Duration};

use hmac::{Hmac, Mac};
use lodeframe::{
    login::{self, Profile, VELOCITY_CHANNEL},
    net::Connection,
    protocol::{
        Encode, Error, State, Uuid, VarInt,
        packets::login::{
            CustomQuery, CustomQueryAnswer, Disconnect, Hello, LoginAcknowledged, LoginFinished,
            ProfileProperty,
        },
    },
};
use sha2::Sha256;
use tokio::{io::duplex, task::JoinHandle};

const T: Duration = Duration::from_secs(5);
const SECRET: &[u8] = b"a shared secret";

type Pipe = Connection<tokio::io::DuplexStream>;

fn skin() -> ProfileProperty {
    ProfileProperty {
        name: "textures".into(),
        value: "dGV4dHVyZXM=".into(),
        signature: Some("c2lnbmVk".into()),
    }
}

/// What a proxy signs: version, address, UUID, name, properties.
fn body(version: i32, address: &str, name: &str, properties: &[ProfileProperty]) -> Vec<u8> {
    let mut body = Vec::new();
    VarInt(version).encode(&mut body).unwrap();
    address.encode(&mut body).unwrap();
    Uuid(0x1234).encode(&mut body).unwrap();
    name.encode(&mut body).unwrap();
    properties.to_vec().encode(&mut body).unwrap();
    body
}

fn signed(secret: &[u8], body: &[u8]) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).unwrap();
    mac.update(body);
    let mut answer = mac.finalize().into_bytes().to_vec();
    answer.extend_from_slice(body);
    answer
}

fn good_body() -> Vec<u8> {
    body(1, "203.0.113.7", "Alex", &[skin()])
}

/// Starts a server login with `limit` and the client that has said hello.
fn start(limit: Duration) -> (JoinHandle<lodeframe::protocol::Result<Profile>>, Pipe) {
    let (a, b) = duplex(1 << 16);
    let mut server = Connection::new(a, T);
    let mut client = Connection::new(b, T);
    server.set_state(State::Login);
    client.set_state(State::Login);
    let task = tokio::spawn(async move {
        let profile = login::velocity(&mut server, None, SECRET, limit).await;
        assert_eq!(server.state() == State::Configuration, profile.is_ok());
        profile
    });
    (task, client)
}

async fn hello(client: &mut Pipe) -> CustomQuery {
    client
        .write_packet(&Hello {
            name: "ProxyName".into(),
            uuid: Uuid(0),
        })
        .await
        .unwrap();
    client.read_packet().await.unwrap()
}

/// Answers the query with `answer` and returns what the server did.
async fn answer_with(answer: Option<Vec<u8>>) -> (lodeframe::protocol::Result<Profile>, Pipe) {
    let (task, mut client) = start(T);
    let query = hello(&mut client).await;
    client
        .write_packet(&CustomQueryAnswer {
            transaction_id: query.transaction_id,
            data: answer,
        })
        .await
        .unwrap();
    let result = task.await.unwrap();
    (result, client)
}

/// The server refused: the client is told `reason` and nothing else.
async fn assert_refused(mut client: Pipe, reason: &str) {
    let told: Disconnect = client.read_packet().await.unwrap();
    assert_eq!(told.reason, format!("{{\"text\":\"{reason}\"}}"));
    assert!(client.read_packet::<LoginFinished>().await.is_err());
}

#[tokio::test]
async fn the_proxys_answer_is_the_player() {
    let (task, mut client) = start(T);
    let query = hello(&mut client).await;
    assert_eq!(query.channel.as_str(), VELOCITY_CHANNEL);
    assert_eq!(query.data, [1]);
    client
        .write_packet(&CustomQueryAnswer {
            transaction_id: query.transaction_id,
            data: Some(signed(SECRET, &good_body())),
        })
        .await
        .unwrap();

    let finished: LoginFinished = client.read_packet().await.unwrap();
    assert_eq!(finished.uuid, Uuid(0x1234));
    assert_eq!(finished.name, "Alex");
    assert_eq!(finished.properties, [skin()]);
    client.write_packet(&LoginAcknowledged).await.unwrap();

    let profile = task.await.unwrap().unwrap();
    assert_eq!(profile.uuid, Uuid(0x1234));
    assert_eq!(profile.name, "Alex");
    assert_eq!(profile.properties, [skin()]);
    assert_eq!(
        profile.remote_addr,
        Some("203.0.113.7".parse::<IpAddr>().unwrap())
    );
}

#[tokio::test]
async fn an_ipv6_address_is_read() {
    let data = signed(SECRET, &body(1, "2001:db8::1", "Alex", &[]));
    let (task, mut client) = start(T);
    let query = hello(&mut client).await;
    client
        .write_packet(&CustomQueryAnswer {
            transaction_id: query.transaction_id,
            data: Some(data),
        })
        .await
        .unwrap();
    client.read_packet::<LoginFinished>().await.unwrap();
    client.write_packet(&LoginAcknowledged).await.unwrap();
    let profile = task.await.unwrap().unwrap();
    assert_eq!(profile.remote_addr, Some("2001:db8::1".parse().unwrap()));
}

#[tokio::test]
async fn another_secret_is_refused() {
    let (result, client) = answer_with(Some(signed(b"not the secret", &good_body()))).await;
    assert!(matches!(result, Err(Error::InvalidValue(_))));
    assert_refused(client, "Unable to verify player details").await;
}

#[tokio::test]
async fn a_changed_body_is_refused() {
    let mut data = signed(SECRET, &good_body());
    // the last byte of the data, one bit
    *data.last_mut().unwrap() ^= 1;
    let (result, client) = answer_with(Some(data)).await;
    assert!(result.is_err());
    assert_refused(client, "Unable to verify player details").await;
}

#[tokio::test]
async fn a_changed_signature_is_refused() {
    let mut data = signed(SECRET, &good_body());
    data[0] ^= 1;
    let (result, client) = answer_with(Some(data)).await;
    assert!(result.is_err());
    assert_refused(client, "Unable to verify player details").await;
}

#[tokio::test]
async fn an_answer_too_short_to_hold_a_signature_is_refused() {
    let (result, client) = answer_with(Some(vec![0; 31])).await;
    assert!(result.is_err());
    assert_refused(client, "Unable to verify player details").await;
}

#[tokio::test]
async fn signed_data_that_is_not_what_was_asked_is_refused() {
    let wrong_version = body(2, "203.0.113.7", "Alex", &[]);
    let bad_address = body(1, "not an address", "Alex", &[]);
    let empty_name = body(1, "203.0.113.7", "", &[]);
    let long_name = body(1, "203.0.113.7", "SeventeenLetters!", &[]);
    let many = body(1, "203.0.113.7", "Alex", &vec![skin(); 65]);
    let mut trailing = good_body();
    trailing.push(0);
    let truncated = good_body()[..10].to_vec();
    for data in [
        wrong_version,
        bad_address,
        empty_name,
        long_name,
        many,
        trailing,
        truncated,
    ] {
        let (result, client) = answer_with(Some(signed(SECRET, &data))).await;
        assert!(result.is_err());
        assert_refused(client, "Unable to verify player details").await;
    }
}

#[tokio::test]
async fn a_client_that_does_not_know_the_channel_is_sent_to_the_proxy() {
    let (result, client) = answer_with(None).await;
    assert!(result.is_err());
    assert_refused(client, "This server requires you to connect with Velocity.").await;
}

#[tokio::test]
async fn a_client_that_does_not_answer_is_turned_away() {
    let (task, mut client) = start(Duration::from_millis(50));
    hello(&mut client).await;
    let result = task.await.unwrap();
    assert!(matches!(result, Err(Error::InvalidValue(_))));
    assert_refused(client, "This server requires you to connect with Velocity.").await;
}

#[tokio::test]
async fn compression_is_on_before_the_query() {
    let (a, b) = duplex(1 << 16);
    let mut server = Connection::new(a, T);
    let mut client = Connection::new(b, T);
    server.set_state(State::Login);
    client.set_state(State::Login);
    let task = tokio::spawn(async move { login::velocity(&mut server, Some(64), SECRET, T).await });
    client
        .write_packet(&Hello {
            name: "ProxyName".into(),
            uuid: Uuid(0),
        })
        .await
        .unwrap();
    let compression: lodeframe::protocol::packets::login::LoginCompression =
        client.read_packet().await.unwrap();
    assert_eq!(compression.threshold.0, 64);
    client.set_compression(Some(64));
    let query: CustomQuery = client.read_packet().await.unwrap();
    // long enough to be compressed
    let properties = vec![
        ProfileProperty {
            name: "textures".into(),
            value: "x".repeat(2000),
            signature: None,
        };
        2
    ];
    client
        .write_packet(&CustomQueryAnswer {
            transaction_id: query.transaction_id,
            data: Some(signed(SECRET, &body(1, "203.0.113.7", "Alex", &properties))),
        })
        .await
        .unwrap();
    client.read_packet::<LoginFinished>().await.unwrap();
    client.write_packet(&LoginAcknowledged).await.unwrap();
    assert_eq!(task.await.unwrap().unwrap().properties, properties);
}
