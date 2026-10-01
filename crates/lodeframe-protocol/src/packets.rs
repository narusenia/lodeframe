// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Hand-written packet definitions, grouped by connection state.

/// Handshake state.
pub mod handshake {
    use crate::{Decode, Encode, Packet, VarInt};

    /// The first packet of every connection: which version and what the client wants next.
    #[derive(Debug, Clone, PartialEq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::handshake::serverbound::INTENTION, state = Handshake, side = Serverbound)]
    pub struct Intention {
        /// The client's protocol version.
        pub protocol_version: VarInt,
        /// The address the client typed, as sent.
        pub server_address: String,
        /// The port the client typed, as sent.
        pub server_port: u16,
        /// `1` status, `2` login, `3` login after a transfer.
        pub next_state: VarInt,
    }
}

/// Status state: the server list ping.
pub mod status {
    use crate::{Decode, Encode, Packet};

    /// Asks for the server description.
    #[derive(Debug, Clone, PartialEq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::status::serverbound::STATUS_REQUEST, state = Status, side = Serverbound)]
    pub struct StatusRequest;

    /// The server description as JSON.
    #[derive(Debug, Clone, PartialEq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::status::clientbound::STATUS_RESPONSE, state = Status, side = Clientbound)]
    pub struct StatusResponse {
        /// `{"version":{..},"players":{..},"description":..}`
        pub json: String,
    }

    /// Asks the server to echo `payload`, so the client can time the round trip.
    #[derive(Debug, Clone, PartialEq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::status::serverbound::PING_REQUEST, state = Status, side = Serverbound)]
    pub struct PingRequest {
        /// Opaque to the server; usually a timestamp.
        pub payload: i64,
    }

    /// The echo of [`PingRequest`].
    #[derive(Debug, Clone, PartialEq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::status::clientbound::PONG_RESPONSE, state = Status, side = Clientbound)]
    pub struct PongResponse {
        /// The payload of the request.
        pub payload: i64,
    }
}

/// Login state.
pub mod login {
    use crate::{Decode, Encode, Packet, Uuid, VarInt};

    /// The client's name and the UUID it claims. Offline mode ignores the UUID.
    #[derive(Debug, Clone, PartialEq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::login::serverbound::HELLO, state = Login, side = Serverbound)]
    pub struct Hello {
        /// At most 16 characters.
        pub name: String,
        /// Claimed UUID.
        pub uuid: Uuid,
    }

    /// Turns compression on for every packet after this one.
    #[derive(Debug, Clone, PartialEq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::login::clientbound::LOGIN_COMPRESSION, state = Login, side = Clientbound)]
    pub struct LoginCompression {
        /// Bodies of at least this many bytes are compressed.
        pub threshold: VarInt,
    }

    /// A profile property, such as the skin textures.
    #[derive(Debug, Clone, PartialEq, Encode, Decode)]
    #[lodeframe(crate = crate)]
    pub struct ProfileProperty {
        /// Property name.
        pub name: String,
        /// Property value.
        pub value: String,
        /// Signature, when the value is signed.
        pub signature: Option<String>,
    }

    /// Accepts the player; the client then acknowledges and enters Configuration.
    #[derive(Debug, Clone, PartialEq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::login::clientbound::LOGIN_FINISHED, state = Login, side = Clientbound)]
    pub struct LoginFinished {
        /// The player's UUID.
        pub uuid: Uuid,
        /// The player's name.
        pub name: String,
        /// Profile properties.
        pub properties: Vec<ProfileProperty>,
        /// Identifies this login session (added in 26.x; the client only stores it).
        pub session_id: Uuid,
    }

    /// The client's confirmation of [`LoginFinished`]; Configuration starts after it.
    #[derive(Debug, Clone, PartialEq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::login::serverbound::LOGIN_ACKNOWLEDGED, state = Login, side = Serverbound)]
    pub struct LoginAcknowledged;
}

/// Configuration state.
pub mod configuration {
    use crate::{Decode, Encode, Identifier, Nbt, Packet, VarInt};

    /// A data pack both sides claim to have, so its data need not be sent.
    #[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
    #[lodeframe(crate = crate)]
    pub struct KnownPack {
        /// Pack namespace, `minecraft` for vanilla.
        pub namespace: String,
        /// Pack id, `core` for vanilla.
        pub id: String,
        /// Pack version; for vanilla, the game version.
        pub version: String,
    }

    /// The channel of the server brand, the name shown next to the server in the debug screen
    /// (F3).
    pub const BRAND_CHANNEL: &str = "minecraft:brand";

    /// A plugin message from the server: `data` is whatever the `channel` defines, taking the rest
    /// of the packet.
    #[derive(Debug, Clone, PartialEq, Eq, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::configuration::clientbound::CUSTOM_PAYLOAD, state = Configuration, side = Clientbound)]
    pub struct ClientboundCustomPayload {
        /// What the message is for.
        pub channel: Identifier,
        /// The message itself.
        pub data: Vec<u8>,
    }

    impl ClientboundCustomPayload {
        /// Tells the client the name of the server, on [`BRAND_CHANNEL`].
        pub fn brand(brand: &str) -> crate::Result<Self> {
            let mut data = Vec::new();
            brand.to_owned().encode(&mut data)?;
            Ok(Self {
                channel: Identifier::new(BRAND_CHANNEL)?,
                data,
            })
        }
    }

    impl Encode for ClientboundCustomPayload {
        fn encode(&self, w: &mut impl std::io::Write) -> crate::Result<()> {
            self.channel.encode(w)?;
            w.write_all(&self.data)?;
            Ok(())
        }
    }

