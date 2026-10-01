// SPDX-License-Identifier: Apache-2.0 OR MIT
//! A flat world with a chat event handler, to check chat against real clients:
//! `cargo run -p lodeframe --example chat [addr]`.
//!
//! - a line ending in `!` is shown in bold red
//! - a line starting with `/` or `.` is not sent to anyone; the sender gets a server message
//! - everything else goes out as typed
//!
//! `RUST_LOG=debug` (or `trace`, `warn`, ...) sets how much is logged; the default is `info`.

use lodeframe::{
    chunk::FlatGenerator,
    registry::Registries,
    server::Server,
    text::{Color, Component},
    world::{ChatEvent, World},
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
            world
                .events_mut()
                .on(|e: &mut ChatEvent, world: &mut World<FlatGenerator>| {
                    let typed = e.message.text.clone();
                    if typed.starts_with(['.', '/']) {
                        e.cancel();
                        let reply =
                            Component::text("commands are not supported yet").color(Color::Gray);
                        world.send_message(e.player, &reply);
                    } else if typed.ends_with('!') {
                        e.message = Component::text(typed).color(Color::Red).bold();
                    }
                });
            world
        })
        .await
}
