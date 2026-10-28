//! The probes of placeables' looks and the visuals on objects: on the
//! proving ground a captive placed on (its `AnimationState`'s look) is used
//! by the player; its `OnUsed` (as the Star Forge's drain scripts do) plays
//! its deactivation and puts a drain beam on it from a placeable beside it,
//! and the drain's flash.

use crate::action::{Action, Verb};
use crate::input::Seat;
use crate::module::{ObjectData, ObjectKind, Placement};
use crate::probe::{Checks, check};
use crate::snapshot::{SimId, look};
use crate::{Snapshot, StepReason, TickInput, WorldConfig};

/// The probe's routines, by catalog number.
const CATALOG: &str = r#"
// 0
void PlayAnimation(int nAnimation, float fSpeed=1.0, float fSeconds=0.0);
// 1
effect EffectBeam(int nBeamVisualEffect, object oEffector, int nBodyPart, int bMissEffect=FALSE);
// 2
effect EffectVisualEffect(int nVisualEffectId, int nMissEffect=FALSE);
// 3
void ApplyEffectToObject(int nDurationType, effect eEffect, object oTarget, float fDuration=0.0f);
// 4
object GetObjectByTag(string sTag, int nNth=0);
"#;

/// `ANIMATION_PLACEABLE_DEACTIVATE`; the drain beam and the drain's flash
/// (`visualeffects.2da`'s `VFX_BEAM_DRAIN_LIFE`, `VFX_PRO_DRAIN`), and how
/// long the beam lasts (ms).
const DEACTIVATE: i32 = 201;
const BEAM: u16 = 2029;
const FLASH: u16 = 1009;
const BEAM_MS: u32 = 2000;

/// How many 8 ms steps each check is read after: the use's script run, the
/// flash gone, the beam ended.
const RUN: u64 = 10;
const FLASH_GONE: u64 = 10 + super::SEEN_MS / 8 + 5;
const BEAM_GONE: u64 = 10 + BEAM_MS as u64 / 8 + 5;

/// The captive's `OnUsed`: `PlayAnimation(DEACTIVATE)`;
/// `ApplyEffectToObject(DURATION_TYPE_TEMPORARY, EffectBeam(BEAM,
/// GetObjectByTag("source"), 0), OBJECT_SELF, 2.0)`;
/// `ApplyEffectToObject(DURATION_TYPE_INSTANT, EffectVisualEffect(FLASH),
/// OBJECT_SELF)`.
fn drained() -> fmt_odyssey::ncs::Ncs {
    use fmt_odyssey::ncs::{Const, Op};
    let action = |routine, args| Op::Action { routine, args };
    let mut asm = script_ncs::probes::Asm::default();
    asm.op(Op::Const(Const::Float(0.0)))
        .op(Op::Const(Const::Float(1.0)))
        .op(Op::Const(Const::Int(DEACTIVATE)))
        .op(action(0, 3))
        .op(Op::Const(Const::Float(BEAM_MS as f32 / 1000.0)))
        .op(Op::Const(Const::Object(0)))
        .op(Op::Const(Const::Int(0)))
        .op(Op::Const(Const::Int(0)))
        .op(Op::Const(Const::Int(0)))
        .op(Op::Const(Const::String(b"source".to_vec())))
        .op(action(4, 2))
        .op(Op::Const(Const::Int(i32::from(BEAM))))
        .op(action(1, 4))
        .op(Op::Const(Const::Int(1)))
        .op(action(3, 4))
        .op(Op::Const(Const::Float(0.0)))
        .op(Op::Const(Const::Object(0)))
        .op(Op::Const(Const::Int(0)))
        .op(Op::Const(Const::Int(i32::from(FLASH))))
        .op(action(2, 2))
        .op(Op::Const(Const::Int(0)))
        .op(action(3, 4))
        .op(Op::Retn);
    asm.build()
}

/// The ground's entry, where the player stands (metres, on its own axes),
/// and the captive beside it.
const CAPTIVE_AT: [f32; 2] = [-4.0, 1.2];
const SOURCE_AT: [f32; 2] = [-6.0, 3.0];

fn floor_at([x, y]: [f32; 2]) -> [f32; 3] {
    crate::determinism::ground_point(x, y, 0.0)
}

