// SPDX-License-Identifier: Apache-2.0 OR MIT
//! A flat world with a chat event handler, to check chat against real clients:
//! `cargo run -p lodeframe --example chat [addr]`.
//!
//! - a line ending in `!` is shown in bold red
//! - a line starting with `/` or `.` is not sent to anyone; the sender gets a server message
//! - everything else goes out as typed
//!
//! `RUST_LOG=debug` (or `trace`, `warn`, ...) sets how much is logged; the default is `info`.

use std::sync::Arc;

use lodeframe::{
    chunk::FlatGenerator,
    clock::SystemClock,
    configuration, instance, login,
    net::{Config, serve},
    play,
    protocol::State,
    registry::Registries,
    status::{self, StatusInfo},
    text::{Color, Component},
    world::{ChatEvent, World},
};
use tokio::net::TcpListener;
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
    let listener = TcpListener::bind(&addr).await?;
    tracing::info!(%addr, "listening");

    let registries = Arc::new(Registries::vanilla());
    let world_registries = registries.clone();
    let lobby = instance::spawn("lobby", SystemClock, move || {
        let mut world = World::new(&world_registries, FlatGenerator::default());
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
    })?;
    serve(listener, Config::default(), move |mut conn, intention| {
        let (registries, lobby) = (registries.clone(), lobby.clone());
        async move {
            tracing::debug!(?intention, "handshake");
            if conn.state() == State::Status {
                return status::respond(&mut conn, &StatusInfo::new("lodeframe")).await;
            }
            let profile = login::offline(&mut conn, Some(256)).await?;
            configuration::run(&mut conn, &registries).await?;
            play::run(conn, profile, lobby).await
        }
    })
    .await
}
