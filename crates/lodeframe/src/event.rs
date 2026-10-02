// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Events and the tree of nodes that handle them.
//!
//! A node holds listeners keyed by event type and owns child nodes. Emitting on a node runs its
//! own listeners, then those of its children, depth first. Adding or removing a child switches
//! the listeners of its whole subtree on or off at once.
//!
//! Order: within a node, a listener with a higher priority runs first, and equal priorities
//! run in the order they were added. Children are ordered the same way by their own priority.
//! There is no ranking across the whole tree, so a deep listener with a high priority can run
//! after a shallow one with a low priority.
//!
//! Listeners run on the instance thread, one after another. They must not block or `.await`;
//! start async work with `ctx.spawn` instead.
//!
//! `C` is whatever the listeners need to act on, usually the instance. Keep the root node next
//! to that state rather than inside it, so a listener can borrow `C` mutably while the node is
//! being walked.

use std::{
    any::{Any, TypeId},
    collections::HashMap,
    marker::PhantomData,
    sync::atomic::{AtomicU64, Ordering},
};

/// Something that can be emitted on an [`EventNode`].
pub trait Event: 'static {
    /// Whether a listener cancelled it. Cancellable events override this; the code that emits an
    /// event skips its default action when [`EventNode::emit`] returns `true`.
    fn is_cancelled(&self) -> bool {
        false
    }

    /// Shows this event as each of its parents, which are traits: a listener for
    /// `dyn PlayerEvent` also hears every event that lists `dyn PlayerEvent` here.
    ///
    /// ```ignore
    /// fn parents<C: 'static>(&mut self, parents: &mut Parents<'_, C>) {
    ///     parents.visit::<dyn PlayerEvent>(self);
    /// }
    /// ```
    ///
    /// Name the parent with a turbofish, `node.on::<dyn PlayerEvent>(|e, ctx| ..)`, and leave the
    /// closure's parameters unannotated: writing `&mut dyn PlayerEvent` there does not compile.
    ///
    /// At each node the listeners of the event's own type run first, then those of its parents
    /// in the order listed. Parent listeners that ask to [`ignore_cancelled`](Listener::ignore_cancelled)
    /// judge the event as it was when the node's parent listeners began.
    fn parents<C: 'static>(&mut self, _parents: &mut Parents<'_, C>) {}
}

