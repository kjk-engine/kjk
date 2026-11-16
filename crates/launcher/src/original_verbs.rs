//! `kjk original` and `kjk report original` (developer tooling). They start
//! the original KOTOR 1 or KOTOR 2 the way `kjk play` starts kjk, to see
//! whether a bug seen in kjk is the original game's too. Both take `kjk
//! play`'s start options (`cli_play::Start`). The game is the one whose
//! modules hold the start's module.
//!
//! The start's state is set by compiled scripts (`script_ncs::asm`),
//! written into the `override` folder of a copy of the install under the
//! names of the game's own scripts (`Hook`). The install itself is never
//! written. The copy is made once in `original/` of kjk's own folder, and
//! made again when the install changes. Each run refills the copy's
//! `override` folder from the install's own, so a mod's files stay, and
//! writes the hooks over it.
//!
//! The game reaches the start's module in one of two ways.
//!
//! - By default, the game goes there from its main menu without a click.
//!   A library loaded into the game (the `original_hook` crate) picks the
//!   module in the developers' hidden warp list. The player is the game's
//!   default one, named "test", not a character made by the player.
//! - With `--created`, the player makes a character once. That first run's
//!   new game autosaves as it arrives in the game's first module, and the
//!   save is kept (`StartSave`). Later runs put it back for the player to
//!   load, and the first module's hook moves on to the start's module.
//!
//! The Mac ports and the Windows builds are started (`Port`). The Mac
//! KOTOR 1 keeps its settings in the user's own folder, so a run changes two
//! of them and puts them back when the game quits. The others keep theirs
//! in the install's folder, so a run changes the copy's.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use fmt_odyssey::ResType;
use fmt_odyssey::gff::Gff;
use kjk_game::by_hand::{Spot, Talk};
use script_ncs::Catalog;
use script_ncs::asm::{self, Expr, action, call, constant, not, text, when};
use vfs::resolver::Resolver;
use vfs::{DirIndex, Discovery, Game, Install};

use crate::cli_play::Start;

/// The options the original game has no script for, refused by name.
fn refuse_untranslatable(start: &Start) -> Result<(), String> {
    let refused = [
        (
            start.made.is_some(),
            "--made: the original game's character is the start save's",
        ),
        (
            !start.followers.is_empty(),
            "--follow: the original game's party takes companions only, with --party",
        ),
        (
            !start.by_hand.skills.is_empty(),
            "--skill: the games' scripts cannot set a skill's rank",
        ),
    ];
    match refused.iter().find(|(given, _)| *given) {
        Some((_, why)) => Err(format!("not for the original game: {why}")),
        None => Ok(()),
    }
}

/// The start the options `given` make when laid over a report's start. An
/// option given wins over the report's. The globals given come after the
/// report's, so a later one wins in the game. A party given replaces the
/// report's. A module given other than the report's drops the report's spot
/// and conversation, which belong to the report's module.
fn laid_over(report: Start, given: Start) -> Start {
    let mut start = report;
    let other_module = given.stage.is_some() && given.stage != start.stage;
    if other_module {
        start.at = None;
        start.by_hand.talk = None;
        start.by_hand.dialog = None;
    }
    start.stage = given.stage.or(start.stage);
    start.arrive = given.arrive.or(start.arrive);
    start.at = given.at.or(start.at);
    start.gold = given.gold.or(start.gold);
    start.alignment = given.alignment.or(start.alignment);
    start.xp = given.xp.or(start.xp);
    if given.by_hand.talk.is_some() {
        start.by_hand.talk = given.by_hand.talk;
        start.by_hand.dialog = given.by_hand.dialog;
    }
    start.globals.extend(given.globals);
    start.scripts.extend(given.scripts);
    start.by_hand.items.extend(given.by_hand.items);
    if !given.by_hand.party.is_empty() {
        start.by_hand.party = given.by_hand.party;
    }
    start
}

/// `start`'s options as `kjk play` and `kjk original` take them.
fn flags(start: &Start) -> Vec<String> {
    let mut flags = Vec::new();
    let mut flag = |name: &str, value: String| flags.extend([format!("--{name}"), value]);
    if let Some(stage) = &start.stage {
        flag("stage", stage.clone());
    }
    if let Some(arrive) = &start.arrive {
        flag("arrive", arrive.clone());
    }
    if let Some(Spot {
        soles: [x, y, z],
        facing,
    }) = start.at
    {
        flag("at", format!("{x:.2},{y:.2},{z:.2},{facing:.0}"));
    }
    for template in &start.by_hand.party {
        flag("party", template.clone());
    }
    for (name, value) in &start.globals {
        flag("global", format!("{name}={value}"));
    }
    for (template, count) in &start.by_hand.items {
        flag("item", format!("{template}={count}"));
    }
    if let Some(gold) = start.gold {
        flag("gold", gold.to_string());
    }
    if let Some(xp) = start.xp {
        flag("xp", xp.to_string());
    }
    if let Some(alignment) = start.alignment {
        flag("alignment", alignment.to_string());
    }
    for script in &start.scripts {
        flag("run", script.clone());
    }
    if let Some(talk) = &start.by_hand.talk {
        flag("talk", talk.clone());
    }
    if let Some(dialog) = &start.by_hand.dialog {
        flag("dialog", dialog.clone());
    }
    flags
}

fn pc() -> Expr {
    call("GetFirstPC", [])
}

/// `statements` run `seconds` later.
fn later(seconds: f32, statements: Vec<Expr>) -> Expr {
    call("DelayCommand", [Expr::Float(seconds), action(statements)])
}

/// Each of `statements` run `seconds` later than it would have run.
fn each_later(seconds: f32, statements: Vec<Expr>) -> Vec<Expr> {
    statements
        .into_iter()
        .map(|statement| later(seconds, vec![statement]))
        .collect()
}

