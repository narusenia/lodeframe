//! Minecraft protocol types, packets and generated data. Independent of the server.

mod codec;
mod error;
mod packet;
mod string;
mod types;
mod varint;

pub use codec::{Decode, Encode, take};
pub use error::{Error, Result};
pub use packet::{Packet, Side, State};
pub use string::Identifier;
pub use types::{BitSet, Position, Uuid};
pub use varint::{VarInt, VarLong};
