//! GFF V3.2: the generic tree format behind most KOTOR data (areas, instance
//! lists, templates, dialogues, GUIs).
//!
//! Header (56 bytes): file type (4), version `V3.2`, then (offset, count)
//! pairs for structs, fields, labels, field data, field indices and list
//! indices; the last three counts are in bytes.
//! Struct (12 bytes): id, data, field count. With one field, `data` is that
//! field's index; with more, a byte offset into the field indices.
//! Field (12 bytes): type, label index, data. Simple types hold their value
//! in `data`; complex types hold a byte offset into the field data; a struct
//! field holds a struct index; a list field holds a byte offset into the list
//! indices, where a count is followed by that many struct indices.
//! Label: 16 bytes, NUL-padded.

use std::cell::Cell;

use crate::FormatError;
use crate::reader::Reader;

const FORMAT: &str = "GFF";
const HEADER_LEN: usize = 56;
/// How many times over its struct and field records a file may be read. A
/// tree reads each once; a file whose lists name a shared struct would
/// otherwise read it once per path, which doubles per level.
const REVISITS: usize = 4;

#[derive(Debug, Clone, PartialEq)]
pub struct Gff {
    /// The 4-byte file type, e.g. `GIT ` or `UTC `.
    pub file_type: [u8; 4],
    pub root: GffStruct,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GffStruct {
    pub id: u32,
    /// In file order. A label can repeat; see `get`.
    pub fields: Vec<GffField>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GffField {
    /// Up to 16 bytes, as stored before the first NUL.
    pub label: Vec<u8>,
    pub value: GffValue,
}

/// Floats are kept as their bits so that writing a file back reproduces it
/// exactly.
#[derive(Debug, Clone, PartialEq)]
pub enum GffValue {
    Byte(u8),
    Char(i8),
    Word(u16),
    Short(i16),
    Dword(u32),
    Int(i32),
    Dword64(u64),
    Int64(i64),
    Float(u32),
    Double(u64),
    /// Bytes in the game's code page, without a terminator.
    String(Vec<u8>),
    ResRef(Vec<u8>),
    LocString(LocString),
    Void(Vec<u8>),
    Struct(Box<GffStruct>),
    List(Vec<GffStruct>),
    /// Four floats (a quaternion), as bits.
    Orientation([u32; 4]),
    /// Three floats, as bits.
    Vector([u32; 3]),
    StrRef(u32),
}

#[derive(Debug, Clone, PartialEq)]
pub struct LocString {
    /// Talk-table index, or `0xffff_ffff` for none.
    pub strref: u32,
    /// (language and gender id, text) in file order.
    pub strings: Vec<(u32, Vec<u8>)>,
}

impl GffStruct {
    /// The value under `label`. When a struct repeats a label, the last
    /// field wins, as it does in the xoreos reference (its fields are a map
    /// filled in file order).
    pub fn get(&self, label: &str) -> Option<&GffValue> {
        self.fields
            .iter()
            .rev()
            .find(|field| field.label == label.as_bytes())
            .map(|field| &field.value)
    }

    /// An integer field of any width, widened.
    pub fn int(&self, label: &str) -> Option<i64> {
        self.get(label)?.int()
    }

    /// An integer field as xoreos reads an unsigned one: any width, cut to
    /// 32 bits.
    pub fn uint(&self, label: &str) -> Option<u32> {
        self.get(label)?.uint()
    }

    /// A float field; a double or an integer one read as a float.
    pub fn float(&self, label: &str) -> Option<f32> {
        self.get(label)?.float()
    }

    pub fn vector(&self, label: &str) -> Option<[f32; 3]> {
        match self.get(label)? {
            GffValue::Vector(bits) => Some(bits.map(f32::from_bits)),
            _ => None,
        }
    }

    /// A resref field, or a string field holding one (`GffValue::resref`);
    /// `None` when absent or empty.
    pub fn resref(&self, label: &str) -> Option<String> {
        self.get(label)?.resref()
    }

    /// A string field's bytes (the game's code page).
    pub fn bytes(&self, label: &str) -> Option<&[u8]> {
        self.get(label)?.bytes()
    }

    pub fn locstring(&self, label: &str) -> Option<&LocString> {
        match self.get(label)? {
            GffValue::LocString(loc) => Some(loc),
            _ => None,
        }
    }

    /// A struct field.
    pub fn child(&self, label: &str) -> Option<&GffStruct> {
        match self.get(label)? {
            GffValue::Struct(child) => Some(child),
            _ => None,
        }
    }

    /// A list field's items; empty when absent.
    pub fn list(&self, label: &str) -> &[GffStruct] {
        match self.get(label) {
            Some(GffValue::List(items)) => items,
            _ => &[],
        }
    }
}

impl GffValue {
    /// An integer of any width, widened (as xoreos reads a signed one).
    pub fn int(&self) -> Option<i64> {
        Some(match self {
            GffValue::Byte(v) => i64::from(*v),
            GffValue::Char(v) => i64::from(*v),
            GffValue::Word(v) => i64::from(*v),
            GffValue::Short(v) => i64::from(*v),
            GffValue::Dword(v) => i64::from(*v),
            GffValue::Int(v) => i64::from(*v),
            GffValue::Dword64(v) => *v as i64,
            GffValue::Int64(v) => *v,
            _ => return None,
        })
    }

    /// An integer of any width cut to 32 bits (as xoreos reads an unsigned one).
    pub fn uint(&self) -> Option<u32> {
        self.int().map(|v| v as u32)
    }

    /// A float or a double; an integer read as a float. xoreos's
    /// double reader takes only the first two; the games' writers put a
    /// whole number in an integer field now and then, read so.
    pub fn float(&self) -> Option<f32> {
        match self {
            GffValue::Float(bits) => Some(f32::from_bits(*bits)),
            GffValue::Double(bits) => Some(f64::from_bits(*bits) as f32),
            other => other.int().map(|v| v as f32),
        }
    }

    /// A resref, or a string holding one: up to its first NUL (reone and
    /// xoreos read a fixed string so), trimmed and lowercased; `None` when
    /// that leaves nothing (the games' empty resref).
    pub fn resref(&self) -> Option<String> {
        let text = crate::resref(self.bytes()?);
        let text = text.trim();
        (!text.is_empty()).then(|| text.to_string())
    }

    /// A string or resref's bytes (the game's code page).
    pub fn bytes(&self) -> Option<&[u8]> {
        match self {
            GffValue::String(bytes) | GffValue::ResRef(bytes) => Some(bytes),
            _ => None,
        }
    }

    pub fn type_id(&self) -> u32 {
        match self {
            GffValue::Byte(_) => 0,
            GffValue::Char(_) => 1,
            GffValue::Word(_) => 2,
            GffValue::Short(_) => 3,
            GffValue::Dword(_) => 4,
            GffValue::Int(_) => 5,
            GffValue::Dword64(_) => 6,
            GffValue::Int64(_) => 7,
            GffValue::Float(_) => 8,
            GffValue::Double(_) => 9,
            GffValue::String(_) => 10,
            GffValue::ResRef(_) => 11,
            GffValue::LocString(_) => 12,
            GffValue::Void(_) => 13,
            GffValue::Struct(_) => 14,
            GffValue::List(_) => 15,
            GffValue::Orientation(_) => 16,
            GffValue::Vector(_) => 17,
            GffValue::StrRef(_) => 18,
        }
    }
}

struct Tables<'a> {
    reader: Reader<'a>,
    structs: (usize, usize),
    fields: (usize, usize),
    labels: (usize, usize),
    data: (usize, usize),
    field_indices: (usize, usize),
    list_indices: (usize, usize),
    /// Struct and field reads still allowed (`REVISITS`).
    budget: Cell<usize>,
}

fn unsupported(what: String) -> FormatError {
    FormatError::Unsupported {
        format: FORMAT,
        what,
    }
}

impl Tables<'_> {
    fn label(&self, index: usize) -> Result<Vec<u8>, FormatError> {
        if index >= self.labels.1 {
            return Err(unsupported(format!(
                "label index {index} of {}",
                self.labels.1
            )));
        }
        let raw = self.reader.slice(self.labels.0 + index * 16, 16)?;
        let end = raw.iter().position(|&b| b == 0).unwrap_or(16);
        Ok(raw[..end].to_vec())
    }

