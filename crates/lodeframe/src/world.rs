// SPDX-License-Identifier: Apache-2.0 OR MIT
//! A playable world: puts players into it, follows their movement and keeps their chunks.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    time::Duration,
};

use crate::{
    chunk::{Chunk, ChunkLoader, ChunkPos, ChunkTracker, Chunks, HEIGHT, MIN_Y},
    data::Data,
    event::{ChildId, Event, EventNode, Listener, ListenerId, Parents},
    instance::{Instance, Message, Packets, Sessions},
    login::Profile,
    protocol::{
        BlockPos, BlockState, Decode, Direction, Identifier, Packet, Result, Uuid, VarInt, Vec3,
        block::{AIR, STONE},
        entity_type::PLAYER,
        ids, packet_body,
        packets::play::{
            ACTION_START_DESTROY_BLOCK, AddEntity, BlockChangedAck, BlockUpdate, Chat,
            ChunkBatchFinished, ChunkBatchStart, Disconnect, DisguisedChat, EntityPositionSync,
            FLAG_SNEAKING, ForgetLevelChunk, GameEvent, INPUT_SNEAK, LEVEL_CHUNKS_LOAD_START,
            Login, MOVE_UNITS_PER_BLOCK, MoveEntityPos, MoveEntityPosRot, MoveEntityRot,
            MovePlayerPos, MovePlayerPosRot, MovePlayerRot, MovePlayerStatusOnly, ON_GROUND,
            POSE_CROUCHING, PlayerAction, PlayerInfo, PlayerInfoAdd, PlayerInfoRemove, PlayerInput,
            PlayerPosition, RemoveEntities, RotateHead, SetChunkCacheCenter, SetEntityFlagsAndPose,
            SpawnInfo, SystemChat, UseItemOn, angle,
        },
        split_packet_id,
    },
    registry::Registries,
    schedule::{Phase, Scheduler},
    task::Tasks,
    text::Component,
};

const DIMENSION: &str = "minecraft:overworld";
const DIMENSION_TYPES: &str = "minecraft:dimension_type";
const BIOMES: &str = "minecraft:worldgen/biome";
const CHAT_TYPES: &str = "minecraft:chat_type";
const CHAT_TYPE: &str = "minecraft:chat";
/// The longest line the chat box takes.
const MAX_CHAT: usize = 256;

/// What events about one player have in common. A listener for `dyn PlayerEvent` hears all of
/// them; see [`Event::parents`].
pub trait PlayerEvent {
    /// The player it is about.
    fn player(&self) -> PlayerId;
}

/// A player came into the world. The others have been told; the player is still getting their
/// chunks, so a message sent from a handler arrives before the ground does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerJoinEvent {
    /// Who came.
    pub player: PlayerId,
    /// Their name.
    pub name: String,
}

impl PlayerEvent for PlayerJoinEvent {
    fn player(&self) -> PlayerId {
        self.player
    }
}

impl Event for PlayerJoinEvent {
    fn parents<C: 'static>(&mut self, parents: &mut Parents<'_, C>) {
        parents.visit::<dyn PlayerEvent>(self);
    }
}

/// A player left the world, or was dropped because their connection could not keep up. The
/// others have been told, and [`player`](Self::player) is already gone: [`Ctx::name`] and the
/// like find nothing for it.
///
/// A player dropped while a handler runs (a message to them could not be sent) leaves after
/// that handler is done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerLeaveEvent {
    /// Who left.
    pub player: PlayerId,
    /// Their name.
    pub name: String,
}

impl PlayerEvent for PlayerLeaveEvent {
    fn player(&self) -> PlayerId {
        self.player
    }
}

impl Event for PlayerLeaveEvent {
    fn parents<C: 'static>(&mut self, parents: &mut Parents<'_, C>) {
        parents.visit::<dyn PlayerEvent>(self);
    }
}

