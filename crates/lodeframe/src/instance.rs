// SPDX-License-Identifier: Apache-2.0 OR MIT
//! An instance owned by one thread and ticked 20 times a second.
//!
//! The instance is built *inside* its thread, so it never has to be `Send`, and the only
//! thing that leaves the thread is an [`InstanceHandle`]. The rest of the program can
//! therefore reach an instance only by sending it [`Message`]s.

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::Duration,
};

use tokio::sync::mpsc::{self, error::TryRecvError};

use crate::{clock::Clock, login::Profile, protocol::Uuid};

/// One tick: 20 ticks per second.
pub const TICK: Duration = Duration::from_millis(50);
/// Past this much lag the loop stops catching up and starts over from now.
pub const MAX_BEHIND: Duration = Duration::from_secs(2);
/// Messages waiting for an instance before senders have to wait.
const INBOX: usize = 4096;
/// Messages of [`Packets`] waiting for one client before it is cut off.
pub const OUTBOX: usize = 256;

/// Packet bodies (id followed by payload) for one connection, to be written in this order.
///
/// An instance sends a connection one of these at a time. A group counts as one message in
/// the connection's queue of [`OUTBOX`], however many packets are in it.
pub type Packets = Vec<Vec<u8>>;

/// Something that can be ticked. Implement this for your world.
pub trait Instance {
    /// Handles one message from a connection. Called for all waiting messages before each tick.
    fn handle(&mut self, message: Message);
    /// Advances the world by one tick.
    fn tick(&mut self);
}

/// What connections tell an instance.
#[derive(Debug)]
pub enum Message {
    /// A player finished configuration. Packets for them go to `outbound`.
    Join {
        /// Who joined.
        profile: Profile,
        /// Bodies (packet id + payload) for the client; see [`Sessions`].
        outbound: mpsc::Sender<Packets>,
    },
    /// A packet from a player: id and payload.
    Packet {
        /// Who sent it.
        player: Uuid,
        /// Packet id followed by the payload.
        body: Vec<u8>,
    },
    /// A connection of the player ended.
    Leave {
        /// Who left.
        player: Uuid,
        /// The channel of the connection that ended, as a weak reference so that holding it does
        /// not keep the connection open. A player can log in again while the old connection is
        /// still going, and the end of the old one must not remove the new one:
        /// [`Sessions::is_current`] tells which it was.
        outbound: mpsc::WeakSender<Packets>,
    },
}

/// Outgoing channels of the players in one instance.
///
/// A client that stops reading fills its channel; it is then dropped, which ends its
/// connection, instead of holding up the tick loop.
#[derive(Debug, Default)]
pub struct Sessions {
    outbound: HashMap<Uuid, mpsc::Sender<Packets>>,
}

impl Sessions {
    /// Registers a player.
    pub fn join(&mut self, player: Uuid, outbound: mpsc::Sender<Packets>) {
        self.outbound.insert(player, outbound);
    }

    /// Whether `connection`, from a [`Message::Leave`], is the one `player` is on now. It is not
    /// if the player logged in again since, or if the player was already dropped.
    pub fn is_current(&self, player: Uuid, connection: &mpsc::WeakSender<Packets>) -> bool {
        match (self.outbound.get(&player), connection.upgrade()) {
            (Some(current), Some(ended)) => current.same_channel(&ended),
            _ => false,
        }
    }

    /// Forgets a player, closing their channel.
    pub fn leave(&mut self, player: Uuid) {
        self.outbound.remove(&player);
    }

    /// Sends `body` to `player`. Returns `false`, having dropped the player, if their
    /// channel is full or closed.
    pub fn send(&mut self, player: Uuid, body: Vec<u8>) -> bool {
        self.send_all(player, vec![body])
    }

