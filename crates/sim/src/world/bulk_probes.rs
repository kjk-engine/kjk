//! The probes of bodies met where their models are drawn (`kjk sim probes
//! --only bulks`; `world::body`, `module::Bulk`), no install: a hovering
//! droid's body (K1's probe droid's, 1.4 m to 3.0 m over its soles) and a
//! rancor's (5 m across, 7.6 m tall) against JA's player box:
//! - a bolt at the droid's middle meets it, one at a man's chest height
//!   under it passes below, as it would not through JA's box;
//! - a bolt at the rancor's head height meets it, over JA's box's top;
//! - a body turned a quarter turns its box with it;
//! - what others aim at (`mind::chest_of`) is the droid's middle;
//! - a frag grenade thrown at the Taris sewers' rancor (its box as K1's
//!   census has it, 14.5 m across) catches it in its blast, thrown from
//!   9 m off and from 3 m off, inside its box: it bursts where the rancor
//!   stands, as KOTOR's grenade goes off at its target's location, and the
//!   blast reaches down the rancor's height to the ground;
//! - a player standing at a wraid's flank, close enough that its eyes are
//!   inside the wraid's box (4.5 m long, 1.9 m tall), meets the wraid with
//!   a Force power's line or a shot along its view (`Bodies::shot`), where
//!   a plain trace from its eyes passes through it.

use collision::{BoxWorld, CollisionWorld, Contents, Extents, UNIT};

use super::bolts::{Reached, caught_by_blast, lob};
use super::{Level, body};
use crate::combat::{Missile, Sheet, grenades, mind, spells};
use crate::math::{Vec3, scale};
use crate::module::Bulk;
use crate::motion::MotionState;
use crate::probe::{Checks, check};
use crate::snapshot::SimId;

/// The probe droid's and the rancor's boxes, round K1's census
/// (`kjk combat bodies k1`).
const DROID: Bulk = Bulk {
    min: [-0.7, -0.7, 1.4],
    max: [0.7, 0.7, 3.0],
};
const RANCOR: Bulk = Bulk {
    min: [-2.5, -1.5, 0.0],
    max: [2.5, 3.0, 7.6],
};

pub fn probes() -> Checks {
    vec![
        ("hovering_droid_met_where_drawn", droid()),
        ("rancor_met_overhead", rancor()),
        ("bulk_turns_with_the_body", turned()),
        ("aimed_at_the_drawn_middle", aimed()),
        ("grenade_catches_the_sewer_rancor", grenaded()),
        ("line_from_inside_meets_the_wraid", wraid_in_face()),
    ]
}

/// A wraid's box (`c_wraid`, `kjk combat bodies k1`).
const WRAID: Bulk = Bulk {
    min: [-1.36, -2.45, -0.05],
    max: [1.25, 2.10, 1.89],
};

/// A player 1 m off a wraid's middle (facing +Y at the origin), looking at
/// it along +X from its flank, its eyes inside the wraid's box: whether a
/// plain trace and a shot from its eyes meet the wraid.
fn wraid_in_face() -> Result<String, String> {
    let wraid = body(SimId(1), &standing(), Some(&WRAID));
    let mut player = MotionState::standing([-1.0 / UNIT, 0.0, -crate::motion::SOLE_Z], 0.0);
    player.view[1] = 0.0;
    let eye = scale(mind::eye(&player), UNIT);
    let end = [eye[0] + 20.0, eye[1], eye[2]];
    let world = BoxWorld::new();
    let bodies = [wraid];
    let with_bodies = collision::Bodies::new(&world, &bodies);
    let plain = with_bodies.trace(eye, end, &Extents::POINT, Contents::BODY);
    let shot = with_bodies.shot(eye, end, &Extents::POINT, Contents::BODY);
    let inside = (0..3).all(|k| eye[k] > wraid.min[k] && eye[k] < wraid.max[k]);
    check(
        inside && plain.entity.is_none() && shot.entity == Some(wraid.entity),
        format!(
            "eyes at {eye:?} inside the box {inside}; a plain trace met {:?}, a shot {:?}",
            plain.entity, shot.entity
        ),
    )
}

/// `tar05_stampy`'s box (`c_rancor`, `kjk combat bodies k1`).
const SEWER_RANCOR: Bulk = Bulk {
    min: [-7.39, -4.34, -0.04],
    max: [7.12, 6.91, 7.58],
};

/// A body standing with its soles on the floor at the origin, facing +Y.
fn standing() -> MotionState {
    MotionState::standing([0.0, 0.0, -crate::motion::SOLE_Z], 90.0)
}

/// Whether a bolt (a point) from 6 m off along -X at `height` metres over
/// the soles meets the body `bulk` makes of `standing` (`None`: JA's box).
fn met_at(bulk: Option<&Bulk>, height: f32) -> bool {
    let target = body(SimId(1), &standing(), bulk);
    let world = BoxWorld::new();
    let bodies = [target];
    let trace = collision::Bodies::new(&world, &bodies).trace(
        [-6.0, 0.0, height],
        [6.0, 0.0, height],
        &Extents::POINT,
        Contents::BODY,
    );
    trace.fraction < 1.0
}

