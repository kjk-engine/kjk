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

use std::collections::VecDeque;

use bevy::prelude::*;
use commands::{Effect, Facts};
use sim::action::{Action, ArenaOrder};

use crate::sim_host::{Loading, OwnSeat, PresentedSnapshot, SceneSwap};

/// The most lines the console's log keeps.
const LOG_LINES: usize = 200;

pub struct ConsoleViewPlugin;

impl Plugin for ConsoleViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ConsoleLog>()
            .add_systems(Update, run_lines.after(console::ConsoleQueue));
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

/// What the line runner reads of play.
type Shown<'w> = (
    Option<Res<'w, SceneSwap>>,
    Option<Res<'w, Loading>>,
    Option<Res<'w, PresentedSnapshot>>,
    Option<Res<'w, OwnSeat>>,
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
    (swap, loading, presented, own): Shown,
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
        presented.as_deref(),
        own.as_deref(),
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
/// presented snapshot shows of this seat's party, and the classes the
/// game's tables have.
fn facts(
    swap: Option<&SceneSwap>,
    presented: Option<&PresentedSnapshot>,
    own: Option<&OwnSeat>,
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
    facts.creatures = (swap.level.scripts()).map(|library| {
        library
            .creatures()
            .keys()
            .map(|key| key.to_ascii_lowercase())
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
        facts.me = crate::cast::me(snapshot, own.0);
        facts.position = (facts.me)
            .and_then(|me| snapshot.get(me))
            .map(|entity| entity.position);
        facts.party = (crate::cast::party_of(snapshot, own.0).iter())
            .map(|member| member.id)
            .collect();
    }
    facts
}
