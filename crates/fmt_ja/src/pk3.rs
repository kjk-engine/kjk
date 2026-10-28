//! PK3 archives: zip files. Only what Jedi Academy's archives use is read:
//! a single-disk central directory, entries stored or deflated.
//!
//! Names are lowercased and use `/`. When one archive lists a name twice,
//! the later entry wins, as in the engine's own lookup.

use std::fmt;

const EOCD_SIGNATURE: u32 = 0x0605_4b50;
const CENTRAL_SIGNATURE: u32 = 0x0201_4b50;
const LOCAL_SIGNATURE: u32 = 0x0403_4b50;
const EOCD_LEN: usize = 22;
const CENTRAL_LEN: usize = 46;
/// The fixed part of a local header; the name and extra field follow it.
pub const LOCAL_HEADER_LEN: usize = 30;
/// The end record sits within the last 22 bytes plus a comment of at most
/// 65535 bytes.
pub const TAIL_LEN: usize = EOCD_LEN + 0xffff;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pk3Error {
    NoEndRecord,
    MultiDisk,
    Truncated {
        what: &'static str,
    },
    BadSignature {
        what: &'static str,
        at: u64,
    },
    UnsupportedMethod {
        name: String,
        method: u16,
    },
    Inflate {
        name: String,
    },
    SizeMismatch {
        name: String,
        expected: u32,
        got: usize,
    },
    CrcMismatch {
        name: String,
        expected: u32,
        got: u32,
    },
}

impl fmt::Display for Pk3Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Pk3Error::NoEndRecord => f.write_str("pk3: no end of central directory record"),
            Pk3Error::MultiDisk => f.write_str("pk3: multi-disk archives are not read"),
            Pk3Error::Truncated { what } => write!(f, "pk3: truncated {what}"),
            Pk3Error::BadSignature { what, at } => write!(f, "pk3: bad {what} signature at {at}"),
            Pk3Error::UnsupportedMethod { name, method } => {
                write!(f, "pk3: {name}: unsupported compression method {method}")
            }
            Pk3Error::Inflate { name } => write!(
                f,
                "pk3: {name}: deflate data is corrupt or longer than its size"
            ),
            Pk3Error::SizeMismatch {
                name,
                expected,
                got,
            } => write!(f, "pk3: {name}: expected {expected} bytes, got {got}"),
            Pk3Error::CrcMismatch {
                name,
                expected,
                got,
            } => write!(
                f,
                "pk3: {name}: crc {got:08x}, directory says {expected:08x}"
            ),
        }
    }
}

impl std::error::Error for Pk3Error {}

/// Where the central directory is, read from the end of the file.
#[derive(Debug, Clone, Copy)]
pub struct Directory {
    pub entries: usize,
    pub offset: u64,
    pub size: u64,
}

#[derive(Debug, Clone)]
pub struct Pk3Entry {
    /// Lowercased, `/`-separated. Folders end in `/`.
    pub name: String,
    pub method: u16,
    pub crc: u32,
    pub compressed: u32,
    pub size: u32,
    /// Offset of the entry's local header in the archive.
    pub local_header: u32,
}

