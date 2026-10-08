// SPDX-License-Identifier: Apache-2.0 OR MIT
use std::io::Write;

use crate::{Decode, Encode, Error, Result, take};

/// Deepest nesting of lists and compounds accepted.
///
/// The vanilla game allows 512, but each level of the recursive decoder costs several KB
/// of stack (measured: 512 levels need over 2 MB in debug and 1 MB or more in release),
/// which overflows a 2 MB worker thread on hostile input. Real data is a handful of
/// levels deep, so the limit is lower.
// ponytail: recursive decoder, raise this only together with an iterative one
const MAX_DEPTH: usize = 64;

/// One NBT value.
///
/// On the wire this is the *network* form used since 1.20.2: the tag id followed by the
/// payload, with no root name. The root may be any tag, not only a compound.
#[derive(Debug, Clone, PartialEq)]
pub enum Nbt {
    /// A signed byte; also how booleans are stored.
    Byte(i8),
    /// A signed 16-bit integer.
    Short(i16),
    /// A signed 32-bit integer.
    Int(i32),
    /// A signed 64-bit integer.
    Long(i64),
    /// A 32-bit float.
    Float(f32),
    /// A 64-bit float.
    Double(f64),
    /// An array of bytes.
    ByteArray(Vec<i8>),
    /// A string, stored as modified UTF-8 (at most 65535 bytes).
    String(String),
    /// A list whose elements all have the same tag.
    List(Vec<Nbt>),
    /// Named values.
    Compound(Compound),
    /// An array of 32-bit integers.
    IntArray(Vec<i32>),
    /// An array of 64-bit integers.
    LongArray(Vec<i64>),
}

/// The named values of a compound tag, in insertion order.
///
/// Decoding keeps every entry it reads, so a hostile input with repeated names costs
/// nothing extra; [`get`](Self::get) returns the last one, like the game does.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Compound(pub Vec<(String, Nbt)>);

impl Compound {
    /// An empty compound.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets `key`, replacing an existing entry of that name, and returns the old value.
    pub fn insert(&mut self, key: impl Into<String>, value: Nbt) -> Option<Nbt> {
        let key = key.into();
        match self.0.iter_mut().find(|(k, _)| *k == key) {
            Some((_, slot)) => Some(std::mem::replace(slot, value)),
            None => {
                self.0.push((key, value));
                None
            }
        }
    }

    /// The value stored under `key`.
    pub fn get(&self, key: &str) -> Option<&Nbt> {
        self.0.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v)
    }
}

impl From<Compound> for Nbt {
    fn from(c: Compound) -> Self {
        Self::Compound(c)
    }
}

impl From<&str> for Nbt {
    fn from(s: &str) -> Self {
        Self::String(s.to_owned())
    }
}

impl From<String> for Nbt {
    fn from(s: String) -> Self {
        Self::String(s)
    }
}

impl Nbt {
    fn tag(&self) -> u8 {
        match self {
            Self::Byte(_) => 1,
            Self::Short(_) => 2,
            Self::Int(_) => 3,
            Self::Long(_) => 4,
            Self::Float(_) => 5,
            Self::Double(_) => 6,
            Self::ByteArray(_) => 7,
            Self::String(_) => 8,
            Self::List(_) => 9,
            Self::Compound(_) => 10,
            Self::IntArray(_) => 11,
            Self::LongArray(_) => 12,
        }
    }

    fn write_payload(&self, w: &mut impl Write, depth: usize) -> Result<()> {
        match self {
            Self::Byte(v) => v.encode(w),
            Self::Short(v) => v.encode(w),
            Self::Int(v) => v.encode(w),
            Self::Long(v) => v.encode(w),
            Self::Float(v) => v.encode(w),
            Self::Double(v) => v.encode(w),
            Self::ByteArray(v) => write_array(w, v),
            Self::String(s) => write_mutf8(w, s),
            Self::List(items) => {
                check_depth(depth + 1)?;
                let tag = items.first().map_or(0, Nbt::tag);
                // elements of different types are all written as compounds, the ones that are
                // not wrapped in a compound with the empty name, as the game does
                let mixed = items.iter().any(|i| i.tag() != tag);
                (if mixed { COMPOUND } else { tag }).encode(w)?;
                write_len(w, items.len())?;
                items.iter().try_for_each(|i| {
                    if mixed && i.tag() != COMPOUND {
                        check_depth(depth + 2)?;
                        i.tag().encode(w)?;
                        write_mutf8(w, "")?;
                        i.write_payload(w, depth + 2)?;
                        0u8.encode(w) // TAG_End
                    } else {
                        i.write_payload(w, depth + 1)
                    }
                })
            }
            Self::Compound(c) => {
                check_depth(depth + 1)?;
                for (name, value) in &c.0 {
                    value.tag().encode(w)?;
                    write_mutf8(w, name)?;
                    value.write_payload(w, depth + 1)?;
                }
                0u8.encode(w) // TAG_End
            }
            Self::IntArray(v) => write_array(w, v),
            Self::LongArray(v) => write_array(w, v),
        }
    }

