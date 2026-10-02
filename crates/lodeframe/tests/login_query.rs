// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Questions the server asks a client that is logging in, over an in-memory pipe.

use std::{io, time::Duration};

use lodeframe::{
    login::Queries,
    net::Connection,
    protocol::{
        Error, Identifier, State, VarInt,
        packets::login::{CustomQuery, CustomQueryAnswer, Hello},
    },
};
use tokio::io::duplex;

const T: Duration = Duration::from_secs(5);

fn channel() -> Identifier {
    Identifier::new("test:ask").unwrap()
}

/// A server and a client in the login state.
fn pair() -> (
    Connection<tokio::io::DuplexStream>,
    Connection<tokio::io::DuplexStream>,
) {
    let (a, b) = duplex(1 << 16);
    let mut server = Connection::new(a, T);
    let mut client = Connection::new(b, T);
    server.set_state(State::Login);
    client.set_state(State::Login);
    (server, client)
}

#[tokio::test]
async fn an_answer_comes_back_with_its_data() {
    let (mut server, mut client) = pair();
    let asking = tokio::spawn(async move {
        Queries::default()
            .ask(&mut server, channel(), b"who?", T)
            .await
    });

    let query: CustomQuery = client.read_packet().await.unwrap();
    assert_eq!((query.channel, query.data), (channel(), b"who?".to_vec()));
    client
        .write_packet(&CustomQueryAnswer {
            transaction_id: query.transaction_id,
            data: Some(b"me".to_vec()),
        })
        .await
        .unwrap();

    assert_eq!(asking.await.unwrap().unwrap(), Some(b"me".to_vec()));
}

#[tokio::test]
async fn a_client_that_does_not_know_the_channel_answers_nothing() {
    let (mut server, mut client) = pair();
    let asking =
        tokio::spawn(async move { Queries::default().ask(&mut server, channel(), &[], T).await });

    let query: CustomQuery = client.read_packet().await.unwrap();
    client
        .write_packet(&CustomQueryAnswer {
            transaction_id: query.transaction_id,
            data: None,
        })
        .await
        .unwrap();

    assert_eq!(asking.await.unwrap().unwrap(), None);
}

#[tokio::test]
async fn every_question_has_its_own_id() {
    let (mut server, mut client) = pair();
    let asking = tokio::spawn(async move {
        let mut queries = Queries::default();
        queries.ask(&mut server, channel(), &[], T).await.unwrap();
        queries.ask(&mut server, channel(), &[], T).await.unwrap();
    });

    let mut ids = Vec::new();
    for _ in 0..2 {
        let query: CustomQuery = client.read_packet().await.unwrap();
        ids.push(query.transaction_id);
        client
            .write_packet(&CustomQueryAnswer {
                transaction_id: query.transaction_id,
                data: None,
            })
            .await
            .unwrap();
    }

    asking.await.unwrap();
    assert_ne!(ids[0], ids[1]);
}

#[tokio::test]
async fn a_client_that_never_answers_times_out() {
    let (mut server, _client) = pair();

    let result = Queries::default()
        .ask(&mut server, channel(), &[], Duration::from_millis(50))
        .await;

    assert!(matches!(
        result,
        Err(Error::Io(e)) if e.kind() == io::ErrorKind::TimedOut
    ));
}

#[tokio::test]
async fn an_answer_to_another_question_is_refused() {
    let (mut server, mut client) = pair();
    let asking =
        tokio::spawn(async move { Queries::default().ask(&mut server, channel(), &[], T).await });

    let query: CustomQuery = client.read_packet().await.unwrap();
    client
        .write_packet(&CustomQueryAnswer {
            transaction_id: VarInt(query.transaction_id.0 + 1),
            data: Some(Vec::new()),
        })
        .await
        .unwrap();

    assert!(asking.await.unwrap().is_err());
}

#[tokio::test]
async fn any_other_packet_in_place_of_the_answer_is_refused() {
    let (mut server, mut client) = pair();
    let asking =
        tokio::spawn(async move { Queries::default().ask(&mut server, channel(), &[], T).await });

    let _: CustomQuery = client.read_packet().await.unwrap();
    client
        .write_packet(&Hello {
            name: "Notch".into(),
            uuid: lodeframe::protocol::Uuid(0),
        })
        .await
        .unwrap();

    assert!(asking.await.unwrap().is_err());
}

#[tokio::test]
async fn nothing_is_asked_outside_the_login_state() {
    let (mut server, _client) = pair();
    server.set_state(State::Configuration);

    let result = Queries::default().ask(&mut server, channel(), &[], T).await;

    assert!(result.is_err());
}