impl Pk3Entry {
    pub fn is_folder(&self) -> bool {
        self.name.ends_with('/')
    }
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

/// Finds the end record in the file's last bytes. `tail` is the last
/// `min(TAIL_LEN, file_len)` bytes and `file_len` the whole length.
pub fn directory(tail: &[u8], file_len: u64) -> Result<Directory, Pk3Error> {
    let at = (0..=tail.len().saturating_sub(EOCD_LEN))
        .rev()
        .find(|&at| u32_at(tail, at) == Some(EOCD_SIGNATURE))
        .ok_or(Pk3Error::NoEndRecord)?;
    let field =
        |offset| u16_at(tail, at + offset).ok_or(Pk3Error::Truncated { what: "end record" });
    if field(4)? != 0 || field(6)? != 0 || field(8)? != field(10)? {
        return Err(Pk3Error::MultiDisk);
    }
    let entries = field(10)? as usize;
    let size = u32_at(tail, at + 12).ok_or(Pk3Error::Truncated { what: "end record" })?;
    let offset = u32_at(tail, at + 16).ok_or(Pk3Error::Truncated { what: "end record" })?;
    if u64::from(offset) + u64::from(size) > file_len {
        return Err(Pk3Error::Truncated {
            what: "central directory",
        });
    }
    Ok(Directory {
        entries,
        offset: u64::from(offset),
        size: u64::from(size),
    })
}

/// Parses the central directory's bytes (`Directory::size` of them, read
/// from `Directory::offset`).
pub fn entries(central: &[u8], directory: Directory) -> Result<Vec<Pk3Entry>, Pk3Error> {
    let mut entries = Vec::with_capacity(directory.entries);
    let mut at = 0;
    for _ in 0..directory.entries {
        let truncated = Pk3Error::Truncated {
            what: "central directory entry",
        };
        if u32_at(central, at) != Some(CENTRAL_SIGNATURE) {
            return Err(Pk3Error::BadSignature {
                what: "central directory",
                at: directory.offset + at as u64,
            });
        }
        let record = central.get(at..at + CENTRAL_LEN).ok_or(truncated.clone())?;
        let name_len = u16_at(record, 28).ok_or(truncated.clone())? as usize;
        let extra_len = u16_at(record, 30).ok_or(truncated.clone())? as usize;
        let comment_len = u16_at(record, 32).ok_or(truncated.clone())? as usize;
        let name = central
            .get(at + CENTRAL_LEN..at + CENTRAL_LEN + name_len)
            .ok_or(truncated.clone())?;
        entries.push(Pk3Entry {
            name: crate::game_path(&String::from_utf8_lossy(name)),
            method: u16_at(record, 10).ok_or(truncated.clone())?,
            crc: u32_at(record, 16).ok_or(truncated.clone())?,
            compressed: u32_at(record, 20).ok_or(truncated.clone())?,
            size: u32_at(record, 24).ok_or(truncated.clone())?,
            local_header: u32_at(record, 42).ok_or(truncated)?,
        });
        at += CENTRAL_LEN + name_len + extra_len + comment_len;
    }
    Ok(entries)
}

/// The offset of the entry's data from its local header, given the local
/// header's first `LOCAL_HEADER_LEN` bytes.
pub fn data_offset(local: &[u8], entry: &Pk3Entry) -> Result<u64, Pk3Error> {
    if u32_at(local, 0) != Some(LOCAL_SIGNATURE) {
        return Err(Pk3Error::BadSignature {
            what: "local header",
            at: u64::from(entry.local_header),
        });
    }
    let truncated = Pk3Error::Truncated {
        what: "local header",
    };
    let name_len = u16_at(local, 26).ok_or(truncated.clone())?;
    let extra_len = u16_at(local, 28).ok_or(truncated)?;
    Ok(LOCAL_HEADER_LEN as u64 + u64::from(name_len) + u64::from(extra_len))
}

/// Decompresses an entry's data and checks its size and CRC-32.
pub fn decode(entry: &Pk3Entry, data: &[u8]) -> Result<Vec<u8>, Pk3Error> {
    let bytes = match entry.method {
        0 => data.to_vec(),
        // Inflating stops at the directory's size, so a small entry cannot
        // claim gigabytes.
        8 => miniz_oxide::inflate::decompress_to_vec_with_limit(data, entry.size as usize)
            .map_err(|_| Pk3Error::Inflate {
                name: entry.name.clone(),
            })?,
        method => {
            return Err(Pk3Error::UnsupportedMethod {
                name: entry.name.clone(),
                method,
            });
        }
    };
    if bytes.len() != entry.size as usize {
        return Err(Pk3Error::SizeMismatch {
            name: entry.name.clone(),
            expected: entry.size,
            got: bytes.len(),
        });
    }
    let crc = crc32(&bytes);
    if crc != entry.crc {
        return Err(Pk3Error::CrcMismatch {
            name: entry.name.clone(),
            expected: entry.crc,
            got: crc,
        });
    }
    Ok(bytes)
}

const CRC_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut value = i as u32;
        let mut bit = 0;
        while bit < 8 {
            value = if value & 1 != 0 {
                0xedb8_8320 ^ (value >> 1)
            } else {
                value >> 1
            };
            bit += 1;
        }
        table[i] = value;
        i += 1;
    }
    table
};

/// CRC-32 as zip uses it (reflected, polynomial 0xedb88320).
pub fn crc32(bytes: &[u8]) -> u32 {
    !bytes.iter().fold(!0u32, |crc, &byte| {
        CRC_TABLE[((crc ^ u32::from(byte)) & 0xff) as usize] ^ (crc >> 8)
    })
}
