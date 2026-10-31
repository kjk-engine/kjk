//! The probes of `minds` (`kjk sim probes`): a mind fighting in place keeps
//! to the navigation mesh (`on_the_mesh`), and a scene's fighter stops where
//! its blows land (`scene_stop`).

use collision::UNIT;

use super::{on_the_mesh, scene_stop};
use crate::combat::mind;
use crate::motion::{MotionState, SOLE_Z};

/// A body 0.3 m inside the east edge of a 4 m square of mesh, its steps
/// each way: one off the edge is not taken, one along it or back in is,
/// and one standing off the mesh moves as it would.
pub fn probes() -> crate::probe::Checks {
    let corner = |x: f32, y: f32| [x, y, 0.0];
    let floor = [
        corner(0.0, 0.0),
        corner(4.0, 0.0),
        corner(4.0, 4.0),
        corner(0.0, 4.0),
    ];
    let nav = collision::nav::NavMesh::build(
        &[
            [floor[0], floor[1], floor[2]],
            [floor[0], floor[2], floor[3]],
        ],
        crate::walk::CLEARANCE * UNIT,
    );
    let level = crate::determinism::proving_ground().with_nav(nav);
    let cases = [
        Step::new("mind_stops_at_a_ledge_ahead", 3.7, 0.0, (127, 0), false),
        Step::new("mind_steps_back_from_a_ledge", 3.7, 180.0, (127, 0), true),
        Step::new("mind_strafes_not_off_a_ledge", 3.7, 90.0, (0, 100), false),
        Step::new("mind_strafes_along_a_ledge", 3.7, 0.0, (0, 100), true),
        Step::new(
            "mind_off_the_mesh_moves_as_it_would",
            6.0,
            0.0,
            (127, 0),
            true,
        ),
    ];
    cases
        .into_iter()
        .map(|step| {
            // Standing on the square's floor, halfway up it.
            let origin = [step.x / UNIT, 2.0 / UNIT, -SOLE_Z];
            let ps = MotionState::standing(origin, step.yaw);
            let (forward, right) = step.keys;
            let command = crate::input::Command {
                forward,
                right,
                ..mind::looking(&ps, 0.0, step.yaw)
            };
            let kept = on_the_mesh(&level, &ps, command);
            let moved = kept.forward != 0 || kept.right != 0;
            let detail = format!(
                "at x {:.1} m facing {}°, forward {forward} right {right}: kept forward {} right {}",
                step.x, step.yaw, kept.forward, kept.right
            );
            let result = if moved == step.moves {
                Ok(detail)
            } else {
                Err(detail)
            };
            (step.name, result)
        })
        .chain(stops())
        .collect()
}

/// Beasts of K1's `appearance.2da` (personal space and `hitdist`,
/// metres), each standing where a scene's fight stops it, facing its foe:
/// there it attacks.
fn stops() -> Vec<(&'static str, Result<String, String>)> {
    let beasts = [
        ("scene_stop_rakghoul_strikes", 0.35, 1.0),
        ("scene_stop_gamorrean_strikes", 0.4, 1.0),
        ("scene_stop_kath_hound_strikes", 0.6, 1.0),
    ];
    beasts
        .into_iter()
        .map(|(name, space, hit_distance)| {
            let strike = crate::module::Strike {
                hit_distance,
                ..crate::module::Strike::BLANK
            };
            let close = mind::Attack::Blow(crate::world::strikes::strike_reach(&strike, space));
            let stop = scene_stop(close, true);
            let me = MotionState::standing([0.0, 0.0, -SOLE_Z], 0.0);
            let foe = MotionState::standing([stop, 0.0, -SOLE_Z], 180.0);
            let at = mind::look_at(&foe, close);
            let command = mind::engage(
                crate::snapshot::SimId(1),
                &me,
                mind::eye(&me),
                at,
                close,
                0,
                true,
            );
            let detail = format!(
                "stops {:.2} m from its foe, its blow {:.2} m: buttons {:#x}",
                stop * UNIT,
                match close {
                    mind::Attack::Blow(reach) => reach * UNIT,
                    _ => 0.0,
                },
                command.buttons
            );
            let attacks = command.buttons & crate::input::buttons::ATTACK != 0;
            (name, if attacks { Ok(detail) } else { Err(detail) })
        })
        .collect()
}

/// A step of the probe: a body at `x` metres facing `yaw` pressing `keys`
/// (forward, right), and whether it should move.
struct Step {
    name: &'static str,
    x: f32,
    yaw: f32,
    keys: (i8, i8),
    moves: bool,
}

impl Step {
    const fn new(name: &'static str, x: f32, yaw: f32, keys: (i8, i8), moves: bool) -> Step {
        Step {
            name,
            x,
            yaw,
            keys,
            moves,
        }
    }
}
