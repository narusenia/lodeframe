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
