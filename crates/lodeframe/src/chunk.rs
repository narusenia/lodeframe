// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Chunks, where they come from, and which ones a player should have.
//!
//! The world is a fixed overworld-sized column: `MIN_Y` to `MIN_Y + HEIGHT`.

use std::collections::{HashMap, HashSet};

use crate::protocol::{
    Result,
    block::{BEDROCK, BlockState, DIRT, GRASS_BLOCK},
    chunk::{ChunkSection, LevelChunkWithLight},
};

/// Lowest block y.
pub const MIN_Y: i32 = -64;
/// Blocks from the bottom to the top of the world.
pub const HEIGHT: i32 = 384;
/// Sections in a chunk.
pub const SECTIONS: usize = (HEIGHT / 16) as usize;

/// A chunk column's coordinates: block `x` is in chunk `x >> 4`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChunkPos {
    /// Chunk x.
    pub x: i32,
    /// Chunk z.
    pub z: i32,
}

impl ChunkPos {
    /// The chunk at `x`, `z`.
    pub const fn new(x: i32, z: i32) -> Self {
        Self { x, z }
    }

    /// How far apart two chunks are as a square ring: the larger of the two axis distances.
    pub fn distance(self, other: Self) -> u32 {
        self.x.abs_diff(other.x).max(self.z.abs_diff(other.z))
    }
}

/// A chunk column: [`SECTIONS`] sections, bottom first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    sections: Vec<ChunkSection>,
}

impl Chunk {
    /// A chunk of nothing but `state`, in `biome` (its id in the biome registry).
    pub fn filled(state: BlockState, biome: u32) -> Self {
        Self {
            sections: vec![ChunkSection::filled(state, biome); SECTIONS],
        }
    }

    /// The block at `x`, `z` (each 0..16) and world height `y`, or `None` outside the world.
    pub fn block(&self, x: usize, y: i32, z: usize) -> Option<BlockState> {
        let (section, y) = Self::locate(y)?;
        Some(self.sections[section].block(x, y, z))
    }

    /// Sets the block at `x`, `z` (each 0..16) and world height `y`. Returns `false` outside the
    /// world.
    pub fn set_block(&mut self, x: usize, y: i32, z: usize, state: BlockState) -> bool {
        let Some((section, y)) = Self::locate(y) else {
            return false;
        };
        self.sections[section].set_block(x, y, z, state);
        true
    }

    fn locate(y: i32) -> Option<(usize, usize)> {
        let rel = usize::try_from(y - MIN_Y)
            .ok()
            .filter(|r| *r < HEIGHT as usize)?;
        Some((rel / 16, rel % 16))
    }

    /// The packet that shows this chunk at `pos`, with full light.
    ///
    /// `biome_count` is the size of the `minecraft:worldgen/biome` registry sent to the client.
    pub fn to_packet(&self, pos: ChunkPos, biome_count: u32) -> Result<LevelChunkWithLight> {
        LevelChunkWithLight::build(pos.x, pos.z, &self.sections, biome_count)
    }
}

/// A source of chunks: a generator, a file format, a database. Implement this to supply your own.
///
/// `load` runs on the instance's thread, so a slow loader holds up the tick. Load ahead of time
/// or cache if yours is slow. Any `Fn(ChunkPos) -> Option<Chunk>` is a loader too.
pub trait ChunkLoader {
    /// The chunk at `pos`, or `None` if there is nothing there.
    fn load(&self, pos: ChunkPos) -> Option<Chunk>;
}

impl<F: Fn(ChunkPos) -> Option<Chunk>> ChunkLoader for F {
    fn load(&self, pos: ChunkPos) -> Option<Chunk> {
        self(pos)
    }
}

/// Flat terrain: layers of one block each, from the bottom of the world up, air above.
#[derive(Debug, Clone)]
pub struct FlatGenerator {
    layers: Vec<(BlockState, u32)>,
    biome: u32,
}

impl FlatGenerator {
    /// `layers` are `(block, thickness)` from the bottom of the world up; `biome` is an id in the
    /// biome registry.
    pub fn new(layers: Vec<(BlockState, u32)>, biome: u32) -> Self {
        Self { layers, biome }
    }
}

