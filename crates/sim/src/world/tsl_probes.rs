//! The probes of TSL's own rules (`kjk sim probes tsl`), on the proving
//! ground, no install:
//! - natural healing (`world::powers`' regeneration, `regeneration.2da`'s
//!   numbers as TSL ships them): a hurt player heals a hit point and more
//!   each `TimePerHP` seconds, never past its maximum, and not at all while
//!   `DisableHealthRegen` holds;
//! - influence (`GetInfluence`, `SetInfluence`, `ModifyInfluence`): -1
//!   unset, moved from `START_INFLUENCE` where unset, set and moved within
//!   0 to `MAX_INFLUENCE`;
//! - puppets (`AddAvailablePUPByTemplate`, `AssignPUP`, `SpawnAvailablePUP`,
//!   `AddPartyPuppet`, `GetIsPuppet`, `GetPUPOwner`): made, given to a
//!   companion, joined, told apart from the rest;
//! - influence on alignment (`script::influence`): a companion made
//!   available starts at its base influence (Kreia does not); its
//!   alignment follows its influence and `G_PC_Align_Val`, which the
//!   player's alignment shift sets; a shift on it in the party moves its
//!   base; a quiet shift moves the player alone; changes are told;
//! - forfeit conditions: what an equip and an item's use break (an energy
//!   shield the one item `FORFEIT_NO_ITEM_BUT_SHIELD` allows);
//! - a puppet's follow (`ActionFollowOwner`, `Action::FollowOwner`): held
//!   while its owner is within the range, a run to the owner and the follow
//!   again beyond it, kept in a save; queued by no one but a puppet;
//! - Bao-Dur met (his influence set) for the screens, and the "Cannot
//!   Equip" lines an item he cannot wear then shows.

use super::{Fighting, Id, SimWorld};
use crate::combat::Sheet;
use crate::input::{Seat, TickInput};
use crate::probe::{Checks, check};
use crate::script::{Pending, Regeneration};
use crate::snapshot::{Kind, SimId, Snapshot, StatusKind};
use crate::{Level, StepReason};
use fmt_odyssey::ncs::{Const, Ncs, Op};
use script_ncs::probes::Asm;
use script_ncs::vm::ObjectId;
use std::sync::Arc;

const MSEC: u32 = 8;

/// TSL's `regeneration.2da` as the Steam install ships it.
pub(super) const TSL: Regeneration = Regeneration {
    time_per_hp: 6.0,
    out_of_combat: 1.0,
    in_combat: 0.0,
    con_bonus: 0.3,
    skill_bonus: 0.1,
    fp: crate::script::FpRegeneration {
        time: 90.0,
        out_of_combat: 1.0,
        in_combat: 0.1,
        wis_bonus: 0.0,
    },
};

/// The companion and puppet numbers and templates the scripts use.
const NPC: i32 = 4;
const OTHER: i32 = 5;
const PUP: i32 = 0;
const COMPANION: &str = "probe_tsl_npc";
const PUPPET: &str = "probe_tsl_pup";
/// Atton and Kreia (`NPC_ATTON`, `NPC_KREIA`), as `influence.2da` lists
/// them by tag.
const ATTON: i32 = 0;
const KREIA: i32 = 6;
const ATTON_TEMPLATE: &str = "probe_atton";
/// `NPC_BAO_DUR`.
const BAO_DUR: i32 = 1;
/// How far the probe puts the puppet from its owner, and its follow ranges
/// (metres): nearer than that, and farther.
const PUT_APART: f32 = 3.0;
const FOLLOW_FAR: f32 = 1.0;
const FOLLOW_NEAR: f32 = 20.0;
const KREIA_TEMPLATE: &str = "probe_kreia";

const CATALOG: &str = r#"
// 0
void SetCustomToken(int nCustomTokenNumber, string sTokenValue);
// 1
string IntToString(int nInteger);
// 2
int GetInfluence(int nNPC);
// 3
void SetInfluence(int nNPC, int nInfluence);
// 4
void ModifyInfluence(int nNPC, int nModifier);
// 5
int AddAvailableNPCByTemplate(int nNPC, string sTemplate);
// 6
int AddAvailablePUPByTemplate(int nPUP, string sTemplate);
// 7
int AssignPUP(int nPUP, int nNPC);
// 8
object SpawnAvailablePUP(int nPUP, location lLocation);
// 9
int AddPartyPuppet(int nPUP, object oidCreature);
// 10
int GetIsPuppet(object oPUP = OBJECT_SELF);
// 11
object GetPUPOwner(object oPUP = OBJECT_SELF);
// 12
location GetLocation(object oObject);
// 13
object GetFirstPC();
// 14
object SpawnAvailableNPC(int nNPC, location lPosition);
// 15
int AddPartyMember(int nNPC, object oCreature);
// 16
int GetIsObjectValid(object oObject);
// 17
void SetGlobalNumber(string sIdentifier, int nValue);
// 18
int GetGlobalNumber(string sIdentifier);
// 19
int GetGoodEvilValue(object oCreature);
// 20
void AdjustAlignment(object oSubject, int nAlignment, int nShift, int bDontModifyNPCs = FALSE);
// 21
object GetObjectByTag(string sTag, int nNth = 0);
// 22
int RemoveNPCFromPartyToBase(int nNPC);
// 23
void SetForfeitConditions(int nForfeitFlags);
// 24
int GetLastForfeitViolation();
// 25
void ActionFollowOwner(float fRange = 2.5);
// 26
void ShowPartySelectionGUI(string sExitScript = "", int nForceNPC1 = -1, int nForceNPC2 = -1, int nAllowCancel = FALSE);
// 27
int GetRunScriptVar();
// 28
object GetFirstObjectInArea(object oArea = OBJECT_INVALID, int nObjectFilter = OBJECT_TYPE_CREATURE);
// 29
object GetNextObjectInArea(object oArea = OBJECT_INVALID, int nObjectFilter = OBJECT_TYPE_CREATURE);
// 30
object GetArea(object oTarget);
// 31
int GetNPCSelectability(int nNPC);
// 32
void SetNPCSelectability(int nNPC, int nSelectability);
// 33
void SetLocalBoolean(object oObject, int nIndex, int nValue);
// 34
int GetLocalBoolean(object oObject, int nIndex);
// 35
void SetLocalNumber(object oObject, int nIdx, int nValue);
// 36
int GetLocalNumber(object oObject, int nIdx);
// 37
void ChangeToStandardFaction(object oCreatureToChange, int nStandardFaction);
// 38
int GetStandardFaction(object oObject);
// 39
void SetGlobalFadeOut(float fWait = 0.0f, float fLength = 0.0f, float fR = 0.0f, float fG = 0.0f, float fB = 0.0f);
// 40
void SetGlobalFadeIn(float fWait = 0.0f, float fLength = 0.0f, float fR = 0.0f, float fG = 0.0f, float fB = 0.0f);
int OBJECT_TYPE_CREATURE = 1;
"#;

