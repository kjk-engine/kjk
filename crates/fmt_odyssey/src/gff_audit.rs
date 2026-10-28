//! Irregularities in a GFF's tables that a tree cannot represent: fields or
//! structs referenced twice or never, repeated labels in one struct, and
//! bytes no field reaches. A file with none of them is "regular", and the
//! writer is expected to reproduce it byte for byte.

use crate::FormatError;
use crate::gff::{Gff, GffStruct};
use crate::reader::Reader;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct GffAudit {
    /// Structs holding one label more than once.
    pub repeated_labels: usize,
    /// Of those, how many repeat a label with different values, so that
    /// "first wins" and "last wins" read different data.
    pub repeated_with_different_values: usize,
    /// Field records referenced by more than one struct.
    pub shared_fields: usize,
    /// Field records no struct references.
    pub orphan_fields: usize,
    /// Structs referenced from more than one place.
    pub shared_structs: usize,
    /// Structs nothing references (other than the root).
    pub orphan_structs: usize,
    /// Bytes of the field data, field indices and list indices blocks that
    /// no field reaches.
    pub unreached_bytes: usize,
}

impl GffAudit {
    pub fn is_regular(&self) -> bool {
        *self == GffAudit::default()
    }
}

fn repeated(node: &GffStruct, audit: &mut GffAudit) {
    let mut seen: Vec<&[u8]> = Vec::new();
    let mut repeats = false;
    let mut differ = false;
    for field in &node.fields {
        if seen.contains(&field.label.as_slice()) {
            repeats = true;
            let values: Vec<_> = node
                .fields
                .iter()
                .filter(|other| other.label == field.label)
                .map(|other| &other.value)
                .collect();
            differ |= values.windows(2).any(|pair| pair[0] != pair[1]);
        }
        seen.push(&field.label);
        match &field.value {
            crate::gff::GffValue::Struct(child) => repeated(child, audit),
            crate::gff::GffValue::List(items) => {
                items.iter().for_each(|item| repeated(item, audit))
            }
            _ => {}
        }
    }
    audit.repeated_labels += usize::from(repeats);
    audit.repeated_with_different_values += usize::from(differ);
}

/// Audits a file that `Gff::parse` accepts.
pub fn audit(bytes: &[u8]) -> Result<GffAudit, FormatError> {
    let gff = Gff::parse(bytes)?;
    let mut audit = GffAudit::default();
    repeated(&gff.root, &mut audit);

    let reader = Reader::new("GFF", bytes);
    let pair = |at: usize| -> Result<(usize, usize), FormatError> {
        Ok((reader.u32(at)? as usize, reader.u32(at + 4)? as usize))
    };
    let (struct_at, struct_count) = pair(8)?;
    let (field_at, field_count) = pair(16)?;
    let (_, data_len) = pair(32)?;
    let (indices_at, indices_len) = pair(40)?;
    let (lists_at, lists_len) = pair(48)?;

    let mut field_refs = vec![0usize; field_count];
    let mut struct_refs = vec![0usize; struct_count];
    let mut data_used = vec![false; data_len];
    let mut indices_used = vec![false; indices_len];
    let mut lists_used = vec![false; lists_len];
    let mark = |used: &mut Vec<bool>, from: usize, len: usize| {
        for flag in used.iter_mut().skip(from).take(len) {
            *flag = true;
        }
    };

    struct_refs[0] += 1;
    for index in 0..struct_count {
        let at = struct_at + index * 12;
        let data = reader.u32(at + 4)? as usize;
        let count = reader.u32(at + 8)? as usize;
        match count {
            0 => {}
            1 => {
                if let Some(slot) = field_refs.get_mut(data) {
                    *slot += 1;
                }
            }
            _ => {
                mark(&mut indices_used, data, count * 4);
                for i in 0..count {
                    let field = reader.u32(indices_at + data + i * 4)? as usize;
                    if let Some(slot) = field_refs.get_mut(field) {
                        *slot += 1;
                    }
                }
            }
        }
    }
    for index in 0..field_count {
        let at = field_at + index * 12;
        let kind = reader.u32(at)?;
        let data = reader.u32(at + 8)? as usize;
        let data_u32 = |offset: usize| -> Result<usize, FormatError> {
            Ok(reader.u32(pair(32)?.0 + offset)? as usize)
        };
        let used = match kind {
            6 | 7 | 9 => 8,
            10 | 13 => 4 + data_u32(data)?,
            11 => 1 + usize::from(reader.slice(pair(32)?.0 + data, 1)?[0]),
            12 => 4 + data_u32(data)?,
            16 => 16,
            17 => 12,
            18 => 8,
            14 => {
                if let Some(slot) = struct_refs.get_mut(data) {
                    *slot += 1;
                }
                0
            }
            15 => {
                let count = reader.u32(lists_at + data)? as usize;
                mark(&mut lists_used, data, 4 + count * 4);
                for i in 0..count {
                    let item = reader.u32(lists_at + data + 4 + i * 4)? as usize;
                    if let Some(slot) = struct_refs.get_mut(item) {
                        *slot += 1;
                    }
                }
                0
            }
            _ => 0,
        };
        if used > 0 {
            mark(&mut data_used, data, used);
        }
    }
    audit.shared_fields = field_refs.iter().filter(|&&n| n > 1).count();
    audit.orphan_fields = field_refs.iter().filter(|&&n| n == 0).count();
    audit.shared_structs = struct_refs.iter().filter(|&&n| n > 1).count();
    audit.orphan_structs = struct_refs.iter().filter(|&&n| n == 0).count();
    audit.unreached_bytes = [&data_used, &indices_used, &lists_used]
        .iter()
        .map(|used| used.iter().filter(|&&flag| !flag).count())
        .sum();
    Ok(audit)
}
