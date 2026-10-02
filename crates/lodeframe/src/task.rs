// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Async work started from a handler, and its result coming back to the instance thread.
//!
//! A handler cannot `.await`. [`Ctx::spawn`] sends a future to the runtime and returns at once;
//! [`Spawned::then`] says what to do with the result. The callback stays on the instance
//! thread (so it need not be `Send`) and runs at the start of the next tick, with the
//! [`Ctx`] like any handler.

use std::{any::Any, collections::HashMap};

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::world::{Ctx, PlayerId};

type Output = Box<dyn Any + Send>;
type Callback = Box<dyn FnOnce(Output, &mut Ctx)>;

/// A finished future: its output, or nothing if it panicked or was dropped before finishing.
struct Done {
    id: u64,
    output: Option<Output>,
}

/// Tells the instance that a future is over, however it ended.
///
/// Sent from `drop` so that a future that panics or is cancelled is reported too, and nobody
/// waits for it for ever.
struct Report {
    id: u64,
    tx: UnboundedSender<Done>,
    output: Option<Output>,
}

impl Drop for Report {
    fn drop(&mut self) {
        // the instance may be gone already: nobody to tell
        let _ = self.tx.send(Done {
            id: self.id,
            output: self.output.take(),
        });
    }
}

/// The async work of one [`Ctx`].
pub(crate) struct Tasks {
    runtime: Option<tokio::runtime::Handle>,
    // results come back unbounded: the senders are tasks, which have nobody to hold up
    tx: UnboundedSender<Done>,
    rx: UnboundedReceiver<Done>,
    callbacks: HashMap<u64, Callback>,
    next: u64,
}

impl Tasks {
    pub(crate) fn new() -> Self {
        let (tx, rx) = unbounded_channel();
        Self {
            runtime: None,
            tx,
            rx,
            callbacks: HashMap::new(),
            next: 0,
        }
    }

    pub(crate) fn attach(&mut self, runtime: tokio::runtime::Handle) {
        self.runtime = Some(runtime);
    }

    fn start<F>(&mut self, future: F) -> u64
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        let runtime = self.runtime.as_ref().expect(
            "ctx.spawn needs a runtime: run the world through `Server`, `TestEnv`, or an \
             instance that passes `Instance::attach` on to it",
        );
        let id = self.next;
        self.next += 1;
        let report = Report {
            id,
            tx: self.tx.clone(),
            output: None,
        };
        runtime.spawn(async move {
            // the whole report moves in, so that dropping it is what tells the instance; naming
            // only its field would capture just that
            let mut report = report;
            report.output = Some(Box::new(future.await));
        });
        id
    }

    /// The results that came in since the last call, with their callbacks, oldest first.
    /// Taken all at once, so that work a callback starts is not handed back in the same tick.
    pub(crate) fn take_done(&mut self) -> Vec<(Callback, Output)> {
        let mut done = Vec::new();
        while let Ok(Done { id, output }) = self.rx.try_recv() {
            // no callback: the caller did not ask for the result
            let Some(callback) = self.callbacks.remove(&id) else {
                continue;
            };
            match output {
                Some(output) => done.push((callback, output)),
                None => {
                    tracing::error!("an async task panicked or was dropped; its result is lost")
                }
            }
        }
        done
    }
}

/// A future that was sent to the runtime. Say what to do with its result with [`then`](Self::then)
/// or [`then_for`](Self::then_for); without either the result is dropped.
#[must_use = "without `then` the result is thrown away"]
pub struct Spawned<'a, T> {
    ctx: &'a mut Ctx,
    id: u64,
    result: std::marker::PhantomData<fn() -> T>,
}

impl<T: Send + 'static> Spawned<'_, T> {
    /// Runs `callback` with the result at the start of the next tick, however many ticks the
    /// future took. Results arrive in the order the futures finished.
    ///
    /// The callback runs even if the player it is about has left; use
    /// [`then_for`](Self::then_for) to skip it then. A future that panics gives no result and
    /// no call.
    pub fn then(self, callback: impl FnOnce(T, &mut Ctx) + 'static) {
        self.ctx.tasks.callbacks.insert(
            self.id,
            Box::new(move |output, ctx| match output.downcast::<T>() {
                Ok(value) => callback(*value, ctx),
                Err(_) => unreachable!("a result has the type of the future that made it"),
            }),
        );
    }

    /// Like [`then`](Self::then), but does not call `callback` if `player` is no longer here
    /// when the result arrives, including when another player of the same name has come since.
    pub fn then_for(self, player: PlayerId, callback: impl FnOnce(T, &mut Ctx) + 'static) {
        self.then(move |value, ctx| {
            if ctx.is_online(player) {
                callback(value, ctx);
            }
        });
    }
}

impl Ctx {
    /// Sends `future` to the runtime, which runs it beside the tick loop. The tick does not wait
    /// for it. Add [`then`](Spawned::then) to get the result back here.
    ///
    /// Handlers must not block or `.await`; this is how they wait for a database or a web
    /// request. Anything the future needs must be moved in: it runs on another thread and
    /// cannot touch the `Ctx`.
    ///
    /// # Panics
    ///
    /// If the world has no runtime yet. `Server` and `TestEnv` give it one; an instance of your
    /// own that wraps a [`World`](crate::world::World) must pass
    /// [`Instance::attach`](crate::instance::Instance::attach) on to it.
    pub fn spawn<F>(&mut self, future: F) -> Spawned<'_, F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        let id = self.tasks.start(future);
        Spawned {
            ctx: self,
            id,
            result: std::marker::PhantomData,
        }
    }

    /// Calls the callbacks of the futures that finished. The world does this at the start of
    /// each tick.
    pub(crate) fn run_done_tasks(&mut self) {
        for (callback, output) in self.tasks.take_done() {
            callback(output, self);
        }
    }
}
