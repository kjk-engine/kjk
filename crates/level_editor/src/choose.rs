//! Search-and-pick fields. A field of this kind lists, under itself, the
//! things whose file name or in-game name holds what is typed: items by
//! their names as the game shows them, the tags of the level's objects,
//! quests, scripts and conversations. A click on a row puts its file name
//! or tag in the field. The Up and Down keys highlight a row, and Enter
//! then puts it in the field. The lists of the game's things are read the first
//! time a field asks for them, and kept (`Names`).

use bevy::log::info;
use bevy_egui::egui;
use fmt_odyssey::ResType;
use fmt_odyssey::gff::Gff;

use crate::state::EditorState;

/// One thing a field offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Choice {
    /// What the field takes: a file name or a tag.
    pub(crate) value: String,
    /// What the game calls it, such as "Energy Shield". Empty when it has
    /// no name of its own.
    pub(crate) name: String,
    /// What kind of thing it is, such as "Belt" or "the project's own".
    pub(crate) note: String,
}

/// What a field did this frame: its text field's response, the value
/// picked from its list, if one was, and whether the field was left with
/// what was typed in it. A click on the list takes the keys from the text
/// field, so a caller that takes the text when the field loses the keys
/// reads `left` instead, which waits for that click.
pub(crate) struct Picked {
    pub(crate) response: egui::Response,
    pub(crate) picked: Option<String>,
    pub(crate) left: bool,
}

impl Picked {
    /// Whether the field's text is done with: picked, or left.
    pub(crate) fn done(&self) -> bool {
        self.picked.is_some() || self.left
    }
}

/// How many rows the list shows at most.
const MOST: usize = 60;

/// The choices that hold `typed` in their value or their name, the best
/// first: the value it is, then values and names that start with it, then
/// names with a word that starts with it, then the rest. With nothing
/// typed, every choice in its order.
pub(crate) fn ranked<'c>(choices: &'c [Choice], typed: &str) -> Vec<&'c Choice> {
    let needle = typed.trim().to_ascii_lowercase();
    if needle.is_empty() {
        return choices.iter().take(MOST).collect();
    }
    let mut scored: Vec<(u8, &Choice)> = choices
        .iter()
        .filter_map(|choice| {
            let value = choice.value.to_ascii_lowercase();
            let name = choice.name.to_ascii_lowercase();
            let score = if value == needle {
                0
            } else if value.starts_with(&needle) {
                1
            } else if name.starts_with(&needle) {
                2
            } else if name
                .split(|c: char| !c.is_alphanumeric())
                .any(|word| word.starts_with(&needle))
            {
                3
            } else if value.contains(&needle) || name.contains(&needle) {
                4
            } else {
                return None;
            };
            Some((score, choice))
        })
        .collect();
    scored.sort_by_key(|(score, _)| *score);
    scored
        .into_iter()
        .take(MOST)
        .map(|(_, choice)| choice)
        .collect()
}

/// What the field remembers between frames: whether its list is open,
/// whether the pointer was over the list, the highlighted row, and whether
/// the arrow keys chose it.
#[derive(Debug, Clone, Copy, Default)]
struct Remembered {
    open: bool,
    over_list: bool,
    highlight: usize,
    chosen: bool,
}

