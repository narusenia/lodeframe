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

#[cfg(test)]
mod tests {
    use super::{configuration::*, login::*};
    use crate::{Decode, Encode, Identifier, Nbt, Uuid, VarInt};

    fn roundtrip<T: Encode + Decode + PartialEq + std::fmt::Debug>(v: T) {
        let mut buf = Vec::new();
        v.encode(&mut buf).unwrap();
        assert_eq!(T::decode(&mut buf.as_slice()).unwrap(), v);
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