impl Default for FlatGenerator {
    /// Bedrock, two dirt, one grass: the surface is at y = -61.
    fn default() -> Self {
        Self::new(
            vec![
                (BEDROCK.default_state(), 1),
                (DIRT.default_state(), 2),
                (GRASS_BLOCK.default_state(), 1),
            ],
            0,
        )
    }
}

impl ChunkLoader for FlatGenerator {
    fn load(&self, _: ChunkPos) -> Option<Chunk> {
        let air = crate::protocol::block::AIR.default_state();
        let mut chunk = Chunk::filled(air, self.biome);
        let mut y = MIN_Y;
        for (state, thickness) in &self.layers {
            for _ in 0..*thickness {
                for z in 0..16 {
                    for x in 0..16 {
                        chunk.set_block(x, y, z, *state);
                    }
                }
                y += 1;
            }
        }
        Some(chunk)
    }
}

/// The chunks of one instance that are loaded, filled from a [`ChunkLoader`] on demand.
pub struct Chunks<L> {
    loader: L,
    loaded: HashMap<ChunkPos, Chunk>,
}

impl<L: ChunkLoader> Chunks<L> {
    /// An empty store over `loader`.
    pub fn new(loader: L) -> Self {
        Self {
            loader,
            loaded: HashMap::new(),
        }
    }

    /// The chunk at `pos`, loading it the first time. `None` if the loader has nothing there.
    pub fn get(&mut self, pos: ChunkPos) -> Option<&Chunk> {
        self.get_mut(pos).map(|chunk| &*chunk)
    }

    /// Like [`get`](Self::get), to change the chunk. Changes stay in memory only: the loader is
    /// not told, and they are lost when the chunk is unloaded.
    pub fn get_mut(&mut self, pos: ChunkPos) -> Option<&mut Chunk> {
        if !self.loaded.contains_key(&pos) {
            let chunk = self.loader.load(pos)?;
            self.loaded.insert(pos, chunk);
        }
        self.loaded.get_mut(&pos)
    }

    /// Drops the chunk at `pos` from memory. It is loaded again if asked for.
    pub fn unload(&mut self, pos: ChunkPos) {
        self.loaded.remove(&pos);
    }

    /// Number of chunks in memory.
    pub fn len(&self) -> usize {
        self.loaded.len()
    }

    /// Whether no chunk is in memory.
    pub fn is_empty(&self) -> bool {
        self.loaded.is_empty()
    }
}

/// What to do about one player's chunks after they moved.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChunkChanges {
    /// Chunks to send, nearest to the player first.
    pub send: Vec<ChunkPos>,
    /// Chunks the player no longer needs.
    pub unload: Vec<ChunkPos>,
}

/// The chunks one player has been sent: decides what to send and drop as they move.
///
/// A player has every chunk within `view_distance` of their own chunk, as a square.
#[derive(Debug, Clone, Default)]
pub struct ChunkTracker {
    sent: HashSet<ChunkPos>,
}

impl ChunkTracker {
    /// A player who has been sent nothing.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records that the player is at `center` with `view_distance`, and returns what changed.
    pub fn update(&mut self, center: ChunkPos, view_distance: u32) -> ChunkChanges {
        let r = view_distance as i32;
        let mut send = Vec::new();
        for z in center.z - r..=center.z + r {
            for x in center.x - r..=center.x + r {
                let pos = ChunkPos::new(x, z);
                if self.sent.insert(pos) {
                    send.push(pos);
                }
            }
        }
        send.sort_by_key(|p| (p.distance(center), p.z, p.x));
        let unload: Vec<ChunkPos> = self
            .sent
            .iter()
            .copied()
            .filter(|p| p.distance(center) > view_distance)
            .collect();
        for p in &unload {
            self.sent.remove(p);
        }
        let mut unload = unload;
        unload.sort_by_key(|p| (p.z, p.x));
        ChunkChanges { send, unload }
    }

    /// Number of chunks the player has.
    pub fn len(&self) -> usize {
        self.sent.len()
    }

