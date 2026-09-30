// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Chunk sections and the chunk packet, in the 26.3 wire format.
//!
//! A chunk is a column of 16x16x16 sections. Each section sends its block states and its
//! biomes (one per 4x4x4 cells) as *paletted containers*. The format was read from the
//! vanilla server's own classes; see `docs/implementation/chunk-plan.md`.

use std::io::Write;

use crate::{
    BitSet, Decode, Encode, Packet, Result, VarInt,
    block::{AIR, BlockState, CAVE_AIR, LAVA, STATE_COUNT, VOID_AIR, WATER},
};

/// Blocks in one section.
pub const SECTION_BLOCKS: usize = 16 * 16 * 16;
/// Biome cells in one section.
pub const SECTION_BIOMES: usize = 4 * 4 * 4;

/// Bits per block entry in a palette: fewer than this is padded up to it.
const MIN_INDIRECT_BLOCK_BITS: u32 = 4;
/// More than this many bits per block entry sends the global ids instead of a palette.
const MAX_INDIRECT_BLOCK_BITS: u32 = 8;
/// The same two limits for biomes.
const MIN_INDIRECT_BIOME_BITS: u32 = 1;
const MAX_INDIRECT_BIOME_BITS: u32 = 3;

/// 16x16x16 blocks and their biomes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkSection {
    blocks: Box<[BlockState; SECTION_BLOCKS]>,
    biomes: Box<[u32; SECTION_BIOMES]>,
}

impl ChunkSection {
    /// A section of one block state and one biome (its network id in the biome registry).
    pub fn filled(state: BlockState, biome: u32) -> Self {
        Self {
            blocks: Box::new([state; SECTION_BLOCKS]),
            biomes: Box::new([biome; SECTION_BIOMES]),
        }
    }

    /// The block at `x`, `y`, `z` (each 0..16) within the section.
    pub fn block(&self, x: usize, y: usize, z: usize) -> BlockState {
        self.blocks[block_index(x, y, z)]
    }

    /// Sets the block at `x`, `y`, `z` (each 0..16) within the section.
    pub fn set_block(&mut self, x: usize, y: usize, z: usize, state: BlockState) {
        self.blocks[block_index(x, y, z)] = state;
    }

    /// Sets the biome of the 4x4x4 cell holding `x`, `y`, `z` (each 0..16).
    pub fn set_biome(&mut self, x: usize, y: usize, z: usize, biome: u32) {
        self.biomes[((y >> 2) * 4 + (z >> 2)) * 4 + (x >> 2)] = biome;
    }

    fn is_air(state: BlockState) -> bool {
        let b = state.block();
        b == AIR || b == CAVE_AIR || b == VOID_AIR
    }

    /// Writes the section: counts, then the two containers.
    ///
    /// `biome_count` is the size of the biome registry sent to the client, which decides the
    /// width of a biome id when a section has too many biomes for a palette.
    pub fn encode(&self, biome_count: u32, w: &mut impl Write) -> Result<()> {
        let non_empty = self.blocks.iter().filter(|s| !Self::is_air(**s)).count();
        // ponytail: water, lava and waterlogged states only; other fluid-like blocks count as 0
        let fluids = self
            .blocks
            .iter()
            .filter(|s| {
                let b = s.block();
                b == WATER || b == LAVA || s.property("waterlogged") == Some("true")
            })
            .count();
        // a section has 4096 blocks, so both counts fit in i16
        (non_empty as i16).encode(w)?;
        (fluids as i16).encode(w)?;
        let ids: Vec<u32> = self.blocks.iter().map(|s| u32::from(s.id())).collect();
        write_paletted(
            &ids,
            MIN_INDIRECT_BLOCK_BITS,
            MAX_INDIRECT_BLOCK_BITS,
            ceil_log2(u32::from(STATE_COUNT)),
            w,
        )?;
        write_paletted(
            &*self.biomes,
            MIN_INDIRECT_BIOME_BITS,
            MAX_INDIRECT_BIOME_BITS,
            ceil_log2(biome_count),
            w,
        )
    }
}

fn block_index(x: usize, y: usize, z: usize) -> usize {
    debug_assert!(x < 16 && y < 16 && z < 16);
    (y * 16 + z) * 16 + x
}

