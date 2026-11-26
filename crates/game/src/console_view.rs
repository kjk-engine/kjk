//! The console's lines run against the table of commands (`crates/commands`).
//!
//! A `--cmds` script hands every line its own parser does not know to
//! `console::LineRequests` (`console::Command::Line`). This plugin fills the
//! table's `Facts` from what the game shows, runs each line, and carries out
//! what each command asks for. An order goes to the simulation through
//! `loot::Outgoing`, the list the arena's keys and the Debug Mode character
//! page write to. A line whose command needs a loaded game waits while a
//! module loads, so a line that a script gives early runs once the world
//! is up.
//!
//! Every command and its result go to `kjk.log` under the log target
//! `console`, and to the console's log, which a dump shows as its `console`
//! fact.
//!
//! The host's game also keeps the simulation's cheat rule
//! (`sim::cheats::Rule`) as the player chose it (`send_rule`). A run with a
//! command script, or with the setting `game.console` turned on, plays
//! under the rule Host only. Any other run plays under the rule Off.

use std::collections::VecDeque;

use bevy::prelude::*;
use commands::{Effect, Facts};
use sim::action::{Action, ArenaOrder, CheatOrder};
use sim::cheats::Rule;

use crate::sim_host::{Loading, OwnSeat, PresentedSnapshot, SceneSwap};

/// The most lines the console's log keeps.
const LOG_LINES: usize = 200;

pub struct ConsoleViewPlugin;

impl Plugin for ConsoleViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ConsoleLog>()
            .init_resource::<Scripted>()
            .add_systems(Update, (send_rule, run_lines.after(console::ConsoleQueue)));
    }
}

/// The console's log: each command echoed with `> ` in front, then what it
/// printed, the last `LOG_LINES` lines.
#[derive(Resource, Default, Debug)]
pub struct ConsoleLog(pub VecDeque<String>);

impl ConsoleLog {
    fn push(&mut self, line: String) {
        if self.0.len() == LOG_LINES {
            self.0.pop_front();
        }
        self.0.push_back(line);
    }
}

/// The run has a command script (`--cmds`).
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct Scripted(pub bool);

/// The settings key of the console's switch. The option on the settings
/// pages comes with the console's overlay.
pub(crate) const CONSOLE: &str = "game.console";

/// How many frames the host waits for its rule to show in the snapshots
/// before it sends the rule again.
const RULE_RESEND_FRAMES: u32 = 120;

/// The cheat rule this run plays under (see the module documentation).
fn wanted_rule(scripted: bool, settings: Option<&crate::settings::Settings>) -> Rule {
    match scripted || settings.is_some_and(|settings| settings.on(CONSOLE)) {
        true => Rule::Host,
        false => Rule::Off,
    }
}

/// What `send_rule` reads of the session: the snapshot shown, this
/// player's seat, and whether the world runs here.
type Seated<'w> = (
    Option<Res<'w, PresentedSnapshot>>,
    Option<Res<'w, OwnSeat>>,
    Option<Res<'w, crate::sim_host::WorldHere>>,
);

/// The first seat's game sends `CheatOrder::Policy` whenever the
/// simulation's rule is not the one this run plays under: when a session
/// starts, after a save from another rule loads, and when the setting
/// changes. An order still on its way is not sent again for
/// `RULE_RESEND_FRAMES` frames. A guest's game never sends it.
fn send_rule(
    scripted: Res<Scripted>,
    settings: Option<Res<crate::settings::Settings>>,
    (presented, own, here): Seated,
    mut outgoing: Option<ResMut<crate::loot::Outgoing>>,
    mut waiting: Local<u32>,
) {
    let (Some(presented), Some(own), Some(outgoing)) = (presented, own, outgoing.as_mut()) else {
        return;
    };
    if here.is_none() || own.0 != sim::Seat(0) {
        return;
    }
    let wanted = wanted_rule(scripted.0, settings.as_deref());
    if presented.0.current().cheats.rule == wanted {
        *waiting = 0;
        return;
    }
    if *waiting > 0 {
        *waiting -= 1;
        return;
    }
    info!(target: "console", "cheat rule: {wanted:?}");
    outgoing.0.push(Action::Cheat(CheatOrder::Policy(wanted)));
    *waiting = RULE_RESEND_FRAMES;
}

/// What the line runner reads of play.
type Shown<'w> = (
    Option<Res<'w, SceneSwap>>,
    Option<Res<'w, Loading>>,
    Option<Res<'w, PresentedSnapshot>>,
    Option<Res<'w, OwnSeat>>,
    Option<Res<'w, crate::target_view::Targeted>>,
);

/// Where the effects of a command go.
type Sinks<'w> = (
    Option<ResMut<'w, crate::loot::Outgoing>>,
    ResMut<'w, console::UiRequests>,
    ResMut<'w, console::VfxRequests>,
    ResMut<'w, console::DumpFacts>,
);

