//! `kjk script levels <game> [module]`: levels checked end to end. The
//! install's classes (`classes.2da` and the tables it names) and
//! `exptable.2da` are read into the module's library; then, as the player
//! in the module (the Endar Spire by default):
//!
//! 1. Trask joins the party; the game's own `k_pend_xpboost` (Trask's
//!    level-up talk) gives the party 1000 XP: the player (level 1) may take
//!    level 2, Trask (a level 3 Soldier) may not yet.
//! 2. The party gains 5000 more: Trask takes level 4 on his own, with the
//!    game's picks (`GetHitDice` reads it).
//! 3. The player asks for a level with more feats than it offers
//!    (refused), then with the game's picks: it takes level 2.
//!
//! A save carries the careers.

use std::process::ExitCode;

use fmt_odyssey::ncs::Ncs;
use serde_json::json;
use sim::levels::Choices;
use vfs::Discovery;

use crate::probe::{Call, Probe, by_tag, int, me, text};
use crate::scenes::Report;
use crate::script_verbs::{catalog, ncs_type, open};

const TRASK: &str = "end_trask";
/// The game's script that tops the party up to level 2's XP.
const BOOST: &str = "k_pend_xpboost";

/// `SetGlobalNumber(name, routine(<trask>))`.
fn read(name: &str, routine: &'static str) -> Vec<Call> {
    [
        by_tag(TRASK, 0),
        vec![
            Call::Routine(routine, 1),
            text(name),
            Call::Routine("SetGlobalNumber", 2),
        ],
    ]
    .concat()
}

/// A character's sheet: the snapshot's, else its instance's.
fn sheet(probe: &mut Probe, id: sim::SimId) -> Option<sim::combat::Sheet> {
    let snapshot = probe.world.snapshot();
    let entity = snapshot.get(id)?;
    entity.sheet.or_else(|| {
        sim::Instances::in_snapshot(probe.world.level(), &snapshot)
            .of_entity(entity)?
            .sheet
    })
}

