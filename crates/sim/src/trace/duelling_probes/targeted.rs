//! The probes of targeted blocks (`kjk sim probes --only targeted`). The
//! session's flag survives a snapshot, an older snapshot reads it as off, and
//! the world turns targeted blocks on only under the Advanced rules with the
//! flag set. The sides (`duel::aim`) follow the spec's tables: the side of
//! a swing by its start quadrant and the two fighters' facings, the side of
//! the crosshair by its offsets from the chest, and the crosshair line
//! agrees with the screen of every camera.

use crate::combat::mind;
use crate::combat::saber::duel::{self, aim, aim::Side};
use crate::combat::saber::{Move, Quad, SaberRules};
use crate::determinism::{Run, Setup};
use crate::math::{self, Vec3};
use crate::motion::MotionState;
use crate::probe::{Checks, check};
use crate::snapshot::{Snapshot, decode_targeted};
use crate::wire::{Reader, Versions};
use crate::{Level, SimWorld};
use collision::UNIT;

pub fn probes() -> Checks {
    vec![
        ("flag_round_trip", flag_round_trip()),
        ("old_snapshot_off", old_snapshot_off()),
        ("ja_ignores_flag", ja_ignores_flag()),
        ("swing_side_table", swing_side_table()),
        ("crosshair_side_table", crosshair_side_table()),
        ("aimed_table", aimed_table()),
        (
            "crosshair_line_matches_shots",
            crosshair_line_matches_shots(),
        ),
    ]
}

/// Passes when every row's result equals its expected value, and lists
/// the rows that differ.
fn table<R: PartialEq + std::fmt::Debug>(rows: Vec<(String, R, R)>) -> Result<String, String> {
    let wrong: Vec<String> = rows
        .iter()
        .filter(|(_, got, want)| got != want)
        .map(|(name, got, want)| format!("{name}: {got:?}, expected {want:?}"))
        .collect();
    match wrong.is_empty() {
        true => Ok(format!("{} rows right", rows.len())),
        false => Err(wrong.join("; ")),
    }
}

/// A move that starts at `quad` and that an ordinary block can stop.
fn blockable_from(quad: Quad) -> Option<Move> {
    Move::ALL
        .iter()
        .copied()
        .find(|&mv| mv.data().start == quad && !duel::unblockable(mv))
}

/// The swing sides of the start quadrants with the two fighters facing each
/// other, with the attacker turned side-on, and with the attacker behind
/// the blocker, and of every move only a perfect block stops.
fn swing_side_table() -> Result<String, String> {
    use Quad::*;
    let (left, right, above) = (Some(Side::Left), Some(Side::Right), Some(Side::Above));
    let facing = [
        (T, above),
        (TL, right),
        (L, right),
        (BL, right),
        (TR, left),
        (R, left),
        (BR, left),
        (B, None),
    ];
    let mut rows = Vec::new();
    let mut quad_row = |label: &str, quad: Quad, attacker: f32, blocker: f32, want| {
        let got = blockable_from(quad).map(|mv| aim::swing_side(mv, attacker, blocker));
        rows.push((format!("{label} {quad:?}"), got, Some(want)));
    };
    for (quad, want) in facing {
        quad_row("facing", quad, 0.0, 180.0, want);
        for turn in [90.0, -90.0] {
            let side_on = if quad == T { above } else { None };
            quad_row(&format!("turned {turn}"), quad, turn, 180.0, side_on);
        }
    }
    quad_row("behind", L, 0.0, 0.0, left);
    for &mv in Move::ALL.iter().filter(|&&mv| duel::unblockable(mv)) {
        rows.push((
            format!("unblockable {}", mv.name()),
            Some(aim::swing_side(mv, 0.0, 180.0)),
            Some(None),
        ));
    }
    table(rows)
}

