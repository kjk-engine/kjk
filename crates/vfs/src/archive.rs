//! ERF-family and RIM archives on disk. The reader is chosen by the file's
//! signature, not its extension, and only the tables are read.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use fmt_odyssey::erf::{self, Erf};
use fmt_odyssey::rim::{self, Rim};
use fmt_odyssey::{ArchiveEntry, FormatError};

#[derive(Debug)]
pub struct Archive {
    pub path: PathBuf,
    pub len: u64,
    /// The 4-byte signature, e.g. `"MOD "` or `"RIM "`.
    pub signature: String,
    pub entries: Vec<ArchiveEntry>,
}

impl Archive {
    pub fn open(path: &Path) -> Result<Archive, String> {
        let mut file = File::open(path).map_err(|error| error.to_string())?;
        let len = file.metadata().map_err(|error| error.to_string())?.len();
        let mut signature = [0; 4];
        file.read_exact(&mut signature)
            .map_err(|error| format!("reading signature: {error}"))?;
        let entries = if &signature == b"RIM " {
            read_tables(&mut file, len, &signature, rim::HEADER_LEN, Rim::table_end)
                .and_then(|bytes| Rim::parse(&bytes).map_err(|error| error.to_string()))
                .map(|rim| rim.entries)?
        } else {
            read_tables(&mut file, len, &signature, erf::HEADER_LEN, Erf::table_end)
                .and_then(|bytes| Erf::parse(&bytes).map_err(|error| error.to_string()))
                .map(|erf| erf.entries)?
        };
        Ok(Archive {
            path: path.to_path_buf(),
            len,
            signature: String::from_utf8_lossy(&signature).into_owned(),
            entries,
        })
    }

    /// Entries whose data does not fit inside the file.
    pub fn out_of_bounds(&self) -> impl Iterator<Item = &ArchiveEntry> {
        self.entries
            .iter()
            .filter(|entry| u64::from(entry.offset) + u64::from(entry.size) > self.len)
    }
}

/// The file's first `table_end(header)` bytes, where `header` is its first
/// `header_len` bytes and `signature` its first 4, already read.
fn read_tables(
    file: &mut File,
    len: u64,
    signature: &[u8; 4],
    header_len: usize,
    table_end: fn(&[u8]) -> Result<usize, FormatError>,
) -> Result<Vec<u8>, String> {
    let mut bytes = signature.to_vec();
    bytes.resize(header_len.min(len as usize).max(4), 0);
    file.read_exact(&mut bytes[4..])
        .map_err(|error| format!("reading header: {error}"))?;
    let end = table_end(&bytes).map_err(|error| error.to_string())?;
    if end as u64 > len {
        return Err(format!("tables end at {end}, file is {len} bytes"));
    }
    let read = bytes.len();
    if end > read {
        bytes.resize(end, 0);
        file.read_exact(&mut bytes[read..])
            .map_err(|error| format!("reading tables: {error}"))?;
    }
    Ok(bytes)
}