/// The server is shutting down. Every player is still here, so a handler can save what it
/// needs; once the handlers are done, everyone is disconnected with [`reason`](Self::reason) and
/// each leaves as usual, with a [`PlayerLeaveEvent`].
///
/// It cannot be cancelled: whether to stop is up to whoever stops the server.
#[derive(Debug, Clone, PartialEq)]
pub struct ShutdownEvent {
    /// What the players are told. It starts as "Server closed"; a handler can change it.
    pub reason: Component,
}

impl Event for ShutdownEvent {}

/// A player said something in the chat. Cancel it to keep it from the others, or replace
/// [`message`](Self::message) to change what they see.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatEvent {
    /// Who said it.
    pub player: PlayerId,
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

impl PlayerEvent for ChatEvent {
    fn player(&self) -> PlayerId {
        self.player
    }
}

impl Event for ChatEvent {
    fn is_cancelled(&self) -> bool {
        self.cancelled
    }

    fn parents<C: 'static>(&mut self, parents: &mut Parents<'_, C>) {
        parents.visit::<dyn PlayerEvent>(self);
    }
}

/// A player breaks a block. Cancel it to leave the block as it is.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockBreakEvent {
    /// Who breaks it.
    pub player: PlayerId,
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

impl PlayerEvent for BlockBreakEvent {
    fn player(&self) -> PlayerId {
        self.player
    }
}

impl Event for BlockBreakEvent {
    fn is_cancelled(&self) -> bool {
        self.cancelled
    }

    fn parents<C: 'static>(&mut self, parents: &mut Parents<'_, C>) {
        parents.visit::<dyn PlayerEvent>(self);
    }
}

/// A player places a block. Cancel it to place nothing, or set [`block`](Self::block) to place
/// something else.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockPlaceEvent {
    /// Who places it.
    pub player: PlayerId,
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

impl PlayerEvent for BlockPlaceEvent {
    fn player(&self) -> PlayerId {
        self.player
    }
}

impl Event for BlockPlaceEvent {
    fn is_cancelled(&self) -> bool {
        self.cancelled
    }

    fn parents<C: 'static>(&mut self, parents: &mut Parents<'_, C>) {
        parents.visit::<dyn PlayerEvent>(self);
    }
}

/// Names one player's stay in a [`World`]. Handlers, tasks and anything else that outlives a
/// call can keep it.
///
/// A player who leaves and comes back, with the same name or not, is a new stay with a new id:
/// the old id finds nothing in [`Ctx`] (`name` is `None`, `is_online` is `false`, nothing is
/// sent) and never reaches the player who came after.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PlayerId {
    uuid: Uuid,
    // counts the joins of the world, so that two stays of one UUID have different ids
    serial: u64,
}

impl PlayerId {
    /// The player's UUID. Two stays of the same player have the same UUID.
    pub fn uuid(&self) -> Uuid {
        self.uuid
    }
}

/// Gives a boxed loader a type of its own: `Box<dyn ChunkLoader>` is not one, since `Box<F>`
/// would clash with the impl for closures.
struct DynLoader(Box<dyn ChunkLoader>);

