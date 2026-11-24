//! What adds each entry of the project's quests, as the quest window shows
//! it under each entry (`ui::adventure_quests`) and `edit quest show` lists
//! it among the messages:
//!
//! - a script of the project whose source calls `AddJournalQuestEntry` with
//!   the quest's tag and the entry's number written out
//!   (`level_edit::journal::added_by_source`), with what runs that script:
//!   the level's events, a new map's module events, the events of the
//!   level's objects, and the lines of the project's conversations;
//! - a line or a reply of one of the project's conversations that adds the
//!   entry when it is spoken.
//!
//! Scripts are read as the editor holds them, so a script being written
//! counts as it is typed. A script that names its quest or its entry by a
//! variable is not found.

use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};

use level_edit::dialog::Side;

use crate::state::{self, EditorState};

/// One thing that adds a quest's entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Adder {
    /// A script of the project. `blocks` says that logic blocks made it.
    /// `runs` names what runs it, as a sentence names it ("the level's
    /// OnHeartbeat event"), each once.
    Script {
        name: String,
        blocks: bool,
        runs: Vec<String>,
    },
    /// A line or a reply of one of the project's conversations, numbered as
    /// the conversation window numbers it, and its text.
    Line {
        conversation: String,
        side: Side,
        index: usize,
        text: String,
    },
}

/// What adds which entry, kept between two changes of the project.
#[derive(Default)]
pub(crate) struct Kept {
    /// What `found` was found for: the project's count of changes, the
    /// round of compiling, and a digest of the scripts being written.
    at: Option<(u64, u32, u64)>,
    found: BTreeMap<(String, u32), Vec<Adder>>,
}

impl Kept {
    /// What adds the entry `entry` of the quest `tag`, found again when the
    /// project has changed since the last time.
    pub(crate) fn of(&mut self, state: &EditorState, tag: &str, entry: u32) -> Vec<Adder> {
        let key = (
            state.editor.changes(),
            state.script_round,
            drafts_digest(state),
        );
        if self.at != Some(key) {
            self.found = find(state);
            self.at = Some(key);
        }
        (self.found.get(&(tag.to_ascii_lowercase(), entry)).cloned()).unwrap_or_else(Vec::new)
    }
}

/// A digest of the text of the scripts being written.
fn drafts_digest(state: &EditorState) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for (name, draft) in &state.drafts {
        name.hash(&mut hasher);
        draft.text.hash(&mut hasher);
    }
    state.project_scripts.len().hash(&mut hasher);
    hasher.finish()
}

/// What runs each script, by the script's name: the level's events, a new
/// map's module events, the events of the level's objects, and the lines
/// of the project's conversations.
fn runners(state: &EditorState) -> BTreeMap<String, BTreeSet<String>> {
    let mut runs: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut add = |script: &str, by: String| {
        let script = script.trim().to_ascii_lowercase();
        if !script.is_empty() {
            runs.entry(script).or_default().insert(by);
        }
    };
    if let Some(are) = state::area_are(state) {
        for label in level_edit::settings::AREA_SCRIPTS {
            if let Some(script) = are.root.resref(label) {
                add(&script, format!("the level's {label} event"));
            }
        }
    }
    if let Some(map) = &state.editor.project.map {
        for (label, script) in &map.module_scripts {
            add(script, format!("the module's {label} event"));
        }
    }
    for entry in state.editor.entries() {
        let Some(gff) = state.behaviour_source(entry.kind, &entry) else {
            continue;
        };
        let what = state::kind_name(entry.kind);
        let tag = state.tag_of(&entry);
        for run in level_edit::links::of_events(&gff) {
            add(
                &run.script,
                format!("the {} event of the {what} {tag}", run.at),
            );
        }
    }
    for (name, gff) in &state.editor.project.conversations {
        let dlg = level_edit::dialog::read(gff);
        for run in level_edit::links::of_conversation(name, &dlg) {
            add(&run.script, run.at);
        }
    }
    // A script that spends a skill check's cost runs the line's own script
    // after: what runs the one runs the other.
    let paid: Vec<(String, BTreeSet<String>)> = (runs.iter())
        .filter_map(|(script, by)| {
            let (_, _, then) = state::payment_of(state, script)?;
            (!then.is_empty()).then(|| (then, by.clone()))
        })
        .collect();
    for (then, by) in paid {
        runs.entry(then).or_default().extend(by);
    }
    runs
}

/// The text of a conversation's line: its own text, else the talk table's.
fn line_text(state: &EditorState, text: &fmt_odyssey::gff::LocString) -> String {
    if let Some((_, bytes)) = text.strings.first() {
        let own: String = bytes.iter().map(|&b| char::from(b)).collect();
        if !own.trim().is_empty() {
            return own.trim().to_string();
        }
    }
    crate::choose::talk_text(state, text.strref).unwrap_or_else(String::new)
}

