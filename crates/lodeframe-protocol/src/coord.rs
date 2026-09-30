// SPDX-License-Identifier: Apache-2.0 OR MIT
use std::io::Write;

use glam::IVec3;

use crate::{Decode, Encode, Error, Result};

/// A 3D vector of `f64`: velocities, directions, offsets. This is [`glam::DVec3`],
/// so all of glam's vector math (`dot`, `length`, `normalize`, ...) is available.
pub type Vec3 = glam::DVec3;

/// x, y, z as three big-endian `f64`s.
impl Encode for Vec3 {
    fn encode(&self, w: &mut impl Write) -> Result<()> {
        self.x.encode(w)?;
        self.y.encode(w)?;
        self.z.encode(w)
    }
}

impl Decode for Vec3 {
    fn decode(r: &mut &[u8]) -> Result<Self> {
        Ok(Self::new(f64::decode(r)?, f64::decode(r)?, f64::decode(r)?))
    }
}

/// A position in the world with a view direction.
///
/// Not a wire type: packets send the coordinates and the rotation in different
/// layouts, so each packet reads and writes the fields itself.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Pos {
    /// The coordinates.
    pub coord: Vec3,
    /// Rotation around the y axis, in degrees. 0 faces south (+z).
    pub yaw: f32,
    /// Rotation up and down, in degrees. Negative looks up.
    pub pitch: f32,
}

impl Pos {
    /// A position at `x`, `y`, `z` facing yaw 0, pitch 0.
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self {
            coord: Vec3::new(x, y, z),
            yaw: 0.0,
            pitch: 0.0,
        }
    }

    /// The same position with a different view direction.
    pub const fn with_view(self, yaw: f32, pitch: f32) -> Self {
        Self { yaw, pitch, ..self }
    }

    /// The block containing this position (each coordinate rounded down, also below zero).
    pub fn block(&self) -> BlockPos {
        let b = self.coord.floor().as_ivec3();
        BlockPos::new(b.x, b.y, b.z)
    }
}

impl From<Vec3> for Pos {
    fn from(coord: Vec3) -> Self {
        Self {
            coord,
            yaw: 0.0,
            pitch: 0.0,
        }
    }
}

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

impl BlockPos {
    /// A block at `x`, `y`, `z`.
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    /// The chunk column containing this block, as `(chunk_x, chunk_z)`.
    pub const fn chunk(&self) -> (i32, i32) {
        // arithmetic shift rounds toward negative infinity
        (self.x >> 4, self.z >> 4)
    }

    /// The vertical 16-block section containing this block (`y >> 4`; `-4` for y = -64).
    pub const fn section(&self) -> i32 {
        self.y >> 4
    }

    /// This block's offset within its chunk section, each in `0..16`.
    pub const fn in_section(&self) -> (u8, u8, u8) {
        (
            (self.x & 15) as u8,
            (self.y & 15) as u8,
            (self.z & 15) as u8,
        )
    }

    /// The block's minimum corner as a vector.
    pub fn to_vec3(&self) -> Vec3 {
        IVec3::new(self.x, self.y, self.z).as_dvec3()
    }
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

    #[test]
    fn chunk_and_section_round_down_below_zero() {
        for (pos, chunk, section, local) in [
            (BlockPos::new(0, 0, 0), (0, 0), 0, (0, 0, 0)),
            (BlockPos::new(15, 15, 15), (0, 0), 0, (15, 15, 15)),
            (BlockPos::new(16, 16, 16), (1, 1), 1, (0, 0, 0)),
            (BlockPos::new(-1, -1, -1), (-1, -1), -1, (15, 15, 15)),
            (BlockPos::new(-16, -16, -16), (-1, -1), -1, (0, 0, 0)),
            (BlockPos::new(-17, -64, -17), (-2, -2), -4, (15, 0, 15)),
            (BlockPos::new(100, 319, -100), (6, -7), 19, (4, 15, 12)),
        ] {
            assert_eq!(pos.chunk(), chunk, "{pos:?}");
            assert_eq!(pos.section(), section, "{pos:?}");
            assert_eq!(pos.in_section(), local, "{pos:?}");
        }
    }

    #[test]
    fn pos_block_rounds_down_not_toward_zero() {
        assert_eq!(Pos::new(0.5, 64.9, 0.0).block(), BlockPos::new(0, 64, 0));
        assert_eq!(
            Pos::new(-0.5, -0.1, -1.0).block(),
            BlockPos::new(-1, -1, -1)
        );
        assert_eq!(
            Pos::new(-16.0, 0.0, 15.999).block(),
            BlockPos::new(-16, 0, 15)
        );
    }

    #[test]
    fn pos_builders() {
        let pos = Pos::new(1.0, 2.0, 3.0).with_view(90.0, -45.0);
        assert_eq!(
            (pos.coord, pos.yaw, pos.pitch),
            (Vec3::new(1.0, 2.0, 3.0), 90.0, -45.0)
        );
        assert_eq!(Pos::from(Vec3::ONE), Pos::new(1.0, 1.0, 1.0));
        assert_eq!(BlockPos::new(-1, 2, 3).to_vec3(), Vec3::new(-1.0, 2.0, 3.0));
    }

    #[test]
    fn vec3_is_three_big_endian_doubles() {
        let v = Vec3::new(1.0, -2.5, 0.0);
        let mut expected = Vec::new();
        1.0_f64.encode(&mut expected).unwrap();
        (-2.5_f64).encode(&mut expected).unwrap();
        0.0_f64.encode(&mut expected).unwrap();
        assert_eq!(encoded(&v), expected);
        assert_eq!(encoded(&v).len(), 24);
        roundtrip(v);
        assert!(matches!(
            Vec3::decode(&mut [0u8; 23].as_slice()),
            Err(Error::UnexpectedEof)
        ));
    }
}
