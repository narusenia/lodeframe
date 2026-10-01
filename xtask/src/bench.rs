// SPDX-License-Identifier: Apache-2.0 OR MIT
//! `cargo xtask bench`: start time, idle memory and tick rate of the lobby under N bots.
//!
//! The lobby and the bots run as separate processes so that the memory read is the server's
//! alone. See `docs/implementation/bench-plan.md` for what is measured and how.

use std::{
    io::{BufRead, BufReader},
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use crate::Result;

/// How long the lobby sits with nobody in it before its memory is read.
const IDLE_WAIT: Duration = Duration::from_secs(2);
/// How far apart bots log in. The same as the bot's own default.
const STAGGER: Duration = Duration::from_millis(20);
/// How long the bots may take to all be in, beyond the stagger itself.
const SETTLE: Duration = Duration::from_secs(2);
/// Times the start and the idle memory are measured; the smallest is reported.
const IDLE_RUNS: usize = 3;

struct Options {
    counts: Vec<u32>,
    window: Duration,
    port: u16,
}

fn parse_options(args: &[String]) -> std::result::Result<Options, String> {
    let mut options = Options {
        counts: vec![1, 10, 50, 75, 100, 250, 500],
        window: Duration::from_secs(10),
        port: 25590,
    };
    let mut args = args.iter();
    while let Some(name) = args.next() {
        let value = args.next().ok_or_else(|| format!("{name} needs a value"))?;
        let bad = || format!("{name}: {value:?} is not a valid value");
        match name.as_str() {
            "--counts" => {
                options.counts = value
                    .split(',')
                    .map(|n| n.trim().parse().map_err(|_| bad()))
                    .collect::<std::result::Result<_, _>>()?;
            }
            "--window" => options.window = Duration::from_secs(value.parse().map_err(|_| bad())?),
            "--port" => options.port = value.parse().map_err(|_| bad())?,
            _ => return Err(format!("unknown option {name}")),
        }
    }
    if options.counts.is_empty() || options.counts.contains(&0) {
        return Err("--counts needs positive numbers".into());
    }
    Ok(options)
}

/// One `tick-stats` line the lobby printed, and when it was read.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Sample {
    at: Instant,
    ticks: u64,
    busy_us: u64,
    busy_max_us: u64,
    late: u64,
    skipped: u64,
}

fn parse_sample(line: &str, at: Instant) -> Option<Sample> {
    let rest = line.strip_prefix("tick-stats ")?;
    let field = |name: &str| -> Option<u64> {
        rest.split(' ')
            .find_map(|kv| kv.strip_prefix(name)?.strip_prefix('='))?
            .parse()
            .ok()
    };
    Some(Sample {
        at,
        ticks: field("ticks")?,
        busy_us: field("busy_us")?,
        busy_max_us: field("busy_max_us")?,
        late: field("late")?,
        skipped: field("skipped")?,
    })
}

/// What happened between two samples.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Window {
    tps: f64,
    avg_tick_ms: f64,
    /// The longest tick since the lobby started: the joins are in it.
    max_tick_ms: f64,
    late: u64,
    skipped: u64,
}

fn window_between(first: &Sample, last: &Sample) -> Option<Window> {
    let seconds = last.at.checked_duration_since(first.at)?.as_secs_f64();
    let ticks = last.ticks.checked_sub(first.ticks)?;
    if seconds <= 0.0 || ticks == 0 {
        return None;
    }
    Some(Window {
        tps: ticks as f64 / seconds,
        avg_tick_ms: (last.busy_us - first.busy_us) as f64 / ticks as f64 / 1000.0,
        max_tick_ms: last.busy_max_us as f64 / 1000.0,
        late: last.late - first.late,
        skipped: last.skipped - first.skipped,
    })
}

/// The first sample at or after `from` and the last at or before `to`.
fn window_in(samples: &[Sample], from: Instant, to: Instant) -> Option<Window> {
    let first = samples.iter().find(|s| s.at >= from)?;
    let last = samples.iter().rev().find(|s| s.at <= to)?;
    window_between(first, last)
}

/// A running lobby, with the `tick-stats` it has printed so far.
struct Lobby {
    child: Child,
    samples: Arc<Mutex<Vec<Sample>>>,
    /// When the lobby cut a player off because it could not send to them. The lobby logs the same
    /// line when the player is simply gone, so only those before the bots leave mean overflow.
    dropped: Arc<Mutex<Vec<Instant>>>,
}

/// Whether `line` is the lobby's log line for cutting a player off.
fn is_drop(line: &str) -> bool {
    line.contains("dropping player: outbound channel full")
}

