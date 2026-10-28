//! `kjk script xp <game> [module]`: experience checked end to end. The
//! install's XP tables (`plot.2da`, `xptable.2da`) and journal are read into
//! the module's library; two scripts made here (`probe`) run as the player
//! in the module (the Endar Spire by default):
//!
//! 1. XP given to the player, set, given to a creature outside the party
//!    (its own), a plot's half given (`GivePlotXP`), a quest's XP read and
//!    the quest taken to an entry worth a share of it; `GetXP` after each.
//! 2. The first creature hostile to the player killed (`EffectDeath`): the
//!    party gains its challenge rating's worth at the player's level.
//!
//! Every gain is a notice to the player (setting XP is not); the story's
//! XP and the journal, not the kill, are told in the status summary
//! (`Progress::statuses`); a save carries the XP.

use std::process::ExitCode;

use serde_json::json;
use sim::combat::sheet::factions;
use sim::StatusKind;
use sim::notice::Notice;
use vfs::Discovery;

use crate::probe::{Call, Probe, by_tag, float, int, me, text};
use crate::scenes::Report;
use crate::script_verbs::{catalog, open};

/// `SetGlobalString(name, IntToString(GetXP(<object>)))`: as text, as a
/// number global holds only a signed byte (KOTOR's).
fn read(name: &str, of: &[Call]) -> Vec<Call> {
    let mut calls = of.to_vec();
    calls.extend([Call::Routine("GetXP", 1)]);
    calls.extend(kept(name));
    calls
}

/// `SetGlobalString(name, IntToString(<the int on top>))`.
fn kept(name: &str) -> [Call; 3] {
    [
        Call::Routine("IntToString", 1),
        text(name),
        Call::Routine("SetGlobalString", 2),
    ]
}

/// `routine(<object>, amount)`.
fn on(routine: &'static str, of: &[Call], amount: i32) -> Vec<Call> {
    let mut calls = vec![int(amount)];
    calls.extend_from_slice(of);
    calls.push(Call::Routine(routine, 2));
    calls
}

/// The plot `GivePlotXP` gives half of, and a quest whose entry is worth a
/// share of its plot: K1's own.
const PLOT: &str = "kor33_mekel";
const QUEST: &str = "tat18ac_dragonhunt";
const ENTRY: i32 = 60;

