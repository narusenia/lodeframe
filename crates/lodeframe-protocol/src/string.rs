use std::io::Write;

use crate::{Decode, Encode, Error, Result, VarInt, take};

/// Longest string the protocol allows, in UTF-16 code units.
const MAX_UTF16: usize = 32767;
/// A UTF-16 code unit is at most 3 bytes of UTF-8.
const MAX_BYTES: usize = MAX_UTF16 * 3;

impl Encode for str {
    fn encode(&self, w: &mut impl Write) -> Result<()> {
        let units = self.encode_utf16().count();
        if units > MAX_UTF16 {
            return Err(Error::LengthTooLarge {
                len: units,
                max: MAX_UTF16,
            });
        }
        // bounded by MAX_BYTES, fits in i32
        VarInt(self.len() as i32).encode(w)?;
        Ok(w.write_all(self.as_bytes())?)
    }
}

impl Encode for String {
    fn encode(&self, w: &mut impl Write) -> Result<()> {
        self.as_str().encode(w)
    }
}

impl Decode for String {
    fn decode(r: &mut &[u8]) -> Result<Self> {
        let len = VarInt::decode(r)?.0;
        let len = usize::try_from(len).map_err(|_| Error::InvalidValue("negative length"))?;
        if len > MAX_BYTES {
            return Err(Error::LengthTooLarge {
                len,
                max: MAX_BYTES,
            });
        }
        let s = std::str::from_utf8(take(r, len)?).map_err(|_| Error::InvalidUtf8)?;
        let units = s.encode_utf16().count();
        if units > MAX_UTF16 {
            return Err(Error::LengthTooLarge {
                len: units,
                max: MAX_UTF16,
            });
        }
        Ok(s.to_owned())
    }
}

/// A namespaced resource name such as `minecraft:stone`.
///
/// A name without a namespace (`stone`) is kept as written; the client reads it
/// as `minecraft:stone`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Identifier(String);

impl Identifier {
    /// Validates `s` and wraps it.
    ///
    /// The namespace allows `a-z 0-9 _ . -`; the path additionally allows `/`.
    pub fn new(s: impl Into<String>) -> Result<Self> {
        let s = s.into();
        let (namespace, path) = s.split_once(':').unwrap_or(("minecraft", &s));
        let ns_ok = |c: char| matches!(c, 'a'..='z' | '0'..='9' | '_' | '.' | '-');
        let path_ok = |c: char| ns_ok(c) || c == '/';
        if namespace.is_empty()
            || path.is_empty()
            || !namespace.chars().all(ns_ok)
            || !path.chars().all(path_ok)
        {
            return Err(Error::InvalidIdentifier);
        }
        Ok(Self(s))
    }

    /// The identifier as written.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Encode for Identifier {
    fn encode(&self, w: &mut impl Write) -> Result<()> {
        self.0.encode(w)
    }
}

impl Decode for Identifier {
    fn decode(r: &mut &[u8]) -> Result<Self> {
        Self::new(String::decode(r)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::tests::{encoded, roundtrip};

    #[test]
    fn string_is_length_prefixed_utf8() {
        assert_eq!(encoded(&"hi".to_owned()), [2, b'h', b'i']);
        roundtrip(String::new());
        roundtrip("日本語 🦀".to_owned());
    }

    #[test]
    fn length_limit_counts_utf16_units() {
        // 🦀 is 2 UTF-16 units, so 16383 of them (32766 units) fit and 16384 do not
        roundtrip("🦀".repeat(16383));
        let too_long = "🦀".repeat(16384);
        assert!(matches!(
            too_long.encode(&mut Vec::new()),
            Err(Error::LengthTooLarge { .. })
        ));
        roundtrip("a".repeat(MAX_UTF16));
        assert!("a".repeat(MAX_UTF16 + 1).encode(&mut Vec::new()).is_err());
    }

    #[test]
    fn oversized_declared_length_is_rejected_before_reading() {
        let mut input = encoded(&VarInt(MAX_BYTES as i32 + 1));
        input.extend(std::iter::repeat_n(b'a', 10));
        assert!(matches!(
            String::decode(&mut input.as_slice()),
            Err(Error::LengthTooLarge { .. })
        ));
        // too many UTF-16 units while within the byte bound
        let mut input = encoded(&VarInt(MAX_UTF16 as i32 + 1));
        input.extend(std::iter::repeat_n(b'a', MAX_UTF16 + 1));
        assert!(matches!(
            String::decode(&mut input.as_slice()),
            Err(Error::LengthTooLarge { .. })
        ));
    }

    #[test]
    fn invalid_utf8_and_truncation_are_errors() {
        assert!(matches!(
            String::decode(&mut [1, 0xff].as_slice()),
            Err(Error::InvalidUtf8)
        ));
        assert!(matches!(
            String::decode(&mut [5, b'a'].as_slice()),
            Err(Error::UnexpectedEof)
        ));
        assert!(matches!(
            String::decode(&mut [0xff, 0xff, 0xff, 0xff, 0x0f].as_slice()),
            Err(Error::InvalidValue(_))
        ));
    }

    #[test]
    fn identifier_validates_characters() {
        for ok in ["minecraft:stone", "stone", "my_pack:a/b/c.png", "a-b.c:d"] {
            roundtrip(Identifier::new(ok).unwrap());
        }
        for bad in [
            "", ":", "a:", ":b", "Stone", "a b", "a:b:c", "a/b:c", "日本",
        ] {
            assert!(
                matches!(Identifier::new(bad), Err(Error::InvalidIdentifier)),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn identifier_is_validated_on_decode() {
        let input = encoded(&"Bad Name".to_owned());
        assert!(matches!(
            Identifier::decode(&mut input.as_slice()),
            Err(Error::InvalidIdentifier)
        ));
    }
}
