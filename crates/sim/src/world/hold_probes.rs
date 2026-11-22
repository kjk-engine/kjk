//! The probes of C11's holds (`kjk sim probes --only holds`;
//! `ForceState::hold`), no install:
//! - a combat hold lasts its own duration, up to `ja::LONGEST_MS`, and first
//!   rolls its save `ja::ROUND_MS` after it starts;
//! - a story hold and kjk's own hold last at most `ja::STUN_MS` and roll no
//!   saves;
//! - the second and third recent holds are shorter, never under a round,
//!   and a fourth is refused without changing anything;
//! - a hold while another runs is refused and does not count as recent;
//! - an old snapshot's running hold keeps its end and rolls no saves;
//! - a held target rolls its save each round (`ForceState::roll_free`),
//!   never before the first round, each roll with `ja::STRUGGLE_BONUS` more,
//!   a natural 20 always breaking free and a natural 1 never;
//! - a fear that ends on damage rolls no more and still counts as recent;
//! - the step rolls a player's due save, tells its seat and floats "Saved";
//! - a script's hold rolls the save the script's target failed against its
//!   caster, else the caster's power DC (Fortitude for a stun, Will for
//!   sleep); a permanent hold, and one by a creator not hostile to the
//!   target, holds as a story's;
//! - the adhesive grenade's entangle, which allows no save, holds its whole
//!   time; a weapon's on-hit stun rolls Will against the property's DC;
//!   Force Whirlwind rolls Reflex against the caster's power DC;
//! - a fourth recent hold from a script is refused and "Immune" floats;
//! - covering the ears after a combat hold ran out rolls no old save;
//! - the anti-goal measure (`full_length_shares`): how often the design's
//!   two examples go their full length.

use fmt_odyssey::ncs::{Const, Ncs, Op};
use script_ncs::probes::Asm;
use script_ncs::vm::ObjectId;

use super::{Fighting, Id, SimWorld};
use crate::combat::force::{ForceState, HoldKind, HoldSave, ja};
use crate::combat::sheet::factions;
use crate::combat::{Fighter, Sheet};
use crate::input::{Command, Seat, TickInput};
use crate::module::{ObjectData, ObjectKind, Placement, Repute};
use crate::probe::{Checks, check};
use crate::script::effects::{Applied, Effect, Hold};
use crate::snapshot::SimId;
use crate::{StepReason, WorldConfig};

const MSEC: u32 = 50;

const SAVE: HoldSave = HoldSave { index: 0, dc: 20 };

fn combat() -> HoldKind {
    HoldKind::Combat(None)
}

/// A fresh character held by `holds`, each `(start, ms)` in turn: whether
/// each took hold, and the state after.
fn held(holds: &[(u64, u64)]) -> (Vec<bool>, ForceState) {
    let mut force = ForceState::default();
    let took = holds
        .iter()
        .map(|&(now, ms)| force.hold(now, ms, combat()))
        .collect();
    (took, force)
}

fn own_duration() -> Result<String, String> {
    let mut force = ForceState::default();
    let took = force.hold(0, 9_000, HoldKind::Combat(Some(SAVE)));
    check(
        took && force.stunned_until == 9_000
            && force.save == Some(SAVE)
            && force.save_due == ja::ROUND_MS,
        format!(
            "a 9 s stun held until {} ms, its first save due at {} ms",
            force.stunned_until, force.save_due
        ),
    )
}

fn longest() -> Result<String, String> {
    let (_, force) = held(&[(0, 100 * ja::LONGEST_MS)]);
    check(
        force.stunned_until == ja::LONGEST_MS,
        format!("a very long hold ended at {} ms", force.stunned_until),
    )
}

fn story_and_own() -> Result<String, String> {
    let seen: Vec<_> = [HoldKind::Story, HoldKind::Own]
        .into_iter()
        .map(|kind| {
            let mut force = ForceState::default();
            force.hold(0, 9_000, kind);
            (
                kind,
                force.stunned_until,
                force.immune_until,
                force.save,
                force.recent,
            )
        })
        .collect();
    let ok = seen.iter().all(|&(kind, until, immune, save, recent)| {
        until == ja::STUN_MS
            && immune == ja::STUN_MS + ja::IMMUNE_MS
            && save.is_none()
            && recent == u8::from(kind == HoldKind::Own)
    });
    check(ok, format!("(kind, until, immune, save, recent): {seen:?}"))
}