/// The global the player's alignment is kept in.
const PC_ALIGN: &str = crate::script::influence::PC_ALIGNMENT;

fn call(routine: u16, args: u8) -> Op {
    Op::Action { routine, args }
}

fn int(value: i32) -> Op {
    Op::Const(Const::Int(value))
}

fn text(value: &str) -> Op {
    Op::Const(Const::String(value.as_bytes().to_vec()))
}

/// Custom token `n` set to what `value` leaves (an int, as a string).
fn token(asm: &mut Asm, n: i32, value: &[Op]) {
    for op in value {
        asm.op(op.clone());
    }
    asm.op(call(1, 1)).op(int(n)).op(call(0, 2));
}

/// The probes' scripts. `probe_influence` sets tokens 1 to 5:
/// `GetInfluence` of a companion never set, then after
/// `ModifyInfluence(+5)` (from `START_INFLUENCE`), `SetInfluence(70)`,
/// `ModifyInfluence(+50)` and `SetInfluence(-5)`. `probe_puppet` makes
/// the companion and its puppet available, the companion beside the
/// player, gives it the puppet and has it join (the puppet comes along).
fn library() -> crate::script::Library {
    let influence = || [int(NPC), call(2, 1)];
    let mut inf = Asm::default();
    token(&mut inf, 1, &influence());
    inf.op(int(5)).op(int(NPC)).op(call(4, 2));
    token(&mut inf, 2, &influence());
    inf.op(int(70)).op(int(NPC)).op(call(3, 2));
    token(&mut inf, 3, &influence());
    inf.op(int(50)).op(int(NPC)).op(call(4, 2));
    token(&mut inf, 4, &influence());
    inf.op(int(-5)).op(int(NPC)).op(call(3, 2));
    token(&mut inf, 5, &influence());
    inf.op(Op::Retn);
    let mut pup = Asm::default();
    pup.op(text(COMPANION))
        .op(int(OTHER))
        .op(call(5, 2))
        .op(Op::MovSp(-4));
    pup.op(text(PUPPET))
        .op(int(PUP))
        .op(call(6, 2))
        .op(Op::MovSp(-4));
    pup.op(call(13, 0)).op(call(12, 1));
    pup.op(int(OTHER)).op(call(14, 2)).op(Op::MovSp(-4));
    pup.op(int(OTHER))
        .op(int(PUP))
        .op(call(7, 2))
        .op(Op::MovSp(-4));
    pup.op(int(0)).op(text(COMPANION)).op(call(21, 2));
    pup.op(int(OTHER)).op(call(15, 2)).op(Op::MovSp(-4));
    pup.op(Op::Retn);
    // The companion sent to base (token 30 whether it went); the duel's
    // rules set (31 the last broken: none); 32 whether the same script
    // still finds its creature by tag (`KillNPCObject` deleted it).
    let mut base = Asm::default();
    token(&mut base, 30, &[int(OTHER), call(22, 1)]);
    base.op(int(20)).op(call(23, 1));
    token(&mut base, 31, &[call(24, 0)]);
    token(
        &mut base,
        32,
        &[int(0), text(COMPANION), call(21, 2), call(16, 1)],
    );
    base.op(Op::Retn);
    // A puppet's follow, far and near; Bao-Dur met.
    let follow_owner = |range: f32| {
        let mut asm = Asm::default();
        asm.op(Op::Const(Const::Float(range)))
            .op(call(25, 1))
            .op(Op::Retn);
        asm.build()
    };
    let mut bao_dur = Asm::default();
    bao_dur
        .op(int(50))
        .op(int(BAO_DUR))
        .op(call(3, 2))
        .op(Op::Retn);
    // The party selection screen, cancel allowed (the Ebon Hawk's ramp);
    // its exit script's run script var (token 50).
    let mut screen = Asm::default();
    screen
        .op(int(1))
        .op(int(-1))
        .op(int(-1))
        .op(text("probe_party_exit"))
        .op(call(26, 4))
        .op(Op::Retn);
    let mut exit = Asm::default();
    token(&mut exit, 50, &[call(27, 0)]);
    exit.op(Op::Retn);
    // Atton made beside the player, not joined; the area's first two
    // creatures, whether each is (tokens 60 and 61).
    let mut atton_beside = Asm::default();
    atton_beside
        .op(text(ATTON_TEMPLATE))
        .op(int(ATTON))
        .op(call(5, 2))
        .op(Op::MovSp(-4));
    atton_beside.op(call(13, 0)).op(call(12, 1));
    atton_beside
        .op(int(ATTON))
        .op(call(14, 2))
        .op(Op::MovSp(-4));
    atton_beside.op(Op::Retn);
    let mut in_area = Asm::default();
    for (n, routine) in [(60, 28), (61, 29)] {
        token(
            &mut in_area,
            n,
            &[
                int(1),
                call(13, 0),
                call(30, 1),
                call(routine, 2),
                call(16, 1),
            ],
        );
    }
    in_area.op(Op::Retn);
    // Local boolean 50 set on OBJECT_INVALID and on the player, each read
    // back (tokens 90 and 91).
    let mut nothing = Asm::default();
    for (n, owner) in [(90, Op::Const(Const::Object(1))), (91, call(13, 0))] {
        nothing
            .op(int(1))
            .op(int(50))
            .op(owner.clone())
            .op(call(33, 3));
        token(&mut nothing, n, &[int(50), owner, call(34, 2)]);
    }
    nothing.op(Op::Retn);
    // The player's local numbers set and read back (tokens 100 to 105, the
    // cases of `LOCAL_NUMBERS`).
    let mut numbers = Asm::default();
    for (n, (index, value)) in (100..).zip(LOCAL_NUMBERS) {
        numbers
            .op(int(value))
            .op(int(index))
            .op(call(13, 0))
            .op(call(35, 3));
        token(&mut numbers, n, &[int(index), call(13, 0), call(36, 2)]);
    }
    numbers.op(Op::Retn);
    // Local booleans set at `LOCAL_BOOLEANS`, tokens 120 on each read back.
    let mut booleans = Asm::default();
    for (n, index) in (120..).zip(LOCAL_BOOLEANS) {
        booleans
            .op(int(1))
            .op(int(index))
            .op(call(13, 0))
            .op(call(33, 3));
        token(&mut booleans, n, &[int(index), call(13, 0), call(34, 2)]);
    }
    booleans.op(Op::Retn);
    // The player moved to Hostile 1; token 110 its standard faction, read
    // a step later.
    let mut pc_faction = Asm::default();
    pc_faction.op(int(1)).op(call(13, 0)).op(call(37, 2));
    pc_faction.op(Op::Retn);
    let mut pc_faction_read = Asm::default();
    token(&mut pc_faction_read, 110, &[call(13, 0), call(38, 1)]);
    pc_faction_read.op(Op::Retn);
    crate::probe::library(
        CATALOG,
        [
            ("probe_influence", inf.build()),
            ("probe_puppet", pup.build()),
            ("probe_to_base", base.build()),
            ("probe_atton_joins", atton_joins()),
            ("probe_follow", follow()),
            ("probe_follow_far", follow_owner(FOLLOW_FAR)),
            ("probe_follow_near", follow_owner(FOLLOW_NEAR)),
            ("probe_bao_dur", bao_dur.build()),
            ("probe_spawned_twice", spawned_twice()),
            ("probe_second_atton", second_atton()),
            ("probe_spawned_again", spawned_again()),
            ("probe_light_point", light_point()),
            ("probe_fade_out", fade(39)),
            ("probe_fade_in", fade(40)),
            (
                crate::script::presentation::SUMMARY_CLOSED,
                summary_closed(),
            ),
            ("probe_party_screen", screen.build()),
            ("probe_party_exit", exit.build()),
            ("probe_atton_beside", atton_beside.build()),
            ("probe_in_area", in_area.build()),
            ("probe_locals_of_nothing", nothing.build()),
            ("probe_local_numbers", numbers.build()),
            ("probe_pc_faction", pc_faction.build()),
            ("probe_local_booleans", booleans.build()),
            ("probe_pc_faction_read", pc_faction_read.build()),
            ("probe_selectability", selectability()),
        ],
    )
    .with_creatures(
        [
            (COMPANION, COMPANION),
            (PUPPET, PUPPET),
            (ATTON_TEMPLATE, "atton"),
            (KREIA_TEMPLATE, "kreia"),
        ]
        .map(|(template, tag)| {
            (
                template.to_string(),
                Arc::new(crate::module::ObjectData::creature(
                    tag,
                    template,
                    [0.0; 3],
                    Sheet::dev_soldier(),
                )),
            )
        })
        .into_iter()
        .collect(),
    )
    .with_regeneration(Some(TSL))
    .with_influence(vec![("atton".into(), 50), ("kreia".into(), 50)])
}

