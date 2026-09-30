// SPDX-License-Identifier: Apache-2.0 OR MIT
//! A playable world: puts players into it, follows their movement and keeps their chunks.

use std::collections::HashMap;

use crate::{
    chunk::{ChunkLoader, ChunkPos, ChunkTracker, Chunks},
    instance::{Instance, Message, Sessions},
    login::Profile,
    protocol::{
        Decode, Identifier, Packet, Result, Uuid, VarInt, Vec3, ids, packet_body,
        packets::play::{
            ChunkBatchFinished, ChunkBatchStart, ForgetLevelChunk, GameEvent,
            LEVEL_CHUNKS_LOAD_START, Login, MovePlayerPos, MovePlayerPosRot, PlayerPosition,
            SetChunkCacheCenter, SpawnInfo,
        },
        split_packet_id,
    },
    registry::Registries,
};

const DIMENSION: &str = "minecraft:overworld";
const DIMENSION_TYPES: &str = "minecraft:dimension_type";
const BIOMES: &str = "minecraft:worldgen/biome";

/// One player in a [`World`].
struct Player {
    pos: Vec3,
    chunk: ChunkPos,
    chunks: ChunkTracker,
}

/// An [`Instance`] where players stand on chunks from a [`ChunkLoader`] and walk around.
pub struct World<L> {
    /// Where new players appear.
    pub spawn: Vec3,
    /// Chunk radius sent to each player.
    pub view_distance: u32,
    chunks: Chunks<L>,
    sessions: Sessions,
    players: HashMap<Uuid, Player>,
    dimension_type: i32,
    biome_count: u32,
    next_entity_id: i32,
}

impl<L: ChunkLoader> World<L> {
    /// A world of the overworld type over `loader`. `registries` must be the ones sent to
    /// the players.
    pub fn new(registries: &Registries, loader: L) -> Self {
        let dimension_type = registries
            .network_id(DIMENSION_TYPES, DIMENSION)
            .expect("the overworld dimension type is in the registry");
        Self {
            spawn: Vec3::new(0.5, -60.0, 0.5),
            view_distance: 8,
            chunks: Chunks::new(loader),
            sessions: Sessions::default(),
            players: HashMap::new(),
            dimension_type: dimension_type as i32,
            biome_count: registries.len(BIOMES).unwrap_or(0) as u32,
            next_entity_id: 0,
        }
    }

    fn join(&mut self, profile: Profile, outbound: tokio::sync::mpsc::Sender<Vec<u8>>) {
        let id = profile.uuid;
        self.sessions.join(id, outbound);
        self.next_entity_id += 1;
        let chunk = chunk_of(self.spawn);
        let mut player = Player {
            pos: self.spawn,
            chunk,
            chunks: ChunkTracker::new(),
        };
        let ok = self.send_join(id, &mut player, self.next_entity_id).is_ok();
        if ok {
            tracing::info!(name = %profile.name, players = self.players.len() + 1, "joined");
            self.players.insert(id, player);
        } else {
            self.sessions.leave(id);
        }
    }

    fn send_join(&mut self, id: Uuid, player: &mut Player, entity_id: i32) -> Result<()> {
        let dimension = Identifier::new(DIMENSION)?;
        let login = Login {
            entity_id,
            hardcore: false,
            dimensions: vec![dimension.clone()],
            max_players: VarInt(0),
            view_distance: VarInt(self.view_distance as i32),
            simulation_distance: VarInt(self.view_distance as i32),
            reduced_debug_info: false,
            show_death_screen: true,
            limited_crafting: false,
            spawn: SpawnInfo {
                // the wire id is the registry id plus one; 0 would mean an inline value
                dimension_type: VarInt(self.dimension_type + 1),
                dimension,
                seed: 0,
                // creative: a flat lobby has nothing to survive
                game_mode: VarInt(1),
                previous_game_mode: VarInt(0),
                is_debug: false,
                is_flat: true,
                last_death: None,
                portal_cooldown: VarInt(0),
                sea_level: VarInt(63),
            },
            online_mode: false,
            enforces_secure_chat: false,
        };
        self.send(id, &login)?;
        self.send(
            id,
            &PlayerPosition {
                teleport_id: VarInt(1),
                position: player.pos,
                velocity: Vec3::ZERO,
                yaw: 0.0,
                pitch: 0.0,
                relative: 0,
            },
        )?;
        self.send(
            id,
            &GameEvent {
                event: LEVEL_CHUNKS_LOAD_START,
                value: 0.0,
            },
        )?;
        self.send(
            id,
            &SetChunkCacheCenter {
                x: VarInt(player.chunk.x),
                z: VarInt(player.chunk.z),
            },
        )?;
        let changes = player.chunks.update(player.chunk, self.view_distance);
        self.send_chunks(id, &changes.send)
    }

