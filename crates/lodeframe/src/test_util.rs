// SPDX-License-Identifier: Apache-2.0 OR MIT
//! A headless harness for testing code built on lodeframe, behind the `test-util` feature.
//!
//! [`TestEnv`] runs an [`Instance`] with no sockets, threads or clock: [`tick`](TestEnv::tick)
//! advances it by hand, and a [`FakePlayer`] stands in for a connection, taking packets from
//! the test and recording what the instance sends back. The same test gives the same result
//! every time.
//!
//! ```
//! use lodeframe::{
//!     event::{Event, EventNode},
//!     instance::{Instance, Message, Sessions},
//!     test_util::{Recorder, TestEnv},
//! };
//!
//! #[derive(Clone, Debug, PartialEq)]
//! struct Greeted(String);
//! impl Event for Greeted {}
//!
//! /// An instance that emits `Greeted` for each player who joins.
//! struct Lobby {
//!     sessions: Sessions,
//!     events: EventNode<Sessions>,
//! }
//!
//! impl Instance for Lobby {
//!     fn handle(&mut self, message: Message) {
//!         if let Message::Join { profile, outbound } = message {
//!             self.sessions.join(profile.uuid, outbound);
//!             self.events.emit(&mut Greeted(profile.name), &mut self.sessions);
//!         }
//!     }
//!     fn tick(&mut self) {}
//! }
//!
//! let mut events = EventNode::new();
//! let greeted = Recorder::<Greeted>::attach(&mut events);
//! let mut env = TestEnv::new(Lobby { sessions: Sessions::default(), events });
//!
//! env.connect("Steve");
//! env.tick(1);
//! assert_eq!(greeted.take(), [Greeted("Steve".into())]);
//! ```

use std::{cell::RefCell, rc::Rc};

use tokio::sync::mpsc;

use crate::{
    event::{Event, EventNode},
    instance::{Instance, Message, Packets},
    login::Profile,
    protocol::{Decode, Encode, Packet, Result, Uuid, packet_body, split_packet_id},
};

/// Packets a [`FakePlayer`] can hold before the instance drops them, as a real connection would
/// be dropped for not reading. Far above what one test sends.
const OUTBOX: usize = 1 << 16;

/// An [`Instance`] driven by hand.
///
/// Async work (`ctx.spawn`) runs on a runtime that only moves inside
/// [`run_until_idle`](Self::run_until_idle), with its clock paused: a future that sleeps for an
/// hour finishes at once, in the order of its timers. Drop the env outside an async context.
pub struct TestEnv<I> {
    instance: I,
    runtime: tokio::runtime::Runtime,
}

impl<I: Instance> TestEnv<I> {
    /// Wraps `instance`.
    pub fn new(mut instance: I) -> Self {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .start_paused(true)
            .build()
            .expect("a current-thread runtime can be built");
        instance.attach(runtime.handle().clone());
        Self { instance, runtime }
    }

    /// The instance, to look at its state.
    pub fn instance(&self) -> &I {
        &self.instance
    }

    /// The instance, to change its state.
    pub fn instance_mut(&mut self) -> &mut I {
        &mut self.instance
    }

    /// Runs `n` ticks.
    pub fn tick(&mut self, n: u32) {
        for _ in 0..n {
            self.instance.tick();
        }
    }