fn repeated() -> Result<String, String> {
    let (took, force) = held(&[
        (0, 12_000),
        (13_000, 12_000),
        (20_000, 12_000),
        (24_000, 12_000),
    ]);
    let (again, after) = {
        let mut force = force;
        (force.hold(33_000, 12_000, combat()), force)
    };
    let second = held(&[(0, 12_000), (13_000, 12_000)]).1.stunned_until;
    check(
        took == [true, true, true, false]
            && second == 19_000
            && force.stunned_until == 23_000
            && again
            && after.stunned_until == 45_000,
        format!(
            "12 s holds took {took:?}; the second ended at {second} ms, the third at {} ms; \
             10 s after the last ended one took again ({again}) and ended at {} ms",
            force.stunned_until, after.stunned_until
        ),
    )
}

fn never_below_a_round() -> Result<String, String> {
    let four = held(&[(0, 4_000), (5_000, 4_000)]).1.stunned_until;
    let two = held(&[(0, 2_000), (3_000, 2_000)]).1.stunned_until;
    check(
        four == 8_000 && two == 5_000,
        format!(
            "a second 4 s hold ended at {four} ms (at least a round), a second 2 s hold at \
             {two} ms (its whole time)"
        ),
    )
}

fn refused_changes_nothing() -> Result<String, String> {
    let (_, before) = held(&[(0, 3_000), (4_000, 3_000), (8_000, 3_000)]);
    let mut after = before;
    let took = after.hold(12_000, 3_000, combat());
    check(
        !took && after == before,
        format!(
            "a fourth recent hold took ({took}); the state changed: {}",
            after != before
        ),
    )
}

fn while_held() -> Result<String, String> {
    let (took, force) = held(&[(0, 9_000), (0, 3_000)]);
    check(
        took == [true, false] && force.stunned_until == 9_000 && force.recent == 1,
        format!(
            "holds took {took:?}, the hold ends at {} ms, recent holds {}",
            force.stunned_until, force.recent
        ),
    )
}

/// A running hold encoded as this build does, then read back as the
/// snapshot before `FEAT_STUN_SAVES`, which ends before the new fields.
fn old_snapshot() -> Result<String, String> {
    use crate::wire::{Reader, Versions, Writer};
    let mut force = ForceState::default();
    force.hold(0, 9_000, HoldKind::Combat(Some(SAVE)));
    let mut new = Writer::default();
    force.encode(&mut new);
    let mut bare = force;
    bare.save = None;
    let mut old = Writer::default();
    bare.encode(&mut old);
    // The new fields' bytes past the old layout: a `None` save's flag and
    // blank, the next roll, the rolls, the recent count and its end.
    let tail = 1 + 1 + 4 + 8 + 1 + 1 + 8;
    let bytes = old.0;
    let bytes = &bytes[..bytes.len() - tail];
    let since = crate::since::snapshot::FEAT_STUN_SAVES;
    let versions = Versions {
        snapshot: since.1 - 1,
        ..Versions::THIS_BUILD
    };
    let mut input = Reader::new("snapshot", bytes).with_versions(versions);
    let read = ForceState::decode(&mut input).map_err(|error| error.to_string())?;
    let mut current = Reader::new("snapshot", &new.0);
    let again = ForceState::decode(&mut current).map_err(|error| error.to_string())?;
    check(
        read.stunned_until == 9_000
            && read.save.is_none()
            && (read.save_due, read.rolls, read.recent, read.recent_until) == (0, 0, 0, 0)
            && again == force,
        format!(
            "an old snapshot's hold ends at {} ms with save {:?}, next roll {}, recent {}; \
             this build's reads back the same: {}",
            read.stunned_until,
            read.save,
            read.save_due,
            read.recent,
            again == force
        ),
    )
}

/// A hold of `ms` with a save of `total` against `dc`, rolled with `d20`
/// each step of `MSEC` until `until`: when it ended, and the struggle bonus
/// of each roll made.
fn struggled(ms: u64, (total, dc): (i32, i32), d20: i32, until: u64) -> (u64, Vec<i32>) {
    let mut force = ForceState::default();
    force.hold(0, ms, HoldKind::Combat(Some(HoldSave { index: 0, dc })));
    let mut bonuses = Vec::new();
    for now in (0..until).step_by(MSEC as usize) {
        if let Some(rolled) = force.roll_free(now, || d20, |_| total) {
            bonuses.push(rolled.total - total);
        }
    }
    (force.stunned_until, bonuses)
}

