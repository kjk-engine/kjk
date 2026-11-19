//! The probes of a creature held at 1 hit point by its Min1HP flag (`kjk
//! sim probes "min1hp damaged"`), on the proving ground, no install. A
//! creature with Min1HP and a damaged script is hit twice by a script's
//! damage, each blow far more than its hit points:
//! - the first blow takes it to 1 hit point, and its damaged script runs
//!   and sees 1;
//! - the second blow takes nothing, as the floor holds it at 1, but its
//!   damaged script runs again. TSLRCM's Visas Marr waits for that run to
//!   yield (`k_visas_damage`, #571).

use super::SimWorld;
use crate::StepReason;
use crate::combat::sheet::{Sheet, flags};
use crate::input::{Seat, TickInput};
use crate::module::{ObjectData, ObjectKind, Placement};
use crate::probe::{Checks, check};
use crate::script::Pending;
use crate::snapshot::Snapshot;
use fmt_odyssey::ncs::{Const, Ncs, Op};
use script_ncs::probes::Asm;

const MSEC: u32 = 8;
const TARGET: &str = "probe_held";
/// The custom token the damaged script writes the creature's hit points to.
const SEEN: i32 = 90;
/// What the clearing script writes to it.
const CLEARED: &str = "none";
/// The steps waited after each blow: one second, past the damaged
/// script's wait (`scripts::DAMAGED_EVERY_MS`).
const WAIT_STEPS: usize = 1000 / MSEC as usize;

/// The routines the probe's scripts call, each after its number.
const CATALOG: &str = r#"
// 0
void SetCustomToken(int nCustomTokenNumber, string sTokenValue);
// 1
string IntToString(int nInteger);
// 2
int GetCurrentHitPoints(object oObject = OBJECT_SELF);
// 3
object GetObjectByTag(string sTag, int nNth = 0);
// 4
void ApplyEffectToObject(int nDurationType, effect eEffect, object oTarget, float fDuration = 0.0f);
// 5
effect EffectDamage(int nDamageAmount, int nDamageType = DAMAGE_TYPE_UNIVERSAL, int nDamagePower = DAMAGE_POWER_NORMAL);
int DAMAGE_TYPE_UNIVERSAL = 8;
int DAMAGE_POWER_NORMAL = 0;
"#;

fn call(routine: u16, args: u8) -> Op {
    Op::Action { routine, args }
}

fn int(value: i32) -> Op {
    Op::Const(Const::Int(value))
}

fn text(value: &str) -> Op {
    Op::Const(Const::String(value.as_bytes().to_vec()))
}

/// The damaged script: token `SEEN` set to the creature's hit points.
fn damaged() -> Ncs {
    let mut asm = Asm::default();
    asm.op(Op::Const(Const::Object(0)))
        .op(call(2, 1))
        .op(call(1, 1))
        .op(int(SEEN))
        .op(call(0, 2))
        .op(Op::Retn);
    asm.build()
}

/// Token `SEEN` set to `CLEARED`.
fn clear() -> Ncs {
    let mut asm = Asm::default();
    asm.op(text(CLEARED))
        .op(int(SEEN))
        .op(call(0, 2))
        .op(Op::Retn);
    asm.build()
}

/// 1000 points of universal damage on the creature, at once.
fn blow() -> Ncs {
    let mut asm = Asm::default();
    asm.op(Op::Const(Const::Float(0.0)))
        .op(int(0))
        .op(text(TARGET))
        .op(call(3, 2))
        .op(int(0))
        .op(int(8))
        .op(int(1000))
        .op(call(5, 3))
        .op(int(0))
        .op(call(4, 4))
        .op(Op::Retn);
    asm.build()
}

fn step(world: &mut SimWorld) -> Snapshot {
    let mut input = TickInput::default();
    input.insert(Seat(0), crate::input::Command::default());
    let tick = world.tick() + 1;
    crate::step(world, tick, &input, MSEC, StepReason::Authority)
}

/// The module runs `script`, then the world plays `steps` steps; what
/// token `SEEN` reads after.
fn run(world: &mut SimWorld, script: &str, steps: usize) -> String {
    let mut state = world.script_state().clone();
    state
        .pending
        .push(Pending::new(script, crate::script::MODULE));
    world.set_script_state(state);
    let mut last = step(world);
    for _ in 1..steps {
        last = step(world);
    }
    super::power_cycle_probes::token(&last, SEEN)
}

/// What token `SEEN` read after the first blow, after the clearing, and
/// after the second blow.
fn blows() -> Result<[String; 3], String> {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    let entry = data.entry.position;
    let sheet = Sheet {
        flags: flags::MIN_ONE_HP,
        ..Sheet::dev_soldier()
    };
    data.placements.push(Placement::Own(ObjectData {
        sheet: Some(sheet),
        template: TARGET.to_string(),
        scripts: vec![("ScriptDamaged".into(), "probe_held_damaged".into())],
        ..ObjectData::bare(
            ObjectKind::Creature,
            TARGET,
            [entry[0] + 2.0, entry[1] + 3.0, entry[2]],
        )
    }));
    let library = crate::probe::library(
        CATALOG,
        [
            ("probe_held_damaged", damaged()),
            ("probe_held_clear", clear()),
            ("probe_held_blow", blow()),
        ],
    );
    let mut world = SimWorld::new(
        crate::WorldConfig::one_seat(),
        ground.with_module(data).with_scripts(library),
    )
    .map_err(|error| error.to_string())?;
    step(&mut world);
    Ok([
        run(&mut world, "probe_held_blow", WAIT_STEPS),
        run(&mut world, "probe_held_clear", 1),
        run(&mut world, "probe_held_blow", WAIT_STEPS),
    ])
}

pub fn probes() -> Checks {
    let seen = match blows() {
        Ok(seen) => seen,
        Err(error) => return vec![("min1hp_damaged_at_one", Err(error))],
    };
    let [first, cleared, second] = &seen;
    vec![
        (
            "min1hp_held_at_one",
            check(
                first == "1" && cleared == CLEARED,
                format!("the damaged script saw {first} after the first blow, then {cleared}"),
            ),
        ),
        (
            "min1hp_damaged_at_one",
            check(
                second == "1",
                format!(
                    "after a second blow at 1 hit point, the damaged script's token reads {second} \
                     (\"{CLEARED}\" if the script did not run)"
                ),
            ),
        ),
    ]
}
