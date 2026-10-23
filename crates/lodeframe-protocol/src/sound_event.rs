// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Sound events, from the generated registry.

use std::io::Write;

use crate::{Decode, Encode, Error, Result, VarInt};

pub use crate::generated::sound_events::*;

/// A sound event, such as `entity.player.levelup`. The id is the sound event registry id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SoundEvent(pub(crate) u16);

impl SoundEvent {
    /// The sound event with registry id `id`.
    pub fn from_id(id: u16) -> Option<Self> {
        (usize::from(id) < SOUND_EVENT_NAMES.len()).then_some(Self(id))
    }

    /// The sound event named `name`, with its namespace (`minecraft:entity.player.levelup`).
    pub fn from_name(name: &str) -> Option<Self> {
        SOUND_EVENT_NAMES
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
        SOUND_EVENT_NAMES[usize::from(self.0)]
    }
}

impl Encode for SoundEvent {
    fn encode(&self, w: &mut impl Write) -> Result<()> {
        VarInt(i32::from(self.0)).encode(w)
    }
}

impl Decode for SoundEvent {
    fn decode(r: &mut &[u8]) -> Result<Self> {
        u16::try_from(VarInt::decode(r)?.0)
            .ok()
            .and_then(Self::from_id)
            .ok_or(Error::InvalidValue("unknown SoundEvent id"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::tests::{encoded, roundtrip};

    #[test]
    fn names_and_ids_agree() {
        assert_eq!(
            ENTITY_PLAYER_LEVELUP.name(),
            "minecraft:entity.player.levelup"
        );
        assert_eq!(
            SoundEvent::from_name("minecraft:entity.player.levelup"),
            Some(ENTITY_PLAYER_LEVELUP)
        );
        assert_eq!(SoundEvent::from_name("minecraft:nope"), None);
        for (i, name) in SOUND_EVENT_NAMES.iter().enumerate() {
            assert_eq!(SoundEvent(i as u16).name(), *name);
        }
    }

    #[test]
    fn is_a_varint_and_rejects_unknown_ids() {
        roundtrip(ENTITY_PLAYER_LEVELUP);
        let bytes = encoded(&VarInt(SOUND_EVENT_NAMES.len() as i32));
        assert!(matches!(
            SoundEvent::decode(&mut bytes.as_slice()),
            Err(Error::InvalidValue(_))
        ));
    }
}