    /// Sends `packets` to `player` as one message. Returns `false`, having dropped the player,
    /// if their channel is full or closed.
    pub fn send_all(&mut self, player: Uuid, packets: Packets) -> bool {
        let Some(tx) = self.outbound.get(&player) else {
            return false;
        };
        if tx.try_send(packets).is_ok() {
            return true;
        }
        tracing::warn!(%player, "dropping player: outbound channel full or closed");
        self.outbound.remove(&player);
        false
    }

    /// Number of players.
    pub fn len(&self) -> usize {
        self.outbound.len()
    }

    /// Whether nobody is here.
    pub fn is_empty(&self) -> bool {
        self.outbound.is_empty()
    }
}

/// Drives an [`Instance`]: drains the inbox, then ticks.
pub struct Runner<I> {
    instance: I,
    inbox: mpsc::Receiver<Message>,
    metrics: Arc<TickMetrics>,
}

impl<I: Instance> Runner<I> {
    /// Wraps `instance` with an inbox. Also returns the handle to send to it.
    pub fn new(instance: I) -> (Self, InstanceHandle) {
        let (tx, inbox) = mpsc::channel(INBOX);
        let metrics = Arc::new(TickMetrics::default());
        let handle = InstanceHandle {
            inbox: tx,
            stop: Arc::new(AtomicBool::new(false)),
            metrics: metrics.clone(),
        };
        (
            Self {
                instance,
                inbox,
                metrics,
            },
            handle,
        )
    }

    /// One step: hands every waiting message to the instance, then ticks it.
    ///
    /// Returns `false` once every handle is gone and nothing is left to do.
    /// This is what tests call instead of [`run`](Self::run).
    pub fn step(&mut self) -> bool {
        loop {
            match self.inbox.try_recv() {
                Ok(m) => self.instance.handle(m),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return false,
            }
        }
        self.instance.tick();
        true
    }

    fn record(&self, busy: Duration) {
        let ns = u64::try_from(busy.as_nanos()).unwrap_or(u64::MAX);
        self.metrics.ticks.fetch_add(1, Ordering::Relaxed);
        self.metrics.busy_ns.fetch_add(ns, Ordering::Relaxed);
        self.metrics.busy_max_ns.fetch_max(ns, Ordering::Relaxed);
    }

    /// Ticks every [`TICK`] until `stop` is set or all handles are gone.
    ///
    /// A tick that runs late is followed by ticks back to back until the loop has caught up.
    /// Past [`MAX_BEHIND`] it gives up catching up and starts over from now.
    pub fn run(&mut self, clock: &impl Clock, stop: &AtomicBool) {
        let mut next = clock.now();
        let mut reported = false;
        while !stop.load(Ordering::Relaxed) {
            let now = clock.now();
            if now < next {
                clock.sleep_until(next);
                continue;
            }
            let behind = now - next;
            if behind > MAX_BEHIND {
                tracing::warn!(
                    behind_ms = behind.as_millis() as u64,
                    "can't keep up, skipping ticks"
                );
                next = now;
                reported = true;
                self.metrics.skipped.fetch_add(1, Ordering::Relaxed);
                self.metrics.late.fetch_add(1, Ordering::Relaxed);
            } else if behind >= TICK {
                self.metrics.late.fetch_add(1, Ordering::Relaxed);
                // one warning per stretch of lag; the rest of the stretch is debug
                if reported {
                    tracing::debug!(behind_ms = behind.as_millis() as u64, "tick behind");
                } else {
                    tracing::warn!(
                        behind_ms = behind.as_millis() as u64,
                        "tick behind, catching up"
                    );
                    reported = true;
                }
            } else {
                reported = false;
            }
            let started = clock.now();
            if !self.step() {
                break;
            }
            self.record(clock.now().saturating_duration_since(started));
            next += TICK;
        }
    }
}

/// A way to send messages to an instance from anywhere. Cloning is cheap.
#[derive(Debug, Clone)]
pub struct InstanceHandle {
    inbox: mpsc::Sender<Message>,
    stop: Arc<AtomicBool>,
    metrics: Arc<TickMetrics>,
}

