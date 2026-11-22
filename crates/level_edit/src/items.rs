//! A project's own items: an item template (`.uti`) copied from one of the
//! game's and changed, such as a datapad with a text of its own or a
//! reward with a name of its own.
//!
//! A new item starts as a copy of one of the install's, so it keeps that
//! item's kind, look and properties. The project holds it among its
//! templates (`Project::templates`) under a name of its own, and it is
//! written to the project's folder as `<name>.uti`. A placeable, a creature
//! or a store then takes it by that name, as it takes any item.
//!
//! What an item does (its properties) starts as its base item's. The
//! editor changes its name, its text, its tag, a few numbers and its
//! properties (`item_props`).

use fmt_odyssey::gff::{Gff, GffField, GffStruct, GffValue, LocString};

use crate::git::RESREF_LEN;

/// A change to one of the project's items.
#[derive(Debug, Clone, PartialEq)]
pub enum ItemChange {
    /// The name the games show for it.
    Name(String),
    /// Its description, as the games show it for an item that is known and
    /// for one that is not: a datapad's text.
    Text(String),
    /// Its tag, which scripts find it by.
    Tag(String),
    /// One of `NUMBERS`, by its field.
    Number(String, f64),
}

/// The numbers of an item the editor changes: the field, what the editor
/// calls it, and its least and most.
pub const NUMBERS: [(&str, &str, f64, f64); 4] = [
    ("Cost", "Worth in credits", 0.0, 1_000_000.0),
    ("StackSize", "How many in a stack", 1.0, 999.0),
    ("Plot", "Cannot be sold or dropped", 0.0, 1.0),
    ("Identified", "Known when found", 0.0, 1.0),
];

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

/// A text of the project's own, with no line of the talk table.
fn own_text(text: &str) -> GffValue {
    let strings = if text.is_empty() {
        Vec::new()
    } else {
        // The games' code page: a byte a character.
        let bytes = text
            .chars()
            .map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?'))
            .collect();
        vec![(0, bytes)]
    };
    GffValue::LocString(LocString {
        strref: u32::MAX,
        strings,
    })
}

/// Why `name` cannot be an item's name, if it cannot: a template's name is
/// 1 to 16 lowercase letters, digits and `_`.
pub fn name_problem(name: &str) -> Option<String> {
    let fits = !name.is_empty()
        && name.len() <= RESREF_LEN
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');
    (!fits).then(|| format!("{name:?}: an item's name is 1 to 16 lowercase letters, digits and _"))
}

/// A new item named `name`, copied from the game's item `base`: it names
/// itself, and its tag is its name.
pub fn new_item(base: &Gff, name: &str) -> Result<Gff, String> {
    if let Some(problem) = name_problem(name) {
        return Err(problem);
    }
    let mut item = base.clone();
    set(
        &mut item.root,
        "TemplateResRef",
        GffValue::ResRef(name.as_bytes().to_vec()),
    );
    set(
        &mut item.root,
        "Tag",
        GffValue::String(name.as_bytes().to_vec()),
    );
    Ok(item)
}

/// The start of the note an item's `Comment` keeps of the game's item it
/// was copied from. The games do not read an item's comment.
const COPIED: &str = "kjk: copied from ";

/// Notes in the item that it was copied from the game's item `base`.
pub fn note_base(item: &mut Gff, base: &str) {
    set(
        &mut item.root,
        "Comment",
        GffValue::String(format!("{COPIED}{base}").into_bytes()),
    );
}

/// The game's item this one was copied from, as it notes it.
pub fn base_of(item: &Gff) -> Option<String> {
    let comment = String::from_utf8_lossy(item.root.bytes("Comment")?).into_owned();
    comment.strip_prefix(COPIED).map(str::to_string)
}

/// Makes `change` in the item `gff`.
pub fn apply(gff: &mut Gff, change: &ItemChange) -> Result<(), String> {
    let root = &mut gff.root;
    match change {
        ItemChange::Name(text) => {
            if text.is_empty() {
                return Err("an item needs a name".into());
            }
            set(root, "LocalizedName", own_text(text));
        }
        ItemChange::Text(text) => {
            set(root, "Description", own_text(text));
            set(root, "DescIdentified", own_text(text));
        }
        ItemChange::Tag(tag) => {
            if tag.is_empty() || tag.len() > crate::git::TAG_LEN {
                return Err(format!(
                    "an item's tag is 1 to {} characters",
                    crate::git::TAG_LEN
                ));
            }
            set(root, "Tag", GffValue::String(tag.as_bytes().to_vec()));
        }
        ItemChange::Number(label, value) => {
            let (_, shown, least, most) = NUMBERS
                .iter()
                .find(|(field, ..)| field == label)
                .ok_or_else(|| format!("{label} is not a number of an item the editor changes"))?;
            if !value.is_finite() || !(*least..=*most).contains(value) {
                return Err(format!("{shown} is from {least} to {most}"));
            }
            let whole = value.round();
            // Stored as the item has it, else as the games' items store it.
            let stored = match (root.get(label), label.as_str()) {
                (Some(GffValue::Byte(_)), _) | (None, "Plot" | "Identified") => {
                    GffValue::Byte(whole as u8)
                }
                (Some(GffValue::Word(_)), _) | (None, "StackSize") => GffValue::Word(whole as u16),
                (Some(GffValue::Int(_)), _) => GffValue::Int(whole as i32),
                _ => GffValue::Dword(whole as u32),
            };
            set(root, label, stored);
        }
    }
    Ok(())
}

/// What the editor says of an item: its name and its text as its own
/// template holds them (empty where they are lines of the talk table).
pub fn shown(gff: &Gff) -> (String, String) {
    let own = |label: &str| {
        (gff.root.locstring(label))
            .and_then(|text| text.strings.first())
            .map_or_else(String::new, |(_, bytes)| {
                bytes.iter().map(|&byte| char::from(byte)).collect()
            })
    };
    (own("LocalizedName"), own("DescIdentified"))
}
