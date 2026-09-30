//! Minecraft protocol types, packets and generated data. Independent of the server.

mod codec;
mod error;
mod string;
mod varint;

pub use codec::{Decode, Encode, take};
pub use error::{Error, Result};
pub use string::Identifier;
pub use varint::{VarInt, VarLong};