fn droid() -> Result<String, String> {
    let middle = (DROID.min[2] + DROID.max[2]) * 0.5;
    let man = 1.1;
    let (drawn, under, ja_under) = (
        met_at(Some(&DROID), middle),
        met_at(Some(&DROID), man),
        met_at(None, man),
    );
    check(
        drawn && !under && ja_under,
        format!(
            "at its middle ({middle:.1} m) met {drawn}; at {man} m under it met {under} \
             (JA's box: {ja_under})"
        ),
    )
}

fn rancor() -> Result<String, String> {
    let head = 6.5;
    let (drawn, ja) = (met_at(Some(&RANCOR), head), met_at(None, head));
    check(
        drawn && !ja,
        format!("at {head} m met {drawn} (JA's box: {ja})"),
    )
}

fn turned() -> Result<String, String> {
    let mut quarter = standing();
    quarter.view[1] = 0.0;
    let ahead = body(SimId(1), &standing(), Some(&RANCOR));
    let side = body(SimId(1), &quarter, Some(&RANCOR));
    let size = |b: &collision::Body, axis: usize| b.max[axis] - b.min[axis];
    let swapped = (size(&ahead, 0) - size(&side, 1)).abs() < 1e-3
        && (size(&ahead, 1) - size(&side, 0)).abs() < 1e-3;
    // Facing +Y its 3 m ahead lies along +Y; facing +X, along +X.
    let reaches = (ahead.max[1] - 3.0).abs() < 1e-3 && (side.max[0] - 3.0).abs() < 1e-3;
    check(
        swapped && reaches,
        format!(
            "facing +Y {:?}..{:?}; facing +X {:?}..{:?}",
            ahead.min, ahead.max, side.min, side.max
        ),
    )
}

/// A frag grenade thrown along +Y from `off` metres short of the sewer
/// rancor (standing on the arena's floor 12 m along +Y, facing +Y), at 1 m
/// over the floor: where it bursts, the body it met, and whether its blast
/// catches the rancor.
fn thrown_at_rancor(off: f32) -> (Vec3, Option<SimId>, bool) {
    let level = Level::arena();
    let rancor = MotionState::standing([0.0, 12.0 / UNIT, -crate::motion::SOLE_Z], 90.0);
    let bodies = [body(SimId(1), &rancor, Some(&SEWER_RANCOR))];
    let mut grenade = Missile {
        owner: SimId(0),
        weapon: Sheet::dev_soldier().main_weapon(),
        cadence_ms: 0,
        share: 1000,
        base: [0.0, (12.0 - off) / UNIT, 1.0 / UNIT],
        from_ms: 0,
        velocity: [0.0, grenades::SPEED, grenades::LIFT],
        half_size: grenades::HALF_SIZE,
        blockable: false,
        until_ms: grenades::FUSE_MS,
        lob: Some(grenades::Lob {
            row: spells::ITEM_ABILITY_GRENADE_FRAGMENTATION,
            resting: false,
        }),
        held: None,
        scripted: false,
        power: false,
    };
    let step = 50;
    let (at, struck) = (step..=grenades::FUSE_MS)
        .step_by(step as usize)
        .find_map(|now| lob(&level, None, &mut grenade, &bodies, now, step as u32))
        .expect("the fuse bursts it");
    let reached = [Reached::body(SimId(1), &rancor, Some(&SEWER_RANCOR))];
    let caught = caught_by_blast(&level, None, &reached, at)
        .iter()
        .any(|(id, _)| *id == SimId(1));
    (at, struck, caught)
}

fn grenaded() -> Result<String, String> {
    let [far, near] = [9.0, 3.0].map(thrown_at_rancor);
    let met = |(_, struck, caught): (Vec3, Option<SimId>, bool)| struck == Some(SimId(1)) && caught;
    let say = |(at, struck, caught): (Vec3, Option<SimId>, bool)| {
        let [x, y, z] = scale(at, UNIT);
        format!("burst at ({x:.1}, {y:.1}, {z:.1}) m, met {struck:?}, caught {caught}")
    };
    check(
        met(far) && met(near),
        format!(
            "from 9 m: {}; from 3 m, inside its box: {}",
            say(far),
            say(near)
        ),
    )
}

fn aimed() -> Result<String, String> {
    let ps = standing();
    let soles = ps.origin[2] + crate::motion::SOLE_Z;
    let at: Vec3 = mind::chest_of(&ps, Some(&DROID));
    let height = (at[2] - soles) * UNIT;
    let man: Vec3 = scale(mind::chest(&ps), UNIT);
    check(
        (height - 2.2).abs() < 1e-3,
        format!(
            "the droid is aimed at {height:.2} m over its soles (a man at {:.2})",
            man[2] - soles * UNIT
        ),
    )
}
