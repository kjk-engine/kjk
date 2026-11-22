//! A project's own quests: the games' journal (`global.jrl`) with quests
//! added to it.
//!
//! The games keep every quest in one file. A quest is a category of it: its
//! tag, the name the journal shows, and its entries, each a number and a
//! text. A script or a conversation adds an entry by the quest's tag and
//! the entry's number (`AddJournalQuestEntry`), and an entry marked as the
//! end closes the quest.
//!
//! A project that adds a quest holds a copy of the whole file among its
//! templates (`Project::templates`, as `global`), written to its folder as
//! `global.jrl`. The games' own quests stay in it as they are.
//!
//! The file is one for the whole game, so two mods that each brought one
//! would take each other's place. An exported mod therefore brings only
//! the project's own quests (`own_quests`), in a file of its own name
//! (`mods`), and kjk adds every mod's quests to the game's journal when it
//! loads (`merge`). The original games do not read that file.

use fmt_odyssey::gff::{Gff, GffField, GffStruct, GffValue, LocString};

/// The name the journal has among the games' files and a project's
/// templates.
pub const NAME: &str = "global";

fn field(label: &str, value: GffValue) -> GffField {
    GffField {
        label: label.as_bytes().to_vec(),
        value,
    }
}

fn set(item: &mut GffStruct, label: &str, value: GffValue) {
    match item
        .fields
        .iter_mut()
        .rev()
        .find(|field| field.label == label.as_bytes())
    {
        Some(field) => field.value = value,
        None => item.fields.push(self::field(label, value)),
    }
}

/// The list `label` of `item`, made where it has none.
fn list<'a>(item: &'a mut GffStruct, label: &str) -> &'a mut Vec<GffStruct> {
    let at = match item
        .fields
        .iter()
        .position(|field| field.label == label.as_bytes())
    {
        Some(at) => at,
        None => {
            item.fields.push(field(label, GffValue::List(Vec::new())));
            item.fields.len() - 1
        }
    };
    let slot = &mut item.fields[at];
    if !matches!(slot.value, GffValue::List(_)) {
        slot.value = GffValue::List(Vec::new());
    }
    match &mut slot.value {
        GffValue::List(items) => items,
        _ => unreachable!("set to a list above"),
    }
}

/// A text of the project's own, with no line of the talk table.
fn own_text(text: &str) -> GffValue {
    let bytes: Vec<u8> = text
        .chars()
        .map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?'))
        .collect();
    GffValue::LocString(LocString {
        strref: u32::MAX,
        strings: vec![(0, bytes)],
    })
}

fn text_of(item: &GffStruct, label: &str) -> String {
    (item.locstring(label))
        .and_then(|text| text.strings.first())
        .map_or_else(String::new, |(_, bytes)| {
            bytes.iter().map(|&byte| char::from(byte)).collect()
        })
}

fn tag_of(category: &GffStruct) -> String {
    category
        .resref("Tag")
        .or_else(|| {
            category
                .bytes("Tag")
                .map(|tag| String::from_utf8_lossy(tag).into_owned())
        })
        .unwrap_or_else(String::new)
}

/// Whether a quest is a project's own: it names itself. The games' own
/// quests take their names from the talk table.
fn own(category: &GffStruct) -> bool {
    (category.locstring("Name"))
        .is_some_and(|name| name.strref == u32::MAX && !name.strings.is_empty())
}

/// A journal of the project's own quests alone, out of `jrl`. `None` when
/// it has none.
pub fn own_quests(jrl: &Gff) -> Option<Gff> {
    let quests: Vec<GffStruct> = (jrl.root.list("Categories").iter())
        .filter(|category| own(category))
        .cloned()
        .collect();
    if quests.is_empty() {
        return None;
    }
    let mut made = empty();
    *list(&mut made.root, "Categories") = quests;
    Some(made)
}

/// Adds the quests of `more` to `jrl`. A quest whose tag `jrl` has already
/// is left out, and its tag is returned.
pub fn merge(jrl: &mut Gff, more: &Gff) -> Vec<String> {
    let mut had: Vec<String> = tags(jrl)
        .into_iter()
        .map(|tag| tag.to_ascii_lowercase())
        .collect();
    let mut twice = Vec::new();
    for category in more.root.list("Categories") {
        let tag = tag_of(category).to_ascii_lowercase();
        if had.contains(&tag) {
            twice.push(tag);
            continue;
        }
        had.push(tag);
        list(&mut jrl.root, "Categories").push(category.clone());
    }
    twice
}

