//! `kjk dialogue` (I18): a module's conversations.
//!
//! - `show <game> <module> [dialog]`: the conversations the module's
//!   level carries (its objects', and those its scripts name), each with
//!   its counts and the scripts it names that the library lacks; with a
//!   name, that tree: each node once, its text from `dialog.tlk`, links
//!   with their conditions, each spoken reply only the host takes in co-op
//!   marked `host` (the story's, `dialogue::story`).
//! - `run <game> <module>`: the module stepped headless for one player,
//!   its conversations answered through the player's command (`--picks`),
//!   the transcript written to the run folder; `--talk TAG` has the
//!   player use an object (a terminal) at the start, `--skill` and
//!   `--item` set the player's ranks and the party's items by hand, and
//!   `--made` makes the player as `kjk play --made` does.
//! - `sweep k1 <module>`: every conversation the module's runs start,
//!   filmed (`scene_sweep`).

use std::collections::BTreeSet;
use std::process::ExitCode;

use fmt_odyssey::dlg::{Dlg, Link};
use serde_json::json;
use sim::script::dialogue::Line;
use vfs::{Discovery, Game};

use crate::lifecycle::Installer;
use crate::transcript::{Talk, text, transcript};

fn condition(link: &Link) -> String {
    let mut parts = Vec::new();
    for (call, not) in [(&link.active, link.not), (&link.active2, link.not2)] {
        if !call.script.is_empty() {
            parts.push(format!("{}{}", if not { "not " } else { "" }, call.script));
        }
    }
    match parts.len() {
        0 => String::new(),
        _ => format!(
            " if {}",
            parts.join(if link.logic == 1 { " or " } else { " and " })
        ),
    }
}

struct Tree<'a> {
    talk: Talk<'a>,
    library: &'a sim::script::Library,
    dlg: &'a Dlg,
    seen: BTreeSet<(bool, u32)>,
    lines: Vec<String>,
}

impl Tree<'_> {
    fn node(&mut self, entry: bool, index: u32, depth: usize) {
        let pad = "  ".repeat(depth);
        let list = if entry {
            &self.dlg.entries
        } else {
            &self.dlg.replies
        };
        let name = format!("{}{index}", if entry { "E" } else { "R" });
        let Some(node) = list.get(index as usize) else {
            self.lines.push(format!("{pad}{name}: past the end"));
            return;
        };
        let mut facts = Vec::new();
        let call = |call: &fmt_odyssey::dlg::Call| match call.ints {
            [0, 0, 0, 0, 0] => call.script.clone(),
            ints => format!("{}{ints:?}", call.script),
        };
        let (script, script2) = (call(&node.script), call(&node.script2));
        for (label, value) in [
            ("speaker", &node.speaker),
            ("script", &script),
            ("script2", &script2),
            ("sound", &node.sound),
            ("vo", &node.vo),
            ("quest", &node.quest),
        ] {
            if !value.is_empty() {
                facts.push(format!("{label} {value}"));
            }
        }
        if let Some(delay) = node.delay {
            facts.push(format!("delay {delay}"));
        }
        if node.camera_angle != 0 {
            facts.push(format!("angle {}", node.camera_angle));
        }
        if node.camera_animation != 0 {
            facts.push(format!("camera animation {}", node.camera_animation));
        }
        if node.camera_id != 0 {
            facts.push(format!("camera {}", node.camera_id));
        }
        if node.wait_flags != 0 {
            facts.push(format!("wait {}", node.wait_flags));
        }
        if !entry && !node.is_silent() && sim::script::dialogue::story(self.library, self.dlg, index) {
            facts.push("host".into());
        }
        let facts = if facts.is_empty() {
            String::new()
        } else {
            format!(" [{}]", facts.join(", "))
        };
        self.lines
            .push(format!("{pad}{name}{facts} {:?}", text(self.talk, node)));
        if !self.seen.insert((entry, index)) {
            return;
        }
        for link in &node.links {
            let child = format!("{}{}", if entry { "R" } else { "E" }, link.index);
            if link.is_child {
                self.lines.push(format!(
                    "{pad}  -> {child}{} (shown elsewhere)",
                    condition(link)
                ));
            } else {
                self.lines
                    .push(format!("{pad}  -> {child}{}", condition(link)));
                self.node(!entry, link.index, depth + 2);
            }
        }
    }
}

