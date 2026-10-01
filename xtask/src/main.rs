// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Developer tasks: `cargo xtask <task>`.

mod bot;
mod codegen;
mod datagen;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn main() {
    let mut args = std::env::args().skip(1);
    let result = match args.next().as_deref() {
        Some("datagen") => datagen::run(args.next().as_deref()),
        Some("bot") => bot::run(args.collect()),
        _ => {
            eprintln!(
                "usage: cargo xtask <task>\n\ntasks:\n  datagen [version]  regenerate protocol data from the vanilla data generator\n  bot [args]         run lodeframe-bot (--addr, --count, --seconds, --stagger-ms)"
            );
            std::process::exit(2);
        }
    };
    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
