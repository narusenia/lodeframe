// SPDX-License-Identifier: Apache-2.0 OR MIT
//! The connection task of a player in the Play state: connects a socket to an instance.

use std::{io, time::Duration};

use tracing::Instrument;

use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::mpsc,
    time::{Instant, MissedTickBehavior, interval_at, sleep_until},
};

use crate::{
    instance::{InstanceHandle, Message, OUTBOX, Packets},
    login::Profile,
    net::{Connection, is_disconnect},
    protocol::{
        Decode, Error, Result, State, ids,
        packets::play::{Disconnect, KeepAlive, KeepAliveResponse},
        split_packet_id,
    },
    text::Component,
};

/// When keep alives go out and how long a client may leave one unanswered.
///
/// The answer to each keep alive also measures the player's latency, see [`Message::Latency`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeepAliveConfig {
    /// Time between one keep alive being answered and the next going out.
    pub interval: Duration,
    /// How long a keep alive may go unanswered before the client is disconnected. Also how long
    /// the connection may stay silent.
    pub timeout: Duration,
}

impl Default for KeepAliveConfig {
    /// A keep alive every 15 seconds, cutting a client that has not answered in 30.
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(15),
            timeout: Duration::from_secs(30),
        }
    }
}

/// The keep alive that was sent and not answered yet.
#[derive(Debug, Default)]
struct KeepAliveTracker {
    last_id: i64,
    pending: Option<(i64, Instant)>,
}

/// A client answered a keep alive wrongly.
#[derive(Debug, PartialEq, Eq)]
enum BadAnswer {
    /// Nothing was waiting for an answer.
    Unexpected,
    /// The id is not the one that was sent.
    WrongId,
}

impl KeepAliveTracker {
    /// The id of a keep alive to send now, or `None` if the last one is still waiting.
    fn send(&mut self, now: Instant) -> Option<i64> {
        if self.pending.is_some() {
            return None;
        }
        self.last_id += 1;
        self.pending = Some((self.last_id, now));
        Some(self.last_id)
    }

    /// Takes the answer `id`, returning how long it took.
    fn answer(&mut self, id: i64, now: Instant) -> std::result::Result<Duration, BadAnswer> {
        match self.pending {
            None => Err(BadAnswer::Unexpected),
            Some((sent, _)) if sent != id => Err(BadAnswer::WrongId),
            Some((_, at)) => {
                self.pending = None;
                Ok(now.saturating_duration_since(at))
            }
        }
    }

    /// When the waiting keep alive times out, if one is waiting.
    fn deadline(&self, timeout: Duration) -> Option<Instant> {
        self.pending.map(|(_, at)| at + timeout)
    }
}

/// The most packets written in one go, beyond which the rest waits for the next write.
const MAX_FRAMES_AT_ONCE: usize = 1024;

/// Joins `profile` to `instance` and relays packets both ways until the connection ends, with
/// the default [`KeepAliveConfig`].
///
/// Packets from the client are passed to the instance as [`Message::Packet`]; keepalive
/// answers are consumed here. The instance leaves the player when this returns.
pub async fn run<S: AsyncRead + AsyncWrite + Unpin>(
    conn: Connection<S>,
    profile: Profile,
    instance: InstanceHandle,
) -> Result<()> {
    run_with(conn, profile, instance, KeepAliveConfig::default()).await
}

/// Like [`run`], with keep alives at `keep_alive`.
///
/// A client that answers with the wrong id, answers without being asked, or leaves a keep alive
/// unanswered for [`KeepAliveConfig::timeout`] is told why and disconnected. Each answer sends
/// the instance a [`Message::Latency`].
pub async fn run_with<S: AsyncRead + AsyncWrite + Unpin>(
    mut conn: Connection<S>,
    profile: Profile,
    instance: InstanceHandle,
    keep_alive: KeepAliveConfig,
) -> Result<()> {
    if conn.state() != State::Play {
        return Err(Error::InvalidValue("not in the play state"));
    }
    let player = profile.uuid;
    let name = profile.name.clone();
    let (outbound, mut from_instance) = mpsc::channel::<Packets>(OUTBOX);
    // weak: the instance's copy alone keeps the channel open, so that it can end the connection
    let connection = outbound.downgrade();
    instance
        .send(Message::Join { profile, outbound })
        .await
        .map_err(|_| Error::InvalidValue("instance has stopped"))?;

    let span = tracing::info_span!("player", name = %name, uuid = %player);
    // the keep alive's own deadline cuts a silent client with a reason; this only backs it up
    conn.set_read_timeout(keep_alive.timeout + Duration::from_secs(1));
    let result = relay(&mut conn, &instance, player, &mut from_instance, keep_alive)
        .instrument(span.clone())
        .await;
    span.in_scope(|| match &result {
        Ok(()) => tracing::info!("left"),
        // the client just going away is the normal way to leave
        Err(e) if is_disconnect(e) => tracing::info!("left"),
        Err(e) => tracing::warn!(error = %e, "left after an error"),
    });
    // the instance may already be gone; there is nobody left to tell
    let _ = instance
        .send(Message::Leave {
            player,
            outbound: connection,
        })
        .await;
    result
}

