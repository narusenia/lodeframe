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

use std::{
    io,
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};

use tokio::{net::TcpListener, task::JoinHandle};

use crate::{
    clock::SystemClock,
    configuration,
    instance::{self, Instance, InstanceHandle, MAX_BEHIND, Pacing, TICK, TickStats},
    login,
    net::{Config, serve},
    play::{self, KeepAliveConfig},
    protocol::{State, packets::configuration::Disconnect},
    registry::Registries,
    status::{self, StatusInfo},
    text::Component,
};

/// Bodies of at least this many bytes are compressed on the wire, unless
/// [`Server::compression_threshold`] says otherwise.
const COMPRESSION_THRESHOLD: usize = 256;

/// Ticks per second that [`Delay`](crate::schedule::Delay) counts in, whatever the tick rate
/// of the server.
const TICKS_PER_SECOND: u32 = 20;

/// How long [`RunningServer::shutdown`] waits, unless [`Server::shutdown_timeout`] says otherwise.
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);

/// What a server is called and where it listens. Start it with [`run`](Self::run) or
/// [`start`](Self::start).
#[derive(Debug, Clone)]
pub struct Server {
    address: String,
    motd: String,
    brand: String,
    compression_threshold: usize,
    keep_alive: KeepAliveConfig,
    known_packs_timeout: Duration,
    max_players: u32,
    tick_rate: u32,
    max_catch_up: Duration,
    nodelay: bool,
    handle_ctrl_c: bool,
    shutdown_timeout: Duration,
}

impl Server {
    /// A server that will listen on `address`, such as `127.0.0.1:25565`. Port 0 picks a free
    /// one; [`RunningServer::addr`] tells which.
    pub fn new(address: impl Into<String>) -> Self {
        Self {
            address: address.into(),
            motd: "lodeframe".into(),
            brand: "Lodeframe".into(),
            compression_threshold: COMPRESSION_THRESHOLD,
            keep_alive: KeepAliveConfig::default(),
            known_packs_timeout: configuration::KNOWN_PACKS_TIMEOUT,
            max_players: 20,
            tick_rate: TICKS_PER_SECOND,
            max_catch_up: MAX_BEHIND,
            nodelay: false,
            handle_ctrl_c: true,
            shutdown_timeout: SHUTDOWN_TIMEOUT,
        }
    }

    /// The line shown under the name in the server list.
    pub fn motd(mut self, motd: impl Into<String>) -> Self {
        self.motd = motd.into();
        self
    }

    /// The name of the server that the game shows next to it in the debug screen (F3). The default
    /// is `Lodeframe`.
    pub fn brand(mut self, brand: impl Into<String>) -> Self {
        self.brand = brand.into();
        self
    }

    /// Bodies of at least this many bytes are compressed on the wire. The default is 256.
    pub fn compression_threshold(mut self, bytes: usize) -> Self {
        self.compression_threshold = bytes;
        self
    }

    /// How often a keep alive goes out (default 15 seconds), and how long a client may leave one
    /// unanswered before it is disconnected (default 30 seconds). `timeout` must be longer
    /// than `interval`.
    ///
    /// The answers also give each player's latency, see [`Ctx::ping`](crate::world::Ctx::ping).
    pub fn keep_alive(mut self, interval: Duration, timeout: Duration) -> Self {
        self.keep_alive = KeepAliveConfig { interval, timeout };
        self
    }

    /// How long a client may take to answer the known packs while it joins. The default is 30
    /// seconds.
    pub fn known_packs_timeout(mut self, timeout: Duration) -> Self {
        self.known_packs_timeout = timeout;
        self
    }

    /// How many players may be on at once, shown in the server list. A player who would be one
    /// too many is told the server is full. The default is 20.
    pub fn max_players(mut self, max_players: u32) -> Self {
        self.max_players = max_players;
        self
    }

    /// Ticks per second. The default is 20, which is what the game expects; another rate
    /// speeds up or slows down everything the instance does per tick. A
    /// [`Delay`](crate::schedule::Delay) in seconds still counts 20 ticks to the second.
    pub fn tick_rate(mut self, ticks_per_second: u32) -> Self {
        self.tick_rate = ticks_per_second;
        self
    }

    /// How far behind the tick loop may fall and still run the late ticks back to back. Past
    /// it the loop starts over from now. The default is 2 seconds.
    pub fn max_catch_up(mut self, behind: Duration) -> Self {
        self.max_catch_up = behind;
        self
    }

    /// Whether to set `TCP_NODELAY` on every connection, which sends small packets at once
    /// instead of waiting to fill them. Off by default.
    pub fn nodelay(mut self, nodelay: bool) -> Self {
        self.nodelay = nodelay;
        self
    }