fn next_id() -> u64 {
    // only hands out distinct numbers: it holds no game state
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Identifies a child of the node that returned it. Unique across the process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChildId(u64);

impl ChildId {
    /// A fresh id, for a child whose adding is put off.
    pub(crate) fn fresh() -> Self {
        Self(next_id())
    }
}

/// Identifies a listener, to take it off again. Unique across the process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ListenerId(u64);

impl ListenerId {
    /// A fresh id, for a listener whose adding is put off.
    pub(crate) fn fresh() -> Self {
        Self(next_id())
    }
}

type Handler<E, C> = Box<dyn FnMut(&mut E, &mut C)>;
type Until<E, C> = Box<dyn FnMut(&E, &C) -> bool>;

/// A handler for `E` with the options of when it runs. `E` is an event, or a trait object for
/// the parent of events (see [`Event::parents`]).
pub struct Listener<E: ?Sized, C> {
    handler: Handler<E, C>,
    priority: i32,
    times: Option<u32>,
    until: Option<Until<E, C>>,
    ignore_cancelled: bool,
}

impl<E: ?Sized + 'static, C: 'static> Listener<E, C> {
    /// A listener that runs `handler` for every `E`, with priority 0.
    pub fn new(handler: impl FnMut(&mut E, &mut C) + 'static) -> Self {
        Self {
            handler: Box::new(handler),
            priority: 0,
            times: None,
            until: None,
            ignore_cancelled: false,
        }
    }

    /// Runs before listeners of a lower priority in the same node. 0 to begin with.
    pub fn priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    /// Takes the listener off after it ran `n` times.
    pub fn times(mut self, n: u32) -> Self {
        self.times = Some(n);
        self
    }

    /// Takes the listener off, without running it, the first time `done` returns `true`.
    /// Together with [`times`](Self::times), whichever comes first.
    pub fn until(mut self, done: impl FnMut(&E, &C) -> bool + 'static) -> Self {
        self.until = Some(Box::new(done));
        self
    }

    /// Skips events that an earlier listener already cancelled. Without this a listener hears
    /// every event, cancelled or not.
    pub fn ignore_cancelled(mut self) -> Self {
        self.ignore_cancelled = true;
        self
    }
}

struct Entry<E: ?Sized, C> {
    id: ListenerId,
    listener: Listener<E, C>,
    done: bool,
}

/// A `Vec<Entry<E, C>>` with its `E` forgotten, for what does not depend on the event type.
trait Erased {
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn remove(&mut self, id: ListenerId) -> bool;
}

impl<E: ?Sized + 'static, C: 'static> Erased for Vec<Entry<E, C>> {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn remove(&mut self, id: ListenerId) -> bool {
        match self.iter().position(|e| e.id == id) {
            Some(at) => {
                self.remove(at);
                true
            }
            None => false,
        }
    }
}

type Gate<E, C> = Box<dyn Fn(&E, &C) -> bool>;

struct GateSlot {
    event: TypeId,
    pass_others: bool,
    // a `Gate<E, C>` for the `E` of `event`
    check: Box<dyn Any>,
}

/// Runs the listeners of `E` held in `list`, a `Vec<Entry<E, C>>`.
fn run_listeners<E: ?Sized + 'static, C: 'static>(
    list: &mut dyn Erased,
    event: &mut E,
    ctx: &mut C,
    cancelled: impl Fn(&E) -> bool,
) {
    let list = list
        .as_any_mut()
        .downcast_mut::<Vec<Entry<E, C>>>()
        .expect("listeners are stored under the type id of their event");
    let mut any_done = false;
    for entry in list.iter_mut() {
        let l = &mut entry.listener;
        if l.ignore_cancelled && cancelled(event) {
            continue;
        }
        if let Some(until) = &mut l.until
            && until(event, ctx)
        {
            entry.done = true;
            any_done = true;
            continue;
        }
        (l.handler)(event, ctx);
        if let Some(n) = &mut l.times {
            *n = n.saturating_sub(1);
            if *n == 0 {
                entry.done = true;
                any_done = true;
            }
        }
    }
    if any_done {
        list.retain(|e| !e.done);
    }
}

enum Op<'a, C> {
    Run {
        listeners: &'a mut HashMap<TypeId, Box<dyn Erased>>,
        ctx: &'a mut C,
        cancelled: bool,
    },
    Gate {
        want: TypeId,
        check: &'a dyn Any,
        ctx: &'a C,
        verdict: &'a mut Option<bool>,
    },
}

/// What an event shows its parents to; see [`Event::parents`].
pub struct Parents<'a, C> {
    op: Op<'a, C>,
}

impl<C: 'static> Parents<'_, C> {
    /// Shows the event as a `P`. Pass `self`; it becomes a `&mut dyn Trait` where `P` is
    /// `dyn Trait`.
    pub fn visit<P: ?Sized + 'static>(&mut self, parent: &mut P) {
        match &mut self.op {
            Op::Run {
                listeners,
                ctx,
                cancelled,
            } => {
                let cancelled = *cancelled;
                if let Some(list) = listeners.get_mut(&TypeId::of::<P>()) {
                    run_listeners(list.as_mut(), parent, &mut **ctx, |_| cancelled);
                }
            }
            Op::Gate {
                want,
                check,
                ctx,
                verdict,
            } => {
                if *want == TypeId::of::<P>() {
                    let check = check
                        .downcast_ref::<Gate<P, C>>()
                        .expect("a gate is stored with the type id of its event");
                    **verdict = Some(check(parent, ctx));
                }
            }
        }
    }
}

/// Something that adds a set of listeners to a node at once. A closure
/// `FnOnce(&mut EventNode<C>)` is one.
pub trait Bundle<C> {
    /// Adds the listeners and children of this bundle to `node`.
    fn register(self, node: &mut EventNode<C>);
}

impl<C, F: FnOnce(&mut EventNode<C>)> Bundle<C> for F {
    fn register(self, node: &mut EventNode<C>) {
        self(node)
    }
}

struct Child<C> {
    id: ChildId,
    priority: i32,
    node: EventNode<C>,
}

