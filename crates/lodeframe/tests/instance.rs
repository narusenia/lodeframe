// SPDX-License-Identifier: Apache-2.0 OR MIT
//! The tick loop, the inbox and outbound channels, and the Play connection task.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use lodeframe::{
    clock::{Clock, SystemClock},
    instance::{self, Instance, Message, OUTBOX, Runner, Sessions, TICK, TickStats},
    login::Profile,
    net::Connection,
    play,
    protocol::{State, Uuid, packets::play::KeepAliveResponse},
};
use tokio::{io::duplex, sync::mpsc, time::timeout};

const T: Duration = Duration::from_secs(5);

/// A clock that only moves when told to, or when the loop sleeps.
#[derive(Clone)]
struct FakeClock {
    base: Instant,
    offset: Rc<Cell<Duration>>,
}

impl FakeClock {
    fn new() -> Self {
        Self {
            base: Instant::now(),
            offset: Rc::default(),
        }
    }

    fn elapsed(&self) -> Duration {
        self.offset.get()
    }

    fn advance(&self, d: Duration) {
        self.offset.set(self.offset.get() + d);
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Instant {
        self.base + self.offset.get()
    }

    fn sleep_until(&self, t: Instant) {
        self.offset.set(self.offset.get().max(t - self.base));
    }
}

/// Takes `costs[n]` of fake time on tick `n` (1 ms after that) and stops after the last.
struct Timed {
    clock: FakeClock,
    costs: Vec<Duration>,
    starts: Rc<RefCell<Vec<Duration>>>,
    stop: Arc<AtomicBool>,
}

impl Instance for Timed {
    fn handle(&mut self, _: Message) {}

