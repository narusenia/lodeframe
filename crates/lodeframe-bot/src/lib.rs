// SPDX-License-Identifier: Apache-2.0 OR MIT
//! A lightweight bot for testing lodeframe servers, written on `lodeframe-protocol` alone.
//!
//! A [`Bot`] logs in with an offline-mode name, waits until the server has put it into the
//! world, and then does what it is told: walk, chat, break and place blocks. It reads every
//! packet the server sends and keeps none of the world, so a hundred of them cost little. The
//! same bot serves the integration tests and the load tests.
//!
//! It speaks to lodeframe servers: login and configuration are done the way they answer them.

use std::{io, time::Duration};

use lodeframe_protocol::{
    BlockPos, Decode, Direction, Encode, FrameDecoder, Nbt, PROTOCOL_VERSION, Packet, Uuid,
    VERSION_NAME, VarInt, Vec3, encode_frame, ids, packet_body,
    packets::{
        configuration::{
            AckFinishConfiguration, ClientboundKnownPacks, FinishConfiguration,
            ServerboundKnownPacks,
        },
        handshake::Intention,
        login::{Hello, LoginAcknowledged, LoginCompression, LoginFinished},
        play::{
            ACTION_START_DESTROY_BLOCK, Chat, KeepAlive, KeepAliveResponse, Login, MovePlayerPos,
            ON_GROUND, PlayerAction, PlayerPosition, UseItemOn,
        },
    },
    split_packet_id,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpStream, ToSocketAddrs},
    time::{Instant, MissedTickBehavior, interval, timeout},
};

pub use lodeframe_protocol::{Error, Result};

/// How long login and configuration may take before the bot gives up.
const JOIN_TIMEOUT: Duration = Duration::from_secs(30);

/// One packet the server sent: its id and the bytes after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// The packet id.
    pub id: i32,
    payload: Vec<u8>,
}

impl Frame {
    fn new(body: &[u8]) -> Result<Self> {
        let (id, payload) = split_packet_id(body)?;
        Ok(Self {
            id,
            payload: payload.to_vec(),
        })
    }

    /// Whether this is a `P`, judging by the id.
    pub fn is<P: Packet>(&self) -> bool {
        self.id == P::ID
    }

    /// Decodes the packet as a `P`.
    pub fn decode<P: Packet + Decode>(&self) -> Result<P> {
        P::decode(&mut self.payload.as_slice())
    }

    /// The bytes after the packet id.
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    /// The chat line, if this is a `DisguisedChat`.
    pub fn chat_line(&self) -> Option<ChatLine> {
        if self.id != ids::play::clientbound::DISGUISED_CHAT {
            return None;
        }
        let raw = RawChat::decode(&mut self.payload.as_slice()).ok()?;
        Some(ChatLine {
            name: plain_text(&raw.name),
            text: plain_text(&raw.message),
        })
    }
}

impl Frame {
    /// The text, if this is a `SystemChat` that is not above the hotbar.
    pub fn system_message(&self) -> Option<String> {
        if self.id != ids::play::clientbound::SYSTEM_CHAT {
            return None;
        }
        let raw = RawSystemChat::decode(&mut self.payload.as_slice()).ok()?;
        (!raw.overlay).then(|| plain_text(&raw.content))
    }
}

/// What a player said, as the bot sees it: the sender's name and the text, without styles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatLine {
    /// The sender's name.
    pub name: String,
    /// The text.
    pub text: String,
}

/// A `DisguisedChat` as it is on the wire. Components are kept as NBT: the bot only reads the
/// text out of them.
#[derive(Decode)]
#[lodeframe(crate = lodeframe_protocol)]
struct RawChat {
    message: Nbt,
    #[allow(dead_code, reason = "part of the wire format")]
    chat_type: VarInt,
    name: Nbt,
    #[allow(dead_code, reason = "part of the wire format")]
    target_name: Option<Nbt>,
}

