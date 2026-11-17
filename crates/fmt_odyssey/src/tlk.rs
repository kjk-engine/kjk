//! TLK V3.0: the talk table (`dialog.tlk`), every string the game shows or
//! speaks, indexed by strref.
//!
//! Header (20 bytes): `TLK V3.0`, language id, entry count, offset of the
//! string data. Entry (40 bytes): flags, sound resref (16), volume variance,
//! pitch variance, string offset (from the string data), string size, sound
//! length (float). A string is present only when flag bit 0 is set; both
//! references read it only then.

use std::collections::BTreeMap;

use crate::reader::Reader;
use crate::{FormatError, resref};

const FORMAT: &str = "TLK";
const HEADER_LEN: usize = 20;
const ENTRY_LEN: usize = 40;

pub const TEXT_PRESENT: u32 = 1;
pub const SOUND_PRESENT: u32 = 2;
pub const SOUND_LENGTH_PRESENT: u32 = 4;

/// The bit a strref has set when it names a line another game's talk table
/// lends (`Tlk::lent`). No line of either game's own table has it.
pub const LENT_FLAG: u32 = 0x0100_0000;

#[derive(Debug, Clone, PartialEq)]
pub struct Tlk {
    pub language: u32,
    pub entries: Vec<TlkEntry>,
    /// The lines another game's talk table lends this one, each by its
    /// strref there with `LENT_FLAG` set. KOTOR 1's table holds the names
    /// and descriptions of the KOTOR 2 Force powers a KOTOR 1 character may
    /// learn this way.
    pub lent: BTreeMap<u32, TlkEntry>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TlkEntry {
    pub flags: u32,
    pub sound: String,
    pub volume_variance: u32,
    pub pitch_variance: u32,
    /// Bytes in the game's code page; empty when `TEXT_PRESENT` is clear.
    pub text: Vec<u8>,
    /// The sound length, as bits.
    pub sound_length: u32,
}

impl Tlk {
    pub fn parse(bytes: &[u8]) -> Result<Tlk, FormatError> {
        let reader = Reader::new(FORMAT, bytes);
        reader.expect_tag(b"TLK V3.0")?;
        reader.slice(0, HEADER_LEN)?;
        let language = reader.u32(8)?;
        let count = reader.u32(12)? as usize;
        let strings = reader.u32(16)? as usize;
        reader.table(HEADER_LEN, count, ENTRY_LEN)?;
        let entries = (0..count)
            .map(|i| {
                let at = HEADER_LEN + i * ENTRY_LEN;
                let flags = reader.u32(at)?;
                let offset = reader.u32(at + 28)? as usize;
                let size = reader.u32(at + 32)? as usize;
                let text = if flags & TEXT_PRESENT != 0 {
                    reader.slice(strings + offset, size)?.to_vec()
                } else {
                    Vec::new()
                };
                Ok(TlkEntry {
                    flags,
                    sound: resref(reader.slice(at + 4, 16)?),
                    volume_variance: reader.u32(at + 20)?,
                    pitch_variance: reader.u32(at + 24)?,
                    text,
                    sound_length: reader.u32(at + 36)?,
                })
            })
            .collect::<Result<_, FormatError>>()?;
        Ok(Tlk {
            language,
            entries,
            lent: BTreeMap::new(),
        })
    }

    /// The text for a strref, or `None` when out of range or absent. A
    /// strref with `LENT_FLAG` set reads a lent line.
    pub fn text(&self, strref: u32) -> Option<&[u8]> {
        let entry = match strref & LENT_FLAG {
            0 => self.entries.get(strref as usize)?,
            _ => self.lent.get(&strref)?,
        };
        (entry.flags & TEXT_PRESENT != 0).then_some(entry.text.as_slice())
    }
}

/// A talk-table string as the games show it: what it holds in braces out
/// (developer notes, and TSL's sort keys before item names: "{cr 02}Crystal,
/// Rubat"), nested braces too; a stray closing brace stays.
pub fn unbraced(raw: &[u8]) -> Vec<u8> {
    let mut depth = 0usize;
    let mut out = Vec::with_capacity(raw.len());
    for &byte in raw {
        match byte {
            b'{' => depth += 1,
            b'}' if depth > 0 => depth -= 1,
            _ if depth > 0 => {}
            _ => out.push(byte),
        }
    }
    out
}
