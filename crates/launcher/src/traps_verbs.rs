//! `kjk script traps <game> [module]`: traps and Demolitions checked end to
//! end on a module's own mines with its own scripts (`sim::traps`,
//! `world::traps`; the Sith base's gas mines, `tar_m09aa`, by default).
//! The base is calmed for the skill checks (its creatures of the player's
//! faction) so no fight turns the take-20 into a d20; one Sith is made
//! hostile again for the last.
//!
//! 1. The first mine's facts: its `traps.2da` row, DCs and faction, its
//!    trap script in the library.
//! 2. Awareness 6 finds it from 8 m (the party sees it, "This mine appears
//!    to be a …"); mines past 20 m stay unseen.
//! 3. Demolitions 4 disarms it, taking 20 out of combat.
//! 4. Demolitions 12 recovers a second (10 harder): its kit carried.
//! 5. The player walking into a third springs it: its row's script (the
//!    game's `k_trp_generic`) runs on the player, who is told, poisoned
//!    and hurt (its Fortitude made hopeless: a gas mine's poison is a
//!    save's);
//!    one of the trap's own faction walking into the fourth does not.
//! 6. With the recovered kit and a frag mine's (`FRAG`) carried, O lays the
//!    deadlier (`traps::best_kit`): the party's mine at the player's feet,
//!    which the layer stands on unharmed; the other kit kept.
//! 7. A save made and loaded with the laid mine in it; a Sith walking onto
//!    it in the loaded world springs it.
//! 8. The traps' scripts ran without faults.

use std::process::ExitCode;
use std::sync::Arc;

use serde_json::json;
use sim::action::Action;
use sim::combat::sheet::{factions, skills};
use sim::locks::lines;
use sim::module::{ObjectData, ObjectKind};
use sim::notice::Notice;
use vfs::Discovery;

use crate::probe::Probe;
use crate::scenes::Report;
use crate::script_verbs::open;
use crate::security_verbs::stand;

/// A module's mine: its index and facts.
type Mine = (u32, Arc<ObjectData>);

/// Ticks a second.
const SECOND: u64 = sim::TICK_HZ as u64;

/// A frag mine's kit (average: `traps.2da` row 4), given for the laying.
const FRAG: &str = "g_i_trapkit005";

fn bits(snapshot: &sim::Snapshot, index: u32) -> Option<u8> {
    snapshot
        .entities
        .iter()
        .find(|entity| entity.kind == sim::Kind::Object(ObjectKind::Trigger, index))
        .and_then(|entity| entity.object)
        .map(|state| state.trap)
}

fn seen(snapshot: &sim::Snapshot, index: u32) -> bool {
    bits(snapshot, index).is_some_and(|bits| bits & sim::trap::PARTY_SEES != 0)
}

/// Armed: there, not disabled.
fn armed(snapshot: &sim::Snapshot, index: u32) -> bool {
    bits(snapshot, index).is_some_and(|bits| bits & sim::trap::DISABLED == 0)
}

fn middle(mine: &ObjectData) -> [f32; 3] {
    sim::traps::centre(&mine.geometry, mine.position)
}

/// How far the mine's outline reaches from its middle (metres).
fn outline(mine: &ObjectData) -> f32 {
    let [x, y, _] = middle(mine);
    mine.geometry
        .iter()
        .map(|point| (mine.position[0] + point[0] - x).hypot(mine.position[1] + point[1] - y))
        .fold(0.0, f32::max)
}

/// The player stood `away` metres from the mine's middle, along +X.
fn near(probe: &mut Probe, mine: &ObjectData, away: f32) -> Result<(), String> {
    let [x, y, _] = middle(mine);
    stand(probe, mine, x + away, y)
}

/// The player's skill ranks set.
fn ranks(probe: &mut Probe, skill: usize, ranks: u8) -> Result<(), String> {
    probe.edit_me(|me| {
        me.sheet.as_mut().ok_or("no sheet")?.skills[skill] = ranks;
        Ok(())
    })
}

fn check_of(told: &[Notice], skill: u32) -> Option<(Option<u8>, i16, u8, bool)> {
    told.iter().find_map(|notice| match notice {
        Notice::Check {
            skill: at,
            roll,
            total,
            dc,
            success,
        } if *at == skill => Some((*roll, *total, *dc, *success)),
        _ => None,
    })
}