/// A `SystemChat` as it is on the wire.
#[derive(Decode)]
#[lodeframe(crate = lodeframe_protocol)]
struct RawSystemChat {
    content: Nbt,
    overlay: bool,
}

/// The text of a component: a bare string, or the `text` of a compound. Children are left out.
fn plain_text(nbt: &Nbt) -> String {
    match nbt {
        Nbt::String(s) => s.clone(),
        Nbt::Compound(c) => match c.get("text") {
            Some(Nbt::String(s)) => s.clone(),
            _ => String::new(),
        },
        _ => String::new(),
    }
}

/// Where bot `index` stands when the bots are spread out: on a grid, `spacing` chunks apart, 32 to
/// a row, starting at the spawn, in the middle of a chunk so that the walk stays inside it.
///
/// With a `spacing` larger than the server's entity view distance no two bots see each other.
pub fn spread_position(index: u32, spacing: u32) -> Vec3 {
    let step = f64::from(spacing) * 16.0;
    Vec3::new(
        f64::from(index % 32) * step + 8.5,
        -60.0,
        f64::from(index / 32) * step + 8.5,
    )
}

/// A logged-in player. Everything it does goes over the connection `S`.
pub struct Bot<S = TcpStream> {
    stream: S,
    decoder: FrameDecoder,
    threshold: Option<usize>,
    name: String,
    uuid: Uuid,
    entity_id: i32,
    position: Vec3,
    sequence: i32,
    received: u64,
}

