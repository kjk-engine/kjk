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
//! - the step rolls a player's due save, tells its seat and floats "Saved".

use super::{Fighting, Id, SimWorld};
use crate::combat::force::{ForceState, HoldKind, HoldSave, ja};
use crate::input::{Command, Seat, TickInput};
use crate::probe::{Checks, check};
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
    ]
}
