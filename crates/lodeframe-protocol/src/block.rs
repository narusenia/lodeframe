// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Blocks and block states, from the generated tables.
//!
//! A state id is `min_state + index`, where `index` is a mixed-radix number: one digit per
//! property, in the order of [`BlockInfo::properties`] (alphabetical), the first being the
//! most significant. The generator verifies this for every state, so the conversions here
//! compute instead of storing one entry per state.

use std::io::Write;

use crate::{Decode, Encode, Error, Result, VarInt};

pub use crate::generated::blocks::*;

/// A block property such as `facing`, with every value it can take, in order.
#[derive(Debug)]
pub struct Property {
    /// The property name.
    pub name: &'static str,
    /// The possible values.
    pub values: &'static [&'static str],
}

/// What is known about one block.
#[derive(Debug)]
pub struct BlockInfo {
    /// The block name with its namespace, e.g. `minecraft:stone`.
    pub name: &'static str,
    /// The id of the block's first state.
    pub min_state: u16,
    /// The id of the block's default state.
    pub default_state: u16,
    /// The properties, sorted by name.
    pub properties: &'static [&'static Property],
}

impl BlockInfo {
    /// How many states the block has.
    pub fn state_count(&self) -> u16 {
        self.properties
            .iter()
            .map(|p| p.values.len() as u16)
            .product()
    }

    /// The property named `name`, and the number of states between two neighbouring values of it.
    fn locate(&self, name: &str) -> Option<(&'static Property, usize)> {
        let at = self.properties.iter().position(|p| p.name == name)?;
        let stride = self.properties[at + 1..]
            .iter()
            .map(|p| p.values.len())
            .product();
        Some((self.properties[at], stride))
    }
}

/// A block type. The id is the block registry id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Block(pub(crate) u16);

impl Block {
    /// The block with registry id `id`.
    pub fn from_id(id: u16) -> Option<Self> {
        (usize::from(id) < BLOCKS.len()).then_some(Self(id))
    }

    /// The block named `name`, with its namespace (`minecraft:stone`).
    // ponytail: linear scan over ~1300 names, index it if this becomes hot
    pub fn from_name(name: &str) -> Option<Self> {
        BLOCKS
            .iter()
            .position(|b| b.name == name)
            .map(|i| Self(i as u16))
    }

    /// The registry id.
    pub fn id(self) -> u16 {
        self.0
    }

    /// The block's table entry.
    pub fn info(self) -> &'static BlockInfo {
        &BLOCKS[usize::from(self.0)]
    }

    /// The name with its namespace.
    pub fn name(self) -> &'static str {
        self.info().name
    }

    /// The state a freshly placed block has.
    pub fn default_state(self) -> BlockState {
        BlockState(self.info().default_state)
    }

    /// Every state of the block, in id order.
    pub fn states(self) -> impl Iterator<Item = BlockState> {
        let info = self.info();
        (info.min_state..info.min_state + info.state_count()).map(BlockState)
    }
}

/// One state of a block: the block plus a value for each of its properties.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockState(pub(crate) u16);

impl BlockState {
    /// The state with id `id`.
    pub fn from_id(id: u16) -> Option<Self> {
        (id < STATE_COUNT).then_some(Self(id))
    }

    /// The state id, as sent in packets and stored in chunks.
    pub fn id(self) -> u16 {
        self.0
    }

    /// The block this is a state of.
    pub fn block(self) -> Block {
        // the first block starts at state 0, so the partition point is never 0
        Block((BLOCKS.partition_point(|b| b.min_state <= self.0) - 1) as u16)
    }

    /// The value of the property `name`, or `None` if the block has no such property.
    pub fn property(self, name: &str) -> Option<&'static str> {
        let info = self.block().info();
        let (property, stride) = info.locate(name)?;
        let index = usize::from(self.0 - info.min_state);
        Some(property.values[index / stride % property.values.len()])
    }

    /// Every property and its value.
    pub fn properties(self) -> impl Iterator<Item = (&'static str, &'static str)> {
        self.block()
            .info()
            .properties
            .iter()
            .map(move |p| (p.name, self.property(p.name).unwrap()))
    }

    /// The state that differs from this one only in `name`, which is set to `value`.
    /// `None` if the block has no such property or the property no such value.
    pub fn with(self, name: &str, value: &str) -> Option<Self> {
        let info = self.block().info();
        let (property, stride) = info.locate(name)?;
        let new = property.values.iter().position(|v| *v == value)?;
        let index = usize::from(self.0 - info.min_state);
        let old = index / stride % property.values.len();
        Some(Self(
            info.min_state + (index - old * stride + new * stride) as u16,
        ))
    }
}