    /// Whether [`run`](Self::run) shuts the server down when it gets Ctrl-C. On by default; a second
    /// Ctrl-C ends it without waiting for the first shutdown to finish. [`start`](Self::start)
    /// never listens for it: whoever starts the server that way calls
    /// [`RunningServer::shutdown`] themselves.
    pub fn handle_ctrl_c(mut self, handle: bool) -> Self {
        self.handle_ctrl_c = handle;
        self
    }

    /// How long a shutdown waits for the instance to say goodbye and the players' connections to
    /// write what they were sent, before it gives up and cuts them. The default is 10 seconds.
    pub fn shutdown_timeout(mut self, timeout: Duration) -> Self {
        self.shutdown_timeout = timeout;
        self
    }

    /// Checks the settings that cannot work, before anything starts.
    fn validate(&self) -> io::Result<()> {
        let invalid =
            |what: &str| Err(io::Error::new(io::ErrorKind::InvalidInput, what.to_owned()));
        if self.tick_rate == 0 {
            return invalid("tick_rate must be at least 1");
        }
        if self.keep_alive.interval.is_zero() {
            return invalid("the keep alive interval must not be zero");
        }
        if self.keep_alive.timeout <= self.keep_alive.interval {
            return invalid("the keep alive timeout must be longer than its interval");
        }
        Ok(())
    }

    /// Starts the server and waits until it stops: listening failed, or Ctrl-C asked for a
    /// [`shutdown`](RunningServer::shutdown) (see [`handle_ctrl_c`](Self::handle_ctrl_c)).
    pub async fn run<I, F>(self, world: F) -> io::Result<()>
    where
        I: Instance + 'static,
        F: FnOnce(&Registries) -> I + Send + 'static,
    {
        let ctrl_c = self.handle_ctrl_c;
        let mut running = self.start(world).await?;
        if !ctrl_c {
            return running.wait().await;
        }
        tokio::select! {
            result = running.join() => return result,
            () = ctrl_c_signal() => {}
        }
        tracing::info!("Ctrl-C: shutting down, again to cut it short");
        tokio::select! {
            () = running.shutdown() => {}
            () = ctrl_c_signal() => {
                tracing::warn!("Ctrl-C again: not waiting");
                running.instance.stop();
            }
        }
        Ok(())
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
        self.validate()?;
        let listener = TcpListener::bind(&self.address).await?;
        let addr = listener.local_addr()?;
        let registries = Arc::new(Registries::vanilla());
        let pacing = Pacing {
            tick: Duration::from_secs(1) / self.tick_rate,
            max_behind: self.max_catch_up,
        };
        debug_assert!(self.tick_rate != TICKS_PER_SECOND || pacing.tick == TICK);
        let instance = {
            let registries = registries.clone();
            instance::spawn_paced(
                "world",
                SystemClock,
                tokio::runtime::Handle::current(),
                pacing,
                move || world(&registries),
            )?
        };
        let slots = Arc::new(Slots::new(self.max_players));
        let shutdown_timeout = self.shutdown_timeout;
        let options = Arc::new(self);
        let task = tokio::spawn(serve(
            listener,
            Config {
                nodelay: options.nodelay,
                ..Config::default()
            },
            {
                let instance = instance.clone();
                let slots = slots.clone();
                move |mut conn, intention| {
                    let (registries, instance, options, slots) = (
                        registries.clone(),
                        instance.clone(),
                        options.clone(),
                        slots.clone(),
                    );
                    async move {
                        tracing::debug!(?intention, "handshake");
                        if conn.state() == State::Status {
                            let mut info = StatusInfo::new(options.motd.clone());
                            info.online = slots.online();
                            info.max_players = options.max_players;
                            return status::respond(&mut conn, &info).await;
                        }
                        let profile =
                            login::offline(&mut conn, Some(options.compression_threshold)).await?;
                        let Some(_slot) = slots.take() else {
                            tracing::info!(name = %profile.name, "refused: the server is full");
                            conn.write_packet(&Disconnect {
                                reason: Component::text("The server is full"),
                            })
                            .await?;
                            return Ok(());
                        };
                        configuration::run_with(
                            &mut conn,
                            &registries,
                            &options.brand,
                            options.known_packs_timeout,
                        )
                        .await?;
                        play::run_with(conn, profile, instance, options.keep_alive).await
                    }
                }
            },
        ));
        tracing::info!(%addr, "listening");
        Ok(RunningServer {
            addr,
            instance,
            task,
            slots,
            shutdown_timeout,
        })
    }
}

