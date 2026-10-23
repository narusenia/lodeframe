// SPDX-License-Identifier: Apache-2.0 OR MIT
//! A minimal lobby server built only on lodeframe's event API.
//!
//! A flat world where players are greeted, can build in a small area above the floor and
//! nowhere else, are told that commands do not exist, and are put back at the spawn when they
//! fall off the world.

use lodeframe::{
    chunk::FlatGenerator,
    player::PlayerMoveEvent,
    registry::Registries,
    text::{Color, Component},
    world::{
        BlockBreakEvent, BlockPlaceEvent, ChatEvent, Ctx, PlayerJoinEvent, PlayerLeaveEvent, World,
    },
};

/// The top of the floor (grass over dirt over bedrock). Nothing at or below it can be broken.
pub const FLOOR: i32 = -61;
/// Blocks can be placed this far from the spawn, in x and z.
pub const BUILD_RADIUS: i32 = 16;
/// The highest block that can be placed.
pub const BUILD_TOP: i32 = -50;
/// A player who falls below this is put back at the spawn.
pub const VOID: f64 = -70.0;

/// The lobby world. Build it with [`lodeframe::server::Server::run`].
pub fn lobby(registries: &Registries) -> World {
    let mut world = World::new(registries, FlatGenerator::default());
    let events = world.events_mut();

    events.on(|e: &mut PlayerJoinEvent, ctx: &mut Ctx| {
        let welcome = Component::text("Welcome to the lobby!")
            .color(Color::Gold)
            .bold();
        ctx.send_message(e.player, &welcome);
        ctx.broadcast(&Component::text(format!("+ {}", e.name)).color(Color::Green));
    });
    events.on(|e: &mut PlayerLeaveEvent, ctx: &mut Ctx| {
        ctx.broadcast(&Component::text(format!("- {}", e.name)).color(Color::Red));
    });

    // fell off the world: sent to the spawn instead of where the client says it is
    events.on(|e: &mut PlayerMoveEvent, ctx: &mut Ctx| {
        if e.position.y < VOID {
            e.position = ctx.spawn;
        }
    });

    events.on(|e: &mut BlockBreakEvent, _: &mut Ctx| {
        if e.pos.y <= FLOOR {
            e.cancel();
        }
    });
    events.on(|e: &mut BlockPlaceEvent, _: &mut Ctx| {
        let inside = e.pos.x.abs() <= BUILD_RADIUS
            && e.pos.z.abs() <= BUILD_RADIUS
            && (FLOOR + 1..=BUILD_TOP).contains(&e.pos.y);
        if !inside {
            e.cancel();
        }
    });

    events.on(|e: &mut ChatEvent, ctx: &mut Ctx| {
        let text = e.message.as_text().unwrap_or_default().to_owned();
        if text.starts_with('.') {
            e.cancel();
            let hint = Component::text("There are no commands in the lobby.").color(Color::Gray);
            ctx.send_message(e.player, &hint);
        } else {
            e.message = Component::text(text).color(Color::White);
        }
    });

    world
}
