// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Configuration: agree on data packs, send the registries, hand over to Play.

use std::{io, time::Duration};

use tokio::{
    io::{AsyncRead, AsyncWrite},
    time::timeout,
};

use crate::{
    net::Connection,
    protocol::{
        Decode, Error, Identifier, Packet, Result, State, VERSION_NAME,
        packets::configuration::{
            AckFinishConfiguration, ClientboundCustomPayload, ClientboundKnownPacks,
            FinishConfiguration, KnownPack, ServerboundKnownPacks, UpdateEnabledFeatures,
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
pub async fn run<S: AsyncRead + AsyncWrite + Unpin>(
    conn: &mut Connection<S>,
    registries: &Registries,
    brand: &str,
) -> Result<()> {
    run_with(conn, registries, brand, KNOWN_PACKS_TIMEOUT).await
}

/// How long [`run`] waits for the client's answer to the known packs.
pub const KNOWN_PACKS_TIMEOUT: Duration = Duration::from_secs(30);

/// Like [`run`], waiting `known_packs_timeout` for the client's answer to the known packs
/// (the connection's read timeout applies to every other packet).
pub async fn run_with<S: AsyncRead + AsyncWrite + Unpin>(
    conn: &mut Connection<S>,
    registries: &Registries,
    brand: &str,
    known_packs_timeout: Duration,
) -> Result<()> {
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

    let packs: ServerboundKnownPacks = timeout(known_packs_timeout, read_until(conn))
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

    read_until::<_, AckFinishConfiguration>(conn).await?;
    conn.set_state(State::Play);
    Ok(())
}

/// Reads until a `P` arrives. Client information and plugin messages on the way are ignored.
async fn read_until<S, P>(conn: &mut Connection<S>) -> Result<P>
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
            // ponytail: client settings and brand are dropped until Play needs them
            id::CLIENT_INFORMATION | id::CUSTOM_PAYLOAD => {}
            _ => return Err(Error::InvalidValue("unexpected packet in configuration")),
        }
    }
}
