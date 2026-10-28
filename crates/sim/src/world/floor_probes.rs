//! The probes of items lying on an area's floor (`kjk sim probes floor
//! items`; `CreateItemOnFloor`), on the proving ground, no install. As
//! K1's scripts use it (Taris's duel: `k_ptar_gun1` drops a pistol at
//! `BP_GUN_SPAWN`, `k_ptar_fight` finds it by `GetNearestObjectByTag` and
//! destroys it), the item made is an object of the area: no one possesses
//! it, it stands where the location is (its position, facing and location
//! read back), the nearest-by-tag search finds it, and the snapshot carries
//! it on the floor (`Goods::floor`, among its items held by the area) for
//! the views to draw; it survives a save; destroyed, it is off the floor
//! and gone. A stackable template lies as its own stack, joining none.

use fmt_odyssey::ncs::{Const, Op};
use script_ncs::probes::Asm;

use super::SimWorld;
use crate::input::{Command, Seat, TickInput};
use crate::inventory::{Catalog, Lying};
use crate::probe::{Checks, check};
use crate::script::{AREA, Pending};
use crate::{StepReason, WorldConfig};

const CATALOG: &str = r#"
// 0
object CreateItemOnFloor(string sTemplate, location lLocation, int bUseAppearAnimation=FALSE);
// 1
object GetFirstPC();
// 2
location GetLocation(object oObject);
// 3
object GetNearestObjectByTag(string sTag, object oTarget=OBJECT_SELF, int nNth=1);
// 4
void DestroyObject(object oDestroy, float fDelay=0.0f, int bNoFade=FALSE, float fDelayUntilFade=0.0f);
// 5
object GetItemPossessor(object oItem);
// 6
int GetIsObjectValid(object oObject);
// 7
string IntToString(int nInteger);
// 8
void SetCustomToken(int nCustomTokenNumber, string sTokenValue);
// 9
float GetFacing(object oTarget);
// 10
string FloatToString(float fFloat, int nWidth=18, int nDecimals=9);
"#;

/// The pistol dropped, and a stackable item dropped twice.
const PISTOL: &str = "probe_pistol";
const CELLS: &str = "probe_cells";

fn call(routine: u16, args: u8) -> Op {
    Op::Action { routine, args }
}

fn int(value: i32) -> Op {
    Op::Const(Const::Int(value))
}

fn text(value: &str) -> Op {
    Op::Const(Const::String(value.as_bytes().to_vec()))
}

/// `template` dropped where the first PC stands; the object made on top.
fn drop(asm: &mut Asm, template: &str) {
    asm.op(int(0))
        .op(call(1, 0))
        .op(call(2, 1))
        .op(text(template))
        .op(call(0, 3));
}

/// The value on top (a string) as custom token `n`.
fn token(asm: &mut Asm, n: i32) {
    asm.op(int(n)).op(call(8, 2));
}

/// The nearest object to the first PC tagged `tag`, on top.
fn nearest(asm: &mut Asm, tag: &str) {
    asm.op(int(1)).op(call(1, 0)).op(text(tag)).op(call(3, 3));
}

