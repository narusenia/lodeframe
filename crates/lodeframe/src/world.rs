// SPDX-License-Identifier: Apache-2.0 OR MIT
//! A playable world: puts players into it, follows their movement and keeps their chunks.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::{
    chunk::{ChunkLoader, ChunkPos, ChunkTracker, Chunks, HEIGHT, MIN_Y},
    event::{Event, EventNode},
    instance::{Instance, Message, Packets, Sessions},
    login::Profile,
    protocol::{
        BlockPos, BlockState, Decode, Direction, Identifier, Packet, Result, Uuid, VarInt, Vec3,
        block::{AIR, STONE},
        entity_type::PLAYER,
        ids, packet_body,
        packets::play::{
            ACTION_START_DESTROY_BLOCK, AddEntity, BlockChangedAck, BlockUpdate, Chat,
            ChunkBatchFinished, ChunkBatchStart, DisguisedChat, EntityPositionSync, FLAG_SNEAKING,
            ForgetLevelChunk, GameEvent, INPUT_SNEAK, LEVEL_CHUNKS_LOAD_START, Login,
            MovePlayerPos, MovePlayerPosRot, MovePlayerRot, MovePlayerStatusOnly, ON_GROUND,
            POSE_CROUCHING, PlayerAction, PlayerInfo, PlayerInfoAdd, PlayerInfoRemove, PlayerInput,
            PlayerPosition, RemoveEntities, RotateHead, SetChunkCacheCenter, SetEntityFlagsAndPose,
            SpawnInfo, SystemChat, UseItemOn, angle,
        },
        split_packet_id,
    },
    registry::Registries,
    text::Component,
};

const DIMENSION: &str = "minecraft:overworld";
const DIMENSION_TYPES: &str = "minecraft:dimension_type";
const BIOMES: &str = "minecraft:worldgen/biome";
const CHAT_TYPES: &str = "minecraft:chat_type";
const CHAT_TYPE: &str = "minecraft:chat";
/// The longest line the chat box takes.
const MAX_CHAT: usize = 256;

/// A player came into the world. The others have been told; the player is still getting their
/// chunks, so a message sent from a handler arrives before the ground does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerJoinEvent {
    /// Who came.
    pub player: Uuid,
    /// Their name.
    pub name: String,
}

impl Event for PlayerJoinEvent {}

/// A player left the world, or was dropped because their connection could not keep up. The
/// others have been told.
///
/// Events raised from inside a handler are not delivered (see
/// [`events_mut`](World::events_mut)), so a player dropped while one is being handled leaves
/// without this.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerLeaveEvent {
    /// Who left.
    pub player: Uuid,
    /// Their name.
    pub name: String,
}

impl Event for PlayerLeaveEvent {}

/// A player said something in the chat. Cancel it to keep it from the others, or replace
/// [`message`](Self::message) to change what they see.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatEvent {
    /// Who said it.
    pub player: Uuid,
    /// Their name, shown in front of the message.
    pub name: String,
    /// What is sent to everyone. It starts as the plain text that was typed; a handler can set
    /// a styled one.
    pub message: Component,
    cancelled: bool,
}

impl ChatEvent {
    /// Keeps the message from being sent.
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }
}

impl Event for ChatEvent {
    fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

/// A player breaks a block. Cancel it to leave the block as it is.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockBreakEvent {
    /// Who breaks it.
    pub player: Uuid,
    /// The block's position.
    pub pos: BlockPos,
    /// The state it has now.
    pub block: BlockState,
    cancelled: bool,
}

impl BlockBreakEvent {
    /// Keeps the block from breaking.
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }
}

impl Event for BlockBreakEvent {
    fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

/// A player places a block. Cancel it to place nothing, or set [`block`](Self::block) to place
/// something else.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockPlaceEvent {
    /// Who places it.
    pub player: Uuid,
    /// Where it goes: next to the block that was clicked, on the face that was clicked.
    pub pos: BlockPos,
    /// The face of the clicked block.
    pub face: Direction,
    /// What is placed. Stone to begin with: the server does not know what the player holds yet.
    pub block: BlockState,
    cancelled: bool,
}

