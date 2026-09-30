// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Minecraft protocol types, packets and generated data. Independent of the server.

mod codec;
mod component;
mod coord;
mod error;
mod nbt;
mod packet;
mod string;
mod types;
mod varint;

pub use codec::{Decode, Encode, take};
pub use coord::{BlockPos, Pos, Vec3};
pub use error::{Error, Result};
pub use glam;
pub use lodeframe_macros::{Decode, Encode, Packet};
pub use nbt::{Compound, Nbt};
pub use packet::{Packet, Side, State};
pub use string::Identifier;
pub use types::{BitSet, Uuid};
pub use varint::{VarInt, VarLong};