pub fn xp(discovery: &Discovery, game: &str, module: &str) -> ExitCode {
    let mut report = Report::default();
    let run = |report: &mut Report| -> Result<serde_json::Value, String> {
        let (game, _, base) = open(discovery, game)?;
        let routines = catalog(&base)?;
        let mut chosen = None;
        let mut probe = Probe::new(discovery, game, module, |level| {
            let own = level.scripts().ok_or("no script library")?;
            let data = level.module().ok_or("no module")?;
            let mut seen = std::collections::BTreeMap::new();
            let (mut bystander, mut enemy) = (None, None);
            for object in data.objects() {
                let Some(sheet) = object.sheet.filter(|_| !object.tag.is_empty()) else {
                    continue;
                };
                let nth = seen.entry(object.tag.clone()).or_insert(0);
                let hostile = data.repute.hostile(sheet.faction, factions::PLAYER);
                let slot = if hostile { &mut enemy } else { &mut bystander };
                if slot.is_none() {
                    *slot = Some((object.tag.clone(), *nth, object.challenge));
                }
                *nth += 1;
            }
            let bystander = bystander.ok_or("no creature outside the party")?;
            let enemy = enemy.ok_or("no creature hostile to the player")?;
            let other = by_tag(&bystander.0, bystander.1);
            let first = vec![
                on("GiveXPToCreature", &[me()], 100),
                read("kjk_given", &[me()]),
                on("SetXP", &[me()], 250),
                read("kjk_set", &[me()]),
                on("GiveXPToCreature", &other, 40),
                read("kjk_other", &other),
                read("kjk_party", &[me()]),
                vec![
                    int(50),
                    text(PLOT),
                    Call::Routine("GivePlotXP", 2),
                    text(QUEST),
                    Call::Routine("GetJournalQuestExperience", 1),
                ],
                kept("kjk_quest").to_vec(),
                vec![
                    int(0),
                    int(ENTRY),
                    text(QUEST),
                    Call::Routine("AddJournalQuestEntry", 3),
                ],
                read("kjk_story", &[me()]),
            ];
            let second = vec![{
                let mut calls = vec![float(0.0)];
                calls.extend(by_tag(&enemy.0, enemy.1));
                calls.extend([
                    int(1),
                    int(0),
                    Call::Routine("EffectDeath", 2),
                    int(0),
                    Call::Routine("ApplyEffectToObject", 4),
                ]);
                calls
            }];
            chosen = Some((bystander, enemy));
            let mut scripts = std::collections::BTreeMap::new();
            scripts.insert(
                "kjk_xp1".to_string(),
                crate::probe::program(&routines, &first)?,
            );
            scripts.insert(
                "kjk_xp2".to_string(),
                crate::probe::program(&routines, &second)?,
            );
            Ok(
                sim::script::Library::new(routines, scripts, sim::script::Talents::default())
                    .with_quests(own.quests().clone())
                    .with_experience(own.experience().clone()),
            )
        })?;
        let ((bystander, ..), (enemy, _, challenge)) = chosen.ok_or("nothing chosen")?;
        let library = probe.world.level().scripts().ok_or("no library")?.clone();
        let experience = library.experience();
        report.check(
            !experience.plots.is_empty() && !experience.kills.is_empty(),
            format!(
                "plot.2da: {} plots; xptable.2da: {} levels by {} challenge ratings",
                experience.plots.len(),
                experience.kills.len(),
                experience.kills.first().map_or(0, Vec::len)
            ),
        );

        let (mut gains, mut statuses, mut told_ms) = (Vec::new(), Vec::new(), None);
        let mut step = |probe: &mut Probe, ticks: u64| {
            let (snapshot, notices) = probe.step_told(ticks);
            for notice in notices {
                if let Notice::Experience(xp) = notice {
                    gains.push(xp);
                }
            }
            // The statuses raised since the last look (each kept a while).
            for status in &snapshot.progress.statuses {
                if told_ms.is_none_or(|ms| status.at_ms > ms) {
                    statuses.push(status.kind.clone());
                }
            }
            told_ms = snapshot.progress.statuses.iter().map(|s| s.at_ms).max().or(told_ms);
        };
        probe.queue("kjk_xp1", probe.me);
        step(&mut probe, 2);
        let number = |probe: &Probe, name: &str| {
            sim::script::global(&probe.world.script_state().strings, name)
                .and_then(|text| text.parse().ok())
                .unwrap_or(-1)
        };
        report.check(
            number(&probe, "kjk_given") == 100 && number(&probe, "kjk_set") == 250,
            format!(
                "GiveXPToCreature(player, 100): {}; SetXP(player, 250): {}",
                number(&probe, "kjk_given"),
                number(&probe, "kjk_set")
            ),
        );
        report.check(
            number(&probe, "kjk_other") == 40 && number(&probe, "kjk_party") == 250,
            format!(
                "{bystander} (outside the party) given 40: its own {}, the party's {}",
                number(&probe, "kjk_other"),
                number(&probe, "kjk_party")
            ),
        );
        let plot = experience.plot_named(PLOT).ok_or("the plot")?;
        let quest = number(&probe, "kjk_quest");
        let share = library.quest_xp(QUEST, ENTRY);
        let story = 250 + plot / 2 + share;
        report.check(
            quest > 0 && share > 0 && number(&probe, "kjk_story") == story,
            format!(
                "GivePlotXP({PLOT}, 50): +{}; {QUEST} worth {quest}, entry {ENTRY} its share +{share}: {}",
                plot / 2,
                number(&probe, "kjk_story")
            ),
        );

        let level = probe
            .world
            .snapshot()
            .get(probe.me)
            .and_then(|e| e.sheet)
            .map_or(1, |sheet| sheet.level);
        let worth = experience.kill(level, challenge);
        probe.queue("kjk_xp2", probe.me);
        step(&mut probe, 10);
        let party = probe.world.script_state().xp(sim::inventory::PARTY);
        report.check(
            worth > 0 && party == story + worth,
            format!(
                "{enemy} (challenge {challenge}) killed at level {level}: +{worth}, the party's {party}"
            ),
        );
        report.check(
            gains
                == [100, plot / 2, share, worth]
                    .into_iter()
                    .filter(|xp| *xp != 0)
                    .collect::<Vec<_>>(),
            format!("the player told of each gain: {gains:?}"),
        );
        // KOTOR's status summary tells the story's XP and the journal, not
        // a kill's, nor what a creature outside the party was given.
        let summary = [
            StatusKind::Xp(100),
            StatusKind::Xp(plot / 2),
            StatusKind::Journal,
            StatusKind::Xp(share),
        ];
        report.check(
            statuses == summary,
            format!("the status summary told {statuses:?}"),
        );
        let state = probe.world.script_state().clone();
        let save = sim::Save {
            hz: sim::TICK_HZ,
            level: probe.world.level().id(),
            snapshot: probe.world.snapshot(),
            script: state.clone(),
        };
        let loaded = sim::Save::decode(&save.encode()).map_err(|e| e.to_string())?;
        report.check(
            loaded.script.xp == state.xp,
            format!("a save carries the XP ({:?})", loaded.script.xp),
        );
        let faults = probe.faults();
        report.check(
            faults.is_empty(),
            format!("the scripts ran without faults ({faults:?})"),
        );
        Ok(json!({ "module": module, "enemy": enemy, "challenge": challenge, "party": party }))
    };
    match run(&mut report) {
        Ok(facts) => report.finish(facts),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