/// `probe_atton_joins`: Atton and Kreia made available by template
/// (tokens 20 and 21 their influence then), Atton made beside the player
/// and joined.
fn atton_joins() -> Ncs {
    let mut asm = Asm::default();
    for (npc, template) in [(ATTON, ATTON_TEMPLATE), (KREIA, KREIA_TEMPLATE)] {
        asm.op(text(template))
            .op(int(npc))
            .op(call(5, 2))
            .op(Op::MovSp(-4));
    }
    token(&mut asm, 20, &[int(ATTON), call(2, 1)]);
    token(&mut asm, 21, &[int(KREIA), call(2, 1)]);
    asm.op(call(13, 0)).op(call(12, 1));
    asm.op(int(ATTON)).op(call(14, 2));
    asm.op(int(ATTON)).op(call(15, 2)).op(Op::MovSp(-4));
    asm.op(Op::Retn);
    asm.build()
}

/// `probe_spawned_twice`: Atton made available, then spawned twice at the
/// player in one script, the second made a party member (TSL's
/// `a_baodur_join` spawns Bao-Dur so).
fn spawned_twice() -> Ncs {
    let mut asm = Asm::default();
    asm.op(text(ATTON_TEMPLATE))
        .op(int(ATTON))
        .op(call(5, 2))
        .op(Op::MovSp(-4));
    asm.op(call(13, 0)).op(call(12, 1));
    asm.op(int(ATTON)).op(call(14, 2)).op(Op::MovSp(-4));
    asm.op(call(13, 0)).op(call(12, 1));
    asm.op(int(ATTON)).op(call(14, 2));
    asm.op(int(ATTON)).op(call(15, 2)).op(Op::MovSp(-4));
    asm.op(Op::Retn);
    asm.build()
}

/// `probe_spawned_again`: token 42 whether Atton spawned again at the
/// player is the Atton already here (`GetObjectByTag`).
fn spawned_again() -> Ncs {
    let mut asm = Asm::default();
    token(
        &mut asm,
        42,
        &[
            int(0),
            text("atton"),
            call(21, 2),
            call(13, 0),
            call(12, 1),
            int(ATTON),
            call(14, 2),
            Op::Equal(fmt_odyssey::ncs::Pair::ObjectObject),
        ],
    );
    asm.op(Op::Retn);
    asm.build()
}

/// The screen faded at once by `routine` (`SetGlobalFadeOut`, `SetGlobalFadeIn`).
fn fade(routine: u16) -> fmt_odyssey::ncs::Ncs {
    let mut asm = Asm::default();
    asm.op(call(routine, 0)).op(Op::Retn);
    asm.build()
}

/// `probe_light_point`: the player given a light side point, which the
/// status summary tells.
fn light_point() -> Ncs {
    let mut asm = Asm::default();
    asm.op(int(0)).op(int(1)).op(int(2)).op(call(13, 0));
    asm.op(call(20, 4)).op(Op::Retn);
    asm.build()
}

/// The probes' `k_align_movie`, run as the status summary closes: token 70
/// set to 1.
fn summary_closed() -> Ncs {
    let mut asm = Asm::default();
    token(&mut asm, 70, &[int(1)]);
    asm.op(Op::Retn);
    asm.build()
}

