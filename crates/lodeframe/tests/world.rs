// SPDX-License-Identifier: Apache-2.0 OR MIT
//! A player joins a world, walks over a chunk border, and gets the chunks they need.

use lodeframe::{
    chunk::FlatGenerator,
    protocol::{
        Vec3,
        ids::play::{clientbound as out, serverbound},
        packets::play::{
            ForgetLevelChunk, Login, MovePlayerPos, PlayerPosition, SetChunkCacheCenter,
        },
    },
    registry::Registries,
    test_util::{Received, TestEnv},
    world::World,
};

fn count(received: &[Received], id: i32) -> usize {
    received.iter().filter(|r| r.id == id).count()
}

fn walk(x: f64) -> MovePlayerPos {
    MovePlayerPos {
        position: Vec3::new(x, -60.0, 0.5),
        flags: 1,
    }
}

fn env() -> TestEnv<World<FlatGenerator>> {
    let mut world = World::new(&Registries::vanilla(), FlatGenerator::default());
    world.view_distance = 2;
    TestEnv::new(world)
}

#[test]
fn a_player_gets_the_world_on_join() {
    let mut env = env();
    let mut steve = env.connect("Steve");

    let joined = steve.drain();
    assert_eq!(joined[0].id, out::LOGIN);
    assert_eq!(joined[1].id, out::PLAYER_POSITION);
    assert_eq!(count(&joined, out::LEVEL_CHUNK_WITH_LIGHT), 25);
    assert_eq!(joined.last().unwrap().id, out::CHUNK_BATCH_FINISHED);

    let login: Login = joined[0].decode().unwrap();
    assert_eq!(login.view_distance.0, 2);
    let at: PlayerPosition = joined[1].decode().unwrap();
    assert_eq!(at.position, Vec3::new(0.5, -60.0, 0.5));
}

#[test]
fn crossing_a_chunk_border_swaps_a_column_of_chunks() {
    let mut env = env();
    let mut steve = env.connect("Steve");
    steve.drain();

    // inside the same chunk: nothing to send
    env.send(&steve, &walk(5.5));
    assert!(steve.drain().is_empty());

    // one chunk east: a new column of 5 in, the far column of 5 out
    env.send(&steve, &walk(16.5));
    let moved = steve.drain();
    assert_eq!(count(&moved, out::LEVEL_CHUNK_WITH_LIGHT), 5);
    let center: SetChunkCacheCenter = moved
        .iter()
        .find(|r| r.is::<SetChunkCacheCenter>())
        .unwrap()
        .decode()
        .unwrap();
    assert_eq!((center.x.0, center.z.0), (1, 0));
    let mut forgotten = Vec::new();
    for r in moved.iter().filter(|r| r.is::<ForgetLevelChunk>()) {
        let f: ForgetLevelChunk = r.decode().unwrap();
        forgotten.push((f.x, f.z));
    }
    assert_eq!(forgotten, [(-2, -2), (-2, -1), (-2, 0), (-2, 1), (-2, 2)]);
}

#[test]
fn a_broken_packet_drops_the_player() {
    let mut env = env();
    let mut steve = env.connect("Steve");
    steve.drain();

    // a move packet with no payload
    env.send_raw(&steve, vec![serverbound::MOVE_PLAYER_POS as u8]);
    assert!(steve.is_disconnected());
}
