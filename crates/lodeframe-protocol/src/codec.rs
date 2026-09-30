use std::io::Write;

use crate::{Error, Result, VarInt};

/// A value that can be written in the Minecraft wire format.
pub trait Encode {
    /// Writes `self` to `w`.
    fn encode(&self, w: &mut impl Write) -> Result<()>;
}

/// A value that can be read from the Minecraft wire format.
pub trait Decode: Sized {
    /// Reads a value from the front of `r`, advancing it past the bytes consumed.
    fn decode(r: &mut &[u8]) -> Result<Self>;
}

/// Splits `n` bytes off the front of `r`.
pub fn take<'a>(r: &mut &'a [u8], n: usize) -> Result<&'a [u8]> {
    if r.len() < n {
        return Err(Error::UnexpectedEof);
    }
    let (head, tail) = r.split_at(n);
    *r = tail;
    Ok(head)
}

impl<T: Encode + ?Sized> Encode for &T {
    fn encode(&self, w: &mut impl Write) -> Result<()> {
        (**self).encode(w)
    }
}

macro_rules! numbers {
    ($($t:ty),*) => {$(
        impl Encode for $t {
            fn encode(&self, w: &mut impl Write) -> Result<()> {
                Ok(w.write_all(&self.to_be_bytes())?)
            }
        }
        impl Decode for $t {
            fn decode(r: &mut &[u8]) -> Result<Self> {
                let bytes = take(r, size_of::<$t>())?;
                // take returned exactly size_of::<$t>() bytes
                Ok(<$t>::from_be_bytes(bytes.try_into().unwrap()))
            }
        }
    )*};
}
numbers!(u8, i8, u16, i16, u32, i32, u64, i64, u128, f32, f64);

impl Encode for bool {
    fn encode(&self, w: &mut impl Write) -> Result<()> {
        u8::from(*self).encode(w)
    }
}

impl Decode for bool {
    fn decode(r: &mut &[u8]) -> Result<Self> {
        match u8::decode(r)? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(Error::InvalidValue("bool")),
        }
    }
}

/// Prefixed optional: a `bool` followed by the value when present.
impl<T: Encode> Encode for Option<T> {
    fn encode(&self, w: &mut impl Write) -> Result<()> {
        match self {
            Some(v) => {
                true.encode(w)?;
                v.encode(w)
            }
            None => false.encode(w),
        }
    }
}

impl<T: Decode> Decode for Option<T> {
    fn decode(r: &mut &[u8]) -> Result<Self> {
        if bool::decode(r)? {
            T::decode(r).map(Some)
        } else {
            Ok(None)
        }
    }
}

/// Length-prefixed array: a `VarInt` count followed by the elements.
impl<T: Encode> Encode for [T] {
    fn encode(&self, w: &mut impl Write) -> Result<()> {
        let len = i32::try_from(self.len()).map_err(|_| Error::LengthTooLarge {
            len: self.len(),
            max: i32::MAX as usize,
        })?;
        VarInt(len).encode(w)?;
        self.iter().try_for_each(|v| v.encode(w))
    }
}

impl<T: Encode> Encode for Vec<T> {
    fn encode(&self, w: &mut impl Write) -> Result<()> {
        self.as_slice().encode(w)
    }
}

/// Every element occupies at least one byte, so a count larger than the remaining
/// input is rejected before anything is allocated.
impl<T: Decode> Decode for Vec<T> {
    fn decode(r: &mut &[u8]) -> Result<Self> {
        let len = VarInt::decode(r)?.0;
        let len = usize::try_from(len).map_err(|_| Error::InvalidValue("negative length"))?;
        if len > r.len() {
            return Err(Error::LengthTooLarge { len, max: r.len() });
        }
        (0..len).map(|_| T::decode(r)).collect()
    }
}

/// Fixed-size array: the elements with no length prefix.
impl<T: Encode, const N: usize> Encode for [T; N] {
    fn encode(&self, w: &mut impl Write) -> Result<()> {
        self.iter().try_for_each(|v| v.encode(w))
    }
}

impl<T: Decode, const N: usize> Decode for [T; N] {
    fn decode(r: &mut &[u8]) -> Result<Self> {
        let v = (0..N).map(|_| T::decode(r)).collect::<Result<Vec<_>>>()?;
        // exactly N elements were collected
        Ok(v.try_into().ok().unwrap())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::fmt::Debug;

    use super::*;

    pub(crate) fn encoded(v: &impl Encode) -> Vec<u8> {
        let mut buf = Vec::new();
        v.encode(&mut buf).unwrap();
        buf
    }

    /// Encodes `v`, decodes it back, and checks the input was consumed exactly.
    pub(crate) fn roundtrip<T: Encode + Decode + PartialEq + Debug>(v: T) {
        let buf = encoded(&v);
        let mut r = buf.as_slice();
        assert_eq!(T::decode(&mut r).unwrap(), v);
        assert!(r.is_empty(), "trailing bytes for {v:?}");
    }

    #[test]
    fn numbers_are_big_endian() {
        assert_eq!(encoded(&0x0102_u16), [1, 2]);
        assert_eq!(encoded(&-2_i32), [0xff, 0xff, 0xff, 0xfe]);
        assert_eq!(encoded(&1.0_f32), [0x3f, 0x80, 0, 0]);
        roundtrip(u64::MAX);
        roundtrip(i64::MIN);
        roundtrip(f64::MIN_POSITIVE);
        roundtrip(u128::MAX);
    }

    #[test]
    fn bool_is_strict() {
        roundtrip(true);
        roundtrip(false);
        assert!(matches!(
            bool::decode(&mut [2u8].as_slice()),
            Err(Error::InvalidValue(_))
        ));
    }

    #[test]
    fn option_is_bool_prefixed() {
        assert_eq!(encoded(&None::<u8>), [0]);
        assert_eq!(encoded(&Some(7u8)), [1, 7]);
        roundtrip(Some(1234_i32));
        roundtrip(None::<i32>);
    }

    #[test]
    fn vec_and_array() {
        assert_eq!(encoded(&vec![1u8, 2, 3]), [3, 1, 2, 3]);
        roundtrip(vec![1_i32, -2, 3]);
        roundtrip(Vec::<u8>::new());
        assert!(encoded(&[1u16, 2]) == [0, 1, 0, 2]);
        roundtrip([1u8, 2, 3, 4]);
    }

    #[test]
    fn truncated_input_is_an_error() {
        assert!(matches!(
            u32::decode(&mut [0u8; 3].as_slice()),
            Err(Error::UnexpectedEof)
        ));
        assert!(matches!(
            <[u8; 4]>::decode(&mut [0u8; 3].as_slice()),
            Err(Error::UnexpectedEof)
        ));
    }

    #[test]
    fn vec_length_beyond_input_is_rejected_without_allocating() {
        // declares i32::MAX elements with nothing following
        let input = [0xff, 0xff, 0xff, 0xff, 0x07];
        assert!(matches!(
            Vec::<u8>::decode(&mut input.as_slice()),
            Err(Error::LengthTooLarge { .. })
        ));
        // negative length
        let input = [0xff, 0xff, 0xff, 0xff, 0x0f];
        assert!(matches!(
            Vec::<u8>::decode(&mut input.as_slice()),
            Err(Error::InvalidValue(_))
        ));
    }
}