/// `probe_selectability`: `GetNPCSelectability` in tokens 80 to 82, of
/// Bao-Dur never made available, then of Atton made available, then of
/// Atton made unselectable (`SetNPCSelectability(NPC_ATTON, 0)`).
fn selectability() -> Ncs {
    let mut asm = Asm::default();
    let selectability = |npc: i32| [int(npc), call(31, 1)];
    token(&mut asm, 80, &selectability(BAO_DUR));
    asm.op(text(ATTON_TEMPLATE))
        .op(int(ATTON))
        .op(call(5, 2))
        .op(Op::MovSp(-4));
    token(&mut asm, 81, &selectability(ATTON));
    asm.op(int(0)).op(int(ATTON)).op(call(32, 2));
    token(&mut asm, 82, &selectability(ATTON));
    asm.op(Op::Retn);
    asm.build()
}

/// `probe_second_atton`: token 40 whether a second object is tagged
/// `atton`, 41 whether a first one is.
fn second_atton() -> Ncs {
    let mut asm = Asm::default();
    for (n, nth) in [(40, 1), (41, 0)] {
        token(
            &mut asm,
            n,
            &[int(nth), text("atton"), call(21, 2), call(16, 1)],
        );
    }
    asm.op(Op::Retn);
    asm.build()
}

/// `probe_follow`, Atton in the party: the player's alignment 0, Atton's
/// influence 100 (token 22 his alignment); the player shifted 100 to the
/// light (23 Atton's, 24 the global); Atton shifted 10 to the dark (25 his
/// alignment); the player shifted quietly to neutral (26 the player's, 27
/// the global, untouched); the global 50 and Atton's influence 80 (28 his
/// alignment: his base, moved).
fn follow() -> Ncs {
    let atton = || [int(0), text("atton"), call(21, 2)];
    let good_evil = |asm: &mut Asm, n: i32, of: &[Op]| {
        let mut ops = of.to_vec();
        ops.push(call(19, 1));
        token(asm, n, &ops);
    };
    let global = || [text(PC_ALIGN), call(18, 1)];
    let set_global = |asm: &mut Asm, value: i32| {
        asm.op(int(value)).op(text(PC_ALIGN)).op(call(17, 2));
    };
    let adjust = |asm: &mut Asm, who: &[Op], side: i32, shift: i32, quiet: i32| {
        asm.op(int(quiet)).op(int(shift)).op(int(side));
        for op in who {
            asm.op(op.clone());
        }
        asm.op(call(20, 4));
    };
    let mut asm = Asm::default();
    set_global(&mut asm, 0);
    asm.op(int(100)).op(int(ATTON)).op(call(3, 2));
    good_evil(&mut asm, 22, &atton());
    adjust(&mut asm, &[call(13, 0)], 2, 100, 0);
    good_evil(&mut asm, 23, &atton());
    token(&mut asm, 24, &global());
    adjust(&mut asm, &atton(), 3, 10, 0);
    good_evil(&mut asm, 25, &atton());
    adjust(&mut asm, &[call(13, 0)], 1, 50, 1);
    good_evil(&mut asm, 26, &[call(13, 0)]);
    token(&mut asm, 27, &global());
    set_global(&mut asm, 50);
    asm.op(int(80)).op(int(ATTON)).op(call(3, 2));
    good_evil(&mut asm, 28, &atton());
    asm.op(Op::Retn);
    asm.build()
}

fn world() -> Result<(SimWorld, Level), String> {
    let level = crate::determinism::proving_ground().with_scripts(library());
    let world = SimWorld::new(crate::WorldConfig::one_seat(), level.clone())
        .map_err(|error| error.to_string())?;
    Ok((world, level))
}

fn step(world: &mut SimWorld) -> Snapshot {
    let mut input = TickInput::default();
    input.insert(Seat(0), crate::input::Command::default());
    let tick = world.tick() + 1;
    crate::step(world, tick, &input, MSEC, StepReason::Authority)
}

fn run(world: &mut SimWorld, script: &str) {
    run_as(world, script, crate::script::MODULE);
}

fn run_as(world: &mut SimWorld, script: &str, caller: ObjectId) {
    let mut state = world.script_state().clone();
    state.pending.push(Pending::new(script, caller));
    world.set_script_state(state);
}

fn tokens(snapshot: &Snapshot, numbers: std::ops::RangeInclusive<i32>) -> Vec<String> {
    numbers
        .map(|n| {
            snapshot
                .talk
                .tokens
                .get(&n)
                .cloned()
                .unwrap_or_else(|| "(unset)".into())
        })
        .collect()
}

fn player(snapshot: &Snapshot) -> Option<SimId> {
    snapshot
        .entities
        .iter()
        .find(|entity| matches!(entity.kind, Kind::Avatar(_)))
        .map(|entity| entity.id)
}

/// The player hurt to a third of its hit points (hundredths), then
/// `seconds` of standing: its hit points before and after, and its
/// maximum (hundredths).
fn healed(off: bool, seconds: u64) -> Result<(i32, i32, i32), String> {
    let (mut world, _) = world()?;
    let first = step(&mut world);
    let me = player(&first).ok_or("no player")?;
    if off {
        let mut state = world.script_state().clone();
        state.party.no_regen = true;
        world.set_script_state(state);
    }
    let max = first
        .get(me)
        .and_then(|entity| entity.sheet)
        .map_or(0, |sheet| i32::from(sheet.max_hp) * 100);
    let hurt = max / 3;
    for (id, mut fighting) in world
        .world
        .query::<(&Id, &mut Fighting)>()
        .iter_mut(&mut world.world)
    {
        if id.0 == me {
            fighting.0.hp = hurt;
        }
    }
    let mut last = first;
    for _ in 0..seconds * 1000 / u64::from(MSEC) {
        last = step(&mut world);
    }
    let after = last
        .get(me)
        .and_then(|entity| entity.fighter)
        .map_or(0, |f| f.hp);
    Ok((hurt, after, max))
}