    impl Decode for ClientboundCustomPayload {
        fn decode(r: &mut &[u8]) -> crate::Result<Self> {
            let channel = Identifier::decode(r)?;
            let data = crate::take(r, r.len())?.to_vec();
            Ok(Self { channel, data })
        }
    }

    /// The server offers its packs.
    #[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::configuration::clientbound::SELECT_KNOWN_PACKS, state = Configuration, side = Clientbound)]
    pub struct ClientboundKnownPacks {
        /// The packs the server has.
        pub packs: Vec<KnownPack>,
    }

    /// The client answers with the subset it also has.
    #[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::configuration::serverbound::SELECT_KNOWN_PACKS, state = Configuration, side = Serverbound)]
    pub struct ServerboundKnownPacks {
        /// The packs the client has.
        pub packs: Vec<KnownPack>,
    }

    /// One registry entry. `data` is omitted when the client can read it from a known pack.
    #[derive(Debug, Clone, PartialEq, Encode, Decode)]
    #[lodeframe(crate = crate)]
    pub struct RegistryEntry {
        /// Entry name.
        pub id: Identifier,
        /// Entry data, when the client does not already have it.
        pub data: Option<Nbt>,
    }

    /// The entries of one registry. Their order is the network ids.
    #[derive(Debug, Clone, PartialEq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::configuration::clientbound::REGISTRY_DATA, state = Configuration, side = Clientbound)]
    pub struct RegistryData {
        /// Registry name, such as `minecraft:dimension_type`.
        pub registry: Identifier,
        /// The entries.
        pub entries: Vec<RegistryEntry>,
    }

    /// The feature flags the server enables, such as `minecraft:vanilla`.
    #[derive(Debug, Clone, PartialEq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::configuration::clientbound::UPDATE_ENABLED_FEATURES, state = Configuration, side = Clientbound)]
    pub struct UpdateEnabledFeatures {
        /// Feature flag names.
        pub features: Vec<Identifier>,
    }

    /// One tag: a name and the ids of its entries.
    #[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
    #[lodeframe(crate = crate)]
    pub struct Tag {
        /// Tag name, such as `minecraft:is_fire`.
        pub name: Identifier,
        /// Network ids of the entries in the tag.
        pub entries: Vec<VarInt>,
    }

    /// The tags of one registry.
    #[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
    #[lodeframe(crate = crate)]
    pub struct RegistryTags {
        /// Registry name, such as `minecraft:block`.
        pub registry: Identifier,
        /// Its tags.
        pub tags: Vec<Tag>,
    }

    /// Tags for every registry that has them. The client refuses to finish configuration
    /// if a loaded registry entry refers to a tag it was not sent.
    #[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::configuration::clientbound::UPDATE_TAGS, state = Configuration, side = Clientbound)]
    pub struct UpdateTags {
        /// Registries with their tags.
        pub registries: Vec<RegistryTags>,
    }

    /// The server is done; the client answers with the serverbound one and enters Play.
    #[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::configuration::clientbound::FINISH_CONFIGURATION, state = Configuration, side = Clientbound)]
    pub struct FinishConfiguration;

    /// The client is done.
    #[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::configuration::serverbound::FINISH_CONFIGURATION, state = Configuration, side = Serverbound)]
    pub struct AckFinishConfiguration;
}

/// Play state. Only what the connection layer itself needs; the game packets come with the
/// units that use them.
pub mod play {
    use lodeframe_text::Component;

    use crate::{BlockPos, BlockState, Decode, Encode, Identifier, Packet, Uuid, VarInt, Vec3};

    /// Sent now and then; the client must answer with the same id or is timed out.
    #[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::play::clientbound::KEEP_ALIVE, state = Play, side = Clientbound)]
    pub struct KeepAlive {
        /// Echoed back by the client.
        pub id: i64,
    }

    /// The client's answer to [`KeepAlive`].
    #[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::play::serverbound::KEEP_ALIVE, state = Play, side = Serverbound)]
    pub struct KeepAliveResponse {
        /// The id of the [`KeepAlive`] being answered.
        pub id: i64,
    }

    /// A block position in a named dimension.
    #[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
    #[lodeframe(crate = crate)]
    pub struct GlobalPos {
        /// The dimension.
        pub dimension: Identifier,
        /// The block.
        pub pos: BlockPos,
    }

    /// Where a player spawns and what world they are in; part of [`Login`].
    #[derive(Debug, Clone, PartialEq, Eq, Encode, Decode)]
    #[lodeframe(crate = crate)]
    pub struct SpawnInfo {
        /// Registry id of the dimension type plus one (0 would mean an inline value).
        pub dimension_type: VarInt,
        /// Name of the dimension.
        pub dimension: Identifier,
        /// Hashed world seed, used for biome noise on the client.
        pub seed: i64,
        /// 0 survival, 1 creative, 2 adventure, 3 spectator.
        pub game_mode: VarInt,
        /// The previous game mode plus one, or 0 for none.
        pub previous_game_mode: VarInt,
        /// Debug world: the client renders it as one.
        pub is_debug: bool,
        /// Flat world: lowers the horizon and changes the void fog.
        pub is_flat: bool,
        /// Dimension and position of the last death.
        pub last_death: Option<GlobalPos>,
        /// Ticks until the player can use a portal again.
        pub portal_cooldown: VarInt,
        /// Sea level of the world.
        pub sea_level: VarInt,
    }

