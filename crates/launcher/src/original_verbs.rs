//! `kjk original` (developer tooling): the original KOTOR 1 started as
//! `kjk play` starts kjk, to see whether a bug seen in kjk is the original
//! game's too. It takes `kjk play`'s start options (`cli_play::Start`):
//! the module, where the player stands, the story's globals, the party,
//! items, gold, XP, alignment, scripts run, and a conversation started.
//! Those the original game has no script for (`--made`, `--follow`,
//! `--skill`) are refused. `kjk report original` takes them from a bug
//! report instead (`plan_of_report`), and prints the `kjk original`
//! command it amounts to.
//!
//! A start is played by compiled scripts (`script_ncs::asm`) put in the
//! `override` folder of a copy of the install, under the names of the
//! game's own scripts, which they replace (`Hook`). The install itself is
//! never written. The copy is made once, each file cloned where the disk
//! can (`reflink-copy`), in `original/` of kjk's own folder. The install
//! is found as kjk finds it (`vfs::discover`), and the copy's data folder
//! is where the install's `data_root` is inside its `root`, whatever the
//! port. Its `override` folder is ours and is emptied before each run.
//!
//! How the game is started differs by port (`Port`): which program, where
//! its settings are, and how it is put in a window with the cheat console
//! on. Only the Mac port is handled yet. Its settings are not in the copy
//! but in the user's own folder, shared with the installed game
//! (`~/Library/Application Support/Knights of the Old Republic`): a run
//! turns the console on (`EnableCheats=1` in `swkotor.ini`) and the window
//! mode (`ASL_DisplayFullScreen` false in `Aspyr/Data.json`), then puts
//! both files back once the game quits. The game is started outside Steam
//! (`start_mac`).
//!
//! Character creation is done once. The first run's new game autosaves as
//! it arrives on the Endar Spire, and that save is kept (`StartSave`).
//! Later runs put it back, and the player loads it. The run then goes to
//! the start's module, where its state is set as the player arrives.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use fmt_odyssey::ResType;
use fmt_odyssey::gff::Gff;
use script_ncs::Catalog;
use script_ncs::asm::{self, Expr, action, call, constant, not, text, when};
use vfs::resolver::Resolver;
use vfs::{DirIndex, Discovery, Game, Install};

use crate::cli_play::Start;

/// A start as the original game is given it: the options of `kjk play`'s
/// `Start` that carry over, each as `kjk play` reads it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plan {
    stage: Option<String>,
    arrive: Option<String>,
    at: Option<([f32; 3], f32)>,
    globals: Vec<(String, i32)>,
    scripts: Vec<String>,
    gold: Option<i32>,
    alignment: Option<u8>,
    xp: Option<i32>,
    items: Vec<(String, u16)>,
    talk: Option<String>,
    dialog: Option<String>,
    party: Vec<String>,
}

impl Plan {
    /// `start`'s options, those the original game has no script for
    /// refused by name.
    pub fn of(start: Start) -> Result<Plan, String> {
        let Start {
            stage,
            arrive,
            at,
            made,
            globals,
            scripts,
            followers,
            gold,
            alignment,
            xp,
            by_hand,
        } = start;
        let by_hand = *by_hand;
        let refused = [
            (made.is_some(), "--made: the original game's character is the start save's"),
            (
                !followers.is_empty(),
                "--follow: the original game's party takes companions only (--party)",
            ),
            (
                !by_hand.skills.is_empty(),
                "--skill: K1's scripts cannot set a skill's rank",
            ),
        ];
        if let Some((_, why)) = refused.iter().find(|(given, _)| *given) {
            return Err(format!("not for the original game: {why}"));
        }
        Ok(Plan {
            stage,
            arrive,
            at,
            globals,
            scripts,
            gold,
            alignment,
            xp,
            items: by_hand.items,
            talk: by_hand.talk,
            dialog: by_hand.dialog,
            party: by_hand.party,
        })
    }