pub fn levels(discovery: &Discovery, game: &str, module: &str) -> ExitCode {
    let mut report = Report::default();
    let run = |report: &mut Report| -> Result<serde_json::Value, String> {
        let (game, index, base) = open(discovery, game)?;
        let routines = catalog(&base)?;
        let resolver = base
            .with_module(&index, module)
            .map_err(|error| error.to_string())?;
        let found = resolver
            .find(BOOST, ncs_type())
            .ok_or(format!("{BOOST}.ncs: not found"))?;
        let bytes = found.location.read().map_err(|error| error.to_string())?;
        let boost = Ncs::parse(&bytes).map_err(|error| error.to_string())?;
        let trask = by_tag(TRASK, 0);
        let join = vec![
            [
                trask.clone(),
                vec![int(0), Call::Routine("AddAvailableNPCByObject", 2)],
            ]
            .concat(),
            [
                trask.clone(),
                vec![int(0), Call::Routine("AddPartyMember", 2)],
            ]
            .concat(),
        ];
        let more = vec![vec![int(5000), me(), Call::Routine("GiveXPToCreature", 2)]];
        let after = vec![read("kjk_dice", "GetHitDice")];
        let mut probe = Probe::new(discovery, game, module, |level| {
            let own = level.scripts().ok_or("no script library")?;
            let mut scripts = std::collections::BTreeMap::new();
            for (name, calls) in [
                ("kjk_join", &join),
                ("kjk_more", &more),
                ("kjk_after", &after),
            ] {
                scripts.insert(name.to_string(), crate::probe::program(&routines, calls)?);
            }
            scripts.insert(BOOST.to_string(), boost);
            Ok(
                sim::script::Library::new(routines, scripts, sim::script::Talents::default())
                    .with_items(own.items().clone())
                    .with_experience(own.experience().clone())
                    .with_classes(own.classes().clone()),
            )
        })?;
        let library = probe.world.level().scripts().ok_or("no library")?.clone();
        let classes = library.classes();
        let soldier = classes.classes.first().ok_or("no classes")?;
        let guardian = classes.classes.get(3).ok_or("no Jedi Guardian")?;
        report.check(
            classes.levels.get(1) == Some(&1000) && soldier.hit_die == 10 && soldier.bab.len() >= 20,
            format!(
                "{} classes; level 2 at {:?} XP, the last at {:?}; Soldier: hit die {}, {} feats to pick from",
                classes.classes.len(),
                classes.levels.get(1),
                classes.levels.last(),
                soldier.hit_die,
                soldier.feats.len()
            ),
        );
        report.check(
            guardian.force_die == 4 && !guardian.powers.is_empty() && !guardian.granted.is_empty(),
            format!(
                "Jedi Guardian: force die {}, {} powers, {} feats granted, {} powers at level 1",
                guardian.force_die,
                guardian.powers.len(),
                guardian.granted.len(),
                guardian.power_picks.first().copied().unwrap_or(0)
            ),
        );
        let snapshot = probe.world.snapshot();
        let trask = crate::scenes::tagged(probe.world.level(), &snapshot, TRASK)
            .first()
            .map(|&at| snapshot.entities[at].id)
            .ok_or("Trask")?;
        let me_id = probe.me;
        let before = sheet(&mut probe, trask).ok_or("Trask's sheet")?;
        let mine = sheet(&mut probe, me_id).ok_or("the player's sheet")?;

        probe.queue("kjk_join", probe.me);
        probe.step(2);
        probe.queue(BOOST, sim::SimId(trask.0));
        probe.step(2);
        let party = |probe: &Probe| probe.world.script_state().xp(sim::inventory::PARTY);
        let trask_now = sheet(&mut probe, trask).ok_or("Trask's sheet")?;
        report.check(
            party(&probe) == 1000
                && classes.may_level(mine.level, 1000)
                && trask_now.level == before.level,
            format!(
                "{BOOST}: the party's XP {}; the player (level {}) may level, Trask stays at {}",
                party(&probe),
                mine.level,
                trask_now.level
            ),
        );

        probe.queue("kjk_more", probe.me);
        probe.step(2);
        probe.queue("kjk_after", probe.me);
        probe.step(1);
        let trask_now = sheet(&mut probe, trask).ok_or("Trask's sheet")?;
        let state = probe.world.script_state().clone();
        let dice = sim::script::global(&state.numbers, "kjk_dice").copied().unwrap_or(-1);
        let hp = i32::from(soldier.hit_die)
            + before.modifier(sim::combat::sheet::Ability::Constitution)
            + sim::combat::passive::hp_per_level(&trask_now.feats);
        report.check(
            trask_now.level == 4
                && i32::from(trask_now.max_hp) == i32::from(before.max_hp) + hp
                && trask_now.bab == before.bab + 1
                && dice == 4
                && state.careers.get(&trask.0) == Some(&vec![(0, 4)]),
            format!(
                "6000 XP: Trask takes level {} on his own: HP {} → {}, attack {} → {}, skills {:?} → {:?}, GetHitDice {dice}",
                trask_now.level,
                before.max_hp,
                trask_now.max_hp,
                before.bab,
                trask_now.bab,
                before.skills,
                trask_now.skills
            ),
        );

        let career = vec![(0, mine.level)];
        let auto = classes.auto(&mine, &career, 0).ok_or("the game's picks")?;
        let greedy = Choices {
            feats: (0..=auto.feats.len() as u16)
                .map(|n| soldier.feats[usize::from(n)])
                .collect(),
            ..auto.clone()
        };
        let ask = |probe: &mut Probe, choices: &Choices| {
            let mut state = probe.world.script_state().clone();
            state.levelling.push((probe.me.0, choices.clone()));
            probe.world.set_script_state(state);
            probe.step_told(1).1
        };
        let told = ask(&mut probe, &greedy).contains(&sim::notice::Notice::LevelRefused);
        let refused = sheet(&mut probe, me_id).ok_or("the player's sheet")?;
        ask(&mut probe, &auto);
        let taken = sheet(&mut probe, me_id).ok_or("the player's sheet")?;
        report.check(
            told && refused.level == mine.level && taken.level == mine.level + 1 && taken.max_hp > mine.max_hp,
            format!(
                "the player: {} feats asked refused, and told (level {}); the game's picks {:?} taken: level {}, HP {} → {}",
                greedy.feats.len(),
                refused.level,
                auto,
                taken.level,
                mine.max_hp,
                taken.max_hp
            ),
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
            loaded.script.careers == state.careers && state.careers.len() == 2,
            format!("a save carries the careers ({:?})", loaded.script.careers),
        );
        let faults = probe.faults();
        report.check(
            faults.is_empty(),
            format!("the probe's scripts ran without faults ({faults:?})"),
        );
        Ok(json!({ "module": module, "careers": format!("{:?}", state.careers) }))
    };
    match run(&mut report) {
        Ok(facts) => report.finish(facts),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
