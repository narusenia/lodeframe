// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Offline-mode login: no authentication, the UUID comes from the name.

use tokio::io::{AsyncRead, AsyncWrite};

use crate::{
    net::Connection,
    protocol::{
        Error, Result, State, Uuid, VarInt,
        packets::login::{Hello, LoginAcknowledged, LoginCompression, LoginFinished},
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
