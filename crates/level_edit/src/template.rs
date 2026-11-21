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
    /// The object's tag, which scripts find it by.
    Tag(String),
    /// Where a door or a trigger leads (its GIT entry's `LinkedToModule`,
    /// `LinkedTo` and `LinkedToFlags`).
    Link(Link),
    /// The conversation the object speaks (`Conversation`); empty for none.
    Conversation(String),
}

/// Where a door or a trigger leads: a module (`LinkedToModule`, empty for
/// nowhere) and the tag of the door or waypoint to arrive at (`LinkedTo`,
/// empty for the module's entry). kjk takes any door or trigger whose link
/// names a module for a way out: a door when it is used, a trigger when it
/// is walked into.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Link {
    pub module: String,
    pub to: String,
    /// The arrival is a door (`LinkedToFlags` 1), not a waypoint (2).
    pub to_door: bool,
}

impl Link {
    /// Whether it leads anywhere.
    pub fn leads(&self) -> bool {
        !self.module.is_empty()
    }
}

/// Where the GIT entry `item` leads.
pub fn link_of(item: &GffStruct) -> Link {
    Link {
        module: item
            .resref("LinkedToModule")
            .map(|module| module.to_ascii_lowercase())
            .unwrap_or_else(String::new),
        to: item
            .bytes("LinkedTo")
            .map(|tag| String::from_utf8_lossy(tag).into_owned())
            .unwrap_or_else(String::new),
        to_door: item.uint("LinkedToFlags") == Some(1),
    }
}

/// Sets where the GIT entry `item` leads.
pub fn set_link(item: &mut GffStruct, link: &Link) -> Result<(), String> {
    if link.module.len() > RESREF_LEN {
        return Err(format!("{:?} is longer than a module's name", link.module));
    }
    if link.to.len() > TAG_LEN {
        return Err(format!("a tag has at most {TAG_LEN} characters"));
    }
    let flags = match (link.leads(), link.to.is_empty(), link.to_door) {
        (false, _, _) | (true, true, _) => 0,
        (true, false, true) => 1,
        (true, false, false) => 2,
    };
    set(
        item,
        "LinkedToModule",
        GffValue::ResRef(link.module.to_ascii_lowercase().into_bytes()),
    );
    let to = if link.leads() {
        link.to.as_bytes()
    } else {
        &[]
    };
    set(item, "LinkedTo", GffValue::String(to.to_vec()));
    set(item, "LinkedToFlags", GffValue::Byte(flags));
    Ok(())
}

/// The longest tag the games keep.
pub const TAG_LEN: usize = crate::git::TAG_LEN;

/// The flags a placeable's template may have changed.
pub const FLAGS: [&str; 2] = ["Useable", "HasInventory"];

/// The template type of a kind made from its template, which the editor
/// gives templates of its own; `None` for a kind read whole from its GIT
/// entry (`whole`).
pub fn restype(kind: Kind) -> Option<ResType> {
    match kind {
        Kind::Placeable => Some(ResType::UTP),
        Kind::Creature => Some(ResType::UTC),
        Kind::Trigger => Some(ResType::UTT),
        Kind::Sound => Some(ResType::UTS),
        Kind::Encounter => Some(ResType::UTE),
        Kind::Store => Some(ResType::UTM),
        Kind::Door | Kind::Waypoint => None,
    }
}

/// Whether the games read an object of `kind` whole from its GIT entry, so
/// its tag and scripts are changed there (a door, a waypoint).
pub fn whole(kind: Kind) -> bool {
    restype(kind).is_none()
}

/// Whether a trigger's own tag stands in its GIT entry: the module reader
/// keeps a trigger's own tag that is not empty (`as_placed`), so the
/// editor changes a trigger's tag there too.
pub fn tag_in_entry(kind: Kind) -> bool {
    whole(kind) || kind == Kind::Trigger
}

/// The template types the project's folder may hold, with their
/// extensions.
pub const TYPES: [(ResType, &str); 6] = [
    (ResType::UTP, "utp"),
    (ResType::UTC, "utc"),
    (ResType::UTT, "utt"),
    (ResType::UTS, "uts"),
    (ResType::UTE, "ute"),
    (ResType::UTM, "utm"),
];

/// The extension a template is written with.
pub fn extension(restype: ResType) -> &'static str {
    TYPES
        .iter()
        .find(|(of, _)| *of == restype)
        .map_or("utp", |(_, extension)| extension)
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
            known_event(&gff.root, label)?;
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
        Change::Tag(tag) => set_tag(&mut gff.root, tag)?,
        Change::Conversation(name) => set_conversation(&mut gff.root, name)?,
        Change::Link(_) => return Err("a link is the GIT entry's, not its template's".into()),
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

fn set_conversation(item: &mut GffStruct, name: &str) -> Result<(), String> {
    if name.len() > RESREF_LEN {
        return Err(format!("{name:?} is longer than a conversation's name"));
    }
    set(
        item,
        "Conversation",
        GffValue::ResRef(name.to_ascii_lowercase().into_bytes()),
    );
    Ok(())
}

fn set_tag(item: &mut GffStruct, tag: &str) -> Result<(), String> {
    if tag.len() > TAG_LEN {
        return Err(format!("a tag has at most {TAG_LEN} characters"));
    }
    set(item, "Tag", GffValue::String(tag.as_bytes().to_vec()));
    Ok(())
}

/// Makes `change` in a GIT entry read whole (`whole`): its tag or an
/// event's script. Flags and items belong to placeables' templates.
pub fn apply_entry(item: &mut GffStruct, change: &Change) -> Result<(), String> {
    match change {
        Change::Script(label, script) => {
            known_event(item, label)?;
            if script.len() > RESREF_LEN {
                return Err(format!("{script:?} is longer than a script's name"));
            }
            set(
                item,
                label,
                GffValue::ResRef(script.to_ascii_lowercase().into_bytes()),
            );
            Ok(())
        }
        Change::Tag(tag) => set_tag(item, tag),
        Change::Link(link) => set_link(item, link),
        Change::Conversation(name) => set_conversation(item, name),
        _ => Err("only a placeable's template has use and items".into()),
    }
}

/// The event scripts a GIT entry read whole names (`scripts`).
pub fn entry_scripts(item: &GffStruct) -> Vec<(String, String)> {
    scripts(&Gff {
        file_type: *b"GIT ",
        root: item.clone(),
    })
}

/// Refuses an event the template or entry does not name. The games read
/// each kind's events by their own labels (a K1 trigger's `ScriptOnEnter`,
/// a placeable's `OnOpen`), and their templates hold every one, empty or
/// not: a label they lack is one the games never read.
fn known_event(item: &GffStruct, label: &str) -> Result<(), String> {
    let named = item
        .fields
        .iter()
        .any(|field| field.label == label.as_bytes() && matches!(field.value, GffValue::ResRef(_)));
    if !is_script(label) || !named {
        return Err(format!("{label} is not one of this object's events"));
    }
    Ok(())
}
