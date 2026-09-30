//! Minecraft protocol types, packets and generated data. Independent of the server.

mod codec;
mod error;
mod varint;

pub use codec::{Decode, Encode, take};
pub use error::{Error, Result};
pub use varint::{VarInt, VarLong};