    /// The first Play packet: puts the client into a world.
    #[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::play::clientbound::LOGIN, state = Play, side = Clientbound)]
    pub struct Login {
        /// Entity id of the player. A plain `i32`, not a VarInt.
        pub entity_id: i32,
        /// Hardcore hearts and death screen.
        pub hardcore: bool,
        /// Names of all dimensions on the server.
        pub dimensions: Vec<Identifier>,
        /// Shown in the player list.
        pub max_players: VarInt,
        /// Chunk radius the client loads.
        pub view_distance: VarInt,
        /// Radius in which the client simulates entities.
        pub simulation_distance: VarInt,
        /// Hides coordinates and more from the debug screen.
        pub reduced_debug_info: bool,
        /// Whether death shows a respawn screen.
        pub show_death_screen: bool,
        /// Whether crafting is limited to unlocked recipes.
        pub limited_crafting: bool,
        /// World and spawn.
        pub spawn: SpawnInfo,
        /// Whether the server checks accounts.
        pub online_mode: bool,
        /// Whether chat must be signed.
        pub enforces_secure_chat: bool,
    }

    /// Moves the client. Answered with [`AcceptTeleportation`]. All fields are absolute here.
    #[derive(Debug, Clone, PartialEq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::play::clientbound::PLAYER_POSITION, state = Play, side = Clientbound)]
    pub struct PlayerPosition {
        /// Echoed in [`AcceptTeleportation`].
        pub teleport_id: VarInt,
        /// The new position.
        pub position: Vec3,
        /// The new velocity.
        pub velocity: Vec3,
        /// Degrees around the y axis.
        pub yaw: f32,
        /// Degrees up and down.
        pub pitch: f32,
        /// Bit set of fields that are relative to the current value; 0 means all absolute.
        pub relative: i32,
    }

    /// The client confirms a [`PlayerPosition`].
    #[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::play::serverbound::ACCEPT_TELEPORTATION, state = Play, side = Serverbound)]
    pub struct AcceptTeleportation {
        /// The id being confirmed.
        pub teleport_id: VarInt,
    }

    /// `GameEvent::event` that ends the "loading terrain" screen.
    pub const LEVEL_CHUNKS_LOAD_START: u8 = 13;

    /// Something that changes on the client, like the game mode or the weather.
    #[derive(Debug, Clone, PartialEq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::play::clientbound::GAME_EVENT, state = Play, side = Clientbound)]
    pub struct GameEvent {
        /// Which event.
        pub event: u8,
        /// Meaning depends on the event.
        pub value: f32,
    }

    /// The chunk the client's view is centered on.
    #[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::play::clientbound::SET_CHUNK_CACHE_CENTER, state = Play, side = Clientbound)]
    pub struct SetChunkCacheCenter {
        /// Chunk x.
        pub x: VarInt,
        /// Chunk z.
        pub z: VarInt,
    }

    /// Tells the client to drop a chunk.
    #[derive(Debug, Clone, PartialEq, Eq, Packet)]
    #[packet(id = crate::ids::play::clientbound::FORGET_LEVEL_CHUNK, state = Play, side = Clientbound)]
    #[lodeframe(crate = crate)]
    pub struct ForgetLevelChunk {
        /// Chunk x.
        pub x: i32,
        /// Chunk z.
        pub z: i32,
    }

    /// One `i64`: z in the upper 32 bits, x in the lower.
    impl Encode for ForgetLevelChunk {
        fn encode(&self, w: &mut impl std::io::Write) -> crate::Result<()> {
            ((i64::from(self.z) << 32) | i64::from(self.x as u32)).encode(w)
        }
    }

    impl Decode for ForgetLevelChunk {
        fn decode(r: &mut &[u8]) -> crate::Result<Self> {
            let v = i64::decode(r)?;
            Ok(Self {
                x: v as i32,
                z: (v >> 32) as i32,
            })
        }
    }

    /// Opens a group of chunk packets.
    #[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::play::clientbound::CHUNK_BATCH_START, state = Play, side = Clientbound)]
    pub struct ChunkBatchStart;

    /// Closes a group of chunk packets.
    #[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::play::clientbound::CHUNK_BATCH_FINISHED, state = Play, side = Clientbound)]
    pub struct ChunkBatchFinished {
        /// Chunks in the group.
        pub count: VarInt,
    }

    /// The client's answer to [`ChunkBatchFinished`].
    #[derive(Debug, Clone, PartialEq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::play::serverbound::CHUNK_BATCH_RECEIVED, state = Play, side = Serverbound)]
    pub struct ChunkBatchReceived {
        /// How many chunks per tick the client wants.
        pub chunks_per_tick: f32,
    }

    /// The client has finished loading the world.
    #[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::play::serverbound::PLAYER_LOADED, state = Play, side = Serverbound)]
    pub struct PlayerLoaded;

    /// Adds players to the tab list. The action set is fixed: add player, game mode, listed,
    /// latency.
    #[derive(Debug, Clone, PartialEq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::play::clientbound::PLAYER_INFO_UPDATE, state = Play, side = Clientbound)]
    pub struct PlayerInfoAdd {
        /// The action bit mask; [`PlayerInfoAdd::ACTIONS`] for what this type sends.
        pub actions: u8,
        /// The players.
        pub players: Vec<PlayerInfo>,
    }

    impl PlayerInfoAdd {
        /// Add player (bit 0), game mode (bit 2), listed (bit 3) and latency (bit 4).
        pub const ACTIONS: u8 = 0b1_1101;

        /// A packet that adds `players`.
        pub fn new(players: Vec<PlayerInfo>) -> Self {
            Self {
                actions: Self::ACTIONS,
                players,
            }
        }
    }

