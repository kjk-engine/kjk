//! ERF-family archives: `ERF `, `MOD `, `SAV ` and `HAK `, version `V1.0`.
//!
//! Header (160 bytes): signature, version, language count, localized string
//! size, entry count, localized string offset, key list offset, resource list
//! offset, build year, build day, description strref, 116 reserved bytes.
//! Key record (24 bytes): resref (16), resource id (u32), type (u16),
//! reserved (u16). Resource record (8 bytes): data offset, data size. The two
//! lists run in parallel: key `i` describes resource `i`.
//!
//! `write` makes an archive from resources: no localized strings, the key
//! list after the header, the resource list after it, then the data in the
//! keys' order. The build date is left at zero, so the same resources make
//! the same bytes.

use crate::reader::Reader;
use crate::{ArchiveEntry, FormatError, ResType, resref};

const FORMAT: &str = "ERF";
pub const HEADER_LEN: usize = 160;
const KEY_RECORD: usize = 24;
const RESOURCE_RECORD: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErfKind {
    Erf,
    Mod,
    Sav,
    Hak,
}

#[derive(Debug, Clone)]
pub struct Erf {
    pub kind: ErfKind,
    pub entries: Vec<ArchiveEntry>,
}

struct Header {
    kind: ErfKind,
    count: usize,
    keys: usize,
    resources: usize,
}

fn header(reader: &Reader) -> Result<Header, FormatError> {
    let tag = reader.slice(0, 8)?;
    let kind = match &tag[..4] {
        b"ERF " => ErfKind::Erf,
        b"MOD " => ErfKind::Mod,
        b"SAV " => ErfKind::Sav,
        b"HAK " => ErfKind::Hak,
        _ => {
            return Err(FormatError::Signature {
                format: FORMAT,
                found: tag.try_into().expect("slice of 8"),
            });
        }
    };
    if &tag[4..] != b"V1.0" {
        return Err(FormatError::Unsupported {
            format: FORMAT,
            what: format!("version {:?}", String::from_utf8_lossy(&tag[4..])),
        });
    }
    reader.slice(0, HEADER_LEN)?;
    Ok(Header {
        kind,
        count: reader.u32(16)? as usize,
        keys: reader.u32(24)? as usize,
        resources: reader.u32(28)? as usize,
    })
}

fn end_of(at: usize, count: usize, size: usize) -> Option<usize> {
    count.checked_mul(size)?.checked_add(at)
}

impl Erf {
    /// How many leading bytes `parse` needs: the header and both lists.
    pub fn table_end(header_bytes: &[u8]) -> Result<usize, FormatError> {
        let head = header(&Reader::new(FORMAT, header_bytes))?;
        end_of(head.keys, head.count, KEY_RECORD)
            .zip(end_of(head.resources, head.count, RESOURCE_RECORD))
            .map(|(keys, resources)| keys.max(resources).max(HEADER_LEN))
            .ok_or(FormatError::Truncated {
                format: FORMAT,
                needed: usize::MAX,
                len: header_bytes.len(),
            })
    }

    /// Parses the header and both lists; `bytes` may stop after them.
    pub fn parse(bytes: &[u8]) -> Result<Erf, FormatError> {
        let reader = Reader::new(FORMAT, bytes);
        let head = header(&reader)?;
        reader.table(head.keys, head.count, KEY_RECORD)?;
        reader.table(head.resources, head.count, RESOURCE_RECORD)?;
        let entries = (0..head.count)
            .map(|i| {
                let key = head.keys + i * KEY_RECORD;
                let resource = head.resources + i * RESOURCE_RECORD;
                Ok(ArchiveEntry {
                    resref: resref(reader.slice(key, 16)?),
                    restype: ResType(u32::from(reader.u16(key + 20)?)),
                    offset: reader.u32(resource)?,
                    size: reader.u32(resource + 4)?,
                    reserved: u32::from(reader.u16(key + 22)?),
                })
            })
            .collect::<Result<_, FormatError>>()?;
        Ok(Erf {
            kind: head.kind,
            entries,
        })
    }
}

/// An archive of `kind` holding `resources` (resref, type, bytes), in their
/// order (above). Refused when a resref is longer than 16 bytes or two
/// resources share a resref and a type.
pub fn write(kind: ErfKind, resources: &[(String, ResType, Vec<u8>)]) -> Result<Vec<u8>, String> {
    let mut seen = std::collections::BTreeSet::new();
    for (resref, restype, _) in resources {
        if resref.len() > 16 || !resref.is_ascii() {
            return Err(format!(
                "{resref:?} is not a resref of 16 characters at most"
            ));
        }
        if !seen.insert((resref.to_ascii_lowercase(), restype.0)) {
            return Err(format!("{resref}.{restype} is in the archive twice"));
        }
    }
    let count = resources.len();
    let keys = HEADER_LEN;
    let records = keys + count * KEY_RECORD;
    let mut at = records + count * RESOURCE_RECORD;
    let mut out = Vec::with_capacity(
        at + resources
            .iter()
            .map(|(_, _, bytes)| bytes.len())
            .sum::<usize>(),
    );
    let tag: &[u8; 4] = match kind {
        ErfKind::Erf => b"ERF ",
        ErfKind::Mod => b"MOD ",
        ErfKind::Sav => b"SAV ",
        ErfKind::Hak => b"HAK ",
    };
    out.extend_from_slice(tag);
    out.extend_from_slice(b"V1.0");
    for word in [
        0,
        0,
        count as u32,
        HEADER_LEN as u32,
        keys as u32,
        records as u32,
        0,
        0,
        u32::MAX,
    ] {
        out.extend_from_slice(&word.to_le_bytes());
    }
    out.resize(HEADER_LEN, 0);
    for (id, (resref, restype, _)) in resources.iter().enumerate() {
        let mut name = [0u8; 16];
        name[..resref.len()].copy_from_slice(resref.to_ascii_lowercase().as_bytes());
        out.extend_from_slice(&name);
        out.extend_from_slice(&(id as u32).to_le_bytes());
        out.extend_from_slice(&(restype.0 as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
    }
    for (_, _, bytes) in resources {
        out.extend_from_slice(&(at as u32).to_le_bytes());
        out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        at += bytes.len();
    }
    for (_, _, bytes) in resources {
        out.extend_from_slice(bytes);
    }
    Ok(out)
}
