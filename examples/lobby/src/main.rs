// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Runs the lobby: `cargo run -p lobby [addr]`.
//!
//! `RUST_LOG=debug` (or `trace`, `warn`, ...) sets how much is logged; the default is `info`.
//!
//! For `cargo xtask bench`: with `LOBBY_STATS` set it prints a `tick-stats` line to the standard
//! output every second, and `LOBBY_VIEW_DISTANCE` sets the chunk radius players get.

use std::time::Duration;

use lodeframe::server::Server;
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
    let view_distance: Option<u32> = std::env::var("LOBBY_VIEW_DISTANCE")
        .ok()
        .and_then(|v| v.parse().ok());
    let server = Server::new(addr)
        .motd("lodeframe lobby")
        .start(move |registries| {
            let mut world = lobby::lobby(registries);
            if let Some(view_distance) = view_distance {
                world.view_distance = view_distance;
            }
            world
        })
        .await?;
    if std::env::var_os("LOBBY_STATS").is_some() {
        let instance = server.instance();
        tokio::spawn(async move {
            let mut every_second = tokio::time::interval(Duration::from_secs(1));
            loop {
                every_second.tick().await;
                let s = instance.tick_stats();
                println!(
                    "tick-stats ticks={} busy_us={} busy_max_us={} late={} skipped={}",
                    s.ticks,
                    s.busy.as_micros(),
                    s.busy_max.as_micros(),
                    s.late,
                    s.skipped
                );
            }
        });
    }
    server.wait().await
}