fn hp(snapshot: &sim::Snapshot, id: sim::SimId) -> Option<i32> {
    snapshot
        .get(id)
        .and_then(|entity| entity.fighter.as_ref())
        .map(|fighter| fighter.whole_hp())
}

fn poisoned(snapshot: &sim::Snapshot, id: sim::SimId) -> bool {
    snapshot
        .get(id)
        .and_then(|entity| entity.fighter.as_ref())
        .is_some_and(|fighter| fighter.pack.poison.is_some())
}

fn carried(snapshot: &sim::Snapshot, template: &str) -> bool {
    snapshot
        .goods
        .carried(sim::inventory::PARTY)
        .any(|(_, item)| item.template.eq_ignore_ascii_case(template))
}

/// Creature `id` stood at the mine's middle.
fn walk_onto(probe: &mut Probe, id: sim::SimId, at: [f32; 3]) -> Result<(), String> {
    probe.edit(id, |entity| {
        if let Some(body) = entity.player.as_mut() {
            body.origin = [
                at[0] / collision::UNIT,
                at[1] / collision::UNIT,
                at[2] / collision::UNIT + 0.125 - sim::motion::SOLE_Z,
            ];
            body.velocity = [0.0; 3];
        }
        entity.position = at;
        Ok(())
    })
}

/// How many times the library's `script` has run.
fn runs(probe: &Probe, script: &str) -> usize {
    probe
        .world
        .script_report()
        .runs
        .get(script)
        .copied()
        .unwrap_or(0)
}

/// The creatures with a fighter in the world (the base's).
fn creatures(snapshot: &sim::Snapshot) -> Vec<sim::SimId> {
    snapshot
        .entities
        .iter()
        .filter(|entity| matches!(entity.kind, sim::Kind::Object(ObjectKind::Creature, _)))
        .filter(|entity| entity.fighter.is_some())
        .map(|entity| entity.id)
        .collect()
}

/// Creature `id` turned to `faction`, as `ChangeToStandardFaction` turns
/// one (`Fighter::faction`).
fn turn(probe: &mut Probe, id: sim::SimId, faction: u8) -> Result<(), String> {
    probe.edit(id, |entity| {
        entity.fighter.as_mut().ok_or("no fighter")?.faction = Some(faction);
        Ok(())
    })
}