fn low_save() -> Result<String, String> {
    let (end, bonuses) = struggled(9_000, (0, 40), 19, 12_000);
    check(
        end == 9_000 && bonuses.len() == 2,
        format!(
            "a 9 s stun on a save of +0 against DC 40 ended at {end} ms after rolls {bonuses:?}"
        ),
    )
}

fn high_save() -> Result<String, String> {
    let (end, bonuses) = struggled(9_000, (40, 10), 2, 12_000);
    check(
        end == ja::ROUND_MS && bonuses.len() == 1,
        format!(
            "a 9 s stun on a save of +40 against DC 10 ended at {end} ms after rolls {bonuses:?}"
        ),
    )
}

fn bonus_grows() -> Result<String, String> {
    let (_, bonuses) = struggled(12_000, (0, 100), 19, 12_000);
    let bonus = ja::STRUGGLE_BONUS;
    check(
        bonuses == [0, bonus, 2 * bonus],
        format!("a 12 s stasis's rolls added {bonuses:?}"),
    )
}

fn natural_rolls() -> Result<String, String> {
    let (twenty, _) = struggled(9_000, (-100, 100), 20, 12_000);
    let (one, _) = struggled(9_000, (100, 0), 1, 12_000);
    check(
        twenty == ja::ROUND_MS && one == 9_000,
        format!(
            "a natural 20 ended a hopeless hold at {twenty} ms; a natural 1 left a sure one to {one} ms"
        ),
    )
}

fn fear_damage() -> Result<String, String> {
    let mut force = ForceState::default();
    force.hold(
        0,
        12_000,
        HoldKind::Combat(Some(HoldSave { index: 2, dc: 20 })),
    );
    force.release(1_000);
    let later = force.due(ja::ROUND_MS);
    check(
        force.stunned_until == 1_000
            && force.immune_until == 1_000 + ja::IMMUNE_MS
            && later.is_none()
            && force.recent == 1
            && force.recent_until == 1_000 + ja::RECENT_MS,
        format!(
            "a 12 s fear damaged at 1 s ended at {} ms, immune until {}, a roll due later {later:?}, \
             recent holds {} until {} ms",
            force.stunned_until, force.immune_until, force.recent, force.recent_until
        ),
    )
}

/// The proving ground with one player: held at the first step by a hold
/// it breaks unless its d20 is a natural 1; when the hold ended, what its
/// seat was told and what floated over it.
fn stepped() -> Result<String, String> {
    let ground = crate::determinism::proving_ground();
    let mut world =
        SimWorld::new(WorldConfig::one_seat(), ground).map_err(|error| error.to_string())?;
    let step = |world: &mut SimWorld| {
        let mut input = TickInput::default();
        input.insert(Seat(0), Command::default());
        let tick = world.tick() + 1;
        crate::step(world, tick, &input, MSEC, StepReason::Authority)
    };
    let start = step(&mut world);
    let player = start.body(Seat(0)).ok_or("no player")?.id;
    let held = start.time_ms;
    let save = HoldSave { index: 0, dc: -100 };
    for (id, mut fighting) in world
        .world
        .query::<(&Id, &mut Fighting)>()
        .iter_mut(&mut world.world)
    {
        if id.0 == player {
            fighting
                .0
                .force
                .hold(held, 9_000, HoldKind::Combat(Some(save)));
        }
    }
    let mut ended = None;
    let mut told = Vec::new();
    let mut floated = Vec::new();
    for _ in 0..(9_000 / u64::from(MSEC)) {
        let snapshot = step(&mut world);
        told.extend(world.take_notices());
        floated.extend(
            snapshot
                .progress
                .feedback
                .iter()
                .filter(|f| f.over == player.0)
                .map(|f| f.at_ms),
        );
        let fighter = snapshot.get(player).and_then(|entity| entity.fighter);
        if ended.is_none() && fighter.is_some_and(|f| !f.force.stunned(snapshot.time_ms)) {
            ended = Some(snapshot.time_ms - held);
        }
    }
    let saves = told
        .iter()
        .filter(|(seat, notice)| {
            *seat == Seat(0) && matches!(notice, crate::notice::Notice::Save { success: true, .. })
        })
        .count();
    floated.dedup();
    check(
        ended == Some(ja::ROUND_MS) && saves == 1 && floated.len() == 1,
        format!(
            "the player's hold ended {ended:?} ms after it began; its seat was told {saves} saves \
             that broke free; feedback floated over it at {floated:?}"
        ),
    )
}