fn run_lines(
    mut requests: ResMut<console::LineRequests>,
    (swap, loading, presented, own, targeted): Shown,
    mut ui: Option<ResMut<crate::ui_view::Ui>>,
    (mut outgoing, mut screens, mut visuals, mut dump): Sinks,
    mut log: ResMut<ConsoleLog>,
) {
    if requests.0.is_empty() {
        return;
    }
    let loaded = swap.is_some() && !loading.as_ref().is_some_and(|loading| loading.active);
    let waiting = |line: &str| {
        !loaded && (commands::words::split(line).iter()).any(|words| commands::needs_game(words))
    };
    let ready = requests.0.iter().take_while(|line| !waiting(line)).count();
    if ready == 0 {
        return;
    }
    let lines: Vec<String> = requests.0.drain(..ready).collect();
    let assets = ui.as_mut().and_then(|ui| ui.assets.as_mut());
    let facts = facts(
        swap.as_deref().filter(|_| loaded),
        (presented.as_deref(), own.as_deref()),
        targeted.as_deref().and_then(|targeted| targeted.object),
        assets,
    );
    for line in lines {
        for (text, ran) in commands::run_line(&line, &facts) {
            log.push(format!("> {text}"));
            info!(target: "console", "> {text}");
            let effects = match ran {
                Ok(effects) => effects,
                Err(refusal) => {
                    warn!(target: "console", "{refusal}");
                    log.push(refusal.0);
                    continue;
                }
            };
            for effect in effects {
                match effect {
                    Effect::Order(action) => {
                        if let Some(outgoing) = outgoing.as_mut() {
                            outgoing.0.push(action);
                        }
                    }
                    Effect::Restore { who } => {
                        let (Some(swap), Some(outgoing)) = (swap.as_ref(), outgoing.as_mut())
                        else {
                            continue;
                        };
                        outgoing.0.push(Action::Arena(ArenaOrder::Restore {
                            who,
                            sheet: Box::new(crate::arena::sheet_for(&swap.level)),
                        }));
                    }
                    Effect::Print(text) => {
                        info!(target: "console", "{text}");
                        log.push(text);
                    }
                    Effect::ClearLog => log.0.clear(),
                    Effect::Ui(words) => screens.0.push(words),
                    Effect::Vfx(words) => visuals.0.push(words),
                }
            }
        }
    }
    dump.0.insert(
        "console".into(),
        serde_json::Value::from(log.0.iter().cloned().collect::<Vec<String>>()),
    );
}

/// The table's facts of this moment: the level when one is up, what the
/// presented snapshot shows of this seat's party, the object the crosshair
/// is on (`target`, an object number), and the classes the game's tables
/// have.
fn facts(
    swap: Option<&SceneSwap>,
    (presented, own): (Option<&PresentedSnapshot>, Option<&OwnSeat>),
    target: Option<u32>,
    assets: Option<&mut ui::assets::GuiAssets>,
) -> Facts {
    let mut facts = Facts {
        version: concat!(env!("KJK_VERSION"), " (", env!("KJK_GIT_SHA"), ")").into(),
        ..Facts::default()
    };
    let Some(swap) = swap else {
        return facts;
    };
    facts.loaded = true;
    facts.arena = swap.level.arena_rules().is_some();
    facts.module = Some(swap.level.name().to_string());
    let library = swap.level.scripts();
    facts.creatures = library.map(|library| {
        library
            .creatures()
            .keys()
            .map(|key| key.to_ascii_lowercase())
            .collect()
    });
    facts.items = library.map(|library| {
        (library.items().iter())
            .map(|item| item.template.to_ascii_lowercase())
            .collect()
    });
    if let Some(assets) = assets {
        let classes = (0..assets.rows("classes"))
            .filter_map(|row| {
                let label = assets.cell("classes", row, "label")?;
                Some((u8::try_from(row).ok()?, label))
            })
            .collect::<Vec<_>>();
        facts.classes = (!classes.is_empty()).then_some(classes);
    }
    if let (Some(presented), Some(own)) = (presented, own) {
        let snapshot = presented.0.current();
        facts.host = own.0 == sim::Seat(0);
        facts.cheats = snapshot.cheats.rule;
        facts.credits = Some(snapshot.goods.gold);
        facts.xp = Some(snapshot.progress.xp);
        facts.target = target
            .and_then(|index| sim::target::entity(snapshot, index))
            .map(|entity| entity.id);
        facts.me = crate::cast::me(snapshot, own.0);
        let level = (facts.me)
            .and_then(|me| snapshot.get(me))
            .and_then(|entity| entity.sheet)
            .map(|sheet| sheet.level);
        facts.next_level_xp =
            (library.zip(level)).and_then(|(library, level)| library.classes().next(level));
        facts.position = (facts.me)
            .and_then(|me| snapshot.get(me))
            .map(|entity| entity.position);
        facts.party = (crate::cast::party_of(snapshot, own.0).iter())
            .map(|member| member.id)
            .collect();
    }
    facts
}
