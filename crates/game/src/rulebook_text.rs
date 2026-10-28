//! The rulebook in the game's own descriptions: a power's, a feat's, a
//! skill's or an item's description goes on with the book's paragraphs for
//! it (`rules::applying` its tags), each a summary that says its own value,
//! drawn from the constants the sim plays by.

use ui::assets::{GuiAssets, ItemLook};

/// The book's paragraphs for things tagged `tags`, to follow a description
/// (Windows-1252, as the talk table's); empty when it says nothing of them.
/// Each section and rule once, its summary a paragraph of its own.
pub(crate) fn told(tags: &[String]) -> Vec<u8> {
    let mut seen: Vec<&str> = Vec::new();
    let mut text = String::new();
    // The controls chapter is the wiki's: its keys and moves would crowd a
    // description box.
    let shown = tags
        .iter()
        .flat_map(|tag| rules::applying(tag))
        .filter(|entry| entry.chapter() != "controls");
    for entry in shown {
        if seen.contains(&entry.path) {
            continue;
        }
        seen.push(entry.path);
        text.push_str("\n\n");
        text.push_str(&flat(entry.summary));
    }
    game_bytes(&text)
}

/// The tags of row `row` of `table` (`spells`, `feat`, `skills`): its label
/// as the book's `applies` names it.
pub(crate) fn row_tags(assets: &mut GuiAssets, table: &str, row: usize) -> Vec<String> {
    let what = match table {
        "spells" => "spell",
        "feat" => "feat",
        "skills" => "skill",
        "classes" => "class",
        _ => return Vec::new(),
    };
    let Some(label) = assets.cell_as_written(table, row, "label") else {
        return Vec::new();
    };
    let mut tags = vec![format!("{what}:{label}")];
    if table == "spells" && sim::combat::force::of_row(row as u16).is_some() {
        tags.push("kind:force".to_string());
    }
    tags
}

/// The tags of an item `look`: its base item's, its kinds', the spell it
/// casts (`session::codex::item_tags`).
pub(crate) fn item_tags(assets: &mut GuiAssets, look: &ItemLook) -> Vec<String> {
    let label = assets
        .cell_as_written("baseitems", usize::from(look.base_item), "label")
        .unwrap_or_else(String::new);
    let casts = look
        .casts
        .and_then(|row| assets.cell_as_written("spells", usize::from(row), "label"));
    session::codex::item_tags(look.base_item, &label, casts.as_deref())
}

/// Prose's wrapped lines as one.
fn flat(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `text` in the talk table's encoding (Windows-1252), which the game's
/// fonts draw: the book's own signs as theirs where it has them, a minus
/// as a hyphen, anything else it lacks as `?`.
fn game_bytes(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| match c {
            c if c.is_ascii() => c as u8,
            '−' | '–' => b'-',
            '—' => 0x97,
            '…' => 0x85,
            '‘' => 0x91,
            '’' => 0x92,
            '“' => 0x93,
            '”' => 0x94,
            '‰' => 0x89,
            '×' => 0xd7,
            '°' => 0xb0,
            '½' => 0xbd,
            '¼' => 0xbc,
            '¾' => 0xbe,
            '±' => 0xb1,
            '·' => 0xb7,
            c if (c as u32) < 0x100 => c as u32 as u8,
            _ => b'?',
        })
        .collect()
}
