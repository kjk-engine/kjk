//! `chitin.key`: the index of every resource stored in the BIF archives.
//!
//! Header (64 bytes): `KEY V1  `, BIF count, entry count, BIF table offset,
//! entry table offset, build year, build day, 32 reserved bytes.
//! BIF record (12 bytes): file size, name offset, name length, drives.
//! Entry record (22 bytes): resref (16), type (u16), id (u32). The id holds
//! the BIF's index in its top 12 bits and the resource's index in that BIF's
//! table in its low 20 bits.

use crate::reader::Reader;
use crate::{FormatError, ResType, resref};

const FORMAT: &str = "KEY";
const HEADER_LEN: usize = 64;
const BIF_RECORD: usize = 12;
const ENTRY_RECORD: usize = 22;

#[derive(Debug, Clone)]
pub struct Key {
    pub bifs: Vec<KeyBif>,
    pub entries: Vec<KeyEntry>,
}

#[derive(Debug, Clone)]
pub struct KeyBif {
    /// Relative to the install, with `/` separators, as the KEY spells it.
    pub path: String,
    pub size: u32,
}

#[derive(Debug, Clone)]
pub struct KeyEntry {
    pub resref: String,
    pub restype: ResType,
    /// The raw id, as the BIF's own table should repeat it.
    pub id: u32,
}

impl KeyEntry {
    pub fn bif_index(&self) -> usize {
        (self.id >> 20) as usize
    }

    pub fn resource_index(&self) -> usize {
        (self.id & 0xf_ffff) as usize
    }
}

impl Key {
    pub fn parse(bytes: &[u8]) -> Result<Key, FormatError> {
        let reader = Reader::new(FORMAT, bytes);
        reader.expect_tag(b"KEY V1  ")?;
        reader.slice(0, HEADER_LEN)?;
        let bif_count = reader.u32(8)? as usize;
        let entry_count = reader.u32(12)? as usize;
        let bif_table = reader.u32(16)? as usize;
        let entry_table = reader.u32(20)? as usize;
        reader.table(bif_table, bif_count, BIF_RECORD)?;
        reader.table(entry_table, entry_count, ENTRY_RECORD)?;

        let mut bifs = Vec::with_capacity(bif_count);
        for record in (0..bif_count).map(|i| bif_table + i * BIF_RECORD) {
            let size = reader.u32(record)?;
            let name_offset = reader.u32(record + 4)? as usize;
            let name_len = reader.u16(record + 8)? as usize;
            let name = reader.slice(name_offset, name_len)?;
            let end = name
                .iter()
                .position(|&byte| byte == 0)
                .unwrap_or(name.len());
            let path = String::from_utf8_lossy(&name[..end])
                .replace('\\', "/")
                .trim_start_matches('/')
                .to_string();
            bifs.push(KeyBif { path, size });
        }

        let mut entries = Vec::with_capacity(entry_count);
        for record in (0..entry_count).map(|i| entry_table + i * ENTRY_RECORD) {
            entries.push(KeyEntry {
                resref: resref(reader.slice(record, 16)?),
                restype: ResType(u32::from(reader.u16(record + 16)?)),
                id: reader.u32(record + 18)?,
            });
        }
        Ok(Key { bifs, entries })
    }
}