/// The routines the scripted hold's script calls, each after its number.
const CATALOG: &str = r#"
// 0
int WillSave(object oCreature, int nDC, int nSaveType = 0, object oSaveVersus = OBJECT_SELF);
// 1
effect EffectStunned();
// 2
void ApplyEffectToObject(int nDurationType, effect eEffect, object oTarget, float fDuration = 0.0f);
// 3
object GetFirstPC();
"#;

/// The DC of the scripted Will save the player fails.
const SCRIPT_DC: i32 = 22;

/// A script that asks the first PC for a Will save against `SCRIPT_DC`
/// and then stuns it for 9 seconds.
fn stuns() -> Ncs {
    let call = |routine: u16, args: u8| Op::Action { routine, args };
    let mut asm = Asm::default();
    asm.op(Op::Const(Const::Object(0)))
        .op(Op::Const(Const::Int(0)))
        .op(Op::Const(Const::Int(SCRIPT_DC)))
        .op(call(3, 0))
        .op(call(0, 4))
        .op(Op::Const(Const::Float(9.0)))
        .op(call(3, 0))
        .op(call(1, 0))
        .op(Op::Const(Const::Int(1)))
        .op(call(2, 4))
        .op(Op::Retn);
    asm.build()
}

/// The hostile Dark Jedi's sheet.
fn foe_sheet() -> Sheet {
    Sheet {
        faction: factions::HOSTILE_1,
        ..Sheet::dev_jedi()
    }
}

/// The proving ground with the player, whose Will save always fails, and
/// a hostile Dark Jedi: the world, the player and the foe.
fn arena() -> Result<(SimWorld, SimId, SimId), String> {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    data.repute = Repute::at_war([factions::PLAYER, factions::HOSTILE_1]);
    let entry = data.entry.position;
    data.placements.push(Placement::Own(ObjectData {
        sheet: Some(foe_sheet()),
        template: "probe_foe".into(),
        ..ObjectData::bare(
            ObjectKind::Creature,
            "probe_foe",
            [entry[0] + 6.0, entry[1] + 6.0, entry[2]],
        )
    }));
    let library = crate::probe::library(CATALOG, [("probe_stuns", stuns())]);
    let mut player = Sheet::dev_soldier();
    player.save_bonus = [i8::MIN; 3];
    let config = WorldConfig {
        sheet: player,
        ..WorldConfig::one_seat()
    };
    let mut world = SimWorld::new(config, ground.with_module(data).with_scripts(library))
        .map_err(|error| error.to_string())?;
    let first = step(&mut world);
    let player = first.body(Seat(0)).ok_or("no player")?.id;
    let foe = first
        .entities
        .iter()
        .find(|entity| entity.id != player && entity.fighter.is_some())
        .ok_or("no foe")?
        .id;
    Ok((world, player, foe))
}

fn step(world: &mut SimWorld) -> crate::snapshot::Snapshot {
    let mut input = TickInput::default();
    input.insert(Seat(0), Command::default());
    let tick = world.tick() + 1;
    crate::step(world, tick, &input, MSEC, StepReason::Authority)
}

/// `who`'s fighter after `steps` steps.
fn after(world: &mut SimWorld, who: SimId, steps: usize) -> Option<(u64, Fighter)> {
    let mut last = step(world);
    for _ in 1..steps {
        last = step(world);
    }
    Some((last.time_ms, last.get(who)?.fighter?))
}

/// `hold` put on `on` by `by` for `ms` (`u64::MAX`: permanently), as a
/// script applies it with no save asked: the step it took hold in, and the
/// fighter after it.
/// The player by itself when `own`, else by the foe.
fn put(own: bool, hold: Hold, ms: u64) -> Result<(u64, Fighter), String> {
    let (mut world, on, foe) = arena()?;
    let by = if own { on } else { foe };
    let now = world.snapshot().time_ms;
    let mut state = world.script_state().clone();
    state.applied.push(Applied {
        target: ObjectId(on.0),
        by: ObjectId(by.0),
        effect: Effect::Hold(hold),
        until: Some(if ms == u64::MAX { ms } else { now + ms }),
        spell: u16::MAX,
        id: 1,
        at: None,
        save: None,
        laid: false,
    });
    world.set_script_state(state);
    let (took, fighter) = after(&mut world, on, 1).ok_or("no fighter")?;
    Ok((took, fighter))
}

