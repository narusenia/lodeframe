// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Answers the server list and accepts offline logins into an empty instance, to check the
//! whole connection path against a real client:
//! `cargo run -p lodeframe --example offline_login [addr]`.
//!
//! `RUST_LOG=debug` (or `trace`, `warn`, ...) sets how much is logged; the default is `info`.

use std::sync::Arc;

use lodeframe::{
    clock::SystemClock,
    configuration,
    instance::{self, Instance, Message, Sessions},
    login,
    net::{Config, serve},
    play,
    protocol::State,
    registry::Registries,
    status::{self, StatusInfo},
};
use tokio::net::TcpListener;
use tracing::Level;

/// An instance with nothing in it but the players.
struct Empty {
    sessions: Sessions,
}

impl Instance for Empty {
    fn handle(&mut self, message: Message) {
        match message {
            Message::Join { profile, outbound } => {
                tracing::info!(name = %profile.name, players = self.sessions.len() + 1, "joined");
                self.sessions.join(profile.uuid, outbound);
            }
            Message::Leave { player } => self.sessions.leave(player),
            Message::Packet { .. } => {}
        }
    }

    fn tick(&mut self) {}
}

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

    let lobby = instance::spawn("lobby", SystemClock, || Empty {
        sessions: Sessions::default(),
    })?;
    let registries = Arc::new(Registries::vanilla());
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