    fn read_payload(tag: u8, r: &mut &[u8], depth: usize) -> Result<Self> {
        Ok(match tag {
            1 => Self::Byte(i8::decode(r)?),
            2 => Self::Short(i16::decode(r)?),
            3 => Self::Int(i32::decode(r)?),
            4 => Self::Long(i64::decode(r)?),
            5 => Self::Float(f32::decode(r)?),
            6 => Self::Double(f64::decode(r)?),
            7 => Self::ByteArray(read_array(r)?),
            8 => Self::String(read_mutf8(r)?),
            9 => {
                check_depth(depth + 1)?;
                let elem = u8::decode(r)?;
                let len = read_len(r)?;
                if elem == 0 {
                    if len > 0 {
                        return Err(Error::InvalidValue("nbt list of TAG_End with elements"));
                    }
                    return Ok(Self::List(Vec::new()));
                }
                // every element takes at least one byte
                check_remaining(len, r)?;
                Self::List(
                    (0..len)
                        .map(|_| {
                            Self::read_payload(elem, r, depth + 1)
                                .map(|item| if elem == COMPOUND { unwrap(item) } else { item })
                        })
                        .collect::<Result<_>>()?,
                )
            }
            10 => {
                check_depth(depth + 1)?;
                let mut entries = Vec::new();
                loop {
                    let tag = u8::decode(r)?;
                    if tag == 0 {
                        break;
                    }
                    let name = read_mutf8(r)?;
                    entries.push((name, Self::read_payload(tag, r, depth + 1)?));
                }
                Self::Compound(Compound(entries))
            }
            11 => Self::IntArray(read_array(r)?),
            12 => Self::LongArray(read_array(r)?),
            0 => return Err(Error::InvalidValue("unexpected nbt TAG_End")),
            _ => return Err(Error::InvalidValue("unknown nbt tag")),
        })
    }
}

impl Encode for Nbt {
    fn encode(&self, w: &mut impl Write) -> Result<()> {
        self.tag().encode(w)?;
        self.write_payload(w, 0)
    }
}

impl Decode for Nbt {
    fn decode(r: &mut &[u8]) -> Result<Self> {
        let tag = u8::decode(r)?;
        Self::read_payload(tag, r, 0)
    }
}

/// The tag of a compound.
const COMPOUND: u8 = 10;

/// Takes a value back out of the compound with the empty name that a list of mixed types wraps
/// it in; a compound of anything else is itself.
fn unwrap(item: Nbt) -> Nbt {
    match item {
        Nbt::Compound(Compound(mut entries)) if entries.len() == 1 && entries[0].0.is_empty() => {
            entries.remove(0).1
        }
        item => item,
    }
}

fn check_depth(depth: usize) -> Result<()> {
    if depth > MAX_DEPTH {
        Err(Error::InvalidValue("nbt nested too deeply"))
    } else {
        Ok(())
    }
}

fn write_len(w: &mut impl Write, len: usize) -> Result<()> {
    let len = i32::try_from(len).map_err(|_| Error::LengthTooLarge {
        len,
        max: i32::MAX as usize,
    })?;
    len.encode(w)
}

fn read_len(r: &mut &[u8]) -> Result<usize> {
    usize::try_from(i32::decode(r)?).map_err(|_| Error::InvalidValue("negative nbt length"))
}

/// Rejects a count larger than the bytes left, before anything is allocated.
fn check_remaining(len: usize, r: &[u8]) -> Result<()> {
    if len > r.len() {
        Err(Error::LengthTooLarge { len, max: r.len() })
    } else {
        Ok(())
    }
}

fn write_array<T: Encode>(w: &mut impl Write, items: &[T]) -> Result<()> {
    write_len(w, items.len())?;
    items.iter().try_for_each(|i| i.encode(w))
}

