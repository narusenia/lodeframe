// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Tasks that run later, or again and again, on the tick loop.
//!
//! [`Ctx::after`] says when the first run is and how it is set up; [`Schedule::run`] takes the
//! task, which says by its return value whether and when to run again. [`once`](Schedule::once)
//! and [`every`](Schedule::every) are shorter ways to write the common cases.
//!
//! Time is counted in ticks, not read from a clock, so `env.tick(n)` in a test moves it exactly.
//! A task gets the [`Ctx`] like any handler; what it emits or changes in the event tree is done
//! at the end of the same tick.

use std::{
    collections::{BTreeMap, HashMap},
    time::Duration,
};

use crate::world::{Ctx, PlayerId};

/// How long one tick is.
pub const TICK: Duration = Duration::from_millis(50);

const TICKS_PER_SECOND: u64 = 20;

/// How long to wait, in ticks. Zero and one both mean the next tick: a task never runs in the
/// tick that scheduled it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Delay(u64);

impl Delay {
    /// The next tick.
    pub const ZERO: Delay = Delay(0);

    /// `n` ticks.
    pub const fn ticks(n: u64) -> Self {
        Self(n)
    }

    /// `n` seconds of 20 ticks each.
    pub const fn secs(n: u64) -> Self {
        Self(n.saturating_mul(TICKS_PER_SECOND))
    }

    /// The number of ticks.
    pub const fn as_ticks(self) -> u64 {
        self.0
    }
}

/// Rounds up to whole ticks, so a task never runs earlier than asked.
impl From<Duration> for Delay {
    fn from(d: Duration) -> Self {
        let ticks = d.as_nanos().div_ceil(TICK.as_nanos());
        Self(u64::try_from(ticks).unwrap_or(u64::MAX))
    }
}

/// A point in time: how many ticks a world has run. See [`Ctx::now`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Tick(u64);

impl Tick {
    /// The number of ticks.
    pub const fn as_ticks(self) -> u64 {
        self.0
    }
}

impl std::ops::Add<Delay> for Tick {
    type Output = Tick;

    fn add(self, delay: Delay) -> Tick {
        Tick(self.0.saturating_add(delay.0))
    }
}

/// What a task wants after it ran.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Next {
    /// Do not run again.
    Stop,
    /// Run again after this long, counted from this run.
    After(Delay),
}

/// Names a scheduled task, to [`cancel`](Ctx::cancel) it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TaskId(u64);

/// Who a task belongs to; it stops with its owner.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Owner {
    /// The world: stops when the world is dropped.
    World,
    /// One player's stay: stops when the player leaves.
    Player(PlayerId),
}

/// Which part of the tick a task runs in.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    Start,
    End,
}

type Run = Box<dyn FnMut(&mut Ctx) -> Next>;

struct Task {
    owner: Owner,
    run: Run,
}

/// The tasks of one [`Ctx`], by the tick they are due and then by when they were made.
#[derive(Default)]
pub(crate) struct Scheduler {
    // the number of the tick that is running, or ran last
    now: u64,
    next: u64,
    start: BTreeMap<(u64, u64), Task>,
    end: BTreeMap<(u64, u64), Task>,
    // where each waiting task is, to cancel it without a search
    waiting: HashMap<u64, (Phase, u64)>,
    // the task that is running: it is in neither queue
    running: Option<u64>,
    cancelled_running: bool,
}

impl Scheduler {
    fn queue(&mut self, phase: Phase) -> &mut BTreeMap<(u64, u64), Task> {
        match phase {
            Phase::Start => &mut self.start,
            Phase::End => &mut self.end,
        }
    }

    fn insert(&mut self, id: u64, phase: Phase, delay: Delay, task: Task) {
        // never the tick that is running: a task that asks for zero must not loop in it
        let due = self.now.saturating_add(delay.0.max(1));
        self.queue(phase).insert((due, id), task);
        self.waiting.insert(id, (phase, due));
    }

    /// The next task of `phase` that is due, taken out of its queue.
    fn pop_due(&mut self, phase: Phase) -> Option<(u64, Task)> {
        let now = self.now;
        let queue = self.queue(phase);
        let entry = queue.first_entry().filter(|e| e.key().0 <= now)?;
        let ((_, id), task) = entry.remove_entry();
        self.waiting.remove(&id);
        Some((id, task))
    }

    fn cancel(&mut self, id: u64) -> bool {
        if self.running == Some(id) {
            self.cancelled_running = true;
            return true;
        }
        let Some((phase, due)) = self.waiting.remove(&id) else {
            return false;
        };
        self.queue(phase).remove(&(due, id)).is_some()
    }

    fn cancel_player(&mut self, player: PlayerId) {
        for queue in [&mut self.start, &mut self.end] {
            queue.retain(|(_, id), task| {
                let keep = task.owner != Owner::Player(player);
                if !keep {
                    self.waiting.remove(id);
                }
                keep
            });
        }
    }
}

/// A task that is not set up yet. Finish it with [`run`](Self::run), [`once`](Self::once) or
/// [`every`](Self::every).
#[must_use = "a task is only scheduled by `run`, `once` or `every`"]
pub struct Schedule<'a> {
    ctx: &'a mut Ctx,
    delay: Delay,
    owner: Owner,
    phase: Phase,
}