    /// One tab list entry of [`PlayerInfoAdd`].
    #[derive(Debug, Clone, PartialEq, Encode, Decode)]
    #[lodeframe(crate = crate)]
    pub struct PlayerInfo {
        /// The player.
        pub uuid: Uuid,
        /// Name shown in the list.
        pub name: String,
        /// Skin and the like.
        pub properties: Vec<super::login::ProfileProperty>,
        /// 0 survival, 1 creative, 2 adventure, 3 spectator.
        pub game_mode: VarInt,
        /// Whether the player shows in the list.
        pub listed: bool,
        /// Ping in milliseconds.
        pub latency: VarInt,
    }

    /// Removes players from the tab list.
    #[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::play::clientbound::PLAYER_INFO_REMOVE, state = Play, side = Clientbound)]
    pub struct PlayerInfoRemove {
        /// The players.
        pub uuids: Vec<Uuid>,
    }

    /// Angle in degrees as one byte, 256 steps to a turn.
    pub fn angle(degrees: f32) -> u8 {
        ((degrees * 256.0 / 360.0).floor() as i32) as u8
    }

    /// How many units of the offsets in [`MoveEntityPos`] and [`MoveEntityPosRot`] make a block.
    pub const MOVE_UNITS_PER_BLOCK: f64 = 4096.0;

    /// An entity moved a little: by `dx`, `dy`, `dz` units of 1/4096 block from where the client
    /// last had it. Larger moves need [`EntityPositionSync`].
    ///
    /// On the wire the flags byte after the entity is a bit set, of which this sends only bit 0,
    /// on the ground.
    #[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::play::clientbound::MOVE_ENTITY_POS, state = Play, side = Clientbound)]
    pub struct MoveEntityPos {
        /// The entity.
        pub entity_id: VarInt,
        /// Whether it is on the ground.
        pub on_ground: bool,
        /// Offset in x.
        pub dx: i16,
        /// Offset in y.
        pub dy: i16,
        /// Offset in z.
        pub dz: i16,
    }

    /// Like [`MoveEntityPos`], and the entity turned: `yaw` and `pitch` are as in [`angle`].
    #[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::play::clientbound::MOVE_ENTITY_POS_ROT, state = Play, side = Clientbound)]
    pub struct MoveEntityPosRot {
        /// The entity.
        pub entity_id: VarInt,
        /// Whether it is on the ground.
        pub on_ground: bool,
        /// Offset in x.
        pub dx: i16,
        /// Offset in y.
        pub dy: i16,
        /// Offset in z.
        pub dz: i16,
        /// See [`angle`]. Comes first on the wire.
        pub yaw: u8,
        /// See [`angle`].
        pub pitch: u8,
    }

    /// An entity turned without moving. The head turns with [`RotateHead`].
    #[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::play::clientbound::MOVE_ENTITY_ROT, state = Play, side = Clientbound)]
    pub struct MoveEntityRot {
        /// The entity.
        pub entity_id: VarInt,
        /// Whether it is on the ground.
        pub on_ground: bool,
        /// See [`angle`].
        pub yaw: u8,
        /// See [`angle`].
        pub pitch: u8,
    }

    /// Makes an entity appear.
    #[derive(Debug, Clone, PartialEq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::play::clientbound::ADD_ENTITY, state = Play, side = Clientbound)]
    pub struct AddEntity {
        /// Id the entity is known by from now on.
        pub entity_id: VarInt,
        /// The entity's UUID; for a player, theirs.
        pub uuid: Uuid,
        /// Kind of entity.
        pub kind: crate::EntityType,
        /// Where it is.
        pub position: Vec3,
        /// Velocity in the low-precision vector encoding. Only `0` (at rest) is supported.
        pub velocity: u8,
        /// Up and down, see [`angle`].
        pub pitch: u8,
        /// Around the y axis, see [`angle`].
        pub yaw: u8,
        /// Head yaw, see [`angle`].
        pub head_yaw: u8,
        /// Kind-specific data; 0 for a player.
        pub data: VarInt,
    }

    /// Puts an entity at an absolute position.
    #[derive(Debug, Clone, PartialEq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::play::clientbound::ENTITY_POSITION_SYNC, state = Play, side = Clientbound)]
    pub struct EntityPositionSync {
        /// The entity.
        pub entity_id: VarInt,
        /// Position path kind; 0 is a single position.
        pub path: u8,
        /// The position.
        pub position: Vec3,
        /// Degrees around the y axis.
        pub yaw: f32,
        /// Degrees up and down.
        pub pitch: f32,
        /// Whether it stands on the ground.
        pub on_ground: bool,
    }

    /// Turns an entity's head.
    #[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::play::clientbound::ROTATE_HEAD, state = Play, side = Clientbound)]
    pub struct RotateHead {
        /// The entity.
        pub entity_id: VarInt,
        /// See [`angle`].
        pub head_yaw: u8,
    }

    /// Makes entities disappear.
    #[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::play::clientbound::REMOVE_ENTITIES, state = Play, side = Clientbound)]
    pub struct RemoveEntities {
        /// The entities.
        pub entity_ids: Vec<VarInt>,
    }

    /// Entity flag bit for sneaking.
    pub const FLAG_SNEAKING: u8 = 0x02;
    /// The crouching pose.
    pub const POSE_CROUCHING: i32 = 5;

