//! The probes of a door's or placeable's facing as scripts read and set it
//! (`kjk sim probes --only facing`; `ObjectKind::facing`), no install. Both
//! games keep an object's facing as the way its model's +Y points, so a
//! placeable's facing is its bearing turned a quarter left. On the proving
//! ground a marker placeable stands with a bearing of `BEARING`, and a
//! creature's spawn script reads it:
//! - a creature made at the marker's location faces `BEARING` + 90 (TSL's
//!   Exile, made at the Ebon Hawk's medical bay marker, lies along the bed);
//! - the script's own creature, turned to the marker's `GetFacing`, faces
//!   the same;
//! - a placeable made at a location facing `MADE_FACING` turns its model's
//!   +Y there: its bearing is `MADE_FACING` − 90.

use fmt_odyssey::ncs::{Const, Op};

use crate::module::{ObjectData, ObjectKind, Placement};
use crate::probe::{Checks, check};
use crate::{Command, Seat, StepReason, TickInput, WorldConfig};

pub fn probes() -> Checks {
    match scenario() {
        Ok(seen) => vec![
            ("made_at_a_placeable_faces_its_front", made_creature(&seen)),
            (
                "get_facing_of_a_placeable_is_its_front",
                turned_maker(&seen),
            ),
            ("made_placeable_turns_its_front", made_placeable(&seen)),
        ],
        Err(error) => vec![("facing_scenario", Err(error))],
    }
}

/// The marker's bearing (degrees).
const BEARING: f32 = 30.0;
/// The facing (degrees) of the location a placeable is made at.
const MADE_FACING: f32 = 50.0;
/// A quarter turn (degrees): from a bearing to a facing.
const QUARTER: f32 = 90.0;
/// How far (degrees) a facing may be off: a view's angle is kept in short
/// steps.
const SLACK: f32 = 0.5;

/// Where the marker, the script's creature and the made placeable stand
/// (the ground's axes, metres).
const MARKER_AT: [f32; 2] = [-3.0, 0.0];
const MAKER_AT: [f32; 2] = [-6.0, 4.5];
const BOX_AT: [f32; 2] = [-3.0, 3.0];
/// How near (metres) a creature stands to where it was put to count as
/// that one: a body is put at the first free place near its mark.
const NEAR_M: f32 = 1.5;

/// The probe's catalog: the routines its script calls, numbered in order.
const CATALOG: &str = r#"
int OBJECT_TYPE_CREATURE = 1;
int OBJECT_TYPE_PLACEABLE = 64;
// 0
vector Vector(float x=0.0f, float y=0.0f, float z=0.0f);
// 1
location Location(vector vPosition, float fOrientation);
// 2
object CreateObject(int nObjectType, string sTemplate, location lLocation, int bUseAppearAnimation=FALSE);
// 3
object GetObjectByTag(string sTag, int nNth=0);
// 4
location GetLocation(object oObject);
// 5
float GetFacing(object oTarget);
// 6
void SetFacing(float fDirection);
"#;

const VECTOR: u16 = 0;
const LOCATION: u16 = 1;
const CREATE_OBJECT: u16 = 2;
const GET_OBJECT_BY_TAG: u16 = 3;
const GET_LOCATION: u16 = 4;
const GET_FACING: u16 = 5;
const SET_FACING: u16 = 6;

/// How long the scenario steps (8 ms steps).
const STEPS: usize = 30;

/// What the scenario left: the made creature's and the script's creature's
/// facings (degrees), and the made placeable's bearing.
struct Seen {
    made: f32,
    maker: f32,
    placeable: f32,
}

/// A point of the proving ground's floor on its own axes
/// (`determinism::ground_point`).
fn floor_at([x, y]: [f32; 2]) -> [f32; 3] {
    crate::determinism::ground_point(x, y, 0.0)
}

fn action(routine: u16, args: u8) -> Op {
    Op::Action { routine, args }
}

/// The marker found by its tag, left on the stack.
fn marker() -> [Op; 3] {
    [
        Op::Const(Const::Int(0)),
        Op::Const(Const::String(b"marker".to_vec())),
        action(GET_OBJECT_BY_TAG, 2),
    ]
}