    /// Bytes of the field data block from `offset`, bounded by the block.
    fn data_at(&self, offset: usize, len: usize) -> Result<&[u8], FormatError> {
        if offset.checked_add(len).is_none_or(|end| end > self.data.1) {
            return Err(unsupported(format!(
                "field data {offset}+{len} outside the {}-byte block",
                self.data.1
            )));
        }
        self.reader.slice(self.data.0 + offset, len)
    }

    /// Takes one record read from the budget.
    fn spend(&self) -> Result<(), FormatError> {
        let left = self.budget.get().checked_sub(1).ok_or_else(|| {
            unsupported(format!(
                "records read more than {REVISITS} times each (shared structs?)"
            ))
        })?;
        self.budget.set(left);
        Ok(())
    }

    fn data_u32(&self, offset: usize) -> Result<u32, FormatError> {
        let bytes = self.data_at(offset, 4)?;
        Ok(u32::from_le_bytes(bytes.try_into().expect("4 bytes")))
    }

    fn structure(&self, index: usize, depth: usize) -> Result<GffStruct, FormatError> {
        if depth > 64 {
            return Err(unsupported(
                "structs nested deeper than 64 (a cycle?)".into(),
            ));
        }
        if index >= self.structs.1 {
            return Err(unsupported(format!(
                "struct index {index} of {}",
                self.structs.1
            )));
        }
        self.spend()?;
        let at = self.structs.0 + index * 12;
        let id = self.reader.u32(at)?;
        let data = self.reader.u32(at + 4)? as usize;
        let count = self.reader.u32(at + 8)? as usize;
        let indices: Vec<usize> = match count {
            0 => Vec::new(),
            1 => vec![data],
            _ => {
                if data
                    .checked_add(count * 4)
                    .is_none_or(|end| end > self.field_indices.1)
                {
                    return Err(unsupported(format!("field indices {data}+{count}")));
                }
                (0..count)
                    .map(|i| {
                        self.reader
                            .u32(self.field_indices.0 + data + i * 4)
                            .map(|v| v as usize)
                    })
                    .collect::<Result<_, _>>()?
            }
        };
        let fields = indices
            .into_iter()
            .map(|field| self.field(field, depth))
            .collect::<Result<_, _>>()?;
        Ok(GffStruct { id, fields })
    }

