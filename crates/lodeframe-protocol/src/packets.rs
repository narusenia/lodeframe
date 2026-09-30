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
