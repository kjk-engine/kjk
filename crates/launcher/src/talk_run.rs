//! A module stepped headless for one player with its conversations
//! running, the player answering each offer (`kjk dialogue run`, the
//! conversation sweeps), and `kjk try-conversation`, which the level
//! editor's conversation window runs. It is in every build of kjk, the
//! player's too.

use std::collections::{BTreeMap, BTreeSet};
use std::process::ExitCode;

use sim::script::dialogue::Pick;
use vfs::{Discovery, Game};

use crate::by_hand::{give_items, set_globals, talk_to, with_skills};
use crate::lifecycle::Installer;
use kjk_game::transcript::{speakers, transcript};

/// How a headless run answers: the replies to take, by their place among
/// those offered (from 1), in order; the first offered when the list runs
/// out.
#[derive(Default)]
pub struct Run {
    pub seconds: u64,
    pub picks: Vec<u8>,
    pub arrive: Option<String>,
    pub globals: Vec<(String, i32)>,
    /// The object the player uses at the start (`--talk`, `--dialog`).
    pub talk: Option<crate::by_hand::Talk>,
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
    /// A conversation added to the module's library before it runs, by
    /// name, where nothing names it yet (`try_conversation`).
    pub added: Option<(String, fmt_odyssey::dlg::Dlg)>,
}

/// A run stepped (`play`): its world at the end, the level's library, each
/// object's name for the transcript, the way the player looked from the
/// start (degrees from +X, the arrival's), each answer it made (the ms its
/// command first carried it, `TickRate::time_ms` of the step before, and
/// the reply's place among those offered, from 1), and the conversations
/// that held the world (S1: while it was held, a party one or one the
/// player stood in).
// The player's build reads only the world and the names.
#[cfg_attr(not(feature = "probes"), allow(dead_code))]
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
// The player's build reads only the world and the names.
#[cfg_attr(not(feature = "probes"), allow(dead_code))]
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
    let mut prepared = installer.prepare(&request, false)?;
    if let Some((name, dlg)) = &run.added {
        let level = prepared.plan.level.clone();
        let library = level
            .scripts()
            .ok_or("the module has no scripts (no nwscript.nss)")?;
        let library = (**library).clone().with_dialogue(name, dlg.clone());
        prepared.plan.level = level.with_scripts(library);
    }
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
    // A companion talked to (`--party`) is there once it joins; any other
    // is talked to first, as before.
    let talk_later = !run.party.is_empty();
    if let Some(talk) = run.talk.as_ref().filter(|_| !talk_later) {
        talk_to(&mut world, talk, None)?;
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
        crate::persona::start(&mut world, resolver, 1, &[], made)?;
    }
    crate::by_hand::party(&mut world, &run.party, Some(installer))?;
    if let Some(talk) = run.talk.as_ref().filter(|_| talk_later) {
        talk_to(&mut world, talk, None)?;
    }
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
#[cfg(feature = "probes")]
pub(crate) fn carried(world: &sim::SimWorld, templates: &[(String, u16)]) -> Vec<String> {
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
                .filter(|item| item.holder == sim::inventory::PARTY && item.is(template))
                .map(|item| u32::from(item.stack))
                .sum();
            format!("-- the party carries {count} {template}")
        }))
        .collect()
}

/// The most calls of scripts a conversation's trial lists.
const MOST_CALLS: usize = 150;

/// `kjk try-conversation`: the conversation `dialog` of `module`, the
/// player talking to the object tagged `on`, its replies picked by `picks`
/// (by their place among those offered, from 1; the first when the list
/// runs out), stepped `seconds`. Prints the transcript: each line, each
/// offer and each pick, then what the conversation's own scripts called,
/// and the scripts' faults.
pub fn try_conversation(
    discovery: &Discovery,
    game: &str,
    module: &str,
    (dialog, on): (&str, &str),
    picks: Vec<u8>,
    seconds: u64,
) -> ExitCode {
    let go = || -> Result<Vec<String>, String> {
        let game = Game::from_key(game)
            .filter(|game| *game != Game::JediAcademy)
            .ok_or(format!("{game:?}: use k1 or tsl"))?;
        let anims = std::sync::Arc::new(sim::motion::AnimTable::stand_in());
        let installer = Installer::new(discovery, game, anims)?;
        // Read through the install's layers, the project's folder first.
        let dialog = dialog.to_ascii_lowercase();
        let bytes = installer
            .resolver(module)?
            .read(&dialog, fmt_odyssey::ResType::DLG)
            .ok_or(format!("{dialog}.dlg is not found"))?;
        let dlg = fmt_odyssey::dlg::Dlg::parse(&bytes)
            .map_err(|error| format!("{dialog}.dlg: {error}"))?;
        // The scripts the conversation itself runs: its nodes' actions,
        // its links' conditions, and the ones at its end.
        let scripts: std::collections::BTreeSet<String> = (dlg.entries.iter())
            .chain(&dlg.replies)
            .flat_map(|node| {
                [&node.script.script, &node.script2.script]
                    .into_iter()
                    .chain(node.links.iter().map(|link| &link.active.script))
            })
            .chain(dlg.starts.iter().map(|link| &link.active.script))
            .chain([&dlg.on_end, &dlg.on_abort])
            .filter(|script| !script.is_empty())
            .map(|script| script.to_ascii_lowercase())
            .collect();
        let run = Run {
            seconds,
            picks,
            talk: Some(crate::by_hand::Talk {
                tag: on.to_string(),
                dialog: Some(dialog.clone()),
            }),
            read_ms: 500,
            added: Some((dialog.clone(), dlg)),
            ..Run::default()
        };
        let Played {
            world,
            library,
            names,
            ..
        } = play(&installer, module, &run)?;
        let report = world.script_report();
        let talk = |strref| installer.text(strref);
        let mut tokens = BTreeMap::new();
        let mut lines: Vec<String> = report
            .talk
            .iter()
            .flat_map(|line| transcript(&talk, &library, &names, (&mut tokens, None), line))
            .collect();
        if lines.is_empty() {
            lines.push(format!(
                "{dialog} said nothing: no line's condition held, or {on} did not talk"
            ));
        }
        // What the conversation's own scripts called: a door a line's
        // script unlocks or opens shows here, and nowhere in the lines
        // themselves. The trace marks each script's run, and its deferred
        // code's (`script_try::run_of`), and the calls that follow are
        // that run's.
        let mut called = Vec::new();
        let mut own = false;
        for line in report.trace_lines() {
            if let Some(script) = crate::script_try::run_of(&line) {
                own = scripts.contains(&script.to_ascii_lowercase());
            }
            if own {
                called.push(line.trim_end().to_string());
            }
        }
        if !called.is_empty() {
            lines.push("-- what the conversation's scripts called:".to_string());
            let kept = called.len().min(MOST_CALLS);
            lines.extend(called.iter().take(kept).cloned());
            if called.len() > kept {
                lines.push(format!("… {} more calls …", called.len() - kept));
            }
        }
        for fault in &report.faults {
            lines.push(format!("fault: {fault}"));
        }
        for name in report.missing.keys() {
            lines.push(format!("missing script: {name}"));
        }
        Ok(lines)
    };
    match go() {
        Ok(lines) => {
            for line in &lines {
                outln!("{line}");
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
