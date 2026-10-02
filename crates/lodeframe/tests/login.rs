// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Offline login and configuration, driven by a scripted client over an in-memory pipe.

use std::time::Duration;

use lodeframe::{
    configuration, login,
    net::Connection,
    protocol::{
        Decode, Nbt, State, Uuid, VERSION_NAME, ids,
        packets::{
            configuration::*,
            login::{Hello, LoginAcknowledged, LoginCompression, LoginFinished},
        },
        split_packet_id,
    },
    registry::Registries,
};
use tokio::io::duplex;

const T: Duration = Duration::from_secs(5);

async fn run(threshold: Option<usize>) {
    let (a, b) = duplex(1 << 20);
    let mut server = Connection::new(a, T);
    let mut client = Connection::new(b, T);
    server.set_state(State::Login);
    client.set_state(State::Login);

    let mut registries = Registries::vanilla();
    registries
        .set("minecraft:dimension_type", "demo:sky", Nbt::from("x"))
        .unwrap();
    let expected_registries = registries.clone();

    let server_task = tokio::spawn(async move {
        let profile = login::offline(&mut server, threshold).await.unwrap();
        assert_eq!(server.state(), State::Configuration);
        configuration::run(&mut server, &registries, "Lodeframe")
            .await
            .unwrap();
        assert_eq!(server.state(), State::Play);
        profile
    });

    // login
    client
        .write_packet(&Hello {
            name: "Notch".into(),
            uuid: Uuid(0),
        })
        .await
        .unwrap();
    if let Some(t) = threshold {
        let c: LoginCompression = client.read_packet().await.unwrap();
        assert_eq!(c.threshold.0 as usize, t);
        client.set_compression(Some(t));
    }
    let done: LoginFinished = client.read_packet().await.unwrap();
    assert_eq!(
        (done.uuid, done.name.as_str()),
        (Uuid::offline("Notch"), "Notch")
    );
    client.write_packet(&LoginAcknowledged).await.unwrap();

    // configuration: the server says what it is called, then what it offers
    let brand: ClientboundCustomPayload = client.read_packet().await.unwrap();
    assert_eq!(brand, ClientboundCustomPayload::brand("Lodeframe").unwrap());
    let _features: UpdateEnabledFeatures = client.read_packet().await.unwrap();
    let offer: ClientboundKnownPacks = client.read_packet().await.unwrap();
    assert_eq!(offer.packs[0].version, VERSION_NAME);
    client
        .write_frame(&[ids::configuration::serverbound::CLIENT_INFORMATION as u8, 0])
        .await
        .unwrap();
    client
        .write_packet(&ServerboundKnownPacks { packs: offer.packs })
        .await
        .unwrap();

    let mut registry_packets = Vec::new();
    loop {
        let body = client.read_frame().await.unwrap();
        let (id, mut payload) = split_packet_id(&body).unwrap();
        match id {
            ids::configuration::clientbound::REGISTRY_DATA => {
                registry_packets.push(RegistryData::decode(&mut payload).unwrap());
            }
            ids::configuration::clientbound::UPDATE_TAGS => {}
            ids::configuration::clientbound::FINISH_CONFIGURATION => break,
            other => panic!("unexpected packet {other}"),
        }
    }
    client.write_packet(&AckFinishConfiguration).await.unwrap();

    assert_eq!(server_task.await.unwrap().name, "Notch");
    let expected: Vec<_> = expected_registries.packets().collect();
    assert_eq!(registry_packets, expected);
    // only the user's entry carries data
    let with_data = registry_packets
        .iter()
        .flat_map(|r| &r.entries)
        .filter(|e| e.data.is_some())
        .count();
    assert_eq!(with_data, 1);
}

#[tokio::test]
async fn login_and_configuration_reach_play_without_compression() {
    run(None).await;
}

#[tokio::test]
async fn login_and_configuration_reach_play_with_compression() {
    run(Some(64)).await;
}

#[tokio::test]
async fn a_client_without_the_core_pack_is_refused() {
    let (a, b) = duplex(1 << 16);
    let mut server = Connection::new(a, T);
    let mut client = Connection::new(b, T);
    server.set_state(State::Configuration);
    let task =
        tokio::spawn(
            async move { configuration::run(&mut server, &Registries::vanilla(), "x").await },
        );
    let _: ClientboundCustomPayload = client.read_packet().await.unwrap();
    let _: UpdateEnabledFeatures = client.read_packet().await.unwrap();
    let _: ClientboundKnownPacks = client.read_packet().await.unwrap();
    client
        .write_packet(&ServerboundKnownPacks { packs: Vec::new() })
        .await
        .unwrap();
    assert!(task.await.unwrap().is_err());
}