fn scripted_save() -> Result<String, String> {
    let (mut world, player, foe) = arena()?;
    let mut state = world.script_state().clone();
    state
        .pending
        .push(crate::script::Pending::new("probe_stuns", ObjectId(foe.0)));
    world.set_script_state(state);
    let (now, fighter) = after(&mut world, player, 2).ok_or("no player")?;
    let force = fighter.force;
    let left = force.stunned_until.saturating_sub(now);
    check(
        force.save
            == Some(HoldSave {
                index: 2,
                dc: SCRIPT_DC,
            })
            && left > 8_000,
        format!(
            "the script's stun after a failed Will save at DC {SCRIPT_DC} rolls {:?}, {left} ms left",
            force.save
        ),
    )
}

fn power_dc() -> Result<String, String> {
    let dc = crate::combat::force::save_dc(&foe_sheet());
    let mut seen = Vec::new();
    for (hold, index) in [(Hold::Stun, 0), (Hold::Whirlwind, 1), (Hold::Sleep, 2)] {
        let save = put(false, hold, 9_000)?.1.force.save;
        seen.push((hold, save, save == Some(HoldSave { index, dc })));
    }
    check(
        seen.iter().all(|(.., ok)| *ok),
        format!("with no save asked, the holds roll {seen:?} (the foe's power DC {dc})"),
    )
}

fn story() -> Result<String, String> {
    let (at, permanent) = put(false, Hold::Stun, u64::MAX)?;
    let (own, own_fighter) = put(true, Hold::Stun, 9_000)?;
    let ok = |since: u64, fighter: &Fighter| {
        fighter.force.stunned_until == since + ja::STUN_MS
            && fighter.force.save.is_none()
            && fighter.force.recent == 0
    };
    check(
        ok(at, &permanent) && ok(own, &own_fighter),
        format!(
            "a permanent hold by a foe: {:?}; a 9 s hold by the player itself: {:?} \
             (until, save, recent; from the hold's start)",
            (
                permanent.force.stunned_until - at,
                permanent.force.save,
                permanent.force.recent
            ),
            (
                own_fighter.force.stunned_until - own,
                own_fighter.force.save,
                own_fighter.force.recent
            ),
        ),
    )
}

fn adhesive() -> Result<String, String> {
    use crate::combat::grenades;
    let row = crate::combat::spells::ITEM_ABILITY_GRENADE_ADHESIVE;
    let effect = grenades::effect(row).ok_or("no adhesive")?;
    let mut fighter = Fighter::of(&Sheet::dev_soldier());
    grenades::afflict(&mut fighter, effect.secondary, effect.dc, SimId(1), 0);
    let whole = match effect.secondary {
        grenades::Secondary::Held(ms) => ms,
        _ => 0,
    };
    check(
        whole > 0
            && fighter.force.stunned_until == whole.min(ja::LONGEST_MS)
            && fighter.force.save.is_none(),
        format!(
            "the adhesive grenade holds until {} ms of its {whole} ms, rolling {:?}",
            fighter.force.stunned_until, fighter.force.save
        ),
    )
}

fn on_hit() -> Result<String, String> {
    use crate::combat::gear::{self, on_hit};
    let mut weapon = Sheet::dev_soldier().main_weapon();
    weapon.on_hit = [
        Some(crate::combat::gear::OnHit {
            kind: on_hit::STUN,
            chance: 100,
            seconds: 6,
            dc: 15,
            param: 0,
        }),
        None,
    ];
    let mut target = Sheet::dev_soldier();
    target.save_bonus = [i8::MIN; 3];
    let fighter = Fighter::of(&target);
    let mut rng = crate::rng::SimRng::new(1, 0);
    let struck = gear::on_hits(&weapon, &target, &fighter, (0, 1000), &mut rng);
    let save = struck.iter().find_map(|struck| match struck {
        gear::Struck::Hold { save, .. } => Some(*save),
        _ => None,
    });
    check(
        save == Some(HoldSave { index: 2, dc: 15 }),
        format!("a weapon's on-hit stun at DC 15 rolls {save:?}"),
    )
}

