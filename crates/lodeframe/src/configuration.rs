// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Configuration: agree on data packs, send the registries, hand over to Play.

use std::{io, time::Duration};

use tokio::{
    io::{AsyncRead, AsyncWrite},
    time::timeout,
};

use crate::{
    instance::PluginMessage,
    net::Connection,
    protocol::{
        Decode, Error, Identifier, Packet, Result, State, VERSION_NAME,
        packets::configuration::{
            AckFinishConfiguration, ClientboundCustomPayload, ClientboundKnownPacks,
            FinishConfiguration, KnownPack, ServerboundCustomPayload, ServerboundKnownPacks,
            UpdateEnabledFeatures,
        },
        split_packet_id,
    },
    registry::Registries,
};

/// Runs configuration on a connection in [`State::Configuration`] and leaves it in
/// [`State::Play`].
///
/// The client must have the vanilla `minecraft:core` pack of this crate's version: the
/// registries are sent by name and the client fills in the vanilla data itself.
///
/// `brand` is the name of the server that the client shows in its debug screen (F3).
///
/// Returns what the client sent on plugin channels on the way, the brand it reports among them.
pub async fn run<S: AsyncRead + AsyncWrite + Unpin>(
    conn: &mut Connection<S>,
    registries: &Registries,
    brand: &str,
) -> Result<Vec<PluginMessage>> {
    run_with(conn, registries, brand, KNOWN_PACKS_TIMEOUT).await
}

/// How long [`run`] waits for the client's answer to the known packs.
pub const KNOWN_PACKS_TIMEOUT: Duration = Duration::from_secs(30);

/// Like [`run`], waiting `known_packs_timeout` for the client's answer to the known packs
/// (the connection's read timeout applies to every other packet).
///
/// At most [`MAX_PLUGIN_MESSAGES`] messages of [`MAX_PLUGIN_BYTES`] bytes together are taken;
/// a client that sends more is an error.
pub async fn run_with<S: AsyncRead + AsyncWrite + Unpin>(
    conn: &mut Connection<S>,
    registries: &Registries,
    brand: &str,
    known_packs_timeout: Duration,
) -> Result<Vec<PluginMessage>> {
    if conn.state() != State::Configuration {
        return Err(Error::InvalidValue("not in the configuration state"));
    }
    let core = KnownPack {
        namespace: "minecraft".into(),
        id: "core".into(),
        version: VERSION_NAME.into(),
    };
    conn.write_packet(&ClientboundCustomPayload::brand(brand)?)
        .await?;
    conn.write_packet(&UpdateEnabledFeatures {
        features: vec![Identifier::new("minecraft:vanilla")?],
    })
    .await?;
    conn.write_packet(&ClientboundKnownPacks {
        packs: vec![core.clone()],
    })
    .await?;

    let mut messages = Vec::new();
    let packs: ServerboundKnownPacks =
        timeout(known_packs_timeout, read_until(conn, &mut messages))
            .await
            .map_err(|_| Error::from(io::Error::from(io::ErrorKind::TimedOut)))??;
    if !packs.packs.contains(&core) {
        return Err(Error::InvalidValue("client lacks the minecraft:core pack"));
    }

    for registry in registries.packets() {
        conn.write_packet(&registry).await?;
    }
    conn.write_packet(&registries.tags_packet()).await?;
    conn.write_packet(&FinishConfiguration).await?;

    read_until::<_, AckFinishConfiguration>(conn, &mut messages).await?;
    conn.set_state(State::Play);
    Ok(messages)
}

/// How many plugin messages the client may send while it joins.
pub const MAX_PLUGIN_MESSAGES: usize = 64;
/// How many bytes of plugin messages (the data of all of them) the client may send while it joins.
pub const MAX_PLUGIN_BYTES: usize = 64 * 1024;

/// Reads until a `P` arrives. Plugin messages on the way are added to `messages`; client
/// information is ignored.
async fn read_until<S, P>(conn: &mut Connection<S>, messages: &mut Vec<PluginMessage>) -> Result<P>
where
    S: AsyncRead + AsyncWrite + Unpin,
    P: Packet + Decode,
{
    use crate::protocol::ids::configuration::serverbound as id;
    loop {
        let body = conn.read_frame().await?;
        let (packet_id, mut payload) = split_packet_id(&body)?;
        match packet_id {
            x if x == P::ID => return P::decode(&mut payload),
            // ponytail: client settings are dropped until Play needs them
            id::CLIENT_INFORMATION => {}
            id::CUSTOM_PAYLOAD => {
                let message = ServerboundCustomPayload::decode(&mut payload)?;
                let bytes: usize = messages.iter().map(|m| m.data.len()).sum();
                if messages.len() >= MAX_PLUGIN_MESSAGES
                    || bytes + message.data.len() > MAX_PLUGIN_BYTES
                {
                    return Err(Error::InvalidValue(
                        "too many plugin messages in configuration",
                    ));
                }
                messages.push(PluginMessage {
                    channel: message.channel,
                    data: message.data,
                });
            }
            _ => return Err(Error::InvalidValue("unexpected packet in configuration")),
        }
    }
}