    /// The two pieces of entity metadata a sneaking player changes: the flags and the pose.
    #[derive(Debug, Clone, PartialEq, Eq, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::play::clientbound::SET_ENTITY_DATA, state = Play, side = Clientbound)]
    pub struct SetEntityFlagsAndPose {
        /// The entity.
        pub entity_id: VarInt,
        /// Entity flags, like [`FLAG_SNEAKING`].
        pub flags: u8,
        /// Pose id, like [`POSE_CROUCHING`]; 0 is standing.
        pub pose: VarInt,
    }

    /// Metadata index and serializer type of the flags (a byte) and of the pose.
    const FLAGS: [u8; 2] = [0, 0];
    const POSE: [u8; 2] = [6, 20];
    const END: u8 = 0xff;

    impl Encode for SetEntityFlagsAndPose {
        fn encode(&self, w: &mut impl std::io::Write) -> crate::Result<()> {
            self.entity_id.encode(w)?;
            w.write_all(&FLAGS)?;
            self.flags.encode(w)?;
            w.write_all(&POSE)?;
            self.pose.encode(w)?;
            END.encode(w)
        }
    }

    impl Decode for SetEntityFlagsAndPose {
        fn decode(r: &mut &[u8]) -> crate::Result<Self> {
            fn expect(r: &mut &[u8], bytes: &[u8]) -> crate::Result<()> {
                if crate::take(r, bytes.len())? == bytes {
                    Ok(())
                } else {
                    Err(crate::Error::InvalidValue("unsupported entity metadata"))
                }
            }
            let entity_id = VarInt::decode(r)?;
            expect(r, &FLAGS)?;
            let flags = u8::decode(r)?;
            expect(r, &POSE)?;
            let pose = VarInt::decode(r)?;
            expect(r, &[END])?;
            Ok(Self {
                entity_id,
                flags,
                pose,
            })
        }
    }

    /// Bit for sneaking in [`PlayerInput::flags`].
    pub const INPUT_SNEAK: u8 = 0x20;

    /// What keys the player holds (forward 1, backward 2, left 4, right 8, jump 0x10, sneak
    /// 0x20, sprint 0x40).
    #[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::play::serverbound::PLAYER_INPUT, state = Play, side = Serverbound)]
    pub struct PlayerInput {
        /// The key bits.
        pub flags: u8,
    }

    /// A line the player typed into the chat box.
    ///
    /// On the wire it also carries a timestamp, a salt, an optional signature and the last
    /// messages the client has seen. Only the text is kept: decoding reads it and ignores the
    /// rest, encoding writes an unsigned message.
    #[derive(Debug, Clone, PartialEq, Eq, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::play::serverbound::CHAT, state = Play, side = Serverbound)]
    pub struct Chat {
        /// What the player typed.
        pub message: String,
    }

    impl Encode for Chat {
        fn encode(&self, w: &mut impl std::io::Write) -> crate::Result<()> {
            self.message.encode(w)?;
            // timestamp, salt, no signature, offset, nothing acknowledged (20 bits), checksum
            w.write_all(&[0; 8 + 8 + 1 + 1 + 3 + 1])?;
            Ok(())
        }
    }

    impl Decode for Chat {
        fn decode(r: &mut &[u8]) -> crate::Result<Self> {
            Ok(Self {
                message: String::decode(r)?,
            })
        }
    }

    /// A chat line that did not come from a signed message, shown as `chat_type` formats it
    /// with `name` as the sender.
    #[derive(Debug, Clone, PartialEq, Encode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::play::clientbound::DISGUISED_CHAT, state = Play, side = Clientbound)]
    pub struct DisguisedChat {
        /// The text.
        pub message: Component,
        /// The chat type, as a holder: the registry id plus one.
        pub chat_type: VarInt,
        /// The sender's name.
        pub name: Component,
        /// The receiver's name, for private messages.
        pub target_name: Option<Component>,
    }

    /// Ends the connection, showing the player `reason`.
    #[derive(Debug, Clone, PartialEq, Encode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::play::clientbound::DISCONNECT, state = Play, side = Clientbound)]
    pub struct Disconnect {
        /// What the player is told.
        pub reason: Component,
    }

    /// A message from the server rather than a player.
    #[derive(Debug, Clone, PartialEq, Encode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::play::clientbound::SYSTEM_CHAT, state = Play, side = Clientbound)]
    pub struct SystemChat {
        /// The text.
        pub content: Component,
        /// Whether it goes above the hotbar instead of into the chat.
        pub overlay: bool,
    }

    /// [`PlayerAction::action`] when the player starts digging a block. In creative mode that
    /// already breaks it.
    pub const ACTION_START_DESTROY_BLOCK: i32 = 0;

    /// The player started or stopped something with a block or an item. Only the digging
    /// actions carry a block; for the others `pos` and `face` are zero.
    #[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::play::serverbound::PLAYER_ACTION, state = Play, side = Serverbound)]
    pub struct PlayerAction {
        /// What the player did, like [`ACTION_START_DESTROY_BLOCK`]. 26.3 numbers them 0 start
        /// digging, 1 change digging direction, 2 abort, 3 stop, 4 drop stack, 5 drop one, 6
        /// release the item in use, 7 swap hands, 8 stab.
        pub action: VarInt,
        /// The block.
        pub pos: BlockPos,
        /// The face, as a [`Direction`](crate::Direction) id.
        pub face: u8,
        /// Answered with a [`BlockChangedAck`].
        pub sequence: VarInt,
    }

