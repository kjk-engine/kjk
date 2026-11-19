//! The probes of C11's holds (`kjk sim probes --only holds`;
//! `ForceState::hold`), no install:
//! - a combat hold lasts its own duration, up to `ja::LONGEST_MS`, and first
//!   rolls its save `ja::ROUND_MS` after it starts;
//! - a story hold and kjk's own hold last at most `ja::STUN_MS` and roll no
//!   saves;
//! - the second and third recent holds are shorter, never under a round,
//!   and a fourth is refused without changing anything;
//! - a hold while another runs is refused and does not count as recent;
//! - an old snapshot's running hold keeps its end and rolls no saves.

use crate::combat::force::{ForceState, HoldKind, HoldSave, ja};
use crate::probe::{Checks, check};

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
    ]
}
