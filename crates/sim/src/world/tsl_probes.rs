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
    // rules set (31 the last broken: none).
    let mut base = Asm::default();
    token(&mut base, 30, &[int(OTHER), call(22, 1)]);
    base.op(int(20)).op(call(23, 1));
    token(&mut base, 31, &[call(24, 0)]);
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
    checks.push(("cannot_equip_lines", cannot_equip_lines()));
    checks.push(("influence_starts", influence_starts()));
    checks.push(("alignment_follows_influence", follows()));
    checks
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
/// creature gone and the table still knowing both.
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
    let sent = tokens(&after, 30..=31) == ["1", "0"]
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
            tokens(&after, 30..=31),
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
