use std::{fmt, io};

/// Result of encoding or decoding.
pub type Result<T> = std::result::Result<T, Error>;

/// Why encoding or decoding failed.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// The input ended before the value was complete.
    UnexpectedEof,
    /// A `VarInt` / `VarLong` used more bytes than its type allows.
    VarIntTooLong,
    /// A length prefix exceeded what the input or the protocol allows.
    LengthTooLarge {
        /// The length that was declared or supplied.
        len: usize,
        /// The largest length that would have been accepted.
        max: usize,
    },
    /// A string was not valid UTF-8.
    InvalidUtf8,
    /// An identifier contained characters outside the allowed set.
    InvalidIdentifier,
    /// A value was outside its valid range or set.
    InvalidValue(&'static str),
    /// The underlying writer failed.
    Io(io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEof => f.write_str("unexpected end of input"),
            Self::VarIntTooLong => f.write_str("varint is too long"),
            Self::LengthTooLarge { len, max } => write!(f, "length {len} exceeds maximum {max}"),
            Self::InvalidUtf8 => f.write_str("string is not valid utf-8"),
            Self::InvalidIdentifier => f.write_str("invalid identifier"),
            Self::InvalidValue(what) => write!(f, "invalid value: {what}"),
            Self::Io(e) => write!(f, "io error: {e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}
