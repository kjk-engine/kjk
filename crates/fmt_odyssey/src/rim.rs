//! RIM archives, version `V1.0`.
//!
//! Header: signature `RIM V1.0`, 4 reserved bytes, entry count, entry table
//! offset. Entry (32 bytes): resref (16), type (u16), 2 reserved bytes,
//! resource id (u32), data offset, data size. The type is read as 16 bits, as
//! both references do; the 2 bytes after it are kept in `reserved` so that
//! a non-zero value shows up in reports.

use crate::reader::Reader;
use crate::{ArchiveEntry, FormatError, ResType, resref};

const FORMAT: &str = "RIM";
pub const HEADER_LEN: usize = 20;
const RECORD: usize = 32;

#[derive(Debug, Clone)]
pub struct Rim {
    pub entries: Vec<ArchiveEntry>,
}

impl Rim {
    /// How many leading bytes `parse` needs: the header and the entry table.
    pub fn table_end(header: &[u8]) -> Result<usize, FormatError> {
        let reader = Reader::new(FORMAT, header);
        reader.expect_tag(b"RIM V1.0")?;
        let count = reader.u32(12)? as usize;
        let table = reader.u32(16)? as usize;
        count
            .checked_mul(RECORD)
            .and_then(|len| len.checked_add(table))
            .map(|end| end.max(HEADER_LEN))
            .ok_or(FormatError::Truncated {
                format: FORMAT,
                needed: usize::MAX,
                len: header.len(),
            })
    }

    /// Parses the header and entry table; `bytes` may stop after the table.
    pub fn parse(bytes: &[u8]) -> Result<Rim, FormatError> {
        let reader = Reader::new(FORMAT, bytes);
        reader.expect_tag(b"RIM V1.0")?;
        let count = reader.u32(12)? as usize;
        let table = reader.u32(16)? as usize;
        reader.table(table, count, RECORD)?;
        let entries = (0..count)
            .map(|i| table + i * RECORD)
            .map(|record| {
                Ok(ArchiveEntry {
                    resref: resref(reader.slice(record, 16)?),
                    restype: ResType(u32::from(reader.u16(record + 16)?)),
                    reserved: u32::from(reader.u16(record + 18)?),
                    offset: reader.u32(record + 24)?,
                    size: reader.u32(record + 28)?,
                })
            })
            .collect::<Result<_, FormatError>>()?;
        Ok(Rim { entries })
    }
}
