// SPDX-License-Identifier: Apache-2.0 OR MIT
//! A minimal lobby server built only on lodeframe's event API.
//!
//! A flat world where players are greeted, can build in a small area above the floor and
//! nowhere else, and are told that commands do not exist.

use lodeframe::{
    chunk::FlatGenerator,
    registry::Registries,
    text::{Color, Component},
    world::{
        BlockBreakEvent, BlockPlaceEvent, ChatEvent, PlayerJoinEvent, PlayerLeaveEvent, World,
    },
};

/// The top of the floor (grass over dirt over bedrock). Nothing at or below it can be broken.
pub const FLOOR: i32 = -61;
/// Blocks can be placed this far from the spawn, in x and z.
pub const BUILD_RADIUS: i32 = 16;
/// The highest block that can be placed.
pub const BUILD_TOP: i32 = -50;

type Lobby = World<FlatGenerator>;

/// The lobby world. Build it with [`lodeframe::server::Server::run`].
pub fn lobby(registries: &Registries) -> Lobby {
    let mut world = World::new(registries, FlatGenerator::default());
    let events = world.events_mut();

    events.on(|e: &mut PlayerJoinEvent, world: &mut Lobby| {
        let welcome = Component::text("Welcome to the lobby!")
            .color(Color::Gold)
            .bold();
        world.send_message(e.player, &welcome);
        world.broadcast(&Component::text(format!("+ {}", e.name)).color(Color::Green));
    });
    events.on(|e: &mut PlayerLeaveEvent, world: &mut Lobby| {
        world.broadcast(&Component::text(format!("- {}", e.name)).color(Color::Red));
    });

    events.on(|e: &mut BlockBreakEvent, _: &mut Lobby| {
        if e.pos.y <= FLOOR {
            e.cancel();
        }
    });
    events.on(|e: &mut BlockPlaceEvent, _: &mut Lobby| {
        let inside = e.pos.x.abs() <= BUILD_RADIUS
            && e.pos.z.abs() <= BUILD_RADIUS
            && (FLOOR + 1..=BUILD_TOP).contains(&e.pos.y);
        if !inside {
            e.cancel();
        }
    });

    events.on(|e: &mut ChatEvent, world: &mut Lobby| {
        let text = e.message.text.clone();
        if text.starts_with('.') {
            e.cancel();
            let hint = Component::text("There are no commands in the lobby.").color(Color::Gray);
            world.send_message(e.player, &hint);
        } else {
            e.message = Component::text(text).color(Color::White);
        }
    });

    world
}