/// A line in the game's message log, under Feedback, for following a run.
fn say(message: &str) -> Expr {
    call("SendMessageToPC", [pc(), text(&format!("kjk: {message}"))])
}

/// The object `talk` names starts its conversation with the player. The
/// party's actions are cleared first. A companion's queue can hold an
/// action that never ends, such as following the leader, and a conversation
/// queued behind it never starts. Nothing starts when the object is
/// missing, the player is in combat, or the player is already talking, and
/// the message log says which.
fn start_talk(talk: &Talk) -> Vec<Expr> {
    let owner = || call("GetObjectByTag", [text(&talk.tag)]);
    let talking = || call("GetIsInConversation", [pc()]);
    let fighting = || call("GetIsInCombat", [pc()]);
    let clear = |index: i32| {
        call(
            "AssignCommand",
            [
                call("GetPartyMemberByIndex", [Expr::Int(index)]),
                action([call("ClearAllActions", [])]),
            ],
        )
    };
    let what = talk.dialog.as_deref().unwrap_or("its conversation");
    let start = vec![
        say(&format!("{} starts {what}", talk.tag)),
        clear(0),
        clear(1),
        clear(2),
        call(
            "AssignCommand",
            [
                owner(),
                action([call(
                    "ActionStartConversation",
                    [pc(), text(talk.dialog.as_deref().unwrap_or(""))],
                )]),
            ],
        ),
        later(
            1.0,
            vec![
                when(talking(), [say("the conversation runs")]),
                when(not(talking()), [say("no conversation a second later")]),
            ],
        ),
    ];
    let ready = when(
        call("GetIsObjectValid", [owner()]),
        [when(not(fighting()), [when(not(talking()), start)])],
    );
    vec![
        when(
            not(call("GetIsObjectValid", [owner()])),
            [say(&format!("no {}", talk.tag))],
        ),
        when(fighting(), [say("the player is in combat")]),
        when(talking(), [say("the player is already in a conversation")]),
        ready,
    ]
}

/// A script of the game's replaced by ours: `script`, as `module`'s data
/// has it. An area's "on enter" script runs for each creature that enters,
/// so ours acts only when the player does.
struct Hook {
    script: String,
    module: String,
    /// Whether the game's own script still runs first, for every creature
    /// entering. It is copied into `override` as `kjk_orig1` and on, and run
    /// with `ExecuteScript`.
    keeps_original: bool,
    for_player: Vec<Expr>,
}

/// What differs between the two games for a run.
struct Original {
    game: Game,
    /// The game's id on Steam, written beside its program so it starts
    /// outside Steam.
    steam_id: &'static str,
    /// The module a new game starts in, and its area's "on enter" script,
    /// for `--created`. Its hook autosaves before anything changes
    /// (`StartSave`), then sets the start's state there or leaves for the
    /// start's module. It leaves out the game's own script, which would
    /// start the opening scene on every load of the start save.
    start_module: &'static str,
    start_script: &'static str,
    /// The settings file, in the install's data folder (the Mac KOTOR 1
    /// keeps its own in the user's folder instead).
    ini: &'static str,
    /// The program, in the Windows install's folder.
    windows_program: &'static str,
    /// The program, in the Mac app's `Contents/MacOS`. It is not the app's
    /// own launcher, which hands the game to Steam and starts Steam's
    /// install, not the copy.
    mac_program: &'static str,
}

const KOTOR_1: Original = Original {
    game: Game::Kotor1,
    steam_id: "32370",
    start_module: "end_m01aa",
    start_script: "k_pend_area01",
    ini: "swkotor.ini",
    windows_program: "swkotor.exe",
    mac_program: "KOTOR_Exe",
};

const KOTOR_2: Original = Original {
    game: Game::Kotor2,
    steam_id: "208580",
    start_module: "001ebo",
    start_script: "a_bet3m4",
    ini: "swkotor2.ini",
    windows_program: "swkotor2.exe",
    mac_program: "KOTOR2sub",
};