    /// `given` laid over `self` (a report's): each option given wins, the
    /// globals given come after the report's (a later one wins), and a
    /// party given replaces the report's.
    fn under(mut self, given: Plan) -> Plan {
        self.stage = given.stage.or(self.stage);
        self.arrive = given.arrive.or(self.arrive);
        self.at = given.at.or(self.at);
        self.gold = given.gold.or(self.gold);
        self.alignment = given.alignment.or(self.alignment);
        self.xp = given.xp.or(self.xp);
        if given.talk.is_some() {
            self.talk = given.talk;
            self.dialog = given.dialog;
        }
        self.globals.extend(given.globals);
        self.scripts.extend(given.scripts);
        self.items.extend(given.items);
        if !given.party.is_empty() {
            self.party = given.party;
        }
        self
    }

    /// Its options as `kjk play` and `kjk original` take them.
    fn flags(&self) -> Vec<String> {
        let mut flags = Vec::new();
        let mut flag = |name: &str, value: String| flags.extend([format!("--{name}"), value]);
        if let Some(stage) = &self.stage {
            flag("stage", stage.clone());
        }
        if let Some(arrive) = &self.arrive {
            flag("arrive", arrive.clone());
        }
        if let Some(([x, y, z], facing)) = self.at {
            flag("at", format!("{x:.2},{y:.2},{z:.2},{facing:.0}"));
        }
        for template in &self.party {
            flag("party", template.clone());
        }
        for (name, value) in &self.globals {
            flag("global", format!("{name}={value}"));
        }
        for (template, count) in &self.items {
            flag("item", format!("{template}={count}"));
        }
        if let Some(gold) = self.gold {
            flag("gold", gold.to_string());
        }
        if let Some(xp) = self.xp {
            flag("xp", xp.to_string());
        }
        if let Some(alignment) = self.alignment {
            flag("alignment", alignment.to_string());
        }
        for script in &self.scripts {
            flag("run", script.clone());
        }
        if let Some(talk) = &self.talk {
            flag("talk", talk.clone());
        }
        if let Some(dialog) = &self.dialog {
            flag("dialog", dialog.clone());
        }
        flags
    }
}

fn pc() -> Expr {
    call("GetFirstPC", [])
}

/// `statements` run `seconds` later.
fn later(seconds: f32, statements: Vec<Expr>) -> Expr {
    call("DelayCommand", [Expr::Float(seconds), action(statements)])
}

/// A line in the game's message log (the journal's feedback), for
/// following a run.
fn say(message: &str) -> Expr {
    call("SendMessageToPC", [pc(), text(&format!("kjk: {message}"))])
}

/// Whether the player is in a conversation.
fn talking() -> Expr {
    call("GetIsInConversation", [pc()])
}