impl ChunkLoader for DynLoader {
    fn load(&self, pos: ChunkPos) -> Option<Chunk> {
        self.0.load(pos)
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
    serial: u64,
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
    // where the clients of those players have this one: the position sums up the offsets they
    // were sent, so it is a hair off `pos`, and never drifts since the next offset starts from it
    known_pos: Vec3,
    known_yaw: u8,
    known_pitch: u8,
    // whether this player is in `World::movers` waiting for the next tick to be sent
    moved: bool,
    data: Data,
    // the round trip of the last keep alive, none until the first is answered
    latency: Option<Duration>,
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

    /// The packets that bring the clients' picture of this player up to date, empty if nothing
    /// they show has changed. Afterwards `known_*` is what the clients have.
    fn catch_up(&mut self) -> Vec<Vec<u8>> {
        let entity_id = VarInt(self.entity_id);
        let (yaw, pitch) = (angle(self.yaw), angle(self.pitch));
        let yaw_turned = yaw != self.known_yaw;
        let turned = yaw_turned || pitch != self.known_pitch;
        let offset = |now: f64, known: f64| ((now - known) * MOVE_UNITS_PER_BLOCK).round();
        let d = [
            offset(self.pos.x, self.known_pos.x),
            offset(self.pos.y, self.known_pos.y),
            offset(self.pos.z, self.known_pos.z),
        ];
        let moved = d.iter().any(|d| *d != 0.0);
        if !moved && !turned {
            return Vec::new();
        }
        let mut out = Vec::new();
        if d.iter().any(|d| d.abs() > f64::from(i16::MAX)) {
            // too far for an offset: say where it is
            out.push(packet_body(&EntityPositionSync {
                entity_id,
                path: 0,
                position: self.pos,
                yaw: self.yaw,
                pitch: self.pitch,
                on_ground: self.on_ground,
            }));
            out.push(packet_body(&RotateHead {
                entity_id,
                head_yaw: yaw,
            }));
            self.known_pos = self.pos;
        } else {
            let [dx, dy, dz] = d.map(|d| d as i16);
            self.known_pos += Vec3::new(
                f64::from(dx) / MOVE_UNITS_PER_BLOCK,
                f64::from(dy) / MOVE_UNITS_PER_BLOCK,
                f64::from(dz) / MOVE_UNITS_PER_BLOCK,
            );
            let on_ground = self.on_ground;
            out.push(match (moved, turned) {
                (true, false) => packet_body(&MoveEntityPos {
                    entity_id,
                    on_ground,
                    dx,
                    dy,
                    dz,
                }),
                (true, true) => packet_body(&MoveEntityPosRot {
                    entity_id,
                    on_ground,
                    dx,
                    dy,
                    dz,
                    yaw,
                    pitch,
                }),
                (false, _) => packet_body(&MoveEntityRot {
                    entity_id,
                    on_ground,
                    yaw,
                    pitch,
                }),
            });
            // the head only turns with the body when the yaw changed
            if yaw_turned {
                out.push(packet_body(&RotateHead {
                    entity_id,
                    head_yaw: yaw,
                }));
            }
        }
        self.known_yaw = yaw;
        self.known_pitch = pitch;
        out.into_iter().flatten().collect()
    }

    fn add_entity(&self, uuid: Uuid) -> AddEntity {
        AddEntity {
            entity_id: VarInt(self.entity_id),
            uuid,
            kind: PLAYER,
            position: self.known_pos,
            velocity: 0,
            pitch: self.known_pitch,
            yaw: self.known_yaw,
            head_yaw: self.known_yaw,
            data: VarInt(0),
        }
    }
}

type TreeChange = Box<dyn FnOnce(&mut EventNode<Ctx>)>;

/// What a handler asked of the world while the tree was in use.
enum Deferred {
    Emit(Box<dyn FnOnce(&mut World)>),
    Tree(TreeChange),
}

/// The most deferred requests one call of [`Instance::handle`] or [`Instance::tick`] works
/// through. A handler that emits the event it handles would otherwise never let the world go.
/// Real chains are a few events long; this is far above that and far below a stall.
const MAX_DEFERRED: usize = 1000;

/// The state of a [`World`]: its chunks and its players. This is what handlers get to act on.
///
/// Players are named by [`PlayerId`]. A handler can keep one and use it later; if the player is
/// gone by then, the calls that take it find nobody and do nothing.
pub struct Ctx {
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
    chunks: Chunks<DynLoader>,
    // the packets of the chunks that were sent, by position; dropped when a block changes
    chunk_packets: HashMap<ChunkPos, Vec<u8>>,
    sessions: Sessions,
    players: HashMap<Uuid, Player>,
    dimension_type: i32,
    biome_count: u32,
    // the holder id of the chat type: the registry id plus one
    chat_type: i32,
    next_entity_id: i32,
    // the serial of the last player who joined
    last_serial: u64,
    // players who moved since the last tick, in the order they first did
    movers: Vec<Uuid>,
    // players who left since `World` last told the handlers
    departed: Vec<PlayerLeaveEvent>,
    // what handlers asked to emit or change in the tree, for `World` to do once they are done
    deferred: VecDeque<Deferred>,
    pub(crate) tasks: Tasks,
    pub(crate) scheduler: Scheduler,
    data: Data,
    // the data of players who left, until their `PlayerLeaveEvent` has been handled
    leaving: HashMap<PlayerId, Data>,
}

impl Ctx {
    fn new(registries: &Registries, loader: Box<dyn ChunkLoader>) -> Self {
        let dimension_type = registries
            .network_id(DIMENSION_TYPES, DIMENSION)
            .expect("the overworld dimension type is in the registry");
        Self {
            spawn: Vec3::new(0.5, -60.0, 0.5),
            view_distance: 8,
            chunks_per_tick: 8,
            entity_view_distance: 5,
            chunks: Chunks::new(DynLoader(loader)),
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
            last_serial: 0,
            movers: Vec::new(),
            departed: Vec::new(),
            deferred: VecDeque::new(),
            tasks: Tasks::new(),
            scheduler: Scheduler::default(),
            data: Data::default(),
            leaving: HashMap::new(),
        }
    }

