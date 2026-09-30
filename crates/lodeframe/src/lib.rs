// SPDX-License-Identifier: Apache-2.0 OR MIT
//! A minimal, lightweight Minecraft: Java Edition server library.
//!
//! This crate is the facade: it re-exports the other lodeframe crates.

pub mod clock;
pub mod configuration;
pub mod instance;
pub mod login;
pub mod net;
pub mod play;
pub mod registry;
pub mod status;

pub use lodeframe_macros as macros;
pub use lodeframe_protocol as protocol;
pub use lodeframe_text as text;