pub fn show(discovery: &Discovery, game: &str, module: &str, dialog: Option<&str>) -> ExitCode {
    let run = || -> Result<serde_json::Value, String> {
        let game = Game::from_key(game).ok_or(format!("{game:?}: use k1 or tsl"))?;
        let anims = std::sync::Arc::new(sim::motion::AnimTable::stand_in());
        let installer = Installer::new(discovery, game, anims)?;
        let prepared = installer.prepare_one(module, None)?;
        let library = prepared
            .plan
            .level
            .scripts()
            .cloned()
            .ok_or("the module has no scripts (no nwscript.nss)")?;
        let Some(name) = dialog else {
            let mut rows = Vec::new();
            for (name, dlg) in library.dialogues() {
                let missing: Vec<&str> = dlg
                    .scripts()
                    .into_iter()
                    .filter(|script| library.program(script).is_none())
                    .collect();
                outln!(
                    "{name}: {} entries, {} replies, {} starts, {} scripts{}",
                    dlg.entries.len(),
                    dlg.replies.len(),
                    dlg.starts.len(),
                    dlg.scripts().len(),
                    if missing.is_empty() {
                        String::new()
                    } else {
                        format!(", not in the library: {}", missing.join(" "))
                    }
                );
                rows.push(json!({ "name": name, "entries": dlg.entries.len(),
                    "replies": dlg.replies.len(), "missing": missing }));
            }
            return Ok(json!({ "module": module, "dialogues": rows }));
        };
        let dlg = library
            .dialogue(name)
            .ok_or(format!("{name}: not among the level's conversations"))?;
        let talk = |strref| installer.text(strref);
        let mut tree = Tree {
            talk: &talk,
            library: &library,
            dlg,
            seen: BTreeSet::new(),
            lines: Vec::new(),
        };
        for (i, start) in dlg.starts.iter().enumerate() {
            tree.lines
                .push(format!("start {i} -> E{}{}", start.index, condition(start)));
            tree.node(true, start.index, 1);
        }
        for line in &tree.lines {
            outln!("{line}");
        }
        Ok(json!({ "module": module, "dialog": name, "lines": tree.lines.len() }))
    };
    match run() {
        Ok(facts) => {
            crate::runtime::write_facts(facts);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

pub(crate) use crate::talk_run::{Played, Run, Step, play, play_watched};
use crate::talk_run::carried;

/// A module installed for one player and stepped headless, its scripts and
/// conversations running (`play`). The transcript goes to `transcript.txt`
/// in the run folder.
pub fn run(discovery: &Discovery, game: &str, module: &str, run: &Run) -> ExitCode {
    let go = || -> Result<(serde_json::Value, Vec<String>), String> {
        let game = Game::from_key(game).ok_or(format!("{game:?}: use k1 or tsl"))?;
        let anims = std::sync::Arc::new(sim::motion::AnimTable::stand_in());
        let installer = Installer::new(discovery, game, anims)?;
        let Played {
            world,
            library,
            names,
            ..
        } = play(&installer, module, run)?;
        let report = world.script_report();
        let talk = |strref| installer.text(strref);
        let mut tokens = std::collections::BTreeMap::new();
        let mut lines: Vec<String> = report
            .talk
            .iter()
            .flat_map(|line| transcript(&talk, &library, &names, (&mut tokens, None), line))
            .collect();
        lines.extend(carried(&world, &run.items));
        let entries = report
            .talk
            .iter()
            .filter(|line| matches!(line, Line::Entry { .. }))
            .count();
        let facts = json!({
            "module": module,
            "seconds": run.seconds,
            "entries": entries,
            "faults": report.faults,
            "missing": report.missing,
            "gaps": report.gaps,
            "journal": world.script_state().journal,
            "party_xp": world.script_state().xp(sim::inventory::PARTY),
        });
        Ok((facts, lines))
    };
    match go() {
        Ok((facts, lines)) => {
            for line in &lines {
                outln!("{line}");
            }
            if let Some(folder) = crate::runtime::write_facts(facts) {
                let path = folder.join("transcript.txt");
                let mut body = lines.join("\n");
                body.push('\n');
                match std::fs::write(&path, body) {
                    Ok(()) => eprintln!("transcript: {}", path.display()),
                    Err(error) => eprintln!("{}: {error}", path.display()),
                }
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