#[tokio::test]
async fn a_client_that_never_answers_the_known_packs_times_out() {
    let (a, b) = duplex(1 << 16);
    let mut server = Connection::new(a, T);
    let mut client = Connection::new(b, T);
    server.set_state(State::Configuration);
    let task = tokio::spawn(async move {
        configuration::run_with(
            &mut server,
            &Registries::vanilla(),
            "x",
            Duration::from_millis(100),
        )
        .await
    });
    let _: ClientboundCustomPayload = client.read_packet().await.unwrap();
    let _: UpdateEnabledFeatures = client.read_packet().await.unwrap();
    let _: ClientboundKnownPacks = client.read_packet().await.unwrap();

    // the client says nothing, so the server gives up long before the connection's own timeout
    let started = std::time::Instant::now();
    let result = tokio::time::timeout(T, task).await.unwrap().unwrap();

    assert!(result.is_err());
    assert!(started.elapsed() < Duration::from_secs(2));
}

/// Runs configuration against a client that sends `messages` before it answers the known packs.
/// Returns what the server took, or the error that ended it.
async fn configure_sending(
    messages: Vec<ServerboundCustomPayload>,
) -> lodeframe::protocol::Result<Vec<lodeframe::instance::PluginMessage>> {
    let (a, b) = duplex(1 << 22);
    let mut server = Connection::new(a, T);
    let mut client = Connection::new(b, T);
    server.set_state(State::Configuration);
    let task =
        tokio::spawn(
            async move { configuration::run(&mut server, &Registries::vanilla(), "x").await },
        );
    let _: ClientboundCustomPayload = client.read_packet().await.unwrap();
    let _: UpdateEnabledFeatures = client.read_packet().await.unwrap();
    let offer: ClientboundKnownPacks = client.read_packet().await.unwrap();
    for message in &messages {
        client.write_packet(message).await.unwrap();
    }
    client
        .write_packet(&ServerboundKnownPacks { packs: offer.packs })
        .await
        .unwrap();
    // the server ends here if it refused the messages, otherwise it finishes the configuration
    loop {
        let Ok(body) = client.read_frame().await else {
            break;
        };
        let (id, _) = split_packet_id(&body).unwrap();
        if id == ids::configuration::clientbound::FINISH_CONFIGURATION {
            client.write_packet(&AckFinishConfiguration).await.unwrap();
            break;
        }
    }
    task.await.unwrap()
}

fn payload(channel: &str, data: Vec<u8>) -> ServerboundCustomPayload {
    ServerboundCustomPayload {
        channel: lodeframe::protocol::Identifier::new(channel).unwrap(),
        data,
    }
}

#[tokio::test]
async fn plugin_messages_sent_in_configuration_are_handed_on_in_order() {
    let taken = configure_sending(vec![
        payload("minecraft:brand", vec![1, b'v']),
        payload("mod:hello", vec![9, 9]),
    ])
    .await
    .unwrap();

    let got: Vec<_> = taken
        .iter()
        .map(|m| (m.channel.as_str(), m.data.clone()))
        .collect();
    assert_eq!(
        got,
        [
            ("minecraft:brand", vec![1, b'v']),
            ("mod:hello", vec![9, 9])
        ]
    );
}

#[tokio::test]
async fn a_client_that_sends_too_many_plugin_messages_in_configuration_is_cut() {
    let many = (0..=configuration::MAX_PLUGIN_MESSAGES)
        .map(|_| payload("mod:spam", Vec::new()))
        .collect();

    assert!(configure_sending(many).await.is_err());
}

#[tokio::test]
async fn a_client_that_sends_too_many_bytes_in_configuration_is_cut() {
    // each fits the limit of one message, the four together do not fit the limit of all
    let big = vec![0; configuration::MAX_PLUGIN_BYTES / 3 + 1];
    let several = (0..4).map(|_| payload("mod:big", big.clone())).collect();

    assert!(configure_sending(several).await.is_err());
}
