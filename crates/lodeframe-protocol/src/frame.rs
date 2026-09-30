// SPDX-License-Identifier: Apache-2.0 OR MIT
//! Packet framing: a length prefix and, once enabled, zlib compression.
//!
//! Plain frame: `VarInt length | body`. With compression on:
//! `VarInt length | VarInt uncompressed length | data`, where an uncompressed length
//! of 0 means `data` is the body as is (it was under the sender's threshold).
//! A body is a packet id followed by its payload.

use std::io::{Read, Write};

use flate2::{Compression, read::ZlibDecoder, write::ZlibEncoder};

use crate::{Decode, Encode, Error, Packet, Result, VarInt};

/// Largest frame on the wire, in bytes: what a 3-byte length prefix can say.
pub const MAX_FRAME_LEN: usize = (1 << 21) - 1;
/// Largest body after decompression, in bytes.
pub const MAX_BODY_LEN: usize = 8 * 1024 * 1024;

/// Writes `body` to `out` as one frame.
///
/// With `threshold` set, bodies of at least that many bytes are compressed.
pub fn encode_frame(body: &[u8], threshold: Option<usize>, out: &mut Vec<u8>) -> Result<()> {
    if body.len() > MAX_BODY_LEN {
        return Err(Error::LengthTooLarge {
            len: body.len(),
            max: MAX_BODY_LEN,
        });
    }
    let mut inner = Vec::new();
    match threshold {
        None => inner.extend_from_slice(body),
        Some(t) if body.len() < t => {
            VarInt(0).encode(&mut inner)?;
            inner.extend_from_slice(body);
        }
        Some(_) => {
            // bounded by MAX_BODY_LEN, fits in i32
            VarInt(body.len() as i32).encode(&mut inner)?;
            let mut z = ZlibEncoder::new(inner, Compression::default());
            z.write_all(body)?;
            inner = z.finish()?;
        }
    }
    if inner.len() > MAX_FRAME_LEN {
        return Err(Error::LengthTooLarge {
            len: inner.len(),
            max: MAX_FRAME_LEN,
        });
    }
    // bounded by MAX_FRAME_LEN, fits in i32
    VarInt(inner.len() as i32).encode(out)?;
    out.extend_from_slice(&inner);
    Ok(())
}

/// Builds a body (packet id + payload) for `packet`.
pub fn packet_body<P: Packet + Encode>(packet: &P) -> Result<Vec<u8>> {
    let mut body = Vec::new();
    VarInt(P::ID).encode(&mut body)?;
    packet.encode(&mut body)?;
    Ok(body)
}

/// Splits a body into its packet id and payload.
pub fn split_packet_id(mut body: &[u8]) -> Result<(i32, &[u8])> {
    let id = VarInt::decode(&mut body)?.0;
    Ok((id, body))
}

/// Reassembles frames from bytes as they arrive.
#[derive(Debug, Default)]
pub struct FrameDecoder {
    // ponytail: drain from the front is O(n) per frame; a ring buffer if profiles show it
    buf: Vec<u8>,
    compressed: bool,
}

impl FrameDecoder {
    /// A decoder with compression off.
    pub fn new() -> Self {
        Self::default()
    }

    /// Switches whether incoming frames carry the uncompressed-length field.
    pub fn set_compression(&mut self, on: bool) {
        self.compressed = on;
    }

