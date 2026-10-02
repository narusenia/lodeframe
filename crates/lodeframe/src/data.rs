// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Typed values attached to a player or a world.
//!
//! A [`Data`] holds values of any type, found by a [`Key`] (a name and the type of the value) or
//! by the type alone. It is for what a game knows about a player or an instance while it runs; it
//! is not saved.
//!
//! ```
//! use lodeframe::data::{Data, Key};
//!
//! const SCORE: Key<u32> = Key::new("mygame:score");
//!
//! let mut data = Data::default();
//! *data.get_or_insert_with(&SCORE, || 0) += 5;
//! assert_eq!(data.get(&SCORE), Some(&5));
//! ```

use std::{
    any::{Any, TypeId},
    collections::HashMap,
    fmt,
    marker::PhantomData,
};

/// Names a value of type `T` in a [`Data`].
///
/// Two keys with the same name but different types are different keys: reading one gives
/// nothing for a value stored with the other. Give names a prefix of your own (`"mygame:score"`)
/// so that two crates do not pick the same one for the same type.
pub struct Key<T> {
    name: &'static str,
    value: PhantomData<fn() -> T>,
}

impl<T> Key<T> {
    /// A key for values of type `T` called `name`. Declare it as a `const` and use it everywhere.
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            value: PhantomData,
        }
    }

    /// The name given to [`new`](Self::new).
    pub const fn name(&self) -> &'static str {
        self.name
    }
}

// by hand: derives would ask `T` for the same traits
impl<T> Clone for Key<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Key<T> {}

impl<T> fmt::Debug for Key<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Key({:?})", self.name)
    }
}

/// Where a value is: under a name and a type, or under the type alone.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Slot {
    name: Option<&'static str>,
    ty: TypeId,
}

impl Slot {
    fn named<T: 'static>(key: &Key<T>) -> Self {
        Self {
            name: Some(key.name),
            ty: TypeId::of::<T>(),
        }
    }

    fn typed<T: 'static>() -> Self {
        Self {
            name: None,
            ty: TypeId::of::<T>(),
        }
    }
}

/// Typed values, found by [`Key`] or by type. See the [module](self) for an example.
///
/// A player's is [`Ctx::player_data`](crate::world::Ctx::player_data), a world's
/// [`Ctx::data`](crate::world::Ctx::data).
#[derive(Default)]
pub struct Data {
    values: HashMap<Slot, Box<dyn Any>>,
}

impl Data {
    fn get_slot<T: 'static>(&self, slot: Slot) -> Option<&T> {
        self.values.get(&slot)?.downcast_ref()
    }

    fn get_slot_mut<T: 'static>(&mut self, slot: Slot) -> Option<&mut T> {
        self.values.get_mut(&slot)?.downcast_mut()
    }

    fn set_slot<T: 'static>(&mut self, slot: Slot, value: T) -> Option<T> {
        let old = self.values.insert(slot, Box::new(value))?;
        old.downcast().ok().map(|old| *old)
    }

    fn remove_slot<T: 'static>(&mut self, slot: Slot) -> Option<T> {
        self.values.remove(&slot)?.downcast().ok().map(|old| *old)
    }

    /// The value under `key`.
    pub fn get<T: 'static>(&self, key: &Key<T>) -> Option<&T> {
        self.get_slot(Slot::named(key))
    }

    /// The value under `key`, to change in place.
    pub fn get_mut<T: 'static>(&mut self, key: &Key<T>) -> Option<&mut T> {
        self.get_slot_mut(Slot::named(key))
    }

    /// Stores `value` under `key`. Returns the value that was there.
    pub fn set<T: 'static>(&mut self, key: &Key<T>, value: T) -> Option<T> {
        self.set_slot(Slot::named(key), value)
    }

    /// Takes the value under `key` out.
    pub fn remove<T: 'static>(&mut self, key: &Key<T>) -> Option<T> {
        self.remove_slot(Slot::named(key))
    }

    /// Whether there is a value under `key`.
    pub fn contains<T: 'static>(&self, key: &Key<T>) -> bool {
        self.values.contains_key(&Slot::named(key))
    }

    /// The value under `key`, stored first as `init()` if there was none.
    pub fn get_or_insert_with<T: 'static>(
        &mut self,
        key: &Key<T>,
        init: impl FnOnce() -> T,
    ) -> &mut T {
        self.values
            .entry(Slot::named(key))
            .or_insert_with(|| Box::new(init()))
            .downcast_mut()
            .expect("a slot holds the type it is named by")
    }

    /// The value of type `T`, for values that have no name.
    pub fn get_by_type<T: 'static>(&self) -> Option<&T> {
        self.get_slot(Slot::typed::<T>())
    }

    /// The value of type `T`, to change in place.
    pub fn get_mut_by_type<T: 'static>(&mut self) -> Option<&mut T> {
        self.get_slot_mut(Slot::typed::<T>())
    }

    /// Stores `value` as the one of its type. Returns the value that was there. It is not the
    /// value of a [`Key`] of the same type: those are separate.
    pub fn set_by_type<T: 'static>(&mut self, value: T) -> Option<T> {
        self.set_slot(Slot::typed::<T>(), value)
    }

    /// Takes the value of type `T` out.
    pub fn remove_by_type<T: 'static>(&mut self) -> Option<T> {
        self.remove_slot(Slot::typed::<T>())
    }
}

