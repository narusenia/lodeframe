// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Events and the tree of nodes that handle them.
//!
//! A node holds handlers keyed by event type and owns child nodes. Emitting on a node runs its
//! own handlers, then those of its children, depth first. Adding or removing a child switches
//! the handlers of its whole subtree on or off at once.
//!
//! Handlers run on the instance thread, one after another. They must not block or `.await`;
//! start async work with `ctx.spawn` instead.
//!
//! `C` is whatever the handlers need to act on, usually the instance. Keep the root node next
//! to that state rather than inside it, so a handler can borrow `C` mutably while the node is
//! being walked.

use std::{
    any::{Any, TypeId},
    collections::HashMap,
    marker::PhantomData,
};

/// Something that can be emitted on an [`EventNode`].
pub trait Event: 'static {
    /// Whether a handler cancelled it. Cancellable events override this; the code that emits an
    /// event skips its default action when [`EventNode::emit`] returns `true`.
    fn is_cancelled(&self) -> bool {
        false
    }
}

/// Identifies a child of the node that returned it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChildId(u64);

type Handler<E, C> = Box<dyn FnMut(&mut E, &mut C)>;

/// A node in the handler tree. See the [module docs](self).
pub struct EventNode<C> {
    // each value is a `Vec<Handler<E, C>>` for the `E` of its key
    handlers: HashMap<TypeId, Box<dyn Any>>,
    children: Vec<(ChildId, EventNode<C>)>,
    next_child: u64,
    // the handlers are type-erased, so `C` would otherwise appear only in `children`
    context: PhantomData<fn(&mut C)>,
}

impl<C> Default for EventNode<C> {
    fn default() -> Self {
        Self {
            handlers: HashMap::new(),
            children: Vec::new(),
            next_child: 0,
            context: PhantomData,
        }
    }
}

impl<C: 'static> EventNode<C> {
    /// A node with no handlers and no children.
    pub fn new() -> Self {
        Self::default()
    }

    /// Runs `handler` for every `E` emitted on this node or above it. Handlers of one node run
    /// in the order they were added.
    pub fn on<E: Event>(&mut self, handler: impl FnMut(&mut E, &mut C) + 'static) -> &mut Self {
        self.handlers
            .entry(TypeId::of::<E>())
            .or_insert_with(|| Box::new(Vec::<Handler<E, C>>::new()))
            .downcast_mut::<Vec<Handler<E, C>>>()
            .expect("handlers are stored under the type id of their event")
            .push(Box::new(handler));
        self
    }

    /// Attaches `node` and everything under it. Its handlers run after this node's own and
    /// after those of children added earlier.
    pub fn add_child(&mut self, node: Self) -> ChildId {
        let id = ChildId(self.next_child);
        self.next_child += 1;
        self.children.push((id, node));
        id
    }

    /// Detaches a child with its whole subtree, which stops handling events. It can be added
    /// again later.
    pub fn remove_child(&mut self, id: ChildId) -> Option<Self> {
        let at = self.children.iter().position(|(c, _)| *c == id)?;
        Some(self.children.remove(at).1)
    }

    /// A child, to add handlers or grandchildren to after it was attached.
    pub fn child_mut(&mut self, id: ChildId) -> Option<&mut Self> {
        self.children
            .iter_mut()
            .find(|(c, _)| *c == id)
            .map(|(_, n)| n)
    }

    /// Runs the handlers for `event` in this subtree, all of them even after one cancels.
    /// Returns whether the event ended up cancelled.
    pub fn emit<E: Event>(&mut self, event: &mut E, ctx: &mut C) -> bool {
        self.run(event, ctx);
        event.is_cancelled()
    }

    fn run<E: Event>(&mut self, event: &mut E, ctx: &mut C) {
        if let Some(handlers) = self.handlers.get_mut(&TypeId::of::<E>()) {
            let handlers = handlers
                .downcast_mut::<Vec<Handler<E, C>>>()
                .expect("handlers are stored under the type id of their event");
            for h in handlers {
                h(event, ctx);
            }
        }
        for (_, child) in &mut self.children {
            child.run(event, ctx);
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
}
