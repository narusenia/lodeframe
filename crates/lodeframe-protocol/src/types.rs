// SPDX-License-Identifier: Apache-2.0 OR MIT
use std::io::Write;

use crate::{Decode, Encode, Result};

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
    fn bitset_is_a_length_prefixed_long_array() {
        assert_eq!(encoded(&BitSet(vec![1])), [1, 0, 0, 0, 0, 0, 0, 0, 1]);
        roundtrip(BitSet(vec![u64::MAX, 0, 5]));
        roundtrip(BitSet::default());
    }
}
