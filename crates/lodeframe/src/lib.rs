// SPDX-License-Identifier: Apache-2.0 OR MIT
//! A minimal, lightweight Minecraft: Java Edition server library.
//!
//! This crate is the facade: it re-exports the other lodeframe crates.

pub mod chunk;
pub mod clock;
pub mod configuration;
pub mod event;
pub mod instance;
pub mod login;
pub mod net;
pub mod play;
pub mod registry;
pub mod schedule;
pub mod server;
pub mod status;
pub mod task;
#[cfg(feature = "test-util")]
pub mod test_util;
pub mod world;

pub use lodeframe_macros as macros;
pub use lodeframe_protocol as protocol;
pub use lodeframe_text as text;
