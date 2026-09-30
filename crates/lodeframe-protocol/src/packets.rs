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
    use crate::{BlockPos, Decode, Encode, Identifier, Packet, VarInt, Vec3};

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
}
