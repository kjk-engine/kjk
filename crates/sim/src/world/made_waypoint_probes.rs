//! The probes of a waypoint a script makes (`kjk sim probes --only
//! made_waypoint`; `CreateObject` with `OBJECT_TYPE_WAYPOINT`). On the
//! proving ground a creature's spawn script makes a waypoint from its
//! template at `MARK_AT`. The party then leaves for a second module and
//! comes back by the waypoint's tag:
//! - the waypoint is made, as KOTOR's `CreateObject` makes one from its UTW;
//! - the party arrives at it, not at the module's entry, as TSLRCM's Red
//!   Eclipse scene brings the party back where the player stood
//!   (`a_delredecl` makes `tr_re_zb2`, and the next module's script names it
//!   in `StartNewModule`).

use fmt_odyssey::ncs::{Const, Op};

use collision::UNIT;

use crate::module::{ObjectData, ObjectKind, Placement};
use crate::probe::{Checks, check};
use crate::snapshot::{Kind, Snapshot};
use crate::{Command, Seat, StepReason, TickInput, WorldConfig};

pub fn probes() -> Checks {
    match scenario() {
        Ok(seen) => vec![
            ("a_script_makes_a_waypoint", made(&seen)),
            ("the_party_arrives_at_a_made_waypoint", arrived(&seen)),
        ],
        Err(error) => vec![("made_waypoint_scenario", Err(error))],
    }
}

/// The waypoint's template and tag.
const WAYPOINT: &str = "probe_mark";
/// Where the waypoint is made, and where the script's creature stands (the
/// ground's axes, metres).
const MARK_AT: [f32; 2] = [-3.0, 3.0];
const MAKER_AT: [f32; 2] = [-6.0, 4.5];
/// How near (metres, seen from above) the first seat must stand to the
/// waypoint to count as arrived at it.
const NEAR_M: f32 = 1.0;
/// How long the scenario steps before the party leaves (8 ms steps).
const STEPS: usize = 30;

/// The probe's catalog: the routines its script calls, numbered in order.
const CATALOG: &str = r#"
int OBJECT_TYPE_CREATURE = 1;
int OBJECT_TYPE_PLACEABLE = 64;
int OBJECT_TYPE_WAYPOINT = 32;
// 0
vector Vector(float x=0.0f, float y=0.0f, float z=0.0f);
// 1
location Location(vector vPosition, float fOrientation);
// 2
object CreateObject(int nObjectType, string sTemplate, location lLocation, int bUseAppearAnimation=FALSE);
"#;

const VECTOR: u16 = 0;
const LOCATION: u16 = 1;
const CREATE_OBJECT: u16 = 2;

/// What the scenario left: the snapshot before the party left, and the
/// first seat's place (metres) once it came back by the waypoint's tag.
struct Seen {
    before: Snapshot,
    back_at: [f32; 3],
}

/// A point of the proving ground's floor on its own axes
/// (`determinism::ground_point`).
fn floor_at([x, y]: [f32; 2]) -> [f32; 3] {
    crate::determinism::ground_point(x, y, 0.0)
}

/// The spawn script: a waypoint of `WAYPOINT` made at `at`.
fn script(at: [f32; 3]) -> fmt_odyssey::ncs::Ncs {
    let mut asm = script_ncs::probes::Asm::default();
    asm.op(Op::Const(Const::Int(0)))
        .op(Op::Const(Const::Float(0.0)))
        .op(Op::Const(Const::Float(at[2])))
        .op(Op::Const(Const::Float(at[1])))
        .op(Op::Const(Const::Float(at[0])))
        .op(Op::Action {
            routine: VECTOR,
            args: 3,
        })
        .op(Op::Action {
            routine: LOCATION,
            args: 2,
        })
        .op(Op::Const(Const::String(WAYPOINT.as_bytes().to_vec())))
        .op(Op::Const(Const::Int(32)))
        .op(Op::Action {
            routine: CREATE_OBJECT,
            args: 4,
        })
        .op(Op::Retn);
    asm.build()
}

fn scenario() -> Result<Seen, String> {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    data.placements.push(Placement::Own(ObjectData {
        tag: "maker".into(),
        scripts: vec![("ScriptSpawn".into(), "probe_make_mark".into())],
        ..ObjectData::unknown("maker", floor_at(MAKER_AT), 0.0)
    }));
    let waypoint = ObjectData {
        kind: ObjectKind::Waypoint,
        tag: WAYPOINT.into(),
        ..ObjectData::unknown(WAYPOINT, [0.0; 3], 0.0)
    };
    let library = crate::probe::library(CATALOG, [("probe_make_mark", script(floor_at(MARK_AT)))])
        .with_waypoints([(WAYPOINT.to_string(), std::sync::Arc::new(waypoint))].into());
    let level = ground.with_module(data).with_scripts(library);
    let mut world = super::SimWorld::new(
        WorldConfig {
            seed: 1,
            seats: 1,
            wanderers: 0,
            spawn: [0.0; 3],
            yaw: 0.0,
            sheet: crate::combat::Sheet::dev_soldier(),
            guests: crate::Guests::Avatars,
        },
        level.clone(),
    )
    .map_err(|error| error.to_string())?;
    let mut input = TickInput::default();
    input.insert(Seat(0), Command::default());
    let mut before = None;
    for step in 0..STEPS {
        before = Some(crate::step(
            &mut world,
            step as u64 + 1,
            &input,
            8,
            StepReason::Authority,
        ));
    }
    let before = before.ok_or("no steps")?;
    let away = crate::determinism::proving_ground_named("probe_away");
    world
        .install(away, None)
        .map_err(|error| format!("leaving: {error}"))?;
    let back = world
        .install(level, Some(WAYPOINT))
        .map_err(|error| format!("coming back: {error}"))?;
    let seat = back
        .entities
        .iter()
        .find_map(|entity| match entity.kind {
            Kind::Avatar(_) => entity.player.map(|player| player.origin),
            _ => None,
        })
        .ok_or("no seat came back")?;
    Ok(Seen {
        before,
        back_at: [seat[0] * UNIT, seat[1] * UNIT, seat[2] * UNIT],
    })
}

/// How far (metres, seen from above) `a` is from `b`.
fn flat_m(a: [f32; 3], b: [f32; 3]) -> f32 {
    let (dx, dy) = (a[0] - b[0], a[1] - b[1]);
    (dx * dx + dy * dy).sqrt()
}

fn made(seen: &Seen) -> Result<String, String> {
    let made: Vec<(ObjectKind, &str)> = seen
        .before
        .spawned
        .iter()
        .map(|made| (made.kind, made.template.as_str()))
        .collect();
    check(
        made.contains(&(ObjectKind::Waypoint, WAYPOINT)),
        format!("made in play: {made:?}"),
    )
}

fn arrived(seen: &Seen) -> Result<String, String> {
    let mark = floor_at(MARK_AT);
    let off = flat_m(seen.back_at, mark);
    check(
        off <= NEAR_M,
        format!(
            "the first seat came back {off:.2} m from the made waypoint at {mark:?} (at {:?})",
            seen.back_at
        ),
    )
}
