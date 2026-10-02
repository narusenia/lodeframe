// SPDX-License-Identifier: Apache-2.0 OR MIT
//! The HAProxy PROXY protocol header, v1 and v2: what a load balancer writes at the start of a
//! connection to say who the client is. [`scan`] looks at the bytes read so far and says
//! whether they are such a header, are not, or are not enough yet; nothing here reads a socket.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

const V1_PREFIX: &[u8] = b"PROXY ";
/// The longest v1 line, `\r\n` included.
const V1_MAX: usize = 107;
const V2_SIGNATURE: &[u8] = b"\r\n\r\n\0\r\nQUIT\n";
const V2_FIXED: usize = 16;
/// The longest v2 body: the address (up to 36 bytes) and the TLVs a balancer may add after it.
const V2_MAX_BODY: usize = 1024;

/// Whether the bytes begin like a header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Sniff {
    /// They match a header's signature so far, and more is needed to tell.
    Maybe,
    /// They are not a header: a Minecraft packet.
    No,
    /// A whole signature, `PROXY ` or v2's twelve bytes, is there.
    Yes,
}

/// Compares `buf` with the two signatures without looking at anything past them, which is all
/// that is done with a connection that nothing vouches for.
pub(crate) fn sniff(buf: &[u8]) -> Sniff {
    let starts = |signature: &[u8]| {
        let n = buf.len().min(signature.len());
        buf[..n] == signature[..n]
    };
    let (v1, v2) = (starts(V1_PREFIX), starts(V2_SIGNATURE));
    if !v1 && !v2 {
        Sniff::No
    } else if (v1 && buf.len() >= V1_PREFIX.len()) || (v2 && buf.len() >= V2_SIGNATURE.len()) {
        Sniff::Yes
    } else {
        Sniff::Maybe
    }
}

/// What the bytes read so far are.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Scan {
    /// Not a header.
    NotHeader,
    /// A header, but not all of it yet.
    NeedMore,
    /// A whole header of `len` bytes, saying the client is `addr`. `None` when it says nothing
    /// about the client (a local check, an unknown protocol), so the socket's peer stands.
    Header {
        len: usize,
        addr: Option<SocketAddr>,
    },
    /// Looks like a header and is wrong.
    Bad(&'static str),
}

pub(crate) fn scan(buf: &[u8]) -> Scan {
    match sniff(buf) {
        Sniff::No => Scan::NotHeader,
        Sniff::Maybe => Scan::NeedMore,
        Sniff::Yes if buf.starts_with(V1_PREFIX) => v1(buf),
        Sniff::Yes => v2(buf),
    }
}

fn v1(buf: &[u8]) -> Scan {
    let window = &buf[..buf.len().min(V1_MAX)];
    let Some(end) = window.windows(2).position(|w| w == b"\r\n") else {
        return if buf.len() >= V1_MAX {
            Scan::Bad("PROXY v1 line too long")
        } else {
            Scan::NeedMore
        };
    };
    match v1_line(&buf[..end]) {
        Ok(addr) => Scan::Header { len: end + 2, addr },
        Err(why) => Scan::Bad(why),
    }
}

/// `PROXY TCP4 <src> <dst> <sport> <dport>`, or `PROXY UNKNOWN` with anything after it.
fn v1_line(line: &[u8]) -> Result<Option<SocketAddr>, &'static str> {
    let bad = "PROXY v1 line malformed";
    let line = std::str::from_utf8(line).map_err(|_| bad)?;
    let parts: Vec<&str> = line.split(' ').collect();
    match parts[..] {
        [_, "UNKNOWN", ..] => Ok(None),
        [_, proto @ ("TCP4" | "TCP6"), src, dst, src_port, dst_port] => {
            let ip = |s: &str| match proto {
                "TCP4" => s.parse::<Ipv4Addr>().map(IpAddr::from).map_err(|_| bad),
                _ => s.parse::<Ipv6Addr>().map(IpAddr::from).map_err(|_| bad),
            };
            // `u16::from_str` would take a sign
            let port = |s: &str| {
                if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(bad);
                }
                s.parse::<u16>().map_err(|_| bad)
            };
            ip(dst)?;
            port(dst_port)?;
            Ok(Some(SocketAddr::new(ip(src)?, port(src_port)?)))
        }
        _ => Err(bad),
    }
}

