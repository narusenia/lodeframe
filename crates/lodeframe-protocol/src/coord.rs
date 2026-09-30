// SPDX-License-Identifier: Apache-2.0 OR MIT
use std::io::Write;

use crate::{Decode, Encode, Error, Result};

/// A block position, packed into one `i64` (x: 26 bits, z: 26 bits, y: 12 bits).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct BlockPos {
    /// Block x, in `-2^25..2^25`.
    pub x: i32,
    /// Block y, in `-2^11..2^11`.
    pub y: i32,
    /// Block z, in `-2^25..2^25`.
    pub z: i32,
}

impl Encode for BlockPos {
    fn encode(&self, w: &mut impl Write) -> Result<()> {
        let xz = -(1 << 25)..(1 << 25);
        if !xz.contains(&self.x)
            || !xz.contains(&self.z)
            || !(-(1 << 11)..(1 << 11)).contains(&self.y)
        {
            return Err(Error::InvalidValue("position out of range"));
        }
        let packed = ((i64::from(self.x) & 0x3ff_ffff) << 38)
            | ((i64::from(self.z) & 0x3ff_ffff) << 12)
            | (i64::from(self.y) & 0xfff);
        packed.encode(w)
    }
}

impl Decode for BlockPos {
    fn decode(r: &mut &[u8]) -> Result<Self> {
        let v = i64::decode(r)?;
        // arithmetic shifts sign-extend each field
        Ok(Self {
            x: (v >> 38) as i32,
            y: (v << 52 >> 52) as i32,
            z: (v << 26 >> 38) as i32,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::tests::{encoded, roundtrip};

    #[test]
    fn block_pos_packs_fields_in_place() {
        assert_eq!(
            encoded(&BlockPos { x: 1, y: 0, z: 0 }),
            [0, 0, 0, 0x40, 0, 0, 0, 0]
        );
        assert_eq!(
            encoded(&BlockPos { x: 0, y: 0, z: 1 }),
            [0, 0, 0, 0, 0, 0, 0x10, 0]
        );
        assert_eq!(
            encoded(&BlockPos { x: 0, y: -1, z: 0 }),
            [0, 0, 0, 0, 0, 0, 0x0f, 0xff]
        );
    }

    #[test]
    fn block_pos_roundtrips_negative_and_extreme_values() {
        for p in [
            BlockPos {
                x: -1,
                y: -1,
                z: -1,
            },
            BlockPos {
                x: 18357644,
                y: 831,
                z: -20882616,
            },
            BlockPos {
                x: -(1 << 25),
                y: -(1 << 11),
                z: -(1 << 25),
            },
            BlockPos {
                x: (1 << 25) - 1,
                y: (1 << 11) - 1,
                z: (1 << 25) - 1,
            },
        ] {
            roundtrip(p);
        }
    }

    #[test]
    fn block_pos_out_of_range_is_rejected_on_encode() {
        for p in [
            BlockPos {
                x: 1 << 25,
                y: 0,
                z: 0,
            },
            BlockPos {
                x: 0,
                y: 1 << 11,
                z: 0,
            },
            BlockPos {
                x: 0,
                y: 0,
                z: -(1 << 25) - 1,
            },
        ] {
            assert!(
                matches!(p.encode(&mut Vec::new()), Err(Error::InvalidValue(_))),
                "{p:?}"
            );
        }
    }
}