    /// Sends `positions` as one batch.
    fn send_chunks(&mut self, id: Uuid, positions: &[ChunkPos]) -> Result<()> {
        if positions.is_empty() {
            return Ok(());
        }
        self.send(id, &ChunkBatchStart)?;
        let mut count = 0;
        for &pos in positions {
            // ponytail: a position the loader has nothing for is skipped, not retried
            let Some(chunk) = self.chunks.get(pos) else {
                continue;
            };
            let body = packet_body(&chunk.to_packet(pos, self.biome_count)?)?;
            self.send_body(id, body)?;
            count += 1;
        }
        self.send(
            id,
            &ChunkBatchFinished {
                count: VarInt(count),
            },
        )
    }

    fn send<P: Packet + crate::protocol::Encode>(&mut self, id: Uuid, packet: &P) -> Result<()> {
        self.send_body(id, packet_body(packet)?)
    }

    fn send_body(&mut self, id: Uuid, body: Vec<u8>) -> Result<()> {
        if self.sessions.send(id, body) {
            Ok(())
        } else {
            Err(crate::protocol::Error::InvalidValue("player is gone"))
        }
    }

    fn packet(&mut self, id: Uuid, body: &[u8]) {
        if self.players.contains_key(&id) && self.on_packet(id, body).is_err() {
            self.leave(id);
        }
    }

    fn on_packet(&mut self, id: Uuid, body: &[u8]) -> Result<()> {
        let (packet_id, mut payload) = split_packet_id(body)?;
        let position = match packet_id {
            ids::play::serverbound::MOVE_PLAYER_POS => {
                MovePlayerPos::decode(&mut payload)?.position
            }
            ids::play::serverbound::MOVE_PLAYER_POS_ROT => {
                MovePlayerPosRot::decode(&mut payload)?.position
            }
            // rotation, teleport and batch answers, ...: nothing to do yet
            _ => return Ok(()),
        };
        let Some(player) = self.players.get_mut(&id) else {
            return Ok(());
        };
        player.pos = position;
        let chunk = chunk_of(position);
        if chunk == player.chunk {
            return Ok(());
        }
        player.chunk = chunk;
        let changes = player.chunks.update(chunk, self.view_distance);
        self.send(
            id,
            &SetChunkCacheCenter {
                x: VarInt(chunk.x),
                z: VarInt(chunk.z),
            },
        )?;
        for pos in &changes.unload {
            self.send(id, &ForgetLevelChunk { x: pos.x, z: pos.z })?;
        }
        // ponytail: chunks stay in memory once loaded; unload them when no player has them
        self.send_chunks(id, &changes.send)
    }

    fn leave(&mut self, id: Uuid) {
        self.players.remove(&id);
        self.sessions.leave(id);
    }
}

fn chunk_of(pos: Vec3) -> ChunkPos {
    ChunkPos::new((pos.x.floor() as i32) >> 4, (pos.z.floor() as i32) >> 4)
}

impl<L: ChunkLoader> Instance for World<L> {
    fn handle(&mut self, message: Message) {
        match message {
            Message::Join { profile, outbound } => self.join(profile, outbound),
            Message::Packet { player, body } => self.packet(player, &body),
            Message::Leave { player } => self.leave(player),
        }
    }

    fn tick(&mut self) {}
}