/// `probe_drop`: the pistol dropped; token 1 whether it has a possessor,
/// token 2 its facing; the cells dropped twice. `probe_take`: token 3
/// whether the pistol is found nearest the PC, then it is destroyed.
fn scripts() -> [(&'static str, fmt_odyssey::ncs::Ncs); 2] {
    let mut dropping = Asm::default();
    drop(&mut dropping, PISTOL);
    dropping
        .op(Op::CpTopSp {
            offset: -4,
            size: 4,
        })
        .op(call(5, 1))
        .op(call(6, 1))
        .op(call(7, 1));
    token(&mut dropping, 1);
    dropping.op(int(0)).op(int(18)).op(Op::CpTopSp {
        offset: -12,
        size: 4,
    });
    dropping.op(call(9, 1)).op(call(10, 3));
    token(&mut dropping, 2);
    dropping.op(Op::MovSp(-4));
    for _ in 0..2 {
        drop(&mut dropping, CELLS);
        dropping.op(Op::MovSp(-4));
    }
    dropping.op(Op::Retn);
    let mut taking = Asm::default();
    nearest(&mut taking, PISTOL);
    taking.op(call(6, 1)).op(call(7, 1));
    token(&mut taking, 3);
    taking
        .op(Op::Const(Const::Float(0.0)))
        .op(int(0))
        .op(Op::Const(Const::Float(0.0)));
    nearest(&mut taking, PISTOL);
    taking.op(call(4, 4)).op(Op::Retn);
    [
        ("probe_drop", dropping.build()),
        ("probe_take", taking.build()),
    ]
}

fn world() -> SimWorld {
    let ground = crate::determinism::proving_ground();
    let plain = super::containers::probes::plain;
    let cells = crate::inventory::ItemFacts {
        stacking: 10,
        ..plain(CELLS)
    };
    let library =
        crate::probe::library(CATALOG, scripts()).with_items(Catalog::new([plain(PISTOL), cells]));
    SimWorld::new(WorldConfig::one_seat(), ground.with_scripts(library))
        .expect("the proving ground makes a world")
}

/// `script` run by the module, stepped.
fn run(world: &mut SimWorld, script: &str) {
    let mut state = world.script_state().clone();
    state
        .pending
        .push(Pending::new(script, crate::script::MODULE));
    world.set_script_state(state);
    let mut input = TickInput::default();
    input.insert(Seat(0), Command::default());
    let tick = world.tick() + 1;
    crate::step(world, tick, &input, 8, StepReason::Authority);
}

/// The state round-tripped through a save's encoding.
fn saved(state: &crate::script::State) -> Result<crate::script::State, String> {
    let mut out = crate::wire::Writer::default();
    state.encode(&mut out);
    let mut input = crate::wire::Reader::new("state", &out.0);
    crate::script::State::decode(&mut input).map_err(|error| error.to_string())
}

pub fn probes() -> Checks {
    let mut world = world();
    run(&mut world, "probe_drop");
    let snapshot = world.snapshot();
    let at = snapshot.body(Seat(0)).map(|body| body.position);
    let state = world.script_state();
    let Some(module) = world.level().module().map(|module| module.name.clone()) else {
        return vec![("floor_items", Err("the ground has no module".into()))];
    };
    let lying: Vec<Lying> = state.lying_in(&module).map(|(_, lying)| lying).collect();
    let held = |template: &str| -> Vec<(u32, u32, u16)> {
        snapshot
            .goods
            .items
            .iter()
            .filter(|(_, item)| item.template == template)
            .map(|(id, item)| (*id, item.holder, item.stack))
            .collect()
    };
    let token =
        |snapshot: &crate::snapshot::Snapshot, n: i32| snapshot.talk.tokens.get(&n).cloned();
    let pistol = held(PISTOL);
    let where_lies = snapshot
        .goods
        .floor
        .iter()
        .find(|(id, _)| pistol.first().is_some_and(|(pistol, ..)| pistol == id))
        .map(|(_, lying)| *lying);
    let placed = where_lies
        .zip(at)
        .is_some_and(|(lying, at)| crate::math::flat_distance(lying.at, at) < 0.01);
    let cells = held(CELLS);
    let save = saved(state).map(|loaded| {
        loaded.lying_in(&module).count() == lying.len() && loaded.floor == state.floor
    });
    let (dropped_tokens, facing) = (token(&snapshot, 1), token(&snapshot, 2));
    run(&mut world, "probe_take");
    let after = world.snapshot();
    let found = token(&after, 3);
    let gone = after.goods.floor.len() == 2
        && !world
            .script_state()
            .items
            .values()
            .any(|item| item.template == PISTOL);
    vec![
        (
            "dropped_lies_on_the_floor",
            check(
                pistol.len() == 1 && pistol[0].1 == AREA.0 && placed && lying.len() == 3,
                format!(
                    "the pistol {pistol:?} (the area is {}), lying at {where_lies:?} where the \
                     PC stands ({at:?}); {} lying in all",
                    AREA.0,
                    lying.len()
                ),
            ),
        ),
        (
            "dropped_has_no_possessor",
            check(
                dropped_tokens.as_deref() == Some("0")
                    && facing
                        .as_deref()
                        .and_then(|facing| facing.trim().trim_end_matches('.').parse::<f32>().ok())
                        .zip(where_lies)
                        .is_some_and(|(facing, lying)| (facing - lying.yaw).abs() < 1.0),
                format!("possessor valid {dropped_tokens:?}, facing {facing:?}"),
            ),
        ),
        (
            "dropped_stacks_apart",
            check(
                cells.len() == 2
                    && cells
                        .iter()
                        .all(|(_, holder, stack)| *holder == AREA.0 && *stack == 1),
                format!("the cells dropped twice: {cells:?}"),
            ),
        ),
        (
            "dropped_kept_in_a_save",
            check(save == Ok(true), format!("{save:?}")),
        ),
        (
            "dropped_found_and_destroyed",
            check(
                found.as_deref() == Some("1") && gone,
                format!(
                    "found nearest the PC {found:?}; destroyed, off the floor and gone {gone} \
                     (lying now {:?})",
                    after.goods.floor
                ),
            ),
        ),
    ]
}
