// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Offline-mode login: no authentication, the UUID comes from the name.

use std::{io, time::Duration};

use tokio::{
    io::{AsyncRead, AsyncWrite},
    time::timeout,
};

use crate::{
    net::Connection,
    protocol::{
        Error, Identifier, Result, State, Uuid, VarInt,
        packets::login::{
            CustomQuery, CustomQueryAnswer, Hello, LoginAcknowledged, LoginCompression,
            LoginFinished,
        },
    },
};

/// Who logged in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    /// The offline-mode UUID of `name`.
    pub uuid: Uuid,
    /// The player's name.
    pub name: String,
}

/// Runs offline login on a connection in [`State::Login`] and leaves it in
/// [`State::Configuration`].
///
/// With `compression_threshold` set, compression is turned on before the profile is sent.
pub async fn offline<S: AsyncRead + AsyncWrite + Unpin>(
    conn: &mut Connection<S>,
    compression_threshold: Option<usize>,
) -> Result<Profile> {
    if conn.state() != State::Login {
        return Err(Error::InvalidValue("not in the login state"));
    }
    let hello: Hello = conn.read_packet().await?;
    if hello.name.is_empty() || hello.name.chars().count() > 16 {
        return Err(Error::InvalidValue("player name length"));
    }
    if let Some(threshold) = compression_threshold {
        let threshold =
            i32::try_from(threshold).map_err(|_| Error::InvalidValue("compression threshold"))?;
        conn.write_packet(&LoginCompression {
            threshold: VarInt(threshold),
        })
        .await?;
        conn.set_compression(Some(threshold as usize));
    }
    let profile = Profile {
        uuid: Uuid::offline(&hello.name),
        name: hello.name,
    };
    conn.write_packet(&LoginFinished {
        uuid: profile.uuid,
        name: profile.name.clone(),
        properties: Vec::new(),
        // ponytail: offline has no session service; reuse the profile UUID as the session id
        session_id: profile.uuid,
    })
    .await?;
    conn.read_packet::<LoginAcknowledged>().await?;
    conn.set_state(State::Configuration);
    Ok(profile)
}

/// Asks the client things on channels of the server's own while it logs in, the way a proxy
/// passes the player on. One per connection, so that every question gets its own id.
///
/// [`offline`] does not ask anything; whatever extends the login calls this between the
/// packets it reads.
#[derive(Debug, Default)]
pub struct Queries {
    next: i32,
}

impl Queries {
    /// Sends `data` on `channel` and waits up to `limit` for the answer.
    ///
    /// `Ok(None)` is a client that does not know the channel. A client that does not answer in
    /// time is a [`TimedOut`](io::ErrorKind::TimedOut) error, and so is an answer to another
    /// question or any other packet: the caller ends the login, with a reason if it likes.
    pub async fn ask<S: AsyncRead + AsyncWrite + Unpin>(
        &mut self,
        conn: &mut Connection<S>,
        channel: Identifier,
        data: &[u8],
        limit: Duration,
    ) -> Result<Option<Vec<u8>>> {
        if conn.state() != State::Login {
            return Err(Error::InvalidValue("not in the login state"));
        }
        let transaction_id = VarInt(self.next);
        self.next = self.next.wrapping_add(1);
        conn.write_packet(&CustomQuery {
            transaction_id,
            channel,
            data: data.to_vec(),
        })
        .await?;
        let answer: CustomQueryAnswer = timeout(limit, conn.read_packet())
            .await
            .map_err(|_| Error::from(io::Error::from(io::ErrorKind::TimedOut)))??;
        if answer.transaction_id != transaction_id {
            return Err(Error::InvalidValue("answer to another login query"));
        }
        Ok(answer.data)
    }
}
