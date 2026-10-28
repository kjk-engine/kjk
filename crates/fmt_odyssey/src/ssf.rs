//! SSF V1.1: a creature's sound set, a list of strrefs (battle cries,
//! "selected", pain, …) into the talk table.
//!
//! Layout: `SSF V1.1`, the offset of the list (12), then strrefs to the end
//! of the file; `0xffff_ffff` marks an unused slot.

use crate::FormatError;
use crate::reader::Reader;

const FORMAT: &str = "SSF";

/// The lines the engines play, by their place in the list (as reone
/// numbers them; the same in K1 and TSL).
pub mod slots {
    /// The grunts of an attack begun (three) and of a blow taken (two).
    pub const ATTACK_GRUNTS: [usize; 3] = [9, 10, 11];
    pub const PAIN_GRUNTS: [usize; 2] = [12, 13];
    /// The death line.
    pub const DEAD: usize = 15;
    /// Security begun ("On it."), failed ("Damn!"), succeeded ("It's
    /// open.").
    pub const BEGIN_UNLOCK: usize = 22;
    pub const UNLOCK_FAILED: usize = 23;
    pub const UNLOCK_SUCCESS: usize = 24;
}
const HEADER_LEN: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ssf {
    /// Where the list starts; 12 in every stock file, kept for writing back.
    pub offset: u32,
    pub strrefs: Vec<u32>,
}

impl Ssf {
    pub fn parse(bytes: &[u8]) -> Result<Ssf, FormatError> {
        let reader = Reader::new(FORMAT, bytes);
        reader.expect_tag(b"SSF V1.1")?;
        let offset = reader.u32(8)?;
        let start = offset as usize;
        if start < HEADER_LEN || start > bytes.len() || !(bytes.len() - start).is_multiple_of(4) {
            return Err(FormatError::Unsupported {
                format: FORMAT,
                what: format!("list at {start} in a {}-byte file", bytes.len()),
            });
        }
        let strrefs = (start..bytes.len())
            .step_by(4)
            .map(|at| reader.u32(at))
            .collect::<Result<_, _>>()?;
        Ok(Ssf { offset, strrefs })
    }

    /// Writes the file back; bytes between the header and `offset` are zero.
    pub fn write(&self) -> Vec<u8> {
        let mut out = b"SSF V1.1".to_vec();
        out.extend_from_slice(&self.offset.to_le_bytes());
        out.resize(self.offset as usize, 0);
        for strref in &self.strrefs {
            out.extend_from_slice(&strref.to_le_bytes());
        }
        out
    }
}