/// A node in the handler tree. See the [module docs](self).
pub struct EventNode<C> {
    // each value is a `Vec<Entry<E, C>>` for the `E` of its key, highest priority first
    listeners: HashMap<TypeId, Box<dyn Erased>>,
    children: Vec<Child<C>>,
    gate: Option<GateSlot>,
    // the listeners are type-erased, so `C` would otherwise appear only in `children`
    context: PhantomData<fn(&mut C)>,
}

impl<C> Default for EventNode<C> {
    fn default() -> Self {
        Self {
            listeners: HashMap::new(),
            children: Vec::new(),
            gate: None,
            context: PhantomData,
        }
    }
}

impl<C: 'static> EventNode<C> {
    /// A node with no listeners and no children.
    pub fn new() -> Self {
        Self::default()
    }

    /// Runs `handler` for every `E` emitted on this node or above it, with the default options.
    /// For priority and the rest, use [`add_listener`](Self::add_listener).
    pub fn on<E: ?Sized + 'static>(
        &mut self,
        handler: impl FnMut(&mut E, &mut C) + 'static,
    ) -> &mut Self {
        self.add_listener(Listener::new(handler));
        self
    }

    /// Adds a listener. Returns the id to take it off with.
    pub fn add_listener<E: ?Sized + 'static>(&mut self, listener: Listener<E, C>) -> ListenerId {
        let id = ListenerId::fresh();
        self.insert_listener(id, listener);
        id
    }

    pub(crate) fn insert_listener<E: ?Sized + 'static>(
        &mut self,
        id: ListenerId,
        listener: Listener<E, C>,
    ) {
        let list = self
            .listeners
            .entry(TypeId::of::<E>())
            .or_insert_with(|| Box::new(Vec::<Entry<E, C>>::new()))
            .as_any_mut()
            .downcast_mut::<Vec<Entry<E, C>>>()
            .expect("listeners are stored under the type id of their event");
        // after the last one that is at least as high, so equal priorities keep their order
        let at = list.partition_point(|e| e.listener.priority >= listener.priority);
        list.insert(
            at,
            Entry {
                id,
                listener,
                done: false,
            },
        );
    }

    /// Takes a listener off, here or anywhere below. Returns whether it was found.
    pub fn remove_listener(&mut self, id: ListenerId) -> bool {
        for list in self.listeners.values_mut() {
            if list.remove(id) {
                return true;
            }
        }
        self.children.iter_mut().any(|c| c.node.remove_listener(id))
    }

    /// Adds the listeners and children of `bundle` to this node.
    pub fn install(&mut self, bundle: impl Bundle<C>) -> &mut Self {
        bundle.register(self);
        self
    }

    /// Lets events through to this node and its subtree only while `check` is true, for events
    /// that are an `E` or have it as a parent. Events of any other type pass without being
    /// checked. Replaces an earlier gate of this node; nest nodes to require several.
    pub fn only_if<E: ?Sized + 'static>(
        &mut self,
        check: impl Fn(&E, &C) -> bool + 'static,
    ) -> &mut Self {
        self.set_gate::<E>(true, Box::new(check))
    }

    /// Like [`only_if`](Self::only_if), but events of any other type are kept out: this node
    /// and its subtree are for `E` only.
    pub fn only_for<E: ?Sized + 'static>(
        &mut self,
        check: impl Fn(&E, &C) -> bool + 'static,
    ) -> &mut Self {
        self.set_gate::<E>(false, Box::new(check))
    }

    fn set_gate<E: ?Sized + 'static>(&mut self, pass_others: bool, check: Gate<E, C>) -> &mut Self {
        self.gate = Some(GateSlot {
            event: TypeId::of::<E>(),
            pass_others,
            check: Box::new(check),
        });
        self
    }

    /// Attaches `node` and everything under it, with priority 0. Its listeners run after this
    /// node's own and after those of children of the same or a higher priority added earlier.
    pub fn add_child(&mut self, node: Self) -> ChildId {
        self.add_child_at(node, 0)
    }

    /// Attaches `node` so that it runs before children of a lower priority.
    pub fn add_child_at(&mut self, node: Self, priority: i32) -> ChildId {
        let id = ChildId::fresh();
        self.insert_child(id, node, priority);
        id
    }

    pub(crate) fn insert_child(&mut self, id: ChildId, node: Self, priority: i32) {
        let at = self.children.partition_point(|c| c.priority >= priority);
        self.children.insert(at, Child { id, priority, node });
    }

    /// Detaches a child with its whole subtree, which stops handling events. It can be added
    /// again later.
    pub fn remove_child(&mut self, id: ChildId) -> Option<Self> {
        let at = self.children.iter().position(|c| c.id == id)?;
        Some(self.children.remove(at).node)
    }

    /// A child, to add listeners or grandchildren to after it was attached.
    pub fn child_mut(&mut self, id: ChildId) -> Option<&mut Self> {
        self.children
            .iter_mut()
            .find(|c| c.id == id)
            .map(|c| &mut c.node)
    }

    /// Runs the listeners for `event` in this subtree, all of them even after one cancels
    /// (unless they ask to ignore cancelled events). Returns whether the event ended up
    /// cancelled.
    pub fn emit<E: Event>(&mut self, event: &mut E, ctx: &mut C) -> bool {
        self.run(event, ctx);
        event.is_cancelled()
    }

    fn lets_through<E: Event>(&self, event: &mut E, ctx: &C) -> bool {
        let Some(gate) = &self.gate else {
            return true;
        };
        if gate.event == TypeId::of::<E>() {
            let check = gate
                .check
                .downcast_ref::<Gate<E, C>>()
                .expect("a gate is stored with the type id of its event");
            return check(event, ctx);
        }
        let mut verdict = None;
        event.parents(&mut Parents {
            op: Op::Gate {
                want: gate.event,
                check: gate.check.as_ref(),
                ctx,
                verdict: &mut verdict,
            },
        });
        verdict.unwrap_or(gate.pass_others)
    }

    fn run<E: Event>(&mut self, event: &mut E, ctx: &mut C) {
        if !self.lets_through(event, ctx) {
            return;
        }
        if let Some(list) = self.listeners.get_mut(&TypeId::of::<E>()) {
            run_listeners(list.as_mut(), event, ctx, |e: &E| e.is_cancelled());
        }
        let cancelled = event.is_cancelled();
        event.parents(&mut Parents {
            op: Op::Run {
                listeners: &mut self.listeners,
                ctx,
                cancelled,
            },
        });
        for child in &mut self.children {
            child.node.run(event, ctx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Place {
        cancelled: bool,
    }
    impl Event for Place {
        fn is_cancelled(&self) -> bool {
            self.cancelled
        }
    }

    struct Chat;
    impl Event for Chat {}

    /// What handlers act on: stands in for the instance.
    #[derive(Default)]
    struct World {
        log: Vec<&'static str>,
    }

    fn logger(name: &'static str) -> impl FnMut(&mut Chat, &mut World) {
        move |_, w| w.log.push(name)
    }

    #[test]
    fn a_child_switches_its_handlers_on_and_off_together() {
        let mut root = EventNode::<World>::new();
        root.on(logger("root"));
        let mut game = EventNode::<World>::new();
        game.on(logger("game a")).on(logger("game b"));
        let mut sub = EventNode::<World>::new();
        sub.on(logger("nested"));
        game.add_child(sub);
        let id = root.add_child(game);
        let mut w = World::default();

        root.emit(&mut Chat, &mut w);
        assert_eq!(w.log, ["root", "game a", "game b", "nested"]);

        let game = root.remove_child(id).unwrap();
        w.log.clear();
        root.emit(&mut Chat, &mut w);
        assert_eq!(w.log, ["root"]);
        assert!(root.remove_child(id).is_none());

        // and back on
        root.add_child(game);
        w.log.clear();
        root.emit(&mut Chat, &mut w);
        assert_eq!(w.log.len(), 4);
    }

    #[test]
    fn a_cancelled_event_is_reported_to_the_emitter() {
        let mut root = EventNode::<World>::new();
        root.on(|e: &mut Place, _| e.cancelled = true);
        let mut w = World::default();
        // the emitter places the block only if nobody cancelled
        assert!(root.emit(&mut Place::default(), &mut w));
        assert!(!EventNode::<World>::new().emit(&mut Place::default(), &mut w));
        // events nobody handles are not cancelled
        assert!(!root.emit(&mut Chat, &mut w));
    }

    #[test]
    fn handlers_can_change_the_context_and_the_event() {
        let mut root = EventNode::<World>::new();
        root.on(|_: &mut Chat, w: &mut World| w.log.push("seen"));
        root.on(|e: &mut Place, w: &mut World| {
            w.log.push("place");
            e.cancelled = true;
        });
        let mut w = World::default();
        let mut p = Place::default();
        root.emit(&mut Chat, &mut w);
        root.emit(&mut p, &mut w);
        assert_eq!(w.log, ["seen", "place"]);
        assert!(p.cancelled);
    }

    #[test]
    fn a_child_can_get_handlers_after_it_was_attached() {
        let mut root = EventNode::<World>::new();
        let id = root.add_child(EventNode::<World>::new());
        root.child_mut(id).unwrap().on(logger("late"));
        let mut w = World::default();
        root.emit(&mut Chat, &mut w);
        assert_eq!(w.log, ["late"]);
    }

    trait Owned {
        fn owner(&self) -> u32;
    }

    struct Dig {
        owner: u32,
        cancelled: bool,
    }
    impl Owned for Dig {
        fn owner(&self) -> u32 {
            self.owner
        }
    }
    impl Event for Dig {
        fn is_cancelled(&self) -> bool {
            self.cancelled
        }
        fn parents<C: 'static>(&mut self, parents: &mut Parents<'_, C>) {
            parents.visit::<dyn Owned>(self);
        }
    }

    struct Say(u32);
    impl Owned for Say {
        fn owner(&self) -> u32 {
            self.0
        }
    }
    impl Event for Say {
        fn parents<C: 'static>(&mut self, parents: &mut Parents<'_, C>) {
            parents.visit::<dyn Owned>(self);
        }
    }

    fn owner_logger(e: &mut (dyn Owned + 'static), w: &mut World) {
        w.log.push(if e.owner() == 7 { "seven" } else { "other" });
    }

    fn dig(owner: u32) -> Dig {
        Dig {
            owner,
            cancelled: false,
        }
    }

    fn chat_logger(name: &'static str) -> Listener<Chat, World> {
        Listener::new(move |_: &mut Chat, w: &mut World| w.log.push(name))
    }

    #[test]
    fn a_higher_priority_runs_first_and_equal_ones_keep_their_order() {
        let mut root = EventNode::<World>::new();
        root.add_listener(chat_logger("low").priority(-5));
        root.add_listener(chat_logger("a"));
        root.add_listener(chat_logger("high").priority(10));
        root.add_listener(chat_logger("b"));
        let mut w = World::default();
        root.emit(&mut Chat, &mut w);
        assert_eq!(w.log, ["high", "a", "b", "low"]);
    }

    #[test]
    fn a_child_with_a_higher_priority_runs_before_its_siblings() {
        let mut root = EventNode::<World>::new();
        for (name, priority) in [("first", 0), ("urgent", 5), ("second", 0)] {
            let mut n = EventNode::<World>::new();
            n.on(logger(name));
            root.add_child_at(n, priority);
        }
        let mut w = World::default();
        root.emit(&mut Chat, &mut w);
        assert_eq!(w.log, ["urgent", "first", "second"]);
    }

    #[test]
    fn a_listener_can_run_a_limited_number_of_times() {
        let mut root = EventNode::<World>::new();
        root.add_listener(chat_logger("once").times(1));
        root.add_listener(chat_logger("twice").times(2));
        root.add_listener(chat_logger("always"));
        let mut w = World::default();
        for _ in 0..3 {
            root.emit(&mut Chat, &mut w);
        }
        assert_eq!(
            w.log,
            ["once", "twice", "always", "twice", "always", "always"]
        );
    }

    #[test]
    fn a_listener_ends_before_running_once_its_condition_holds() {
        let mut root = EventNode::<World>::new();
        root.add_listener(chat_logger("until").until(|_, w: &World| w.log.len() >= 3));
        let mut w = World::default();
        for _ in 0..5 {
            root.emit(&mut Chat, &mut w);
        }
        assert_eq!(w.log, ["until", "until", "until"]);
    }

    #[test]
    fn a_listener_can_be_taken_off_by_its_id() {
        let mut root = EventNode::<World>::new();
        let gone = root.add_listener(chat_logger("gone"));
        root.add_listener(chat_logger("kept"));
        let mut deep = EventNode::<World>::new();
        let deep_id = deep.add_listener(chat_logger("deep"));
        root.add_child(deep);
        assert!(root.remove_listener(gone));
        assert!(root.remove_listener(deep_id));
        assert!(!root.remove_listener(gone));
        let mut w = World::default();
        root.emit(&mut Chat, &mut w);
        assert_eq!(w.log, ["kept"]);
    }

    #[test]
    fn a_listener_can_ignore_what_was_cancelled() {
        let mut root = EventNode::<World>::new();
        let note = |name: &'static str| {
            Listener::new(move |_: &mut Place, w: &mut World| w.log.push(name))
        };
        root.add_listener(
            Listener::new(|e: &mut Place, _: &mut World| e.cancelled = true).priority(10),
        );
        root.add_listener(note("hears"));
        root.add_listener(note("ignores").ignore_cancelled());
        let mut w = World::default();
        assert!(root.emit(&mut Place::default(), &mut w));
        assert_eq!(w.log, ["hears"]);
    }

    #[test]
    fn a_gate_lets_events_through_only_while_it_holds_and_others_pass() {
        let mut root = EventNode::<World>::new();
        let mut game = EventNode::<World>::new();
        game.only_if::<Place>(|e, _| !e.cancelled);
        game.on(|_: &mut Place, w: &mut World| w.log.push("place"));
        game.on(logger("chat"));
        root.add_child(game);
        let mut w = World::default();
        root.emit(&mut Place::default(), &mut w);
        root.emit(&mut Place { cancelled: true }, &mut w);
        root.emit(&mut Chat, &mut w);
        assert_eq!(w.log, ["place", "chat"]);
    }

    #[test]
    fn a_gate_for_one_type_keeps_the_others_out() {
        let mut root = EventNode::<World>::new();
        let mut only = EventNode::<World>::new();
        only.only_for::<Place>(|_, _| true);
        only.on(|_: &mut Place, w: &mut World| w.log.push("place"));
        only.on(logger("chat"));
        root.add_child(only);
        let mut w = World::default();
        root.emit(&mut Chat, &mut w);
        root.emit(&mut Place::default(), &mut w);
        assert_eq!(w.log, ["place"]);
    }

    #[test]
    fn a_gate_can_look_at_the_parent_of_an_event() {
        let mut root = EventNode::<World>::new();
        let mut mine = EventNode::<World>::new();
        mine.only_if::<dyn Owned>(|e, _| e.owner() == 1);
        mine.on(|_: &mut Dig, w: &mut World| w.log.push("dig"));
        mine.on(|_: &mut Say, w: &mut World| w.log.push("say"));
        root.add_child(mine);
        let mut w = World::default();
        root.emit(&mut dig(2), &mut w);
        root.emit(&mut dig(1), &mut w);
        root.emit(&mut Say(2), &mut w);
        root.emit(&mut Say(1), &mut w);
        assert_eq!(w.log, ["dig", "say"]);
    }

    #[test]
    fn a_listener_for_a_parent_hears_every_event_that_has_it() {
        let mut root = EventNode::<World>::new();
        root.on(owner_logger);
        root.on(|_: &mut Dig, w: &mut World| w.log.push("dig"));
        let mut w = World::default();
        root.emit(&mut dig(7), &mut w);
        root.emit(&mut Say(3), &mut w);
        root.emit(&mut Chat, &mut w);
        // the event's own listeners come before those of its parents
        assert_eq!(w.log, ["dig", "seven", "other"]);
    }

    #[test]
    fn a_bundle_adds_its_listeners_to_the_node() {
        struct Greeting;
        impl Bundle<World> for Greeting {
            fn register(self, node: &mut EventNode<World>) {
                node.on(logger("hello")).on(logger("again"));
            }
        }
        let mut root = EventNode::<World>::new();
        root.install(Greeting).install(|n: &mut EventNode<World>| {
            n.on(logger("closure"));
        });
        let mut w = World::default();
        root.emit(&mut Chat, &mut w);
        assert_eq!(w.log, ["hello", "again", "closure"]);
    }
}
