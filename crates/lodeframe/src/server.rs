// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Starting a server: listens, runs the connection path for every client and ticks one
//! instance.
//!
//! ```no_run
//! use lodeframe::{chunk::FlatGenerator, registry::Registries, server::Server, world::World};
//!
//! #[tokio::main]
//! async fn main() -> std::io::Result<()> {
//!     Server::new("127.0.0.1:25565")
//!         .motd("a flat world")
//!         .run(|registries: &Registries| World::new(registries, FlatGenerator::default()))
//!         .await
//! }
//! ```

use std::{io, net::SocketAddr, sync::Arc};

use tokio::{net::TcpListener, task::JoinHandle};

use crate::{
    clock::SystemClock,
    configuration,
    instance::{self, Instance, InstanceHandle, TickStats},
    login,
    net::{Config, serve},
    play,
    protocol::State,
    registry::Registries,
    status::{self, StatusInfo},
};

/// Bodies of at least this many bytes are compressed on the wire.
// ponytail: fixed; make it an option when someone needs another value
const COMPRESSION_THRESHOLD: usize = 256;

/// What a server is called and where it listens. Start it with [`run`](Self::run) or
/// [`start`](Self::start).
#[derive(Debug, Clone)]
pub struct Server {
    address: String,
    motd: String,
}

impl Server {
    /// A server that will listen on `address`, such as `127.0.0.1:25565`. Port 0 picks a free
    /// one; [`RunningServer::addr`] tells which.
    pub fn new(address: impl Into<String>) -> Self {
        Self {
            address: address.into(),
            motd: "lodeframe".into(),
        }
    }

    /// The line shown under the name in the server list.
    pub fn motd(mut self, motd: impl Into<String>) -> Self {
        self.motd = motd.into();
        self
    }

    /// Starts the server and waits until it stops, which is only when listening fails.
    pub async fn run<I, F>(self, world: F) -> io::Result<()>
    where
        I: Instance + 'static,
        F: FnOnce(&Registries) -> I + Send + 'static,
    {
        self.start(world).await?.wait().await
    }

    /// Starts listening and returns at once, with the server running in the background.
    ///
    /// `world` builds the instance that players join. It runs on the instance's own thread,
    /// so the instance need not be `Send`, and gets the registries that are sent to the
    /// players.
    pub async fn start<I, F>(self, world: F) -> io::Result<RunningServer>
    where
        I: Instance + 'static,
        F: FnOnce(&Registries) -> I + Send + 'static,
    {
        let listener = TcpListener::bind(&self.address).await?;
        let addr = listener.local_addr()?;
        let registries = Arc::new(Registries::vanilla());
        let instance = {
            let registries = registries.clone();
            instance::spawn("world", SystemClock, move || world(&registries))?
        };
        let info = StatusInfo::new(self.motd);
        let task = tokio::spawn(serve(listener, Config::default(), {
            let instance = instance.clone();
            move |mut conn, intention| {
                let (registries, instance, info) =
                    (registries.clone(), instance.clone(), info.clone());
                async move {
                    tracing::debug!(?intention, "handshake");
                    if conn.state() == State::Status {
                        return status::respond(&mut conn, &info).await;
                    }
                    let profile = login::offline(&mut conn, Some(COMPRESSION_THRESHOLD)).await?;
                    configuration::run(&mut conn, &registries).await?;
                    play::run(conn, profile, instance).await
                }
            }
        }));
        tracing::info!(%addr, "listening");
        Ok(RunningServer {
            addr,
            instance,
            task,
        })
    }
}

/// A server that is listening. Dropping it leaves it running; call [`stop`](Self::stop).
#[derive(Debug)]
pub struct RunningServer {
    addr: SocketAddr,
    instance: InstanceHandle,
    task: JoinHandle<io::Result<()>>,
}

impl RunningServer {
    /// The address it listens on, with the real port if it was asked to pick one.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// A handle to the instance, to send it messages or to read its [`TickStats`] while the server
    /// runs somewhere else (for example in [`wait`](Self::wait)).
    pub fn instance(&self) -> InstanceHandle {
        self.instance.clone()
    }

    /// How the instance's tick loop has been doing since the server started.
    pub fn tick_stats(&self) -> TickStats {
        self.instance.tick_stats()
    }

    /// Stops listening and stops the instance, which ends every connection.
    pub fn stop(&self) {
        self.task.abort();
        self.instance.stop();
    }

    /// Waits until the server stops: [`stop`](Self::stop) was called, or listening failed.
    pub async fn wait(self) -> io::Result<()> {
        match self.task.await {
            Ok(result) => result,
            Err(e) if e.is_cancelled() => Ok(()),
            Err(e) => Err(io::Error::other(e)),
        }
    }
}