    /// The player used the item in hand on a face of a block, which places a block.
    #[derive(Debug, Clone, PartialEq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::play::serverbound::USE_ITEM_ON, state = Play, side = Serverbound)]
    pub struct UseItemOn {
        /// 0 for the main hand, 1 for the off hand.
        pub hand: VarInt,
        /// The block that was clicked.
        pub pos: BlockPos,
        /// The face that was clicked, as a [`Direction`](crate::Direction) id.
        pub face: VarInt,
        /// Where on the block the click was, from its minimum corner.
        pub cursor_x: f32,
        /// See `cursor_x`.
        pub cursor_y: f32,
        /// See `cursor_x`.
        pub cursor_z: f32,
        /// Whether the click was inside the block.
        pub inside: bool,
        /// Whether the click was on the world border.
        pub world_border_hit: bool,
        /// Answered with a [`BlockChangedAck`].
        pub sequence: VarInt,
    }

    /// Changes one block.
    #[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::play::clientbound::BLOCK_UPDATE, state = Play, side = Clientbound)]
    pub struct BlockUpdate {
        /// The block.
        pub pos: BlockPos,
        /// Its new state.
        pub state: BlockState,
    }

    /// Tells the client the server is done with the changes it predicted up to `sequence`.
    /// Without it the client keeps showing what it predicted.
    #[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::play::clientbound::BLOCK_CHANGED_ACK, state = Play, side = Clientbound)]
    pub struct BlockChangedAck {
        /// The sequence of the last action that was handled.
        pub sequence: VarInt,
    }

    /// Bit 0 of the flags byte in the move packets.
    pub const ON_GROUND: u8 = 1;
    /// Bit 1 of the flags byte in the move packets.
    pub const HORIZONTAL_COLLISION: u8 = 2;

    /// The client moved.
    #[derive(Debug, Clone, PartialEq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::play::serverbound::MOVE_PLAYER_POS, state = Play, side = Serverbound)]
    pub struct MovePlayerPos {
        /// New position.
        pub position: Vec3,
        /// [`ON_GROUND`] | [`HORIZONTAL_COLLISION`].
        pub flags: u8,
    }

    /// The client moved and turned.
    #[derive(Debug, Clone, PartialEq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::play::serverbound::MOVE_PLAYER_POS_ROT, state = Play, side = Serverbound)]
    pub struct MovePlayerPosRot {
        /// New position.
        pub position: Vec3,
        /// Degrees around the y axis.
        pub yaw: f32,
        /// Degrees up and down.
        pub pitch: f32,
        /// [`ON_GROUND`] | [`HORIZONTAL_COLLISION`].
        pub flags: u8,
    }

    /// The client turned.
    #[derive(Debug, Clone, PartialEq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::play::serverbound::MOVE_PLAYER_ROT, state = Play, side = Serverbound)]
    pub struct MovePlayerRot {
        /// Degrees around the y axis.
        pub yaw: f32,
        /// Degrees up and down.
        pub pitch: f32,
        /// [`ON_GROUND`] | [`HORIZONTAL_COLLISION`].
        pub flags: u8,
    }

    /// Sent while standing still, so the server learns about ground contact.
    #[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = crate::ids::play::serverbound::MOVE_PLAYER_STATUS_ONLY, state = Play, side = Serverbound)]
    pub struct MovePlayerStatusOnly {
        /// [`ON_GROUND`] | [`HORIZONTAL_COLLISION`].
        pub flags: u8,
    }
}

#[cfg(test)]
mod tests {
    use super::{configuration::*, login::*, play::*};
    use lodeframe_text::Component;

    use crate::{Decode, Encode, Identifier, Nbt, Uuid, VarInt};

    fn roundtrip<T: Encode + Decode + PartialEq + std::fmt::Debug>(v: T) {
        let mut buf = Vec::new();
        v.encode(&mut buf).unwrap();
        assert_eq!(T::decode(&mut buf.as_slice()).unwrap(), v);
    }