/// The number of bits needed to tell `n` values apart.
fn ceil_log2(n: u32) -> u32 {
    if n <= 1 {
        0
    } else {
        32 - (n - 1).leading_zeros()
    }
}

/// Packs `values` into longs, `bits` per value, without a value crossing a long.
pub fn pack(values: impl IntoIterator<Item = u64>, bits: u32, count: usize) -> Vec<i64> {
    let per_long = (64 / bits) as usize;
    let mut longs = vec![0u64; count.div_ceil(per_long)];
    for (i, v) in values.into_iter().enumerate().take(count) {
        longs[i / per_long] |= v << ((i % per_long) as u32 * bits);
    }
    longs.into_iter().map(|l| l as i64).collect()
}

/// Writes one paletted container: the width in bits, the palette, then the packed entries.
///
/// One distinct value is sent as a palette of one with no entries (width 0). Up to
/// `max_indirect` bits sends a palette and indexes into it. More sends the values themselves
/// at `direct_bits`.
fn write_paletted(
    values: &[u32],
    min_indirect: u32,
    max_indirect: u32,
    direct_bits: u32,
    w: &mut impl Write,
) -> Result<()> {
    let mut palette: Vec<u32> = Vec::new();
    for v in values {
        if !palette.contains(v) {
            palette.push(*v);
            // past the largest indirect palette nothing more is learned
            if palette.len() > 1 << max_indirect {
                break;
            }
        }
    }
    if palette.len() == 1 {
        0u8.encode(w)?;
        VarInt(palette[0] as i32).encode(w)?;
        return Ok(());
    }
    let bits = ceil_log2(palette.len() as u32).max(min_indirect);
    if bits <= max_indirect {
        (bits as u8).encode(w)?;
        VarInt(palette.len() as i32).encode(w)?;
        for p in &palette {
            VarInt(*p as i32).encode(w)?;
        }
        let index = |v: &u32| palette.iter().position(|p| p == v).unwrap() as u64;
        for l in pack(values.iter().map(index), bits, values.len()) {
            l.encode(w)?;
        }
    } else {
        (direct_bits as u8).encode(w)?;
        for l in pack(
            values.iter().map(|v| u64::from(*v)),
            direct_bits,
            values.len(),
        ) {
            l.encode(w)?;
        }
    }
    Ok(())
}

/// The heightmaps the client wants, by their id in vanilla's enum.
const WORLD_SURFACE: i32 = 1;
const MOTION_BLOCKING: i32 = 4;
const MOTION_BLOCKING_NO_LEAVES: i32 = 5;

/// One heightmap: its type and the packed heights.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
#[lodeframe(crate = crate)]
pub struct Heightmap {
    /// The type id in vanilla's enum.
    pub kind: VarInt,
    /// 256 heights packed 9 bits each (for a 384-high world), seven to a long.
    pub data: Vec<i64>,
}

/// Light for a chunk. Bit `i` of a mask is the section `i - 1` counted from the bottom, so the
/// masks have one more bit below and above the chunk.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
#[lodeframe(crate = crate)]
pub struct LightData {
    /// Sections with sky light data.
    pub sky_mask: BitSet,
    /// Sections with block light data.
    pub block_mask: BitSet,
    /// Sections whose sky light is all zero.
    pub empty_sky_mask: BitSet,
    /// Sections whose block light is all zero.
    pub empty_block_mask: BitSet,
    /// One 2048-byte nibble array per bit set in `sky_mask`.
    pub sky: Vec<Vec<u8>>,
    /// One 2048-byte nibble array per bit set in `block_mask`.
    pub block: Vec<Vec<u8>>,
}

impl LightData {
    /// Full light (15) in every section of a chunk `sections` high, sky and block alike.
    pub fn full(sections: usize) -> Self {
        let lit = sections + 2;
        let mask = BitSet(vec![(1u64 << lit) - 1]);
        Self {
            sky_mask: mask.clone(),
            block_mask: mask,
            empty_sky_mask: BitSet::default(),
            empty_block_mask: BitSet::default(),
            sky: vec![vec![0xff; 2048]; lit],
            block: vec![vec![0xff; 2048]; lit],
        }
    }
}