impl fmt::Debug for Data {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Data({} values)", self.values.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCORE: Key<u32> = Key::new("test:score");
    const LIVES: Key<u32> = Key::new("test:lives");
    const SCORE_TEXT: Key<String> = Key::new("test:score");

    #[test]
    fn a_value_is_read_back_and_set_returns_the_one_it_replaced() {
        let mut data = Data::default();
        assert_eq!(data.get(&SCORE), None);
        assert_eq!(data.set(&SCORE, 1), None);
        assert_eq!(data.set(&SCORE, 2), Some(1));
        assert_eq!(data.get(&SCORE), Some(&2));
        *data.get_mut(&SCORE).unwrap() += 1;
        assert_eq!(data.remove(&SCORE), Some(3));
        assert!(!data.contains(&SCORE));
    }

    #[test]
    fn a_key_of_another_type_under_the_same_name_finds_nothing() {
        let mut data = Data::default();
        data.set(&SCORE, 7);

        assert_eq!(data.get(&SCORE_TEXT), None);
        assert_eq!(data.remove(&SCORE_TEXT), None);
        // and has a slot of its own
        data.set(&SCORE_TEXT, "seven".into());
        assert_eq!(data.get(&SCORE), Some(&7));
        assert_eq!(data.get(&SCORE_TEXT).map(String::as_str), Some("seven"));
    }

    #[test]
    fn the_same_type_under_two_names_holds_two_values() {
        let mut data = Data::default();
        data.set(&SCORE, 1);
        data.set(&LIVES, 3);

        assert_eq!((data.get(&SCORE), data.get(&LIVES)), (Some(&1), Some(&3)));
    }

    #[test]
    fn get_or_insert_with_only_makes_a_value_when_there_is_none() {
        let mut data = Data::default();
        *data.get_or_insert_with(&SCORE, || 10) += 1;
        *data.get_or_insert_with(&SCORE, || unreachable!()) += 1;

        assert_eq!(data.get(&SCORE), Some(&12));
    }

    #[test]
    fn a_type_is_a_key_of_its_own_apart_from_named_keys() {
        struct Lives(u8);
        let mut data = Data::default();
        data.set(&SCORE, 5);

        assert!(data.set_by_type(7u32).is_none());
        assert!(data.set_by_type(Lives(3)).is_none());
        assert_eq!(data.get_by_type::<u32>(), Some(&7));
        assert_eq!(data.get(&SCORE), Some(&5));
        data.get_mut_by_type::<Lives>().unwrap().0 -= 1;
        assert_eq!(data.remove_by_type::<Lives>().map(|l| l.0), Some(2));
        assert!(data.get_by_type::<Lives>().is_none());
    }
}
