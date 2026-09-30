// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Answers the server list and accepts offline logins into a flat world, to check the
//! whole connection path against a real client:
//! `cargo run -p lodeframe --example offline_login [addr]`.
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
    world::World,
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
        World::new(&world_registries, FlatGenerator::default())
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