fn v2(buf: &[u8]) -> Scan {
    if buf.len() < V2_FIXED {
        return Scan::NeedMore;
    }
    // the high nibble is the version, the low one the command: 0 local, 1 proxy
    let (version, command) = (buf[12] >> 4, buf[12] & 0x0f);
    if version != 2 || command > 1 {
        return Scan::Bad("PROXY v2 version or command unknown");
    }
    let body_len = usize::from(u16::from_be_bytes([buf[14], buf[15]]));
    if body_len > V2_MAX_BODY {
        return Scan::Bad("PROXY v2 header too long");
    }
    let len = V2_FIXED + body_len;
    if buf.len() < len {
        return Scan::NeedMore;
    }
    let body = &buf[V2_FIXED..len];
    // the high nibble is the address family, the low one the transport: 1 stream
    let (family, transport) = (buf[13] >> 4, buf[13] & 0x0f);
    let addr = match (command, family, transport) {
        (1, 1, 1) => {
            let Some(body) = body.get(..12) else {
                return Scan::Bad("PROXY v2 address too short");
            };
            let ip = Ipv4Addr::new(body[0], body[1], body[2], body[3]);
            Some(SocketAddr::new(
                ip.into(),
                u16::from_be_bytes([body[8], body[9]]),
            ))
        }
        (1, 2, 1) => {
            let Some(body) = body.get(..36) else {
                return Scan::Bad("PROXY v2 address too short");
            };
            let mut ip = [0u8; 16];
            ip.copy_from_slice(&body[..16]);
            Some(SocketAddr::new(
                Ipv6Addr::from(ip).into(),
                u16::from_be_bytes([body[32], body[33]]),
            ))
        }
        // local, unspecified, unix, datagram: nothing about a TCP client
        _ => None,
    };
    Scan::Header { len, addr }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v2_header(command: u8, family_transport: u8, body: &[u8]) -> Vec<u8> {
        let mut h = V2_SIGNATURE.to_vec();
        h.push(0x20 | command);
        h.push(family_transport);
        h.extend((body.len() as u16).to_be_bytes());
        h.extend(body);
        h
    }

    fn v4_body(src: [u8; 4], port: u16) -> Vec<u8> {
        let mut b = src.to_vec();
        b.extend([10, 0, 0, 1]);
        b.extend(port.to_be_bytes());
        b.extend(25565u16.to_be_bytes());
        b
    }

    fn header(len: usize, addr: &str) -> Scan {
        Scan::Header {
            len,
            addr: Some(addr.parse().unwrap()),
        }
    }

    #[test]
    fn a_minecraft_packet_is_not_a_header_from_its_first_two_bytes() {
        // a handshake: length, id 0
        assert_eq!(sniff(&[0x10, 0x00]), Sniff::No);
        assert_eq!(sniff(&[0x50, 0x00]), Sniff::No); // length 80 is a 'P'
        assert_eq!(sniff(&[0x0d, 0x00]), Sniff::No); // length 13 is v2's first byte
        assert_eq!(sniff(&[0xfe]), Sniff::No);
        assert_eq!(sniff(b""), Sniff::Maybe);
    }

    #[test]
    fn signatures_are_told_apart_from_the_first_whole_one() {
        assert_eq!(sniff(b"PRO"), Sniff::Maybe);
        assert_eq!(sniff(b"PROXY "), Sniff::Yes);
        assert_eq!(sniff(b"PROXZ"), Sniff::No);
        assert_eq!(sniff(&V2_SIGNATURE[..11]), Sniff::Maybe);
        assert_eq!(sniff(V2_SIGNATURE), Sniff::Yes);
    }

    #[test]
    fn v1_tcp4_and_tcp6_give_the_client() {
        let line = b"PROXY TCP4 203.0.113.7 10.0.0.1 40000 25565\r\n\x10\x00";
        assert_eq!(scan(line), header(line.len() - 2, "203.0.113.7:40000"));
        let line = b"PROXY TCP6 2001:db8::7 ::1 1 25565\r\n";
        assert_eq!(scan(line), header(line.len(), "[2001:db8::7]:1"));
    }

    #[test]
    fn v1_unknown_says_nothing_about_the_client() {
        let line = b"PROXY UNKNOWN\r\n";
        assert_eq!(
            scan(line),
            Scan::Header {
                len: line.len(),
                addr: None
            }
        );
        let line = b"PROXY UNKNOWN ffff:f...f:ffff ffff:f...f:ffff 65535 65535\r\n";
        assert_eq!(
            scan(line),
            Scan::Header {
                len: line.len(),
                addr: None
            }
        );
    }

    #[test]
    fn v1_waits_for_the_end_of_the_line() {
        let line = b"PROXY TCP4 203.0.113.7 10.0.0.1 40000 25565\r\n";
        for n in 0..line.len() {
            assert_eq!(scan(&line[..n]), Scan::NeedMore, "{n}");
        }
    }

    #[test]
    fn v1_that_is_wrong_is_bad() {
        for line in [
            &b"PROXY TCP4 203.0.113.7 10.0.0.1 40000\r\n"[..],
            b"PROXY TCP4 2001:db8::7 10.0.0.1 1 2\r\n",
            b"PROXY TCP6 203.0.113.7 ::1 1 2\r\n",
            b"PROXY TCP4 203.0.113.7 10.0.0.1 +4 2\r\n",
            b"PROXY TCP4 203.0.113.7 10.0.0.1 65536 2\r\n",
            b"PROXY TCP4 203.0.113.7 10.0.0.1  1 2\r\n",
            b"PROXY UDP4 203.0.113.7 10.0.0.1 1 2\r\n",
            b"PROXY \xff\r\n",
            b"PROXY \r\n",
        ] {
            assert!(matches!(scan(line), Scan::Bad(_)), "{line:?}");
        }
        let long = [b"PROXY TCP4 ".as_slice(), &[b'1'; V1_MAX]].concat();
        assert!(matches!(scan(&long), Scan::Bad(_)));
        // the terminator after the longest allowed line is still found
        let mut line = b"PROXY UNKNOWN ".to_vec();
        line.resize(V1_MAX - 2, b'x');
        line.extend(b"\r\n");
        assert_eq!(
            scan(&line),
            Scan::Header {
                len: V1_MAX,
                addr: None
            }
        );
    }

    #[test]
    fn v2_proxy_gives_the_client_and_skips_tlvs() {
        let mut body = v4_body([203, 0, 113, 7], 40000);
        body.extend([0x04, 0x00, 0x02, 0xaa, 0xbb]);
        let h = v2_header(1, 0x11, &body);
        let mut with_data = h.clone();
        with_data.extend([0x10, 0x00]);
        assert_eq!(scan(&with_data), header(h.len(), "203.0.113.7:40000"));

        let mut body = vec![0u8; 36];
        body[15] = 7;
        body[32..34].copy_from_slice(&5u16.to_be_bytes());
        let h = v2_header(1, 0x21, &body);
        assert_eq!(scan(&h), header(h.len(), "[::7]:5"));
    }

    #[test]
    fn v2_that_names_no_tcp_client_says_nothing() {
        for h in [
            v2_header(0, 0x11, &v4_body([1, 2, 3, 4], 5)),
            v2_header(0, 0x00, &[]),
            v2_header(1, 0x00, &[]),
            v2_header(1, 0x31, &[0; 216]),
            v2_header(1, 0x12, &v4_body([1, 2, 3, 4], 5)),
        ] {
            assert_eq!(
                scan(&h),
                Scan::Header {
                    len: h.len(),
                    addr: None
                },
                "{h:?}"
            );
        }
    }

    #[test]
    fn v2_waits_for_all_of_it() {
        let h = v2_header(1, 0x11, &v4_body([1, 2, 3, 4], 5));
        for n in 0..h.len() {
            assert_eq!(scan(&h[..n]), Scan::NeedMore, "{n}");
        }
    }

    #[test]
    fn v2_that_is_wrong_is_bad() {
        let mut version = v2_header(1, 0x11, &v4_body([1, 2, 3, 4], 5));
        version[12] = 0x11;
        let mut command = v2_header(1, 0x11, &v4_body([1, 2, 3, 4], 5));
        command[12] = 0x22;
        for h in [
            version,
            command,
            v2_header(1, 0x11, &[0; 11]),
            v2_header(1, 0x21, &[0; 35]),
            v2_header(1, 0x11, &[0; V2_MAX_BODY + 1]),
        ] {
            assert!(matches!(scan(&h), Scan::Bad(_)), "{h:?}");
        }
    }
}