/// A run: the snapshot after each of the steps that matter (placed, used
/// and run, flash gone, beam ended), and the captive's and the source's ids.
type Ran = (Vec<Snapshot>, SimId, SimId);

fn scenario() -> Result<Ran, String> {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    let captive = data.placements.len() as u32;
    let placeable = |tag: &str, at, state| ObjectData {
        kind: ObjectKind::Placeable,
        tag: tag.into(),
        state,
        useable: true,
        ..ObjectData::unknown(tag, floor_at(at), 0.0)
    };
    data.placements.extend([
        Placement::Own(ObjectData {
            scripts: vec![("OnUsed".into(), "probe_drained".into())],
            ..placeable("captive", CAPTIVE_AT, look::ON)
        }),
        Placement::Own(placeable("source", SOURCE_AT, look::DEFAULT)),
    ]);
    let library = crate::probe::library(CATALOG, [("probe_drained", drained())]);
    let mut world = crate::SimWorld::new(
        WorldConfig::one_seat(),
        ground.with_module(data).with_scripts(library),
    )
    .map_err(|error| error.to_string())?;
    let mut snapshots = vec![world.snapshot()];
    let mut used = TickInput::default();
    used.act(
        Seat(0),
        Action::Use {
            object: captive,
            verb: Verb::Use,
        },
    );
    for step in 1..=BEAM_GONE {
        let input = if step == 1 {
            &used
        } else {
            &TickInput::default()
        };
        let snapshot = crate::step(&mut world, step, input, 8, StepReason::Authority);
        if [RUN, FLASH_GONE, BEAM_GONE].contains(&step) {
            snapshots.push(snapshot);
        }
    }
    let id = |index: u32| {
        snapshots[0]
            .entities
            .iter()
            .find(|entity| entity.kind == crate::Kind::Object(ObjectKind::Placeable, index))
            .map(|entity| entity.id)
            .ok_or(format!("no placeable {index}"))
    };
    let (captive, source) = (id(captive)?, id(captive + 1)?);
    Ok((snapshots, captive, source))
}

/// The checks: the captive placed on; used, its script leaves it off; the
/// beam on it from the source, its flash with it, both carried by the
/// snapshot (which round-trips); the flash gone after `SEEN_MS`, the beam
/// after its time; two runs the same. For the approved test
/// `placeable_looks`.
pub fn probes() -> Checks {
    let (snapshots, captive, source) = match scenario() {
        Ok(ran) => ran,
        Err(why) => return vec![("scenario", Err(why))],
    };
    let state = |at: usize| {
        snapshots[at]
            .get(captive)
            .and_then(|entity| entity.object)
            .map(|object| object.state)
    };
    let rows = |at: usize| -> Vec<(u16, bool, SimId)> {
        snapshots[at]
            .visuals
            .iter()
            .filter(|visual| visual.on == captive)
            .map(|visual| (visual.visual.row, visual.visual.beam, visual.by))
            .collect()
    };
    let beam = (BEAM, true, source);
    let again = scenario().map(|(ran, ..)| ran.last().map(Snapshot::hash));
    let round_trip =
        Snapshot::decode(&snapshots[1].encode()).map(|decoded| decoded == snapshots[1]);
    vec![
        (
            "placed_look",
            check(state(0) == Some(look::ON), format!("placed {:?}", state(0))),
        ),
        (
            "script_animation_sets_look",
            check(state(1) == Some(look::OFF), format!("used {:?}", state(1))),
        ),
        (
            "visuals_on_placeable",
            check(
                rows(1).contains(&beam) && rows(1).iter().any(|(row, ..)| *row == FLASH),
                format!("{:?}", rows(1)),
            ),
        ),
        (
            "visuals_round_trip",
            check(round_trip == Ok(true), format!("{round_trip:?}")),
        ),
        (
            "fleeting_gone",
            check(rows(2) == [beam], format!("{:?}", rows(2))),
        ),
        (
            "lasting_ends",
            check(rows(3).is_empty(), format!("{:?}", rows(3))),
        ),
        (
            "same_every_run",
            check(
                again
                    .as_ref()
                    .is_ok_and(|hash| *hash == snapshots.last().map(Snapshot::hash)),
                format!("{again:?}"),
            ),
        ),
    ]
}