/// A search-and-pick field of `width` points that edits `text`, with
/// `hint` shown while it is empty. `choices` gives what it offers, and is
/// asked only while the list is open.
pub(crate) fn field(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    text: &mut String,
    (hint, width): (&str, f32),
    choices: impl FnOnce() -> Vec<Choice>,
) -> Picked {
    let id = ui.make_persistent_id(("choose", id_salt));
    let text_id = id.with("text");
    let mut remembered: Remembered = ui
        .data(|data| data.get_temp(id))
        .unwrap_or_else(Remembered::default);
    let focused = ui.memory(|memory| memory.has_focus(text_id));
    let engaged = focused || remembered.open;
    // The arrow keys move the highlight, not the text's cursor.
    let (down, up) = if focused && remembered.open {
        ui.input_mut(|input| {
            (
                input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
            )
        })
    } else {
        (false, false)
    };
    let enter = ui.input(|input| input.key_pressed(egui::Key::Enter));
    let response = ui.add(
        egui::TextEdit::singleline(text)
            .id(text_id)
            .hint_text(hint)
            .desired_width(width),
    );
    if response.changed() {
        remembered.highlight = 0;
        remembered.chosen = false;
    }
    remembered.open = response.has_focus() || (remembered.open && remembered.over_list);
    let mut picked = None;
    if remembered.open || response.lost_focus() {
        let choices = choices();
        let rows = ranked(&choices, text);
        if down {
            remembered.highlight = (remembered.highlight + 1).min(rows.len().saturating_sub(1));
            remembered.chosen = true;
        }
        if up {
            remembered.highlight = remembered.highlight.saturating_sub(1);
            remembered.chosen = true;
        }
        remembered.highlight = remembered.highlight.min(rows.len().saturating_sub(1));
        // Enter in the field takes the row the arrow keys chose, else the
        // row that is what was typed. Other text typed stays as it is: a
        // new name may hold an older one.
        if response.lost_focus() && enter {
            let typed = text.trim();
            let row = if remembered.chosen {
                rows.get(remembered.highlight)
            } else {
                rows.first()
                    .filter(|row| row.value.eq_ignore_ascii_case(typed))
            };
            picked = row.map(|row| row.value.clone());
            remembered.open = false;
        }
        if remembered.open && !rows.is_empty() {
            let shown = egui::Area::new(id.with("list"))
                .order(egui::Order::Foreground)
                .fixed_pos(response.rect.left_bottom())
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_min_width(width.max(280.0));
                        egui::ScrollArea::vertical()
                            .max_height(260.0)
                            .show(ui, |ui| {
                                for (at, row) in rows.iter().enumerate() {
                                    let lit = remembered.chosen && at == remembered.highlight;
                                    let line = row_text(ui, row);
                                    let clicked = ui.selectable_label(lit, line);
                                    if lit && (down || up) {
                                        clicked.scroll_to_me(None);
                                    }
                                    if clicked.clicked() {
                                        picked = Some(row.value.clone());
                                    }
                                }
                            });
                    });
                });
            remembered.over_list = shown.response.contains_pointer();
        } else {
            remembered.over_list = false;
        }
    } else {
        remembered.over_list = false;
    }
    if let Some(value) = &picked {
        text.clone_from(value);
        remembered.open = false;
        remembered.over_list = false;
        remembered.chosen = false;
    }
    let left = picked.is_none() && engaged && !response.has_focus() && !remembered.open;
    if left {
        remembered.chosen = false;
    }
    ui.data_mut(|data| data.insert_temp(id, remembered));
    Picked {
        response,
        picked,
        left,
    }
}

/// A row of the list: the value, then its name and its note, dimmer.
fn row_text(ui: &egui::Ui, choice: &Choice) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let style = ui.style();
    let strong = egui::TextFormat {
        font_id: egui::TextStyle::Body.resolve(style),
        color: style.visuals.strong_text_color(),
        ..Default::default()
    };
    let weak = egui::TextFormat {
        font_id: egui::TextStyle::Small.resolve(style),
        color: style.visuals.weak_text_color(),
        valign: egui::Align::Center,
        ..Default::default()
    };
    if choice.name.is_empty() {
        job.append(&choice.value, 0.0, strong);
    } else {
        job.append(&choice.name, 0.0, strong);
        job.append(&format!("  {}", choice.value), 0.0, weak.clone());
    }
    if !choice.note.is_empty() {
        job.append(&format!("  ({})", choice.note), 0.0, weak);
    }
    job
}

/// The lists of the game's things the fields offer, read the first time
/// one is asked for.
#[derive(Default)]
pub(crate) struct Names {
    items: std::sync::OnceLock<Vec<Choice>>,
    conversations: std::sync::OnceLock<Vec<Choice>>,
    creatures: std::sync::OnceLock<Vec<Choice>>,
}

/// A text of the talk table, as the editor shows text (one byte a letter).
pub(crate) fn talk_text(state: &EditorState, strref: u32) -> Option<String> {
    let talk = state.setup.talk.as_ref()?;
    let bytes = talk.text(strref)?;
    let text: String = bytes.iter().map(|&b| char::from(b)).collect();
    let text = text.trim().to_string();
    (!text.is_empty()).then_some(text)
}

