// SPDX-License-Identifier: Apache-2.0 OR MIT
//! `cargo xtask bot [args]`: run `lodeframe-bot` with `args`, for load tests.

use std::process::Command;

use crate::Result;

/// Runs the bot in release mode: a debug build would make the bots, not the server, the limit.
pub fn run(args: Vec<String>) -> Result<()> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let status = Command::new(cargo)
        .args(["run", "--quiet", "--release", "-p", "lodeframe-bot", "--"])
        .args(args)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        // the bot has said what went wrong; pass its exit code on
        std::process::exit(status.code().unwrap_or(1));
    }
}
