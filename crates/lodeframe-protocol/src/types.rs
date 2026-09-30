use std::io::Write;

use crate::{Decode, Encode, Error, Result};

/// A 128-bit UUID, sent as two big-endian longs (most significant first).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Uuid(pub u128);

impl Encode for Uuid {
    fn encode(&self, w: &mut impl Write) -> Result<()> {
        self.0.encode(w)
    }
}

impl Decode for Uuid {
    fn decode(r: &mut &[u8]) -> Result<Self> {
        u128::decode(r).map(Self)
    }
}

/// A block position, packed into one `i64` (x: 26 bits, z: 26 bits, y: 12 bits).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Position {
    /// Block x, in `-2^25..2^25`.
    pub x: i32,
    /// Block y, in `-2^11..2^11`.
    pub y: i32,
    /// Block z, in `-2^25..2^25`.
    pub z: i32,
}

impl Encode for Position {
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

impl Decode for Position {
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

/// A set of bits, sent as a length-prefixed array of longs (bit 0 is the lowest bit of the first long).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct BitSet(pub Vec<u64>);

impl Encode for BitSet {
    fn encode(&self, w: &mut impl Write) -> Result<()> {
        self.0.encode(w)
    }
}

impl Decode for BitSet {
    fn decode(r: &mut &[u8]) -> Result<Self> {
        Vec::decode(r).map(Self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::tests::{encoded, roundtrip};

    #[test]
    fn uuid_is_most_significant_long_first() {
        let uuid = Uuid(0x0011_2233_4455_6677_8899_aabb_ccdd_eeff);
        assert_eq!(
            encoded(&uuid),
            [
                0, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
                0xee, 0xff
            ]
        );
        roundtrip(uuid);
    }

    #[test]
    fn position_packs_fields_in_place() {
        assert_eq!(
            encoded(&Position { x: 1, y: 0, z: 0 }),
            [0, 0, 0, 0x40, 0, 0, 0, 0]
        );
        assert_eq!(
            encoded(&Position { x: 0, y: 0, z: 1 }),
            [0, 0, 0, 0, 0, 0, 0x10, 0]
        );
        assert_eq!(
            encoded(&Position { x: 0, y: -1, z: 0 }),
            [0, 0, 0, 0, 0, 0, 0x0f, 0xff]
        );
    }

    #[test]
    fn position_roundtrips_negative_and_extreme_values() {
        for p in [
            Position {
                x: -1,
                y: -1,
                z: -1,
            },
            Position {
                x: 18357644,
                y: 831,
                z: -20882616,
            },
            Position {
                x: -(1 << 25),
                y: -(1 << 11),
                z: -(1 << 25),
            },
            Position {
                x: (1 << 25) - 1,
                y: (1 << 11) - 1,
                z: (1 << 25) - 1,
            },
        ] {
            roundtrip(p);
        }
    }

    #[test]
    fn position_out_of_range_is_rejected_on_encode() {
        for p in [
            Position {
                x: 1 << 25,
                y: 0,
                z: 0,
            },
            Position {
                x: 0,
                y: 1 << 11,
                z: 0,
            },
            Position {
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

    #[test]
    fn bitset_is_a_length_prefixed_long_array() {
        assert_eq!(encoded(&BitSet(vec![1])), [1, 0, 0, 0, 0, 0, 0, 0, 1]);
        roundtrip(BitSet(vec![u64::MAX, 0, 5]));
        roundtrip(BitSet::default());
    }
}