/// A whole chunk column with its light.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Packet)]
#[lodeframe(crate = crate)]
#[packet(id = crate::ids::play::clientbound::LEVEL_CHUNK_WITH_LIGHT, state = Play, side = Clientbound)]
pub struct LevelChunkWithLight {
    /// Chunk x.
    pub x: i32,
    /// Chunk z.
    pub z: i32,
    /// The heightmaps the client uses.
    pub heightmaps: Vec<Heightmap>,
    /// The sections, bottom first, back to back.
    pub data: Vec<u8>,
    /// Block entities. Always empty for now; the element type is fixed once they exist.
    pub block_entities: Vec<u8>,
    /// Light.
    pub light: LightData,
}

impl LevelChunkWithLight {
    /// Builds the packet for a column of `sections`, bottom first, with full light.
    ///
    /// `biome_count` is the size of the biome registry sent to the client.
    // ponytail: every non-air block counts for MOTION_BLOCKING; plants and leaves are wrong
    pub fn build(x: i32, z: i32, sections: &[ChunkSection], biome_count: u32) -> Result<Self> {
        let mut data = Vec::new();
        for s in sections {
            s.encode(biome_count, &mut data)?;
        }
        let height = sections.len() * 16;
        let bits = ceil_log2(height as u32 + 1);
        // the highest non-air block of each column, counted from the bottom of the world
        let mut tops = vec![0u64; 256];
        for (i, section) in sections.iter().enumerate() {
            for y in 0..16 {
                for zz in 0..16 {
                    for xx in 0..16 {
                        if !ChunkSection::is_air(section.block(xx, y, zz)) {
                            tops[zz * 16 + xx] = (i * 16 + y + 1) as u64;
                        }
                    }
                }
            }
        }
        let packed = pack(tops.iter().copied(), bits, 256);
        let heightmaps = [WORLD_SURFACE, MOTION_BLOCKING, MOTION_BLOCKING_NO_LEAVES]
            .into_iter()
            .map(|kind| Heightmap {
                kind: VarInt(kind),
                data: packed.clone(),
            })
            .collect();
        Ok(Self {
            x,
            z,
            heightmaps,
            data,
            block_entities: Vec::new(),
            light: LightData::full(sections.len()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{BEDROCK, STONE};

    fn encoded(section: &ChunkSection, biomes: u32) -> Vec<u8> {
        let mut out = Vec::new();
        section.encode(biomes, &mut out).unwrap();
        out
    }

    #[test]
    fn pack_puts_values_low_bits_first_and_never_crosses_a_long() {
        // 4 bits: 16 to a long
        let longs = pack((0..32).map(|i| i % 16), 4, 32);
        assert_eq!(longs, [0xfedcba9876543210u64 as i64; 2]);
        // 9 bits: 7 to a long, the top bit of each long unused
        let longs = pack([1, 2, 3, 4, 5, 6, 7, 8], 9, 8);
        assert_eq!(longs.len(), 2);
        assert_eq!(
            longs[0],
            1 | 2 << 9 | 3 << 18 | 4 << 27 | 5 << 36 | 6 << 45 | 7 << 54
        );
        assert_eq!(longs[1], 8);
    }

    #[test]
    fn an_all_air_section_is_counts_and_two_single_value_containers() {
        let s = ChunkSection::filled(AIR.default_state(), 3);
        assert_eq!(encoded(&s, 67), [0, 0, 0, 0, 0, 0, 0, 3]);
    }

    #[test]
    fn two_states_use_a_four_bit_palette_and_256_longs() {
        let mut s = ChunkSection::filled(AIR.default_state(), 0);
        s.set_block(0, 0, 0, STONE.default_state());
        let out = encoded(&s, 67);
        // non-empty 1, fluids 0, bits 4, palette in first-seen order [stone, air], then 256 longs
        assert_eq!(&out[..4], [0, 1, 0, 0]);
        assert_eq!(&out[4..8], [4, 2, STONE.default_state().id() as u8, 0]);
        // block 0 is index 0 and every other block is index 1, in the nibbles of the first long
        assert_eq!(&out[8..16], 0x1111_1111_1111_1110i64.to_be_bytes());
        assert_eq!(out.len(), 8 + 256 * 8 + 2);
    }

    #[test]
    fn many_states_fall_back_to_global_ids_at_16_bits() {
        let mut s = ChunkSection::filled(AIR.default_state(), 0);
        for i in 0..300u16 {
            s.set_block(
                (i % 16) as usize,
                (i / 16 % 16) as usize,
                0,
                BlockState::from_id(i).unwrap(),
            );
        }
        let out = encoded(&s, 67);
        // bits 16, no palette, 4096 / 4 longs
        assert_eq!(out[4], 16);
        assert_eq!(out.len(), 4 + 1 + 1024 * 8 + 2);
    }

    #[test]
    fn biomes_use_a_palette_up_to_3_bits_then_global_ids() {
        let mut s = ChunkSection::filled(AIR.default_state(), 0);
        s.set_biome(0, 0, 0, 5);
        let out = encoded(&s, 67);
        // biome container: bits 1, palette [5, 0] (first seen), one long with cell 0 = index 0
        let tail = &out[out.len() - (1 + 1 + 2 + 8)..];
        assert_eq!(&tail[..4], [1, 2, 5, 0]);
        assert_eq!(&tail[4..], (-2i64).to_be_bytes());
        // nine distinct biomes need 4 bits: global ids at ceil(log2(67)) = 7 bits
        let mut s = ChunkSection::filled(AIR.default_state(), 0);
        for i in 0..9 {
            s.set_biome(i % 4 * 4, i / 4 % 4 * 4, 0, i as u32);
        }
        let out = encoded(&s, 67);
        // 64 cells at 7 bits, nine to a long
        let biome = &out[out.len() - (1 + 8 * 8)..];
        assert_eq!(biome[0], 7);
    }

    #[test]
    fn fluids_are_counted() {
        let mut s = ChunkSection::filled(AIR.default_state(), 0);
        s.set_block(0, 0, 0, WATER.default_state());
        s.set_block(1, 0, 0, LAVA.default_state());
        let out = encoded(&s, 1);
        assert_eq!(&out[..4], [0, 2, 0, 2]);
    }

    #[test]
    fn heightmaps_follow_the_top_block_of_each_column() {
        let mut bottom = ChunkSection::filled(AIR.default_state(), 0);
        for z in 0..16 {
            for x in 0..16 {
                bottom.set_block(x, 0, z, BEDROCK.default_state());
            }
        }
        bottom.set_block(2, 5, 3, STONE.default_state());
        let mut sections = vec![bottom];
        sections.extend((0..23).map(|_| ChunkSection::filled(AIR.default_state(), 0)));
        let p = LevelChunkWithLight::build(1, -2, &sections, 67).unwrap();
        assert_eq!((p.x, p.z), (1, -2));
        let kinds: Vec<i32> = p.heightmaps.iter().map(|h| h.kind.0).collect();
        assert_eq!(kinds, [1, 4, 5]);
        let longs = &p.heightmaps[0].data;
        assert_eq!(longs.len(), 37);
        let height = |x: usize, z: usize| {
            let i = z * 16 + x;
            (longs[i / 7] >> (i % 7 * 9)) & 0x1ff
        };
        assert_eq!(height(0, 0), 1); // bedrock at y index 0 -> height 1
        assert_eq!(height(2, 3), 6); // stone at y index 5 -> height 6
    }

    #[test]
    fn light_covers_the_chunk_plus_one_section_either_side() {
        let l = LightData::full(24);
        assert_eq!(l.sky.len(), 26);
        assert_eq!(l.sky_mask.0, [(1 << 26) - 1]);
        assert!(
            l.block
                .iter()
                .all(|a| a.len() == 2048 && a.iter().all(|b| *b == 0xff))
        );
    }

    #[test]
    fn the_packet_roundtrips() {
        let sections = vec![ChunkSection::filled(STONE.default_state(), 1); 2];
        let p = LevelChunkWithLight::build(0, 0, &sections, 4).unwrap();
        let mut buf = Vec::new();
        p.encode(&mut buf).unwrap();
        assert_eq!(LevelChunkWithLight::decode(&mut buf.as_slice()).unwrap(), p);
    }
}
