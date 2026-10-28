//! The journal (I19, `docs/WORLD-STATE.md`): KOTOR's quests (`global.jrl`)
//! by tag, each with its entries, in the library; the entry each quest has
//! reached is the scripts' (`State::journal`, a save carries it), set by
//! `AddJournalQuestEntry` and by a conversation's node as it plays (its
//! `Quest` and `QuestEntry`). One journal for the session: the story's.

use std::collections::BTreeMap;

use script_ncs::vm::Value;

use super::dialogue::Line;
use super::engine::{Bound, Engine, int, text};
use super::key;
use super::state::State;
use crate::snapshot::StatusKind;
use crate::wire::{DecodeError, Reader, Writer};

/// A quest's entry: its number (what scripts set), whether it ends the
/// quest, its text (a talk-table line) and the share of the quest's plot
/// XP reaching it gives.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QuestEntry {
    pub id: u32,
    pub end: bool,
    pub text: u32,
    pub xp: f32,
}

/// A quest of `global.jrl`: its name (a talk-table line), priority (0 the
/// highest), planet (`planetary.2da` row, −1 none), its `plot.2da` row (−1
/// none) and its entries in file order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Quest {
    pub name: u32,
    pub priority: u32,
    pub planet: i32,
    pub plot: i32,
    pub entries: Vec<QuestEntry>,
}

impl Quest {
    pub fn entry(&self, id: u32) -> Option<&QuestEntry> {
        self.entries.iter().find(|entry| entry.id == id)
    }
}

/// Every quest, by lowercase tag.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Quests(pub BTreeMap<String, Quest>);

impl Quests {
    pub fn get(&self, tag: &str) -> Option<&Quest> {
        self.0.get(&key(tag))
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// What a library's digest takes of them.
    pub(crate) fn digest_into(&self, out: &mut Writer) {
        out.u32(self.0.len() as u32);
        for (tag, quest) in &self.0 {
            out.string(tag);
            out.u32(quest.name);
            out.u32(quest.priority);
            out.u32(quest.planet as u32);
            out.u32(quest.plot as u32);
            out.u32(quest.entries.len() as u32);
            for entry in &quest.entries {
                out.u32(entry.id);
                out.u8(u8::from(entry.end));
                out.u32(entry.text);
                out.f32(entry.xp);
            }
        }
    }
}

/// A journal (quest, entry reached; the latest updated last) in a save or
/// a snapshot.
pub(crate) fn encode_journal(out: &mut Writer, journal: &[(String, i32)]) {
    out.u32(journal.len() as u32);
    for (quest, entry) in journal {
        out.string(quest);
        out.u32(*entry as u32);
    }
}

pub(crate) fn decode_journal(input: &mut Reader) -> Result<Vec<(String, i32)>, DecodeError> {
    (0..input.u32()?)
        .map(|_| Ok((input.string()?, input.u32()? as i32)))
        .collect()
}

impl State {
    /// The entry quest `tag` has reached, 0 for none (`GetJournalEntry`).
    pub fn quest(&self, tag: &str) -> i32 {
        let tag = key(tag);
        self.journal
            .iter()
            .find(|(of, _)| *of == tag)
            .map_or(0, |(_, entry)| *entry)
    }

    /// Quest `tag` reaches entry `entry`, unless it is past it already and
    /// `lower` does not allow going back (`AddJournalQuestEntry`'s
    /// `bAllowOverrideHigher`); the quest moves to the end, the latest
    /// updated. Whether it changed.
    pub(crate) fn set_quest(&mut self, tag: &str, entry: i32, lower: bool) -> bool {
        let tag = key(tag);
        let now = self.quest(&tag);
        if tag.is_empty() || now == entry || (now > entry && !lower) {
            return false;
        }
        self.journal.retain(|(of, _)| *of != tag);
        self.journal.push((tag, entry));
        true
    }
}

impl Engine<'_> {
    /// The journal's routines.
    pub(super) fn journal_call(&mut self, bound: Bound, args: &[Value]) -> Value {
        let tag = text(args, 0);
        match bound {
            Bound::AddJournalQuestEntry => {
                let entry = int(args, 1);
                if self.state.set_quest(tag, entry, int(args, 2) != 0) {
                    self.report.talk.push(Line::Journal {
                        ms: self.view.now_ms,
                        quest: key(tag),
                        entry,
                    });
                    self.out.statuses.push(StatusKind::Journal);
                    let xp = self.library.quest_xp(tag, entry);
                    self.out.story_xp(self.state, xp);
                }
                Value::Int(0)
            }
            Bound::RemoveJournalQuestEntry => {
                let tag = key(tag);
                self.state.journal.retain(|(of, _)| *of != tag);
                Value::Int(0)
            }
            _ => Value::Int(self.state.quest(tag)),
        }
    }
}
