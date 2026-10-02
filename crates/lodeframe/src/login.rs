// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Login without authentication: offline mode, where the UUID comes from the name, and
//! Velocity modern forwarding, where a proxy that shares a secret says who the player is.

use std::{io, net::IpAddr, time::Duration};

use hmac::{Hmac, Mac};
use sha2::Sha256;

use tokio::{
    io::{AsyncRead, AsyncWrite},
    time::timeout,
};

use crate::{
    net::Connection,
    protocol::{
        Decode, Error, Identifier, Result, State, Uuid, VarInt,
        packets::login::{
            CustomQuery, CustomQueryAnswer, Disconnect, Hello, LoginAcknowledged, LoginCompression,
            LoginFinished, ProfileProperty,
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
    /// The skin and the like a proxy forwarded; empty without one.
    pub properties: Vec<ProfileProperty>,
    /// The address a proxy said the client connected from; `None` without one.
    pub remote_addr: Option<IpAddr>,
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
        properties: Vec::new(),
        remote_addr: None,
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

/// The channel Velocity asks and answers on.
pub const VELOCITY_CHANNEL: &str = "velocity:player_info";
/// The forwarding format asked for: the player's identity without the chat key, which
/// nothing here uses. A proxy answers in the version asked for, or a lower one.
pub const VELOCITY_VERSION: i32 = 1;
const SIGNATURE_LEN: usize = 32;
const MAX_PROPERTIES: usize = 64;
const MAX_NAME: usize = 16;
const NOT_PROXIED: &str = "This server requires you to connect with Velocity.";
const NOT_VERIFIED: &str = "Unable to verify player details";

/// Runs Velocity modern forwarding on a connection in [`State::Login`] and leaves it in
/// [`State::Configuration`]: asks the proxy who the player is, checks the answer against
/// `secret` and takes the UUID, name, skin and address from it.
///
/// A client that does not answer within `limit`, does not know the channel (it did not come
/// through a proxy) or answers with a signature that does not match `secret` is told why and
/// the login ends with an error.
pub async fn velocity<S: AsyncRead + AsyncWrite + Unpin>(
    conn: &mut Connection<S>,
    compression_threshold: Option<usize>,
    secret: &[u8],
    limit: Duration,
) -> Result<Profile> {
    if conn.state() != State::Login {
        return Err(Error::InvalidValue("not in the login state"));
    }
    // the name here is the proxy's own view; the forwarded one replaces it
    conn.read_packet::<Hello>().await?;
    if let Some(threshold) = compression_threshold {
        let threshold =
            i32::try_from(threshold).map_err(|_| Error::InvalidValue("compression threshold"))?;
        conn.write_packet(&LoginCompression {
            threshold: VarInt(threshold),
        })
        .await?;
        conn.set_compression(Some(threshold as usize));
    }
    let channel = Identifier::new(VELOCITY_CHANNEL)?;
    let asked = Queries::default()
        .ask(conn, channel, &[VELOCITY_VERSION as u8], limit)
        .await;
    let answer = match asked {
        Ok(Some(answer)) => answer,
        Ok(None) => return refuse(conn, NOT_PROXIED, "the client did not know the channel").await,
        Err(Error::Io(e)) if e.kind() == io::ErrorKind::TimedOut => {
            return refuse(conn, NOT_PROXIED, "no answer to the forwarding query").await;
        }
        Err(e) => return Err(e),
    };
    let profile = match forwarded(secret, &answer) {
        Ok(profile) => profile,
        Err(why) => return refuse(conn, NOT_VERIFIED, why).await,
    };
    conn.write_packet(&LoginFinished {
        uuid: profile.uuid,
        name: profile.name.clone(),
        properties: profile.properties.clone(),
        session_id: profile.uuid,
    })
    .await?;
    conn.read_packet::<LoginAcknowledged>().await?;
    conn.set_state(State::Configuration);
    Ok(profile)
}

/// Tells the client `reason` and ends the login with an error that says `why` in the log.
async fn refuse<S: AsyncRead + AsyncWrite + Unpin, T>(
    conn: &mut Connection<S>,
    reason: &'static str,
    why: &'static str,
) -> Result<T> {
    // the reasons are plain text with nothing to escape
    debug_assert!(!reason.contains(['"', '\\']));
    // the client may be gone already, which is no worse than the refusal itself
    let _ = conn
        .write_packet(&Disconnect {
            reason: format!("{{\"text\":\"{reason}\"}}"),
        })
        .await;
    Err(Error::InvalidValue(why))
}

/// Checks the signature of what the proxy answered and reads the player out of it.
fn forwarded(secret: &[u8], answer: &[u8]) -> std::result::Result<Profile, &'static str> {
    let (signature, mut body) = answer
        .split_at_checked(SIGNATURE_LEN)
        .ok_or("forwarded data too short")?;
    // `new_from_slice` takes any key length
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC takes any key length");
    mac.update(body);
    // constant time; nothing is read from the body before this passes
    mac.verify_slice(signature)
        .map_err(|_| "forwarded data signature does not match")?;
    let bad = |_| "forwarded data malformed";
    let version = VarInt::decode(&mut body).map_err(bad)?.0;
    if version != VELOCITY_VERSION {
        return Err("forwarded data version not asked for");
    }
    let address = String::decode(&mut body).map_err(bad)?;
    let remote_addr = address
        .parse::<IpAddr>()
        .map_err(|_| "forwarded address is not an IP address")?;
    let uuid = Uuid::decode(&mut body).map_err(bad)?;
    let name = String::decode(&mut body).map_err(bad)?;
    if name.is_empty() || name.chars().count() > MAX_NAME {
        return Err("forwarded name length");
    }
    let properties = Vec::<ProfileProperty>::decode(&mut body).map_err(bad)?;
    if properties.len() > MAX_PROPERTIES {
        return Err("too many forwarded properties");
    }
    if !body.is_empty() {
        return Err("forwarded data has trailing bytes");
    }
    Ok(Profile {
        uuid,
        name,
        properties,
        remote_addr: Some(remote_addr),
    })
}
