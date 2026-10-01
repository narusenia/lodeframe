// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Bots play on a real server over TCP: they see each other, walk, chat, and place and break
//! blocks.

use std::time::Duration;

use lodeframe::{
    chunk::FlatGenerator,
    protocol::{
        BlockPos, Direction, Vec3,
        block::{AIR, STONE},
        packets::play::{
            AddEntity, BlockChangedAck, BlockUpdate, EntityPositionSync, PlayerInfoAdd,
            PlayerInfoRemove, RemoveEntities,
        },
    },
    registry::Registries,
    server::{RunningServer, Server},
    world::World,
};
use lodeframe_bot::{Bot, ChatLine, Frame, spread_position};

/// How long a bot waits for one packet. Generous: this only bounds a failing test.
const WAIT: Duration = Duration::from_secs(10);

/// The top layer of the flat world: grass at y = -61, air above it.
const GROUND: BlockPos = BlockPos::new(0, -61, 0);

/// A server on a free port with a flat world.
async fn start() -> RunningServer {
    Server::new("127.0.0.1:0")
        .start(|registries: &Registries| {
            let mut world = World::new(registries, FlatGenerator::default());
            // few chunks, so that a debug build joins quickly
            world.view_distance = 2;
            world
        })
        .await
        .unwrap()
}

fn entity_added(id: i32) -> impl FnMut(&Frame) -> Option<AddEntity> {
    move |f| {
        f.decode::<AddEntity>()
            .ok()
            .filter(|a| f.is::<AddEntity>() && a.entity_id.0 == id)
    }
}

fn block_set(
    pos: BlockPos,
    state: lodeframe::protocol::BlockState,
) -> impl FnMut(&Frame) -> Option<()> {
    move |f| {
        let update = f
            .decode::<BlockUpdate>()
            .ok()
            .filter(|_| f.is::<BlockUpdate>())?;
        (update.pos == pos && update.state == state).then_some(())
    }
}

fn confirmed(sequence: i32) -> impl FnMut(&Frame) -> Option<()> {
    move |f| {
        let ack = f
            .decode::<BlockChangedAck>()
            .ok()
            .filter(|_| f.is::<BlockChangedAck>())?;
        (ack.sequence.0 == sequence).then_some(())
    }
}