pub fn probes() -> Checks {
    let mut checks = Vec::new();
    checks.push((
        "natural_healing",
        healed(false, 13).and_then(|(hurt, after, max)| {
            // Two `TimePerHP` marks in 13 s: at least two hit points.
            check(
                after >= hurt + 200 && after <= max,
                format!("hurt to {hurt}, after 13 s {after} (max {max}) hundredths"),
            )
        }),
    ));
    checks.push((
        "healing_held_off",
        healed(true, 13).and_then(|(hurt, after, _)| {
            check(
                after == hurt,
                format!("DisableHealthRegen: hurt to {hurt}, after 13 s {after} hundredths"),
            )
        }),
    ));
    checks.push(("influence", influence()));
    checks.push(("puppets", puppets()));
    checks.push(("forfeits", forfeits()));
    checks.push(("forfeit_item_but_shield", forfeit_items()));
    checks.push(("follow_owner_held", follow_owner(true)));
    checks.push(("follow_owner_runs", follow_owner(false)));
    checks.push(("bao_dur_met", bao_dur_met()));
    checks.push(("spawned_twice_is_one", spawned_twice_is_one()));
    checks.push(("spawned_again_is_the_same", spawned_again_is_the_same()));
    checks.push(("selectability_of_the_unknown", selectability_told()));
    checks.push((
        "summary_closed_runs_align_movie",
        summary_closed_runs_align_movie(),
    ));
    checks.push(("summary_waits_for_the_fade", summary_waits_for_the_fade()));
    checks.push(("cannot_equip_lines", cannot_equip_lines()));
    checks.push(("influence_starts", influence_starts()));
    checks.push(("alignment_follows_influence", follows()));
    checks.push(("restrict_mode_ends_with_module", restrict_ends()));
    checks.push(("minigame_takes_no_companions", minigame_alone()));
    checks.push(("party_screen_accepted", party_screen_accepted()));
    checks.push(("area_iteration_skips_the_dead", area_skips_the_dead()));
    checks.push(("locals_of_nothing", locals_of_nothing()));
    for (name, game, wanted) in [
        (
            "local_booleans_k1",
            "k1",
            ["1", "-2147483648", "0", "0", "0", "0"],
        ),
        (
            "local_booleans_tsl",
            "tsl",
            ["1", "-2147483648", "1", "-2147483648", "0", "0"],
        ),
    ] {
        checks.push((name, local_booleans(game, wanted)));
    }
    for (name, game, wanted) in [("pc_faction_k1", "k1", "0"), ("pc_faction_tsl", "tsl", "0")] {
        checks.push((name, pc_faction(game, wanted)));
    }
    for (name, game, wanted) in [
        ("local_numbers_k1", "k1", ["255", "0", "127", "5", "0", "0"]),
        (
            "local_numbers_tsl",
            "tsl",
            ["255", "0", "127", "5", "5", "0"],
        ),
    ] {
        checks.push((name, local_numbers(game, wanted)));
    }
    checks
}

/// Atton made beside the player (not joined): the area's creatures are
/// the player and he; dead, the area's iteration passes him over
/// (`k_battle_death`, the battle of Khoonda's end, counts on it).
/// A local set on OBJECT_INVALID is kept nowhere and reads 0, as the
/// executable's `ExecuteCommandSetLocalBoolean` sets nothing and its
/// `GetLocal...` reads 0 when `GetGameObject` finds no object (403DXN's
/// `tr_circle`, the battle circle's sergeant gone, read 1 there and took
/// 50 XP from the party); the player's own local still holds.
/// The local numbers set (index, value) by `probe_local_numbers`: -1, 200
/// (out of a byte's signed range), 127, the last of K1's eight slots, the
/// first past them, and one past TSL's 32.
const LOCAL_NUMBERS: [(i32, i32); 6] = [(0, -1), (1, 200), (2, 127), (7, 5), (8, 5), (32, 5)];

/// Each game's local numbers as its executable keeps them
/// (`script::engine::local_number`): a byte read back unsigned (-1 reads
/// 255), a number outside -128 to 127 not kept, K1's eight slots and TSL's
/// 32, nothing past them.
fn local_numbers(game: &str, wanted: [&str; 6]) -> Result<String, String> {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    data.game = game.into();
    let level = ground.with_module(data).with_scripts(library());
    let mut world =
        SimWorld::new(crate::WorldConfig::one_seat(), level).map_err(|error| error.to_string())?;
    step(&mut world);
    run(&mut world, "probe_local_numbers");
    let read = tokens(&step(&mut world), 100..=105);
    check(read == wanted, format!("{game}: {read:?}"))
}

/// The local booleans set and read back by `probe_local_booleans`: the
/// first, K1's last (95), its first past them (96), TSL's last (159), its
/// first past them (160), and one below 0.
const LOCAL_BOOLEANS: [i32; 6] = [0, 95, 96, 159, 160, -1];

/// Each game's local booleans as its executable keeps them
/// (`script::engine::local_boolean`): K1's var table 96 bits, TSL's 160,
/// an index tested unsigned; none past them kept, read false; a true one
/// read as its bit in its word (`local_boolean_read`: 95 and 159 are bit 31,
/// 96 is bit 0 of TSL's fourth word).
fn local_booleans(game: &str, wanted: [&str; 6]) -> Result<String, String> {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    data.game = game.into();
    let level = ground.with_module(data).with_scripts(library());
    let mut world =
        SimWorld::new(crate::WorldConfig::one_seat(), level).map_err(|error| error.to_string())?;
    step(&mut world);
    run(&mut world, "probe_local_booleans");
    let read = tokens(&step(&mut world), 120..=125);
    check(read == wanted, format!("{game}: {read:?}"))
}

/// The player's own character moved to Hostile 1 by
/// `ChangeToStandardFaction`: neither executable moves it (K1's and TSL's
/// `ExecuteCommandChangeFaction` test `CSWSCreatureStats`'s PC flag first),
/// so the Exile stays in the player's faction after Dxun's battle circle.
fn pc_faction(game: &str, wanted: &str) -> Result<String, String> {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    data.game = game.into();
    let level = ground.with_module(data).with_scripts(library());
    let mut world =
        SimWorld::new(crate::WorldConfig::one_seat(), level).map_err(|error| error.to_string())?;
    step(&mut world);
    run(&mut world, "probe_pc_faction");
    step(&mut world);
    run(&mut world, "probe_pc_faction_read");
    let read = tokens(&step(&mut world), 110..=110);
    check(
        read == [wanted],
        format!("{game}: the player's faction after the change {read:?}"),
    )
}

fn locals_of_nothing() -> Result<String, String> {
    let (mut world, _) = world()?;
    step(&mut world);
    run(&mut world, "probe_locals_of_nothing");
    let read = tokens(&step(&mut world), 90..=91);
    check(
        read == ["0", "262144"],
        format!(
            "local boolean 50 (bit 18 of its word) read back on OBJECT_INVALID and the player: {read:?}"
        ),
    )
}