impl BlockPlaceEvent {
    /// Keeps the block from being placed.
    pub fn cancel(&mut self) {
        self.cancelled = true;
    }
}

impl Event for BlockPlaceEvent {
    fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

/// The chunk of `pos` and the block's place in it, or `None` where the world has no block: above
/// or below its height, or too far out to be sent.
fn locate(pos: BlockPos) -> Option<(ChunkPos, usize, usize)> {
    let reach = -(1 << 25)..(1 << 25);
    let height = MIN_Y..MIN_Y + HEIGHT;
    (reach.contains(&pos.x) && reach.contains(&pos.z) && height.contains(&pos.y)).then(|| {
        (
            chunk_of_block(pos.x, pos.z),
            (pos.x & 15) as usize,
            (pos.z & 15) as usize,
        )
    })
}

fn chunk_of_block(x: i32, z: i32) -> ChunkPos {
    ChunkPos::new(x >> 4, z >> 4)
}

/// Whether the chat box would have let the player type `text`. The game itself rejects
/// anything else, so a client that sends it is not the game.
fn is_chat_line(text: &str) -> bool {
    !text.is_empty()
        && text.chars().count() <= MAX_CHAT
        && text
            .chars()
            .all(|c| c != '\u{a7}' && c >= ' ' && c != '\u{7f}')
}

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
    // chunks the player is to get but has not been sent yet, nearest first
    pending: VecDeque<ChunkPos>,
    // the players this one can see, who can see this one: their entities are spawned for each
    // other. Moves and sneaking are sent to these only.
    visible: HashSet<Uuid>,
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

