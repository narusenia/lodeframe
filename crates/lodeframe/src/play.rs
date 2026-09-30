// SPDX-License-Identifier: Apache-2.0 OR MIT
//! The connection task of a player in the Play state: connects a socket to an instance.

use std::time::Duration;

use tracing::Instrument;

use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::mpsc,
    time::{Instant, MissedTickBehavior, interval_at},
};

use crate::{
    instance::{InstanceHandle, Message, OUTBOX},
    login::Profile,
    net::Connection,
    protocol::{Error, Result, State, ids, packets::play::KeepAlive, split_packet_id},
};

/// How often a keepalive goes out. The connection's read timeout then acts as the client
/// timeout: a client that stops answering is dropped when the timeout passes.
const KEEP_ALIVE_EVERY: Duration = Duration::from_secs(15);

/// Joins `profile` to `instance` and relays packets both ways until the connection ends.
///
/// Packets from the client are passed to the instance as [`Message::Packet`]; keepalive
/// answers are consumed here. The instance leaves the player when this returns.
pub async fn run<S: AsyncRead + AsyncWrite + Unpin>(
    mut conn: Connection<S>,
    profile: Profile,
    instance: InstanceHandle,
) -> Result<()> {
    if conn.state() != State::Play {
        return Err(Error::InvalidValue("not in the play state"));
    }
    let player = profile.uuid;
    let name = profile.name.clone();
    let (outbound, mut from_instance) = mpsc::channel::<Vec<u8>>(OUTBOX);
    instance
        .send(Message::Join { profile, outbound })
        .await
        .map_err(|_| Error::InvalidValue("instance has stopped"))?;

    let span = tracing::info_span!("player", name = %name, uuid = %player);
    let result = relay(&mut conn, &instance, player, &mut from_instance)
        .instrument(span.clone())
        .await;
    span.in_scope(|| match &result {
        Ok(()) => tracing::info!("left"),
        Err(e) => tracing::info!(error = %e, "left"),
    });
    // the instance may already be gone; there is nobody left to tell
    let _ = instance.send(Message::Leave { player }).await;
    result
}

async fn relay<S: AsyncRead + AsyncWrite + Unpin>(
    conn: &mut Connection<S>,
    instance: &InstanceHandle,
    player: crate::protocol::Uuid,
    from_instance: &mut mpsc::Receiver<Vec<u8>>,
) -> Result<()> {
    let mut keep_alive = interval_at(Instant::now() + KEEP_ALIVE_EVERY, KEEP_ALIVE_EVERY);
    keep_alive.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut next_id = 0i64;
    loop {
        tokio::select! {
            body = conn.read_frame() => {
                let body = body?;
                let (id, _) = split_packet_id(&body)?;
                if id == ids::play::serverbound::KEEP_ALIVE {
                    continue;
                }
                instance
                    .send(Message::Packet { player, body })
                    .await
                    .map_err(|_| Error::InvalidValue("instance has stopped"))?;
            }
            body = from_instance.recv() => match body {
                Some(body) => conn.write_frame(&body).await?,
                // the instance dropped this player
                None => return Ok(()),
            },
            _ = keep_alive.tick() => {
                next_id += 1;
                conn.write_packet(&KeepAlive { id: next_id }).await?;
            }
        }
    }
}
