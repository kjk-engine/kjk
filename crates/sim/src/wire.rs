//! The byte form shared by snapshots, saves and recordings: little-endian,
//! fixed widths, no padding, floats by their bits. One state has exactly one
//! encoding, which is what makes a hash of it mean "the same state".

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// The first bytes are not this format's signature.
    Signature {
        format: &'static str,
        found: [u8; 4],
    },
    /// A version this build does not read.
    Version { format: &'static str, found: u32 },
    /// The data ends before a field it announces.
    Truncated {
        format: &'static str,
        needed: usize,
        len: usize,
    },
    /// A field holds a value no encoder writes.
    Invalid {
        format: &'static str,
        what: &'static str,
        at: usize,
    },
    /// Bytes are left after the last field.
    Trailing { format: &'static str, extra: usize },
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::Signature { format, found } => {
                write!(f, "{format}: not this format (starts with {found:?})")
            }
            DecodeError::Version { format, found } => {
                write!(f, "{format}: version {found} is not read by this build")
            }
            DecodeError::Truncated {
                format,
                needed,
                len,
            } => write!(f, "{format}: truncated, needs {needed} bytes, has {len}"),
            DecodeError::Invalid { format, what, at } => {
                write!(f, "{format}: invalid {what} at byte {at}")
            }
            DecodeError::Trailing { format, extra } => {
                write!(f, "{format}: {extra} bytes after the end")
            }
        }
    }
}

impl std::error::Error for DecodeError {}

#[derive(Default)]
pub(crate) struct Writer(pub Vec<u8>);

impl Writer {
    pub fn u8(&mut self, value: u8) {
        self.0.push(value);
    }

    pub fn i8(&mut self, value: i8) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    pub fn u16(&mut self, value: u16) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    pub fn u32(&mut self, value: u32) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    pub fn u64(&mut self, value: u64) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    pub fn f32(&mut self, value: f32) {
        self.u32(value.to_bits());
    }

    pub fn bytes(&mut self, bytes: &[u8]) {
        self.0.extend_from_slice(bytes);
    }

    /// A length (u32) and the bytes.
    pub fn section(&mut self, bytes: &[u8]) {
        self.u32(u32::try_from(bytes.len()).expect("sections stay under 4 GB"));
        self.bytes(bytes);
    }

    /// A string as a section of its UTF-8 bytes.
    pub fn string(&mut self, text: &str) {
        self.section(text.as_bytes());
    }
}

/// An optional value at a fixed width: a flag, then the value, or `blank`
/// in its place, so a record keeps one length whatever it holds (the
/// network's deltas XOR records of equal length).
pub(crate) fn write_fixed<T>(
    out: &mut Writer,
    value: Option<&T>,
    blank: &T,
    write: fn(&T, &mut Writer),
) {
    match value {
        Some(value) => {
            out.u8(1);
            write(value, out);
        }
        None => {
            out.u8(0);
            write(blank, out);
        }
    }
}

/// Reads what `write_fixed` wrote: an absent value must have left exactly
/// `blank`, so one state keeps one encoding.
pub(crate) fn read_fixed<T: PartialEq>(
    input: &mut Reader,
    blank: &T,
    read: fn(&mut Reader) -> Result<T, DecodeError>,
    what: &'static str,
) -> Result<Option<T>, DecodeError> {
    let at = input.at();
    let flag = input.u8()?;
    let value = read(input)?;
    match flag {
        0 if value == *blank => Ok(None),
        1 => Ok(Some(value)),
        _ => Err(input.invalid(what, at)),
    }
}

/// Reads fields in order, failing as `Truncated` instead of panicking.
pub(crate) struct Reader<'a> {
    format: &'static str,
    bytes: &'a [u8],
    at: usize,
    /// The format's version these bytes are in: the header's, else this
    /// build's (`u32::MAX`, every `since` true).
    version: u32,
}

impl<'a> Reader<'a> {
    pub fn new(format: &'static str, bytes: &'a [u8]) -> Self {
        Reader {
            format,
            bytes,
            at: 0,
            version: u32::MAX,
        }
    }

    /// These bytes as written by `version` of their format: a part of a
    /// format read by a reader of its own (the save's scripts).
    pub fn with_version(self, version: u32) -> Self {
        Reader { version, ..self }
    }

    /// Whether these bytes are of `version` or later: whether a change
    /// that came in `version` (`crate::since`) is in them.
    pub fn since(&self, version: u32) -> bool {
        self.version >= version
    }

    /// The 4-byte signature, then a u32 version that must be `version`.
    pub fn header(&mut self, magic: &[u8; 4], version: u32) -> Result<(), DecodeError> {
        self.header_since(magic, version, version).map(|_| ())
    }

    /// The 4-byte signature, then a u32 version from `oldest` to `current`:
    /// the one found, which `since` then answers by.
    pub fn header_since(
        &mut self,
        magic: &[u8; 4],
        oldest: u32,
        current: u32,
    ) -> Result<u32, DecodeError> {
        let found: [u8; 4] = self.take(4)?.try_into().expect("take of 4");
        if &found != magic {
            return Err(DecodeError::Signature {
                format: self.format,
                found,
            });
        }
        let found = self.u32()?;
        if !(oldest..=current).contains(&found) {
            return Err(DecodeError::Version {
                format: self.format,
                found,
            });
        }
        self.version = found;
        Ok(found)
    }

    pub fn take(&mut self, len: usize) -> Result<&'a [u8], DecodeError> {
        let end = self.at.checked_add(len).ok_or(DecodeError::Truncated {
            format: self.format,
            needed: usize::MAX,
            len: self.bytes.len(),
        })?;
        let slice = self.bytes.get(self.at..end).ok_or(DecodeError::Truncated {
            format: self.format,
            needed: end,
            len: self.bytes.len(),
        })?;
        self.at = end;
        Ok(slice)
    }

    pub fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.take(1)?[0])
    }

    pub fn i8(&mut self) -> Result<i8, DecodeError> {
        Ok(i8::from_le_bytes([self.u8()?]))
    }

    pub fn u16(&mut self) -> Result<u16, DecodeError> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    pub fn u32(&mut self) -> Result<u32, DecodeError> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    pub fn u64(&mut self) -> Result<u64, DecodeError> {
        Ok(u64::from_le_bytes(self.array()?))
    }

    pub fn f32(&mut self) -> Result<f32, DecodeError> {
        Ok(f32::from_bits(self.u32()?))
    }

    pub fn array<const N: usize>(&mut self) -> Result<[u8; N], DecodeError> {
        Ok(self.take(N)?.try_into().expect("take of N"))
    }

    /// A length (u32) and that many bytes.
    pub fn section(&mut self) -> Result<&'a [u8], DecodeError> {
        let len = self.u32()? as usize;
        self.take(len)
    }

    /// A string written by `Writer::string`.
    pub fn string(&mut self) -> Result<String, DecodeError> {
        let at = self.at;
        let bytes = self.section()?;
        String::from_utf8(bytes.to_vec()).map_err(|_| self.invalid("text (not UTF-8)", at))
    }

    /// Where the next field starts, for errors.
    pub fn at(&self) -> usize {
        self.at
    }

    pub fn invalid(&self, what: &'static str, at: usize) -> DecodeError {
        DecodeError::Invalid {
            format: self.format,
            what,
            at,
        }
    }

    /// Fails unless every byte was read.
    pub fn finish(self) -> Result<(), DecodeError> {
        match self.bytes.len() - self.at {
            0 => Ok(()),
            extra => Err(DecodeError::Trailing {
                format: self.format,
                extra,
            }),
        }
    }
}
