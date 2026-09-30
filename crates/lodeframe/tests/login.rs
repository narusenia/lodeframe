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
        configuration::run(&mut server, &registries).await.unwrap();
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

    // configuration
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
        tokio::spawn(async move { configuration::run(&mut server, &Registries::vanilla()).await });
    let _: UpdateEnabledFeatures = client.read_packet().await.unwrap();
    let _: ClientboundKnownPacks = client.read_packet().await.unwrap();
    client
        .write_packet(&ServerboundKnownPacks { packs: Vec::new() })
        .await
        .unwrap();
    assert!(task.await.unwrap().is_err());
}
