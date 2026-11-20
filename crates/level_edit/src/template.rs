//! The templates a project brings: copies of the install's placeable and
//! creature templates (`.utp`, `.utc`), changed for one object.
//!
//! A placeable or a creature placed in an area is made from its template.
//! The area's GIT entry gives it a place and names the template; the games
//! read nothing else of it (`asset_odyssey::module`, `as_placed`). The
//! template says what the object does. It names the scripts the object runs
//! on each event (a placeable's `OnOpen`, `OnUsed`, a creature's
//! `ScriptHeartbeat`, …). It says whether a placeable can be used
//! (`Useable`) and holds items (`HasInventory`), and which items it starts
//! with (`ItemList`).
//!
//! So an object's scripts and its use are changed in a template of its
//! own. The first change copies the object's template into the project
//! under a new name (`kje1_metalbox001`), points the object's GIT entry at
//! the copy, and changes the copy. The project's folder is read over the
//! install's (`vfs::project`), so play makes the object from the copy.

use fmt_odyssey::ResType;
use fmt_odyssey::gff::{Gff, GffField, GffStruct, GffValue};

use crate::git::{Kind, RESREF_LEN};

/// The start of the names the editor gives its copies: a name no template
/// of the games has.
pub const PREFIX: &str = "kje";

/// A template the project holds.
#[derive(Debug, Clone, PartialEq)]
pub struct Template {
    /// `UTP` for a placeable, `UTC` for a creature.
    pub restype: ResType,
    /// The template it was copied from.
    pub from: String,
    pub gff: Gff,
}

/// A change to an object's template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// An event's script: the label (`OnOpen`, `ScriptHeartbeat`) and the
    /// script's resref, empty for none.
    Script(String, String),
    /// A placeable's `Useable` or `HasInventory`.
    Flag(String, bool),
    /// An item it starts with, by its template's resref (`.uti`).
    AddItem(String),
    /// Removes the item at this place in `ItemList`.
    RemoveItem(usize),
}

/// The flags a placeable's template may have changed.
pub const FLAGS: [&str; 2] = ["Useable", "HasInventory"];

/// The template type of a kind the editor gives templates of its own.
pub fn restype(kind: Kind) -> Option<ResType> {
    match kind {
        Kind::Placeable => Some(ResType::UTP),
        Kind::Creature => Some(ResType::UTC),
        _ => None,
    }
}

/// The extension a template is written with.
pub fn extension(restype: ResType) -> &'static str {
    if restype == ResType::UTC {
        "utc"
    } else {
        "utp"
    }
}

/// Whether `label` names an event's script: a placeable's `On…` field or a
/// creature's `Script…` field.
pub fn is_script(label: &str) -> bool {
    label.starts_with("On") || label.starts_with("Script")
}

/// The event scripts a template names: every resref field labelled as one
/// (`is_script`), in the template's order, empty ones too. The games'
/// templates hold all their events, empty or not.
pub fn scripts(gff: &Gff) -> Vec<(String, String)> {
    gff.root
        .fields
        .iter()
        .filter_map(|field| {
            let label = String::from_utf8_lossy(&field.label).into_owned();
            match &field.value {
                GffValue::ResRef(resref) if is_script(&label) => {
                    Some((label, String::from_utf8_lossy(resref).to_ascii_lowercase()))
                }
                _ => None,
            }
        })
        .collect()
}

/// A flag of the template; `None` where it has no such field.
pub fn flag(gff: &Gff, label: &str) -> Option<bool> {
    gff.root.uint(label).map(|value| value != 0)
}

/// The items a template starts with (`ItemList`'s `InventoryRes`).
pub fn items(gff: &Gff) -> Vec<String> {
    gff.root
        .list("ItemList")
        .iter()
        .map(|item| {
            item.resref("InventoryRes")
                .unwrap_or_else(String::new)
                .to_ascii_lowercase()
        })
        .collect()
}

fn set(item: &mut GffStruct, label: &str, value: GffValue) {
    match item
        .fields
        .iter_mut()
        .rev()
        .find(|field| field.label == label.as_bytes())
    {
        Some(field) => field.value = value,
        None => item.fields.push(GffField {
            label: label.as_bytes().to_vec(),
            value,
        }),
    }
}

fn item_list(gff: &mut Gff) -> &mut Vec<GffStruct> {
    let at = match gff
        .root
        .fields
        .iter()
        .position(|field| field.label == b"ItemList")
    {
        Some(at) => at,
        None => {
            gff.root.fields.push(GffField {
                label: b"ItemList".to_vec(),
                value: GffValue::List(Vec::new()),
            });
            gff.root.fields.len() - 1
        }
    };
    let field = &mut gff.root.fields[at];
    if !matches!(field.value, GffValue::List(_)) {
        field.value = GffValue::List(Vec::new());
    }
    match &mut field.value {
        GffValue::List(items) => items,
        _ => unreachable!("set to a list above"),
    }
}

/// Makes `change` in `gff`, a template of `restype`.
pub fn apply(gff: &mut Gff, restype: ResType, change: &Change) -> Result<(), String> {
    match change {
        Change::Script(label, script) => {
            if !is_script(label) {
                return Err(format!("{label} is not an event's script"));
            }
            if script.len() > RESREF_LEN {
                return Err(format!("{script:?} is longer than a script's name"));
            }
            set(
                &mut gff.root,
                label,
                GffValue::ResRef(script.to_ascii_lowercase().into_bytes()),
            );
        }
        Change::Flag(label, on) => {
            if restype != ResType::UTP || !FLAGS.contains(&label.as_str()) {
                return Err(format!(
                    "{label}: only a placeable's Useable and HasInventory are changed here"
                ));
            }
            set(&mut gff.root, label, GffValue::Byte(u8::from(*on)));
        }
        Change::AddItem(item) => {
            if restype != ResType::UTP {
                return Err("only a placeable's items are changed here".into());
            }
            if item.is_empty() || item.len() > RESREF_LEN {
                return Err(format!("{item:?} is not an item template's name"));
            }
            let list = item_list(gff);
            let at = list.len();
            list.push(GffStruct {
                id: at as u32,
                fields: vec![
                    GffField {
                        label: b"InventoryRes".to_vec(),
                        value: GffValue::ResRef(item.to_ascii_lowercase().into_bytes()),
                    },
                    GffField {
                        label: b"Repos_PosX".to_vec(),
                        value: GffValue::Word(at as u16),
                    },
                    GffField {
                        label: b"Repos_Posy".to_vec(),
                        value: GffValue::Word(0),
                    },
                ],
            });
        }
        Change::RemoveItem(at) => {
            let list = item_list(gff);
            if *at >= list.len() {
                return Err(format!("there is no item {at}"));
            }
            list.remove(*at);
            for (place, item) in list.iter_mut().enumerate() {
                item.id = place as u32;
                set(item, "Repos_PosX", GffValue::Word(place as u16));
            }
        }
    }
    Ok(())
}

/// A name for a copy of `from`: `kjeN_from`, cut to a resref's length, for
/// the smallest N from 1 that `taken` refuses.
pub fn fresh_name(from: &str, taken: impl Fn(&str) -> bool) -> String {
    (1..)
        .map(|n| {
            let mut name = format!("{PREFIX}{n}_{from}");
            name.truncate(RESREF_LEN);
            name
        })
        .find(|name| !taken(name))
        .expect("some number is free")
}
