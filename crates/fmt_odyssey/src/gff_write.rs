//! Writes a `Gff` back to V3.2 bytes.
//!
//! The layout reproduces the games' own files, so that a file read and
//! written again comes out identical. Everything is appended in write order,
//! depth first: a struct takes the next struct index when it is created; each
//! field is appended as it is written; a struct or list field writes its
//! children right away, before the parent's next field. A struct's block of
//! field indices is reserved, at its full size, when its second field is
//! written. A list reserves its block of indices before its items are
//! written. Labels are numbered by first use in the same order.

use crate::gff::{Gff, GffStruct, GffValue};

/// Written for a struct with no fields.
const NO_FIELDS: u32 = u32::MAX;

#[derive(Default)]
struct Out {
    structs: Vec<[u32; 3]>,
    fields: Vec<[u32; 3]>,
    labels: Vec<Vec<u8>>,
    data: Vec<u8>,
    field_indices: Vec<u8>,
    list_indices: Vec<u8>,
}

fn put(buffer: &mut [u8], at: usize, value: u32) {
    buffer[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

impl Out {
    fn label(&mut self, label: &[u8]) -> u32 {
        match self.labels.iter().position(|known| known == label) {
            Some(index) => index as u32,
            None => {
                self.labels.push(label.to_vec());
                (self.labels.len() - 1) as u32
            }
        }
    }

    fn structure(&mut self, node: &GffStruct) -> u32 {
        let index = self.structs.len();
        self.structs.push([0; 3]);
        let count = node.fields.len();
        let mut data = NO_FIELDS;
        for (k, field) in node.fields.iter().enumerate() {
            let label = self.label(&field.label);
            let field_index = self.fields.len() as u32;
            self.fields.push([0; 3]);
            match k {
                0 => data = field_index,
                1 => {
                    let at = self.field_indices.len();
                    self.field_indices.resize(at + count * 4, 0);
                    put(&mut self.field_indices, at, data);
                    put(&mut self.field_indices, at + 4, field_index);
                    data = at as u32;
                }
                _ => put(&mut self.field_indices, data as usize + k * 4, field_index),
            }
            let value = self.value(&field.value);
            self.fields[field_index as usize] = [field.value.type_id(), label, value];
        }
        self.structs[index] = [node.id, data, count as u32];
        index as u32
    }

    /// The field's 4-byte data word, appending whatever it points at.
    fn value(&mut self, value: &GffValue) -> u32 {
        let offset = self.data.len() as u32;
        let mut simple = [0u8; 4];
        match value {
            GffValue::Byte(v) => simple[0] = *v,
            GffValue::Char(v) => simple[0] = *v as u8,
            GffValue::Word(v) => simple[..2].copy_from_slice(&v.to_le_bytes()),
            GffValue::Short(v) => simple[..2].copy_from_slice(&v.to_le_bytes()),
            GffValue::Dword(v) | GffValue::Float(v) => return *v,
            GffValue::Int(v) => return *v as u32,
            GffValue::Dword64(v) | GffValue::Double(v) => {
                self.data.extend_from_slice(&v.to_le_bytes());
                return offset;
            }
            GffValue::Int64(v) => {
                self.data.extend_from_slice(&v.to_le_bytes());
                return offset;
            }
            GffValue::String(text) | GffValue::Void(text) => {
                self.data
                    .extend_from_slice(&(text.len() as u32).to_le_bytes());
                self.data.extend_from_slice(text);
                return offset;
            }
            GffValue::ResRef(text) => {
                self.data.push(text.len() as u8);
                self.data.extend_from_slice(text);
                return offset;
            }
            GffValue::LocString(loc) => {
                let body: usize = loc.strings.iter().map(|(_, text)| 8 + text.len()).sum();
                self.data
                    .extend_from_slice(&((8 + body) as u32).to_le_bytes());
                self.data.extend_from_slice(&loc.strref.to_le_bytes());
                self.data
                    .extend_from_slice(&(loc.strings.len() as u32).to_le_bytes());
                for (language, text) in &loc.strings {
                    self.data.extend_from_slice(&language.to_le_bytes());
                    self.data
                        .extend_from_slice(&(text.len() as u32).to_le_bytes());
                    self.data.extend_from_slice(text);
                }
                return offset;
            }
            GffValue::Orientation(words) => {
                words
                    .iter()
                    .for_each(|w| self.data.extend_from_slice(&w.to_le_bytes()));
                return offset;
            }
            GffValue::Vector(words) => {
                words
                    .iter()
                    .for_each(|w| self.data.extend_from_slice(&w.to_le_bytes()));
                return offset;
            }
            GffValue::StrRef(strref) => {
                self.data.extend_from_slice(&4u32.to_le_bytes());
                self.data.extend_from_slice(&strref.to_le_bytes());
                return offset;
            }
            GffValue::Struct(child) => return self.structure(child),
            GffValue::List(items) => {
                let at = self.list_indices.len();
                self.list_indices.resize(at + 4 + items.len() * 4, 0);
                put(&mut self.list_indices, at, items.len() as u32);
                for (i, item) in items.iter().enumerate() {
                    let index = self.structure(item);
                    put(&mut self.list_indices, at + 4 + i * 4, index);
                }
                return at as u32;
            }
        }
        u32::from_le_bytes(simple)
    }
}

impl Gff {
    pub fn write(&self) -> Vec<u8> {
        let mut out = Out::default();
        out.structure(&self.root);

        let sizes = [
            (out.structs.len() * 12, out.structs.len()),
            (out.fields.len() * 12, out.fields.len()),
            (out.labels.len() * 16, out.labels.len()),
            (out.data.len(), out.data.len()),
            (out.field_indices.len(), out.field_indices.len()),
            (out.list_indices.len(), out.list_indices.len()),
        ];
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&self.file_type);
        bytes.extend_from_slice(b"V3.2");
        let mut at = 56u32;
        for (len, count) in sizes {
            bytes.extend_from_slice(&at.to_le_bytes());
            bytes.extend_from_slice(&(count as u32).to_le_bytes());
            at += len as u32;
        }
        for record in out.structs.iter().chain(&out.fields) {
            record
                .iter()
                .for_each(|word| bytes.extend_from_slice(&word.to_le_bytes()));
        }
        for label in &out.labels {
            let mut padded = [0u8; 16];
            padded[..label.len()].copy_from_slice(label);
            bytes.extend_from_slice(&padded);
        }
        bytes.extend_from_slice(&out.data);
        bytes.extend_from_slice(&out.field_indices);
        bytes.extend_from_slice(&out.list_indices);
        bytes
    }
}
