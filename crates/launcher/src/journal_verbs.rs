//! `kjk script journal <game> [module]`: the journal checked end to end. The
//! install's `global.jrl` is read into the module's library (its quests and
//! their entries, a talk-table line each); a script made here (`probe`) runs
//! as the player in the module (the Endar Spire by default) on the first
//! quest the journal lists and another:
//!
//! 1. The first quest set to its second entry, then to its first (refused:
//!    a quest does not go back), then to its first allowing it; the other
//!    quest set and removed; `GetJournalEntry` read after each.
//! 2. A save carries the journal.

use std::process::ExitCode;

use serde_json::json;
use vfs::Discovery;

use crate::probe::{Call, Probe, int, text};
use crate::scenes::Report;
use crate::script_verbs::{catalog, open};

/// `AddJournalQuestEntry(tag, entry, lower)`.
fn add(tag: &str, entry: u32, lower: bool) -> Vec<Call> {
    vec![
        int(i32::from(lower)),
        int(entry as i32),
        text(tag),
        Call::Routine("AddJournalQuestEntry", 3),
    ]
}

/// `SetGlobalNumber(name, GetJournalEntry(tag))`.
fn read(name: &str, tag: &str) -> Vec<Call> {
    vec![
        text(tag),
        Call::Routine("GetJournalEntry", 1),
        text(name),
        Call::Routine("SetGlobalNumber", 2),
    ]
}

pub fn journal(discovery: &Discovery, game: &str, module: &str) -> ExitCode {
    let mut report = Report::default();
    let run = |report: &mut Report| -> Result<serde_json::Value, String> {
        let (game, _, base) = open(discovery, game)?;
        let routines = catalog(&base)?;
        let talk = base.talk.get().ok_or("no dialog.tlk")?;
        let mut quests = None;
        let mut probe = Probe::new(discovery, game, module, |level| {
            let own = level.scripts().ok_or("no script library")?.quests().clone();
            let mut tags = own.0.iter().filter(|(_, quest)| quest.entries.len() >= 2);
            let (first, quest) = tags.next().ok_or("no quest with two entries")?;
            let (other, _) = tags.next().ok_or("no second quest")?;
            let (lower, higher) = (quest.entries[0].id, quest.entries[1].id);
            let (lower, higher) = (lower.min(higher), lower.max(higher));
            let calls = [
                add(first, higher, false),
                read("kjk_higher", first),
                add(first, lower, false),
                read("kjk_refused", first),
                add(first, lower, true),
                read("kjk_lowered", first),
                add(other, 1, false),
                vec![text(other), Call::Routine("RemoveJournalQuestEntry", 1)],
                read("kjk_removed", other),
            ];
            quests = Some((own.clone(), first.clone(), other.clone(), lower, higher));
            let mut scripts = std::collections::BTreeMap::new();
            scripts.insert(
                "kjk_journal".to_string(),
                crate::probe::program(&routines, &calls)?,
            );
            Ok(
                sim::script::Library::new(routines, scripts, sim::script::Talents::default())
                    .with_quests(own),
            )
        })?;
        let (quests, first, other, lower, higher) = quests.ok_or("no quests read")?;
        let line =
            |strref: u32| -> String { fmt_odyssey::code_page(talk.text(strref).unwrap_or(&[])) };
        let entries: usize = quests.0.values().map(|quest| quest.entries.len()).sum();
        let ends = quests
            .0
            .values()
            .filter(|quest| quest.entries.iter().any(|entry| entry.end))
            .count();
        let quest = quests.get(&first).ok_or("the first quest")?;
        report.check(
            !quests.is_empty() && ends > 0,
            format!(
                "global.jrl: {} quests, {entries} entries, {ends} with an ending entry",
                quests.len()
            ),
        );
        let name = line(quest.name);
        let text = line(quest.entry(higher).ok_or("its entry")?.text);
        report.check(
            !name.is_empty() && !text.is_empty(),
            format!("{first}: {name:?}, entry {higher}: {text:?}"),
        );

        probe.queue("kjk_journal", probe.me);
        probe.step(2);
        let state = probe.world.script_state().clone();
        let number = |name: &str| sim::script::global(&state.numbers, name).copied().unwrap_or(-1);
        report.check(
            number("kjk_higher") == higher as i32,
            format!("{first} set to {higher}: {}", number("kjk_higher")),
        );
        report.check(
            number("kjk_refused") == higher as i32,
            format!(
                "{first} set back to {lower} refused: {}",
                number("kjk_refused")
            ),
        );
        report.check(
            number("kjk_lowered") == lower as i32,
            format!(
                "{first} set back to {lower}, allowed: {}",
                number("kjk_lowered")
            ),
        );
        report.check(
            number("kjk_removed") == 0 && state.journal == vec![(first.clone(), lower as i32)],
            format!("{other} set and removed: the journal {:?}", state.journal),
        );
        let save = sim::Save {
            hz: sim::TICK_HZ,
            level: probe.world.level().id(),
            snapshot: probe.world.snapshot(),
            script: state.clone(),
        };
        let loaded = sim::Save::decode(&save.encode()).map_err(|e| e.to_string())?;
        report.check(
            loaded.script.journal == state.journal,
            format!("a save carries the journal ({:?})", loaded.script.journal),
        );
        let faults = probe.faults();
        report.check(
            faults.is_empty(),
            format!("the scripts ran without faults ({faults:?})"),
        );
        Ok(json!({
            "module": module,
            "quests": quests.len(),
            "entries": entries,
            "quest": first,
            "name": name,
            "journal": state.journal,
        }))
    };
    match run(&mut report) {
        Ok(facts) => report.finish(facts),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