async fn relay<S: AsyncRead + AsyncWrite + Unpin>(
    conn: &mut Connection<S>,
    instance: &InstanceHandle,
    player: crate::protocol::Uuid,
    from_instance: &mut mpsc::Receiver<Packets>,
    config: KeepAliveConfig,
) -> Result<()> {
    let mut every = interval_at(Instant::now() + config.interval, config.interval);
    every.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut keep_alive = KeepAliveTracker::default();
    loop {
        // a keep alive that is waiting has a deadline; with none waiting, never
        let deadline = keep_alive.deadline(config.timeout);
        tokio::select! {
            body = conn.read_frame() => {
                let body = body?;
                let (id, mut payload) = split_packet_id(&body)?;
                if id == ids::play::serverbound::KEEP_ALIVE {
                    let answer = KeepAliveResponse::decode(&mut payload)?;
                    match keep_alive.answer(answer.id, Instant::now()) {
                        Ok(rtt) => {
                            // the instance may be gone; the next send below finds out
                            let _ = instance.send(Message::Latency { player, rtt }).await;
                        }
                        Err(BadAnswer::Unexpected) => {
                            return kick(conn, "Unexpected keep alive answer").await;
                        }
                        Err(BadAnswer::WrongId) => {
                            return kick(conn, "Wrong keep alive id").await;
                        }
                    }
                    continue;
                }
                instance
                    .send(Message::Packet { player, body })
                    .await
                    .map_err(|_| Error::InvalidValue("instance has stopped"))?;
            }
            body = from_instance.recv() => match body {
                Some(first) => {
                    // everything that is already waiting goes out in one write
                    let mut batch = first;
                    while batch.len() < MAX_FRAMES_AT_ONCE {
                        let Ok(next) = from_instance.try_recv() else { break };
                        batch.extend(next);
                    }
                    conn.write_frames(&batch).await?;
                }
                // the instance dropped this player
                None => return Ok(()),
            },
            _ = every.tick() => {
                if let Some(id) = keep_alive.send(Instant::now()) {
                    conn.write_packet(&KeepAlive { id }).await?;
                }
            }
            () = async {
                match deadline {
                    Some(at) => sleep_until(at).await,
                    None => std::future::pending().await,
                }
            } => return kick(conn, "Timed out").await,
        }
    }
}

/// Tells the player `reason` and ends the connection with an error that says it was cut.
async fn kick<S: AsyncRead + AsyncWrite + Unpin>(
    conn: &mut Connection<S>,
    reason: &str,
) -> Result<()> {
    // the client may already be gone; there is nobody left to tell
    let _ = conn
        .write_packet(&Disconnect {
            reason: Component::text(reason),
        })
        .await;
    Err(io::Error::new(io::ErrorKind::TimedOut, reason.to_owned()).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn an_answer_gives_the_time_since_the_keep_alive_went_out() {
        let mut t = KeepAliveTracker::default();
        let id = t.send(Instant::now()).unwrap();
        tokio::time::advance(Duration::from_millis(40)).await;

        assert_eq!(t.answer(id, Instant::now()), Ok(Duration::from_millis(40)));
    }

    #[tokio::test(start_paused = true)]
    async fn nothing_goes_out_while_the_last_one_is_waiting() {
        let mut t = KeepAliveTracker::default();
        let first = t.send(Instant::now()).unwrap();

        assert_eq!(t.send(Instant::now()), None);
        t.answer(first, Instant::now()).unwrap();
        assert_ne!(t.send(Instant::now()), Some(first));
    }

    #[tokio::test(start_paused = true)]
    async fn a_wrong_or_unasked_answer_is_refused() {
        let mut t = KeepAliveTracker::default();
        assert_eq!(t.answer(1, Instant::now()), Err(BadAnswer::Unexpected));

        let id = t.send(Instant::now()).unwrap();
        assert_eq!(t.answer(id + 1, Instant::now()), Err(BadAnswer::WrongId));
        // the right one still works after a wrong one was seen
        assert!(t.answer(id, Instant::now()).is_ok());
        // and an answer is taken only once
        assert_eq!(t.answer(id, Instant::now()), Err(BadAnswer::Unexpected));
    }

    #[tokio::test(start_paused = true)]
    async fn the_deadline_counts_from_sending() {
        let mut t = KeepAliveTracker::default();
        assert_eq!(t.deadline(Duration::from_secs(30)), None);

        let at = Instant::now();
        t.send(at).unwrap();
        assert_eq!(
            t.deadline(Duration::from_secs(30)),
            Some(at + Duration::from_secs(30))
        );
    }
}