/// The text of the localized string `label` of `gff`: its own text, else
/// the talk table's.
fn localized(state: &EditorState, gff: &Gff, label: &str) -> String {
    let Some(loc) = gff.root.locstring(label) else {
        return String::new();
    };
    if let Some((_, bytes)) = loc.strings.first() {
        let own: String = bytes.iter().map(|&b| char::from(b)).collect();
        if !own.trim().is_empty() {
            return own.trim().to_string();
        }
    }
    talk_text(state, loc.strref).unwrap_or_else(String::new)
}

/// The name of each row of `baseitems.2da`, as the game shows it, else
/// its label.
fn base_item_names(state: &EditorState) -> Vec<String> {
    let Some(table) = state.setup.resolver.two_da("baseitems") else {
        return Vec::new();
    };
    (0..table.row_labels.len())
        .map(|row| {
            let named = table
                .text(row, "name")
                .and_then(|strref| strref.parse::<u32>().ok())
                .and_then(|strref| talk_text(state, strref));
            named
                .or_else(|| table.text(row, "label"))
                .unwrap_or_else(String::new)
        })
        .collect()
}

/// The game's items, by their names as the game shows them, and what kind
/// of item each is.
pub(crate) fn game_items(state: &EditorState) -> &[Choice] {
    state.names.items.get_or_init(|| {
        let started = std::time::Instant::now();
        let kinds = base_item_names(state);
        let items: Vec<Choice> = (state.setup.items.iter())
            .map(|resref| {
                let gff = (state.setup.resolver.read(resref, ResType::UTI))
                    .and_then(|bytes| Gff::parse(&bytes).ok());
                let (name, note) = gff.map_or_else(
                    || (String::new(), String::new()),
                    |gff| {
                        let kind = (gff.root.int("BaseItem"))
                            .and_then(|row| usize::try_from(row).ok())
                            .and_then(|row| kinds.get(row).cloned())
                            .unwrap_or_else(String::new);
                        (localized(state, &gff, "LocalizedName"), kind)
                    },
                );
                Choice {
                    value: resref.clone(),
                    name,
                    note,
                }
            })
            .collect();
        info!(
            target: "editor",
            "read the names of {} items in {:.0} ms",
            items.len(),
            started.elapsed().as_secs_f64() * 1000.0
        );
        items
    })
}

/// Every item an object can be given: the project's own first, then the
/// game's.
pub(crate) fn items(state: &EditorState) -> Vec<Choice> {
    let mut all: Vec<Choice> = (state.editor.project.templates.iter())
        .filter(|(_, made)| made.restype == ResType::UTI)
        .map(|(name, made)| Choice {
            value: name.clone(),
            name: level_edit::items::shown(&made.gff).0,
            note: "the project's own".to_string(),
        })
        .collect();
    all.extend(game_items(state).iter().cloned());
    all
}

/// Whether an object can be given the item `name`: the project has it, or
/// the game does.
pub(crate) fn item_known(state: &EditorState, name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    state.setup.items.binary_search(&name).is_ok()
        || (state.editor.project.templates.get(&name))
            .is_some_and(|made| made.restype == ResType::UTI)
}

/// The game's creature templates, by the creatures' names.
fn game_creatures(state: &EditorState) -> &[Choice] {
    state.names.creatures.get_or_init(|| {
        (state.setup.creatures.iter())
            .map(|resref| {
                let gff = (state.setup.resolver.read(resref, ResType::UTC))
                    .and_then(|bytes| Gff::parse(&bytes).ok());
                let name = gff.map_or_else(String::new, |gff| {
                    let first = localized(state, &gff, "FirstName");
                    let last = localized(state, &gff, "LastName");
                    format!("{first} {last}").trim().to_string()
                });
                Choice {
                    value: resref.clone(),
                    name,
                    note: String::new(),
                }
            })
            .collect()
    })
}

/// Every creature template an encounter can spawn.
pub(crate) fn creatures(state: &EditorState) -> Vec<Choice> {
    game_creatures(state).to_vec()
}

/// The tags of the level's objects, each with its kind and template.
pub(crate) fn tags(state: &EditorState) -> Vec<Choice> {
    let mut seen = std::collections::BTreeSet::new();
    let mut all = Vec::new();
    for entry in state.editor.entries() {
        let tag = state.tag_of(&entry);
        if tag.is_empty() || !seen.insert(tag.to_ascii_lowercase()) {
            continue;
        }
        all.push(Choice {
            value: tag,
            name: String::new(),
            note: format!("{:?}, {}", entry.kind, entry.template).to_lowercase(),
        });
    }
    all.sort_by(|a, b| {
        a.value
            .to_ascii_lowercase()
            .cmp(&b.value.to_ascii_lowercase())
    });
    all
}