fn area_skips_the_dead() -> Result<String, String> {
    let (mut world, _) = world()?;
    step(&mut world);
    run(&mut world, "probe_atton_beside");
    step(&mut world);
    run(&mut world, "probe_in_area");
    let alive = tokens(&step(&mut world), 60..=61);
    let atton = world
        .script_state()
        .party
        .available
        .get(&(ATTON as u8))
        .and_then(|npc| npc.object)
        .ok_or("Atton not made")?;
    for (id, mut fighting) in world
        .world
        .query::<(&Id, &mut Fighting)>()
        .iter_mut(&mut world.world)
    {
        if id.0.0 == atton {
            fighting.0.hp = 0;
            fighting.0.life = crate::combat::Life::Dead;
        }
    }
    step(&mut world);
    run(&mut world, "probe_in_area");
    let dead = tokens(&step(&mut world), 60..=61);
    check(
        alive == ["1", "1"] && dead == ["1", "0"],
        format!("the area's first two creatures: Atton alive {alive:?}, dead {dead:?}"),
    )
}

/// The party selection screen shown, then closed by the player taking the
/// party as it is: its exit script reads `party::ACCEPTED` as its run
/// script var (TSL's Ebon Hawk ramp leaves the ship on it, stays on 0).
fn party_screen_accepted() -> Result<String, String> {
    let (mut world, _) = world()?;
    step(&mut world);
    run(&mut world, "probe_party_screen");
    step(&mut world);
    let mut input = TickInput::default();
    input.insert(Seat(0), crate::input::Command::default());
    input.act(Seat(0), crate::action::Action::ChooseParty(None));
    let tick = world.tick() + 1;
    crate::step(&mut world, tick, &input, MSEC, StepReason::Authority);
    let mut last = step(&mut world);
    for _ in 0..3 {
        last = step(&mut world);
    }
    let var = tokens(&last, 50..=50).concat();
    check(
        var == crate::script::party::ACCEPTED.to_string(),
        format!("the exit script's run script var: {var}"),
    )
}

/// Atton joined, then a minigame's area installed (Peragus's hangar
/// turret), then a plain one: not in the minigame's, back in the next.
fn minigame_alone() -> Result<String, String> {
    let (mut world, level) = world()?;
    step(&mut world);
    run(&mut world, "probe_atton_joins");
    step(&mut world);
    let atton = world
        .script_state()
        .party
        .available
        .get(&(ATTON as u8))
        .and_then(|npc| npc.object)
        .ok_or("Atton not joined")?;
    let module = (**level.module().ok_or("no module")?).clone();
    let elsewhere = |name: &str| {
        let mut module = module.clone();
        module.name = name.into();
        module
    };
    let turret = level
        .clone()
        .with_module(elsewhere("turret"))
        .with_minigame(crate::minigame::Rules::bare(crate::minigame::Kind::Turret));
    let there = |snapshot: &Snapshot| snapshot.entities.iter().any(|e| e.id.0 == atton);
    let in_turret = world.install(turret, None).map_err(|e| e.to_string())?;
    let after = world
        .install(level.with_module(elsewhere("after")), None)
        .map_err(|e| e.to_string())?;
    check(
        !there(&in_turret) && there(&after),
        format!(
            "Atton in the minigame's area {}, in the next {}",
            there(&in_turret),
            there(&after)
        ),
    )
}

/// The player restrict mode set (as the asteroid walk's entry script
/// does), then the next module installed: restricted in the first, not in
/// the next.
fn restrict_ends() -> Result<String, String> {
    let (mut world, level) = world()?;
    step(&mut world);
    let mut state = world.script_state().clone();
    state.party.restrict = true;
    world.set_script_state(state);
    step(&mut world);
    let before = world.script_state().party.restrict;
    let mut elsewhere = (**level.module().ok_or("no module")?).clone();
    elsewhere.name = "elsewhere".into();
    world
        .install(level.with_module(elsewhere), None)
        .map_err(|error| error.to_string())?;
    step(&mut world);
    let after = world.script_state().party.restrict;
    check(
        before && !after,
        format!("restricted {before} in the module, {after} in the next"),
    )
}

/// Atton made available reads his base influence, Kreia unset.
fn influence_starts() -> Result<String, String> {
    let (mut world, _) = world()?;
    step(&mut world);
    run(&mut world, "probe_atton_joins");
    let after = step(&mut world);
    let seen = tokens(&after, 20..=21);
    check(seen == ["50", "-1"], format!("Atton, Kreia: {seen:?}"))
}

/// Atton's alignment through `probe_follow` (his base the template's):
/// 0 at full influence under a fully dark player; 100 once the player's
/// shift to the light sets the global to 100; still 100 after a shift on
/// him (his base moved, the pull full); the quiet shift moves the player
/// alone; under a neutral global, his moved base. The statuses told.
fn follows() -> Result<String, String> {
    let (mut world, _) = world()?;
    step(&mut world);
    run(&mut world, "probe_atton_joins");
    step(&mut world);
    step(&mut world);
    run(&mut world, "probe_follow");
    let after = step(&mut world);
    let seen = tokens(&after, 22..=28);
    let base = i32::from(Sheet::dev_soldier().good_evil);
    let want = ["0", "100", "100", "100", "50", "100"]
        .iter()
        .map(|v| v.to_string())
        .chain([(base - 10).to_string()])
        .collect::<Vec<_>>();
    let statuses: Vec<_> = after.progress.statuses.iter().map(|s| &s.kind).collect();
    let told = statuses.contains(&&StatusKind::Influence {
        npc: ATTON as u8,
        gained: true,
    }) && statuses.contains(&&StatusKind::Alignment { light: true });
    check(
        seen == want && told,
        format!("Atton's base {base}: {seen:?}, want {want:?}; statuses {statuses:?}"),
    )
}

fn influence() -> Result<String, String> {
    let (mut world, _) = world()?;
    step(&mut world);
    run(&mut world, "probe_influence");
    let after = step(&mut world);
    let seen = tokens(&after, 1..=5);
    check(
        seen == ["-1", "55", "70", "100", "0"],
        format!("unset, +5, set 70, +50, set -5: {seen:?}"),
    )
}

