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

use std::collections::{BTreeMap, BTreeSet};
use std::process::ExitCode;

use fmt_odyssey::dlg::{Dlg, Link};
use serde_json::json;
use sim::script::dialogue::{Line, Pick};
use vfs::{Discovery, Game};

use crate::by_hand::{give_items, set_globals, talk_to, with_skills};
use crate::lifecycle::Installer;
use crate::transcript::{Talk, speakers, text, transcript};

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
        for (label, value) in [
            ("speaker", &node.speaker),
            ("script", &node.script.script),
            ("script2", &node.script2.script),
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

/// How a headless run answers: the replies to take, by their place among
/// those offered (from 1), in order; the first offered when the list runs
/// out.
#[derive(Default)]
pub struct Run {
    pub seconds: u64,
    pub picks: Vec<u8>,
    pub arrive: Option<String>,
    pub globals: Vec<(String, i32)>,
    /// The tag of an object the player uses at the start.
    pub talk: Option<String>,
    pub skills: Vec<(usize, u8)>,
    pub items: Vec<(String, u16)>,
    /// The player made by quick creation (class, portrait, name), as `kjk
    /// play --made`; else the seat's default persona.
    pub made: Option<(u8, String, String)>,
    /// Companions' templates joined to the party at the start, as `kjk play
    /// --party` (`by_hand::party`).
    pub party: Vec<String>,
    /// How long (ms) the player reads the replies offered before it
    /// answers; 0 answers in the step they are offered.
    pub read_ms: u64,
}

/// A run stepped (`play`): its world at the end, the level's library, each
/// object's name for the transcript, the way the player looked from the
/// start (degrees from +X, the arrival's), each answer it made (the ms its
/// command first carried it, `TickRate::time_ms` of the step before, and
/// the reply's place among those offered, from 1), and the conversations
/// that held the world (S1: while it was held, a party one or one the
/// player stood in).
pub(crate) struct Played {
    pub world: sim::SimWorld,
    pub library: std::sync::Arc<sim::script::Library>,
    pub names: BTreeMap<u32, String>,
    pub yaw: f32,
    pub answers: Vec<(u64, u8)>,
    pub holding: BTreeSet<u32>,
}

/// `module` installed by `installer` for one player arriving as `run`
/// says, set up by hand as `kjk play` sets its stage up (globals, items, the
/// object talked to, the persona, the party), and stepped `run.seconds`: seat 0's
/// command looks the arrival's way and carries the answers, each offer
/// answered by `run.picks` once its line has played and `run.read_ms`
/// more have passed.
pub(crate) fn play(installer: &Installer, module: &str, run: &Run) -> Result<Played, String> {
    play_watched(installer, module, run, &mut |_| true)
}

/// A step of a run (`play_watched`): its sim ms at the end and its length
/// (ms), the snapshot it made, the world it left and the level.
pub(crate) struct Step<'a> {
    pub now: u64,
    pub msec: u32,
    pub snapshot: &'a sim::Snapshot,
    pub world: &'a sim::SimWorld,
    pub level: &'a sim::Level,
}

