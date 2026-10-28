//! Modules made in memory for the checks: a `Source` over named bytes, and
//! the GFF and module files a check needs, written here so no install is
//! read.

use std::collections::BTreeMap;

use fmt_odyssey::ResType;
use fmt_odyssey::gff::{Gff, GffField, GffStruct, GffValue};

use crate::module::Source;

/// Files by name (`area.are`), read as a module's `Source`.
pub struct Files(pub BTreeMap<String, Vec<u8>>);

impl Source for Files {
    fn read(&self, resref: &str, restype: ResType) -> Option<Vec<u8>> {
        let key = format!("{resref}.{}", restype.extension()?);
        self.0.get(&key).cloned()
    }
    fn exists(&self, resref: &str, restype: ResType) -> bool {
        self.read(resref, restype).is_some()
    }
}

/// A GFF struct of `fields`.
pub fn node(fields: Vec<(&str, GffValue)>) -> GffStruct {
    GffStruct {
        id: 0,
        fields: fields
            .into_iter()
            .map(|(label, value)| GffField {
                label: label.as_bytes().to_vec(),
                value,
            })
            .collect(),
    }
}

/// A GFF file of `file_type` (`b"ARE "`) with `fields` at its root, written.
pub fn gff(file_type: &[u8; 4], fields: Vec<(&str, GffValue)>) -> Vec<u8> {
    Gff {
        file_type: *file_type,
        root: node(fields),
    }
    .write()
}

/// A resref field's value.
pub fn resref(name: &str) -> GffValue {
    GffValue::ResRef(name.as_bytes().to_vec())
}

/// A module `blank` with one area `area`: its ARE has `are_fields`, its
/// layout places `rooms` and its GIT lists nothing.
pub fn blank_module(are_fields: Vec<(&str, GffValue)>, rooms: &[&str]) -> Files {
    let area = node(vec![("Area_Name", resref("area"))]);
    let mut layout = format!("beginlayout\nroomcount {}\n", rooms.len());
    for room in rooms {
        layout.push_str(&format!("{room} 0 0 0\n"));
    }
    layout.push_str("donelayout\n");
    Files(BTreeMap::from([
        (
            "module.ifo".into(),
            gff(b"IFO ", vec![("Mod_Area_list", GffValue::List(vec![area]))]),
        ),
        ("area.are".into(), gff(b"ARE ", are_fields)),
        ("area.git".into(), gff(b"GIT ", Vec::new())),
        ("area.lyt".into(), layout.into_bytes()),
    ]))
}