/// The statements that set `start`'s state once the player has arrived.
/// The player is put on the spot at once. The globals, gold, XP, alignment
/// and items follow half a second later. The party joins at 1 second, the
/// scripts run at 1.5 seconds, and the conversation starts at 3 seconds.
fn arrival(start: &Start, data: &Resolver) -> Result<Vec<Expr>, String> {
    let mut out = Vec::new();
    if let Some(Spot {
        soles: [x, y, z],
        facing,
    }) = start.at
    {
        let spot = call(
            "Location",
            [
                call("Vector", [Expr::Float(x), Expr::Float(y), Expr::Float(z)]),
                Expr::Float(facing),
            ],
        );
        out.push(call(
            "AssignCommand",
            [pc(), action([call("JumpToLocation", [spot])])],
        ));
    }
    let mut state = Vec::new();
    let kinds = global_kinds(data)?;
    for (name, value) in &start.globals {
        let routine = match kinds.iter().find(|(known, _)| known.eq_ignore_ascii_case(name)) {
            Some((_, GlobalKind::Boolean)) => "SetGlobalBoolean",
            Some((_, GlobalKind::Number)) => "SetGlobalNumber",
            None => return Err(format!("--global {name}: not in globalcat.2da")),
        };
        state.push(call(routine, [text(name), Expr::Int(*value)]));
    }
    if let Some(gold) = start.gold {
        // `--gold` sets the party's gold. It does not add to it.
        state.push(call(
            "TakeGoldFromCreature",
            [call("GetGold", [pc()]), pc(), constant("TRUE")],
        ));
        state.push(call("GiveGoldToCreature", [pc(), Expr::Int(gold)]));
    }
    if let Some(xp) = start.xp {
        state.push(call("SetXP", [pc(), Expr::Int(xp)]));
    }
    if let Some(alignment) = start.alignment {
        state.push(call(
            "SetGoodEvilValue",
            [pc(), Expr::Int(i32::from(alignment))],
        ));
    }
    for (template, count) in &start.by_hand.items {
        state.push(call(
            "CreateItemOnObject",
            [text(template), pc(), Expr::Int(i32::from(*count))],
        ));
    }
    if !state.is_empty() {
        out.push(later(0.5, state));
    }
    let mut party = Vec::new();
    for template in &start.by_hand.party {
        let npc = Expr::Int(npc_number(data, template)?);
        party.push(call(
            "AddAvailableNPCByTemplate",
            [npc.clone(), text(template)],
        ));
        party.push(call(
            "AddPartyMember",
            [
                npc.clone(),
                call("SpawnAvailableNPC", [npc, call("GetLocation", [pc()])]),
            ],
        ));
    }
    if !party.is_empty() {
        out.push(later(1.0, party));
    }
    let scripts: Vec<Expr> = start
        .scripts
        .iter()
        .map(|script| call("ExecuteScript", [text(script), pc()]))
        .collect();
    if !scripts.is_empty() {
        out.push(later(1.5, scripts));
    }
    let talk = kjk_game::by_hand::talk(start.by_hand.talk.clone(), start.by_hand.dialog.clone());
    if let Some(talk) = talk {
        out.push(later(3.0, start_talk(&talk)));
    }
    Ok(out)
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum GlobalKind {
    Boolean,
    Number,
}

/// The globals `globalcat.2da` declares, by name, with their kinds.
fn global_kinds(data: &Resolver) -> Result<Vec<(String, GlobalKind)>, String> {
    let table = data
        .two_da("globalcat")
        .ok_or("globalcat.2da: not in the install")?;
    Ok((0..table.cells.len())
        .filter_map(|row| {
            let name = table.text(row, "name")?;
            let kind = match table.text(row, "type")?.to_ascii_lowercase().as_str() {
                "boolean" => GlobalKind::Boolean,
                "number" => GlobalKind::Number,
                _ => return None,
            };
            Some((name, kind))
        })
        .collect())
}

/// A companion's number in the party table: the `npc.2da` row whose tag is
/// its template's tag. For example, `p_carth` is tagged `Carth`, row 2.
fn npc_number(data: &Resolver, template: &str) -> Result<i32, String> {
    let bytes = data
        .read(template, ResType::UTC)
        .ok_or(format!("--party {template}: no such creature template"))?;
    let utc = Gff::parse(&bytes).map_err(|error| format!("{template}.utc: {error}"))?;
    let tag = utc
        .root
        .bytes("Tag")
        .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
        .ok_or(format!("{template}.utc: no tag"))?;
    let table = data.two_da("npc").ok_or("npc.2da: not in the install")?;
    (0..table.cells.len())
        .find(|&row| {
            table
                .text(row, "tag")
                .is_some_and(|known| known.eq_ignore_ascii_case(&tag))
        })
        .map(|row| row as i32)
        .ok_or(format!(
            "--party {template}: its tag {tag} is no companion's in npc.2da"
        ))
}

/// The install, opened once for a run: its game, folder index and shared
/// layers.
struct Opened {
    original: &'static Original,
    index: DirIndex,
    base: Resolver,
}

impl Opened {
    /// The install of the game whose modules hold `module`. KOTOR 1's and
    /// KOTOR 2's module names never meet.
    fn holding(discovery: &Discovery, module: &str) -> Result<Opened, String> {
        for original in [&KOTOR_1, &KOTOR_2] {
            if discovery.get(original.game).is_none() {
                continue;
            }
            let (_, index, base) = crate::script_verbs::open(discovery, original.game.key())?;
            if base.with_module(&index, module).is_ok() {
                return Ok(Opened {
                    original,
                    index,
                    base,
                });
            }
        }
        Err(format!(
            "{module}: no module of the KOTOR 1 or KOTOR 2 found here"
        ))
    }

    /// `module`'s data: the shared layers and the module's own archives.
    fn module(&self, module: &str) -> Result<Resolver, String> {
        self.base
            .with_module(&self.index, module)
            .map_err(|error| format!("{module}: {error}"))
    }
}

/// The module `start` begins in.
fn module_of(start: &Start) -> Result<&str, String> {
    start
        .stage
        .as_deref()
        .ok_or("--stage: the module to start in, for example tar_m02ac".into())
}

/// The scripts a run writes into `override`, by name, as file bytes. The
/// start's module's hook sets the start's state as the player arrives.
/// With `created`, the game's first module's hook autosaves and goes to the
/// start's module.
fn scripts(
    start: &Start,
    catalog: &Catalog,
    opened: &Opened,
    created: bool,
) -> Result<Vec<(String, Vec<u8>)>, String> {
    let module = module_of(start)?;
    let data = opened.module(module)?;
    let mut on_arrival = arrival(start, &data)?;
    let mut hooks = Vec::new();
    let original = opened.original;
    if !created {
        // The warp list starts the module at its entry. A door or waypoint
        // to arrive at is reached by a jump, unless `--at` places the
        // player.
        if let (Some(arrive), None) = (&start.arrive, start.at) {
            let there = call("GetObjectByTag", [text(arrive)]);
            on_arrival.insert(
                0,
                call(
                    "AssignCommand",
                    [pc(), action([call("JumpToObject", [there])])],
                ),
            );
        }
        hooks.push(Hook {
            script: entry_script(&data, module)?,
            module: module.to_string(),
            keeps_original: true,
            for_player: each_later(0.5, on_arrival),
        });
    } else if module.eq_ignore_ascii_case(original.start_module) {
        if start.arrive.is_some() {
            return Err(format!(
                "--arrive: every run with --created starts at {}'s entry",
                original.start_module
            ));
        }
        let mut at_start = vec![later(0.5, vec![call("DoSinglePlayerAutoSave", [])])];
        at_start.extend(each_later(0.5, on_arrival));
        hooks.push(Hook {
            script: original.start_script.into(),
            module: original.start_module.into(),
            keeps_original: false,
            for_player: at_start,
        });
    } else {
        let arrive = start.arrive.as_deref().unwrap_or("");
        hooks.push(Hook {
            script: original.start_script.into(),
            module: original.start_module.into(),
            keeps_original: false,
            for_player: vec![
                later(0.5, vec![call("DoSinglePlayerAutoSave", [])]),
                later(
                    1.5,
                    vec![call("StartNewModule", [text(module), text(arrive)])],
                ),
            ],
        });
        hooks.push(Hook {
            script: entry_script(&data, module)?,
            module: module.to_string(),
            keeps_original: true,
            for_player: each_later(0.5, on_arrival),
        });
    }
    let mut files = Vec::new();
    let mut originals = 0;
    for hook in hooks {
        let mut statements = Vec::new();
        if hook.keeps_original {
            originals += 1;
            let name = format!("kjk_orig{originals}");
            let own = opened
                .module(&hook.module)?
                .read(&hook.script, ResType::NCS)
                .ok_or(format!("{}: no {}.ncs", hook.module, hook.script))?;
            files.push((name.clone(), own));
            statements.push(call("ExecuteScript", [text(&name), Expr::Me]));
        }
        let player = call("GetIsPC", [call("GetEnteringObject", [])]);
        statements.push(when(player, hook.for_player));
        files.push((hook.script, asm::script(catalog, &statements)?.write()));
    }
    Ok(files)
}

/// The "on enter" script of `module`'s area, from the module's `data`. A
/// KOTOR module has one area.
fn entry_script(data: &Resolver, module: &str) -> Result<String, String> {
    let gff = |resref: &str, restype: ResType| {
        let bytes = data
            .read(resref, restype)
            .ok_or(format!("{module}: no {resref}"))?;
        Gff::parse(&bytes).map_err(|error| format!("{module}: {resref}: {error}"))
    };
    let area = gff("module", ResType::IFO)?
        .root
        .resref("Mod_Entry_Area")
        .ok_or(format!("{module}: module.ifo names no area"))?;
    gff(&area, ResType::ARE)?
        .root
        .resref("OnEnter")
        .filter(|script| !script.is_empty())
        .ok_or(format!(
            "{module}: its area {area} has no \"on enter\" script to hook"
        ))
}

/// The start a bug report gives. The module and the conversation come from
/// its breadcrumbs. The conversation is started by the speaker of the line
/// it had reached. The player's spot, the story's globals, the party, the
/// gold and the XP come from its save of the moment (`now.kjkv`), when this
/// build reads it. Otherwise the reason is printed and they are left out.
fn start_of_report(report: &Path, opened: &Opened) -> Result<Start, String> {
    let report = crate::report_verbs::Report::open(report)?;
    let crumbs = report.text("breadcrumbs.txt").unwrap_or(String::new());
    let crumb = |key: &str| crumb(&crumbs, key);
    let module = module_of_report(&crumbs)?;
    let data = opened.module(&module)?;
    let mut start = Start {
        stage: Some(module.clone()),
        ..Start::default()
    };
    // The breadcrumb reads like `conversation banter node 89`, where 89 is
    // the entry shown.
    if let Some(talk) = crumb("conversation") {
        let mut words = talk.split_whitespace();
        let dialog = words.next().map(str::to_string);
        let node = words
            .skip_while(|word| *word != "node")
            .nth(1)
            .and_then(|node| node.parse::<usize>().ok());
        if let (Some(dialog), Some(node)) = (dialog, node) {
            let speaker = data
                .read(&dialog, ResType::DLG)
                .and_then(|bytes| fmt_odyssey::dlg::Dlg::parse(&bytes).ok())
                .and_then(|dlg| dlg.entries.get(node).map(|entry| entry.speaker.clone()))
                .filter(|speaker| !speaker.is_empty());
            match speaker {
                Some(speaker) => {
                    start.by_hand.talk = Some(speaker);
                    start.by_hand.dialog = Some(dialog);
                }
                None => outln!(
                    "The report's conversation {dialog} names no speaker for node {node}, so it is not started."
                ),
            }
        }
    }
    match saved_state(&report, &data) {
        Ok(saved) => Ok(laid_over(start, saved)),
        Err(error) => {
            outln!(
                "The report's save gives no spot, globals, party, gold or XP ({error})."
            );
            Ok(start)
        }
    }
}

/// The value of the breadcrumb line that starts with `key`, unless it is
/// empty or `none`.
fn crumb<'a>(crumbs: &'a str, key: &str) -> Option<&'a str> {
    crumbs
        .lines()
        .find_map(|line| line.strip_prefix(key)?.strip_prefix(' '))
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "none")
}