    /// Emits `event` once the handler that is running is done, in the order requested and
    /// within the same tick. The handler cannot see how it ended up (cancelled or not): the
    /// tree is in use until then. Events that are about a default action should be emitted by
    /// the code that does the action instead.
    pub fn emit<E: Event>(&mut self, event: E) {
        self.deferred
            .push_back(Deferred::Emit(Box::new(move |world| {
                let mut event = event;
                world.emit(&mut event);
            })));
    }

    /// Adds a listener to the root of the tree once the handler that is running is done, so it
    /// takes effect from the next event on, not for the one being handled.
    pub fn add_listener<E: ?Sized + 'static>(&mut self, listener: Listener<E, Ctx>) -> ListenerId {
        let id = ListenerId::fresh();
        self.deferred
            .push_back(Deferred::Tree(Box::new(move |root| {
                root.insert_listener(id, listener)
            })));
        id
    }

    /// Takes a listener off the tree once the handler that is running is done.
    pub fn remove_listener(&mut self, id: ListenerId) {
        self.deferred
            .push_back(Deferred::Tree(Box::new(move |root| {
                root.remove_listener(id);
            })));
    }

    /// Attaches `node` to the root of the tree once the handler that is running is done.
    pub fn add_node(&mut self, node: EventNode<Ctx>) -> ChildId {
        self.add_node_at(node, 0)
    }

    /// Like [`add_node`](Self::add_node), with a priority among the children of the root.
    pub fn add_node_at(&mut self, node: EventNode<Ctx>, priority: i32) -> ChildId {
        let id = ChildId::fresh();
        self.deferred
            .push_back(Deferred::Tree(Box::new(move |root| {
                root.insert_child(id, node, priority)
            })));
        id
    }

    /// Detaches a child of the root once the handler that is running is done.
    pub fn remove_node(&mut self, id: ChildId) {
        self.deferred
            .push_back(Deferred::Tree(Box::new(move |root| {
                root.remove_child(id);
            })));
    }

    /// The id of the player with this UUID who is here now, or `None` if nobody is.
    pub fn player_id(&self, uuid: Uuid) -> Option<PlayerId> {
        self.players.get(&uuid).map(|p| PlayerId {
            uuid,
            serial: p.serial,
        })
    }

    /// Whether the player is still here. `false` for a player who left, even if another player
    /// with the same UUID is here now.
    pub fn is_online(&self, player: PlayerId) -> bool {
        self.resolve(player).is_some()
    }

    /// What the game attached to `player`, or `None` if they are gone. It goes with them: a
    /// player who joins later under the same name starts with nothing.
    pub fn player_data(&self, player: PlayerId) -> Option<&Data> {
        self.resolve(player).map(|p| &p.data)
    }

