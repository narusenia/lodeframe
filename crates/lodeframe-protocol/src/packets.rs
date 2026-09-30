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