fn read_array<T: Decode>(r: &mut &[u8]) -> Result<Vec<T>> {
    let len = read_len(r)?;
    check_remaining(len, r)?; // every element takes at least one byte
    (0..len).map(|_| T::decode(r)).collect()
}

/// Writes a string as modified UTF-8 with a `u16` byte length: NUL is `C0 80`, and each
/// UTF-16 surrogate of a supplementary character is a 3-byte sequence of its own.
fn write_mutf8(w: &mut impl Write, s: &str) -> Result<()> {
    let mut bytes = Vec::with_capacity(s.len());
    for unit in s.encode_utf16() {
        match unit {
            0 => bytes.extend([0xc0, 0x80]),
            1..=0x7f => bytes.push(unit as u8),
            0x80..=0x7ff => bytes.extend([0xc0 | (unit >> 6) as u8, 0x80 | (unit & 0x3f) as u8]),
            _ => bytes.extend([
                0xe0 | (unit >> 12) as u8,
                0x80 | ((unit >> 6) & 0x3f) as u8,
                0x80 | (unit & 0x3f) as u8,
            ]),
        }
    }
    let len = u16::try_from(bytes.len()).map_err(|_| Error::LengthTooLarge {
        len: bytes.len(),
        max: u16::MAX as usize,
    })?;
    len.encode(w)?;
    Ok(w.write_all(&bytes)?)
}

