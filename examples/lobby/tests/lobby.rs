// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Bots in the lobby: they are greeted, can build in the build area only, and are told that
//! there are no commands.

use std::time::Duration;

use lobby::{BUILD_RADIUS, BUILD_TOP, lobby};
use lodeframe::{
    protocol::{
        BlockPos, BlockState, Direction,
        block::{AIR, STONE},
        packets::play::{BlockChangedAck, BlockUpdate},
    },
    registry::Registries,
    server::{RunningServer, Server},
};
use lodeframe_bot::{Bot, Frame};

/// How long a bot waits for one packet. Generous: this only bounds a failing test.
const WAIT: Duration = Duration::from_secs(10);

/// The top layer of the floor: grass at y = -61.
const GROUND: BlockPos = BlockPos::new(0, -61, 0);

async fn start() -> RunningServer {
    Server::new("127.0.0.1:0")
        .start(|registries: &Registries| {
            let mut world = lobby(registries);
            // few chunks, so that a debug build joins quickly
            world.view_distance = 2;
            world
        })
        .await
        .unwrap()
}

fn told(text: &'static str) -> impl FnMut(&Frame) -> Option<()> {
    move |f| {
        f.system_message()
            .is_some_and(|m| m.contains(text))
            .then_some(())
    }
}

fn block_update(f: &Frame) -> Option<BlockUpdate> {
    f.decode::<BlockUpdate>()
        .ok()
        .filter(|_| f.is::<BlockUpdate>())
}

fn block_set(pos: BlockPos, state: BlockState) -> impl FnMut(&Frame) -> Option<()> {
    move |f| {
        block_update(f)
            .filter(|u| u.pos == pos && u.state == state)
            .map(|_| ())
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

#[tokio::test(flavor = "multi_thread")]
async fn players_are_greeted_and_the_others_see_them_come_and_go() {
    let server = start().await;
    let mut alice = Bot::connect(server.addr(), "Alice").await.unwrap();
    alice.recv_until(WAIT, told("Welcome")).await.unwrap();

    let mut bob = Bot::connect(server.addr(), "Bob").await.unwrap();
    bob.recv_until(WAIT, told("Welcome")).await.unwrap();
    alice.recv_until(WAIT, told("+ Bob")).await.unwrap();

    drop(bob);
    alice.recv_until(WAIT, told("- Bob")).await.unwrap();

    server.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_floor_cannot_be_broken_and_the_build_area_can_be_built_in() {
    let server = start().await;
    let mut alice = Bot::connect(server.addr(), "Alice").await.unwrap();
    let mut bob = Bot::connect(server.addr(), "Bob").await.unwrap();

    // the floor stays: Bob's client is told what is there, and Alice hears nothing about it
    let dug = bob.dig(GROUND).await.unwrap();
    bob.recv_until(WAIT, confirmed(dug)).await.unwrap();

    // inside the area a block is placed, for both; Alice's first block change is this one,
    // so nothing was broken above
    let inside = BlockPos::new(3, -60, 3);
    let placed = bob
        .place(BlockPos::new(3, -61, 3), Direction::Up)
        .await
        .unwrap();
    let first = alice.recv_until(WAIT, block_update).await.unwrap();
    assert_eq!((first.pos, first.state), (inside, STONE.default_state()));
    bob.recv_until(WAIT, confirmed(placed)).await.unwrap();

    // outside the radius, and above the top, nothing is placed: Bob is told the air is still
    // there
    for (click, resync) in [
        (
            BlockPos::new(BUILD_RADIUS + 1, -61, 0),
            BlockPos::new(BUILD_RADIUS + 1, -60, 0),
        ),
        (
            BlockPos::new(3, BUILD_TOP, 3),
            BlockPos::new(3, BUILD_TOP + 1, 3),
        ),
    ] {
        let sequence = bob.place(click, Direction::Up).await.unwrap();
        bob.recv_until(WAIT, block_set(resync, AIR.default_state()))
            .await
            .unwrap();
        bob.recv_until(WAIT, confirmed(sequence)).await.unwrap();
    }
    // and it is still possible to break what was built
    let broken = bob.dig(inside).await.unwrap();
    alice
        .recv_until(WAIT, block_set(inside, AIR.default_state()))
        .await
        .unwrap();
    bob.recv_until(WAIT, confirmed(broken)).await.unwrap();

    server.stop();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_command_is_refused_and_the_next_line_goes_through() {
    let server = start().await;
    let mut alice = Bot::connect(server.addr(), "Alice").await.unwrap();
    let mut bob = Bot::connect(server.addr(), "Bob").await.unwrap();

    alice.chat(".help").await.unwrap();
    alice.recv_until(WAIT, told("no commands")).await.unwrap();
    alice.chat("hello").await.unwrap();

    // the first line Bob gets is the one that was let through
    let line = bob.recv_until(WAIT, Frame::chat_line).await.unwrap();
    assert_eq!((line.name.as_str(), line.text.as_str()), ("Alice", "hello"));

    server.stop();
}
