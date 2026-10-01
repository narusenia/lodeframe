// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Runs the lobby: `cargo run -p lobby [addr]`.
//!
//! `RUST_LOG=debug` (or `trace`, `warn`, ...) sets how much is logged; the default is `info`.

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
    Server::new(addr)
        .motd("lodeframe lobby")
        .run(lobby::lobby)
        .await
}
