// SPDX-License-Identifier: Apache-2.0 OR MIT
//! A flat world with block event handlers, to check breaking and placing against real
//! clients: `cargo run -p lodeframe --example blocks [addr]`.
//!
//! - placed blocks are cobblestone, whatever is in hand
//! - nothing can be placed higher than y = -50
//! - the bedrock at the bottom cannot be broken
//!
//! `RUST_LOG=debug` (or `trace`, `warn`, ...) sets how much is logged; the default is `info`.

use lodeframe::{
    chunk::FlatGenerator,
    protocol::block::{BEDROCK, COBBLESTONE},
    registry::Registries,
    server::Server,
    world::{BlockBreakEvent, BlockPlaceEvent, Ctx, World},
};
use tracing::Level;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let level = std::env::var("RUST_LOG")
        .ok()
        .and_then(|v| v.parse::<Level>().ok())
        .unwrap_or(Level::INFO);
    tracing_subscriber::fmt().with_max_level(level).init();

    let addr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:25565".into());
    Server::new(addr)
        .run(|registries: &Registries| {
            let mut world = World::new(registries, FlatGenerator::default());
            let events = world.events_mut();
            events.on(|e: &mut BlockPlaceEvent, _: &mut Ctx| {
                if e.pos.y > -50 {
                    e.cancel();
                } else {
                    e.block = COBBLESTONE.default_state();
                }
            });
            events.on(|e: &mut BlockBreakEvent, _: &mut Ctx| {
                if e.block == BEDROCK.default_state() {
                    e.cancel();
                }
            });
            world
        })
        .await
}
