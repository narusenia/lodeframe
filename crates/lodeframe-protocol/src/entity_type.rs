// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Entity types, from the generated registry.

use std::io::Write;

use crate::{Decode, Encode, Error, Result, VarInt};

pub use crate::generated::entity_types::*;

/// An entity type. The id is the entity type registry id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EntityType(pub(crate) u16);

impl EntityType {
    /// The entity type with registry id `id`.
    pub fn from_id(id: u16) -> Option<Self> {
        (usize::from(id) < ENTITY_TYPE_NAMES.len()).then_some(Self(id))
    }

    /// The entity type named `name`, with its namespace (`minecraft:player`).
    pub fn from_name(name: &str) -> Option<Self> {
        ENTITY_TYPE_NAMES
            .iter()
            .position(|n| *n == name)
            .map(|i| Self(i as u16))
    }

    /// The registry id.
    pub fn id(self) -> u16 {
        self.0
    }

    /// The name with its namespace.
    pub fn name(self) -> &'static str {
        ENTITY_TYPE_NAMES[usize::from(self.0)]
    }
}

impl Encode for EntityType {
    fn encode(&self, w: &mut impl Write) -> Result<()> {
        VarInt(i32::from(self.0)).encode(w)
    }
}

impl Decode for EntityType {
    fn decode(r: &mut &[u8]) -> Result<Self> {
        u16::try_from(VarInt::decode(r)?.0)
            .ok()
            .and_then(Self::from_id)
            .ok_or(Error::InvalidValue("unknown EntityType id"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::tests::{encoded, roundtrip};

    #[test]
    fn names_and_ids_agree() {
        assert_eq!(PLAYER.name(), "minecraft:player");
        assert_eq!(EntityType::from_name("minecraft:player"), Some(PLAYER));
        assert_eq!(EntityType::from_name("minecraft:nope"), None);
        for (i, name) in ENTITY_TYPE_NAMES.iter().enumerate() {
            assert_eq!(EntityType(i as u16).name(), *name);
        }
    }

    #[test]
    fn is_a_varint_and_rejects_unknown_ids() {
        roundtrip(PLAYER);
        let bytes = encoded(&VarInt(ENTITY_TYPE_NAMES.len() as i32));
        assert!(matches!(
            EntityType::decode(&mut bytes.as_slice()),
            Err(Error::InvalidValue(_))
        ));
    }
}