impl Lobby {
    /// Starts the lobby and waits until it accepts a connection. Returns how long that took.
    fn start(binary: &Path, addr: SocketAddr) -> Result<(Self, Duration)> {
        let begun = Instant::now();
        let mut child = Command::new(binary)
            .arg(addr.to_string())
            .env("LOBBY_STATS", "1")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let samples = Arc::new(Mutex::new(Vec::new()));
        let dropped = Arc::new(Mutex::new(Vec::new()));
        let stdout = child.stdout.take().ok_or("the lobby has no stdout")?;
        let (seen, cut) = (samples.clone(), dropped.clone());
        thread::spawn(move || {
            // keeps reading until the lobby ends, or the pipe would fill and stop it
            for line in BufReader::new(stdout).lines().map_while(|l| l.ok()) {
                if let Some(sample) = parse_sample(&line, Instant::now()) {
                    seen.lock().unwrap().push(sample);
                } else if is_drop(&line) {
                    cut.lock().unwrap().push(Instant::now());
                }
            }
        });
        let lobby = Self {
            child,
            samples,
            dropped,
        };
        loop {
            if TcpStream::connect_timeout(&addr, Duration::from_millis(50)).is_ok() {
                return Ok((lobby, begun.elapsed()));
            }
            if begun.elapsed() > Duration::from_secs(10) {
                return Err("the lobby did not start listening in 10 s".into());
            }
            thread::sleep(Duration::from_millis(1));
        }
    }

    /// The resident memory of the process, in kilobytes.
    fn rss_kb(&self) -> Option<u64> {
        let out = Command::new("ps")
            .args(["-o", "rss=", "-p", &self.child.id().to_string()])
            .output()
            .ok()?;
        String::from_utf8(out.stdout).ok()?.trim().parse().ok()
    }

    fn samples(&self) -> Vec<Sample> {
        self.samples.lock().unwrap().clone()
    }

    /// Players cut off up to `until`.
    fn dropped_until(&self, until: Instant) -> u64 {
        self.dropped
            .lock()
            .unwrap()
            .iter()
            .filter(|at| **at <= until)
            .count() as u64
    }
}