/// How the tick loop has been doing since it started, from [`InstanceHandle::tick_stats`].
///
/// The numbers only grow. To see a stretch of time, take two samples and subtract; for the
/// ticks per second, divide the ticks between them by the seconds between them. The maximum is
/// the largest since the start and cannot be subtracted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TickStats {
    /// Ticks run.
    pub ticks: u64,
    /// Time spent inside ticks, handling messages and ticking the instance.
    pub busy: Duration,
    /// The longest a single tick took.
    pub busy_max: Duration,
    /// Ticks that started at least one tick (50 ms) after they were due.
    pub late: u64,
    /// Times the loop was so far behind ([`MAX_BEHIND`]) that it gave up catching up.
    pub skipped: u64,
}

#[derive(Debug, Default)]
struct TickMetrics {
    ticks: AtomicU64,
    busy_ns: AtomicU64,
    busy_max_ns: AtomicU64,
    late: AtomicU64,
    skipped: AtomicU64,
}

impl InstanceHandle {
    /// Sends `message`, waiting while the instance's inbox is full. That wait is what stops a
    /// connection from reading more from its socket than the instance can take.
    ///
    /// Fails if the instance has stopped.
    pub async fn send(&self, message: Message) -> Result<(), Stopped> {
        self.inbox.send(message).await.map_err(|_| Stopped)
    }

    /// Asks the instance thread to stop after its current tick.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// How the tick loop has been doing, see [`TickStats`]. Only [`Runner::run`] counts; a
    /// runner driven by hand through [`Runner::step`] does not.
    pub fn tick_stats(&self) -> TickStats {
        let m = &self.metrics;
        TickStats {
            ticks: m.ticks.load(Ordering::Relaxed),
            busy: Duration::from_nanos(m.busy_ns.load(Ordering::Relaxed)),
            busy_max: Duration::from_nanos(m.busy_max_ns.load(Ordering::Relaxed)),
            late: m.late.load(Ordering::Relaxed),
            skipped: m.skipped.load(Ordering::Relaxed),
        }
    }
}

/// The instance has stopped and takes no more messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stopped;

impl std::fmt::Display for Stopped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("instance has stopped")
    }
}

impl std::error::Error for Stopped {}

/// Starts a thread, builds the instance on it with `factory`, and ticks it on `clock`.
///
/// The thread ends when [`InstanceHandle::stop`] is called or every handle is dropped.
///
/// State cannot be smuggled in from outside the thread unless it is `Send`; the instance
/// itself need not be. (This only checks that it fails to compile, not why.)
///
/// ```compile_fail
/// use std::rc::Rc;
/// use lodeframe::{clock::SystemClock, instance::{spawn, Instance, Message}};
///
/// struct Holds(Rc<()>);
/// impl Instance for Holds {
///     fn handle(&mut self, _: Message) {}
///     fn tick(&mut self) {}
/// }
///
/// let shared = Rc::new(());
/// // `shared` is created outside the thread and moved into it: not `Send`.
/// let _ = spawn("x", SystemClock, move || Holds(shared));
/// ```
pub fn spawn<I, F, C>(name: &str, clock: C, factory: F) -> std::io::Result<InstanceHandle>
where
    I: Instance + 'static,
    F: FnOnce() -> I + Send + 'static,
    C: Clock + Send + 'static,
{
    // Runner::new needs the instance, which only exists on the new thread; the handle is
    // sent back so the caller gets it before the first tick.
    let (tx, rx) = std::sync::mpsc::channel();
    let thread_name = name.to_owned();
    thread::Builder::new()
        .name(thread_name.clone())
        .spawn(move || {
            let (mut runner, handle) = Runner::new(factory());
            let stop = handle.stop.clone();
            if tx.send(handle).is_err() {
                return;
            }
            tracing::info!(instance = %thread_name, "instance started");
            runner.run(&clock, &stop);
            tracing::info!(instance = %thread_name, "instance stopped");
        })?;
    rx.recv()
        .map_err(|_| std::io::Error::other("instance thread failed to start"))
}