/// The module a report's breadcrumbs name.
fn module_of_report(crumbs: &str) -> Result<String, String> {
    crumb(crumbs, "module")
        .and_then(|value| value.split_whitespace().next())
        .map(str::to_string)
        .ok_or("the report's breadcrumbs name no module".into())
}

/// What the report's save of the moment gives a start. A global the
/// game's `globalcat.2da` does not declare is kjk's own, and is left out.
fn saved_state(report: &crate::report_verbs::Report, data: &Resolver) -> Result<Start, String> {
    let bytes = report.bytes("now.kjkv").ok_or("no now.kjkv")?;
    let save = sim::Save::decode(bytes).map_err(|error| format!("now.kjkv: {error}"))?;
    let body = save
        .snapshot
        .body(sim::Seat(0))
        .ok_or("now.kjkv: the first player has no body")?;
    let [x, y, z] = body.position;
    let facing = body.player.map(|motion| motion.view[1]).unwrap_or(0.0);
    let state = &save.script;
    let kinds = global_kinds(data)?;
    let declared = |name: &String| {
        kinds
            .iter()
            .any(|(known, _)| known.eq_ignore_ascii_case(name))
    };
    let all: Vec<(String, i32)> = state
        .numbers
        .iter()
        .map(|(name, value)| (name.clone(), *value))
        .chain(
            state
                .booleans
                .iter()
                .map(|(name, value)| (name.clone(), i32::from(*value))),
        )
        .collect();
    let (globals, own): (Vec<_>, Vec<_>) = all.into_iter().partition(|(name, _)| declared(name));
    if !own.is_empty() {
        outln!(
            "{} of the save's globals are kjk's own, not the game's, and are left out.",
            own.len()
        );
    }
    let party = state
        .party
        .members
        .iter()
        .filter_map(|npc| state.party.available.get(npc))
        .map(|npc| npc.template.clone())
        .collect();
    Ok(Start {
        at: Some(Spot {
            soles: [x, y, z + sim::motion::SOLE_Z * collision::UNIT],
            facing,
        }),
        globals,
        gold: Some(state.gold),
        xp: state.xp.get(&sim::inventory::PARTY).copied(),
        by_hand: Box::new(crate::cli_play::ByHand {
            party,
            ..crate::cli_play::ByHand::default()
        }),
        ..Start::default()
    })
}