    fn field(&self, index: usize, depth: usize) -> Result<GffField, FormatError> {
        if index >= self.fields.1 {
            return Err(unsupported(format!(
                "field index {index} of {}",
                self.fields.1
            )));
        }
        self.spend()?;
        let at = self.fields.0 + index * 12;
        let kind = self.reader.u32(at)?;
        let label = self.label(self.reader.u32(at + 4)? as usize)?;
        let raw = self.reader.slice(at + 8, 4)?;
        let data = u32::from_le_bytes(raw.try_into().expect("4 bytes"));
        let offset = data as usize;
        let bits32 = |bytes: &[u8]| u32::from_le_bytes(bytes.try_into().expect("4 bytes"));
        let value = match kind {
            0 => GffValue::Byte(raw[0]),
            1 => GffValue::Char(raw[0] as i8),
            2 => GffValue::Word(u16::from_le_bytes([raw[0], raw[1]])),
            3 => GffValue::Short(i16::from_le_bytes([raw[0], raw[1]])),
            4 => GffValue::Dword(data),
            5 => GffValue::Int(data as i32),
            8 => GffValue::Float(data),
            6 => GffValue::Dword64(u64::from_le_bytes(
                self.data_at(offset, 8)?.try_into().expect("8"),
            )),
            7 => GffValue::Int64(i64::from_le_bytes(
                self.data_at(offset, 8)?.try_into().expect("8"),
            )),
            9 => GffValue::Double(u64::from_le_bytes(
                self.data_at(offset, 8)?.try_into().expect("8"),
            )),
            10 => {
                let len = self.data_u32(offset)? as usize;
                GffValue::String(self.data_at(offset + 4, len)?.to_vec())
            }
            11 => {
                let len = usize::from(self.data_at(offset, 1)?[0]);
                GffValue::ResRef(self.data_at(offset + 1, len)?.to_vec())
            }
            12 => {
                let total = self.data_u32(offset)? as usize;
                let strref = self.data_u32(offset + 4)?;
                let count = self.data_u32(offset + 8)? as usize;
                let mut at = offset + 12;
                let mut strings = Vec::with_capacity(count.min(64));
                for _ in 0..count {
                    let language = self.data_u32(at)?;
                    let len = self.data_u32(at + 4)? as usize;
                    strings.push((language, self.data_at(at + 8, len)?.to_vec()));
                    at += 8 + len;
                }
                if at != offset + 4 + total {
                    return Err(unsupported(format!(
                        "localized string says {total} bytes, holds {}",
                        at - offset - 4
                    )));
                }
                GffValue::LocString(LocString { strref, strings })
            }
            13 => {
                let len = self.data_u32(offset)? as usize;
                GffValue::Void(self.data_at(offset + 4, len)?.to_vec())
            }
            14 => GffValue::Struct(Box::new(self.structure(offset, depth + 1)?)),
            15 => {
                if offset
                    .checked_add(4)
                    .is_none_or(|end| end > self.list_indices.1)
                {
                    return Err(unsupported(format!("list indices offset {offset}")));
                }
                let count = self.reader.u32(self.list_indices.0 + offset)? as usize;
                if offset + 4 + count * 4 > self.list_indices.1 {
                    return Err(unsupported(format!("list of {count} at {offset}")));
                }
                let mut items = Vec::with_capacity(count);
                for i in 0..count {
                    let item = self.reader.u32(self.list_indices.0 + offset + 4 + i * 4)?;
                    items.push(self.structure(item as usize, depth + 1)?);
                }
                GffValue::List(items)
            }
            16 => {
                let b = self.data_at(offset, 16)?;
                GffValue::Orientation([0, 4, 8, 12].map(|i| bits32(&b[i..i + 4])))
            }
            17 => {
                let b = self.data_at(offset, 12)?;
                GffValue::Vector([0, 4, 8].map(|i| bits32(&b[i..i + 4])))
            }
            18 => {
                let len = self.data_u32(offset)?;
                if len != 4 {
                    return Err(unsupported(format!("StrRef field of {len} bytes")));
                }
                GffValue::StrRef(self.data_u32(offset + 4)?)
            }
            other => return Err(unsupported(format!("field type {other}"))),
        };
        Ok(GffField { label, value })
    }
}

