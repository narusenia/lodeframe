// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Connections: accept, frame, and sort by handshake.

use std::{
    future::Future,
    io,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::{sleep, timeout},
};

use crate::protocol::{
    Decode, Encode, Error, FrameDecoder, Packet, Result, State, encode_frame, ids, packet_body,
    packets::handshake::Intention, split_packet_id,
};
use crate::proxy_protocol::{Scan, Sniff, scan, sniff};

/// Settings for [`serve`].
#[derive(Debug, Clone)]
pub struct Config {
    /// How long one frame may take to arrive, counting from when it is awaited.
    pub read_timeout: Duration,
    /// Whether to set `TCP_NODELAY` on accepted sockets.
    pub nodelay: bool,
    /// Whether a load balancer writes who the client is at the start of each connection.
    pub proxy_protocol: ProxyProtocol,
}

/// The HAProxy PROXY protocol, v1 and v2: a header at the very start of a connection, before the
/// handshake, with the address of the client a load balancer is passing on.
///
/// Anyone can write such a header, so it is believed only from the addresses in `trusted`,
/// which are the balancers' addresses as the socket sees them. A header from any other address
/// is refused, even a well-formed one. Where the header names a client, that address stands in
/// for the socket's peer: it is what [`serve`] hands to its handler.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub enum ProxyProtocol {
    /// No header is read (the default). One that arrives is not a handshake, and the
    /// connection ends.
    #[default]
    Off,
    /// A connection from `trusted` may begin with a header, and one without is let in as it is.
    /// A header from anywhere else is refused. `trusted` must not be empty.
    Optional {
        /// The addresses the balancers connect from.
        trusted: Vec<IpAddr>,
    },
    /// Every connection must begin with a header from `trusted`; any other is refused, one
    /// from another address without reading it. `trusted` must not be empty.
    Required {
        /// The addresses the balancers connect from.
        trusted: Vec<IpAddr>,
    },
}

impl Default for Config {
    fn default() -> Self {
        Self {
            read_timeout: Duration::from_secs(30),
            nodelay: false,
            proxy_protocol: ProxyProtocol::Off,
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
    proxied: Option<SocketAddr>,
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
            proxied: None,
        }
    }

    /// The address of the client, when a PROXY protocol header said it ([`ProxyProtocol`]).
    /// `None` for a connection that came without one, and for one whose header said nothing
    /// about the client.
    pub fn proxied_addr(&self) -> Option<SocketAddr> {
        self.proxied
    }

    /// Takes `bytes` as read from the stream already, in front of what it yields next.
    fn unread(&mut self, bytes: &[u8]) {
        self.decoder.push(bytes);
    }

    /// Changes how long one frame may take to arrive, from the next read on.
    pub fn set_read_timeout(&mut self, read_timeout: Duration) {
        self.read_timeout = read_timeout;
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

    /// Writes several bodies as frames with one write and one flush, which costs far fewer system
    /// calls than [`write_frame`](Self::write_frame) for each.
    pub async fn write_frames(&mut self, bodies: &[Vec<u8>]) -> Result<()> {
        let mut wire = Vec::new();
        for body in bodies {
            encode_frame(body, self.threshold, &mut wire)?;
        }
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

/// Accepts connections forever, running `handler` for each one after its handshake, with the
/// address the connection came from.
///
/// A connection that fails its handshake or whose handler errors is dropped; the others go on.
/// Each connection runs in a `conn` span carrying the peer address. A peer simply going away
/// is logged at debug, anything else at warn.
pub async fn serve<F, Fut>(listener: TcpListener, config: Config, handler: F) -> io::Result<()>
where
    F: Fn(Connection<TcpStream>, Intention, SocketAddr) -> Fut + Send + Sync + 'static,
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
        if config.nodelay
            && let Err(e) = stream.set_nodelay(true)
        {
            tracing::debug!(error = %e, "set_nodelay failed");
        }
        let handler = handler.clone();
        let (read_timeout, proxy_protocol) = (config.read_timeout, config.proxy_protocol.clone());
        let span = tracing::info_span!("conn", %peer);
        tokio::spawn(
            async move {
                let mut stream = stream;
                let (proxied, leftover) =
                    match admit(&mut stream, peer.ip(), &proxy_protocol, read_timeout).await {
                        Ok(admitted) => admitted,
                        Err(e) if is_disconnect(&e) => {
                            tracing::debug!(error = %e, "connection closed");
                            return;
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "connection refused");
                            return;
                        }
                    };
                let mut conn = Connection::new(stream, read_timeout);
                conn.unread(&leftover);
                conn.proxied = proxied;
                // the client the balancer names stands in for the balancer
                let peer = proxied.unwrap_or(peer);
                let intention = match conn.read_handshake().await {
                    Ok(i) => i,
                    Err(e) => {
                        tracing::debug!(error = %e, "handshake failed");
                        return;
                    }
                };
                match handler(conn, intention, peer).await {
                    Ok(()) => tracing::debug!("connection closed"),
                    Err(e) if is_disconnect(&e) => tracing::debug!(error = %e, "connection closed"),
                    Err(e) => tracing::warn!(error = %e, "connection failed"),
                }
            }
            .instrument(span),
        );
    }
}

/// Applies `mode` to a connection that has just been accepted from `peer`, before anything of it
/// is taken as a Minecraft packet.
///
/// Returns the client a header named, if one did, and the bytes read that are not the header:
/// the start of the handshake, to be [`unread`](Connection::unread).
async fn admit<S: AsyncRead + Unpin>(
    stream: &mut S,
    peer: IpAddr,
    mode: &ProxyProtocol,
    limit: Duration,
) -> Result<(Option<SocketAddr>, Vec<u8>)> {
    let (trusted, required) = match mode {
        ProxyProtocol::Off => return Ok((None, Vec::new())),
        ProxyProtocol::Optional { trusted } => (trusted, false),
        ProxyProtocol::Required { trusted } => (trusted, true),
    };
    // `::ffff:127.0.0.1` on a dual-stack socket is 127.0.0.1
    let from_trusted = trusted
        .iter()
        .any(|t| t.to_canonical() == peer.to_canonical());
    if required && !from_trusted {
        return Err(Error::InvalidValue(
            "a PROXY header is required and the connection is not from a trusted proxy",
        ));
    }
    let read = async {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 1024];
        loop {
            if from_trusted {
                match scan(&buf) {
                    Scan::NeedMore => {}
                    Scan::NotHeader if required => {
                        return Err(Error::InvalidValue("a PROXY header is required"));
                    }
                    Scan::NotHeader => return Ok((None, buf)),
                    Scan::Header { len, addr } => return Ok((addr, buf.split_off(len))),
                    Scan::Bad(why) => return Err(Error::InvalidValue(why)),
                }
            } else {
                // nothing vouches for this connection: only look at whether a header begins
                match sniff(&buf) {
                    Sniff::Maybe => {}
                    Sniff::No => return Ok((None, buf)),
                    Sniff::Yes => {
                        return Err(Error::InvalidValue(
                            "a PROXY header from a connection that is not from a trusted proxy",
                        ));
                    }
                }
            }
            let n = stream.read(&mut chunk).await?;
            if n == 0 {
                return Err(io::Error::from(io::ErrorKind::UnexpectedEof).into());
            }
            buf.extend_from_slice(&chunk[..n]);
        }
    };
    timeout(limit, read)
        .await
        .map_err(|_| Error::from(io::Error::from(io::ErrorKind::TimedOut)))?
}