/// `play`, `watch` shown each step as it is made; the run ends early once
/// `watch` answers false.
pub(crate) fn play_watched(
    installer: &Installer,
    module: &str,
    run: &Run,
    watch: &mut dyn FnMut(&Step) -> bool,
) -> Result<Played, String> {
    let request = installer.request(module, run.arrive.as_deref(), 1);
    let prepared = installer.prepare(&request, false)?;
    let level = prepared.plan.level.clone();
    let library = level
        .scripts()
        .cloned()
        .ok_or("the module has no scripts (no nwscript.nss)")?;
    let (spot, _) = level
        .module()
        .ok_or("a plan has its module")?
        .entry_for(run.arrive.as_deref());
    let config = sim::WorldConfig {
        seed: 1,
        seats: 1,
        wanderers: 0,
        spawn: spot.position,
        yaw: spot.yaw,
        sheet: with_skills(sim::combat::Sheet::dev_soldier(), &run.skills),
        guests: sim::Guests::Avatars,
    };
    // Committed as a host installs it: the arrival honoured.
    let mut world = sim::SimWorld::new(config, sim::Level::arena()).map_err(|e| e.to_string())?;
    prepared
        .plan
        .commit(&mut world)
        .map_err(|e| e.to_string())?;
    set_globals(&mut world, &run.globals);
    give_items(&mut world, &run.items)?;
    if let Some(tag) = &run.talk {
        talk_to(&mut world, tag)?;
    }
    if let Ok(resolver) = installer.base_resolver() {
        let made = run
            .made
            .as_ref()
            .map(|(class, portrait, name)| crate::persona::Made::Quick {
                class: *class,
                portrait,
                name,
            });
        crate::persona::start(&mut world, resolver, 1, made)?;
    }
    crate::by_hand::party(&mut world, &run.party, Some(installer))?;
    let rate = sim::TickRate::default();
    let mut picks = run.picks.iter();
    let mut answered = BTreeSet::new();
    let mut names: BTreeMap<u32, String> = BTreeMap::new();
    let (mut answers, mut holding) = (Vec::new(), BTreeSet::new());
    let mut reading: Option<((u32, u64), u64, u8)> = None;
    // Seat 0's command carries the answers, as a window's does.
    let looking = sim::moves::view_angles(0.0, spot.yaw);
    let mut reply = 0;
    let first = world.snapshot().tick + 1;
    for tick in first..first + run.seconds * u64::from(sim::TICK_HZ) {
        let mut input = sim::TickInput::default();
        let command = sim::Command {
            angles: looking,
            reply,
            ..sim::Command::default()
        };
        input.insert(sim::Seat(0), command);
        let snapshot = sim::step(
            &mut world,
            tick,
            &input,
            rate.msec(tick),
            sim::StepReason::Authority,
        );
        if names.is_empty() {
            names = speakers(&snapshot, &level);
        }
        if snapshot.holds_world() {
            let bodies = snapshot.seat_bodies();
            holding.extend(
                snapshot
                    .talk
                    .conversations
                    .iter()
                    .filter(|c| c.party || bodies.iter().any(|body| c.holds(*body)))
                    .map(|c| c.id),
            );
        }
        let offered = |c: &sim::script::dialogue::Conversation| {
            c.until_ms == u64::MAX && !c.replies.is_empty()
        };
        let now = rate.time_ms(tick);
        let fresh = snapshot.talk.conversations.iter().find_map(|c| {
            (offered(c) && answered.insert((c.id, c.shown_ms)))
                .then_some(((c.id, c.shown_ms), c.replies.len()))
        });
        if let Some((line, count)) = fresh {
            let nth = picks.next().map_or(0, |n| n.saturating_sub(1));
            reading = Some((
                line,
                now + run.read_ms,
                nth.min(count.saturating_sub(1) as u8),
            ));
        }
        // The answer once the replies have been read, if they still wait.
        if let Some((line, _, nth)) = reading.filter(|&(_, due, _)| due <= now) {
            reading = None;
            let waits = snapshot
                .talk
                .conversations
                .iter()
                .any(|c| (c.id, c.shown_ms) == line && offered(c));
            if waits {
                reply = sim::script::dialogue::answer(reply, Pick::Reply(nth));
                answers.push((now, nth + 1));
            }
        }
        let step = Step {
            now,
            msec: rate.msec(tick),
            snapshot: &snapshot,
            world: &world,
            level: &level,
        };
        if !watch(&step) {
            break;
        }
    }
    Ok(Played {
        world,
        library,
        names,
        yaw: spot.yaw,
        answers,
        holding,
    })
}

/// The custom tokens scripts set, and what the party carries of
/// `templates`, for the transcript's end.
fn carried(world: &sim::SimWorld, templates: &[(String, u16)]) -> Vec<String> {
    let state = world.script_state();
    let tokens: Vec<String> = state
        .talk
        .tokens
        .iter()
        .map(|(n, value)| format!("<CUSTOM{n}>={value}"))
        .collect();
    let tokens = (!tokens.is_empty()).then(|| format!("-- tokens {}", tokens.join(" ")));
    tokens
        .into_iter()
        .chain(templates.iter().map(|(template, _)| {
            let count: u32 = state
                .items
                .values()
                .filter(|item| item.holder == sim::inventory::PARTY && &item.template == template)
                .map(|item| u32::from(item.stack))
                .sum();
            format!("-- the party carries {count} {template}")
        }))
        .collect()
}

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
            .flat_map(|line| transcript(&talk, &library, &names, &mut tokens, line))
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