    fn tick(&mut self) {
        let n = self.starts.borrow().len();
        self.starts.borrow_mut().push(self.clock.elapsed());
        self.clock.advance(
            self.costs
                .get(n)
                .copied()
                .unwrap_or(Duration::from_millis(1)),
        );
        if n + 1 >= self.costs.len() {
            self.stop.store(true, Ordering::Relaxed);
        }
    }
}

fn run_timed(costs: Vec<Duration>) -> Vec<Duration> {
    let clock = FakeClock::new();
    let starts = Rc::new(RefCell::new(Vec::new()));
    let stop = Arc::new(AtomicBool::new(false));
    let (mut runner, _handle) = Runner::new(Timed {
        clock: clock.clone(),
        costs,
        starts: starts.clone(),
        stop: stop.clone(),
    });
    runner.run(&clock, &stop);
    starts.take()
}

/// What the loop counted while ticks cost `costs`.
fn stats_of(costs: Vec<Duration>) -> TickStats {
    let clock = FakeClock::new();
    let stop = Arc::new(AtomicBool::new(false));
    let (mut runner, handle) = Runner::new(Timed {
        clock: clock.clone(),
        costs,
        starts: Rc::default(),
        stop: stop.clone(),
    });
    runner.run(&clock, &stop);
    handle.tick_stats()
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

#[test]
fn ticks_run_every_50_ms() {
    let starts = run_timed(vec![ms(1); 5]);
    assert_eq!(starts, [ms(0), ms(50), ms(100), ms(150), ms(200)]);
}

#[test]
fn a_slow_tick_is_followed_by_catching_up() {
    let mut costs = vec![ms(1); 20];
    costs[0] = ms(300);
    let starts = run_timed(costs);
    // the missed ticks run back to back, then the schedule is back on the 50 ms grid
    assert_eq!(starts[1], ms(300));
    assert!(starts[2] < ms(310));
    assert_eq!(*starts.last().unwrap(), ms(950));
}

#[test]
fn past_two_seconds_behind_the_loop_starts_over() {
    let mut costs = vec![ms(1); 8];
    costs[1] = ms(3000);
    let starts = run_timed(costs);
    // no burst of 60 missed ticks: after the skip they are 50 ms apart again
    // the slow tick starts at 50 ms and ends at 3050 ms; the loop restarts from there
    assert_eq!(starts[2], ms(3050));
    for w in starts[2..].windows(2) {
        assert_eq!(w[1] - w[0], TICK);
    }
}

#[test]
fn ticks_and_their_time_are_counted() {
    let stats = stats_of(vec![ms(10); 5]);

    assert_eq!(stats.ticks, 5);
    assert_eq!(stats.busy, ms(50));
    assert_eq!(stats.busy_max, ms(10));
    assert_eq!((stats.late, stats.skipped), (0, 0));
}

#[test]
fn a_slow_tick_makes_the_next_ones_late() {
    let mut costs = vec![ms(1); 20];
    costs[0] = ms(300);

    let stats = stats_of(costs);

    assert_eq!(stats.busy_max, ms(300));
    // 250 ms behind after the slow tick: the ticks that run back to back until caught up
    assert!(stats.late >= 2, "{stats:?}");
    assert_eq!(stats.skipped, 0);
}

#[test]
fn giving_up_on_catching_up_is_counted() {
    let mut costs = vec![ms(1); 8];
    costs[1] = ms(3000);

    let stats = stats_of(costs);

    assert_eq!(stats.busy_max, ms(3000));
    assert_eq!(stats.skipped, 1);
}

struct Counter {
    ticks: Arc<AtomicUsize>,
    _not_send: Rc<()>,
}

impl Instance for Counter {
    fn handle(&mut self, _: Message) {}

    fn tick(&mut self) {
        self.ticks.fetch_add(1, Ordering::Relaxed);
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Runtime::new().unwrap()
}

/// Remembers that it was attached, and from which thread.
struct Attached(Arc<std::sync::Mutex<Option<String>>>);

impl Instance for Attached {
    fn attach(&mut self, runtime: tokio::runtime::Handle) {
        // the handle works: work can be sent through it
        let task = runtime.spawn(async { 1 });
        drop(task);
        let name = std::thread::current().name().map(str::to_owned);
        *self.0.lock().unwrap() = name;
    }
    fn handle(&mut self, _: Message) {}
    fn tick(&mut self) {}
}

#[test]
fn the_instance_is_attached_to_the_runtime_on_its_own_thread_before_it_runs() {
    let seen = Arc::new(std::sync::Mutex::new(None));
    let s = seen.clone();
    let rt = runtime();
    let handle = instance::spawn("attach-test", SystemClock, rt.handle().clone(), move || {
        Attached(s)
    })
    .unwrap();
    // `spawn` returns once the instance exists; attaching happens before its first tick
    std::thread::sleep(ms(120));
    handle.stop();
    assert_eq!(seen.lock().unwrap().as_deref(), Some("attach-test"));
}

#[test]
fn a_thread_ticks_in_real_time_and_the_instance_need_not_be_send() {
    let ticks = Arc::new(AtomicUsize::new(0));
    let t = ticks.clone();
    // Counter holds an Rc, so this only compiles because the instance is built on its thread
    let handle = instance::spawn("test", SystemClock, runtime().handle().clone(), move || {
        Counter {
            ticks: t,
            _not_send: Rc::new(()),
        }
    })
    .unwrap();
    std::thread::sleep(ms(520));
    handle.stop();
    let n = ticks.load(Ordering::Relaxed);
    assert!((8..=13).contains(&n), "{n} ticks in 520 ms");
    std::thread::sleep(ms(120));
    let after = ticks.load(Ordering::Relaxed);
    std::thread::sleep(ms(120));
    assert_eq!(ticks.load(Ordering::Relaxed), after, "stopped");
}

#[test]
fn the_thread_ends_when_every_handle_is_dropped() {
    let ticks = Arc::new(AtomicUsize::new(0));
    let t = ticks.clone();
    let handle = instance::spawn("test", SystemClock, runtime().handle().clone(), move || {
        Counter {
            ticks: t,
            _not_send: Rc::new(()),
        }
    })
    .unwrap();
    drop(handle);
    std::thread::sleep(ms(150));
    let after = ticks.load(Ordering::Relaxed);
    std::thread::sleep(ms(150));
    assert_eq!(ticks.load(Ordering::Relaxed), after);
}

struct Null;

impl Instance for Null {
    fn handle(&mut self, _: Message) {}
    fn tick(&mut self) {}
}

#[tokio::test]
async fn a_full_inbox_holds_the_sender_until_the_instance_ticks() {
    let (mut runner, handle) = Runner::new(Null);
    let leave = || Message::Leave {
        player: Uuid(1),
        outbound: mpsc::channel(1).0.downgrade(),
    };
    // fill the inbox without ticking
    let mut sent = 0;
    while timeout(ms(20), handle.send(leave())).await.is_ok() {
        sent += 1;
    }
    assert!(sent >= 1000, "inbox held only {sent}");
    let waiting = tokio::spawn({
        let handle = handle.clone();
        async move { handle.send(leave()).await }
    });
    tokio::time::sleep(ms(20)).await;
    assert!(!waiting.is_finished());
    runner.step();
    assert!(timeout(T, waiting).await.unwrap().unwrap().is_ok());
}

#[tokio::test]
async fn a_player_who_stops_reading_is_dropped_and_the_others_carry_on() {
    let mut sessions = Sessions::default();
    let (slow, mut slow_rx) = mpsc::channel(OUTBOX);
    let (fast, mut fast_rx) = mpsc::channel(OUTBOX);
    sessions.join(Uuid(1), slow);
    sessions.join(Uuid(2), fast);
    for i in 0..OUTBOX {
        assert!(sessions.send(Uuid(1), vec![i as u8]));
        assert!(sessions.send(Uuid(2), vec![i as u8]));
        fast_rx.recv().await.unwrap(); // the fast one keeps up
    }
    assert!(
        !sessions.send(Uuid(1), vec![0]),
        "the slow player is cut off"
    );
    assert_eq!(sessions.len(), 1);
    assert!(sessions.send(Uuid(2), vec![9]));
    // the slow player's channel ends once what was queued is read
    for _ in 0..OUTBOX {
        slow_rx.recv().await.unwrap();
    }
    assert!(slow_rx.recv().await.is_none());
}

#[tokio::test]
async fn a_group_of_packets_takes_one_place_in_the_queue_and_arrives_in_order() {
    let mut sessions = Sessions::default();
    let (tx, mut rx) = mpsc::channel(OUTBOX);
    sessions.join(Uuid(1), tx);

    // OUTBOX messages of 100 packets each fit: the queue counts messages, not packets
    for round in 0..OUTBOX {
        let group: Vec<Vec<u8>> = (0..100).map(|i| vec![round as u8, i]).collect();
        assert!(sessions.send_all(Uuid(1), group));
    }
    let first = rx.recv().await.unwrap();
    assert_eq!(first.len(), 100);
    assert_eq!(first[0], [0, 0]);
    assert_eq!(first[99], [0, 99]);

    // one place is free now; the next message after it is the one that overflows
    assert!(sessions.send_all(Uuid(1), vec![vec![1]; 100]));
    assert!(!sessions.send_all(Uuid(1), vec![vec![1]]));
    assert_eq!(sessions.len(), 0);
}

#[tokio::test]
async fn only_the_connection_a_player_is_on_now_is_current() {
    let mut sessions = Sessions::default();
    let (first, _first_rx) = mpsc::channel(OUTBOX);
    let first_ended = first.downgrade();
    sessions.join(Uuid(1), first);
    assert!(sessions.is_current(Uuid(1), &first_ended));

    // the player logs in again: the first connection is not theirs any more
    let (second, _second_rx) = mpsc::channel(OUTBOX);
    let second_ended = second.downgrade();
    sessions.join(Uuid(1), second);
    assert!(!sessions.is_current(Uuid(1), &first_ended));
    assert!(sessions.is_current(Uuid(1), &second_ended));

    // nor after they were dropped, or for a player who is not here
    sessions.leave(Uuid(1));
    assert!(!sessions.is_current(Uuid(1), &second_ended));
    assert!(!sessions.is_current(Uuid(2), &second_ended));
}

/// Records what reaches it and answers every packet with its own body.
struct Echo {
    sessions: Sessions,
    log: Arc<std::sync::Mutex<Vec<String>>>,
}

impl Instance for Echo {
    fn handle(&mut self, message: Message) {
        match message {
            Message::Join { profile, outbound } => {
                self.log
                    .lock()
                    .unwrap()
                    .push(format!("join {}", profile.name));
                self.sessions.join(profile.uuid, outbound);
            }
            Message::Packet { player, body } => {
                self.log.lock().unwrap().push(format!("packet {body:?}"));
                self.sessions.send(player, body);
            }
            Message::Leave { player, .. } => {
                self.log.lock().unwrap().push("leave".into());
                self.sessions.leave(player);
            }
        }
    }

    fn tick(&mut self) {}
}

#[tokio::test]
async fn play_relays_packets_both_ways_and_keeps_keepalives_to_itself() {
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let (mut runner, handle) = Runner::new(Echo {
        sessions: Sessions::default(),
        log: log.clone(),
    });
    let (a, b) = duplex(1 << 16);
    let mut server = Connection::new(a, T);
    server.set_state(State::Play);
    let mut client = Connection::new(b, T);
    let profile = Profile {
        uuid: Uuid(7),
        name: "Steve".into(),
    };
    let task = tokio::spawn(play::run(server, profile, handle));

    client
        .write_packet(&KeepAliveResponse { id: 1 })
        .await
        .unwrap();
    client.write_frame(&[0x05, 1, 2]).await.unwrap();
    // let the connection task deliver both, then tick
    for _ in 0..50 {
        tokio::time::sleep(ms(10)).await;
        runner.step();
        if log.lock().unwrap().len() >= 2 {
            break;
        }
    }
    assert_eq!(*log.lock().unwrap(), ["join Steve", "packet [5, 1, 2]"]);
    // the echo comes back to the client
    assert_eq!(client.read_frame().await.unwrap(), [0x05, 1, 2]);

    drop(client);
    assert!(timeout(T, task).await.unwrap().unwrap().is_err());
    runner.step();
    assert_eq!(log.lock().unwrap().last().unwrap(), "leave");
}