/// The puppet given to a companion comes with it as it joins (made beside
/// it, in the party), and leaves with it sent to base, the companion's
/// creature gone at once (the script that sent it finds no creature by its
/// tag: TSL's `KillNPCObject` deletes it, so 403DXN's `k_403_enter` makes
/// Kreia anew) and the table still knowing both.
fn puppets() -> Result<String, String> {
    let (mut world, _) = world()?;
    step(&mut world);
    run(&mut world, "probe_puppet");
    step(&mut world);
    step(&mut world);
    let party = world.script_state().party.clone();
    let puppet = party.puppets.get(&(PUP as u8)).cloned();
    let made = puppet.as_ref().and_then(|p| p.object);
    let owner = puppet.as_ref().and_then(|p| p.owner);
    let joined = party.puppet_creatures().collect::<Vec<_>>();
    let companion = party
        .available
        .get(&(OTHER as u8))
        .and_then(|npc| npc.object);
    let came = made.is_some()
        && owner == Some(OTHER as u8)
        && joined == vec![(PUP as u8, made.unwrap_or(0))]
        && party.has(OTHER as u8)
        && companion.is_some()
        && made != companion;
    run(&mut world, "probe_to_base");
    let after = step(&mut world);
    let party = &world.script_state().party;
    let puppet = party.puppets.get(&(PUP as u8));
    let sent = tokens(&after, 30..=32) == ["1", "0", "0"]
        && !party.has(OTHER as u8)
        && party
            .available
            .get(&(OTHER as u8))
            .is_some_and(|npc| npc.object.is_none())
        && puppet.is_some_and(|p| !p.joined && p.object.is_none())
        && party.forfeit == 20;
    check(
        came && sent,
        format!(
            "puppet made {made:?}, owner {owner:?}, joined {joined:?}, its companion {companion:?}; \
             sent to base {:?}, its creature {:?}, the puppet {puppet:?}",
            tokens(&after, 30..=32),
            party.available.get(&(OTHER as u8)).map(|npc| npc.object),
        ),
    )
}

/// The duel rules an equip breaks, as the executable tests them.
fn forfeits() -> Result<String, String> {
    use crate::combat::sheet::{Delivery, SaberShape};
    use crate::inventory::slots;
    use crate::script::party::forfeit::{self, equipping};
    let saber = Some(Delivery::Saber(SaberShape::Single));
    let cases = [
        (
            forfeit::ARMOR,
            slots::BODY,
            "a_armor",
            None,
            Some(forfeit::ARMOR),
        ),
        (forfeit::ARMOR, slots::HEAD, "a_mask", None, None),
        (
            forfeit::WEAPONS | forfeit::LIGHTSABER,
            slots::RIGHT_WEAPON,
            "g_saber",
            saber,
            Some(forfeit::WEAPONS),
        ),
        (
            forfeit::DXUN_SWORD_ONLY,
            slots::RIGHT_WEAPON,
            "PL_SWORD",
            None,
            None,
        ),
        (
            forfeit::DXUN_SWORD_ONLY,
            slots::LEFT_WEAPON,
            "g_sword",
            None,
            Some(forfeit::DXUN_SWORD_ONLY),
        ),
        (
            forfeit::LIGHTSABER,
            slots::LEFT_WEAPON,
            "g_saber",
            saber,
            Some(forfeit::LIGHTSABER),
        ),
        (forfeit::LIGHTSABER, 18, "g_saber", saber, None),
    ];
    let wrong: Vec<_> = cases
        .iter()
        .filter(|(rules, slot, tag, delivery, want)| {
            equipping(*rules, *slot, tag, *delivery) != *want
        })
        .map(|(rules, slot, tag, ..)| (rules, slot, tag))
        .collect();
    check(
        wrong.is_empty(),
        format!("{} cases, wrong {wrong:?}", cases.len()),
    )
}

/// What using an item breaks, as the executable tests it: any item first,
/// then any but an energy shield.
fn forfeit_items() -> Result<String, String> {
    use crate::script::party::forfeit::{ITEM_BUT_SHIELD, ITEMS, using};
    let cases = [
        (ITEMS, true, Some(ITEMS)),
        (ITEMS | ITEM_BUT_SHIELD, false, Some(ITEMS)),
        (ITEM_BUT_SHIELD, false, Some(ITEM_BUT_SHIELD)),
        (ITEM_BUT_SHIELD, true, None),
        (0, false, None),
    ];
    let wrong: Vec<_> = cases
        .iter()
        .filter(|(rules, shield, want)| using(*rules, *shield) != *want)
        .collect();
    check(
        wrong.is_empty(),
        format!("{} cases, wrong {wrong:?}", cases.len()),
    )
}

/// The puppet of `probe_puppet`, put `PUT_APART` from its owner, following
/// it: `held`, at a range wider than that, its follow holds the queue step
/// after step; else it queues a run to where the owner stands and the
/// follow again behind it. The companion, no puppet, queues nothing; the
/// queue comes back from a save as it was.
fn follow_owner(held: bool) -> Result<String, String> {
    use crate::script::actions::{Action, Target};
    let (mut world, _) = world()?;
    step(&mut world);
    run(&mut world, "probe_puppet");
    step(&mut world);
    step(&mut world);
    let party = world.script_state().party.clone();
    let puppet = party
        .puppet_creatures()
        .next()
        .map(|(_, id)| id)
        .ok_or("no puppet joined")?;
    let owner = party
        .puppet_owner(puppet)
        .ok_or("the puppet has no owner")?;
    let (script, range) = match held {
        true => ("probe_follow_near", FOLLOW_NEAR),
        false => ("probe_follow_far", FOLLOW_FAR),
    };
    for (id, mut body) in world
        .world
        .query::<(&Id, &mut super::Player)>()
        .iter_mut(&mut world.world)
    {
        if id.0 == SimId(puppet) {
            body.0.origin[0] += PUT_APART / collision::UNIT;
        }
    }
    run_as(&mut world, script, ObjectId(puppet));
    run_as(&mut world, script, ObjectId(owner));
    // Queued in the first step, it moves on in the next (and holds on).
    step(&mut world);
    let mut last = step(&mut world);
    if held {
        last = step(&mut world);
    }
    let state = world.script_state();
    let queue: Vec<Action> = state.actions.get(&puppet).map_or(Vec::new(), |queue| {
        queue.iter().map(|queued| queued.action.clone()).collect()
    });
    let follow = Action::FollowOwner { range };
    let at = last.get(SimId(owner)).map(|entity| entity.position);
    let went = match queue.as_slice() {
        [again] if held => *again == follow,
        [
            Action::Move {
                to: Target::Point(to, _),
                run: true,
                range: moved,
                ..
            },
            again,
        ] if !held => {
            *moved == range
                && *again == follow
                && at.is_some_and(|at| crate::math::flat_distance(*to, at) < 0.01)
        }
        _ => false,
    };
    let none = !state.actions.get(&owner).is_some_and(|queue| {
        queue
            .iter()
            .any(|queued| matches!(queued.action, Action::FollowOwner { .. }))
    });
    let mut out = crate::wire::Writer::default();
    state.encode(&mut out);
    let kept = crate::script::State::decode(&mut crate::wire::Reader::new("state", &out.0))
        .is_ok_and(|back| back.actions.get(&puppet) == state.actions.get(&puppet));
    check(
        went && none && kept,
        format!(
            "the puppet's queue {queue:?} (owner at {at:?}); the companion's {:?}; kept {kept}",
            state.actions.get(&owner)
        ),
    )
}

