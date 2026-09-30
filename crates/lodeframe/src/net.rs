// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Connections: accept, frame, and sort by handshake.

use std::{future::Future, io, sync::Arc, time::Duration};

use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::{sleep, timeout},
};

use crate::protocol::{
    Decode, Encode, Error, FrameDecoder, Packet, Result, State, encode_frame, ids, packet_body,
    packets::handshake::Intention, split_packet_id,
};

/// Settings for [`serve`].
#[derive(Debug, Clone)]
pub struct Config {
    /// How long one frame may take to arrive, counting from when it is awaited.
    pub read_timeout: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            read_timeout: Duration::from_secs(30),
        }
    }
}

/// One client connection: frames in, frames out, and the protocol state.
pub struct Connection<S> {
    stream: S,
    decoder: FrameDecoder,
    state: State,
    threshold: Option<usize>,
    read_timeout: Duration,
}

impl<S: AsyncRead + AsyncWrite + Unpin> Connection<S> {
    /// Wraps `stream`, starting in [`State::Handshake`] with compression off.
    pub fn new(stream: S, read_timeout: Duration) -> Self {
        Self {
            stream,
            decoder: FrameDecoder::new(),
            state: State::Handshake,
            threshold: None,
            read_timeout,
        }
    }

    /// The current protocol state.
    pub fn state(&self) -> State {
        self.state
    }

    /// Moves to `state`.
    pub fn set_state(&mut self, state: State) {
        self.state = state;
    }

    /// Turns compression on (bodies of at least `threshold` bytes are compressed) or off.
    ///
    /// Call it right after the packet that announces it has been written or read.
    pub fn set_compression(&mut self, threshold: Option<usize>) {
        self.threshold = threshold;
        self.decoder.set_compression(threshold.is_some());
    }

    /// Reads the next body (packet id + payload).
    ///
    /// Times out with `io::ErrorKind::TimedOut`; a closed stream is `UnexpectedEof`.
    pub async fn read_frame(&mut self) -> Result<Vec<u8>> {
        let read = async {
            let mut chunk = [0u8; 4096];
            loop {
                if let Some(body) = self.decoder.next_frame()? {
                    return Ok(body);
                }
                let n = self.stream.read(&mut chunk).await?;
                if n == 0 {
                    return Err(io::Error::from(io::ErrorKind::UnexpectedEof).into());
                }
                self.decoder.push(&chunk[..n]);
            }
        };
        timeout(self.read_timeout, read)
            .await
            .map_err(|_| Error::from(io::Error::from(io::ErrorKind::TimedOut)))?
    }

    /// Writes one body (packet id + payload) as a frame.
    // ponytail: no write timeout; a client that stops reading holds only its own task
    pub async fn write_frame(&mut self, body: &[u8]) -> Result<()> {
        let mut wire = Vec::new();
        encode_frame(body, self.threshold, &mut wire)?;
        self.stream.write_all(&wire).await?;
        Ok(self.stream.flush().await?)
    }

    /// Encodes and writes `packet`.
    pub async fn write_packet<P: Packet + Encode>(&mut self, packet: &P) -> Result<()> {
        self.write_frame(&packet_body(packet)?).await
    }

    /// Reads a packet that must be `P`; any other id is an error.
    pub async fn read_packet<P: Packet + Decode>(&mut self) -> Result<P> {
        let body = self.read_frame().await?;
        let (id, mut payload) = split_packet_id(&body)?;
        if id != P::ID {
            return Err(Error::InvalidValue("unexpected packet id"));
        }
        P::decode(&mut payload)
    }

    /// Reads the handshake and moves to Status or Login as the client asked.
    pub async fn read_handshake(&mut self) -> Result<Intention> {
        if self.state != State::Handshake {
            return Err(Error::InvalidValue("not in the handshake state"));
        }
        debug_assert_eq!(Intention::ID, ids::handshake::serverbound::INTENTION);
        let intention: Intention = self.read_packet().await?;
        self.state = match intention.next_state.0 {
            1 => State::Status,
            // 3 is a login that follows a transfer
            2 | 3 => State::Login,
            _ => return Err(Error::InvalidValue("handshake next state")),
        };
        Ok(intention)
    }
}

/// Whether `e` is just the peer going away rather than something wrong.
pub(crate) fn is_disconnect(e: &Error) -> bool {
    use io::ErrorKind::{BrokenPipe, ConnectionAborted, ConnectionReset, TimedOut, UnexpectedEof};
    matches!(e, Error::Io(e) if matches!(e.kind(), UnexpectedEof | ConnectionReset | ConnectionAborted | BrokenPipe | TimedOut))
}

/// Accepts connections forever, running `handler` for each one after its handshake.
///
/// A connection that fails its handshake or whose handler errors is dropped; the others go on.
/// Each connection runs in a `conn` span carrying the peer address. A peer simply going away
/// is logged at debug, anything else at warn.
pub async fn serve<F, Fut>(listener: TcpListener, config: Config, handler: F) -> io::Result<()>
where
    F: Fn(Connection<TcpStream>, Intention) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    use tracing::Instrument;

    let handler = Arc::new(handler);
    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(e) => {
                // e.g. out of file descriptors: back off instead of spinning
                tracing::warn!(error = %e, "accept failed");
                sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        let handler = handler.clone();
        let mut conn = Connection::new(stream, config.read_timeout);
        let span = tracing::info_span!("conn", %peer);
        tokio::spawn(
            async move {
                let intention = match conn.read_handshake().await {
                    Ok(i) => i,
                    Err(e) => {
                        tracing::debug!(error = %e, "handshake failed");
                        return;
                    }
                };
                match handler(conn, intention).await {
                    Ok(()) => tracing::debug!("connection closed"),
                    Err(e) if is_disconnect(&e) => tracing::debug!(error = %e, "connection closed"),
                    Err(e) => tracing::warn!(error = %e, "connection failed"),
                }
            }
            .instrument(span),
        );
    }
}