/// A server that is listening. Dropping it leaves it running; call [`stop`](Self::stop).
#[derive(Debug)]
pub struct RunningServer {
    addr: SocketAddr,
    instance: InstanceHandle,
    task: JoinHandle<io::Result<()>>,
    slots: Arc<Slots>,
    shutdown_timeout: Duration,
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

    /// How many players are connected, counting those still joining.
    pub fn online(&self) -> u32 {
        self.slots.online()
    }

    /// How the instance's tick loop has been doing since the server started.
    pub fn tick_stats(&self) -> TickStats {
        self.instance.tick_stats()
    }

    /// Stops listening and asks the instance to shut down, which ends every connection. Returns
    /// at once; [`shutdown`](Self::shutdown) also waits for it to be done.
    ///
    /// The instance runs [`Instance::shutdown`] first: for a [`World`](crate::world::World)
    /// that is a [`ShutdownEvent`](crate::world::ShutdownEvent), then everyone is told why they
    /// are disconnected. To end it without that, use [`InstanceHandle::stop`] on
    /// [`instance`](Self::instance).
    pub fn stop(&self) {
        self.task.abort();
        self.instance.shutdown();
    }

    /// [`stop`](Self::stop), then waits until the instance is done and every connection has
    /// written what it was sent, for up to [`Server::shutdown_timeout`]. Past that the instance
    /// is stopped without waiting for its handlers.
    pub async fn shutdown(&self) {
        self.stop();
        let done = async {
            self.instance.stopped().await;
            while self.slots.online() > 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        };
        if tokio::time::timeout(self.shutdown_timeout, done)
            .await
            .is_err()
        {
            tracing::warn!(
                timeout = ?self.shutdown_timeout,
                "shutdown took too long, cutting what is left"
            );
            self.instance.stop();
        }
    }

    /// Waits until the server stops: [`stop`](Self::stop) was called, or listening failed.
    pub async fn wait(mut self) -> io::Result<()> {
        self.join().await
    }

    async fn join(&mut self) -> io::Result<()> {
        match (&mut self.task).await {
            Ok(result) => result,
            Err(e) if e.is_cancelled() => Ok(()),
            Err(e) => Err(io::Error::other(e)),
        }
    }
}

/// Waits for Ctrl-C. If the signal cannot be listened for, never comes: the server then runs
/// until it is stopped some other way.
async fn ctrl_c_signal() {
    if let Err(e) = tokio::signal::ctrl_c().await {
        tracing::warn!(error = %e, "cannot listen for Ctrl-C");
        std::future::pending::<()>().await;
    }
}

/// Counts the players on and keeps them under the maximum.
///
/// Only a number is shared, so that the server list can answer without waiting for the instance.
#[derive(Debug)]
struct Slots {
    online: AtomicU32,
    max: u32,
}

/// A place taken in [`Slots`], given back when dropped.
#[derive(Debug)]
struct Slot(Arc<Slots>);

impl Slots {
    fn new(max: u32) -> Self {
        Self {
            online: AtomicU32::new(0),
            max,
        }
    }

    fn online(&self) -> u32 {
        self.online.load(Ordering::Relaxed)
    }

    /// Takes a place if one is free. Counts and checks in one step, so that players arriving
    /// together cannot all pass the check.
    fn take(self: &Arc<Self>) -> Option<Slot> {
        self.online
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |online| {
                (online < self.max).then_some(online + 1)
            })
            .ok()
            .map(|_| Slot(self.clone()))
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.online.fetch_sub(1, Ordering::AcqRel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_server_takes_no_more_until_someone_leaves() {
        let slots = Arc::new(Slots::new(2));
        let a = slots.take().unwrap();
        let _b = slots.take().unwrap();

        assert!(slots.take().is_none());
        assert_eq!(slots.online(), 2);
        drop(a);
        assert_eq!(slots.online(), 1);
        assert!(slots.take().is_some());
    }

    #[test]
    fn a_server_with_no_places_takes_nobody() {
        assert!(Arc::new(Slots::new(0)).take().is_none());
    }

    #[test]
    fn settings_that_cannot_work_are_refused() {
        let kinds = |server: Server| server.validate().map_err(|e| e.kind());
        let s = || Server::new("127.0.0.1:0");

        assert_eq!(kinds(s()), Ok(()));
        assert_eq!(kinds(s().tick_rate(0)), Err(io::ErrorKind::InvalidInput));
        let secs = Duration::from_secs;
        assert_eq!(
            kinds(s().keep_alive(secs(0), secs(5))),
            Err(io::ErrorKind::InvalidInput)
        );
        assert_eq!(
            kinds(s().keep_alive(secs(10), secs(10))),
            Err(io::ErrorKind::InvalidInput)
        );
        assert_eq!(kinds(s().keep_alive(secs(10), secs(11))), Ok(()));
    }
}
