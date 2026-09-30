// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Answers the server list and accepts offline logins, stopping at Play, to check both
//! against a real client: `cargo run -p lodeframe --example offline_login [addr]`.

use lodeframe::{
    configuration, login,
    net::{Config, serve},
    protocol::State,
    registry::Registries,
    status::{self, StatusInfo},
};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let addr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:25565".into());
    let listener = TcpListener::bind(&addr).await?;
    eprintln!("listening on {addr}");
    let registries = std::sync::Arc::new(Registries::vanilla());
    serve(listener, Config::default(), move |mut conn, intention| {
        let registries = registries.clone();
        async move {
            eprintln!("handshake: {intention:?}");
            if conn.state() == State::Status {
                return status::respond(&mut conn, &StatusInfo::new("lodeframe")).await;
            }
            let result = async {
                let profile = login::offline(&mut conn, Some(256)).await?;
                eprintln!("login: {profile:?}");
                configuration::run(&mut conn, &registries).await?;
                eprintln!("{}: reached play", profile.name);
                Ok(())
            }
            .await;
            if let Err(e) = &result {
                eprintln!("failed: {e}");
            }
            result
        }
    })
    .await
}
