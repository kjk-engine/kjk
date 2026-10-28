//! BIF archives: resource data, indexed by `chitin.key`.
//!
//! Header (20 bytes): `BIFFV1  `, variable resource count, fixed resource
//! count, variable table offset. Variable record (16 bytes): id, data offset,
//! data size, type (u32). KOTOR uses no fixed resources; a BIF that has them
//! is refused.

use crate::reader::Reader;
use crate::{FormatError, ResType};

const FORMAT: &str = "BIF";
pub const HEADER_LEN: usize = 20;
const RECORD: usize = 16;

#[derive(Debug, Clone)]
pub struct Bif {
    pub resources: Vec<BifResource>,
}

#[derive(Debug, Clone, Copy)]
pub struct BifResource {
    pub id: u32,
    pub offset: u32,
    pub size: u32,
    pub restype: ResType,
}

impl Bif {
    /// How many leading bytes `parse` needs: the header and the whole
    /// variable table. Lets a caller read the table without the data.
    pub fn table_end(header: &[u8]) -> Result<usize, FormatError> {
        let reader = Reader::new(FORMAT, header);
        reader.expect_tag(b"BIFFV1  ")?;
        let count = reader.u32(8)? as usize;
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

    /// Parses the header and variable table. `bytes` may stop after the
    /// table; resource data is not read.
    pub fn parse(bytes: &[u8]) -> Result<Bif, FormatError> {
        let reader = Reader::new(FORMAT, bytes);
        reader.expect_tag(b"BIFFV1  ")?;
        let count = reader.u32(8)? as usize;
        let fixed = reader.u32(12)?;
        let table = reader.u32(16)? as usize;
        if fixed != 0 {
            return Err(FormatError::Unsupported {
                format: FORMAT,
                what: format!("{fixed} fixed resources"),
            });
        }
        reader.table(table, count, RECORD)?;
        let resources = (0..count)
            .map(|i| table + i * RECORD)
            .map(|record| {
                Ok(BifResource {
                    id: reader.u32(record)?,
                    offset: reader.u32(record + 4)?,
                    size: reader.u32(record + 8)?,
                    restype: ResType(reader.u32(record + 12)?),
                })
            })
            .collect::<Result<_, FormatError>>()?;
        Ok(Bif { resources })
    }
}
