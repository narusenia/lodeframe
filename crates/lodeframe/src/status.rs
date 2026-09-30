// SPDX-License-Identifier: Apache-2.0 OR MIT
//! The server list entry: description, player count and version.

use std::fmt::Write as _;

use tokio::io::{AsyncRead, AsyncWrite};

use crate::{
    net::Connection,
    protocol::{
        PROTOCOL_VERSION, Result, VERSION_NAME,
        packets::status::{PingRequest, PongResponse, StatusRequest, StatusResponse},
    },
};

/// What the server list shows for this server.
///
/// Build a new one per request to change it at runtime (for example, the live player count).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusInfo {
    /// The line shown under the server name.
    pub motd: String,
    /// Players currently online.
    pub online: u32,
    /// Player slots shown.
    pub max_players: u32,
}

impl StatusInfo {
    /// An empty server with 20 slots.
    pub fn new(motd: impl Into<String>) -> Self {
        Self {
            motd: motd.into(),
            online: 0,
            max_players: 20,
        }
    }

    /// The JSON the client expects. The version is always the one this crate speaks.
    // ponytail: plain-text motd and no favicon; rich motd once text has a JSON form
    pub fn to_json(&self) -> String {
        let mut motd = String::new();
        for c in self.motd.chars() {
            match c {
                '"' => motd.push_str("\\\""),
                '\\' => motd.push_str("\\\\"),
                c if c < ' ' => write!(motd, "\\u{:04x}", c as u32).unwrap(),
                c => motd.push(c),
            }
        }
        format!(
            r#"{{"version":{{"name":"{VERSION_NAME}","protocol":{PROTOCOL_VERSION}}},"players":{{"max":{},"online":{}}},"description":{{"text":"{motd}"}}}}"#,
            self.max_players, self.online
        )
    }
}

/// Answers a server list ping on a connection in [`State::Status`](crate::protocol::State::Status).
///
/// Sends `info`, then echoes the ping. A client that hangs up after the description
/// without pinging is not an error.
pub async fn respond<S: AsyncRead + AsyncWrite + Unpin>(
    conn: &mut Connection<S>,
    info: &StatusInfo,
) -> Result<()> {
    conn.read_packet::<StatusRequest>().await?;
    conn.write_packet(&StatusResponse {
        json: info.to_json(),
    })
    .await?;
    let PingRequest { payload } = match conn.read_packet().await {
        Ok(ping) => ping,
        Err(crate::protocol::Error::Io(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
            return Ok(());
        }
        Err(e) => return Err(e),
    };
    conn.write_packet(&PongResponse { payload }).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_escapes_the_motd() {
        let json = StatusInfo::new("a\"b\\c\nd").to_json();
        assert!(
            json.contains(r#""description":{"text":"a\"b\\c\u000ad"}"#),
            "{json}"
        );
        assert!(json.contains(r#""players":{"max":20,"online":0}"#));
        assert!(json.contains(&format!(r#""protocol":{PROTOCOL_VERSION}"#)));
    }
}