    /// Adds received bytes.
    pub fn push(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    /// Takes the next complete body, or `None` if more bytes are needed.
    ///
    /// A length prefix over 3 bytes (more than [`MAX_FRAME_LEN`]) is rejected as soon as it is
    /// read, without waiting for the body.
    pub fn next_frame(&mut self) -> Result<Option<Vec<u8>>> {
        let Some((len, prefix)) = peek_length(&self.buf)? else {
            return Ok(None);
        };
        if len == 0 {
            return Err(Error::InvalidValue("empty frame"));
        }
        let end = prefix + len;
        if self.buf.len() < end {
            return Ok(None);
        }
        let body = self.open(&self.buf[prefix..end]);
        self.buf.drain(..end);
        body.map(Some)
    }

    fn open(&self, mut frame: &[u8]) -> Result<Vec<u8>> {
        if !self.compressed {
            return Ok(frame.to_vec());
        }
        let size = usize::try_from(VarInt::decode(&mut frame)?.0)
            .map_err(|_| Error::InvalidValue("negative uncompressed length"))?;
        if size == 0 {
            return Ok(frame.to_vec());
        }
        if size > MAX_BODY_LEN {
            return Err(Error::LengthTooLarge {
                len: size,
                max: MAX_BODY_LEN,
            });
        }
        // Stop one byte past the declared size so a zip bomb costs at most size + 1 bytes.
        let mut body = Vec::with_capacity(size.min(frame.len().saturating_mul(4)));
        ZlibDecoder::new(frame)
            .take(size as u64 + 1)
            .read_to_end(&mut body)
            .map_err(|_| Error::InvalidValue("corrupt zlib data"))?;
        if body.len() != size {
            return Err(Error::InvalidValue("uncompressed length mismatch"));
        }
        Ok(body)
    }
}

/// Reads the length prefix without consuming it: `(length, prefix bytes)`.
fn peek_length(buf: &[u8]) -> Result<Option<(usize, usize)>> {
    let mut len = 0usize;
    for i in 0..3 {
        let Some(&b) = buf.get(i) else {
            return Ok(None);
        };
        len |= usize::from(b & 0x7f) << (7 * i);
        if b & 0x80 == 0 {
            return Ok(Some((len, i + 1)));
        }
    }
    Err(Error::VarIntTooLong)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(body: &[u8], threshold: Option<usize>) -> Vec<u8> {
        let mut wire = Vec::new();
        encode_frame(body, threshold, &mut wire).unwrap();
        let mut d = FrameDecoder::new();
        d.set_compression(threshold.is_some());
        d.push(&wire);
        let out = d.next_frame().unwrap().unwrap();
        assert!(d.next_frame().unwrap().is_none());
        out
    }

    #[test]
    fn frames_roundtrip_with_and_without_compression() {
        let big = vec![7u8; 5000];
        let small = [0u8, 1, 2];
        assert_eq!(roundtrip(&small, None), small);
        assert_eq!(roundtrip(&big, None), big);
        assert_eq!(roundtrip(&small, Some(256)), small);
        assert_eq!(roundtrip(&big, Some(256)), big);
        assert_eq!(roundtrip(&big, Some(0)), big);
    }

    #[test]
    fn compression_only_applies_from_the_threshold() {
        let (mut below, mut at) = (Vec::new(), Vec::new());
        encode_frame(&[9; 9], Some(10), &mut below).unwrap();
        encode_frame(&[9; 10], Some(10), &mut at).unwrap();
        // length, uncompressed length 0, body as is
        assert_eq!(&below[..2], [10, 0]);
        assert_eq!(below.len(), 11);
        assert_eq!(at[1], 10); // declared uncompressed length
    }

    #[test]
    fn a_frame_split_at_every_byte_still_decodes() {
        let mut wire = Vec::new();
        encode_frame(&[5; 300], Some(64), &mut wire).unwrap();
        encode_frame(&[6; 2], Some(64), &mut wire).unwrap();
        let mut d = FrameDecoder::new();
        d.set_compression(true);
        let mut got = Vec::new();
        for b in &wire {
            d.push(&[*b]);
            while let Some(f) = d.next_frame().unwrap() {
                got.push(f);
            }
        }
        assert_eq!(got, [vec![5; 300], vec![6; 2]]);
    }

    #[test]
    fn an_oversized_length_is_rejected_before_the_body_arrives() {
        let mut d = FrameDecoder::new();
        d.push(&[0x80, 0x80, 0x80]); // continuation on the third byte: > 3 bytes
        assert!(matches!(d.next_frame(), Err(Error::VarIntTooLong)));

        // The largest 3-byte prefix is exactly MAX_FRAME_LEN: accepted, body still awaited.
        let mut d = FrameDecoder::new();
        d.push(&[0xff, 0xff, 0x7f]);
        assert!(d.next_frame().unwrap().is_none());
    }

    #[test]
    fn a_declared_size_over_the_limit_is_rejected_before_inflating() {
        let mut inner = Vec::new();
        VarInt(MAX_BODY_LEN as i32 + 1).encode(&mut inner).unwrap();
        inner.extend_from_slice(&[0x78, 0x9c]);
        let mut wire = Vec::new();
        VarInt(inner.len() as i32).encode(&mut wire).unwrap();
        wire.extend_from_slice(&inner);
        let mut d = FrameDecoder::new();
        d.set_compression(true);
        d.push(&wire);
        assert!(matches!(d.next_frame(), Err(Error::LengthTooLarge { .. })));
    }

    #[test]
    fn a_zip_bomb_is_rejected() {
        // Declares 10 bytes but inflates to 100 000.
        let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
        z.write_all(&vec![0u8; 100_000]).unwrap();
        let data = z.finish().unwrap();
        let mut inner = vec![10];
        inner.extend_from_slice(&data);
        let mut wire = Vec::new();
        VarInt(inner.len() as i32).encode(&mut wire).unwrap();
        wire.extend_from_slice(&inner);
        let mut d = FrameDecoder::new();
        d.set_compression(true);
        d.push(&wire);
        assert!(matches!(d.next_frame(), Err(Error::InvalidValue(_))));
    }

    #[test]
    fn corrupt_zlib_and_empty_frames_are_rejected() {
        let mut d = FrameDecoder::new();
        d.set_compression(true);
        d.push(&[4, 3, 1, 2, 3]);
        assert!(d.next_frame().is_err());
        let mut d = FrameDecoder::new();
        d.push(&[0]);
        assert!(d.next_frame().is_err());
    }

    #[test]
    fn packet_id_splits_off_the_body() {
        assert_eq!(split_packet_id(&[0x05, 1, 2]).unwrap(), (5, &[1, 2][..]));
        assert!(split_packet_id(&[]).is_err());
    }
}