impl Bot<TcpStream> {
    /// Connects to `addr` and logs in as `name`. Returns when the server has put the bot into
    /// the world.
    pub async fn connect(addr: impl ToSocketAddrs, name: &str) -> Result<Self> {
        let stream = TcpStream::connect(addr).await?;
        stream.set_nodelay(true)?;
        Self::login(stream, name).await
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> Bot<S> {
    /// Logs in as `name` over `stream`: handshake, login, configuration, and the first packets
    /// of the play state. Gives up after 30 seconds.
    pub async fn login(stream: S, name: &str) -> Result<Self> {
        let mut bot = Self {
            stream,
            decoder: FrameDecoder::new(),
            threshold: None,
            name: name.into(),
            uuid: Uuid::offline(name),
            entity_id: 0,
            position: Vec3::ZERO,
            sequence: 0,
            received: 0,
        };
        timeout(JOIN_TIMEOUT, bot.join())
            .await
            .map_err(|_| Error::from(io::Error::from(io::ErrorKind::TimedOut)))??;
        Ok(bot)
    }

    async fn join(&mut self) -> Result<()> {
        self.send(&Intention {
            protocol_version: VarInt(PROTOCOL_VERSION),
            server_address: "localhost".into(),
            server_port: 25565,
            next_state: VarInt(2),
        })
        .await?;
        self.send(&Hello {
            name: self.name.clone(),
            uuid: self.uuid,
        })
        .await?;
        loop {
            let frame = self.read_frame().await?;
            if frame.is::<LoginCompression>() {
                let threshold =
                    usize::try_from(frame.decode::<LoginCompression>()?.threshold.0).ok();
                self.threshold = threshold;
                self.decoder.set_compression(threshold.is_some());
            } else if frame.is::<LoginFinished>() {
                self.send(&LoginAcknowledged).await?;
                break;
            } else {
                return Err(Error::InvalidValue("unexpected packet in login"));
            }
        }
        loop {
            let frame = self.read_frame().await?;
            if frame.is::<ClientboundKnownPacks>() {
                // the server must hear the packs it offered, or it will not send the registries
                let packs = frame.decode::<ClientboundKnownPacks>()?.packs;
                debug_assert!(packs.iter().all(|p| p.version == VERSION_NAME));
                self.send(&ServerboundKnownPacks { packs }).await?;
            } else if frame.is::<FinishConfiguration>() {
                self.send(&AckFinishConfiguration).await?;
                break;
            }
            // features, registries and tags: nothing to keep
        }
        let (mut in_world, mut placed) = (false, false);
        while !(in_world && placed) {
            let frame = self.recv().await?;
            if frame.is::<Login>() {
                self.entity_id = frame.decode::<Login>()?.entity_id;
                in_world = true;
            } else if frame.is::<PlayerPosition>() {
                self.position = frame.decode::<PlayerPosition>()?.position;
                placed = true;
            }
        }
        Ok(())
    }

    /// The name the bot logged in with.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The bot's UUID.
    pub fn uuid(&self) -> Uuid {
        self.uuid
    }

    /// The bot's entity id, which other players' packets use for it.
    pub fn entity_id(&self) -> i32 {
        self.entity_id
    }

    /// Where the bot is: where the server put it, then wherever it last moved to.
    pub fn position(&self) -> Vec3 {
        self.position
    }

    /// How many packets the bot has read from the server.
    pub fn received(&self) -> u64 {
        self.received
    }

    /// Reads the next packet. Does not answer anything, and is safe to cancel, so it can sit in
    /// a `select!`. Call [`answer`](Self::answer) with what it returns.
    pub async fn read_frame(&mut self) -> Result<Frame> {
        let mut chunk = [0u8; 8192];
        loop {
            if let Some(body) = self.decoder.next_frame()? {
                self.received += 1;
                return Frame::new(&body);
            }
            let n = self.stream.read(&mut chunk).await?;
            if n == 0 {
                return Err(io::Error::from(io::ErrorKind::UnexpectedEof).into());
            }
            self.decoder.push(&chunk[..n]);
        }
    }

    /// Answers `frame` if the server expects it to be: a keepalive is echoed.
    pub async fn answer(&mut self, frame: &Frame) -> Result<()> {
        if frame.is::<KeepAlive>() {
            let id = frame.decode::<KeepAlive>()?.id;
            self.send(&KeepAliveResponse { id }).await?;
        }
        Ok(())
    }

    /// Reads the next packet and answers it, see [`answer`](Self::answer).
    pub async fn recv(&mut self) -> Result<Frame> {
        let frame = self.read_frame().await?;
        self.answer(&frame).await?;
        Ok(frame)
    }

    /// Reads packets until `pick` returns something for one, which is returned. Packets read on
    /// the way are dropped. Fails with `TimedOut` after `limit`, so a test waits for a packet
    /// that never comes instead of for ever.
    pub async fn recv_until<T>(
        &mut self,
        limit: Duration,
        mut pick: impl FnMut(&Frame) -> Option<T>,
    ) -> Result<T> {
        let deadline = Instant::now() + limit;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let frame = timeout(left, self.recv())
                .await
                .map_err(|_| Error::from(io::Error::from(io::ErrorKind::TimedOut)))??;
            if let Some(found) = pick(&frame) {
                return Ok(found);
            }
        }
    }

    /// Sends `packet`.
    pub async fn send<P: Packet + Encode>(&mut self, packet: &P) -> Result<()> {
        let body = packet_body(packet)?;
        let mut wire = Vec::new();
        encode_frame(&body, self.threshold, &mut wire)?;
        self.stream.write_all(&wire).await?;
        Ok(self.stream.flush().await?)
    }

    /// Moves to `position`, on the ground.
    pub async fn move_to(&mut self, position: Vec3) -> Result<()> {
        self.position = position;
        self.send(&MovePlayerPos {
            position,
            flags: ON_GROUND,
        })
        .await
    }

    /// Says `text` in the chat.
    pub async fn chat(&mut self, text: &str) -> Result<()> {
        self.send(&Chat {
            message: text.into(),
        })
        .await
    }

    /// Starts digging the block at `pos`, which breaks it in creative mode. Returns the
    /// sequence the server will confirm.
    pub async fn dig(&mut self, pos: BlockPos) -> Result<i32> {
        let sequence = self.next_sequence();
        self.send(&PlayerAction {
            action: VarInt(ACTION_START_DESTROY_BLOCK),
            pos,
            face: Direction::Up.id(),
            sequence: VarInt(sequence),
        })
        .await?;
        Ok(sequence)
    }

    /// Uses the item in hand on the `face` of the block at `pos`. Returns the sequence the
    /// server will confirm.
    pub async fn place(&mut self, pos: BlockPos, face: Direction) -> Result<i32> {
        let sequence = self.next_sequence();
        self.send(&UseItemOn {
            hand: VarInt(0),
            pos,
            face: VarInt(i32::from(face.id())),
            cursor_x: 0.5,
            cursor_y: 0.5,
            cursor_z: 0.5,
            inside: false,
            world_border_hit: false,
            sequence: VarInt(sequence),
        })
        .await?;
        Ok(sequence)
    }

    fn next_sequence(&mut self) -> i32 {
        self.sequence += 1;
        self.sequence
    }

    /// Walks in a circle around where it is for `duration`, chatting, placing and breaking a
    /// block now and then, and reading everything the server sends. For load tests.
    ///
    /// `index` tells the bots of one test apart: each walks at its own phase and builds in its
    /// own column.
    pub async fn wander(&mut self, index: u32, duration: Duration) -> Result<()> {
        let start = Instant::now();
        let home = self.position;
        // its own column, a few blocks from where it walks
        let spot = BlockPos::new(
            home.x.floor() as i32 + (index % 64) as i32,
            -61,
            home.z.floor() as i32 + 8,
        );
        let mut tick = interval(Duration::from_millis(50));
        tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut step = 0u32;
        loop {
            tokio::select! {
                _ = tick.tick() => {
                    if start.elapsed() >= duration {
                        return Ok(());
                    }
                    step += 1;
                    let angle = f64::from(step) * 0.1 + f64::from(index);
                    self.move_to(home + Vec3::new(angle.cos() * 3.0, 0.0, angle.sin() * 3.0)).await?;
                    match step % 40 {
                        0 => self.chat("hi").await?,
                        10 => {
                            self.place(spot, Direction::Up).await?;
                        }
                        30 => {
                            self.dig(spot.offset(Direction::Up)).await?;
                        }
                        _ => {}
                    }
                }
                frame = self.read_frame() => {
                    let frame = frame?;
                    self.answer(&frame).await?;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use lodeframe_protocol::Compound;
    use tokio::io::{DuplexStream, duplex};

    use super::*;

    fn bot(stream: DuplexStream, threshold: Option<usize>) -> Bot<DuplexStream> {
        let mut decoder = FrameDecoder::new();
        decoder.set_compression(threshold.is_some());
        Bot {
            stream,
            decoder,
            threshold,
            name: "bot".into(),
            uuid: Uuid::offline("bot"),
            entity_id: 1,
            position: Vec3::ZERO,
            sequence: 0,
            received: 0,
        }
    }

    async fn write_body(server: &mut DuplexStream, body: &[u8], threshold: Option<usize>) {
        let mut wire = Vec::new();
        encode_frame(body, threshold, &mut wire).unwrap();
        server.write_all(&wire).await.unwrap();
    }

    #[tokio::test]
    async fn frames_are_read_with_and_without_compression() {
        for threshold in [None, Some(64)] {
            let (client, mut server) = duplex(1 << 16);
            let mut bot = bot(client, threshold);
            // a small packet and one that is compressed when compression is on
            let small = [&[0x20][..], b"small"].concat();
            let big = [&[0x21][..], &[7u8; 500]].concat();
            write_body(&mut server, &small, threshold).await;
            write_body(&mut server, &big, threshold).await;

            let first = bot.read_frame().await.unwrap();
            let second = bot.read_frame().await.unwrap();

            assert_eq!((first.id, first.payload()), (0x20, &b"small"[..]));
            assert_eq!((second.id, second.payload().len()), (0x21, 500));
            assert_eq!(bot.received(), 2);
        }
    }

    #[tokio::test]
    async fn a_keepalive_is_echoed() {
        let (client, mut server) = duplex(1 << 16);
        let mut bot = bot(client, None);
        write_body(
            &mut server,
            &packet_body(&KeepAlive { id: 42 }).unwrap(),
            None,
        )
        .await;

        let frame = bot.recv().await.unwrap();

        assert!(frame.is::<KeepAlive>());
        let mut reply = FrameDecoder::new();
        let mut chunk = [0u8; 64];
        let n = server.read(&mut chunk).await.unwrap();
        reply.push(&chunk[..n]);
        let answer = Frame::new(&reply.next_frame().unwrap().unwrap()).unwrap();
        assert_eq!(answer.decode::<KeepAliveResponse>().unwrap().id, 42);
    }

    #[tokio::test]
    async fn waiting_for_a_packet_that_never_comes_times_out() {
        let (client, _server) = duplex(1 << 16);
        let mut bot = bot(client, None);

        let result = bot
            .recv_until(Duration::from_millis(50), |_| Some(()))
            .await;

        assert!(matches!(result, Err(Error::Io(e)) if e.kind() == io::ErrorKind::TimedOut));
    }

    fn chat_frame(message: Nbt, name: Nbt) -> Frame {
        let mut payload = Vec::new();
        message.encode(&mut payload).unwrap();
        VarInt(1).encode(&mut payload).unwrap();
        name.encode(&mut payload).unwrap();
        // no target name
        payload.push(0);
        Frame {
            id: ids::play::clientbound::DISGUISED_CHAT,
            payload,
        }
    }

    #[test]
    fn a_chat_line_is_read_from_plain_and_styled_components() {
        let plain = chat_frame(Nbt::from("hello"), Nbt::from("Alice"));
        assert_eq!(
            plain.chat_line(),
            Some(ChatLine {
                name: "Alice".into(),
                text: "hello".into()
            })
        );

        let mut styled = Compound::new();
        styled.insert("text", Nbt::from("hello"));
        styled.insert("color", Nbt::from("red"));
        let styled = chat_frame(Nbt::Compound(styled), Nbt::from("Alice"));
        assert_eq!(styled.chat_line().unwrap().text, "hello");

        let other = Frame {
            id: ids::play::clientbound::KEEP_ALIVE,
            payload: Vec::new(),
        };
        assert_eq!(other.chat_line(), None);
    }

    #[test]
    fn spread_bots_stand_in_their_own_chunks_beyond_each_others_sight() {
        let chunk = |p: Vec3| ((p.x.floor() as i32) >> 4, (p.z.floor() as i32) >> 4);
        let spacing = 4u32;
        let spots: Vec<_> = (0..70)
            .map(|i| chunk(spread_position(i, spacing)))
            .collect();
        for (i, a) in spots.iter().enumerate() {
            for b in &spots[i + 1..] {
                let apart = a.0.abs_diff(b.0).max(a.1.abs_diff(b.1));
                assert!(apart >= spacing, "{a:?} and {b:?} are {apart} chunks apart");
            }
        }
        // and the walk of 3 blocks around it stays in the chunk
        let p = spread_position(5, spacing);
        assert_eq!(chunk(p + Vec3::new(3.0, 0.0, 3.0)), chunk(p));
        assert_eq!(chunk(p - Vec3::new(3.0, 0.0, 3.0)), chunk(p));
    }

    #[test]
    fn a_system_message_is_read_unless_it_is_an_overlay() {
        let frame = |overlay: bool| {
            let mut payload = Vec::new();
            Nbt::from("welcome").encode(&mut payload).unwrap();
            payload.push(u8::from(overlay));
            Frame {
                id: ids::play::clientbound::SYSTEM_CHAT,
                payload,
            }
        };
        assert_eq!(frame(false).system_message().as_deref(), Some("welcome"));
        assert_eq!(frame(true).system_message(), None);
    }
}
