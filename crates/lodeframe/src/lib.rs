// SPDX-License-Identifier: Apache-2.0 OR MIT
//! A minimal, lightweight Minecraft: Java Edition server library.
//!
//! This crate is the facade: it re-exports the other lodeframe crates.

pub mod net;

pub use lodeframe_macros as macros;
pub use lodeframe_protocol as protocol;
pub use lodeframe_text as text;
