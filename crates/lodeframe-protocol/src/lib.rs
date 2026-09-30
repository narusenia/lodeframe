// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Minecraft protocol types, packets and generated data. Independent of the server.

pub mod block;
pub mod chunk;
mod codec;
mod component;
mod coord;
pub mod entity_type;
mod error;
mod frame;
mod generated;
mod md5;
mod nbt;
mod packet;
pub mod packets;
mod string;
mod types;
mod varint;

pub use block::{Block, BlockInfo, BlockState, Property};
pub use codec::{Decode, Encode, take};
pub use coord::{BlockPos, Pos, Vec3};
pub use entity_type::EntityType;
pub use error::{Error, Result};
pub use frame::{
    FrameDecoder, MAX_BODY_LEN, MAX_FRAME_LEN, encode_frame, packet_body, split_packet_id,
};
pub use generated::datapack_registries::DATAPACK_REGISTRIES;
pub use generated::packet_ids as ids;
pub use generated::tags::{DYNAMIC_TAGS, STATIC_TAGS};
pub use generated::version::{PROTOCOL_VERSION, VERSION_NAME, WORLD_VERSION};
pub use glam;
pub use lodeframe_macros::{Decode, Encode, Packet};
pub use nbt::{Compound, Nbt};
pub use packet::{Packet, Side, State};
pub use string::Identifier;
pub use types::{BitSet, Uuid};
pub use varint::{VarInt, VarLong};