macro_rules! as_varint {
    ($t:ident, $from:expr) => {
        impl Encode for $t {
            fn encode(&self, w: &mut impl Write) -> Result<()> {
                VarInt(i32::from(self.0)).encode(w)
            }
        }

        impl Decode for $t {
            fn decode(r: &mut &[u8]) -> Result<Self> {
                u16::try_from(VarInt::decode(r)?.0)
                    .ok()
                    .and_then($from)
                    .ok_or(Error::InvalidValue(concat!(
                        "unknown ",
                        stringify!($t),
                        " id"
                    )))
            }
        }
    };
}
as_varint!(Block, Block::from_id);
as_varint!(BlockState, BlockState::from_id);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::tests::{encoded, roundtrip};

    #[test]
    fn the_tables_cover_every_state_exactly_once() {
        let total: u32 = BLOCKS.iter().map(|b| u32::from(b.state_count())).sum();
        assert_eq!(total, u32::from(STATE_COUNT));
        assert_eq!(BLOCKS[0].min_state, 0);
        for (i, b) in BLOCKS.iter().enumerate() {
            let block = Block(i as u16);
            assert_eq!(
                block.states().count(),
                usize::from(b.state_count()),
                "{}",
                b.name
            );
            assert!(
                block.states().any(|s| s == block.default_state()),
                "{}",
                b.name
            );
        }
    }

    #[test]
    fn every_state_roundtrips_through_its_properties() {
        for id in 0..STATE_COUNT {
            let state = BlockState(id);
            let block = state.block();
            assert!(
                block.states().any(|s| s == state),
                "state {id} is not in {}",
                block.name()
            );
            for (name, value) in state.properties() {
                assert_eq!(state.property(name), Some(value));
                assert_eq!(
                    state.with(name, value),
                    Some(state),
                    "{} {name}",
                    block.name()
                );
            }
        }
    }

    #[test]
    fn changing_one_property_leaves_the_others() {
        let stairs = Block::from_name("minecraft:oak_stairs").unwrap();
        let state = stairs.default_state();
        for (name, value) in state.properties() {
            let info = stairs.info();
            let other = info
                .properties
                .iter()
                .find(|p| p.name == name)
                .unwrap()
                .values
                .iter()
                .find(|v| **v != value);
            let Some(other) = other else { continue };
            let changed = state.with(name, other).unwrap();
            assert_eq!(changed.property(name), Some(*other));
            for (n, v) in state.properties().filter(|(n, _)| *n != name) {
                assert_eq!(
                    changed.property(n),
                    Some(v),
                    "{n} changed while setting {name}"
                );
            }
            assert_ne!(changed, state);
        }
    }

    #[test]
    fn oak_stairs_have_the_expected_shape() {
        let stairs = Block::from_name("minecraft:oak_stairs").unwrap();
        assert_eq!(stairs.name(), "minecraft:oak_stairs");
        let names: Vec<_> = stairs.info().properties.iter().map(|p| p.name).collect();
        assert_eq!(names, ["facing", "half", "shape", "waterlogged"]);
        assert_eq!(stairs.info().state_count(), 4 * 2 * 5 * 2);
        let default: Vec<_> = stairs.default_state().properties().collect();
        assert_eq!(
            default,
            [
                ("facing", "north"),
                ("half", "bottom"),
                ("shape", "straight"),
                ("waterlogged", "false")
            ]
        );
    }

    #[test]
    fn lookups_and_misuse() {
        assert_eq!(AIR.default_state(), BlockState(0));
        assert_eq!(Block::from_name("minecraft:air"), Some(AIR));
        assert_eq!(STONE.name(), "minecraft:stone");
        assert_eq!(Block::from_name("minecraft:nope"), None);
        assert_eq!(BlockState(STONE.info().min_state).block(), STONE);
        let state = Block::from_name("minecraft:oak_stairs")
            .unwrap()
            .default_state();
        assert_eq!(state.property("nope"), None);
        assert_eq!(state.with("facing", "sideways"), None);
        assert_eq!(state.with("nope", "north"), None);
        assert_eq!(BlockState::from_id(STATE_COUNT), None);
        assert_eq!(Block::from_id(BLOCKS.len() as u16), None);
    }

    #[test]
    fn ids_are_varints_and_out_of_range_is_rejected() {
        assert_eq!(encoded(&BlockState(300)), [0xac, 0x02]);
        roundtrip(BlockState(STATE_COUNT - 1));
        roundtrip(STONE);
        for bad in [VarInt(i32::from(STATE_COUNT)), VarInt(-1), VarInt(i32::MAX)] {
            let bytes = encoded(&bad);
            assert!(
                matches!(
                    BlockState::decode(&mut bytes.as_slice()),
                    Err(Error::InvalidValue(_))
                ),
                "{bad:?}"
            );
        }
        let bytes = encoded(&VarInt(BLOCKS.len() as i32));
        assert!(matches!(
            Block::decode(&mut bytes.as_slice()),
            Err(Error::InvalidValue(_))
        ));
    }
}