/// What adds each quest entry, by the quest's tag in lowercase and the
/// entry's number.
fn find(state: &EditorState) -> BTreeMap<(String, u32), Vec<Adder>> {
    let mut found: BTreeMap<(String, u32), Vec<Adder>> = BTreeMap::new();
    let mut names: BTreeSet<String> = state.project_scripts.iter().cloned().collect();
    names.extend(state.drafts.keys().cloned());
    let runners = runners(state);
    for name in names {
        let Some(text) = state::script_text(state, &name) else {
            continue;
        };
        let mut added = level_edit::journal::added_by_source(&text);
        added.sort();
        added.dedup();
        if added.is_empty() {
            continue;
        }
        let runs: Vec<String> = (runners.get(&name).into_iter().flatten().cloned()).collect();
        for key in added {
            found.entry(key).or_default().push(Adder::Script {
                name: name.clone(),
                blocks: level_edit::blocks::is_blocks(&text),
                runs: runs.clone(),
            });
        }
    }
    for (name, gff) in &state.editor.project.conversations {
        let dlg = level_edit::dialog::read(gff);
        for (nodes, side) in [(&dlg.entries, Side::Entry), (&dlg.replies, Side::Reply)] {
            for (index, node) in nodes.iter().enumerate() {
                if node.quest.trim().is_empty() {
                    continue;
                }
                let key = (node.quest.trim().to_ascii_lowercase(), node.quest_entry);
                found.entry(key).or_default().push(Adder::Line {
                    conversation: name.clone(),
                    side,
                    index,
                    text: line_text(state, &node.text),
                });
            }
        }
    }
    found
}

/// `things` as a sentence lists them: "a", "a and b", "a, b and c". Past
/// `most`, the rest are counted ("a, b, c and 4 more").
pub(crate) fn listed(things: &[String], most: usize) -> String {
    let shown: Vec<&str> = things.iter().take(most).map(String::as_str).collect();
    let more = things.len().saturating_sub(most);
    match (shown.as_slice(), more) {
        ([], _) => String::new(),
        ([one], 0) => (*one).to_string(),
        (shown, 0) => {
            let (last, rest) = shown.split_last().unwrap_or((&"", &[]));
            format!("{} and {last}", rest.join(", "))
        }
        (shown, more) => format!("{} and {more} more", shown.join(", ")),
    }
}

/// A sentence that says what runs a script: "The level's OnEnter event
/// runs it." `them` is for logic blocks, which are many.
pub(crate) fn runs_sentence(runs: &[String], them: bool) -> String {
    let object = if them { "them" } else { "it" };
    if runs.is_empty() {
        return format!("Nothing runs {object} yet, so the entry is never added.");
    }
    let who = listed(runs, 3);
    let verb = if runs.len() == 1 { "runs" } else { "run" };
    let mut who = who.chars();
    let first = who.next().map(|c| c.to_ascii_uppercase());
    format!(
        "{}{} {verb} {object}.",
        first.into_iter().collect::<String>(),
        who.as_str()
    )
}

/// The sentences that say what adds one entry, for the quest window and
/// the messages.
pub(crate) fn sentences(adder: &Adder) -> String {
    match adder {
        Adder::Script {
            name,
            blocks: false,
            runs,
        } => format!(
            "The script {name} adds this entry. {}",
            runs_sentence(runs, false)
        ),
        Adder::Script {
            name,
            blocks: true,
            runs,
        } => format!(
            "The logic blocks of the script {name} add this entry. {}",
            runs_sentence(runs, true)
        ),
        Adder::Line {
            conversation,
            side,
            index,
            text,
        } => {
            let what = match side {
                Side::Entry => "Line",
                Side::Reply => "Reply",
            };
            let short: String = text.chars().take(80).collect();
            let cut = if text.chars().count() > 80 { "..." } else { "" };
            format!(
                "{what} {index} of the conversation {conversation} adds this entry when it is spoken. It reads: \"{short}{cut}\""
            )
        }
    }
}

/// What adds each entry of the quest `tag`, one line each, for `edit quest
/// show`.
pub(crate) fn described(state: &EditorState, tag: &str) -> Vec<String> {
    let Some((_, _, entries)) = level_edit::journal::quest(&state::journal(state), tag) else {
        return Vec::new();
    };
    let found = find(state);
    let mut lines = Vec::new();
    for (id, _, _) in entries {
        let adders = (found.get(&(tag.to_ascii_lowercase(), id)).cloned()).unwrap_or_else(Vec::new);
        if adders.is_empty() {
            lines.push(format!(
                "entry {id}: nothing in the project adds this entry yet."
            ));
        }
        for adder in adders {
            lines.push(format!("entry {id}: {}", sentences(&adder)));
        }
    }
    lines
}