/// Bao-Dur is met for the screens once a script sets his influence.
fn bao_dur_met() -> Result<String, String> {
    let (mut world, _) = world()?;
    let before = step(&mut world).screens.bao_dur_met;
    run(&mut world, "probe_bao_dur");
    let after = step(&mut world).screens.bao_dur_met;
    check(
        !before && after,
        format!("before {before}, after SetInfluence(NPC_BAO_DUR, 50) {after}"),
    )
}

/// A companion spawned twice in one script (`SpawnAvailableNPC`) stands
/// once: the second takes the first one's place, the party table keeping
/// one creature a companion.
fn spawned_twice_is_one() -> Result<String, String> {
    let (mut world, _) = world()?;
    run(&mut world, "probe_spawned_twice");
    step(&mut world);
    run(&mut world, "probe_second_atton");
    let told = tokens(&step(&mut world), 40..=41);
    check(
        told == ["0", "1"],
        format!("(a second, a first) atton: {told:?}"),
    )
}

/// A companion spawned again in a later step while its creature is here
/// (`SpawnAvailableNPC`) is that creature, moved: Nar Shaddaa's Atton,
/// spawned to warn the player and spawned again by his warning's first
/// line (`a_atton_cut_1`), speaks it to its end.
fn spawned_again_is_the_same() -> Result<String, String> {
    let (mut world, _) = world()?;
    run(&mut world, "probe_atton_beside");
    step(&mut world);
    run(&mut world, "probe_spawned_again");
    step(&mut world);
    run(&mut world, "probe_second_atton");
    let told = tokens(&step(&mut world), 40..=42);
    check(
        told == ["0", "1", "1"],
        format!("(a second, a first, the same) atton: {told:?}"),
    )
}

/// `GetNPCSelectability` is -1 for a companion not in the party table
/// (TSL's nwscript.nss), else 1 or 0 as it may be picked: the Ebon Hawk's
/// Visas Marr arrives only while she is not yet available
/// (`k_003ebo_enter`'s test of 1 or 0).
fn selectability_told() -> Result<String, String> {
    let (mut world, _) = world()?;
    run(&mut world, "probe_selectability");
    let told = tokens(&step(&mut world), 80..=82);
    check(
        told == ["-1", "1", "0"],
        format!("(never available, available, unselectable): {told:?}"),
    )
}

/// TSL's executable runs `k_align_movie` as the player closes the status
/// summary (`CSWGuiStatusSummary::HandleInputEvent`), which shows once
/// statuses are told, no conversation holds the world and the module's
/// opening fade has lifted: not before a status is told, then once. The
/// Visas Marr movies, and so Visas's arrival on the Ebon Hawk, hang on it.
fn summary_closed_runs_align_movie() -> Result<String, String> {
    let (mut world, _) = world()?;
    let before = tokens(&step(&mut world), 70..=70);
    run(&mut world, "probe_light_point");
    let lifted = crate::snapshot::LIFT_MS.div_ceil(MSEC) + 2;
    let mut last = step(&mut world);
    for _ in 0..lifted {
        last = step(&mut world);
    }
    let after = tokens(&last, 70..=70);
    check(
        before == ["(unset)"] && after == ["1"],
        format!("k_align_movie's token before {before:?}, after a light side point {after:?}"),
    )
}

/// The status summary waits while a fade covers the screen (TSL's
/// `MainLoop` asks `IsGlobalFadeObscuring` and `IsGlobalFading` before
/// `ShowStatusSummary`): a light side point told under a fade out runs no
/// `k_align_movie` until `SetGlobalFadeIn` clears the screen.
fn summary_waits_for_the_fade() -> Result<String, String> {
    let (mut world, _) = world()?;
    step(&mut world);
    run(&mut world, "probe_fade_out");
    run(&mut world, "probe_light_point");
    step(&mut world);
    let covered = tokens(&step(&mut world), 70..=70);
    run(&mut world, "probe_fade_in");
    step(&mut world);
    let cleared = tokens(&step(&mut world), 70..=70);
    check(
        covered == ["(unset)"] && cleared == ["1"],
        format!("k_align_movie's token under the fade {covered:?}, once it lifts {cleared:?}"),
    )
}

/// The lines an item's description begins with: "Cannot Equip:", "Bao-Dur",
/// a blank line, for each base item named once he is met (the grenade 97
/// too, Q-0266); none before, none for a shield (base item 46).
fn cannot_equip_lines() -> Result<String, String> {
    use crate::snapshot::cannot_equip::{BAO_DUR, BASE_ITEMS, CANNOT_EQUIP, lines};
    let line = |strref| match strref {
        CANNOT_EQUIP => b"Cannot Equip".to_vec(),
        BAO_DUR => b"Bao-Dur".to_vec(),
        _ => Vec::new(),
    };
    let named = BASE_ITEMS
        .iter()
        .all(|base| lines(*base, true, line) == b"Cannot Equip:\nBao-Dur\n\n")
        && BASE_ITEMS == [35, 36, 37, 97, 98, 99, 102];
    let unmet = lines(BASE_ITEMS[0], false, line).is_empty();
    let shield = lines(46, true, line).is_empty();
    check(
        named && unmet && shield,
        format!("named {named}, before met none {unmet}, a shield none {shield}"),
    )
}
