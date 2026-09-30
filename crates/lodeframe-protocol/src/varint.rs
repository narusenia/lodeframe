use std::io::Write;

use crate::{Decode, Encode, Error, Result};

macro_rules! varint {
    ($(#[$doc:meta])* $name:ident($int:ty, $uint:ty), max_bytes = $max:expr) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
        pub struct $name(pub $int);

        impl Encode for $name {
            fn encode(&self, w: &mut impl Write) -> Result<()> {
                let mut v = self.0 as $uint;
                loop {
                    let byte = (v & 0x7f) as u8;
                    v >>= 7;
                    if v == 0 {
                        return Ok(w.write_all(&[byte])?);
                    }
                    w.write_all(&[byte | 0x80])?;
                }
            }
        }

        impl Decode for $name {
            fn decode(r: &mut &[u8]) -> Result<Self> {
                let mut v: $uint = 0;
                for i in 0..$max {
                    let byte = u8::decode(r)?;
                    v |= <$uint>::from(byte & 0x7f) << (7 * i);
                    if byte & 0x80 == 0 {
                        return Ok(Self(v as $int));
                    }
                }
                Err(Error::VarIntTooLong)
            }
        }
    };
}

varint!(
    /// A variable-length `i32`, 1 to 5 bytes.
    VarInt(i32, u32), max_bytes = 5
);
varint!(
    /// A variable-length `i64`, 1 to 10 bytes.
    VarLong(i64, u64), max_bytes = 10
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::tests::{encoded, roundtrip};

    #[test]
    fn varint_matches_the_documented_examples() {
        for (value, bytes) in [
            (0, &[0x00][..]),
            (1, &[0x01]),
            (127, &[0x7f]),
            (128, &[0x80, 0x01]),
            (255, &[0xff, 0x01]),
            (25565, &[0xdd, 0xc7, 0x01]),
            (2097151, &[0xff, 0xff, 0x7f]),
            (i32::MAX, &[0xff, 0xff, 0xff, 0xff, 0x07]),
            (-1, &[0xff, 0xff, 0xff, 0xff, 0x0f]),
            (i32::MIN, &[0x80, 0x80, 0x80, 0x80, 0x08]),
        ] {
            assert_eq!(encoded(&VarInt(value)), bytes, "encode {value}");
            assert_eq!(
                VarInt::decode(&mut &bytes[..]).unwrap(),
                VarInt(value),
                "decode {value}"
            );
        }
    }

    #[test]
    fn varlong_matches_the_documented_examples() {
        for (value, bytes) in [
            (0, &[0x00][..]),
            (2147483647, &[0xff, 0xff, 0xff, 0xff, 0x07]),
            (
                i64::MAX,
                &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7f],
            ),
            (
                -1,
                &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01],
            ),
            (
                i64::MIN,
                &[0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x01],
            ),
        ] {
            assert_eq!(encoded(&VarLong(value)), bytes, "encode {value}");
            assert_eq!(
                VarLong::decode(&mut &bytes[..]).unwrap(),
                VarLong(value),
                "decode {value}"
            );
        }
    }

    #[test]
    fn roundtrip_around_every_byte_boundary() {
        for shift in 0..31 {
            for delta in [-1_i64, 0, 1] {
                let v = (1_i64 << shift) + delta;
                roundtrip(VarInt(v as i32));
                roundtrip(VarInt(-(v as i32)));
            }
        }
        for shift in 0..63 {
            let v = 1_i64 << shift;
            roundtrip(VarLong(v));
            roundtrip(VarLong(-v));
        }
    }

    #[test]
    fn overlong_input_is_rejected() {
        // a sixth byte is never read: five continuation bytes are already too many
        let input = [0x80, 0x80, 0x80, 0x80, 0x80, 0x00];
        assert!(matches!(
            VarInt::decode(&mut input.as_slice()),
            Err(Error::VarIntTooLong)
        ));
        let input = [0x80u8; 11];
        assert!(matches!(
            VarLong::decode(&mut input.as_slice()),
            Err(Error::VarIntTooLong)
        ));
    }

    #[test]
    fn truncated_input_is_an_error() {
        assert!(matches!(
            VarInt::decode(&mut [0x80].as_slice()),
            Err(Error::UnexpectedEof)
        ));
        assert!(matches!(
            VarInt::decode(&mut [].as_slice()),
            Err(Error::UnexpectedEof)
        ));
    }
}
