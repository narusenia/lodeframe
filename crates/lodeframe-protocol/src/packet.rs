// SPDX-License-Identifier: Apache-2.0 OR MIT
/// The connection state a packet belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum State {
    /// The first packet of every connection.
    Handshake,
    /// Server list ping.
    Status,
    /// Authentication.
    Login,
    /// Registry and resource setup before play.
    Configuration,
    /// The game itself.
    Play,
}

/// Which way a packet travels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    /// Server to client.
    Clientbound,
    /// Client to server.
    Serverbound,
}

/// A packet with a fixed id. Packet ids are only unique per state and side.
///
/// Usually derived: `#[derive(Packet)] #[packet(id = 0x00, state = Play, side = Clientbound)]`.
pub trait Packet {
    /// The packet id within its state and side.
    const ID: i32;
    /// The state the packet is valid in.
    const STATE: State;
    /// The direction the packet travels.
    const SIDE: Side;
}

#[cfg(test)]
mod tests {
    use crate::{Decode, Encode, Packet, Side, State, VarInt};

    // Inside this crate the derives must point at `crate` instead of `::lodeframe`.
    #[derive(Debug, PartialEq, Encode, Decode, Packet)]
    #[lodeframe(crate = crate)]
    #[packet(id = 0x01, state = Status, side = Serverbound)]
    struct Ping {
        payload: i64,
        seq: VarInt,
    }

    #[test]
    fn derives_work_inside_the_protocol_crate() {
        assert_eq!(
            (Ping::ID, Ping::STATE, Ping::SIDE),
            (1, State::Status, Side::Serverbound)
        );
        let ping = Ping {
            payload: -1,
            seq: VarInt(2),
        };
        let mut buf = Vec::new();
        ping.encode(&mut buf).unwrap();
        assert_eq!(buf, [0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 2]);
        assert_eq!(Ping::decode(&mut buf.as_slice()).unwrap(), ping);
    }
}