/// Four 3 s stuns from the foe, each once the last one's immunity ended:
/// the fourth is refused, and "Immune" floats over the player then.
fn fourth_refused() -> Result<String, String> {
    let (mut world, player, foe) = arena()?;
    let mut took = Vec::new();
    let mut immune = Vec::new();
    for _ in 0..4 {
        let now = world.snapshot().time_ms;
        let mut state = world.script_state().clone();
        state.applied.push(Applied {
            target: ObjectId(player.0),
            by: ObjectId(foe.0),
            effect: Effect::Hold(Hold::Stun),
            until: Some(now + ja::ROUND_MS),
            spell: u16::MAX,
            id: 1,
            at: None,
            save: None,
            laid: false,
        });
        world.set_script_state(state);
        let snapshot = step(&mut world);
        let fighter = snapshot
            .get(player)
            .and_then(|entity| entity.fighter)
            .ok_or("no player")?;
        took.push(fighter.force.stunned(snapshot.time_ms));
        immune.push(
            snapshot
                .progress
                .feedback
                .iter()
                .any(|f| f.over == player.0 && f.at_ms == snapshot.time_ms),
        );
        let wait = (ja::ROUND_MS + ja::IMMUNE_MS) / u64::from(MSEC);
        for _ in 0..wait {
            step(&mut world);
        }
    }
    check(
        took == [true, true, true, false] && immune == [false, false, false, true],
        format!("four stuns took {took:?}; feedback floated over the player at each: {immune:?}"),
    )
}

/// A 9 s stun whose two rolls failed runs out, then the character covers
/// its ears: no save is due while it covers them.
fn ears_after_hold() -> Result<String, String> {
    let mut force = ForceState::default();
    force.hold(
        0,
        9_000,
        HoldKind::Combat(Some(HoldSave { index: 0, dc: 100 })),
    );
    for now in (0..9_000).step_by(MSEC as usize) {
        force.roll_free(now, || 19, |_| 0);
    }
    let covered = force.cover_ears(20_000);
    let due = force.due(20_000 + u64::from(MSEC));
    check(
        covered && due.is_none(),
        format!("the ears covered ({covered}) after a stun ran out; a roll due then: {due:?}"),
    )
}

/// How many of `count` staged holds of `ms`, on a save of `save` against
/// `dc`, went their full length, in percent.
fn full_share(ms: u64, (save, dc): (i32, i32), count: u32) -> f64 {
    let mut rng = crate::rng::SimRng::new(19, 11);
    let full = (0..count)
        .filter(|_| {
            let mut force = ForceState::default();
            force.hold(0, ms, HoldKind::Combat(Some(HoldSave { index: 0, dc })));
            for now in (0..ms).step_by(MSEC as usize) {
                let d20 = || rng.below(rules::interaction::checks::DIE) as i32 + 1;
                force.roll_free(now, d20, |_| save);
            }
            force.stunned_until == ms
        })
        .count();
    full as f64 * 100.0 / f64::from(count)
}

/// The design's anti-goal measure: a level 8 player with Fortitude +6
/// stunned for 9 s at DC 22, and a boss with Fortitude +10 in a 12 s
/// Stasis at DC 23, 2000 holds each. The design expects about 37% and 2%
/// of them to go their full length.
fn full_length_shares() -> Result<String, String> {
    let player = full_share(9_000, (6, 22), 2000);
    let boss = full_share(12_000, (10, 23), 2000);
    check(
        (player - 37.5).abs() <= 5.0 && (boss - 2.1).abs() <= 5.0,
        format!(
            "the player's 9 s stun went its full length {player:.1}% of the time, the boss's 12 s \
             stasis {boss:.1}%"
        ),
    )
}

pub fn probes() -> Checks {
    vec![
        ("combat_hold_lasts_its_duration", own_duration()),
        ("combat_hold_stops_at_longest", longest()),
        ("story_and_own_holds_keep_todays_cap", story_and_own()),
        ("repeated_holds_shorten_then_refuse", repeated()),
        ("shortened_hold_never_below_a_round", never_below_a_round()),
        ("refused_hold_changes_nothing", refused_changes_nothing()),
        ("second_hold_while_held_is_refused", while_held()),
        ("old_snapshot_hold_rolls_no_saves", old_snapshot()),
        ("low_save_holds_full", low_save()),
        ("high_save_breaks_after_a_round", high_save()),
        ("struggle_bonus_grows", bonus_grows()),
        ("fear_ends_on_damage_and_counts", fear_damage()),
        ("natural_rolls", natural_rolls()),
        ("step_rolls_a_due_save", stepped()),
        ("scripted_stun_uses_its_save", scripted_save()),
        ("scripted_hold_without_save_uses_power_dc", power_dc()),
        ("permanent_or_friendly_hold_is_story", story()),
        ("adhesive_holds_its_whole_time", adhesive()),
        ("on_hit_stun_uses_will", on_hit()),
        ("fourth_recent_hold_shows_immune", fourth_refused()),
        ("ears_after_a_hold_roll_nothing", ears_after_hold()),
        ("full_length_shares", full_length_shares()),
    ]
}