    #[test]
    fn play_packets_roundtrip() {
        roundtrip(Login {
            entity_id: 1,
            hardcore: false,
            dimensions: vec![Identifier::new("minecraft:overworld").unwrap()],
            max_players: VarInt(20),
            view_distance: VarInt(10),
            simulation_distance: VarInt(10),
            reduced_debug_info: false,
            show_death_screen: true,
            limited_crafting: false,
            spawn: SpawnInfo {
                dimension_type: VarInt(1),
                dimension: Identifier::new("minecraft:overworld").unwrap(),
                seed: 0,
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
        });
        roundtrip(ForgetLevelChunk { x: -3, z: 7 });
        roundtrip(MovePlayerPosRot {
            position: crate::Vec3::new(1.0, 64.0, -2.5),
            yaw: 90.0,
            pitch: -10.0,
            flags: ON_GROUND,
        });
        // z in the upper 32 bits, x in the lower
        let mut buf = Vec::new();
        ForgetLevelChunk { x: -1, z: 2 }.encode(&mut buf).unwrap();
        assert_eq!(buf, [0, 0, 0, 2, 0xff, 0xff, 0xff, 0xff]);
    }

    /// Expected bytes come from encoding the same packets with the 26.3 server's own codecs.
    #[test]
    fn player_visibility_packets_match_the_vanilla_encoding() {
        fn bytes<T: Encode>(v: T) -> Vec<u8> {
            let mut b = Vec::new();
            v.encode(&mut b).unwrap();
            b
        }
        let uuid = Uuid(0x0102030405060708_090a0b0c0d0e0f10);
        assert_eq!(angle(45.0), 0x20);
        assert_eq!(angle(90.0), 0x40);
        assert_eq!(angle(180.0), 0x80);
        assert_eq!(
            bytes(EntityPositionSync {
                entity_id: VarInt(7),
                path: 0,
                position: crate::Vec3::new(1.5, 2.5, 3.5),
                yaw: 90.0,
                pitch: 45.0,
                on_ground: true,
            }),
            [
                7, 0, 0x3f, 0xf8, 0, 0, 0, 0, 0, 0, 0x40, 4, 0, 0, 0, 0, 0, 0, 0x40, 0xc, 0, 0, 0,
                0, 0, 0, 0x42, 0xb4, 0, 0, 0x42, 0x34, 0, 0, 1
            ]
        );
        assert_eq!(
            bytes(RemoveEntities {
                entity_ids: vec![VarInt(7), VarInt(9)]
            }),
            [2, 7, 9]
        );
        assert_eq!(
            bytes(SetEntityFlagsAndPose {
                entity_id: VarInt(7),
                flags: FLAG_SNEAKING,
                pose: VarInt(POSE_CROUCHING),
            }),
            [7, 0, 0, 2, 6, 0x14, 5, 0xff]
        );
        assert_eq!(
            bytes(PlayerInput {
                flags: INPUT_SNEAK | 1
            }),
            [0x21]
        );
        let mut expected = vec![1];
        expected.extend(uuid.0.to_be_bytes());
        assert_eq!(bytes(PlayerInfoRemove { uuids: vec![uuid] }), expected);
        // the entry layout the vanilla client decoded: name, no properties, mode, listed, latency
        let add = PlayerInfoAdd::new(vec![PlayerInfo {
            uuid,
            name: "Steve".into(),
            properties: vec![],
            game_mode: VarInt(1),
            listed: true,
            latency: VarInt(0),
        }]);
        let mut expected = vec![0x1d, 1];
        expected.extend(uuid.0.to_be_bytes());
        expected.extend([5, b'S', b't', b'e', b'v', b'e', 0, 1, 1, 0]);
        assert_eq!(bytes(add.clone()), expected);
        roundtrip(add);
        roundtrip(SetEntityFlagsAndPose {
            entity_id: VarInt(7),
            flags: 0,
            pose: VarInt(0),
        });
        roundtrip(AddEntity {
            entity_id: VarInt(7),
            uuid,
            kind: crate::entity_type::PLAYER,
            position: crate::Vec3::new(1.0, 2.0, 3.0),
            velocity: 0,
            pitch: 1,
            yaw: 2,
            head_yaw: 3,
            data: VarInt(0),
        });
    }

    #[test]
    fn login_and_configuration_packets_roundtrip() {
        roundtrip(Hello {
            name: "Notch".into(),
            uuid: Uuid::offline("Notch"),
        });
        roundtrip(LoginCompression {
            threshold: VarInt(256),
        });
        roundtrip(LoginFinished {
            uuid: Uuid::offline("Notch"),
            name: "Notch".into(),
            properties: vec![ProfileProperty {
                name: "textures".into(),
                value: "v".into(),
                signature: Some("s".into()),
            }],
            session_id: Uuid(7),
        });
        roundtrip(ClientboundKnownPacks {
            packs: vec![KnownPack {
                namespace: "minecraft".into(),
                id: "core".into(),
                version: "26.3".into(),
            }],
        });
        roundtrip(RegistryData {
            registry: Identifier::new("minecraft:dimension_type").unwrap(),
            entries: vec![
                RegistryEntry {
                    id: Identifier::new("minecraft:overworld").unwrap(),
                    data: None,
                },
                RegistryEntry {
                    id: Identifier::new("demo:sky").unwrap(),
                    data: Some(Nbt::from("x")),
                },
            ],
        });
    }

    #[test]
    fn a_registry_entry_without_data_is_just_the_name_and_a_false() {
        let mut buf = Vec::new();
        RegistryEntry {
            id: Identifier::new("a:b").unwrap(),
            data: None,
        }
        .encode(&mut buf)
        .unwrap();
        assert_eq!(buf, [3, b'a', b':', b'b', 0]);
    }

    #[test]
    fn a_chat_line_decodes_from_what_the_game_sends() {
        // from the 26.3 codec: "hi", timestamp, salt, no signature, offset 3, 1 acknowledged, checksum
        let unsigned = [
            0x02, 0x68, 0x69, 0, 0, 0, 1, 2, 3, 4, 5, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77,
            0x88, 0, 3, 1, 0, 0, 0x7f,
        ];
        assert_eq!(Chat::decode(&mut &unsigned[..]).unwrap().message, "hi");
        // a signed line has 256 more bytes after the text; only the text is read
        let mut signed = vec![0x02, 0x68, 0x69, 0, 0, 0, 0, 0, 0, 0, 1, 1];
        signed.extend([0xAB; 256]);
        assert_eq!(Chat::decode(&mut &signed[..]).unwrap().message, "hi");
    }

    #[test]
    fn an_encoded_chat_line_is_as_long_as_an_unsigned_one() {
        let mut buf = Vec::new();
        Chat {
            message: "hi".into(),
        }
        .encode(&mut buf)
        .unwrap();
        assert_eq!(buf.len(), 25);
        assert_eq!(&buf[..3], [2, b'h', b'i']);
        assert_eq!(Chat::decode(&mut buf.as_slice()).unwrap().message, "hi");
    }

    #[test]
    fn server_chat_packets_are_a_component_then_their_fields() {
        let mut buf = Vec::new();
        SystemChat {
            content: Component::text("hi"),
            overlay: false,
        }
        .encode(&mut buf)
        .unwrap();
        assert_eq!(buf, [8, 0, 2, b'h', b'i', 0]);

        let mut buf = Vec::new();
        DisguisedChat {
            message: Component::text("hi"),
            chat_type: VarInt(1),
            name: Component::text("a"),
            target_name: None,
        }
        .encode(&mut buf)
        .unwrap();
        assert_eq!(buf, [8, 0, 2, b'h', b'i', 1, 8, 0, 1, b'a', 0]);
    }

    #[test]
    fn block_edit_packets_match_the_vanilla_encoding() {
        use crate::{BlockPos, BlockState};

        // all from the 26.3 codecs; the block is (1, -61, -2), packed in 8 bytes
        let pos = [0, 0, 0, 0x7f, 0xff, 0xff, 0xef, 0xc3];

        let dig = [&[0][..], &pos, &[1, 7]].concat();
        let action = PlayerAction::decode(&mut dig.as_slice()).unwrap();
        assert_eq!(action.action.0, ACTION_START_DESTROY_BLOCK);
        assert_eq!(action.pos, BlockPos::new(1, -61, -2));
        assert_eq!((action.face, action.sequence.0), (1, 7));
        let mut buf = Vec::new();
        action.encode(&mut buf).unwrap();
        assert_eq!(buf, dig);

        let click = [
            &[0][..],
            &pos,
            &[
                1, 0x3e, 0x80, 0, 0, 0x3f, 0, 0, 0, 0x3e, 0x80, 0, 0, 1, 0, 9,
            ],
        ]
        .concat();
        let place = UseItemOn::decode(&mut click.as_slice()).unwrap();
        assert_eq!((place.hand.0, place.face.0, place.sequence.0), (0, 1, 9));
        assert_eq!(place.pos, BlockPos::new(1, -61, -2));
        assert_eq!(
            (place.cursor_x, place.cursor_y, place.cursor_z),
            (0.25, 0.5, 0.25)
        );
        assert!(place.inside && !place.world_border_hit);
        let mut buf = Vec::new();
        place.encode(&mut buf).unwrap();
        assert_eq!(buf, click);

        // (1, -60, -2) is stone, state 1
        let update = [0, 0, 0, 0x7f, 0xff, 0xff, 0xef, 0xc4, 1];
        let mut buf = Vec::new();
        BlockUpdate {
            pos: BlockPos::new(1, -60, -2),
            state: BlockState::from_id(1).unwrap(),
        }
        .encode(&mut buf)
        .unwrap();
        assert_eq!(buf, update);

        let mut buf = Vec::new();
        BlockChangedAck {
            sequence: VarInt(300),
        }
        .encode(&mut buf)
        .unwrap();
        assert_eq!(buf, [0xac, 0x02]);
    }

    #[test]
    fn the_brand_payload_matches_the_vanilla_encoding() {
        // from the 26.3 codec: the channel, then the brand as a string
        let expected = [&[0x0f][..], b"minecraft:brand", &[0x09], b"Lodeframe"].concat();
        let payload = ClientboundCustomPayload::brand("Lodeframe").unwrap();
        let mut buf = Vec::new();
        payload.encode(&mut buf).unwrap();
        assert_eq!(buf, expected);
        let back = ClientboundCustomPayload::decode(&mut buf.as_slice()).unwrap();
        assert_eq!(back, payload);
        assert_eq!(back.channel, Identifier::new(BRAND_CHANNEL).unwrap());
    }

    #[test]
    fn relative_move_packets_match_the_vanilla_encoding() {
        // all from the 26.3 codecs: entity 300, offsets (4096, -4096, 1), angles 64 and -32
        let pos = MoveEntityPos {
            entity_id: VarInt(300),
            on_ground: true,
            dx: 4096,
            dy: -4096,
            dz: 1,
        };
        let mut buf = Vec::new();
        pos.encode(&mut buf).unwrap();
        assert_eq!(buf, [0xac, 0x02, 0x01, 0x10, 0x00, 0xf0, 0x00, 0x00, 0x01]);
        assert_eq!(MoveEntityPos::decode(&mut buf.as_slice()).unwrap(), pos);

        let pos_rot = MoveEntityPosRot {
            entity_id: VarInt(300),
            on_ground: false,
            dx: 4096,
            dy: -4096,
            dz: 1,
            yaw: 64,
            pitch: 0xe0,
        };
        let mut buf = Vec::new();
        pos_rot.encode(&mut buf).unwrap();
        assert_eq!(
            buf,
            [
                0xac, 0x02, 0x00, 0x10, 0x00, 0xf0, 0x00, 0x00, 0x01, 0x40, 0xe0
            ]
        );
        assert_eq!(
            MoveEntityPosRot::decode(&mut buf.as_slice()).unwrap(),
            pos_rot
        );

        let rot = MoveEntityRot {
            entity_id: VarInt(300),
            on_ground: true,
            yaw: 64,
            pitch: 0xe0,
        };
        let mut buf = Vec::new();
        rot.encode(&mut buf).unwrap();
        assert_eq!(buf, [0xac, 0x02, 0x01, 0x40, 0xe0]);
        assert_eq!(MoveEntityRot::decode(&mut buf.as_slice()).unwrap(), rot);
    }

    #[test]
    fn disconnect_is_a_component_alone() {
        // from the 26.3 codec: the text "hi"
        let mut buf = Vec::new();
        Disconnect {
            reason: Component::text("hi"),
        }
        .encode(&mut buf)
        .unwrap();
        assert_eq!(buf, [8, 0, 2, b'h', b'i']);
    }
}