impl Gff {
    pub fn parse(bytes: &[u8]) -> Result<Gff, FormatError> {
        let reader = Reader::new(FORMAT, bytes);
        let head = reader.slice(0, 8)?;
        if &head[4..8] != b"V3.2" {
            return Err(FormatError::Signature {
                format: FORMAT,
                found: head.try_into().expect("8 bytes"),
            });
        }
        reader.slice(0, HEADER_LEN)?;
        let pair = |at: usize| -> Result<(usize, usize), FormatError> {
            Ok((reader.u32(at)? as usize, reader.u32(at + 4)? as usize))
        };
        let (structs, fields) = (pair(8)?, pair(16)?);
        let tables = Tables {
            structs,
            fields,
            labels: pair(24)?,
            data: pair(32)?,
            field_indices: pair(40)?,
            list_indices: pair(48)?,
            reader,
            budget: Cell::new(structs.1.saturating_add(fields.1).saturating_mul(REVISITS)),
        };
        tables
            .reader
            .table(tables.structs.0, tables.structs.1, 12)?;
        tables.reader.table(tables.fields.0, tables.fields.1, 12)?;
        tables.reader.table(tables.labels.0, tables.labels.1, 16)?;
        tables.reader.table(tables.data.0, tables.data.1, 1)?;
        tables
            .reader
            .table(tables.field_indices.0, tables.field_indices.1, 1)?;
        tables
            .reader
            .table(tables.list_indices.0, tables.list_indices.1, 1)?;
        if tables.structs.1 == 0 {
            return Err(unsupported("no root struct".into()));
        }
        Ok(Gff {
            file_type: head[..4].try_into().expect("4 bytes"),
            root: tables.structure(0, 0)?,
        })
    }
}
