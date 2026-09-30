// SPDX-License-Identifier: Apache-2.0 OR MIT
//! A player joins a world, walks over a chunk border, and gets the chunks they need.

use lodeframe::{
    chunk::FlatGenerator,
    instance::{Instance, Message},
    login::Profile,
    protocol::{
        Uuid, Vec3, ids::play::clientbound as out, packet_body, packets::play::MovePlayerPos,
        split_packet_id,
    },
    registry::Registries,
    world::World,
};
use tokio::sync::mpsc;

fn ids(rx: &mut mpsc::Receiver<Vec<u8>>) -> Vec<i32> {
    let mut ids = Vec::new();
    while let Ok(body) = rx.try_recv() {
        ids.push(split_packet_id(&body).unwrap().0);
    }
    ids
}

fn count(ids: &[i32], id: i32) -> usize {
    ids.iter().filter(|i| **i == id).count()
}

#[test]
fn a_player_gets_the_world_on_join_and_new_chunks_when_crossing_a_border() {
    let mut world = World::new(&Registries::vanilla(), FlatGenerator::default());
    world.view_distance = 2;
    let (tx, mut rx) = mpsc::channel(1024);
    let id = Uuid(1);
    world.handle(Message::Join {
        profile: Profile {
            uuid: id,
            name: "Steve".into(),
        },
        outbound: tx,
    });

    let joined = ids(&mut rx);
    assert_eq!(joined[0], out::LOGIN);
    assert_eq!(joined[1], out::PLAYER_POSITION);
    assert_eq!(count(&joined, out::LEVEL_CHUNK_WITH_LIGHT), 25);
    assert_eq!(joined.last(), Some(&out::CHUNK_BATCH_FINISHED));

    // inside the same chunk: nothing to send
    let walk = |x: f64| Message::Packet {
        player: id,
        body: packet_body(&MovePlayerPos {
            position: Vec3::new(x, -60.0, 0.5),
            flags: 1,
        })
        .unwrap(),
    };
    world.handle(walk(5.5));
    assert!(ids(&mut rx).is_empty());

    // one chunk east: a new column of 5 in, the far column of 5 out
    world.handle(walk(16.5));
    let moved = ids(&mut rx);
    assert_eq!(count(&moved, out::SET_CHUNK_CACHE_CENTER), 1);
    assert_eq!(count(&moved, out::FORGET_LEVEL_CHUNK), 5);
    assert_eq!(count(&moved, out::LEVEL_CHUNK_WITH_LIGHT), 5);

    // a broken packet drops the player
    world.handle(Message::Packet {
        player: id,
        body: vec![ids_move_pos(), 0],
    });
    assert!(rx.try_recv().is_err());
    assert!(matches!(
        rx.try_recv(),
        Err(mpsc::error::TryRecvError::Disconnected)
    ));
}

fn ids_move_pos() -> u8 {
    lodeframe::protocol::ids::play::serverbound::MOVE_PLAYER_POS as u8
}