impl Drop for Lobby {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// What the bot program printed at the end: `connected C, failed F, packets read P`.
#[derive(Debug, Default, PartialEq, Eq)]
struct BotResult {
    connected: u32,
    failed: u32,
}

fn parse_bot_result(out: &str) -> Option<BotResult> {
    let line = out.lines().find(|l| l.starts_with("connected "))?;
    let number = |name: &str| -> Option<u32> {
        line.split([' ', ','])
            .skip_while(|w| *w != name)
            .nth(1)?
            .parse()
            .ok()
    };
    Some(BotResult {
        connected: number("connected")?,
        failed: number("failed")?,
    })
}

struct Row {
    count: u32,
    bots: BotResult,
    dropped: u64,
    window: Option<Window>,
    rss_mb: Option<f64>,
}

pub fn run(args: Vec<String>) -> Result<()> {
    let options = parse_options(&args).map_err(|e| {
        format!("{e}\nusage: cargo xtask bench [--counts 1,10,50,75,100,250,500] [--window <seconds>] [--port <port>]")
    })?;
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let built = Command::new(&cargo)
        .args(["build", "--release", "-p", "lobby", "-p", "lodeframe-bot"])
        .status()?;
    if !built.success() {
        return Err("the release build failed".into());
    }
    let release = root.join("target/release");
    let (lobby, bot) = (release.join("lobby"), release.join("lodeframe-bot"));
    let addr: SocketAddr = ([127, 0, 0, 1], options.port).into();

    println!("idle ({IDLE_RUNS} runs, smallest)");
    let (mut start, mut idle) = (Duration::MAX, u64::MAX);
    for _ in 0..IDLE_RUNS {
        let (server, took) = Lobby::start(&lobby, addr)?;
        thread::sleep(IDLE_WAIT);
        start = start.min(took);
        idle = idle.min(
            server
                .rss_kb()
                .ok_or("could not read the memory of the lobby")?,
        );
    }

    let mut rows = Vec::new();
    for &count in &options.counts {
        println!("{count} bots ...");
        let (server, _) = Lobby::start(&lobby, addr)?;
        let settle = STAGGER * count + SETTLE;
        let total = settle + options.window;
        let bots = Command::new(&bot)
            .args([
                "--addr",
                &addr.to_string(),
                "--count",
                &count.to_string(),
                "--seconds",
                // a little longer than the window, so the bots are still in when it ends
                &(total.as_secs() + 2).to_string(),
                "--stagger-ms",
                &STAGGER.as_millis().to_string(),
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        thread::sleep(settle);
        let from = Instant::now();
        thread::sleep(options.window);
        let to = Instant::now();
        let rss_kb = server.rss_kb();
        let out = bots.wait_with_output()?;
        let samples = server.samples();
        let dropped = server.dropped_until(to);
        drop(server);
        rows.push(Row {
            count,
            dropped,
            bots: parse_bot_result(&String::from_utf8_lossy(&out.stdout)).unwrap_or_default(),
            window: window_in(&samples, from, to),
            rss_mb: rss_kb.map(|k| k as f64 / 1024.0),
        });
    }

    println!();
    println!(
        "machine: {} {}, {} cpus; the bots run on the same machine",
        std::env::consts::OS,
        std::env::consts::ARCH,
        thread::available_parallelism().map_or(0, |n| n.get())
    );
    println!(
        "start to listening: {} ms; idle memory: {:.1} MB",
        start.as_millis(),
        idle as f64 / 1024.0
    );
    println!();
    println!(
        "| bots | connected | failed | cut off | TPS | avg tick ms | max tick ms | late | skipped | RSS MB |"
    );
    println!("|---|---|---|---|---|---|---|---|---|---|");
    for row in &rows {
        let cell = |v: Option<String>| v.unwrap_or_else(|| "n/a".into());
        println!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            row.count,
            row.bots.connected,
            row.bots.failed,
            row.dropped,
            cell(row.window.map(|w| format!("{:.1}", w.tps))),
            cell(row.window.map(|w| format!("{:.2}", w.avg_tick_ms))),
            cell(row.window.map(|w| format!("{:.1}", w.max_tick_ms))),
            cell(row.window.map(|w| w.late.to_string())),
            cell(row.window.map(|w| w.skipped.to_string())),
            cell(row.rss_mb.map(|m| format!("{m:.1}"))),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(at: Instant, ticks: u64, busy_us: u64) -> Sample {
        Sample {
            at,
            ticks,
            busy_us,
            busy_max_us: 900,
            late: ticks / 100,
            skipped: 0,
        }
    }

    #[test]
    fn a_stats_line_is_read_and_other_lines_are_not() {
        let now = Instant::now();
        let s = parse_sample(
            "tick-stats ticks=100 busy_us=1234 busy_max_us=900 late=2 skipped=1",
            now,
        )
        .unwrap();
        assert_eq!(
            (s.ticks, s.busy_us, s.busy_max_us, s.late, s.skipped),
            (100, 1234, 900, 2, 1)
        );
        assert_eq!(parse_sample("INFO lodeframe: joined", now), None);
        assert_eq!(parse_sample("tick-stats ticks=1", now), None);
    }

    #[test]
    fn a_window_is_the_difference_between_two_samples() {
        let t0 = Instant::now();
        let first = sample(t0, 100, 10_000);
        let last = sample(t0 + Duration::from_secs(5), 200, 60_000);

        let w = window_between(&first, &last).unwrap();

        assert!((w.tps - 20.0).abs() < 1e-9);
        // 50 000 us over 100 ticks
        assert!((w.avg_tick_ms - 0.5).abs() < 1e-9);
        assert_eq!(w.max_tick_ms, 0.9);
        assert_eq!(w.late, 1);
    }

    #[test]
    fn the_window_uses_the_samples_inside_it() {
        let t0 = Instant::now();
        let at = |s: u64| t0 + Duration::from_secs(s);
        let samples = [
            sample(at(1), 20, 0),
            sample(at(2), 40, 0),
            sample(at(3), 60, 0),
            sample(at(4), 80, 0),
        ];

        let w = window_in(&samples, at(2), at(3)).unwrap();

        // from the sample at 2 s to the one at 3 s: 20 ticks in 1 s
        assert!((w.tps - 20.0).abs() < 1e-9);
        // fewer than two samples in the window: nothing to say
        assert_eq!(window_in(&samples, at(3), at(3)), None);
        assert_eq!(window_in(&samples, at(5), at(6)), None);
    }

    #[test]
    fn a_cut_off_is_recognised_in_the_log() {
        assert!(is_drop(
            "2026 WARN lodeframe::instance: dropping player: outbound channel full or closed player=x"
        ));
        assert!(!is_drop("2026 INFO lodeframe::world: joined name=Steve"));
    }

    #[test]
    fn the_bot_summary_is_read() {
        let out = "something\nconnected 48, failed 2, packets read 99\n";
        assert_eq!(
            parse_bot_result(out),
            Some(BotResult {
                connected: 48,
                failed: 2
            })
        );
        assert_eq!(parse_bot_result("nothing"), None);
    }

    #[test]
    fn options_are_read() {
        let o = parse_options(&[
            "--counts".into(),
            "5, 10".into(),
            "--window".into(),
            "3".into(),
        ])
        .unwrap();
        assert_eq!((o.counts, o.window), (vec![5, 10], Duration::from_secs(3)));
        assert!(parse_options(&["--counts".into(), "0".into()]).is_err());
        assert!(parse_options(&["--nope".into(), "1".into()]).is_err());
        assert!(parse_options(&["--window".into()]).is_err());
    }
}