/// The object tagged `tag` starts `dialog` (else its own conversation)
/// with the player, as `kjk play --talk` and `--dialog` do. As the game's
/// banter triggers do (`k_bant_trig`), the party's actions are cleared
/// first: a companion's queue holds actions that do not end, such as
/// following the leader, and a conversation queued behind them never
/// starts. Each step is told in the message log.
fn start_talk(tag: &str, dialog: Option<&str>) -> Vec<Expr> {
    let owner = || call("GetObjectByTag", [text(tag)]);
    let clear = |index: i32| {
        call(
            "AssignCommand",
            [
                call("GetPartyMemberByIndex", [Expr::Int(index)]),
                action([call("ClearAllActions", [])]),
            ],
        )
    };
    let what = dialog.map_or("its conversation".to_string(), |dialog| dialog.to_string());
    vec![
        when(not(call("GetIsObjectValid", [owner()])), [say(&format!("no {tag}"))]),
        when(call("GetIsInCombat", [pc()]), [say("the player is in combat")]),
        when(talking(), [say("the player is already in a conversation")]),
        say(&format!("{tag} starts {what}")),
        clear(0),
        clear(1),
        clear(2),
        call(
            "AssignCommand",
            [
                owner(),
                action([call(
                    "ActionStartConversation",
                    [pc(), text(dialog.unwrap_or(""))],
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
    ]
}

/// A script of the game's replaced by ours: `script`, as `module`'s data
/// has it. The game runs an area's "on enter" script each time a creature
/// enters the area, so ours acts only when the player does
/// (`GetIsPC(GetEnteringObject())`, as the game's own scripts check).
struct Hook {
    script: String,
    module: String,
    /// Whether the game's own script still runs, first and for every
    /// creature entering. It is copied into `override` under a name of
    /// ours (`kjk_orig1`, ...) and run by `ExecuteScript`.
    keeps_original: bool,
    /// What it does when the player enters.
    for_player: Vec<Expr>,
}

/// The module a new game starts in, and its first area's "on enter"
/// script. Each run's start: it autosaves as the player arrives, before
/// anything changes (`StartSave`), then sets the start's state there, or
/// leaves for the start's module. The game's own script is left out: it
/// plays the opening movie and starts Trask's conversation, on every load
/// of the start save too.
const START_MODULE: &str = "end_m01aa";
const START_SCRIPT: &str = "k_pend_area01";

/// What sets `plan`'s state once the player has arrived, each statement
/// delayed from that moment: the spot at once; the globals, gold, XP,
/// alignment and items half a second on; the party at 1 second, beside
/// the player; the scripts at 1.5 seconds; the conversation at 3 seconds,
/// tried again at 6 and 9 where none runs yet (one that ran and ended is
/// not started again: the first try's report says it started).
fn arrival(plan: &Plan, data: &Resolver) -> Result<Vec<Expr>, String> {
    let mut out = Vec::new();
    if let Some(([x, y, z], facing)) = plan.at {
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
    for (name, value) in &plan.globals {
        let routine = match kinds.iter().find(|(known, _)| known.eq_ignore_ascii_case(name)) {
            Some((_, Kind::Boolean)) => "SetGlobalBoolean",
            Some((_, Kind::Number)) => "SetGlobalNumber",
            None => return Err(format!("--global {name}: not in globalcat.2da")),
        };
        state.push(call(routine, [text(name), Expr::Int(*value)]));
    }
    if let Some(gold) = plan.gold {
        // `kjk play --gold` sets the party's gold, not adds to it.
        state.push(call(
            "TakeGoldFromCreature",
            [call("GetGold", [pc()]), pc(), constant("TRUE")],
        ));
        state.push(call("GiveGoldToCreature", [pc(), Expr::Int(gold)]));
    }
    if let Some(xp) = plan.xp {
        state.push(call("SetXP", [pc(), Expr::Int(xp)]));
    }
    if let Some(alignment) = plan.alignment {
        state.push(call(
            "SetGoodEvilValue",
            [pc(), Expr::Int(i32::from(alignment))],
        ));
    }
    for (template, count) in &plan.items {
        state.push(call(
            "CreateItemOnObject",
            [text(template), pc(), Expr::Int(i32::from(*count))],
        ));
    }
    if !state.is_empty() {
        out.push(later(0.5, state));
    }
    let mut party = Vec::new();
    for template in &plan.party {
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
    let scripts: Vec<Expr> = plan
        .scripts
        .iter()
        .map(|script| call("ExecuteScript", [text(script), pc()]))
        .collect();
    if !scripts.is_empty() {
        out.push(later(1.5, scripts));
    }
    if let Some(tag) = &plan.talk {
        let talk = || start_talk(tag, plan.dialog.as_deref());
        out.push(later(3.0, talk()));
        out.push(later(6.0, vec![when(not(talking()), talk())]));
        out.push(later(9.0, vec![when(not(talking()), talk())]));
    }
    Ok(out)
}

/// A global's kind, as `globalcat.2da` declares it.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Boolean,
    Number,
}

/// The globals `globalcat.2da` declares, by name.
fn global_kinds(data: &Resolver) -> Result<Vec<(String, Kind)>, String> {
    let table = data.two_da("globalcat").ok_or("globalcat.2da: not in the install")?;
    Ok((0..table.cells.len())
        .filter_map(|row| {
            let name = table.text(row, "name")?;
            let kind = match table.text(row, "type")?.to_ascii_lowercase().as_str() {
                "boolean" => Kind::Boolean,
                "number" => Kind::Number,
                _ => return None,
            };
            Some((name, kind))
        })
        .collect())
}

/// A companion's `NPC_*` number: the `npc.2da` row whose tag is its
/// template's (`p_carth`'s `Carth`, row 2), as `kjk play --party` finds it.
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

/// The hooks a run puts in `override`, by name, as file bytes: the start's
/// hook, which goes to `plan`'s module, and that module's, which sets the
/// plan's state as the player arrives.
fn scripts(
    plan: &Plan,
    catalog: &Catalog,
    index: &DirIndex,
    base: &Resolver,
) -> Result<Vec<(String, Vec<u8>)>, String> {
    let module = plan
        .stage
        .as_deref()
        .ok_or("--stage: the module to start in (e.g. tar_m02ac)")?;
    let data = module_data(index, base, module)?;
    let there = arrival(plan, &data)?;
    let mut start = vec![later(0.5, vec![call("DoSinglePlayerAutoSave", [])])];
    let mut hooks = Vec::new();
    if module.eq_ignore_ascii_case(START_MODULE) {
        if plan.arrive.is_some() {
            return Err(format!("--arrive: every run starts at {START_MODULE}'s entry"));
        }
        start.extend(there.into_iter().map(|statement| later(0.5, vec![statement])));
    } else {
        let arrive = plan.arrive.as_deref().unwrap_or("");
        start.push(later(
            1.5,
            vec![call("StartNewModule", [text(module), text(arrive)])],
        ));
        hooks.push(Hook {
            script: entry_script(&data, module)?,
            module: module.to_string(),
            keeps_original: true,
            for_player: there
                .into_iter()
                .map(|statement| later(0.5, vec![statement]))
                .collect(),
        });
    }
    hooks.insert(
        0,
        Hook {
            script: START_SCRIPT.into(),
            module: START_MODULE.into(),
            keeps_original: false,
            for_player: start,
        },
    );
    let mut files = Vec::new();
    let mut originals = 0;
    for hook in hooks {
        let mut statements = Vec::new();
        if hook.keeps_original {
            originals += 1;
            let name = format!("kjk_orig{originals}");
            let own = module_data(index, base, &hook.module)?
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

/// `module`'s data (the shared layers and its own archives).
fn module_data(index: &DirIndex, base: &Resolver, module: &str) -> Result<Resolver, String> {
    base.with_module(index, module)
        .map_err(|error| format!("{module}: {error}"))
}

/// The "on enter" script of the area of `module` (whose `data` this is; a
/// KOTOR module has one area).
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

/// A player's movement box's origin above its soles (24 units).
const ORIGIN_ABOVE_SOLES_M: f32 = 24.0 * 0.0254;

/// The start a bug report gives: the module its breadcrumbs name, the
/// conversation they name (`--dialog`, said by the speaker of the line it
/// had reached, `--talk`), and, where its save of the moment (`now.kjkv`)
/// reads, the player's spot, the story's globals, the party, its gold and
/// its XP. A save this build does not read (one made by another build's
/// format) gives none of those; the reason is printed.
fn plan_of_report(report: &Path, data: &dyn Fn(&str) -> Result<Resolver, String>) -> Result<Plan, String> {
    let report = crate::report_verbs::Report::open(report)?;
    let crumbs = report.text("breadcrumbs.txt").unwrap_or(String::new());
    let crumb = |key: &str| {
        crumbs
            .lines()
            .find_map(|line| line.strip_prefix(key)?.strip_prefix(' '))
            .map(str::trim)
            .filter(|value| !value.is_empty() && *value != "none")
    };
    let module = crumb("module")
        .and_then(|value| value.split_whitespace().next())
        .ok_or("the report's breadcrumbs name no module")?
        .to_string();
    let mut plan = Plan {
        stage: Some(module.clone()),
        ..Plan::default()
    };
    // `conversation banter node 89`: the conversation, and the entry shown.
    if let Some(talk) = crumb("conversation") {
        let mut words = talk.split_whitespace();
        let dialog = words.next().map(str::to_string);
        let node = words
            .skip_while(|word| *word != "node")
            .nth(1)
            .and_then(|node| node.parse::<usize>().ok());
        if let (Some(dialog), Some(node)) = (dialog, node) {
            let speaker = data(&module)?
                .read(&dialog, ResType::DLG)
                .and_then(|bytes| fmt_odyssey::dlg::Dlg::parse(&bytes).ok())
                .and_then(|dlg| dlg.entries.get(node).map(|entry| entry.speaker.clone()))
                .filter(|speaker| !speaker.is_empty());
            match speaker {
                Some(speaker) => {
                    plan.talk = Some(speaker);
                    plan.dialog = Some(dialog);
                }
                None => outln!(
                    "the report's conversation {dialog} (node {node}): no speaker named, so not started"
                ),
            }
        }
    }
    match saved_state(&report) {
        Ok(saved) => plan = plan.under(saved),
        Err(error) => outln!(
            "the report's save gives nothing ({error}): no spot, globals, party, gold or XP from it"
        ),
    }
    Ok(plan)
}

/// What the report's save of the moment gives a start.
fn saved_state(report: &crate::report_verbs::Report) -> Result<Plan, String> {
    let bytes = report.bytes("now.kjkv").ok_or("no now.kjkv")?;
    let save = sim::Save::decode(bytes).map_err(|error| format!("now.kjkv: {error}"))?;
    let body = save
        .snapshot
        .body(sim::Seat(0))
        .ok_or("now.kjkv: the first player has no body")?;
    let [x, y, z] = body.position;
    let facing = body.player.map(|motion| motion.view[1]).unwrap_or(0.0);
    let state = &save.script;
    let globals = state
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
    let party = state
        .party
        .members
        .iter()
        .filter_map(|npc| state.party.available.get(npc))
        .map(|npc| npc.template.clone())
        .collect();
    Ok(Plan {
        at: Some(([x, y, z - ORIGIN_ABOVE_SOLES_M], facing)),
        globals,
        party,
        gold: Some(state.gold),
        xp: state.xp.get(&sim::inventory::PARTY).copied(),
        ..Plan::default()
    })
}

/// How an install of the original game is started. The ports differ in
/// the program, where the settings are, and how the window mode is set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Port {
    /// Aspyr's Mac port: the data inside a `.app`.
    Mac,
    /// The Windows game (`swkotor.exe`), on Windows or through Proton.
    Windows,
}

impl Port {
    fn of(install: &Install) -> Result<Port, String> {
        let in_app = install
            .data_root
            .ancestors()
            .any(|path| path.extension().is_some_and(|ext| ext == "app"));
        if in_app {
            Ok(Port::Mac)
        } else if install.markers.contains(&"swkotor.exe") {
            Ok(Port::Windows)
        } else {
            Err(format!(
                "{}: not a port kjk original knows (the Mac port's .app, or swkotor.exe)",
                install.root.display()
            ))
        }
    }

    /// Starts the game in `copy` (a copy of the install's `root`) with the
    /// cheat console on and in a window, the start save put back in its
    /// saves, and waits until it quits.
    fn start(self, install: &Install, copy: &Path, start: &StartSave) -> Result<(), String> {
        match self {
            Port::Mac => start_mac(install, copy, start),
            Port::Windows => {
                Err("the Windows game is not handled yet: its program, window mode and console setting are still to be written and tested".into())
            }
        }
    }
}

/// KOTOR 1's id on Steam.
const STEAM_APP_ID: &str = "32370";

fn start_mac(install: &Install, copy: &Path, start: &StartSave) -> Result<(), String> {
    let inside = install
        .data_root
        .strip_prefix(&install.root)
        .map_err(|_| "the install's data is not inside its folder")?;
    let app = inside
        .ancestors()
        .find(|path| path.extension().is_some_and(|ext| ext == "app"))
        .map(|app| copy.join(app))
        .ok_or("the Mac port's data is not inside a .app in the install")?;
    // Not the app's own launcher (`AppBundleExe`, what `open` starts): it
    // hands the game to Steam, which starts Steam's install, not the copy.
    // The game itself runs without Steam when its Steam id is beside it,
    // started from its data folder.
    let program = app.join("Contents/MacOS");
    std::fs::write(program.join("steam_appid.txt"), STEAM_APP_ID)
        .map_err(|error| error.to_string())?;
    let settings = std::env::home_dir()
        .ok_or("no home folder")?
        .join("Library/Application Support/Knights of the Old Republic");
    let ini = settings.join("swkotor.ini");
    let data = settings.join("Aspyr").join("Data.json");
    let _ini = Restore::take(ini.clone());
    let _data = Restore::take(data.clone());
    // A file the game has not written yet reads as empty.
    let read = |path: &Path| std::fs::read_to_string(path).unwrap_or(String::new());
    std::fs::create_dir_all(settings.join("Aspyr")).map_err(|error| error.to_string())?;
    std::fs::write(&ini, with_cheats(&read(&ini))).map_err(|error| error.to_string())?;
    std::fs::write(&data, windowed(&read(&data))?).map_err(|error| error.to_string())?;
    let saves = settings.join("saves");
    let loaded = start.put_back(&saves)?;
    match &loaded {
        Some(name) => outln!(
            "in the game: Load Game, then the save \"{name}\" (the Endar Spire's start, kept from the first run)"
        ),
        None => outln!(
            "in the game: a new game, once. The Endar Spire's start is autosaved as it loads, and kept when the game quits, for later runs to load"
        ),
    }
    outln!(
        "starting {}; {} and {} are put back when it quits",
        program.join("KOTOR_Exe").display(),
        ini.display(),
        data.display()
    );
    let status = Command::new(program.join("KOTOR_Exe"))
        .current_dir(copy.join(inside))
        .status()
        .map_err(|error| format!("KOTOR_Exe: {error}"))?;
    if loaded.is_none() {
        match start.keep(&saves)? {
            Some(name) => outln!("kept the save \"{name}\" as the start for later runs"),
            None => outln!("no autosave found in {}: nothing kept", saves.display()),
        }
    }
    if !status.success() {
        return Err(format!("KOTOR_Exe: {status}"));
    }
    Ok(())
}

/// The save later runs start from: the autosave a new game makes as it
/// arrives on the Endar Spire (`ON_ENTER`), before the setup's work. It is
/// kept in kjk's folder (`original/k1-start/`), since the game writes its
/// autosave again on every arrival, and put back before each run. Saves
/// are folders in the game's `saves` folder.
struct StartSave {
    kept: PathBuf,
}

impl StartSave {
    /// The autosave's folder in `saves`, by its name.
    fn autosave(saves: &Path) -> Option<PathBuf> {
        std::fs::read_dir(saves).ok()?.flatten().find_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            (entry.path().is_dir() && name.contains("autosave")).then(|| entry.path())
        })
    }

    /// The kept save copied into `saves` under its own name, replacing
    /// what is there; its name, or none when nothing is kept yet.
    fn put_back(&self, saves: &Path) -> Result<Option<String>, String> {
        let Some(save) = std::fs::read_dir(&self.kept)
            .ok()
            .and_then(|mut entries| entries.find_map(|entry| entry.ok()))
        else {
            return Ok(None);
        };
        let target = saves.join(save.file_name());
        if target.exists() {
            std::fs::remove_dir_all(&target).map_err(|error| error.to_string())?;
        }
        copy_tree(&save.path(), &target)?;
        Ok(Some(save.file_name().to_string_lossy().into_owned()))
    }

    /// The autosave in `saves` kept; its name, or none when there is none.
    fn keep(&self, saves: &Path) -> Result<Option<String>, String> {
        let Some(autosave) = StartSave::autosave(saves) else {
            return Ok(None);
        };
        let name = autosave.file_name().ok_or("an autosave without a name")?;
        if self.kept.exists() {
            std::fs::remove_dir_all(&self.kept).map_err(|error| error.to_string())?;
        }
        copy_tree(&autosave, &self.kept.join(name))?;
        Ok(Some(name.to_string_lossy().into_owned()))
    }

    /// Forgets the kept save: the next run starts a new game again.
    fn forget(&self) -> Result<(), String> {
        if self.kept.exists() {
            std::fs::remove_dir_all(&self.kept).map_err(|error| error.to_string())?;
        }
        Ok(())
    }
}

/// `swkotor.ini` with `EnableCheats=1` under `[Game Options]`.
fn with_cheats(ini: &str) -> String {
    let mut out = Vec::new();
    let mut section = "";
    let mut written = false;
    for line in ini.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            if section == "[Game Options]" && !written {
                out.push("EnableCheats=1".to_string());
                written = true;
            }
            section = trimmed;
        }
        if section == "[Game Options]" && trimmed.starts_with("EnableCheats=") {
            if !written {
                out.push("EnableCheats=1".to_string());
                written = true;
            }
            continue;
        }
        out.push(line.to_string());
    }
    if !written {
        if section != "[Game Options]" {
            out.push("[Game Options]".to_string());
        }
        out.push("EnableCheats=1".to_string());
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

/// The copy of `install`'s `root` kjk runs, made once in `base`.
fn copy_of(install: &Install, base: &Path) -> Result<PathBuf, String> {
    let copy = base.join(install.game.key());
    if !copy.exists() {
        outln!("copying {} to {}", install.root.display(), copy.display());
        let partial = base.join(format!("{}.partial", install.game.key()));
        let _ = std::fs::remove_dir_all(&partial);
        copy_tree(&install.root, &partial)?;
        std::fs::rename(&partial, &copy).map_err(|error| error.to_string())?;
    }
    Ok(copy)
}

/// `from`'s files and folders copied into `to`, each file cloned where the
/// disk can. A symbolic link is refused: no port's install has one.
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
    /// Start the game; else only write the copy and its scripts.
    pub launch: bool,
    /// Forget the kept start save: a new game makes it again.
    pub new_start: bool,
}

/// `kjk original`: the original game started as `start` says.
pub fn original(discovery: &Discovery, start: Start, options: &Options) -> ExitCode {
    finish(Plan::of(start).and_then(|plan| run(discovery, &plan, options)))
}

/// `kjk report original`: the original game started as the report at
/// `zip` was made, the options given laid over it.
pub fn report(discovery: &Discovery, zip: &Path, start: Start, options: &Options) -> ExitCode {
    let plan = || -> Result<Plan, String> {
        let given = Plan::of(start)?;
        let (_, index, base) = crate::script_verbs::open(discovery, "k1")?;
        let data = |module: &str| module_data(&index, &base, module);
        let plan = plan_of_report(zip, &data)?.under(given);
        outln!("kjk original {}", plan.flags().join(" "));
        Ok(plan)
    };
    finish(plan().and_then(|plan| run(discovery, &plan, options)))
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

fn run(discovery: &Discovery, plan: &Plan, options: &Options) -> Result<(), String> {
    let (_, index, resolver) = crate::script_verbs::open(discovery, "k1")?;
    let catalog = crate::script_verbs::catalog(&resolver)?;
    let scripts = scripts(plan, &catalog, &index, &resolver)?;
    let install = discovery.get(Game::Kotor1).ok_or("k1: not found")?;
    let base = original_folder()?;
    let copy = copy_of(install, &base)?;
    let start = StartSave {
        kept: base.join(format!("{}-start", install.game.key())),
    };
    if options.new_start {
        start.forget()?;
    }
    let data = install
        .data_root
        .strip_prefix(&install.root)
        .map_err(|_| "the install's data is not inside its folder")?;
    let over = copy.join(data).join("override");
    if over.exists() {
        std::fs::remove_dir_all(&over).map_err(|error| error.to_string())?;
    }
    std::fs::create_dir_all(&over).map_err(|error| error.to_string())?;
    for (name, bytes) in &scripts {
        let path = over.join(format!("{name}.ncs"));
        std::fs::write(&path, bytes).map_err(|error| error.to_string())?;
        outln!("{}", path.display());
        if name.starts_with("kjk_orig") {
            outln!("; the game's own script, run first by the hook after it\n");
        } else if let Ok(ncs) = fmt_odyssey::ncs::Ncs::parse(bytes) {
            outln!("{}", script_ncs::disasm::listing(&ncs, Some(&catalog)));
        }
    }
    if !options.launch {
        outln!("not launched (--no-launch)");
        return Ok(());
    }
    Port::of(install)?.start(install, &copy, &start)
}