    /// Like [`player_data`](Self::player_data), to change.
    pub fn player_data_mut(&mut self, player: PlayerId) -> Option<&mut Data> {
        self.players
            .get_mut(&player.uuid)
            .filter(|p| p.serial == player.serial)
            .map(|p| &mut p.data)
    }

    /// The data of a player who is leaving, while the handlers of their [`PlayerLeaveEvent`]
    /// run. `None` for anyone else, and for them once those handlers are done. Take what is
    /// worth keeping (`std::mem::take` leaves an empty [`Data`] behind).
    pub fn leaving_data(&mut self, player: PlayerId) -> Option<&mut Data> {
        self.leaving.get_mut(&player)
    }

    /// What the game attached to this world. It lasts as long as the world.
    pub fn data(&self) -> &Data {
        &self.data
    }

    /// Like [`data`](Self::data), to change.
    pub fn data_mut(&mut self) -> &mut Data {
        &mut self.data
    }

    /// How long a keep alive took to be answered, the last time one was: the player's ping. `None`
    /// if they are gone or have not answered one yet, which is the first 15 seconds by default.
    pub fn ping(&self, player: PlayerId) -> Option<Duration> {
        self.resolve(player).and_then(|p| p.latency)
    }

    /// The player's name, or `None` if they are gone.
    pub fn name(&self, player: PlayerId) -> Option<&str> {
        self.resolve(player).map(|p| p.name.as_str())
    }