/// The quests of the journal the project has (its own copy, else the
/// game's), by their names.
pub(crate) fn quests(state: &EditorState) -> Vec<Choice> {
    let journal = crate::state::journal(state);
    (journal.root.list("Categories").iter())
        .map(|category| {
            let tag = category
                .bytes("Tag")
                .map(|tag| String::from_utf8_lossy(tag).into_owned())
                .unwrap_or_else(String::new);
            let loc = category.locstring("Name");
            // A quest of the project names itself. The game's take their
            // names from the talk table.
            let mine = loc.is_some_and(|loc| loc.strref == u32::MAX && !loc.strings.is_empty());
            let name = loc.map_or_else(String::new, |loc| {
                (loc.strings.first())
                    .map(|(_, bytes)| bytes.iter().map(|&b| char::from(b)).collect::<String>())
                    .filter(|own| !own.trim().is_empty())
                    .or_else(|| talk_text(state, loc.strref))
                    .unwrap_or_else(String::new)
            });
            Choice {
                value: tag,
                name,
                note: if mine {
                    "the project's own".to_string()
                } else {
                    String::new()
                },
            }
        })
        .filter(|choice| !choice.value.is_empty())
        .collect()
}

/// The scripts a field can name: the project's own and those being
/// written first, then the game's compiled scripts.
pub(crate) fn scripts(state: &EditorState) -> Vec<Choice> {
    let mut own: Vec<String> = state.project_scripts.iter().cloned().collect();
    own.extend(state.drafts.keys().cloned());
    own.sort();
    own.dedup();
    let mut all: Vec<Choice> = own
        .into_iter()
        .map(|value| Choice {
            value,
            name: String::new(),
            note: "the project's own".to_string(),
        })
        .collect();
    all.extend(state.setup.scripts.iter().map(|value| Choice {
        value: value.clone(),
        name: String::new(),
        note: String::new(),
    }));
    all
}

/// The conversations an object can speak: the project's own first, then
/// the game's.
pub(crate) fn conversations(state: &EditorState) -> Vec<Choice> {
    let mut all: Vec<Choice> = (state.editor.project.conversations.keys())
        .map(|value| Choice {
            value: value.clone(),
            name: String::new(),
            note: "the project's own".to_string(),
        })
        .collect();
    all.extend(game_conversations(state));
    all
}

/// The game's conversations, by their file names.
pub(crate) fn game_conversations(state: &EditorState) -> Vec<Choice> {
    state
        .names
        .conversations
        .get_or_init(|| {
            let mut names = state.setup.resolver.names(ResType::DLG);
            names.sort();
            names.dedup();
            names
                .into_iter()
                .map(|value| Choice {
                    value,
                    name: String::new(),
                    note: String::new(),
                })
                .collect()
        })
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choice(value: &str, name: &str) -> Choice {
        Choice {
            value: value.into(),
            name: name.into(),
            note: String::new(),
        }
    }

    #[test]
    fn ranks_the_value_then_names_by_their_words() {
        let all = [
            choice("g_a_mstrrobe07", "Master Jedi Robe"),
            choice("g_i_frarmbnds01", "Energy Shield"),
            choice("g_w_lghtsbr01", "Lightsaber"),
            choice("g_w_shortsbr01", "Short Lightsaber"),
            choice("robe", ""),
        ];
        let values = |typed: &str| -> Vec<&str> {
            ranked(&all, typed)
                .into_iter()
                .map(|choice| choice.value.as_str())
                .collect()
        };
        // The value it is comes first, then names with a word that starts
        // with it.
        assert_eq!(values("robe"), ["robe", "g_a_mstrrobe07"]);
        // A name that starts with it comes before a later word of a name.
        assert_eq!(values("light"), ["g_w_lghtsbr01", "g_w_shortsbr01"]);
        assert_eq!(values("SHIELD"), ["g_i_frarmbnds01"]);
        assert_eq!(values("g_w"), ["g_w_lghtsbr01", "g_w_shortsbr01"]);
        assert!(values("nothing").is_empty());
        assert_eq!(values("").len(), all.len());
    }
}