    /// Whether the player has no chunks.
    pub fn is_empty(&self) -> bool {
        self.sent.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;
    use crate::protocol::block::{AIR, STONE};

    #[test]
    fn the_default_flat_world_has_grass_at_minus_61() {
        let c = FlatGenerator::default().load(ChunkPos::new(0, 0)).unwrap();
        assert_eq!(c.block(3, -64, 4), Some(BEDROCK.default_state()));
        assert_eq!(c.block(3, -62, 4), Some(DIRT.default_state()));
        assert_eq!(c.block(3, -61, 4), Some(GRASS_BLOCK.default_state()));
        assert_eq!(c.block(3, -60, 4), Some(AIR.default_state()));
        assert_eq!(c.block(0, -65, 0), None);
        assert_eq!(c.block(0, 320, 0), None);
        assert_eq!(c.block(0, 319, 0), Some(AIR.default_state()));
    }

    #[test]
    fn a_chunk_becomes_a_packet_with_every_section() {
        let c = FlatGenerator::default().load(ChunkPos::new(0, 0)).unwrap();
        let p = c.to_packet(ChunkPos::new(2, -3), 67).unwrap();
        assert_eq!((p.x, p.z), (2, -3));
        assert_eq!(p.light.sky.len(), SECTIONS + 2);
        // the surface is in section 0, so its column's height is 4 (bedrock, 2 dirt, grass)
        let first = p.heightmaps[0].data[0];
        assert_eq!(first & 0x1ff, 4);
    }

    #[test]
    fn chunks_are_loaded_once_and_again_after_unloading() {
        let loads = Cell::new(0);
        let mut chunks = Chunks::new(|_: ChunkPos| {
            loads.set(loads.get() + 1);
            Some(Chunk::filled(STONE.default_state(), 0))
        });
        assert!(chunks.get(ChunkPos::new(1, 1)).is_some());
        assert!(chunks.get(ChunkPos::new(1, 1)).is_some());
        assert_eq!((loads.get(), chunks.len()), (1, 1));
        chunks.unload(ChunkPos::new(1, 1));
        assert!(chunks.is_empty());
        chunks.get(ChunkPos::new(1, 1));
        assert_eq!(loads.get(), 2);
    }

    #[test]
    fn a_loader_with_nothing_there_gives_none() {
        let mut chunks =
            Chunks::new(|p: ChunkPos| (p.x == 0).then(|| Chunk::filled(AIR.default_state(), 0)));
        assert!(chunks.get(ChunkPos::new(0, 5)).is_some());
        assert!(chunks.get(ChunkPos::new(1, 5)).is_none());
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn the_first_update_sends_a_square_nearest_first() {
        let mut t = ChunkTracker::new();
        let c = t.update(ChunkPos::new(10, -4), 2);
        assert_eq!(c.send.len(), 25);
        assert_eq!(c.send[0], ChunkPos::new(10, -4));
        assert!(
            c.send
                .windows(2)
                .all(|w| w[0].distance(ChunkPos::new(10, -4))
                    <= w[1].distance(ChunkPos::new(10, -4)))
        );
        assert!(c.unload.is_empty());
        assert_eq!(t.len(), 25);
        // standing still changes nothing
        assert_eq!(t.update(ChunkPos::new(10, -4), 2), ChunkChanges::default());
    }

    #[test]
    fn moving_one_chunk_sends_a_new_edge_and_drops_the_old_one() {
        let mut t = ChunkTracker::new();
        t.update(ChunkPos::new(0, 0), 2);
        let c = t.update(ChunkPos::new(1, 0), 2);
        assert_eq!(c.send.len(), 5);
        assert!(c.send.iter().all(|p| p.x == 3));
        assert_eq!(c.unload.len(), 5);
        assert!(c.unload.iter().all(|p| p.x == -2));
        assert_eq!(t.len(), 25);
    }

    #[test]
    fn a_smaller_view_distance_drops_the_outer_ring() {
        let mut t = ChunkTracker::new();
        t.update(ChunkPos::new(0, 0), 3);
        let c = t.update(ChunkPos::new(0, 0), 2);
        assert!(c.send.is_empty());
        assert_eq!(c.unload.len(), 49 - 25);
    }

    #[test]
    fn a_custom_loader_can_be_a_struct() {
        struct Stone;
        impl ChunkLoader for Stone {
            fn load(&self, _: ChunkPos) -> Option<Chunk> {
                Some(Chunk::filled(STONE.default_state(), 0))
            }
        }
        let mut chunks = Chunks::new(Stone);
        assert_eq!(
            chunks.get(ChunkPos::new(0, 0)).unwrap().block(0, 0, 0),
            Some(STONE.default_state())
        );
    }
}
