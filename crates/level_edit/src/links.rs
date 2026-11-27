//! The links between a level's objects that their scripts make.
//!
//! An object does something to another one through a script. A terminal's
//! conversation runs a script, and the script finds the turrets by their
//! tag. Nothing else ties the terminal to the turrets, so nothing in the
//! level's object list says that the one acts on the others. This module
//! finds those links, for the editor to show (`docs/LEVEL-EDIT.md`, "What
//! an object's scripts name").
//!
//! A link is found in three steps.
//!
//! - **The scripts an object runs** (`Run`): one for each of its events
//!   (`of_events`), and those of its conversation (`of_conversation`): the
//!   script of each line and reply, each condition, and the two scripts run
//!   as the conversation ends.
//! - **The strings each compiled script holds** (`strings`). A script
//!   finds an object by its tag, and the tag is a string of the script.
//! - **The objects of the level that carry such a string as their tag**
//!   (`links`), compared without case, as the games compare tags.
//!
//! A script that finds its objects another way shows no link: one that
//! walks every creature of the area, or one that builds a tag from parts.
//! A script that another script starts is not followed either.

use std::collections::BTreeMap;
use std::sync::Arc;

use fmt_odyssey::dlg::{Dlg, Link as TalkLink};
use fmt_odyssey::gff::Gff;
use fmt_odyssey::ncs::{Const, Ncs, Op};

/// A script an object runs, and where it hangs on the object.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Run {
    /// An event (`OnUsed`), or a place in the object's conversation (`the
    /// conversation kjd1_infoterm, reply 0`).
    pub at: String,
    pub script: String,
}

/// The scripts of an object's events, from what says what it does: its
/// template, or a door's or a waypoint's own entry.
pub fn of_events(gff: &Gff) -> Vec<Run> {
    crate::template::scripts(gff)
        .into_iter()
        .filter(|(_, script)| !script.is_empty())
        .map(|(at, script)| Run { at, script })
        .collect()
}

/// The scripts of the conversation `name`. A line or a reply is numbered
/// as the conversation window numbers it. A condition is named by the line
/// or the reply it decides.
pub fn of_conversation(name: &str, dlg: &Dlg) -> Vec<Run> {
    fn add(runs: &mut Vec<Run>, name: &str, place: &str, script: &str) {
        if !script.is_empty() {
            runs.push(Run {
                at: format!("the conversation {name}, {place}"),
                script: script.to_ascii_lowercase(),
            });
        }
    }
    fn conditions(runs: &mut Vec<Run>, name: &str, links: &[TalkLink], side: &str) {
        for link in links {
            let place = format!("the condition of {side} {}", link.index);
            for call in [&link.active, &link.active2] {
                add(runs, name, &place, &call.script);
            }
        }
    }
    let mut runs: Vec<Run> = Vec::new();
    conditions(&mut runs, name, &dlg.starts, "line");
    for (nodes, side, other) in [
        (&dlg.entries, "line", "reply"),
        (&dlg.replies, "reply", "line"),
    ] {
        for (index, node) in nodes.iter().enumerate() {
            for call in [&node.script, &node.script2] {
                add(&mut runs, name, &format!("{side} {index}"), &call.script);
            }
            conditions(&mut runs, name, &node.links, other);
        }
    }
    add(&mut runs, name, "when it ends", &dlg.on_end);
    add(&mut runs, name, "when it is cut short", &dlg.on_abort);
    runs.sort();
    runs.dedup();
    runs
}

/// The strings a compiled script holds, each once, in the order they first
/// stand in it. A script that does not read holds none.
pub fn strings(compiled: &[u8]) -> Vec<String> {
    let Ok(ncs) = Ncs::parse(compiled) else {
        return Vec::new();
    };
    let mut found: Vec<String> = Vec::new();
    for instruction in &ncs.code {
        if let Op::Const(Const::String(bytes)) = &instruction.op {
            // The games' code page: every byte one character.
            let string: String = bytes.iter().map(|&byte| char::from(byte)).collect();
            if !string.is_empty() && !found.contains(&string) {
                found.push(string);
            }
        }
    }
    found
}

/// One object of the level: its tag and the scripts it runs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Object {
    pub tag: String,
    pub runs: Vec<Run>,
}

/// The object `from` runs a script that names the object `to` by its tag.
/// Both are places in the list of objects `links` was given.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Link {
    pub from: usize,
    pub to: usize,
    pub run: Run,
}

/// Every link between `objects`. `strings_of` gives the strings a script
/// holds by the script's name. An object's script that names the object's
/// own tag makes no link to that object.
pub fn links(objects: &[Object], strings_of: &mut dyn FnMut(&str) -> Arc<[String]>) -> Vec<Link> {
    let mut tagged: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (at, object) in objects.iter().enumerate() {
        if !object.tag.is_empty() {
            tagged
                .entry(object.tag.to_ascii_lowercase())
                .or_default()
                .push(at);
        }
    }
    let mut found: Vec<Link> = Vec::new();
    for (from, object) in objects.iter().enumerate() {
        for run in &object.runs {
            for string in strings_of(&run.script).iter() {
                let Some(named) = tagged.get(&string.to_ascii_lowercase()) else {
                    continue;
                };
                for &to in named.iter().filter(|&&to| to != from) {
                    found.push(Link {
                        from,
                        to,
                        run: run.clone(),
                    });
                }
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

/// The tags among `tags` that `strings` name, each once, as the level
/// writes them: what one script names of a level.
pub fn named<'t>(strings: &[String], tags: impl Iterator<Item = &'t str>) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for tag in tags {
        let names = strings
            .iter()
            .any(|string| string.eq_ignore_ascii_case(tag));
        let had = found.iter().any(|had| had.eq_ignore_ascii_case(tag));
        if !tag.is_empty() && names && !had {
            found.push(tag.to_string());
        }
    }
    found
}