/// Where the install's data is inside its folder, which is also where the
/// copy's is inside the copy.
fn data_inside(install: &Install) -> Result<&Path, String> {
    install
        .data_root
        .strip_prefix(&install.root)
        .map_err(|_| "the install's data is not inside its folder".to_string())
}

/// The player the warp list starts a game with: the character file
/// `test.bic` in the data folder's `localvault`. KOTOR 2 ships one, a level
/// 12 player. KOTOR 1 ships none, and its game waits for one forever on the
/// loading screen. The copy is given one made from Carth's creature
/// template, as the player: its tag and conversation cleared, and the three
/// fields a character file has and a template does not.
fn give_test_player(base: &Resolver, copy_data: &Path) -> Result<(), String> {
    use fmt_odyssey::gff::{GffField, GffValue};
    let vault = copy_data.join("localvault");
    let held = std::fs::read_dir(&vault).ok().is_some_and(|entries| {
        entries
            .flatten()
            .any(|entry| entry.file_name().eq_ignore_ascii_case("test.bic"))
    });
    if held {
        return Ok(());
    }
    let bytes = base
        .read("p_carth", ResType::UTC)
        .ok_or("p_carth.utc: not in the install, so the game has no player to start with")?;
    let mut player = Gff::parse(&bytes).map_err(|error| format!("p_carth.utc: {error}"))?;
    player.file_type = *b"BIC ";
    let fields = &mut player.root.fields;
    let mut set = |label: &str, value: GffValue| {
        fields.retain(|field| field.label != label.as_bytes());
        fields.push(GffField {
            label: label.as_bytes().to_vec(),
            value,
        });
    };
    set("IsPC", GffValue::Byte(1));
    set("Tag", GffValue::String(Vec::new()));
    set("Conversation", GffValue::ResRef(Vec::new()));
    set("Experience", GffValue::Dword(0));
    set("LvlStatList", GffValue::List(Vec::new()));
    set("PlayerCreated", GffValue::Int(1));
    std::fs::create_dir_all(&vault).map_err(|error| format!("{}: {error}", vault.display()))?;
    write(&vault.join("test.bic"), &player.write())?;
    outln!("{} is the player the game starts with.", vault.join("test.bic").display());
    Ok(())
}

/// The kind of build a run starts.
enum Port {
    /// Aspyr's Mac port. Its data is inside a `.app`, which this is,
    /// relative to the install's folder. Its program runs under Rosetta.
    Mac { app: PathBuf },
    /// A Windows build, its program in the install's folder.
    Windows,
}

impl Port {
    /// The install's port, refused before anything is copied when it is
    /// neither.
    fn of(install: &Install, original: &Original) -> Result<Port, String> {
        if cfg!(windows) && install.root.join(original.windows_program).is_file() {
            return Ok(Port::Windows);
        }
        let app = data_inside(install)?
            .ancestors()
            .find(|path| path.extension().is_some_and(|ext| ext == "app"));
        match app {
            Some(app) if cfg!(target_os = "macos") => Ok(Port::Mac {
                app: app.to_path_buf(),
            }),
            _ => Err(format!(
                "{}: kjk original starts the Mac ports and the Windows builds, and this install is neither",
                install.root.display()
            )),
        }
    }
}

/// The settings a run changes and puts back, and where the game keeps its
/// saves.
struct Settings {
    saves: Option<PathBuf>,
    /// The user's own files that are written back when the run ends.
    _restored: Vec<Restore>,
}