/// The crosshair sides of a line turned by (right, up) degrees off the way
/// to a chest 1.5 meters ahead. A positive right turn puts the crosshair to
/// the right of the chest on the screen.
fn crosshair_side_table() -> Result<String, String> {
    let (left, right, above) = (Some(Side::Left), Some(Side::Right), Some(Side::Above));
    let cases = [
        (0.0, 0.0, None),
        (2.0, 2.0, None),
        (10.0, 0.0, right),
        (-10.0, 0.0, left),
        (0.0, 10.0, above),
        (6.0, 8.0, above),
        (8.0, 6.0, right),
        (-8.0, 6.0, left),
        (0.0, -10.0, None),
        (2.9, 0.0, None),
        (3.0, 0.0, right),
        (-3.0, 0.0, left),
    ];
    let chest = [1.5 / UNIT, 0.0, 0.0];
    let rows = cases
        .into_iter()
        .map(|(right_deg, up_deg, want)| {
            let (way, ..) = math::angle_vectors([-up_deg, -right_deg, 0.0]);
            let got = aim::crosshair_side([0.0; 3], way, chest);
            (format!("({right_deg}, {up_deg})"), got, want)
        })
        .collect();
    table(rows)
}

fn aimed_table() -> Result<String, String> {
    use Side::*;
    let mut cases: Vec<(Option<Side>, Option<Side>, bool)> =
        [None, Some(Left), Some(Right), Some(Above)]
            .into_iter()
            .map(|crosshair| (None, crosshair, true))
            .collect();
    cases.extend([
        (Some(Left), Some(Left), true),
        (Some(Left), Some(Right), false),
        (Some(Left), None, false),
        (Some(Above), Some(Above), true),
    ]);
    let rows = cases
        .into_iter()
        .map(|(swing, crosshair, want)| {
            (
                format!("{swing:?} {crosshair:?}"),
                aim::aimed(swing, crosshair),
                want,
            )
        })
        .collect();
    table(rows)
}

/// The side seen from a camera at `camera` looking along `view`: where the
/// crosshair, the middle of the screen, is against the chest's place on it.
fn screen_side(camera: Vec3, view: Vec3, chest: Vec3) -> Option<Side> {
    let (forward, right, up) = math::angle_vectors(view);
    let to = math::sub(chest, camera);
    let ahead = math::dot(to, forward);
    aim::side_of_offsets(
        -math::atan2_deg(math::dot(to, right), ahead),
        -math::atan2_deg(math::dot(to, up), ahead),
    )
}

/// For the third-person camera, the shoulder camera on each side and first
/// person, each looking some way off an attacker 1.5 meters ahead, the side
/// of the crosshair line (`aim::crosshair_line`) equals the side seen on
/// the camera's screen.
fn crosshair_line_matches_shots() -> Result<String, String> {
    let me = MotionState::standing([0.0, 0.0, 24.0], 0.0);
    let them = [1.5 / UNIT, 0.0, me.origin[2]];
    let chest = aim::chest(them, me.eye_height);
    let level = math::sub(chest, mind::eye(&me));
    let at_chest = -math::atan2_deg(level[2], level[0]);
    let looks = [
        (0.0, 0.0),
        (20.0, 0.0),
        (-20.0, 0.0),
        (0.0, 20.0),
        (0.0, -20.0),
    ];
    let mut rows = Vec::new();
    let mut seen = Vec::new();
    for (right_deg, up_deg) in looks {
        let looking = MotionState {
            view: [at_chest - up_deg, -right_deg, 0.0],
            ..me
        };
        let (forward, right, _) = math::angle_vectors(looking.view);
        let third = math::mad(mind::crosshair(&looking), -80.0, forward);
        let cameras = [
            ("third person", third),
            ("right shoulder", math::mad(third, 20.0, right)),
            ("left shoulder", math::mad(third, -20.0, right)),
            ("first person", mind::eye(&looking)),
        ];
        for (name, camera) in cameras {
            let aimed = MotionState {
                command_aim: mind::aim_for(&looking, camera),
                ..looking
            };
            let (from, way) = aim::crosshair_line(&aimed);
            let got = aim::crosshair_side(from, way, chest);
            seen.push(got);
            rows.push((
                format!("{name} ({right_deg}, {up_deg})"),
                got,
                screen_side(camera, looking.view, chest),
            ));
        }
    }
    let every = [Some(Side::Left), Some(Side::Right), Some(Side::Above), None];
    if let Some(missing) = every.iter().find(|side| !seen.contains(side)) {
        return Err(format!("no look gave the side {missing:?}"));
    }
    table(rows)
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