impl Schedule<'_> {
    /// Makes the task belong to `player`: it stops when they leave, and a player who joins under
    /// the same name later does not inherit it. If the player is already gone, nothing is
    /// scheduled.
    pub fn for_player(mut self, player: PlayerId) -> Self {
        self.owner = Owner::Player(player);
        self
    }

    /// Runs the task at the end of its tick, after the players were sent what the tick had for
    /// them, instead of at the start.
    pub fn at_end(mut self) -> Self {
        self.phase = Phase::End;
        self
    }

    /// Schedules `task`. It runs once the delay is over, and again after whatever it returns.
    ///
    /// ```
    /// # use lodeframe::{schedule::{Delay, Next}, world::Ctx};
    /// # fn countdown(ctx: &mut Ctx) {
    /// let mut left = 3;
    /// ctx.after(Delay::secs(1)).run(move |_ctx| {
    ///     left -= 1;
    ///     if left == 0 { Next::Stop } else { Next::After(Delay::secs(1)) }
    /// });
    /// # }
    /// ```
    pub fn run(self, task: impl FnMut(&mut Ctx) -> Next + 'static) -> TaskId {
        let scheduler = &mut self.ctx.scheduler;
        let id = scheduler.next;
        scheduler.next += 1;
        let gone = matches!(self.owner, Owner::Player(p) if !self.ctx.is_online(p));
        if !gone {
            self.ctx.scheduler.insert(
                id,
                self.phase,
                self.delay,
                Task {
                    owner: self.owner,
                    run: Box::new(task),
                },
            );
        }
        TaskId(id)
    }

    /// Runs `task` once.
    pub fn once(self, task: impl FnOnce(&mut Ctx) + 'static) -> TaskId {
        let mut task = Some(task);
        self.run(move |ctx| {
            if let Some(task) = task.take() {
                task(ctx);
            }
            Next::Stop
        })
    }

    /// Runs `task` again and again, `period` after each run, until it is cancelled or its owner
    /// is gone.
    pub fn every(
        self,
        period: impl Into<Delay>,
        mut task: impl FnMut(&mut Ctx) + 'static,
    ) -> TaskId {
        let period = period.into();
        self.run(move |ctx| {
            task(ctx);
            Next::After(period)
        })
    }
}

impl Ctx {
    /// Starts setting up a task that first runs `delay` from now. A task never runs in the tick
    /// that scheduled it, however short the delay.
    ///
    /// It runs on the world's own tick loop, with this `Ctx`, in the order of its due tick and
    /// then of when it was scheduled. It stops when [`cancel`](Self::cancel)led, when it returns
    /// [`Next::Stop`], and when its owner (the world, or the player given to
    /// [`for_player`](Schedule::for_player)) is gone.
    pub fn after(&mut self, delay: impl Into<Delay>) -> Schedule<'_> {
        Schedule {
            ctx: self,
            delay: delay.into(),
            owner: Owner::World,
            phase: Phase::Start,
        }
    }

    /// The tick that is running or ran last: 0 until the world has ticked, then 1 more with each
    /// tick. Counted in ticks, not read from a clock, so `env.tick(n)` moves it by `n`.
    pub fn now(&self) -> Tick {
        Tick(self.scheduler.now)
    }

    /// Stops a task that has not run again yet. Returns whether there was such a task; a task
    /// may cancel itself, and then does not run again whatever it returns.
    pub fn cancel(&mut self, task: TaskId) -> bool {
        self.scheduler.cancel(task.0)
    }

    /// Stops the tasks that belong to `player`. Called when they leave.
    pub(crate) fn cancel_tasks_of(&mut self, player: PlayerId) {
        self.scheduler.cancel_player(player);
    }

    /// Runs the tasks of `phase` that are due. The start of a tick is what moves time on.
    pub(crate) fn run_tasks(&mut self, phase: Phase) {
        if phase == Phase::Start {
            self.scheduler.now += 1;
        }
        while let Some((id, mut task)) = self.scheduler.pop_due(phase) {
            self.scheduler.running = Some(id);
            self.scheduler.cancelled_running = false;
            let next = (task.run)(self);
            self.scheduler.running = None;
            let alive = match task.owner {
                Owner::World => true,
                Owner::Player(player) => self.is_online(player),
            };
            if let Next::After(delay) = next
                && alive
                && !self.scheduler.cancelled_running
            {
                self.scheduler.insert(id, phase, delay, task);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_duration_rounds_up_to_whole_ticks() {
        assert_eq!(Delay::from(Duration::ZERO), Delay::ZERO);
        assert_eq!(Delay::from(Duration::from_millis(50)), Delay::ticks(1));
        assert_eq!(Delay::from(Duration::from_millis(51)), Delay::ticks(2));
        assert_eq!(Delay::from(Duration::from_secs(1)), Delay::secs(1));
    }

    #[test]
    fn seconds_are_twenty_ticks_and_do_not_overflow() {
        assert_eq!(Delay::secs(3).as_ticks(), 60);
        assert_eq!(Delay::secs(u64::MAX).as_ticks(), u64::MAX);
    }
}
