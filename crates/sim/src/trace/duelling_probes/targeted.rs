//! The probes of targeted blocks (`kjk sim probes --only targeted`). The
//! session's flag survives a snapshot, an older snapshot reads it as off, and
//! the world turns targeted blocks on only under the Advanced rules with the
//! flag set.

use crate::combat::saber::SaberRules;
use crate::determinism::{Run, Setup};
use crate::probe::{Checks, check};
use crate::snapshot::{Snapshot, decode_targeted};
use crate::wire::{Reader, Versions};
use crate::{Level, SimWorld};

pub fn probes() -> Checks {
    vec![
        ("flag_round_trip", flag_round_trip()),
        ("old_snapshot_off", old_snapshot_off()),
        ("ja_ignores_flag", ja_ignores_flag()),
    ]
}

/// A snapshot of a new world in the placeholder level, under `rules`, with
/// the flag at `targeted`.
fn snapshot_with(rules: SaberRules, targeted: bool) -> Snapshot {
    let mut snapshot = Run::new(&Setup::pinned()).last;
    snapshot.saber_rules = rules;
    snapshot.targeted_blocks = targeted;
    snapshot
}

fn flag_round_trip() -> Result<String, String> {
    let held = snapshot_with(SaberRules::DuelAdvanced, true);
    let again = Snapshot::decode(&held.encode()).map_err(|e| e.to_string())?;
    check(
        again == held,
        format!("read back targeted_blocks = {}", again.targeted_blocks),
    )
}

fn old_snapshot_off() -> Result<String, String> {
    let before = crate::since::snapshot::FEAT_TARGETED_BLOCKS.1 - 1;
    let versions = Versions {
        snapshot: before,
        ..Versions::THIS_BUILD
    };
    let mut read = Reader::new("targeted", &[]).with_versions(versions);
    let older = decode_targeted(&mut read).map_err(|e| e.to_string())?;
    check(
        !older,
        format!("from snapshot version {before} targeted_blocks = {older}"),
    )
}

/// The world's answer for every set of rules and both values of the flag:
/// only the Advanced rules with the flag set turn targeted blocks on.
fn ja_ignores_flag() -> Result<String, String> {
    let cases = [
        (SaberRules::Ja, false, false),
        (SaberRules::Ja, true, false),
        (SaberRules::DuelAdvanced, false, false),
        (SaberRules::DuelAdvanced, true, true),
    ];
    let mut seen = Vec::new();
    let mut right = true;
    for (rules, flag, want) in cases {
        let world = SimWorld::from_snapshot(&snapshot_with(rules, flag), Level::arena())
            .map_err(|e| e.to_string())?;
        let got = world.targeted();
        right &= got == want;
        seen.push(format!("{} with the flag {flag}: {got}", rules.name()));
    }
    check(right, seen.join("; "))
}
