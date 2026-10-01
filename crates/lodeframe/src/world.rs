// SPDX-License-Identifier: Apache-2.0 OR MIT
//! A playable world: puts players into it, follows their movement and keeps their chunks.

use std::collections::HashMap;

use crate::{
    chunk::{ChunkLoader, ChunkPos, ChunkTracker, Chunks},
    instance::{Instance, Message, Sessions},
    login::Profile,
    protocol::{
        Decode, Identifier, Packet, Result, Uuid, VarInt, Vec3,
        entity_type::PLAYER,
        ids, packet_body,
        packets::play::{
            AddEntity, ChunkBatchFinished, ChunkBatchStart, EntityPositionSync, FLAG_SNEAKING,
            ForgetLevelChunk, GameEvent, INPUT_SNEAK, LEVEL_CHUNKS_LOAD_START, Login,
            MovePlayerPos, MovePlayerPosRot, MovePlayerRot, MovePlayerStatusOnly, ON_GROUND,
            POSE_CROUCHING, PlayerInfo, PlayerInfoAdd, PlayerInfoRemove, PlayerInput,
            PlayerPosition, RemoveEntities, RotateHead, SetChunkCacheCenter, SetEntityFlagsAndPose,
            SpawnInfo, angle,
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
    entity_id: i32,
    name: String,
    pos: Vec3,
    yaw: f32,
    pitch: f32,
    on_ground: bool,
    sneaking: bool,
    chunk: ChunkPos,
    chunks: ChunkTracker,
}

impl Player {
    fn info(&self, uuid: Uuid) -> PlayerInfo {
        PlayerInfo {
            uuid,
            name: self.name.clone(),
            properties: Vec::new(),
            // creative, as in `send_join`
            game_mode: VarInt(1),
            listed: true,
            latency: VarInt(0),
        }
    }

    fn add_entity(&self, uuid: Uuid) -> AddEntity {
        AddEntity {
            entity_id: VarInt(self.entity_id),
            uuid,
            kind: PLAYER,
            position: self.pos,
            velocity: 0,
            pitch: angle(self.pitch),
            yaw: angle(self.yaw),
            head_yaw: angle(self.yaw),
            data: VarInt(0),
        }
    }
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
        let entity_id = self.next_entity_id;
        let chunk = chunk_of(self.spawn);
        let mut player = Player {
            entity_id,
            name: profile.name.clone(),
            pos: self.spawn,
            yaw: 0.0,
            pitch: 0.0,
            on_ground: false,
            sneaking: false,
            chunk,
            chunks: ChunkTracker::new(),
        };
        if self.send_join(id, &player, entity_id).is_err() {
            self.sessions.leave(id);
            return;
        }
        let join_chunks = player.chunks.update(player.chunk, self.view_distance).send;
        tracing::info!(name = %profile.name, players = self.players.len() + 1, "joined");
        // the newcomer sees everyone (and themselves in the list), everyone sees the newcomer
        let mut tab: Vec<PlayerInfo> = self.players.iter().map(|(u, p)| p.info(*u)).collect();
        tab.push(player.info(id));
        let existing: Vec<AddEntity> = self.players.iter().map(|(u, p)| p.add_entity(*u)).collect();
        let arrival = [
            packet_body(&PlayerInfoAdd::new(vec![player.info(id)])),
            packet_body(&player.add_entity(id)),
        ];
        self.players.insert(id, player);
        let shown = self
            .send(id, &PlayerInfoAdd::new(tab))
            .and_then(|()| existing.iter().try_for_each(|e| self.send(id, e)));
        if shown.is_err() {
            self.leave(id);
            return;
        }
        for body in arrival.into_iter().flatten() {
            self.send_others(id, body);
        }
        // the chunks come last: encoding them takes long, and the others must not wait for it
        if self.send_chunks(id, &join_chunks).is_err() {
            self.leave(id);
        }
    }

    fn send_join(&mut self, id: Uuid, player: &Player, entity_id: i32) -> Result<()> {
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
        )
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
        // what the packet says about where the player is and which way they face
        let (position, rotation, flags) = match packet_id {
            ids::play::serverbound::MOVE_PLAYER_POS => {
                let p = MovePlayerPos::decode(&mut payload)?;
                (Some(p.position), None, p.flags)
            }
            ids::play::serverbound::MOVE_PLAYER_POS_ROT => {
                let p = MovePlayerPosRot::decode(&mut payload)?;
                (Some(p.position), Some((p.yaw, p.pitch)), p.flags)
            }
            ids::play::serverbound::MOVE_PLAYER_ROT => {
                let p = MovePlayerRot::decode(&mut payload)?;
                (None, Some((p.yaw, p.pitch)), p.flags)
            }
            ids::play::serverbound::MOVE_PLAYER_STATUS_ONLY => (
                None,
                None,
                MovePlayerStatusOnly::decode(&mut payload)?.flags,
            ),
            ids::play::serverbound::PLAYER_INPUT => {
                let sneaking = PlayerInput::decode(&mut payload)?.flags & INPUT_SNEAK != 0;
                return self.set_sneaking(id, sneaking);
            }
            // teleport and batch answers, ...: nothing to do yet
            _ => return Ok(()),
        };
        let Some(player) = self.players.get_mut(&id) else {
            return Ok(());
        };
        player.on_ground = flags & ON_GROUND != 0;
        let moved = position.is_some_and(|p| p != player.pos)
            || rotation.is_some_and(|r| r != (player.yaw, player.pitch));
        if let Some(p) = position {
            player.pos = p;
        }
        if let Some((yaw, pitch)) = rotation {
            player.yaw = yaw;
            player.pitch = pitch;
        }
        if moved {
            let sync = EntityPositionSync {
                entity_id: VarInt(player.entity_id),
                path: 0,
                position: player.pos,
                yaw: player.yaw,
                pitch: player.pitch,
                on_ground: player.on_ground,
            };
            let head = RotateHead {
                entity_id: VarInt(player.entity_id),
                head_yaw: angle(player.yaw),
            };
            // ponytail: an absolute sync per move; send deltas if bandwidth shows in M1-18
            for body in [packet_body(&sync), packet_body(&head)]
                .into_iter()
                .flatten()
            {
                self.send_others(id, body);
            }
        }
        let Some(player) = self.players.get_mut(&id) else {
            return Ok(());
        };
        let chunk = chunk_of(player.pos);
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

    fn set_sneaking(&mut self, id: Uuid, sneaking: bool) -> Result<()> {
        let Some(player) = self.players.get_mut(&id) else {
            return Ok(());
        };
        if player.sneaking == sneaking {
            return Ok(());
        }
        player.sneaking = sneaking;
        let data = SetEntityFlagsAndPose {
            entity_id: VarInt(player.entity_id),
            flags: if sneaking { FLAG_SNEAKING } else { 0 },
            pose: VarInt(if sneaking { POSE_CROUCHING } else { 0 }),
        };
        self.send_others(id, packet_body(&data)?);
        Ok(())
    }

    /// Sends `body` to every player but `except`. Players who can't take it are dropped.
    fn send_others(&mut self, except: Uuid, body: Vec<u8>) {
        let others: Vec<Uuid> = self
            .players
            .keys()
            .copied()
            .filter(|u| *u != except)
            .collect();
        for other in others {
            if !self.sessions.send(other, body.clone()) {
                self.leave(other);
            }
        }
    }

    fn leave(&mut self, id: Uuid) {
        self.sessions.leave(id);
        let Some(player) = self.players.remove(&id) else {
            return;
        };
        let gone = [
            packet_body(&RemoveEntities {
                entity_ids: vec![VarInt(player.entity_id)],
            }),
            packet_body(&PlayerInfoRemove { uuids: vec![id] }),
        ];
        for body in gone.into_iter().flatten() {
            self.send_others(id, body);
        }
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