fn said(name: &'static str, text: &'static str) -> impl FnMut(&Frame) -> Option<()> {
    move |f| {
        (f.chat_line()
            == Some(ChatLine {
                name: name.into(),
                text: text.into(),
            }))
        .then_some(())
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn two_bots_see_each_other_walk_chat_and_edit_blocks() {
    let server = start().await;
    let addr = server.addr();
    let mut alice = Bot::connect(addr, "Alice").await.unwrap();
    let mut bob = Bot::connect(addr, "Bob").await.unwrap();
    let (alice_id, bob_id) = (alice.entity_id(), bob.entity_id());
    assert_ne!(alice_id, bob_id);

    // they see each other, and are in each other's tab list
    let seen = alice.recv_until(WAIT, entity_added(bob_id)).await.unwrap();
    assert_eq!(seen.uuid, bob.uuid());
    // the tab list comes before the entities, and a wait drops what it skips
    bob.recv_until(WAIT, |f| {
        let info = f
            .decode::<PlayerInfoAdd>()
            .ok()
            .filter(|_| f.is::<PlayerInfoAdd>())?;
        info.players.iter().any(|p| p.name == "Alice").then_some(())
    })
    .await
    .unwrap();
    bob.recv_until(WAIT, entity_added(alice_id)).await.unwrap();

    // a walk shows up for the other one
    let there = Vec3::new(5.5, -60.0, 0.5);
    alice.move_to(there).await.unwrap();
    let synced = bob
        .recv_until(WAIT, |f| {
            let sync = f
                .decode::<EntityPositionSync>()
                .ok()
                .filter(|_| f.is::<EntityPositionSync>())?;
            (sync.entity_id.0 == alice_id && sync.position == there).then_some(sync)
        })
        .await
        .unwrap();
    assert!(synced.on_ground);

    // a chat line reaches both, the sender too
    alice.chat("hello").await.unwrap();
    alice
        .recv_until(WAIT, said("Alice", "hello"))
        .await
        .unwrap();
    bob.recv_until(WAIT, said("Alice", "hello")).await.unwrap();

    // a block placed on the ground is stone above it, for both, and confirmed to the placer
    let above = BlockPos::new(0, -60, 0);
    let placed = alice.place(GROUND, Direction::Up).await.unwrap();
    bob.recv_until(WAIT, block_set(above, STONE.default_state()))
        .await
        .unwrap();
    alice
        .recv_until(WAIT, block_set(above, STONE.default_state()))
        .await
        .unwrap();
    alice.recv_until(WAIT, confirmed(placed)).await.unwrap();

    // and breaking it leaves air
    let broken = alice.dig(above).await.unwrap();
    bob.recv_until(WAIT, block_set(above, AIR.default_state()))
        .await
        .unwrap();
    alice
        .recv_until(WAIT, block_set(above, AIR.default_state()))
        .await
        .unwrap();
    alice.recv_until(WAIT, confirmed(broken)).await.unwrap();

    // leaving takes the player out of the world and the tab list
    let alice_uuid = alice.uuid();
    drop(alice);
    bob.recv_until(WAIT, |f| {
        let gone = f
            .decode::<RemoveEntities>()
            .ok()
            .filter(|_| f.is::<RemoveEntities>())?;
        gone.entity_ids
            .iter()
            .any(|e| e.0 == alice_id)
            .then_some(())
    })
    .await
    .unwrap();
    bob.recv_until(WAIT, |f| {
        let gone = f
            .decode::<PlayerInfoRemove>()
            .ok()
            .filter(|_| f.is::<PlayerInfoRemove>())?;
        gone.uuids.contains(&alice_uuid).then_some(())
    })
    .await
    .unwrap();

    server.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn bots_that_wander_are_kept_for_the_whole_run() {
    let server = start().await;
    let addr = server.addr();
    let mut bots = Vec::new();
    for index in 0..3 {
        bots.push(tokio::spawn(async move {
            let mut bot = Bot::connect(addr, &format!("bot{index}")).await?;
            bot.wander(index, Duration::from_secs(2)).await?;
            Ok::<_, lodeframe_bot::Error>(bot.received())
        }));
    }
    for bot in bots {
        let received = bot.await.unwrap().unwrap();
        // at least the join, and the others' walking
        assert!(received > 10, "only {received} packets");
    }

    server.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_bot_out_of_sight_is_removed_and_its_moves_do_not_arrive_but_its_chat_does() {
    let server = start().await;
    let mut alice = Bot::connect(server.addr(), "Alice").await.unwrap();
    let mut bob = Bot::connect(server.addr(), "Bob").await.unwrap();
    let bob_id = bob.entity_id();
    alice.recv_until(WAIT, entity_added(bob_id)).await.unwrap();

    // four chunks away, with a view distance of 2: out of sight
    let far = spread_position(1, 4);
    bob.move_to(far).await.unwrap();
    alice
        .recv_until(WAIT, |f| {
            let gone = f
                .decode::<RemoveEntities>()
                .ok()
                .filter(|_| f.is::<RemoveEntities>())?;
            gone.entity_ids.iter().any(|e| e.0 == bob_id).then_some(())
        })
        .await
        .unwrap();

    // what Bob does there does not reach Alice, but what he says does
    bob.move_to(far + Vec3::new(1.0, 0.0, 0.0)).await.unwrap();
    bob.chat("can you hear me").await.unwrap();
    let heard = alice
        .recv_until(WAIT, |f| {
            assert!(
                !f.is::<EntityPositionSync>(),
                "a move arrived from out of sight"
            );
            f.chat_line()
        })
        .await
        .unwrap();
    assert_eq!(
        (heard.name.as_str(), heard.text.as_str()),
        ("Bob", "can you hear me")
    );

    server.stop();
}