    /// The player this id names, if they are still here.
    fn resolve(&self, id: PlayerId) -> Option<&Player> {
        self.players.get(&id.uuid).filter(|p| p.serial == id.serial)
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

    /// Shows `message` to one player in the chat. Does nothing if they are gone.
    pub fn send_message(&mut self, player: PlayerId, message: &Component) {
        if !self.is_online(player) {
            return;
        }
        let player = player.uuid;
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

    /// Brings a player in as far as the others knowing of them. Returns the event for it, or
    /// `None` if the player could not be brought in. [`finish_join`](Self::finish_join) sends
    /// the chunks once the handlers have run.
    fn join(
        &mut self,
        profile: Profile,
        outbound: tokio::sync::mpsc::Sender<Packets>,
    ) -> Option<PlayerJoinEvent> {
        let id = profile.uuid;
        if self.players.contains_key(&id) {
            // the same player logged in again: the old connection goes, as in the game
            self.kick(id, "You logged in from another location");
        }
        self.sessions.join(id, outbound);
        self.next_entity_id += 1;
        let entity_id = self.next_entity_id;
        self.last_serial += 1;
        let serial = self.last_serial;
        let chunk = chunk_of(self.spawn);
        let mut player = Player {
            serial,
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
            known_pos: self.spawn,
            known_yaw: 0,
            known_pitch: 0,
            moved: false,
            data: Data::default(),
            latency: None,
        };
        if self.send_join(id, &player, entity_id).is_err() {
            self.sessions.leave(id);
            return None;
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
            self.show_many(id, &near)
                .then_some(())
                .ok_or(crate::protocol::Error::InvalidValue("player is gone"))
        });
        if shown.is_err() {
            // nobody else has heard of the player, so there is nothing to take back
            self.players.remove(&id);
            self.sessions.leave(id);
            return None;
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
        Some(PlayerJoinEvent {
            player: PlayerId { uuid: id, serial },
            name: profile.name,
        })
    }

    /// Sends the chunks of a player who just joined, the nearest first; `tick` sends the rest.
    fn finish_join(&mut self, id: Uuid) {
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

    /// Handles a packet that raises no event: movement and sneaking.
    fn on_packet(&mut self, id: Uuid, packet_id: i32, mut payload: &[u8]) -> Result<()> {
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
        // the others are told at the next tick, once, however many times the player moved
        if moved && !std::mem::replace(&mut player.moved, true) {
            self.movers.push(id);
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

    /// The event for a line `id` typed in the chat, or `None` if they are gone or the game would
    /// not have let them type it.
    fn chat_event(&self, id: Uuid, text: String) -> Option<ChatEvent> {
        let player = self.players.get(&id)?;
        if !is_chat_line(&text) {
            tracing::debug!(name = %player.name, "ignoring a chat line the game would not send");
            return None;
        }
        Some(ChatEvent {
            player: PlayerId {
                uuid: id,
                serial: player.serial,
            },
            name: player.name.clone(),
            message: Component::text(text),
            cancelled: false,
        })
    }

    /// Sends a chat line that no handler cancelled to everyone.
    fn say(&mut self, event: ChatEvent) -> Result<()> {
        let line = DisguisedChat {
            message: event.message,
            chat_type: VarInt(self.chat_type),
            name: Component::text(event.name),
            target_name: None,
        };
        self.send_many(None, packet_body(&line)?);
        Ok(())
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

    /// Tells `id` why they are being disconnected, then removes them as if they had left. Their
    /// connection ends once it has written the message.
    fn kick(&mut self, id: Uuid, reason: &str) {
        self.kick_with(id, Component::text(reason));
    }

    /// Like [`kick`](Self::kick), with a styled `reason`.
    fn kick_with(&mut self, id: Uuid, reason: Component) {
        if let Ok(body) = packet_body(&Disconnect { reason }) {
            self.sessions.send(id, body);
        }
        self.leave(id);
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

    /// Spawns all of `targets` for `viewer` in one message, so that a crowd does not fill the
    /// queue of a connection. Returns whether the message could be queued.
    fn show_many(&mut self, viewer: Uuid, targets: &[Uuid]) -> bool {
        let mut packets = Packets::new();
        for target in targets {
            if let Some(t) = self.players.get(target) {
                packets.extend(packet_body(&t.add_entity(*target)));
                if t.sneaking {
                    packets.extend(packet_body(&t.sneak_data()));
                }
            }
        }
        packets.is_empty() || self.sessions.send_all(viewer, packets)
    }

    /// Removes the entities of all of `targets` from `viewer` in one packet. Returns whether it
    /// could be queued.
    fn hide_many(&mut self, viewer: Uuid, targets: &[Uuid]) -> bool {
        let entity_ids: Vec<VarInt> = targets
            .iter()
            .filter_map(|t| self.players.get(t))
            .map(|t| VarInt(t.entity_id))
            .collect();
        if entity_ids.is_empty() {
            return true;
        }
        match packet_body(&RemoveEntities { entity_ids }) {
            Ok(body) => self.sessions.send(viewer, body),
            Err(_) => true,
        }
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
        let (entering, leaving): (Vec<Uuid>, Vec<Uuid>) = (
            now.difference(&before).copied().collect(),
            before.difference(&now).copied().collect(),
        );
        let mut gone = HashSet::new();
        for &other in &entering {
            self.link(id, other);
            if !self.show(other, id) {
                gone.insert(other);
            }
        }
        for &other in &leaving {
            self.unlink(id, other);
            if !self.hide(other, id) {
                gone.insert(other);
            }
        }
        // the player moving gets all of them in one message
        if !self.show_many(id, &entering) || !self.hide_many(id, &leaving) {
            gone.insert(id);
        }
        for player in gone {
            self.leave(player);
        }
    }

    /// Tells the players who can see them what the players who moved since the last tick did: one
    /// message for each of the players, with all the moves they can see in it.
    fn send_moves(&mut self) {
        let mut groups: HashMap<Uuid, Packets> = HashMap::new();
        for id in std::mem::take(&mut self.movers) {
            let Some(player) = self.players.get_mut(&id) else {
                continue;
            };
            player.moved = false;
            let bodies = player.catch_up();
            if bodies.is_empty() {
                continue;
            }
            for viewer in &player.visible {
                groups
                    .entry(*viewer)
                    .or_default()
                    .extend(bodies.iter().cloned());
            }
        }
        for (viewer, packets) in groups {
            if !self.sessions.send_all(viewer, packets) {
                self.leave(viewer);
            }
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
        let gone = PlayerId {
            uuid: id,
            serial: player.serial,
        };
        self.cancel_tasks_of(gone);
        self.leaving.insert(gone, player.data);
        // the handlers hear of it once `World` is done with what it was doing
        self.departed.push(PlayerLeaveEvent {
            player: gone,
            name: player.name,
        });
    }

    /// Sends the chunks that waiting players are due, one batch each.
    fn tick(&mut self) {
        self.send_moves();
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

fn chunk_of(pos: Vec3) -> ChunkPos {
    ChunkPos::new((pos.x.floor() as i32) >> 4, (pos.z.floor() as i32) >> 4)
}

/// An [`Instance`] where players stand on chunks from a [`ChunkLoader`] and walk around.
///
/// A world is a [`Ctx`] with the handlers that run on it, and derefs to the `Ctx`, so
/// `world.view_distance = 4` and `world.set_block(..)` work on it as they do in a handler.
pub struct World {
    ctx: Ctx,
    events: EventNode<Ctx>,
}

impl World {
    /// A world of the overworld type over `loader`. `registries` must be the ones sent to
    /// the players.
    pub fn new(registries: &Registries, loader: impl ChunkLoader + 'static) -> Self {
        Self {
            ctx: Ctx::new(registries, Box::new(loader)),
            events: EventNode::new(),
        }
    }

    /// The handlers of this world. Events are emitted on it with the [`Ctx`] as the context.
    ///
    /// A handler gets the `Ctx` only, so it cannot reach this node while it runs. Attach the
    /// handlers before the world runs.
    pub fn events_mut(&mut self) -> &mut EventNode<Ctx> {
        &mut self.events
    }

    /// Runs the handlers of `event`. Returns whether it ended up cancelled.
    fn emit<E: Event>(&mut self, event: &mut E) -> bool {
        self.events.emit(event, &mut self.ctx)
    }

    /// Tells the handlers of the players who left since the last call, including those who left
    /// because of what a handler did.
    fn flush_departures(&mut self) {
        while !self.ctx.departed.is_empty() {
            for mut event in std::mem::take(&mut self.ctx.departed) {
                self.emit(&mut event);
                self.ctx.leaving.remove(&event.player);
            }
        }
    }

    /// Does what the handlers asked for while the tree was in use, and what that leads to:
    /// departures first, then the requests in the order they were made.
    fn settle(&mut self) {
        let mut done = 0;
        loop {
            self.flush_departures();
            let Some(request) = self.ctx.deferred.pop_front() else {
                return;
            };
            if done == MAX_DEFERRED {
                tracing::error!(
                    dropped = self.ctx.deferred.len() + 1,
                    "handlers keep asking for more; dropping the rest of the requests"
                );
                self.ctx.deferred.clear();
                continue;
            }
            done += 1;
            match request {
                Deferred::Emit(emit) => emit(self),
                Deferred::Tree(change) => change(&mut self.events),
            }
        }
    }

    fn join(&mut self, profile: Profile, outbound: tokio::sync::mpsc::Sender<Packets>) {
        let joined = self.ctx.join(profile, outbound);
        // a player who was replaced by this one has left before this one is here
        self.flush_departures();
        if let Some(mut event) = joined {
            self.emit(&mut event);
            self.ctx.finish_join(event.player.uuid);
        }
    }

    fn packet(&mut self, id: Uuid, body: &[u8]) {
        if self.ctx.players.contains_key(&id) && self.on_packet(id, body).is_err() {
            self.ctx.leave(id);
        }
    }

    fn on_packet(&mut self, id: Uuid, body: &[u8]) -> Result<()> {
        let (packet_id, mut payload) = split_packet_id(body)?;
        match packet_id {
            ids::play::serverbound::CHAT => {
                let line = Chat::decode(&mut payload)?;
                self.chat(id, line.message)
            }
            ids::play::serverbound::PLAYER_ACTION => {
                self.player_action(id, PlayerAction::decode(&mut payload)?)
            }
            ids::play::serverbound::USE_ITEM_ON => self.place(id, UseItemOn::decode(&mut payload)?),
            _ => self.ctx.on_packet(id, packet_id, payload),
        }
    }

    fn chat(&mut self, id: Uuid, text: String) -> Result<()> {
        let Some(mut event) = self.ctx.chat_event(id, text) else {
            return Ok(());
        };
        if self.emit(&mut event) {
            return Ok(());
        }
        self.ctx.say(event)
    }

    fn player_action(&mut self, id: Uuid, action: PlayerAction) -> Result<()> {
        // creative: starting to dig breaks the block; the other actions need nothing here
        if action.action.0 != ACTION_START_DESTROY_BLOCK {
            return Ok(());
        }
        let Some(player) = self.ctx.player_id(id) else {
            return Ok(());
        };
        let pos = action.pos;
        let mut changed = false;
        if let Some(block) = self.ctx.block(pos).filter(|b| *b != AIR.default_state()) {
            let mut event = BlockBreakEvent {
                player,
                pos,
                block,
                cancelled: false,
            };
            changed = !self.emit(&mut event) && self.ctx.set_block(pos, AIR.default_state());
        }
        self.ctx.finish_edit(id, pos, changed, action.sequence)
    }

    fn place(&mut self, id: Uuid, click: UseItemOn) -> Result<()> {
        let Some(player) = self.ctx.player_id(id) else {
            return Ok(());
        };
        // ponytail: any hand places; reach, collision and what is already there are not checked
        let mut changed = false;
        let mut pos = click.pos;
        if let Some(face) = Direction::from_id(click.face.0) {
            pos = click.pos.offset(face);
            if self.ctx.block(pos).is_some() {
                let mut event = BlockPlaceEvent {
                    player,
                    pos,
                    face,
                    block: STONE.default_state(),
                    cancelled: false,
                };
                changed = !self.emit(&mut event) && self.ctx.set_block(pos, event.block);
            }
        }
        self.ctx.finish_edit(id, pos, changed, click.sequence)
    }
}

impl std::ops::Deref for World {
    type Target = Ctx;

    fn deref(&self) -> &Ctx {
        &self.ctx
    }
}

impl std::ops::DerefMut for World {
    fn deref_mut(&mut self) -> &mut Ctx {
        &mut self.ctx
    }
}

impl Instance for World {
    fn attach(&mut self, runtime: tokio::runtime::Handle) {
        self.ctx.tasks.attach(runtime);
    }

    fn handle(&mut self, message: Message) {
        match message {
            Message::Join { profile, outbound } => self.join(profile, outbound),
            Message::Packet { player, body } => self.packet(player, &body),
            Message::Latency { player, rtt } => {
                if let Some(p) = self.ctx.players.get_mut(&player) {
                    p.latency = Some(rtt);
                }
            }
            Message::Leave { player, outbound } => {
                // a connection that was replaced by a newer one of the same player does not
                // take the newer one with it
                if self.ctx.sessions.is_current(player, &outbound) {
                    self.ctx.leave(player);
                }
            }
        }
        self.settle();
    }

    fn shutdown(&mut self) {
        let mut event = ShutdownEvent {
            reason: Component::text("Server closed"),
        };
        self.emit(&mut event);
        // what the handlers asked for goes first, so that it can still reach the players
        self.settle();
        let players: Vec<Uuid> = self.ctx.players.keys().copied().collect();
        for id in players {
            self.ctx.kick_with(id, event.reason.clone());
        }
        self.settle();
    }

    fn tick(&mut self) {
        self.ctx.run_done_tasks();
        self.ctx.run_tasks(Phase::Start);
        self.ctx.tick();
        self.ctx.run_tasks(Phase::End);
        self.settle();
    }
}