    fn sneak_data(&self) -> SetEntityFlagsAndPose {
        SetEntityFlagsAndPose {
            entity_id: VarInt(self.entity_id),
            flags: if self.sneaking { FLAG_SNEAKING } else { 0 },
            pose: VarInt(if self.sneaking { POSE_CROUCHING } else { 0 }),
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
    /// The most chunks one player is sent in one tick, at least 1. A player who joins or crosses
    /// a chunk border gets the first batch at once and the rest over the next ticks, so that
    /// encoding chunks does not hold up everyone else. Each chunk is encoded once for all
    /// players, which costs about 2 ms in a release build.
    pub chunks_per_tick: usize,
    /// How far, in chunks, players see each other: those no more than this many chunks apart
    /// (the larger of the two axes) have each other's entities spawned and get each other's moves.
    /// At most [`view_distance`](Self::view_distance) counts, since there is no ground beyond it.
    /// Set it before players join.
    pub entity_view_distance: u32,
    chunks: Chunks<L>,
    // the packets of the chunks that were sent, by position; dropped when a block changes
    chunk_packets: HashMap<ChunkPos, Vec<u8>>,
    sessions: Sessions,
    players: HashMap<Uuid, Player>,
    dimension_type: i32,
    biome_count: u32,
    // the holder id of the chat type: the registry id plus one
    chat_type: i32,
    next_entity_id: i32,
    events: EventNode<World<L>>,
}

impl<L: ChunkLoader + 'static> World<L> {
    /// A world of the overworld type over `loader`. `registries` must be the ones sent to
    /// the players.
    pub fn new(registries: &Registries, loader: L) -> Self {
        let dimension_type = registries
            .network_id(DIMENSION_TYPES, DIMENSION)
            .expect("the overworld dimension type is in the registry");
        Self {
            spawn: Vec3::new(0.5, -60.0, 0.5),
            view_distance: 8,
            chunks_per_tick: 8,
            entity_view_distance: 5,
            chunks: Chunks::new(loader),
            chunk_packets: HashMap::new(),
            sessions: Sessions::default(),
            players: HashMap::new(),
            dimension_type: dimension_type as i32,
            biome_count: registries.len(BIOMES).unwrap_or(0) as u32,
            chat_type: registries
                .network_id(CHAT_TYPES, CHAT_TYPE)
                .expect("the chat type is in the registry") as i32
                + 1,
            next_entity_id: 0,
            events: EventNode::new(),
        }
    }

    /// The handlers of this world. Events are emitted on it with the world as the context.
    ///
    /// While an event is being handled this node is empty. Handlers added to it from inside a
    /// handler are lost, and so are the events raised inside a handler, such as a player
    /// leaving because a message could not be sent to them. Attach handlers before the world
    /// runs.
    pub fn events_mut(&mut self) -> &mut EventNode<Self> {
        &mut self.events
    }

    /// The block at `pos`, loading its chunk if needed. `None` outside the world's height and
    /// where the loader has no chunk.
    pub fn block(&mut self, pos: BlockPos) -> Option<BlockState> {
        let (chunk, x, z) = locate(pos)?;
        self.chunks.get(chunk)?.block(x, pos.y, z)
    }

    /// Sets the block at `pos` and shows it to everyone. Returns `false`, changing nothing,
    /// outside the world's height and where the loader has no chunk.
    ///
    /// The change lives in memory only; see [`Chunks::get_mut`].
    pub fn set_block(&mut self, pos: BlockPos, state: BlockState) -> bool {
        let Some((at, x, z)) = locate(pos) else {
            return false;
        };
        let Ok(body) = packet_body(&BlockUpdate { pos, state }) else {
            return false;
        };
        let Some(chunk) = self.chunks.get_mut(at) else {
            return false;
        };
        if !chunk.set_block(x, pos.y, z, state) {
            return false;
        }
        // the packet kept for this chunk shows the old block
        self.chunk_packets.remove(&at);
        self.send_in_view(at, body);
        true
    }

    /// Shows `message` to one player in the chat. Does nothing if they are not here.
    pub fn send_message(&mut self, player: Uuid, message: &Component) {
        if !self.players.contains_key(&player) {
            return;
        }
        let Ok(body) = packet_body(&SystemChat {
            content: message.clone(),
            overlay: false,
        }) else {
            return;
        };
        if self.send_body(player, body).is_err() {
            self.leave(player);
        }
    }

    /// Shows `message` to everyone in the chat.
    pub fn broadcast(&mut self, message: &Component) {
        let Ok(body) = packet_body(&SystemChat {
            content: message.clone(),
            overlay: false,
        }) else {
            return;
        };
        self.send_many(None, body);
    }

    fn join(&mut self, profile: Profile, outbound: tokio::sync::mpsc::Sender<Packets>) {
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
            pending: VecDeque::new(),
            visible: HashSet::new(),
        };
        if self.send_join(id, &player, entity_id).is_err() {
            self.sessions.leave(id);
            return;
        }
        player.pending = player
            .chunks
            .update(player.chunk, self.view_distance)
            .send
            .into();
        tracing::info!(name = %profile.name, players = self.players.len() + 1, "joined");
        // everyone is in everyone's tab list; entities are only spawned for those in view
        let mut tab: Vec<PlayerInfo> = self.players.iter().map(|(u, p)| p.info(*u)).collect();
        tab.push(player.info(id));
        let arrival = packet_body(&PlayerInfoAdd::new(vec![player.info(id)]));
        let range = self.entity_range();
        let near: Vec<Uuid> = self
            .players
            .iter()
            .filter(|(_, p)| p.chunk.distance(chunk) <= range)
            .map(|(u, _)| *u)
            .collect();
        self.players.insert(id, player);
        // the newcomer first: the list, then the players in view
        let shown = self.send(id, &PlayerInfoAdd::new(tab)).and_then(|()| {
            near.iter().try_for_each(|n| {
                self.show(id, *n)
                    .then_some(())
                    .ok_or(crate::protocol::Error::InvalidValue("player is gone"))
            })
        });
        if shown.is_err() {
            // nobody else has heard of the player, so there is nothing to take back
            self.players.remove(&id);
            self.sessions.leave(id);
            return;
        }
        // then the others: the list first, since a player needs an entry in it to be spawned
        if let Ok(body) = arrival {
            self.send_others(id, body);
        }
        for other in near {
            self.link(id, other);
            if !self.show(other, id) {
                self.leave(other);
            }
        }
        self.emit(&mut PlayerJoinEvent {
            player: id,
            name: profile.name,
        });
        // the chunks come last, the nearest first; `tick` sends the rest
        if self.flush_chunks(id).is_err() {
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

    /// The packet that shows the chunk at `pos`, encoded the first time and kept until a block in
    /// the chunk changes. `None` if the loader has nothing there.
    fn chunk_body(&mut self, pos: ChunkPos) -> Result<Option<Vec<u8>>> {
        if let Some(body) = self.chunk_packets.get(&pos) {
            return Ok(Some(body.clone()));
        }
        let Some(chunk) = self.chunks.get(pos) else {
            return Ok(None);
        };
        let body = packet_body(&chunk.to_packet(pos, self.biome_count)?)?;
        self.chunk_packets.insert(pos, body.clone());
        Ok(Some(body))
    }

    /// Sends the next batch of the chunks `id` is waiting for, at most `chunks_per_tick`.
    fn flush_chunks(&mut self, id: Uuid) -> Result<()> {
        let Some(player) = self.players.get_mut(&id) else {
            return Ok(());
        };
        let count = self.chunks_per_tick.max(1).min(player.pending.len());
        let batch: Vec<ChunkPos> = player.pending.drain(..count).collect();
        self.send_chunks(id, &batch)
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
            let Some(body) = self.chunk_body(pos)? else {
                continue;
            };
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
            ids::play::serverbound::CHAT => {
                let line = Chat::decode(&mut payload)?;
                return self.chat(id, line.message);
            }
            ids::play::serverbound::PLAYER_ACTION => {
                return self.player_action(id, PlayerAction::decode(&mut payload)?);
            }
            ids::play::serverbound::USE_ITEM_ON => {
                return self.place(id, UseItemOn::decode(&mut payload)?);
            }
            // commands, teleport and batch answers, ...: nothing to do yet
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
                self.send_visible(id, body);
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
        // what is no longer wanted is not sent; the rest is nearest first from the new centre
        player.pending.retain(|p| !changes.unload.contains(p));
        player.pending.extend(changes.send.iter().copied());
        player
            .pending
            .make_contiguous()
            .sort_by_key(|p| (p.distance(chunk), p.z, p.x));
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
        self.update_visibility(id);
        self.flush_chunks(id)
    }

    fn chat(&mut self, id: Uuid, text: String) -> Result<()> {
        let Some(player) = self.players.get(&id) else {
            return Ok(());
        };
        if !is_chat_line(&text) {
            tracing::debug!(name = %player.name, "ignoring a chat line the game would not send");
            return Ok(());
        }
        let name = player.name.clone();
        let mut event = ChatEvent {
            player: id,
            name: name.clone(),
            message: Component::text(text),
            cancelled: false,
        };
        if self.emit(&mut event) {
            return Ok(());
        }
        let line = DisguisedChat {
            message: event.message,
            chat_type: VarInt(self.chat_type),
            name: Component::text(name),
            target_name: None,
        };
        self.send_many(None, packet_body(&line)?);
        Ok(())
    }

    /// Runs the handlers of `event`. Returns whether it ended up cancelled.
    fn emit<E: Event>(&mut self, event: &mut E) -> bool {
        // handlers get `&mut self`, so the node is taken out of it while they run
        let mut events = std::mem::take(&mut self.events);
        let cancelled = events.emit(event, self);
        self.events = events;
        cancelled
    }

    fn player_action(&mut self, id: Uuid, action: PlayerAction) -> Result<()> {
        // creative: starting to dig breaks the block; the other actions need nothing here
        if action.action.0 != ACTION_START_DESTROY_BLOCK || !self.players.contains_key(&id) {
            return Ok(());
        }
        let pos = action.pos;
        let mut changed = false;
        if let Some(block) = self.block(pos).filter(|b| *b != AIR.default_state()) {
            let mut event = BlockBreakEvent {
                player: id,
                pos,
                block,
                cancelled: false,
            };
            changed = !self.emit(&mut event) && self.set_block(pos, AIR.default_state());
        }
        self.finish_edit(id, pos, changed, action.sequence)
    }

    fn place(&mut self, id: Uuid, click: UseItemOn) -> Result<()> {
        if !self.players.contains_key(&id) {
            return Ok(());
        }
        // ponytail: any hand places; reach, collision and what is already there are not checked
        let mut changed = false;
        let mut pos = click.pos;
        if let Some(face) = Direction::from_id(click.face.0) {
            pos = click.pos.offset(face);
            if self.block(pos).is_some() {
                let mut event = BlockPlaceEvent {
                    player: id,
                    pos,
                    face,
                    block: STONE.default_state(),
                    cancelled: false,
                };
                changed = !self.emit(&mut event) && self.set_block(pos, event.block);
            }
        }
        self.finish_edit(id, pos, changed, click.sequence)
    }

    /// Ends a block edit for `id`. If nothing changed, tells them what is at `pos`, so what their
    /// client predicted is undone; then confirms `sequence` either way.
    fn finish_edit(
        &mut self,
        id: Uuid,
        pos: BlockPos,
        changed: bool,
        sequence: VarInt,
    ) -> Result<()> {
        if !changed && let Some(state) = self.block(pos) {
            self.send(id, &BlockUpdate { pos, state })?;
        }
        self.send(id, &BlockChangedAck { sequence })
    }

    fn set_sneaking(&mut self, id: Uuid, sneaking: bool) -> Result<()> {
        let Some(player) = self.players.get_mut(&id) else {
            return Ok(());
        };
        if player.sneaking == sneaking {
            return Ok(());
        }
        player.sneaking = sneaking;
        let data = player.sneak_data();
        self.send_visible(id, packet_body(&data)?);
        Ok(())
    }

    /// How far apart, in chunks, players can be and still see each other.
    fn entity_range(&self) -> u32 {
        self.entity_view_distance.min(self.view_distance)
    }

    /// Spawns `target` for `viewer`, sitting down if `target` is. Returns whether the packets
    /// could be queued.
    fn show(&mut self, viewer: Uuid, target: Uuid) -> bool {
        let Some(t) = self.players.get(&target) else {
            return true;
        };
        let mut bodies = vec![packet_body(&t.add_entity(target))];
        if t.sneaking {
            bodies.push(packet_body(&t.sneak_data()));
        }
        bodies
            .into_iter()
            .flatten()
            .all(|body| self.sessions.send(viewer, body))
    }

    /// Removes the entity of `target` from `viewer`. Returns whether the packet could be queued.
    fn hide(&mut self, viewer: Uuid, target: Uuid) -> bool {
        let Some(t) = self.players.get(&target) else {
            return true;
        };
        match packet_body(&RemoveEntities {
            entity_ids: vec![VarInt(t.entity_id)],
        }) {
            Ok(body) => self.sessions.send(viewer, body),
            Err(_) => true,
        }
    }

    /// Records that `a` and `b` see each other.
    fn link(&mut self, a: Uuid, b: Uuid) {
        if let Some(p) = self.players.get_mut(&a) {
            p.visible.insert(b);
        }
        if let Some(p) = self.players.get_mut(&b) {
            p.visible.insert(a);
        }
    }

    /// Records that `a` and `b` no longer see each other.
    fn unlink(&mut self, a: Uuid, b: Uuid) {
        if let Some(p) = self.players.get_mut(&a) {
            p.visible.remove(&b);
        }
        if let Some(p) = self.players.get_mut(&b) {
            p.visible.remove(&a);
        }
    }

    /// Spawns and removes entities for `id` and the players that came into or went out of each
    /// other's view, after `id` moved to another chunk.
    fn update_visibility(&mut self, id: Uuid) {
        let Some(me) = self.players.get(&id) else {
            return;
        };
        let (centre, range) = (me.chunk, self.entity_range());
        let before = me.visible.clone();
        let now: HashSet<Uuid> = self
            .players
            .iter()
            .filter(|(u, p)| **u != id && p.chunk.distance(centre) <= range)
            .map(|(u, _)| *u)
            .collect();
        let mut gone = HashSet::new();
        for &other in now.difference(&before) {
            self.link(id, other);
            if !self.show(id, other) {
                gone.insert(id);
            }
            if !self.show(other, id) {
                gone.insert(other);
            }
        }
        for &other in before.difference(&now) {
            self.unlink(id, other);
            if !self.hide(id, other) {
                gone.insert(id);
            }
            if !self.hide(other, id) {
                gone.insert(other);
            }
        }
        for player in gone {
            self.leave(player);
        }
    }

    /// Sends `body` to the players who can see `id`. Players who can't take it are dropped.
    fn send_visible(&mut self, id: Uuid, body: Vec<u8>) {
        let Some(player) = self.players.get(&id) else {
            return;
        };
        let viewers: Vec<Uuid> = player.visible.iter().copied().collect();
        for viewer in viewers {
            if !self.sessions.send(viewer, body.clone()) {
                self.leave(viewer);
            }
        }
    }

    /// Sends `body` to the players who have the chunk `at` in view. Players who can't take it are
    /// dropped.
    fn send_in_view(&mut self, at: ChunkPos, body: Vec<u8>) {
        let viewers: Vec<Uuid> = self
            .players
            .iter()
            .filter(|(_, p)| p.chunk.distance(at) <= self.view_distance)
            .map(|(u, _)| *u)
            .collect();
        for viewer in viewers {
            if !self.sessions.send(viewer, body.clone()) {
                self.leave(viewer);
            }
        }
    }

    /// Sends `body` to every player but `except`. Players who can't take it are dropped.
    fn send_others(&mut self, except: Uuid, body: Vec<u8>) {
        self.send_many(Some(except), body);
    }

    /// Sends `body` to every player, or every one but `except`. Players who can't take it are
    /// dropped.
    fn send_many(&mut self, except: Option<Uuid>, body: Vec<u8>) {
        let others: Vec<Uuid> = self
            .players
            .keys()
            .copied()
            .filter(|u| Some(*u) != except)
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
        // those who could see the player lose the entity, everyone loses the list entry
        if let Ok(body) = packet_body(&RemoveEntities {
            entity_ids: vec![VarInt(player.entity_id)],
        }) {
            for viewer in &player.visible {
                if let Some(other) = self.players.get_mut(viewer) {
                    other.visible.remove(&id);
                }
                // a viewer who is gone is found when the list entry is sent
                self.sessions.send(*viewer, body.clone());
            }
        }
        if let Ok(body) = packet_body(&PlayerInfoRemove { uuids: vec![id] }) {
            self.send_others(id, body);
        }
        self.emit(&mut PlayerLeaveEvent {
            player: id,
            name: player.name,
        });
    }
}

fn chunk_of(pos: Vec3) -> ChunkPos {
    ChunkPos::new((pos.x.floor() as i32) >> 4, (pos.z.floor() as i32) >> 4)
}

impl<L: ChunkLoader + 'static> Instance for World<L> {
    fn handle(&mut self, message: Message) {
        match message {
            Message::Join { profile, outbound } => self.join(profile, outbound),
            Message::Packet { player, body } => self.packet(player, &body),
            Message::Leave { player } => self.leave(player),
        }
    }

    fn tick(&mut self) {
        let waiting: Vec<Uuid> = self
            .players
            .iter()
            .filter(|(_, p)| !p.pending.is_empty())
            .map(|(id, _)| *id)
            .collect();
        for id in waiting {
            if self.flush_chunks(id).is_err() {
                self.leave(id);
            }
        }
    }
}