/// The spawn script: a creature made at the marker's location, a placeable
/// made at a location facing `MADE_FACING`, and itself turned to the
/// marker's facing.
fn script(box_at: [f32; 3]) -> fmt_odyssey::ncs::Ncs {
    let mut asm = script_ncs::probes::Asm::default();
    asm.op(Op::Const(Const::Int(0)));
    for op in marker() {
        asm.op(op);
    }
    asm.op(action(GET_LOCATION, 1))
        .op(Op::Const(Const::String(b"probe_body".to_vec())))
        .op(Op::Const(Const::Int(1)))
        .op(action(CREATE_OBJECT, 4))
        .op(Op::Const(Const::Int(0)))
        .op(Op::Const(Const::Float(MADE_FACING)))
        .op(Op::Const(Const::Float(box_at[2])))
        .op(Op::Const(Const::Float(box_at[1])))
        .op(Op::Const(Const::Float(box_at[0])))
        .op(action(VECTOR, 3))
        .op(action(LOCATION, 2))
        .op(Op::Const(Const::String(b"probe_box".to_vec())))
        .op(Op::Const(Const::Int(64)))
        .op(action(CREATE_OBJECT, 4));
    for op in marker() {
        asm.op(op);
    }
    asm.op(action(GET_FACING, 1))
        .op(action(SET_FACING, 1))
        .op(Op::Retn);
    asm.build()
}

fn scenario() -> Result<Seen, String> {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    data.placements.push(Placement::Own(ObjectData {
        kind: ObjectKind::Placeable,
        tag: "marker".into(),
        ..ObjectData::unknown("marker", floor_at(MARKER_AT), BEARING)
    }));
    data.placements.push(Placement::Own(ObjectData {
        tag: "maker".into(),
        scripts: vec![("ScriptSpawn".into(), "probe_face".into())],
        ..ObjectData::unknown("maker", floor_at(MAKER_AT), 0.0)
    }));
    let body = ObjectData::unknown("probe_body", [0.0; 3], 0.0);
    let placeable = ObjectData {
        kind: ObjectKind::Placeable,
        ..ObjectData::unknown("probe_box", [0.0; 3], 0.0)
    };
    let library = crate::probe::library(CATALOG, [("probe_face", script(floor_at(BOX_AT)))])
        .with_creatures([("probe_body".to_string(), std::sync::Arc::new(body))].into())
        .with_placeables([("probe_box".to_string(), std::sync::Arc::new(placeable))].into());
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
        level,
    )
    .map_err(|error| error.to_string())?;
    let mut input = TickInput::default();
    input.insert(Seat(0), Command::default());
    let mut last = None;
    for step in 0..STEPS {
        last = Some(crate::step(
            &mut world,
            step as u64 + 1,
            &input,
            8,
            StepReason::Authority,
        ));
    }
    let last = last.ok_or("no steps")?;
    let creature_at = |on: [f32; 2]| {
        let at = floor_at(on);
        last.entities
            .iter()
            .filter(|entity| !matches!(entity.kind, crate::Kind::Avatar(_)))
            .filter_map(|entity| Some((entity.position, entity.player?.view[1])))
            .find(|(position, _)| {
                let (dx, dy) = (position[0] - at[0], position[1] - at[1]);
                (dx * dx + dy * dy).sqrt() < NEAR_M
            })
            .map(|(_, yaw)| yaw)
            .ok_or(format!(
                "no creature at {on:?} (the ground's axes, {at:?}); creatures at {:?}",
                last.entities
                    .iter()
                    .filter(|entity| entity.player.is_some())
                    .map(|entity| entity.position)
                    .collect::<Vec<_>>()
            ))
    };
    let placeable = last
        .spawned
        .iter()
        .find(|made| made.template == "probe_box")
        .ok_or("no placeable made")?
        .yaw;
    Ok(Seen {
        made: creature_at(MARKER_AT)?,
        maker: creature_at(MAKER_AT)?,
        placeable,
    })
}

/// Whether facings `a` and `b` (degrees) are the same within `SLACK`.
fn same(a: f32, b: f32) -> bool {
    ((a - b + 180.0).rem_euclid(360.0) - 180.0).abs() <= SLACK
}

fn made_creature(seen: &Seen) -> Result<String, String> {
    let wanted = BEARING + QUARTER;
    check(
        same(seen.made, wanted),
        format!(
            "a creature made at a placeable of bearing {BEARING} faces {:.1} (wanted {wanted})",
            seen.made
        ),
    )
}

fn turned_maker(seen: &Seen) -> Result<String, String> {
    let wanted = BEARING + QUARTER;
    check(
        same(seen.maker, wanted),
        format!(
            "a creature turned to the placeable's GetFacing faces {:.1} (wanted {wanted})",
            seen.maker
        ),
    )
}

fn made_placeable(seen: &Seen) -> Result<String, String> {
    let wanted = MADE_FACING - QUARTER;
    check(
        same(seen.placeable, wanted),
        format!(
            "a placeable made facing {MADE_FACING} has bearing {:.1} (wanted {wanted})",
            seen.placeable
        ),
    )
}
