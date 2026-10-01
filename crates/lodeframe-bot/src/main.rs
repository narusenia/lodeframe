// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Runs bots against a server: `lodeframe-bot [--addr <addr>] [--count <n>] [--seconds <s>]
//! [--stagger-ms <ms>]`, or `cargo xtask bot` with the same arguments.
//!
//! Each bot logs in as `bot0`, `bot1`, ..., walks around and builds for the given time. It
//! prints how many connected, how many did not and how many packets they read, and exits with
//! 1 if any bot failed.

use std::{process::ExitCode, str::FromStr, time::Duration};

use lodeframe_bot::Bot;

/// The value of `--name`, or `default` if it is not given. `Err` if it is given badly.
fn option<T: FromStr>(args: &[String], name: &str, default: T) -> Result<T, String> {
    match args.iter().position(|a| a == name) {
        None => Ok(default),
        Some(at) => args
            .get(at + 1)
            .and_then(|v| v.parse().ok())
            .ok_or_else(|| format!("{name} needs a value")),
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let parsed = (|| {
        Ok::<_, String>((
            option(&args, "--addr", "127.0.0.1:25565".to_string())?,
            option(&args, "--count", 1u32)?,
            option(&args, "--seconds", 10u64)?,
            option(&args, "--stagger-ms", 20u64)?,
        ))
    })();
    let (addr, count, seconds, stagger) = match parsed {
        Ok(p) => p,
        Err(e) => {
            eprintln!(
                "error: {e}\nusage: lodeframe-bot [--addr <addr>] [--count <n>] [--seconds <s>] [--stagger-ms <ms>]"
            );
            return ExitCode::from(2);
        }
    };
    tracing::info!(%addr, count, seconds, "starting");

    let mut bots = Vec::new();
    for index in 0..count {
        let addr = addr.clone();
        bots.push(tokio::spawn(async move {
            let mut bot = Bot::connect(&addr, &format!("bot{index}")).await?;
            let outcome = bot.wander(index, Duration::from_secs(seconds)).await;
            if let Err(e) = &outcome {
                tracing::warn!(bot = bot.name(), error = %e, "stopped");
            }
            Ok::<_, lodeframe_bot::Error>((bot.received(), outcome.is_ok()))
        }));
        tokio::time::sleep(Duration::from_millis(stagger)).await;
    }

    let (mut connected, mut failed, mut packets) = (0u32, 0u32, 0u64);
    for bot in bots {
        match bot.await {
            Ok(Ok((received, finished))) => {
                connected += 1;
                packets += received;
                failed += u32::from(!finished);
            }
            Ok(Err(e)) => {
                tracing::warn!(error = %e, "did not connect");
                failed += 1;
            }
            Err(e) => {
                tracing::warn!(error = %e, "bot task died");
                failed += 1;
            }
        }
    }
    println!("connected {connected}, failed {failed}, packets read {packets}");
    ExitCode::from(u8::from(failed > 0))
}