/// Turns the cheat console on and the game into a window, and finds its
/// saves. The Mac KOTOR 1 keeps its settings in the user's folder. The
/// others keep theirs in the data folder, which is the copy's.
fn settings(port: &Port, original: &Original, copy_data: &Path) -> Result<Settings, String> {
    let read = |path: &Path| std::fs::read_to_string(path).unwrap_or(String::new());
    if let (Port::Mac { .. }, Game::Kotor1) = (port, original.game) {
        let settings = std::env::home_dir()
            .ok_or("no home folder")?
            .join("Library/Application Support/Knights of the Old Republic");
        let ini = settings.join(original.ini);
        let aspyr = settings.join("Aspyr").join("Data.json");
        let restored = vec![Restore::take(ini.clone()), Restore::take(aspyr.clone())];
        std::fs::create_dir_all(settings.join("Aspyr")).map_err(|error| error.to_string())?;
        write(&ini, with_cheats(&read(&ini)).as_bytes())?;
        write(&aspyr, windowed(&read(&aspyr))?.as_bytes())?;
        outln!(
            "{} and {} are put back when the game quits.",
            ini.display(),
            aspyr.display()
        );
        return Ok(Settings {
            saves: Some(settings.join("saves")),
            _restored: restored,
        });
    }
    let ini = copy_data.join(original.ini);
    let mut text = with_cheats(&read(&ini));
    for section in ["[Graphics Options]", "[Display Options]"] {
        text = with_setting(&text, section, "FullScreen", "0");
    }
    // The Windows KOTOR 1 keeps to full screen unless this is set too.
    text = with_setting(&text, "[Graphics Options]", "AllowWindowedMode", "1");
    write(&ini, text.as_bytes())?;
    let saves = match port {
        Port::Windows => Some(copy_data.join("saves")),
        // The Mac KOTOR 2 keeps its saves in `cloudsaves/<Steam user>`,
        // beside the app.
        Port::Mac { .. } => std::fs::read_dir(copy_data.join("../../../cloudsaves"))
            .ok()
            .and_then(|entries| {
                entries
                    .flatten()
                    .map(|entry| entry.path())
                    .find(|path| path.is_dir())
            }),
    };
    Ok(Settings {
        saves,
        _restored: Vec::new(),
    })
}

/// The library `kjk original` loads into the game, and on Windows the
/// program that loads it (the `original_hook` crate).
struct HookFiles {
    library: PathBuf,
    inject: Option<PathBuf>,
}

/// The hook's files for `port`. They are taken from beside kjk's own
/// program when they are there, as a package would hold them. Otherwise
/// they are built from the workspace kjk was built from, for the game's
/// architecture: x86_64 macOS, or 32-bit Windows.
fn hook_files(port: &Port) -> Result<HookFiles, String> {
    let (target, library, inject) = match port {
        Port::Mac { .. } => ("x86_64-apple-darwin", "libkjk_original.dylib", None),
        Port::Windows if cfg!(target_env = "gnu") => {
            ("i686-pc-windows-gnu", "kjk_original.dll", Some("kjk-inject.exe"))
        }
        Port::Windows => ("i686-pc-windows-msvc", "kjk_original.dll", Some("kjk-inject.exe")),
    };
    let found = |folder: &Path| {
        let library = folder.join(library);
        let inject = inject.map(|inject| folder.join(inject));
        (library.is_file() && inject.as_ref().is_none_or(|inject| inject.is_file()))
            .then_some(HookFiles { library, inject })
    };
    let beside = std::env::current_exe()
        .ok()
        .and_then(|program| program.parent().map(Path::to_path_buf));
    if let Some(files) = beside.as_deref().and_then(found) {
        return Ok(files);
    }
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    outln!("Building the hook for {target} (cargo build -p original_hook).");
    let built = Command::new(std::env::var("CARGO").unwrap_or("cargo".into()))
        .args(["build", "-p", "original_hook", "--release", "--target", target])
        .current_dir(&workspace)
        .status()
        .map_err(|error| format!("cargo: {error}"))?;
    if !built.success() {
        return Err(format!(
            "the hook did not build for {target}. Its target may be missing: rustup target add {target}"
        ));
    }
    let target_folder = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or(workspace.join("target"));
    found(&target_folder.join(target).join("release"))
        .ok_or(format!("the hook's files are not in {}", target_folder.display()))
}

/// Starts the game in `copy` with the cheat console on, in a window, and
/// waits until it quits. Without `created`, the hook takes the game from its
/// main menu to `module`. With it, the start save is put back for the player
/// to load, or made by the first run's new game and kept.
#[allow(clippy::too_many_arguments)]
fn launch(
    port: &Port,
    original: &Original,
    copy: &Path,
    data: &Path,
    module: &str,
    kept: &StartSave,
    hook: Option<&HookFiles>,
    log: &Path,
) -> Result<(), String> {
    let copy_data = copy.join(data);
    let program_folder = match port {
        Port::Mac { app } => copy.join(app).join("Contents/MacOS"),
        Port::Windows => copy.to_path_buf(),
    };
    // The game runs without Steam when its Steam id is beside it.
    write(
        &program_folder.join("steam_appid.txt"),
        original.steam_id.as_bytes(),
    )?;
    let settings = settings(port, original, &copy_data)?;
    let created = hook.is_none();
    let loaded = match (&settings.saves, created) {
        (Some(saves), true) => kept.put_back(saves)?,
        _ => None,
    };
    if created {
        let first = original.start_module;
        match &loaded {
            Some(name) => outln!(
                "In the game, choose Load Game, then the save \"{name}\". It is {first}'s start, kept from the first run."
            ),
            None => outln!(
                "In the game, start a new game, once. {first}'s start is autosaved as it loads, and kept for later runs."
            ),
        }
    } else {
        outln!("The game goes from its main menu to {module} by itself.");
    }
    let program = match port {
        Port::Mac { .. } => program_folder.join(original.mac_program),
        Port::Windows => program_folder.join(original.windows_program),
    };
    let mut command = match (port, hook) {
        (Port::Windows, Some(HookFiles { library, inject: Some(inject) })) => {
            let mut command = Command::new(inject);
            command.arg(&program).arg(library);
            command
        }
        _ => Command::new(&program),
    };
    command.current_dir(&copy_data);
    if let Some(hook) = hook {
        command
            .env("KJK_ORIGINAL_MODULE", module)
            .env("KJK_ORIGINAL_LOG", log);
        if let Port::Mac { .. } = port {
            command.env("DYLD_INSERT_LIBRARIES", &hook.library);
        }
    }
    let _ = std::fs::remove_file(log);
    outln!(
        "Starting {}. To end the run, quit from the game's menu and confirm.",
        program.display()
    );
    let status = command
        .status()
        .map_err(|error| format!("{}: {error}", program.display()))?;
    if hook.is_some() {
        let said = std::fs::read_to_string(log).unwrap_or(String::new());
        for line in said.lines() {
            outln!("hook: {line}");
        }
    }
    if let (Some(saves), true, None) = (&settings.saves, created, &loaded) {
        match kept.keep(saves)? {
            Some(name) => outln!("The save \"{name}\" is kept as the start for later runs."),
            None => outln!("No autosave was found in {}, so nothing is kept.", saves.display()),
        }
    }
    if !status.success() {
        return Err(format!("{}: {status}", program.display()));
    }
    Ok(())
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|error| format!("{}: {error}", path.display()))
}