pub fn traps(discovery: &Discovery, game: &str, module: &str) -> ExitCode {
    let mut report = Report::default();
    let run = |report: &mut Report| -> Result<serde_json::Value, String> {
        let (game, ..) = open(discovery, game)?;
        let mut probe = Probe::own(discovery, game, module)?;
        let level = probe.world.level().clone();
        let data = level.module().ok_or("no module data")?;
        let library = level.scripts().cloned().ok_or("the module has no scripts")?;
        let mines: Vec<Mine> = level
            .placed()
            .iter()
            .enumerate()
            .filter(|(_, object)| object.kind == ObjectKind::Trigger && object.trap.is_some())
            .map(|(index, object)| (index as u32, object.clone()))
            .collect();
        let [first, second, third, fourth, ..] = mines.as_slice() else {
            return Err(format!("{module} has {} mines; four are needed", mines.len()));
        };
        let me = probe.me;
        let (start, _) = probe.step_told(2);
        // The base calmed: its creatures of the player's faction.
        let base = creatures(&start);
        let (kin, sith) = match base.as_slice() {
            [kin, sith, ..] => (*kin, *sith),
            _ => return Err(format!("{module} has fewer than two creatures")),
        };
        for id in &base {
            turn(&mut probe, *id, factions::PLAYER)?;
        }

        // 1. The facts.
        let (index, mine) = first;
        let trap = mine.trap.clone().ok_or("no trap")?;
        let row = data
            .traps
            .get(usize::from(trap.row))
            .cloned()
            .ok_or(format!("no traps.2da row {}", trap.row))?;
        report.check(
            trap.detect_dc > 0
                && trap.disarm_dc > 0
                && library.program(&row.script).is_some()
                && !row.kit.is_empty(),
            format!(
                "{} #{index}: traps.2da row {} ({}), DCs detect {} disarm {}, faction {}, its script {} in the library, its kit {}",
                mine.tag,
                trap.row,
                row.label,
                trap.detect_dc,
                trap.disarm_dc,
                mine.faction,
                row.script,
                row.kit
            ),
        );

        // 2. Found with Awareness.
        let unseen_at_start = mines.iter().all(|(index, _)| !seen(&start, *index));
        ranks(&mut probe, skills::AWARENESS, 6)?;
        near(&mut probe, mine, 8.0)?;
        let (found, told) = probe.step_told(2 * SECOND);
        let named = told.iter().any(|notice| matches!(notice, Notice::TrapSeen { .. }));
        let [x, y, _] = middle(mine);
        let far: Vec<u32> = mines
            .iter()
            .filter(|(_, other)| {
                let [ox, oy, _] = middle(other);
                (ox - x).hypot(oy - y) > sim::traps::DETECT_RANGE_M + 8.0 + sim::traps::RADIUS_M
            })
            .map(|(index, _)| *index)
            .collect();
        let far_unseen = far.iter().all(|index| !seen(&found, *index));
        report.check(
            unseen_at_start && seen(&found, *index) && named && far_unseen,
            format!(
                "none seen at the start ({unseen_at_start}); Awareness 6 from 8 m finds #{index} ({}), told {named}; {} past 20 m unseen ({far_unseen})",
                seen(&found, *index),
                far.len()
            ),
        );

        // 3. Disarmed.
        ranks(&mut probe, skills::DEMOLITIONS, 4)?;
        let reach = (outline(mine) + 0.4).min(sim::containers::REACH - 0.1);
        near(&mut probe, mine, reach)?;
        probe.step(4);
        let (_, mut told) = probe.act(Action::Disarm(*index));
        let (after, more) = probe.step_told(sim::traps::DISARM_MS / 8 + 10);
        told.extend(more);
        let check = check_of(&told, lines::DISARM_MINE);
        report.check(
            check.is_some_and(|(roll, total, dc, success)| {
                roll.is_none() && success == (20 + total >= i16::from(dc))
            }) && check.is_some_and(|(.., success)| success != armed(&after, *index)),
            format!("Demolitions 4 from {reach:.1} m, out of combat, takes 20: {check:?}; #{index} disarmed: {}", !armed(&after, *index)),
        );

        // 4. Recovered.
        let (index, mine) = second;
        near(&mut probe, mine, 8.0)?;
        let found = probe.step(2 * SECOND);
        ranks(&mut probe, skills::DEMOLITIONS, 12)?;
        let reach = (outline(mine) + 0.4).min(sim::containers::REACH - 0.1);
        near(&mut probe, mine, reach)?;
        probe.step(4);
        let (_, mut told) = probe.act(Action::Recover(*index));
        let (after, more) = probe.step_told(sim::traps::DISARM_MS / 8 + 10);
        told.extend(more);
        let check = check_of(&told, lines::RECOVER_MINE);
        let kit = after
            .goods
            .carried(sim::inventory::PARTY)
            .find(|(_, item)| item.template.eq_ignore_ascii_case(&row.kit))
            .map(|(id, _)| id);
        report.check(
            seen(&found, *index)
                && check.is_some_and(|(_, _, dc, success)| success && dc == trap.disarm_dc + 10)
                && !armed(&after, *index)
                && kit.is_some(),
            format!("Demolitions 12 recovers #{index} (DC {} + 10): {check:?}; the kit {} carried: {}", trap.disarm_dc, row.kit, kit.is_some()),
        );

        // 5. Sprung by the player; not by the trap's own faction.
        let (index, mine) = third;
        let before = (runs(&probe, &row.script), hp(&after, me));
        let at = middle(mine);
        // The gas's poison is a Fortitude save's (the row's DC): one the
        // player cannot make, so it takes hold.
        probe.edit_me(|me| {
            me.sheet.as_mut().ok_or("no sheet")?.saves[0] = -10;
            Ok(())
        })?;
        stand(&mut probe, mine, at[0], at[1])?;
        // Past the poison's first period.
        let (hurt, told) = probe.step_told(4 * SECOND);
        let ran = runs(&probe, &row.script) - before.0;
        let warned = told.contains(&Notice::Line(lines::MINE_TRIGGERED));
        let (was, now) = (before.1.unwrap_or(0), hp(&hurt, me).unwrap_or(0));
        let sick = poisoned(&hurt, me);
        report.check(
            ran >= 1 && warned && sick && now < was && !armed(&hurt, *index),
            format!(
                "the player (Fortitude −10: no save) walking into #{index} springs it: {} ran {ran} time(s), told {warned}, poisoned {sick}, vitality {was} → {now}, the one-shot gone: {}",
                row.script,
                !armed(&hurt, *index)
            ),
        );
        let (index, mine) = fourth;
        turn(&mut probe, kin, mine.faction)?;
        let before = runs(&probe, &row.script);
        walk_onto(&mut probe, kin, middle(mine))?;
        let spared = probe.step(SECOND);
        report.check(
            runs(&probe, &row.script) == before && armed(&spared, *index),
            format!("one of the trap's faction ({}) walking into #{index}: still armed", mine.faction),
        );

        // 6. The recovered kit laid where the player stands, away from the
        // others.
        let [x, y, _] = middle(second.1.as_ref());
        stand(&mut probe, second.1.as_ref(), x, y)?;
        // The poison cured and the fight it made over: out of combat.
        probe.edit_me(|me| {
            let fighter = me.fighter.as_mut().ok_or("no fighter")?;
            fighter.pack.poison = None;
            fighter.combat_until = 0;
            Ok(())
        })?;
        let mut state = probe.world.script_state().clone();
        state.make_item(library.items(), FRAG, sim::inventory::PARTY, 1, None);
        probe.world.set_script_state(state);
        probe.step(4);
        let (_, mut told) = probe.press(sim::Command {
            buttons: sim::buttons::LAY_MINE,
            ..sim::Command::default()
        });
        let (laid_now, more) = probe.step_told(sim::traps::LAY_MS / 8 + 10);
        told.extend(more);
        let check = check_of(&told, lines::SET_MINE);
        let instances = sim::Instances::in_snapshot(&level, &laid_now);
        let laid = laid_now
            .entities
            .iter()
            .find_map(|entity| match entity.kind {
                sim::Kind::Object(ObjectKind::Trigger, index) => instances
                    .made_from(index)
                    .and_then(|made| sim::traps::laid_row(&made.template))
                    .map(|row| (index, entity.position, row)),
                _ => None,
            })
            .ok_or("no mine laid")?;
        let stood_on = probe.step(SECOND);
        let kept_kit = carried(&stood_on, &row.kit) && !carried(&stood_on, FRAG);
        report.check(
            check.is_some_and(|(roll, .., success)| roll.is_none() && success)
                && laid.2 == 4
                && kept_kit
                && seen(&laid_now, laid.0)
                && armed(&stood_on, laid.0),
            format!(
                "O lays the deadlier kit: {check:?}, the party's mine #{} (row {}) at its feet, seen by the party, {} kept: {kept_kit}; the layer standing on it a second, it stays armed: {}",
                laid.0,
                laid.2,
                row.kit,
                armed(&stood_on, laid.0)
            ),
        );

        // 7. A save with it; a Sith walking onto it in the loaded world.
        let snapshot = probe.world.snapshot();
        let save = sim::Save {
            hz: sim::TICK_HZ,
            level: level.id(),
            snapshot: snapshot.clone(),
            script: probe.world.script_state().clone(),
        };
        let loaded = sim::Save::decode(&save.encode()).map_err(|e| e.to_string())?;
        let kept = loaded.snapshot == snapshot && armed(&loaded.snapshot, laid.0);
        probe.world =
            sim::SimWorld::from_snapshot(&loaded.snapshot, level.clone()).map_err(|e| e.to_string())?;
        probe.world.set_script_state(loaded.script);
        stand(&mut probe, first.1.as_ref(), x + 8.0, y)?;
        turn(&mut probe, sith, factions::HOSTILE_1)?;
        let before = (runs(&probe, &row.script), hp(&probe.world.snapshot(), sith));
        walk_onto(&mut probe, sith, laid.1)?;
        let sprung = probe.step(3 * SECOND);
        let ran = runs(&probe, &row.script) - before.0;
        let (was, now) = (before.1.unwrap_or(0), hp(&sprung, sith).unwrap_or(0));
        report.check(
            kept && ran >= 1 && now < was && !armed(&sprung, laid.0),
            format!(
                "a save carries the laid mine ({kept}); loaded, a hostile walking onto it springs it: {} ran {ran} time(s), vitality {was} → {now}",
                row.script
            ),
        );

        // 8. Faults.
        let faults: Vec<String> = probe
            .world
            .script_report()
            .faults
            .iter()
            .filter(|fault| fault.contains(&row.script))
            .cloned()
            .collect();
        report.check(
            faults.is_empty(),
            format!("{} ran without faults ({faults:?})", row.script),
        );
        Ok(json!({
            "module": module,
            "mines": mines.len(),
            "row": trap.row,
            "script": row.script,
            "kit": row.kit,
        }))
    };
    match run(&mut report) {
        Ok(facts) => report.finish(facts),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