/// Why `tag` cannot be a quest's tag, if it cannot.
pub fn tag_problem(tag: &str) -> Option<String> {
    let fits = !tag.is_empty()
        && tag.len() <= crate::git::TAG_LEN
        && tag
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
    (!fits).then(|| {
        format!(
            "{tag:?}: a quest's tag is 1 to {} letters, digits and _",
            crate::git::TAG_LEN
        )
    })
}

/// An empty journal, for a game that has none.
pub fn empty() -> Gff {
    Gff {
        file_type: *b"JRL ",
        root: GffStruct {
            id: u32::MAX,
            fields: vec![field("Categories", GffValue::List(Vec::new()))],
        },
    }
}

fn category_mut<'a>(jrl: &'a mut Gff, tag: &str) -> Option<&'a mut GffStruct> {
    list(&mut jrl.root, "Categories")
        .iter_mut()
        .find(|category| tag_of(category).eq_ignore_ascii_case(tag))
}

/// Adds the quest `tag` to the journal, named `name`, with no entry yet.
/// Refused when the journal has a quest of that tag.
pub fn add_quest(jrl: &mut Gff, tag: &str, name: &str) -> Result<(), String> {
    if let Some(problem) = tag_problem(tag) {
        return Err(problem);
    }
    if name.is_empty() {
        return Err("a quest needs a name".into());
    }
    if category_mut(jrl, tag).is_some() {
        return Err(format!("the journal has a quest {tag} already"));
    }
    let categories = list(&mut jrl.root, "Categories");
    let id = categories.len() as u32;
    categories.push(GffStruct {
        id,
        fields: vec![
            field("Name", own_text(name)),
            field("Tag", GffValue::String(tag.as_bytes().to_vec())),
            field("Priority", GffValue::Dword(4)),
            field("PlanetID", GffValue::Int(-1)),
            field("PlotIndex", GffValue::Int(-1)),
            field("Comment", GffValue::String(Vec::new())),
            field("EntryList", GffValue::List(Vec::new())),
        ],
    });
    Ok(())
}

/// Sets the entry `id` of the quest `tag`: its text, and whether it closes
/// the quest. An entry the quest does not have yet is added, and the
/// entries are kept in the order of their numbers.
pub fn set_entry(jrl: &mut Gff, tag: &str, id: u32, text: &str, end: bool) -> Result<(), String> {
    if text.is_empty() {
        return Err("a journal entry needs a text".into());
    }
    let category =
        category_mut(jrl, tag).ok_or_else(|| format!("the journal has no quest {tag}"))?;
    let entries = list(category, "EntryList");
    let numbered = |entry: &GffStruct| entry.int("ID").map(|n| n as u32);
    match entries.iter_mut().find(|entry| numbered(entry) == Some(id)) {
        Some(entry) => {
            set(entry, "Text", own_text(text));
            set(entry, "End", GffValue::Word(u16::from(end)));
        }
        None => {
            entries.push(GffStruct {
                id: 0,
                fields: vec![
                    field("ID", GffValue::Dword(id)),
                    field("Text", own_text(text)),
                    field("End", GffValue::Word(u16::from(end))),
                    field("XP_Percentage", GffValue::Float(0f32.to_bits())),
                ],
            });
            entries.sort_by_key(|entry| numbered(entry).unwrap_or(u32::MAX));
            for (at, entry) in entries.iter_mut().enumerate() {
                entry.id = at as u32;
            }
        }
    }
    Ok(())
}

/// Takes the quest `tag` out of the journal.
pub fn drop_quest(jrl: &mut Gff, tag: &str) -> Result<(), String> {
    let categories = list(&mut jrl.root, "Categories");
    let at = categories
        .iter()
        .position(|category| tag_of(category).eq_ignore_ascii_case(tag))
        .ok_or_else(|| format!("the journal has no quest {tag}"))?;
    categories.remove(at);
    Ok(())
}

/// One quest as the editor says it: its tag, its name, and each entry's
/// number, text and whether it closes the quest.
pub type Shown = (String, String, Vec<(u32, String, bool)>);

/// The quest `tag` of the journal, if it has it.
pub fn quest(jrl: &Gff, tag: &str) -> Option<Shown> {
    let category = jrl
        .root
        .list("Categories")
        .iter()
        .find(|category| tag_of(category).eq_ignore_ascii_case(tag))?;
    let entries = category
        .list("EntryList")
        .iter()
        .filter_map(|entry| {
            Some((
                entry.int("ID")? as u32,
                text_of(entry, "Text"),
                entry.int("End").unwrap_or(0) != 0,
            ))
        })
        .collect();
    Some((tag_of(category), text_of(category, "Name"), entries))
}

/// Every quest's tag in the journal, in the file's order.
pub fn tags(jrl: &Gff) -> Vec<String> {
    (jrl.root.list("Categories").iter()).map(tag_of).collect()
}