/// Removes the folder at `path`, if there is one.
fn remove_folder(path: &Path) -> Result<(), String> {
    match path.exists() {
        true => std::fs::remove_dir_all(path).map_err(|error| format!("{}: {error}", path.display())),
        false => Ok(()),
    }
}

/// The save later runs with `--created` start from: the autosave a new game
/// makes as it arrives in the game's first module (`Original::start_module`),
/// before the start's state is set. It is kept
/// in kjk's folder, because the game writes its autosave again on every
/// arrival, and put back before each run. Saves are folders in the game's
/// `saves` folder.
struct StartSave {
    kept: PathBuf,
}

impl StartSave {
    /// The autosave's folder in `saves`, found by its name.
    fn autosave(saves: &Path) -> Option<PathBuf> {
        std::fs::read_dir(saves).ok()?.flatten().find_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            (entry.path().is_dir() && name.contains("autosave")).then(|| entry.path())
        })
    }

    /// Copies the kept save into `saves` under its own name, replacing what
    /// is there, and gives its name. It gives none when nothing is kept yet.
    fn put_back(&self, saves: &Path) -> Result<Option<String>, String> {
        let Some(save) = std::fs::read_dir(&self.kept)
            .ok()
            .and_then(|mut entries| entries.find_map(|entry| entry.ok()))
        else {
            return Ok(None);
        };
        let target = saves.join(save.file_name());
        remove_folder(&target)?;
        copy_tree(&save.path(), &target)?;
        Ok(Some(save.file_name().to_string_lossy().into_owned()))
    }

    /// Keeps the autosave in `saves`, and gives its name. It gives none when
    /// there is no autosave.
    fn keep(&self, saves: &Path) -> Result<Option<String>, String> {
        let Some(autosave) = StartSave::autosave(saves) else {
            return Ok(None);
        };
        let name = autosave.file_name().ok_or("an autosave without a name")?;
        remove_folder(&self.kept)?;
        copy_tree(&autosave, &self.kept.join(name))?;
        Ok(Some(name.to_string_lossy().into_owned()))
    }

    /// Forgets the kept save, so the next run starts a new game again.
    fn forget(&self) -> Result<(), String> {
        remove_folder(&self.kept)
    }
}

/// The game's settings file with `EnableCheats=1` under `[Game Options]`.
fn with_cheats(ini: &str) -> String {
    with_setting(ini, "[Game Options]", "EnableCheats", "1")
}

/// The settings file `ini` with `key=value` in `section`, in place of the
/// key's line there, or added to the section, or added in a new section at
/// the end.
fn with_setting(ini: &str, section: &str, key: &str, value: &str) -> String {
    let setting = format!("{key}={value}");
    let mut out = Vec::new();
    let mut current = "";
    let mut written = false;
    for line in ini.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            if current == section && !written {
                out.push(setting.clone());
                written = true;
            }
            current = trimmed;
        }
        if current == section && trimmed.starts_with(&format!("{key}=")) {
            if !written {
                out.push(setting.clone());
                written = true;
            }
            continue;
        }
        out.push(line.to_string());
    }
    if !written {
        if current != section {
            out.push(section.to_string());
        }
        out.push(setting);
    }
    out.join("\r\n") + "\r\n"
}

/// Aspyr's `Data.json` with the game in a window.
fn windowed(json: &str) -> Result<String, String> {
    let mut data: serde_json::Value = if json.trim().is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_str(json).map_err(|error| format!("Aspyr/Data.json: {error}"))?
    };
    let root = data
        .as_object_mut()
        .ok_or("Aspyr/Data.json: not an object")?;
    let prefs = root
        .entry("Prefs")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or("Aspyr/Data.json: Prefs is not an object")?;
    prefs.insert("ASL_DisplayFullScreen".into(), false.into());
    serde_json::to_string_pretty(&data).map_err(|error| error.to_string())
}

/// A settings file as it was before a run, written back when dropped.
struct Restore {
    path: PathBuf,
    bytes: Option<Vec<u8>>,
}

impl Restore {
    fn take(path: PathBuf) -> Restore {
        let bytes = std::fs::read(&path).ok();
        Restore { path, bytes }
    }
}

impl Drop for Restore {
    fn drop(&mut self) {
        let result = match &self.bytes {
            Some(bytes) => std::fs::write(&self.path, bytes),
            None => std::fs::remove_file(&self.path).or(Ok(())),
        };
        if let Err(error) = result {
            eprintln!("{}: not put back: {error}", self.path.display());
        }
    }
}