    /// Runs the async work the instance has started until all of it is done, without running
    /// any callback: those wait for the next [`tick`](Self::tick), as they do in a real server.
    /// So a test of `ctx.spawn` is `env.run_until_idle(); env.tick(1);`.
    ///
    /// # Panics
    ///
    /// If work is still going after 5 seconds of real time, for example a future that waits for
    /// something that never comes.
    pub fn run_until_idle(&mut self) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let alive = self.runtime.metrics().num_alive_tasks();
            if alive == 0 {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "{alive} async task(s) still running after 5 s"
            );
            // the clock is paused, so this waits for the earliest timer of any task, then returns
            self.runtime.block_on(async {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            });
        }
    }

    /// Joins a player called `name`. The instance has handled the join when this returns.
    pub fn connect(&mut self, name: &str) -> FakePlayer {
        let uuid = Uuid::offline(name);
        let (outbound, inbox) = mpsc::channel(OUTBOX);
        let connection = outbound.downgrade();
        self.instance.handle(Message::Join {
            profile: Profile {
                uuid,
                name: name.into(),
            },
            outbound,
        });
        FakePlayer {
            uuid,
            inbox,
            connection,
        }
    }

    /// Sends `packet` as `player`. The instance has handled it when this returns.
    pub fn send<P: Packet + Encode>(&mut self, player: &FakePlayer, packet: &P) {
        let body = packet_body(packet).expect("the packet encodes");
        self.send_raw(player, body);
    }

    /// Sends a packet body (id followed by payload) as `player`, for bytes a packet type would
    /// not produce.
    pub fn send_raw(&mut self, player: &FakePlayer, body: Vec<u8>) {
        self.instance.handle(Message::Packet {
            player: player.uuid,
            body,
        });
    }

    /// Ends `player`'s connection.
    pub fn disconnect(&mut self, player: FakePlayer) {
        self.instance.handle(Message::Leave {
            player: player.uuid,
            outbound: player.connection,
        });
    }
}

/// A player with no client behind them: what the instance sends them is kept for the test.
#[derive(Debug)]
pub struct FakePlayer {
    uuid: Uuid,
    inbox: mpsc::Receiver<Packets>,
    connection: mpsc::WeakSender<Packets>,
}

impl FakePlayer {
    /// The player's UUID.
    pub fn uuid(&self) -> Uuid {
        self.uuid
    }

    /// Everything sent since the last call, oldest first.
    pub fn drain(&mut self) -> Vec<Received> {
        let mut out = Vec::new();
        while let Ok(packets) = self.inbox.try_recv() {
            out.extend(packets.into_iter().map(Received::new));
        }
        out
    }

    /// Like [`drain`](Self::drain), keeping the messages apart: the packets the instance sent in
    /// one go (for instance all the moves of a tick) come together, oldest message first.
    pub fn drain_messages(&mut self) -> Vec<Vec<Received>> {
        let mut out = Vec::new();
        while let Ok(packets) = self.inbox.try_recv() {
            out.push(packets.into_iter().map(Received::new).collect());
        }
        out
    }

    /// Like [`drain`](Self::drain), keeping only the packets that are a `P`.
    pub fn drain_as<P: Packet + Decode>(&mut self) -> Vec<P> {
        self.drain()
            .iter()
            .filter(|r| r.is::<P>())
            .map(|r| r.decode().expect("a packet with this id decodes"))
            .collect()
    }

    /// Whether the instance has dropped this player, ending their connection. Packets sent
    /// before that can still be drained.
    pub fn is_disconnected(&mut self) -> bool {
        matches!(
            self.inbox.try_recv(),
            Err(mpsc::error::TryRecvError::Disconnected)
        )
    }
}

/// One packet a [`FakePlayer`] was sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Received {
    /// The packet id.
    pub id: i32,
    payload: Vec<u8>,
}

impl Received {
    fn new(body: Vec<u8>) -> Self {
        let (id, payload) = split_packet_id(&body).expect("the instance sends valid packets");
        Self {
            id,
            payload: payload.to_vec(),
        }
    }

    /// The bytes after the packet id, for packets that cannot be decoded.
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    /// Whether this is a `P`, judging by the id.
    pub fn is<P: Packet>(&self) -> bool {
        self.id == P::ID
    }

    /// Decodes the packet as a `P`.
    pub fn decode<P: Packet + Decode>(&self) -> Result<P> {
        P::decode(&mut self.payload.as_slice())
    }
}

/// Keeps a copy of every `E` that reaches the node it was attached to.
///
/// The copy is taken when the recorder's own handler runs, so it shows the event as earlier
/// handlers of that node left it and does not see later changes.
pub struct Recorder<E> {
    seen: Rc<RefCell<Vec<E>>>,
}

impl<E: Event + Clone> Recorder<E> {
    /// Adds a recording handler for `E` to `node`.
    pub fn attach<C: 'static>(node: &mut EventNode<C>) -> Self {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let sink = seen.clone();
        node.on(move |event: &mut E, _: &mut C| sink.borrow_mut().push(event.clone()));
        Self { seen }
    }

    /// The events seen since the last call, oldest first.
    pub fn take(&self) -> Vec<E> {
        std::mem::take(&mut self.seen.borrow_mut())
    }
}