fn read_mutf8(r: &mut &[u8]) -> Result<String> {
    let len = usize::from(u16::decode(r)?);
    let bytes = take(r, len)?;
    let cont = |i: usize| match bytes.get(i) {
        Some(b) if b & 0xc0 == 0x80 => Ok(u16::from(b & 0x3f)),
        _ => Err(Error::InvalidUtf8),
    };
    let mut units = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        let (unit, used) = match b {
            0x00..=0x7f => (u16::from(b), 1),
            0xc0..=0xdf => ((u16::from(b & 0x1f) << 6) | cont(i + 1)?, 2),
            0xe0..=0xef => (
                (u16::from(b & 0x0f) << 12) | (cont(i + 1)? << 6) | cont(i + 2)?,
                3,
            ),
            _ => return Err(Error::InvalidUtf8),
        };
        units.push(unit);
        i += used;
    }
    String::from_utf16(&units).map_err(|_| Error::InvalidUtf8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::tests::{encoded, roundtrip};

    fn compound(entries: impl IntoIterator<Item = (&'static str, Nbt)>) -> Nbt {
        let mut c = Compound::new();
        for (k, v) in entries {
            c.insert(k, v);
        }
        Nbt::Compound(c)
    }

    #[test]
    fn a_list_of_mixed_types_is_written_as_compounds_with_the_odd_ones_wrapped() {
        // the bytes the 26.3 game writes for `["b", {k: 1b}]`
        let list = Nbt::List(vec![Nbt::from("b"), compound([("k", Nbt::Byte(1))])]);
        let expected = [
            9, 10, 0, 0, 0, 2, // a list of two compounds
            8, 0, 0, 0, 1, b'b', 0, // "b" in a compound named ""
            1, 0, 1, b'k', 1, 0, // the compound itself
        ];
        assert_eq!(encoded(&list), expected);
        // and read back as what it was
        roundtrip(list);
    }

    #[test]
    fn lists_of_one_type_are_not_wrapped() {
        let strings = Nbt::List(vec![Nbt::from("a"), Nbt::from("b")]);
        assert_eq!(encoded(&strings)[..6], [9, 8, 0, 0, 0, 2]);
        let compounds = Nbt::List(vec![compound([("k", Nbt::Byte(1))]); 2]);
        assert_eq!(encoded(&compounds)[..6], [9, 10, 0, 0, 0, 2]);
        roundtrip(strings);
        roundtrip(compounds);
    }

    #[test]
    fn a_compound_of_the_empty_name_alone_in_a_list_of_compounds_is_unwrapped() {
        // what the game does, so a real compound like this cannot be told from a wrapped value
        let wrapped = Nbt::List(vec![compound([("", Nbt::Int(5))]); 2]);
        let mut bytes = Vec::new();
        wrapped.encode(&mut bytes).unwrap();
        let Nbt::List(items) = Nbt::decode(&mut bytes.as_slice()).unwrap() else {
            panic!("a list")
        };
        assert_eq!(items, [Nbt::Int(5), Nbt::Int(5)]);
        // with another entry it is a compound like any other
        let other = Nbt::List(vec![compound([("", Nbt::Int(5)), ("k", Nbt::Int(6))])]);
        roundtrip(other);
    }

    #[test]
    fn wrapping_counts_as_a_level_of_depth() {
        // a list of mixed types at the deepest level that is allowed has no room for the wrap
        let nested =
            |depth: usize, inner: Nbt| (0..depth).fold(inner, |inner, _| Nbt::List(vec![inner]));
        let mixed = Nbt::List(vec![Nbt::Byte(0), Nbt::from("x")]);
        assert!(
            nested(MAX_DEPTH - 2, mixed.clone())
                .encode(&mut Vec::new())
                .is_ok()
        );
        assert!(
            nested(MAX_DEPTH - 1, mixed)
                .encode(&mut Vec::new())
                .is_err()
        );
    }

    #[test]
    fn network_form_has_no_root_name() {
        // the "hello world" example from the NBT specification, minus its root name
        let nbt = compound([("name", Nbt::from("Bananrama"))]);
        let mut expected = vec![0x0a, 0x08, 0x00, 0x04];
        expected.extend(b"name");
        expected.extend([0x00, 0x09]);
        expected.extend(b"Bananrama");
        expected.push(0x00);
        assert_eq!(encoded(&nbt), expected);
        assert_eq!(Nbt::decode(&mut expected.as_slice()).unwrap(), nbt);
    }

    #[test]
    fn the_root_may_be_any_tag() {
        assert_eq!(encoded(&Nbt::from("hi")), [8, 0, 2, b'h', b'i']);
        roundtrip(Nbt::from("hi"));
        roundtrip(Nbt::Int(-5));
    }

    #[test]
    fn every_tag_roundtrips() {
        roundtrip(compound([
            ("byte", Nbt::Byte(-1)),
            ("short", Nbt::Short(300)),
            ("int", Nbt::Int(i32::MIN)),
            ("long", Nbt::Long(i64::MAX)),
            ("float", Nbt::Float(1.5)),
            ("double", Nbt::Double(-2.25)),
            ("bytes", Nbt::ByteArray(vec![1, -2, 3])),
            ("string", Nbt::from("日本語 🦀")),
            ("list", Nbt::List(vec![Nbt::Int(1), Nbt::Int(2)])),
            ("nested", compound([("x", Nbt::Byte(1))])),
            ("ints", Nbt::IntArray(vec![1, 2, 3])),
            ("longs", Nbt::LongArray(vec![i64::MIN, 0])),
            ("empty", Nbt::List(vec![])),
        ]));
    }

    #[test]
    fn lists_are_typed() {
        // an empty list is written with element tag 0
        assert_eq!(encoded(&Nbt::List(vec![])), [9, 0, 0, 0, 0, 0]);
        assert_eq!(
            encoded(&Nbt::List(vec![Nbt::Byte(7)])),
            [9, 1, 0, 0, 0, 1, 7]
        );
        // (a list of mixed types is written as compounds: see the test of that)
        // TAG_End elements are only valid in an empty list
        let input = [9, 0, 0, 0, 0, 1];
        assert!(matches!(
            Nbt::decode(&mut input.as_slice()),
            Err(Error::InvalidValue(_))
        ));
    }

    #[test]
    fn compound_keeps_order_and_replaces_on_insert() {
        let mut c = Compound::new();
        assert_eq!(c.insert("b", Nbt::Byte(1)), None);
        c.insert("a", Nbt::Byte(2));
        assert_eq!(c.insert("b", Nbt::Byte(3)), Some(Nbt::Byte(1)));
        assert_eq!(
            c.0.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
            ["b", "a"]
        );
        assert_eq!(c.get("b"), Some(&Nbt::Byte(3)));
        assert_eq!(c.get("missing"), None);
    }

    #[test]
    fn decoded_duplicates_resolve_to_the_last() {
        let input = [10, 1, 0, 1, b'k', 1, 1, 0, 1, b'k', 2, 0];
        let Nbt::Compound(c) = Nbt::decode(&mut input.as_slice()).unwrap() else {
            panic!()
        };
        assert_eq!(c.0.len(), 2);
        assert_eq!(c.get("k"), Some(&Nbt::Byte(2)));
    }

    #[test]
    fn strings_are_modified_utf8() {
        // NUL is C0 80, never a raw 0
        assert_eq!(encoded(&Nbt::from("\0")), [8, 0, 2, 0xc0, 0x80]);
        // U+1F980 is a surrogate pair, each half as its own 3-byte sequence
        assert_eq!(
            encoded(&Nbt::from("🦀")),
            [8, 0, 6, 0xed, 0xa0, 0xbe, 0xed, 0xb6, 0x80]
        );
        assert_eq!(encoded(&Nbt::from("é")), [8, 0, 2, 0xc3, 0xa9]);
        roundtrip(Nbt::from("a\0b🦀é日"));
    }

    #[test]
    fn invalid_modified_utf8_is_rejected() {
        let decode = |bytes: &[u8]| {
            let mut input = vec![8, 0, bytes.len() as u8];
            input.extend(bytes);
            Nbt::decode(&mut input.as_slice())
        };
        // standard 4-byte UTF-8 is not modified UTF-8
        assert!(matches!(
            decode(&[0xf0, 0x9f, 0xa6, 0x80]),
            Err(Error::InvalidUtf8)
        ));
        // a lone surrogate
        assert!(matches!(
            decode(&[0xed, 0xa0, 0xbe]),
            Err(Error::InvalidUtf8)
        ));
        // truncated sequence, bad continuation byte
        assert!(matches!(decode(&[0xe3, 0x81]), Err(Error::InvalidUtf8)));
        assert!(matches!(decode(&[0xc3, 0x29]), Err(Error::InvalidUtf8)));
    }

    #[test]
    fn string_length_is_limited_to_u16() {
        let long = "a".repeat(65536);
        assert!(matches!(
            Nbt::from(long).encode(&mut Vec::new()),
            Err(Error::LengthTooLarge { .. })
        ));
        roundtrip(Nbt::from("a".repeat(65535)));
    }

    #[test]
    fn hostile_lengths_are_rejected_before_allocating() {
        // arrays and lists declaring i32::MAX elements with nothing behind them
        for head in [
            [7u8, 0x7f, 0xff, 0xff, 0xff].as_slice(),
            &[11, 0x7f, 0xff, 0xff, 0xff],
            &[12, 0x7f, 0xff, 0xff, 0xff],
        ] {
            assert!(
                matches!(Nbt::decode(&mut &*head), Err(Error::LengthTooLarge { .. })),
                "{head:?}"
            );
        }
        let list = [9u8, 1, 0x7f, 0xff, 0xff, 0xff];
        assert!(matches!(
            Nbt::decode(&mut list.as_slice()),
            Err(Error::LengthTooLarge { .. })
        ));
        // negative length
        let neg = [7u8, 0xff, 0xff, 0xff, 0xff];
        assert!(matches!(
            Nbt::decode(&mut neg.as_slice()),
            Err(Error::InvalidValue(_))
        ));
    }

    #[test]
    fn unknown_and_misplaced_tags_are_errors() {
        assert!(matches!(
            Nbt::decode(&mut [13u8].as_slice()),
            Err(Error::InvalidValue(_))
        ));
        assert!(matches!(
            Nbt::decode(&mut [0u8].as_slice()),
            Err(Error::InvalidValue(_))
        ));
        assert!(matches!(
            Nbt::decode(&mut [3u8, 0, 0].as_slice()),
            Err(Error::UnexpectedEof)
        ));
        assert!(matches!(
            Nbt::decode(&mut [10u8].as_slice()),
            Err(Error::UnexpectedEof)
        ));
    }

    #[test]
    fn nesting_is_limited_in_both_directions() {
        let nested =
            |depth: usize| (0..depth).fold(Nbt::Byte(0), |inner, _| Nbt::List(vec![inner]));
        // a chain of single-element lists, MAX_DEPTH deep, is the deepest allowed
        roundtrip(nested(MAX_DEPTH));
        assert!(matches!(
            nested(MAX_DEPTH + 1).encode(&mut Vec::new()),
            Err(Error::InvalidValue(_))
        ));
        // the same, written by hand so decoding is what fails: far too many lists of one list
        let mut input = vec![9u8];
        for _ in 0..600 {
            input.extend([9, 0, 0, 0, 1]);
        }
        assert!(matches!(
            Nbt::decode(&mut input.as_slice()),
            Err(Error::InvalidValue(_))
        ));
    }
}