/// kjk's folder for the original games: their copies and start saves.
fn original_folder() -> Result<PathBuf, String> {
    Ok(store::kjk_folder(|name| std::env::var(name).ok())
        .ok_or("no folder for kjk's files")?
        .join("original"))
}

/// The file in a copy that names the install it was made from.
const COPY_SOURCE: &str = "kjk-source.txt";

/// The copy of `install`'s folder that kjk runs, made once in `base`. It is
/// made again when it does not name `install` as its source.
fn copy_of(install: &Install, base: &Path) -> Result<PathBuf, String> {
    let copy = base.join(install.game.key());
    let source = install.root.display().to_string();
    let made_from = std::fs::read_to_string(copy.join(COPY_SOURCE)).ok();
    if copy.exists() && made_from.as_deref() != Some(source.as_str()) {
        outln!("{} is not a copy of {source}, so it is copied again.", copy.display());
        remove_folder(&copy)?;
    }
    if !copy.exists() {
        outln!("Copying {} to {}.", install.root.display(), copy.display());
        let partial = base.join(format!("{}.partial", install.game.key()));
        remove_folder(&partial)?;
        copy_tree(&install.root, &partial)?;
        write(&partial.join(COPY_SOURCE), source.as_bytes())?;
        std::fs::rename(&partial, &copy).map_err(|error| error.to_string())?;
    }
    Ok(copy)
}

/// Copies `from`'s files and folders into `to`, each file cloned where the
/// disk can. A symbolic link is refused, since no port's install has one.
fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    let error = |path: &Path, error: std::io::Error| format!("{}: {error}", path.display());
    std::fs::create_dir_all(to).map_err(|e| error(to, e))?;
    for entry in std::fs::read_dir(from).map_err(|e| error(from, e))? {
        let entry = entry.map_err(|e| error(from, e))?;
        let (source, target) = (entry.path(), to.join(entry.file_name()));
        let kind = entry.file_type().map_err(|e| error(&source, e))?;
        if kind.is_dir() {
            copy_tree(&source, &target)?;
        } else if kind.is_file() {
            reflink_copy::reflink_or_copy(&source, &target).map_err(|e| error(&source, e))?;
        } else {
            return Err(format!("{}: not a file or a folder", source.display()));
        }
    }
    Ok(())
}

/// How a run goes, besides its start.
pub struct Options {
    /// Whether the game is started. Without, only the copy and its scripts
    /// are written.
    pub launch: bool,
    /// Whether the player is a character made by the player, from the kept
    /// start save, in place of the game's default player.
    pub created: bool,
    /// Whether the kept start save is forgotten, so a new game makes it
    /// again.
    pub new_start: bool,
}

/// `kjk original`: the original game started as `start` says.
pub fn original(discovery: &Discovery, start: Start, options: &Options) -> ExitCode {
    let started = || -> Result<(), String> {
        refuse_untranslatable(&start)?;
        let opened = Opened::holding(discovery, module_of(&start)?)?;
        run(discovery, &opened, &start, options)
    };
    finish(started())
}

/// `kjk report original`: the original game started as the report at `zip`
/// was made, with the options given laid over it.
pub fn report(discovery: &Discovery, zip: &Path, given: Start, options: &Options) -> ExitCode {
    let started = || -> Result<(), String> {
        refuse_untranslatable(&given)?;
        let crumbs = crate::report_verbs::Report::open(zip)?
            .text("breadcrumbs.txt")
            .unwrap_or(String::new());
        let opened = Opened::holding(discovery, &module_of_report(&crumbs)?)?;
        let start = laid_over(start_of_report(zip, &opened)?, given);
        outln!("kjk original {}", flags(&start).join(" "));
        run(discovery, &opened, &start, options)
    };
    finish(started())
}

fn finish(result: Result<(), String>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run(discovery: &Discovery, opened: &Opened, start: &Start, options: &Options) -> Result<(), String> {
    let original = opened.original;
    let install = discovery
        .get(original.game)
        .ok_or(format!("{}: not found", original.game.key()))?;
    let port = Port::of(install, original)?;
    let catalog = crate::script_verbs::catalog(&opened.base)?;
    let scripts = scripts(start, &catalog, opened, options.created)?;
    let base = original_folder()?;
    let copy = copy_of(install, &base)?;
    let kept = StartSave {
        kept: base.join(format!("{}-start", install.game.key())),
    };
    if options.new_start {
        kept.forget()?;
    }
    let data = data_inside(install)?;
    let override_folder = copy.join(data).join("override");
    remove_folder(&override_folder)?;
    let installed = install.data_root.join("override");
    if installed.is_dir() {
        copy_tree(&installed, &override_folder)?;
    } else {
        std::fs::create_dir_all(&override_folder).map_err(|error| error.to_string())?;
    }
    for (name, bytes) in &scripts {
        let path = override_folder.join(format!("{name}.ncs"));
        write(&path, bytes)?;
        outln!("{}", path.display());
        if name.starts_with("kjk_orig") {
            outln!("; the game's own script, run first by the hook after it\n");
        } else if let Ok(ncs) = fmt_odyssey::ncs::Ncs::parse(bytes) {
            outln!("{}", script_ncs::disasm::listing(&ncs, Some(&catalog)));
        }
    }
    if !options.launch {
        outln!("Not started, because of --no-launch.");
        return Ok(());
    }
    let hook = match options.created {
        true => None,
        false => {
            give_test_player(&opened.base, &copy.join(data))?;
            Some(hook_files(&port)?)
        }
    };
    let log = base.join(format!("{}-hook.log", install.game.key()));
    launch(
        &port,
        original,
        &copy,
        data,
        module_of(start)?,
        &kept,
        hook.as_ref(),
        &log,
    )
}
