//! The probes of TSL's combat and Force rules (`kjk sim probes tsl force`;
//! `context/artifacts/i35/combat-force.md`, its executable's numbers), on
//! the proving ground made TSL's, no install:
//! - the Forms' rules (`combat::forms`): each Form's attack, defence against
//!   its target and others, deflection, damage, critical multiplier, threat
//!   range, confirmation and extra attack, a lightsaber Form only with a
//!   lightsaber in the right hand;
//! - a power's cost (`ForceRules::cost`): the alignment's multiplier for a
//!   party member, eased by Charisma, the room's Force rating, Force
//!   Potency's 1.2; where a place's room is (`ForceRules::rating`);
//! - Force point regeneration (`force::regenerate_tsl`): the bases, the
//!   Forms, the feat, the effects, the room by alignment; in the world the
//!   party regenerates by it and no one else does;
//! - the Forms and powers levels grant (`levels::granted_powers`);
//! - the routines (`script::force`, `script::traps`): the spell book, the
//!   Forms in use, a Form picked (`Action::Form`), bonus Force points and
//!   their status, the stat adjusters and skill ranks, the racial subtype,
//!   the queue and gait, fake combat, the flourish, `ForceHeartbeat`,
//!   spawned mines, the Demolitions a mine keeps and a door mine's blast
//!   reads;
//! - the forfeit rule on a power the player orders
//!   (`FORFEIT_NO_FORCE_POWERS`): broken by the press of one it knows and
//!   can pay for, by the power button or a held power's own; not by one it
//!   cannot pay for, nor under other rules.

use fmt_odyssey::ncs::{Const, Ncs, Op};
use script_ncs::probes::Asm;
use script_ncs::vm::ObjectId;

use super::{Fighting, Id, PlayerSheet, SimWorld};
use crate::combat::Sheet;
use crate::combat::forms::{self, *};
use crate::combat::sheet::{Ability, Side, Weapon, factions, skills};
use crate::input::{Seat, TickInput};
use crate::module::{ForceRoom, ForceRules, ObjectData, ObjectKind, Placement};
use crate::probe::{Checks, check};
use crate::script::{FpRegeneration, Pending, Regeneration};
use crate::snapshot::{Kind, SimId, Snapshot, StatusKind};
use crate::{Level, StepReason};

const MSEC: u32 = 8;

const CATALOG: &str = r#"
// 0
void SetCustomToken(int nCustomTokenNumber, string sTokenValue);
// 1
string IntToString(int nInteger);
// 2
object GetFirstPC();
// 3
int GetSpellBaseForcePointCost(int nSpellID);
// 4
int GetSpellAcquired(int nSpell, object oCreature=OBJECT_SELF);
// 5
int GetHasSpell(int nSpell, object oCreature=OBJECT_SELF);
// 6
void GrantSpell(int nSpell, object oCreature);
// 7
int GetSpellFormMask(int nSpellID);
// 8
void SetCurrentForm(object oCreature, int nFormID);
// 9
int IsFormActive(object oCreature, int nFormID);
// 10
void AddBonusForcePoints(object oCreature, int nBonusFP);
// 11
void SetBonusForcePoints(object oCreature, int nBonusFP);
// 12
int GetBonusForcePoints(object oCreature);
// 13
int GetMaxForcePoints(object oObject=OBJECT_SELF);
// 14
void AdjustCreatureAttributes(object oObject, int nAttribute, int nAmount);
// 15
void AdjustCreatureSkills(object oObject, int nSkill, int nAmount);
// 16
int GetSkillRank(int nSkill, object oTarget=OBJECT_SELF);
// 17
int GetSkillRankBase(int nSkill, object oObject=OBJECT_SELF);
// 18
void ModifyFortitudeSavingThrowBase(object aObject, int aModValue);
// 19
void ModifyReflexSavingThrowBase(object aObject, int aModValue);
// 20
void ModifyWillSavingThrowBase(object oObject, int aModValue);
// 21
int GetRacialSubType(object oTarget);
// 22
object GetObjectByTag(string sTag, int nNth=0);
// 23
int IsRunning(object oCreature);
// 24
int GetCombatActionsPending(object oCreature);
// 25
int IsInTotalDefense(object oCreature);
// 26
void SetFakeCombatState(object oObject, int nEnable);
// 27
int GetIsInCombat(object oCreature=OBJECT_SELF, int bOnlyCountReal=FALSE);
// 28
void CreatureFlourishWeapon(object oObject);
// 29
void ForceHeartbeat(object oCreature);
// 30
int GetOwnerDemolitionsSkill(object oObject);
// 31
void SpawnMine(int nMineType, location lPoint, int nDetectDCBase, int nDisarmDCBase, object oCreator);
// 32
void DetonateMine(object oMine);
// 33
location GetLocation(object oObject);
// 34
void ActionWait(float fSeconds);
// 35
void SetLocalNumber(object oObject, int nIdx, int nValue);
// 36
int GetLocalNumber(object oObject, int nIdx);
// 37
vector Vector(float x=0.0, float y=0.0, float z=0.0);
// 38
int HasLineOfSight(vector vSource, vector vTarget, object oSource=OBJECT_INVALID, object oTarget=OBJECT_INVALID);
// 39
void SetHealTarget(object oidHealer, object oidTarget);
// 40
object GetHealTarget(object oidHealer);
// 41
int GetIsObjectValid(object oObject);
"#;

/// The rows the probes use: a power the player knows and one it does not
/// (Breath Control, granted), a light and a dark power, a mine's row.
const KNOWN: u16 = 9;
const GRANTED: u16 = 270;
const LIGHT: u16 = 5;
const DARK: u16 = 6;
const MINE_ROW: u8 = 1;
/// The NPC Jedi's tag, the waypoint mines spawn at.
const JEDI: &str = "probe_jedi";
const SPOT: &str = "probe_spot";

fn call(routine: u16, args: u8) -> Op {
    Op::Action { routine, args }
}

fn int(value: i32) -> Op {
    Op::Const(Const::Int(value))
}

fn text(value: &str) -> Op {
    Op::Const(Const::String(value.as_bytes().to_vec()))
}

fn pc() -> Op {
    call(2, 0)
}

fn tagged(tag: &str) -> [Op; 3] {
    [int(0), text(tag), call(22, 2)]
}

/// Custom token `n` set to the int `value` leaves.
fn token(asm: &mut Asm, n: i32, value: &[Op]) {
    for op in value {
        asm.op(op.clone());
    }
    asm.op(call(1, 1)).op(int(n)).op(call(0, 2));
}

/// `routine(who, arg)` with no result.
fn on(asm: &mut Asm, routine: u16, who: &[Op], arg: i32) {
    asm.op(int(arg));
    for op in who {
        asm.op(op.clone());
    }
    asm.op(call(routine, 2));
}

/// `routine(row, who)` with a result, as token `n`.
fn ask(asm: &mut Asm, n: i32, routine: u16, row: i32, who: &[Op]) {
    let mut value: Vec<Op> = who.to_vec();
    value.push(int(row));
    value.push(call(routine, 2));
    token(asm, n, &value);
}

fn done(mut asm: Asm) -> Ncs {
    asm.op(Op::Retn);
    asm.build()
}

/// `probe_book`: the spell book (tokens 1 to 8).
fn book() -> Ncs {
    let mut asm = Asm::default();
    token(&mut asm, 1, &[int(i32::from(GRANTED)), call(3, 1)]);
    token(&mut asm, 2, &[int(999), call(3, 1)]);
    ask(&mut asm, 3, 4, i32::from(KNOWN), &[pc()]);
    ask(&mut asm, 4, 5, i32::from(KNOWN), &[pc()]);
    ask(&mut asm, 5, 4, i32::from(GRANTED), &[pc()]);
    asm.op(pc()).op(int(i32::from(GRANTED))).op(call(6, 2));
    ask(&mut asm, 6, 4, i32::from(GRANTED), &[pc()]);
    token(&mut asm, 7, &[int(i32::from(SHII_CHO)), call(7, 1)]);
    token(&mut asm, 8, &[int(i32::from(GRANTED)), call(7, 1)]);
    done(asm)
}

/// `IsFormActive(pc, form)` as token `n`.
fn active_token(asm: &mut Asm, n: i32, form: u16) {
    token(asm, n, &[int(i32::from(form)), pc(), call(9, 2)]);
}

/// `probe_forms`: a Form it does not know set (token 10), Shii-Cho set
/// (11, and Force Focus not in force, 12), Force Focus set (13).
fn form_routines() -> Ncs {
    let mut asm = Asm::default();
    on(&mut asm, 8, &[pc()], i32::from(MAKASHI));
    active_token(&mut asm, 10, MAKASHI);
    on(&mut asm, 8, &[pc()], i32::from(SHII_CHO));
    active_token(&mut asm, 11, SHII_CHO);
    active_token(&mut asm, 12, FOCUS);
    on(&mut asm, 8, &[pc()], i32::from(FOCUS));
    active_token(&mut asm, 13, FOCUS);
    done(asm)
}

/// `probe_bonus_add`: 25 bonus Force points added (token 20 the bonus).
/// `probe_bonus_read`: the most FP (21), then the bonus set to 10 (22).
fn bonus() -> (Ncs, Ncs) {
    let mut add = Asm::default();
    on(&mut add, 10, &[pc()], 25);
    token(&mut add, 20, &[pc(), call(12, 1)]);
    let mut read = Asm::default();
    token(&mut read, 21, &[pc(), call(13, 1)]);
    on(&mut read, 11, &[pc()], 10);
    token(&mut read, 22, &[pc(), call(12, 1)]);
    (done(add), done(read))
}

/// `probe_stats`: Constitution +2, Demolitions +3, base saves -1, +1, +2.
/// `probe_ranks`: Awareness's rank (token 30) and base (31), Demolitions'
/// (32, 33), Stealth's of no ranks (34), a door's (35); the racial subtype
/// of no object (36), of the door (37), of the player (38).
fn stats() -> (Ncs, Ncs) {
    let mut asm = Asm::default();
    asm.op(int(2))
        .op(int(Ability::Constitution as i32))
        .op(pc())
        .op(call(14, 3));
    asm.op(int(3))
        .op(int(skills::DEMOLITIONS as i32))
        .op(pc())
        .op(call(15, 3));
    for (routine, by) in [(18, -1), (19, 1), (20, 2)] {
        on(&mut asm, routine, &[pc()], by);
    }
    let mut ranks = Asm::default();
    let gate = tagged("gate");
    for (n, routine, skill, who) in [
        (30, 16, skills::AWARENESS, &[pc()][..]),
        (31, 17, skills::AWARENESS, &[pc()][..]),
        (32, 16, skills::DEMOLITIONS, &[pc()][..]),
        (33, 17, skills::DEMOLITIONS, &[pc()][..]),
        (34, 16, skills::STEALTH, &[pc()][..]),
        (35, 16, skills::AWARENESS, &gate[..]),
    ] {
        ask(&mut ranks, n, routine, skill as i32, who);
    }
    token(&mut ranks, 36, &[Op::Const(Const::Object(1)), call(21, 1)]);
    let mut door = gate.to_vec();
    door.push(call(21, 1));
    token(&mut ranks, 37, &door);
    token(&mut ranks, 38, &[pc(), call(21, 1)]);
    (done(asm), done(ranks))
}

/// `probe_queue`, run as the NPC: its queue before and after a wait is
/// queued (tokens 40, 41), whether it runs (42), in total defence (43).
fn queue() -> Ncs {
    let me = || [Op::Const(Const::Object(0))];
    let mut asm = Asm::default();
    token(&mut asm, 40, &[me()[0].clone(), call(24, 1)]);
    asm.op(Op::Const(Const::Float(5.0))).op(call(34, 1));
    token(&mut asm, 41, &[me()[0].clone(), call(24, 1)]);
    token(&mut asm, 42, &[me()[0].clone(), call(23, 1)]);
    token(&mut asm, 43, &[me()[0].clone(), call(25, 1)]);
    done(asm)
}

/// `probe_fake_on` / `probe_fake_off`: the NPC's fake combat set and let
/// go. `probe_fake_ask`: in combat (token 50), in real combat (51).
/// `probe_flourish`: its weapon flourished.
fn fake() -> [(&'static str, Ncs); 4] {
    let jedi = tagged(JEDI);
    let set = |on_off: i32| {
        let mut asm = Asm::default();
        on(&mut asm, 26, &jedi, on_off);
        done(asm)
    };
    let mut ask = Asm::default();
    for (n, only_real) in [(50, 0), (51, 1)] {
        let mut value = vec![int(only_real)];
        value.extend(jedi.iter().cloned());
        value.push(call(27, 2));
        token(&mut ask, n, &value);
    }
    let mut flourish = Asm::default();
    for op in jedi.iter() {
        flourish.op(op.clone());
    }
    flourish.op(call(28, 1));
    [
        ("probe_fake_on", set(1)),
        ("probe_fake_off", set(0)),
        ("probe_fake_ask", done(ask)),
        ("probe_flourish", done(flourish)),
    ]
}

/// `probe_beat`, the NPC's heartbeat: counts its runs in its local number
/// 1, told as token 60. `probe_force_beat`: `ForceHeartbeat` on it.
fn beats() -> (Ncs, Ncs) {
    let me = Op::Const(Const::Object(0));
    let mut beat = Asm::default();
    beat.op(int(1))
        .op(me.clone())
        .op(call(36, 2))
        .op(int(1))
        .op(Op::Add(fmt_odyssey::ncs::Pair::IntInt))
        .op(int(1))
        .op(me.clone())
        .op(call(35, 3));
    token(&mut beat, 60, &[int(1), me.clone(), call(36, 2)]);
    let mut force = Asm::default();
    for op in tagged(JEDI) {
        force.op(op);
    }
    force.op(call(29, 1));
    (done(beat), done(force))
}

/// `probe_mines`: a mine spawned at the spot with no creator (DCs +2, +3),
/// then one the player spawned. Run as a mine, `probe_mine_owner`: the
/// Demolitions it was set with (token 70); `probe_detonate` sets it off.
fn mines() -> [(&'static str, Ncs); 3] {
    let spot = tagged(SPOT);
    let mut spawn = Asm::default();
    for creator in [Op::Const(Const::Object(1)), pc()] {
        spawn.op(creator).op(int(3)).op(int(2));
        for op in spot.iter() {
            spawn.op(op.clone());
        }
        spawn
            .op(call(33, 1))
            .op(int(i32::from(MINE_ROW)))
            .op(call(31, 5));
    }
    let me = Op::Const(Const::Object(0));
    let mut owner = Asm::default();
    token(&mut owner, 70, &[me.clone(), call(30, 1)]);
    let mut detonate = Asm::default();
    detonate.op(me).op(call(32, 1));
    [
        ("probe_mines", done(spawn)),
        ("probe_mine_owner", done(owner)),
        ("probe_detonate", done(detonate)),
    ]
}

/// The proving ground's lines `probe_sight` asks after, each its two
/// ends' (x, y) on the floor: across the open floor; along the foot of the
/// ramp's high end (1.1 m up there: under a standing eye, over the feet);
/// through the wall across.
const LINES: [[[f32; 2]; 2]; 3] = [
    [[-6.0, -1.0], [-2.0, 3.0]],
    [[-3.2, -6.0], [-3.2, -3.0]],
    [[-3.0, -4.0], [3.0, -4.0]],
];

/// `Vector(x, y, z)` of a floor point.
fn point(asm: &mut Asm, [x, y]: [f32; 2]) {
    let [x, y, z] = crate::determinism::ground_point(x, y, 0.0);
    for v in [z, y, x] {
        asm.op(Op::Const(Const::Float(v)));
    }
    asm.op(call(37, 3));
}

/// `probe_sight`: `HasLineOfSight` between each line's ends (tokens 80 to
/// 82). `probe_heal`: a heal target set on the door (ignored) and on the
/// NPC, each asked after (83, 84).
fn sight_and_heal() -> (Ncs, Ncs) {
    let mut sight = Asm::default();
    for (n, [a, b]) in (80..).zip(LINES) {
        sight
            .op(Op::Const(Const::Object(1)))
            .op(Op::Const(Const::Object(1)));
        point(&mut sight, b);
        point(&mut sight, a);
        sight
            .op(call(38, 4))
            .op(call(1, 1))
            .op(int(n))
            .op(call(0, 2));
    }
    let mut heal = Asm::default();
    for (n, healer) in [(83, "gate"), (84, JEDI)] {
        heal.op(pc());
        for op in tagged(healer) {
            heal.op(op);
        }
        heal.op(call(39, 2));
        let mut value = tagged(healer).to_vec();
        value.extend([call(40, 1), call(41, 1)]);
        token(&mut heal, n, &value);
    }
    (done(sight), done(heal))
}

fn library(regeneration: bool) -> crate::script::Library {
    let (add, read) = bonus();
    let (adjust, ranks) = stats();
    let (beat, force_beat) = beats();
    let (sight, heal) = sight_and_heal();
    let mut scripts = vec![
        ("probe_book", book()),
        ("probe_forms", form_routines()),
        ("probe_bonus_add", add),
        ("probe_bonus_read", read),
        ("probe_stats", adjust),
        ("probe_ranks", ranks),
        ("probe_queue", queue()),
        ("probe_beat", beat),
        ("probe_force_beat", force_beat),
        ("probe_sight", sight),
        ("probe_heal", heal),
    ];
    scripts.extend(fake());
    scripts.extend(mines());
    let library = crate::probe::library(CATALOG, scripts);
    if regeneration {
        library.with_regeneration(Some(Regeneration {
            time_per_hp: 6.0,
            fp: TSL_FP,
            ..Regeneration::default()
        }))
    } else {
        library
    }
}

/// TSL's `regeneration.2da` Force rows.
const TSL_FP: FpRegeneration = FpRegeneration {
    time: 90.0,
    out_of_combat: 1.0,
    in_combat: 0.1,
    wis_bonus: 0.0,
};

/// TSL's `forceadjust.2da` (good, evil) by alignment tenth.
const ADJUST: [[f32; 2]; 11] = [
    [1.75, 0.5],
    [1.5, 0.7],
    [1.5, 0.8],
    [1.25, 0.85],
    [1.25, 0.9],
    [1.0, 1.0],
    [0.9, 1.25],
    [0.85, 1.25],
    [0.8, 1.5],
    [0.7, 1.5],
    [0.5, 1.75],
];

/// TSL's Force rules for the probes' rows: `LIGHT` and `DARK` sided, the
/// Forms' mask; `rooms` as given.
fn rules(rooms: Vec<ForceRoom>) -> ForceRules {
    let mut sides = vec![Side::Neutral; 300];
    sides[usize::from(LIGHT)] = Side::Light;
    sides[usize::from(DARK)] = Side::Dark;
    let mut masks = vec![0; 300];
    for form in forms::ALL {
        masks[usize::from(form)] = forms::FORM_MASK;
    }
    ForceRules {
        sides,
        masks,
        adjust: ADJUST.to_vec(),
        rooms,
    }
}

/// A lightsaber, and a blaster pistol.
fn saber() -> Weapon {
    Sheet::dev_jedi().weapons[0].expect("the dev Jedi holds a lightsaber")
}

fn blaster() -> Weapon {
    Sheet::dev_soldier().weapons[0].expect("the dev soldier holds a blaster")
}

/// The probes' TSL Jedi: the dev Jedi knowing Shii-Cho and Force Focus,
/// INT 14, Demolitions 4.
fn jedi_sheet() -> Sheet {
    let mut sheet = Sheet::dev_jedi();
    sheet.powers.insert(SHII_CHO);
    sheet.powers.insert(FOCUS);
    sheet.abilities[Ability::Intelligence as usize] = 14;
    sheet.skills[skills::DEMOLITIONS] = 4;
    sheet.flags |= crate::combat::sheet::flags::TSL;
    sheet
}

/// The proving ground made TSL's: the probes' costs, mine row and Force
/// rules, an NPC Jedi of a neutral faction (heartbeat `probe_beat`) and a
/// waypoint.
fn world(regeneration: bool) -> Result<SimWorld, String> {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    data.game = "tsl".into();
    data.force_costs = vec![0; 300];
    for (row, cost) in [(KNOWN, 10), (GRANTED, 20), (LIGHT, 10), (DARK, 10)] {
        data.force_costs[usize::from(row)] = cost;
    }
    data.traps = vec![crate::traps::TrapRow::default(); 2];
    data.traps[usize::from(MINE_ROW)] = crate::traps::TrapRow {
        label: "TRAP_MINE_PROBE".into(),
        script: String::new(),
        set_dc: 15,
        detect_mod: 15,
        disarm_mod: 25,
        ..crate::traps::TrapRow::default()
    };
    data.force = rules(Vec::new());
    let mut npc = ObjectData::creature(
        JEDI,
        JEDI,
        crate::determinism::ground_point(-6.0, 3.0, 0.0),
        Sheet {
            faction: factions::NEUTRAL,
            ..jedi_sheet()
        },
    );
    npc.scripts = vec![("ScriptHeartbeat".into(), "probe_beat".into())];
    let spot = ObjectData {
        kind: ObjectKind::Waypoint,
        tag: SPOT.into(),
        ..ObjectData::unknown(SPOT, crate::determinism::ground_point(-6.0, -3.0, 0.0), 0.0)
    };
    data.placements.extend([npc, spot].map(Placement::Own));
    let level: Level = ground.with_module(data).with_scripts(library(regeneration));
    SimWorld::new(
        crate::WorldConfig {
            sheet: jedi_sheet(),
            ..crate::WorldConfig::one_seat()
        },
        level,
    )
    .map_err(|error| error.to_string())
}

fn step(world: &mut SimWorld, input: &TickInput) -> Snapshot {
    let mut input = input.clone();
    if input.actions().is_empty() {
        input.insert(Seat(0), crate::input::Command::default());
    }
    let tick = world.tick() + 1;
    crate::step(world, tick, &input, MSEC, StepReason::Authority)
}

fn idle(world: &mut SimWorld) -> Snapshot {
    step(world, &TickInput::default())
}

/// `ms` of standing.
fn wait(world: &mut SimWorld, ms: u64) -> Snapshot {
    let mut last = idle(world);
    for _ in 1..ms / u64::from(MSEC) {
        last = idle(world);
    }
    last
}

/// Runs `script` as `caller` in the next step.
fn run(world: &mut SimWorld, script: &str, caller: ObjectId) -> Snapshot {
    let mut state = world.script_state().clone();
    state.pending.push(Pending::new(script, caller));
    world.set_script_state(state);
    idle(world)
}

fn module(world: &mut SimWorld, script: &str) -> Snapshot {
    run(world, script, crate::script::MODULE)
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

/// The NPC Jedi: its id.
fn npc(snapshot: &Snapshot) -> Option<SimId> {
    snapshot
        .entities
        .iter()
        .find(|entity| matches!(entity.kind, Kind::Object(ObjectKind::Creature, _)))
        .map(|entity| entity.id)
}

/// `change` on `who`'s fighter.
fn fight(world: &mut SimWorld, who: SimId, change: impl FnOnce(&mut crate::combat::Fighter)) {
    for (id, mut fighting) in world
        .world
        .query::<(&Id, &mut Fighting)>()
        .iter_mut(&mut world.world)
    {
        if id.0 == who {
            change(&mut fighting.0);
            return;
        }
    }
}

/// `change` on `who`'s sheet (a player's own).
fn reshape(world: &mut SimWorld, who: SimId, change: impl FnOnce(&mut Sheet)) {
    for (id, mut sheet) in world
        .world
        .query::<(&Id, &mut PlayerSheet)>()
        .iter_mut(&mut world.world)
    {
        if id.0 == who {
            change(&mut sheet.0);
            return;
        }
    }
}

fn sheet_of(snapshot: &Snapshot, who: SimId) -> Option<Sheet> {
    snapshot.get(who).and_then(|entity| entity.sheet)
}

fn fighter_of(snapshot: &Snapshot, who: SimId) -> Option<crate::combat::Fighter> {
    snapshot.get(who).and_then(|entity| entity.fighter)
}

/// A sheet holding `weapon` in its right hand in `form`.
fn holding(weapon: Weapon, form: u16) -> Sheet {
    let mut sheet = jedi_sheet();
    sheet.weapons = [Some(weapon), None];
    sheet.form = form;
    sheet
}

/// Each Form's rules with a lightsaber (against a lightsaber wielder and a
/// gunner), and with a blaster: attack, defence against its target and
/// others, deflection, damage, critical multiplier, threat range (a weapon
/// of 2), confirmation, extra attacks.
fn form_numbers() -> Result<String, String> {
    type Row = (u16, [i32; 10]);
    let expected: [Row; 11] = [
        (SHII_CHO, [1, 1, 0, 3, 0, 0, 0, 2, 0, 0]),
        (MAKASHI, [3, 0, 0, 0, -5, 3, 0, 2, 0, 0]),
        (SORESU, [0, 0, 2, 0, 4, 0, 0, 1, 0, 0]),
        (ATARU, [0, 0, 3, -2, -4, 0, 0, 3, 0, 0]),
        (SHIEN, [2, 2, -5, 0, 2, 0, 1, 2, 0, 0]),
        (NIMAN, [1, 1, 1, 1, 1, 0, 0, 2, 0, 0]),
        (JUYO, [0, 0, -2, -4, 0, 0, 0, 2, 4, 1]),
        (FOCUS, [0, 0, 0, 0, 0, 0, 0, 2, 0, 0]),
        (POTENCY, [0, 0, 0, 0, 0, 0, 0, 2, 0, 0]),
        (AFFINITY, [0, 0, 0, 0, 0, 0, 0, 2, 0, 0]),
        (MASTERY, [0, 0, 0, 0, 0, 0, 0, 2, 0, 0]),
    ];
    let numbers = |sheet: &Sheet| {
        let weapon = forms::weapon(sheet, saber());
        let base = saber();
        [
            forms::to_hit(sheet, &holding(saber(), 0)),
            forms::to_hit(sheet, &holding(blaster(), 0)),
            forms::defence(sheet, true),
            forms::defence(sheet, false),
            i32::from(forms::deflection(sheet)),
            i32::from(weapon.damage_bonus - base.damage_bonus),
            i32::from(weapon.crit_mult) - i32::from(base.crit_mult),
            i32::from(weapon.crit_threat),
            forms::confirm(sheet),
            forms::extra_attacks(sheet),
        ]
    };
    let wrong: Vec<String> = expected
        .iter()
        .filter_map(|(form, want)| {
            let got = numbers(&holding(saber(), *form));
            (got != *want).then(|| format!("{form}: {got:?}, not {want:?}"))
        })
        .collect();
    // With a blaster in hand the lightsaber Forms do nothing; the Force
    // Forms stay in force.
    let held: Vec<(u16, bool)> = forms::ALL
        .map(|form| (form, forms::is_active(&holding(blaster(), form), form)))
        .collect();
    let blasting = held
        .iter()
        .all(|(form, on)| *on == !forms::SABER_FORMS.contains(form));
    let idle = forms::SABER_FORMS
        .clone()
        .all(|form| numbers(&holding(blaster(), form)) == [0, 0, 0, 0, 0, 0, 0, 2, 0, 0]);
    check(
        wrong.is_empty() && blasting && idle,
        format!("{wrong:?}; with a blaster in force: {held:?}"),
    )
}

/// A power's cost: a light (80) party member of CHA 14 pays 14 for a dark
/// power of 10 (1.5 eased to 1.4), 16 under Force Potency; one out of the
/// party 10; a light power 8 (0.8); in a room of Force rating −100 the
/// light pays 5 more (13), the dark (20) 5 less (a dark power at 0.8: 3);
/// at least 1.
fn costs() -> Result<String, String> {
    let force = rules(Vec::new());
    let caster = |good_evil: u8, form: u16| Sheet {
        good_evil,
        form,
        abilities: [14, 14, 12, 10, 16, 14],
        ..holding(saber(), form)
    };
    let cost = |row: u16, base: u16, sheet: Sheet, party: bool, rating: i32| {
        force.cost(row, base, &sheet, party, rating)
    };
    let mut potent = caster(80, POTENCY);
    potent.powers.insert(POTENCY);
    let got = [
        cost(DARK, 10, caster(80, 0), true, 0),
        cost(DARK, 10, potent, true, 0),
        cost(DARK, 10, caster(80, 0), false, 0),
        cost(LIGHT, 10, caster(80, 0), true, 0),
        cost(LIGHT, 10, caster(80, 0), true, -100),
        cost(DARK, 10, caster(20, 0), true, -100),
        cost(DARK, 2, caster(20, 0), true, -100),
        cost(DARK, 0, caster(80, 0), true, 0),
    ];
    check(got == [14, 16, 10, 8, 13, 3, 1, 0], format!("{got:?}"))
}

/// A place's room: the first room in order with a floor face under it.
fn rooms() -> Result<String, String> {
    let square = |x: f32, y: f32| {
        vec![
            [[x, y], [x + 2.0, y], [x + 2.0, y + 2.0]],
            [[x, y], [x + 2.0, y + 2.0], [x, y + 2.0]],
        ]
    };
    let force = rules(vec![
        ForceRoom::new(0, square(0.0, 0.0)),
        ForceRoom::new(-50, square(1.0, 1.0)),
    ]);
    let got = [[1.5, 1.5, 0.0], [2.5, 2.5, 9.0], [5.0, 5.0, 0.0]].map(|at| force.rating(at));
    check(got == [0, -50, 0], format!("{got:?}"))
}

/// A second of TSL's Force point regeneration for a pool of 90 FP
/// (hundredths gained): out of a fight 100, in one 10, Niman 30, Force
/// Affinity 60, Force Focus 150, the feat 125, a 50% effect 150, a light
/// one in a room of rating 50 150 and a dark one 50; by 8 ms steps 100.
fn regeneration() -> Result<String, String> {
    let gained = |sheet: Sheet, (fighting, percent, rating): (bool, i32, i32), steps: u32| {
        let mut state = crate::combat::force::ForceState::default();
        let mut fp = 0;
        for _ in 0..steps {
            crate::combat::force::regenerate_tsl(
                &mut state,
                &mut fp,
                &sheet,
                TSL_FP,
                (fighting, percent, rating, 0),
                1000 / steps,
            );
        }
        fp
    };
    let pool = |form: u16| Sheet {
        max_fp: 90,
        good_evil: 50,
        ..holding(saber(), form)
    };
    let mut feat = pool(0);
    feat.feats.insert(crate::combat::force::REGENERATE_FEAT);
    let aligned = |good_evil: u8| Sheet {
        good_evil,
        ..pool(0)
    };
    let got = [
        gained(pool(0), (false, 0, 0), 1),
        gained(pool(0), (true, 0, 0), 1),
        gained(pool(NIMAN), (true, 0, 0), 1),
        gained(pool(AFFINITY), (true, 0, 0), 1),
        gained(pool(FOCUS), (false, 0, 0), 1),
        gained(feat, (false, 0, 0), 1),
        gained(pool(0), (false, 50, 0), 1),
        gained(aligned(80), (false, 0, 50), 1),
        gained(aligned(20), (false, 0, 50), 1),
        gained(pool(0), (false, 0, 0), 125),
    ];
    check(
        got == [100, 10, 30, 60, 150, 125, 150, 150, 50, 100],
        format!("{got:?}"),
    )
}

/// The Forms a Guardian, Consular and Sentinel are granted at Jedi levels
/// 11 to 14 (a prestige class's levels counted), none before or after; a
/// prestige class's powers at its own levels.
fn granted() -> Result<String, String> {
    use crate::levels::granted_powers;
    let got: Vec<Vec<u16>> = [
        vec![(3, 10)],
        vec![(3, 11)],
        vec![(3, 12)],
        vec![(4, 13)],
        vec![(5, 14)],
        vec![(3, 15)],
        vec![(4, 10), (15, 1)],
        vec![(5, 4), (13, 7)],
        vec![(3, 8), (12, 5)],
    ]
    .iter()
    .map(granted_powers)
    .collect();
    let want: Vec<Vec<u16>> = vec![
        vec![],
        vec![258],
        vec![259],
        vec![259],
        vec![259],
        vec![],
        vec![258, 144],
        vec![258, 157],
        vec![260, 168],
    ];
    check(got == want, format!("{got:?}"))
}

/// The spell book's routines, the player at 0 FP.
fn book_routines() -> Result<String, String> {
    let mut world = world(false)?;
    let first = idle(&mut world);
    let me = player(&first).ok_or("no player")?;
    fight(&mut world, me, |fighter| fighter.fp = 0);
    let after = module(&mut world, "probe_book");
    let seen = tokens(&after, 1..=8);
    let granted = sheet_of(&after, me).is_some_and(|sheet| sheet.powers.has(GRANTED));
    check(
        seen == ["20", "0", "1", "0", "0", "1", "64", "0"] && granted,
        format!("{seen:?}, granted {granted}"),
    )
}

/// The Forms' routines, and the HUD's pick (`Action::Form`): a known Form
/// in use and its line told; one not known refused.
fn form_choice() -> Result<String, String> {
    let mut world = world(false)?;
    let first = idle(&mut world);
    let me = player(&first).ok_or("no player")?;
    let after = module(&mut world, "probe_forms");
    let seen = tokens(&after, 10..=13);
    let mut input = TickInput::default();
    input.insert(Seat(0), crate::input::Command::default());
    input.act(Seat(0), crate::action::Action::Form(SHII_CHO));
    let picked = step(&mut world, &input);
    let told = world.take_notices();
    let line = crate::notice::Notice::Line(forms::initiated(SHII_CHO));
    let mut refuse = TickInput::default();
    refuse.insert(Seat(0), crate::input::Command::default());
    refuse.act(Seat(0), crate::action::Action::Form(MAKASHI));
    let refused = step(&mut world, &refuse);
    let form = |snapshot: &Snapshot| sheet_of(snapshot, me).map(|sheet| sheet.form);
    check(
        seen == ["0", "1", "0", "1"]
            && form(&picked) == Some(SHII_CHO)
            && told.iter().any(|(_, notice)| *notice == line)
            && form(&refused) == Some(SHII_CHO),
        format!(
            "{seen:?}, picked {:?}, refused {:?}, told {told:?}",
            form(&picked),
            form(&refused)
        ),
    )
}

/// Bonus Force points: 25 added (the most +25, the current points not),
/// told as raised; set to 10, told as lowered by the new value.
fn bonus_points() -> Result<String, String> {
    let mut world = world(false)?;
    let first = idle(&mut world);
    let me = player(&first).ok_or("no player")?;
    let most = sheet_of(&first, me).map_or(0, |sheet| i32::from(sheet.force_points()));
    let fp = fighter_of(&first, me).map_or(0, |fighter| fighter.fp);
    let added = module(&mut world, "probe_bonus_add");
    let fp_after = fighter_of(&added, me).map_or(0, |fighter| fighter.fp);
    let read = module(&mut world, "probe_bonus_read");
    let seen = [tokens(&added, 20..=20), tokens(&read, 21..=22)].concat();
    let statuses: Vec<StatusKind> = [&added, &read]
        .iter()
        .flat_map(|snapshot| snapshot.progress.statuses.iter().map(|s| s.kind.clone()))
        .filter(|kind| matches!(kind, StatusKind::MaxForcePoints { .. }))
        .collect();
    let want = [
        StatusKind::MaxForcePoints {
            gained: true,
            by: 25,
        },
        StatusKind::MaxForcePoints {
            gained: false,
            by: 10,
        },
    ];
    let first_told = statuses.first() == Some(&want[0]);
    let lowered = statuses.contains(&want[1]);
    check(
        seen == ["25".to_string(), (most + 25).to_string(), "10".to_string()]
            && fp_after == fp
            && first_told
            && lowered,
        format!("{seen:?} (most {most}), fp {fp} then {fp_after}, {statuses:?}"),
    )
}

/// The stat adjusters: Constitution +2 at level 6 raises the most hit
/// points and the hit points by 6, Demolitions +3, base saves -1, +1, +2
/// (Fortitude's back to where it was with Constitution's modifier up);
/// then the ranks (Awareness 0 + WIS +3; the base
/// alone; Demolitions 7 + INT +2; Stealth of no ranks 0; a door's -1) and
/// the racial subtype (no object 0, the door -1, the player 0).
fn adjusters() -> Result<String, String> {
    let mut world = world(false)?;
    let first = idle(&mut world);
    let me = player(&first).ok_or("no player")?;
    let before = sheet_of(&first, me).ok_or("no sheet")?;
    let adjusted = module(&mut world, "probe_stats");
    let after = sheet_of(&adjusted, me).ok_or("no sheet after")?;
    let hp = fighter_of(&adjusted, me).map_or(0, |fighter| fighter.hp);
    let ranks = module(&mut world, "probe_ranks");
    let seen = tokens(&ranks, 30..=38);
    let saves: Vec<i32> = (0..3).map(|at| after.save(at) - before.save(at)).collect();
    check(
        after.abilities[Ability::Constitution as usize] == 14
            && after.max_hp == before.max_hp + 6
            && hp == i32::from(after.max_hp) * 100
            && after.skills[skills::DEMOLITIONS] == 7
            && saves == [0, 1, 2]
            && seen == ["3", "0", "9", "7", "0", "-1", "0", "-1", "0"],
        format!(
            "CON {}, HP {} → {} ({hp}), Demolitions {}, saves moved {saves:?}, {seen:?}",
            after.abilities[Ability::Constitution as usize],
            before.max_hp,
            after.max_hp,
            after.skills[skills::DEMOLITIONS]
        ),
    )
}

/// The NPC's queue and gait: empty, then a wait queued; standing still;
/// never in total defence.
fn queue_and_gait() -> Result<String, String> {
    let mut world = world(false)?;
    let first = idle(&mut world);
    let jedi = npc(&first).ok_or("no NPC")?;
    let after = run(&mut world, "probe_queue", ObjectId(jedi.0));
    let seen = tokens(&after, 40..=43);
    check(seen == ["0", "1", "0", "0"], format!("{seen:?}"))
}

/// Fake combat: held past 8 s (its weapon out, in combat but not in real
/// combat), let go at once; a flourish fakes combat for 8 s, once a second
/// at most, its blade lighting.
fn fake_combat() -> Result<String, String> {
    let mut world = world(false)?;
    let first = idle(&mut world);
    let jedi = npc(&first).ok_or("no NPC")?;
    module(&mut world, "probe_fake_on");
    let held = wait(&mut world, 9000);
    let asked = module(&mut world, "probe_fake_ask");
    let seen = tokens(&asked, 50..=51);
    let now = asked.time_ms;
    let out = fighter_of(&asked, jedi).is_some_and(|fighter| !fighter.weapon_away(now));
    let faking = fighter_of(&held, jedi).is_some_and(|fighter| fighter.faking(held.time_ms));
    let off = module(&mut world, "probe_fake_off");
    let let_go = fighter_of(&off, jedi).is_some_and(|fighter| !fighter.faking(off.time_ms));
    let flourished = module(&mut world, "probe_flourish");
    let again = module(&mut world, "probe_flourish");
    let lit = fighter_of(&again, jedi).map(|fighter| (fighter.saber.on, fighter.fake_until));
    let kept = fighter_of(&again, jedi).map(|fighter| fighter.fake_until);
    let lapsed = wait(&mut world, 8500);
    let gone = fighter_of(&lapsed, jedi).is_some_and(|fighter| !fighter.faking(lapsed.time_ms));
    let until = flourished.time_ms + crate::combat::COMBAT_MS;
    check(
        faking
            && out
            && seen == ["1", "0"]
            && let_go
            && lit == Some((true, until))
            && kept == Some(until)
            && gone,
        format!(
            "held {faking}, out {out}, {seen:?}, let go {let_go}, flourish {lit:?} then \
             {kept:?} (to {until}), lapsed {gone}"
        ),
    )
}

/// `ForceHeartbeat`: the heartbeat runs at once, and the next a whole
/// period later.
fn forced_heartbeat() -> Result<String, String> {
    let mut world = world(false)?;
    let first = idle(&mut world);
    let count = |snapshot: &Snapshot| {
        snapshot
            .talk
            .tokens
            .get(&60)
            .and_then(|text| text.parse::<i32>().ok())
            .unwrap_or(0)
    };
    let before = count(&first);
    let forced = module(&mut world, "probe_force_beat");
    // Its own run is next step's, as an event's.
    let ran = idle(&mut world);
    let short = wait(&mut world, 5800);
    let whole = wait(&mut world, 400);
    let got = [
        count(&forced).max(count(&ran)),
        count(&short),
        count(&whole),
    ];
    check(
        got == [before + 1, before + 1, before + 2],
        format!("from {before}: {got:?}"),
    )
}

/// Mines a script spawns: hostile without a creator, its DCs the row's
/// plus those given, found by no one; the player's of its faction with its
/// Demolitions then (4 + INT 2), which `GetOwnerDemolitionsSkill` reads
/// and a door mine's blast reads though the player lost its ranks since;
/// `DetonateMine` takes the other away.
fn spawned_mines() -> Result<String, String> {
    let mut world = world(false)?;
    let first = idle(&mut world);
    let me = player(&first).ok_or("no player")?;
    let made = module(&mut world, "probe_mines");
    let kept: Vec<(u32, crate::script::TrapState)> = world
        .script_state()
        .traps
        .iter()
        .map(|(id, trap)| (*id, trap.clone()))
        .collect();
    let hostile = kept.iter().find(|(_, trap)| {
        trap.creator.is_none()
            && trap.faction == Some(factions::HOSTILE_1)
            && trap.laid
                == Some(crate::traps::Laid {
                    detect_dc: 17,
                    disarm_dc: 28,
                })
            && trap.detected_by.is_empty()
            && trap.owner_demolitions == 0
    });
    let own = kept.iter().find(|(_, trap)| {
        trap.creator == Some(me.0)
            && trap.faction == Some(factions::PLAYER)
            && trap.owner_demolitions == 6
    });
    let (Some(&(hostile, _)), Some(&(own, _))) = (hostile, own) else {
        return Err(format!("the mines kept: {kept:?}"));
    };
    let asked = run(&mut world, "probe_mine_owner", ObjectId(own));
    let owner = tokens(&asked, 70..=70);
    let triggers = |snapshot: &Snapshot| {
        snapshot
            .entities
            .iter()
            .filter(|entity| matches!(entity.kind, Kind::Object(ObjectKind::Trigger, _)))
            .count()
    };
    run(&mut world, "probe_detonate", ObjectId(hostile));
    let set_off = idle(&mut world);
    // The player's mine set on the gate, the player's Demolitions gone
    // before it blasts.
    let gate = set_off
        .entities
        .iter()
        .find(|entity| matches!(entity.kind, Kind::Object(ObjectKind::Door, _)))
        .map(|entity| entity.id.0)
        .ok_or("no gate")?;
    let mut state = world.script_state().clone();
    state.door_mines.insert(
        gate,
        crate::script::DoorMine {
            setter: me.0,
            mine: own,
            strength: 3,
            blast_at: set_off.time_ms,
            open_at: None,
        },
    );
    world.set_script_state(state);
    reshape(&mut world, me, |sheet| {
        sheet.skills[skills::DEMOLITIONS] = 0
    });
    idle(&mut world);
    let blast = world
        .take_notices()
        .into_iter()
        .find_map(|(_, notice)| match notice {
            crate::notice::Notice::Blast { demolitions, .. } => Some(demolitions),
            _ => None,
        });
    check(
        kept.len() == 2
            && owner == ["6"]
            && triggers(&set_off) + 1 == triggers(&made)
            && blast == Some(6),
        format!(
            "{} mines, owner {owner:?}, triggers {} then {}, blast {blast:?}",
            kept.len(),
            triggers(&made),
            triggers(&set_off)
        ),
    )
}

/// `HasLineOfSight` traces the places as given: clear across the floor,
/// blocked along the foot of the ramp's high end (an eye's height would
/// clear it) and by the wall; a heal target is a creature's only.
fn sight_and_heal_targets() -> Result<String, String> {
    let mut world = world(false)?;
    idle(&mut world);
    let sight = module(&mut world, "probe_sight");
    let healed = module(&mut world, "probe_heal");
    let seen = [tokens(&sight, 80..=82), tokens(&healed, 83..=84)].concat();
    check(seen == ["1", "0", "0", "0", "1"], format!("{seen:?}"))
}

/// `GetRandomDestination`'s pick (its executable's): whole metres off
/// along each axis within the range on a free floor; on a floor walkable
/// only within 3 m, a shorter line or a turned one within it; walkable
/// nowhere, where it stands; a range of 0, where it stands.
fn random_destinations() -> Result<String, String> {
    use crate::script::random_destination;
    let from = [10.0, 20.0, 0.0];
    let mut seed = 7u32;
    let mut below = |n: u32| {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        (seed >> 16) % n
    };
    let floor = |_: [f32; 3]| Some(0.0);
    let anywhere = |_: [f32; 3], _: [f32; 3]| true;
    let near = |a: [f32; 3], b: [f32; 3]| crate::math::length(crate::math::sub(a, b)) <= 3.0;
    let nowhere = |_: [f32; 3], _: [f32; 3]| false;
    let free: Vec<[f32; 3]> = (0..20)
        .map(|_| random_destination(from, 5, &mut below, (&floor, &anywhere)))
        .collect();
    let whole = free.iter().all(|at| {
        let (dx, dy) = (at[0] - from[0], at[1] - from[1]);
        dx.fract() == 0.0
            && dy.fract() == 0.0
            && (-5.0..5.0).contains(&dx)
            && (-5.0..5.0).contains(&dy)
    });
    let kept: Vec<[f32; 3]> = (0..20)
        .map(|_| random_destination(from, 9, &mut below, (&floor, &near)))
        .collect();
    let within = kept.iter().all(|at| near(from, *at));
    let stays = [
        random_destination(from, 9, &mut below, (&floor, &nowhere)),
        random_destination(from, 0, &mut below, (&floor, &anywhere)),
    ];
    check(
        whole && within && stays == [from; 2],
        format!("free {whole}, within 3 m {within}, {stays:?}; {free:?}"),
    )
}

/// In TSL the party's Force users regenerate by its rule (a pool of 60 FP
/// out of a fight: 2 FP in 3 s) and its other creatures not at all.
fn party_regenerates() -> Result<String, String> {
    let mut world = world(true)?;
    let first = idle(&mut world);
    let (me, jedi) = (
        player(&first).ok_or("no player")?,
        npc(&first).ok_or("no NPC")?,
    );
    for who in [me, jedi] {
        fight(&mut world, who, |fighter| fighter.fp = 1000);
    }
    let after = wait(&mut world, 3000);
    let fp = |who: SimId| fighter_of(&after, who).map_or(0, |fighter| fighter.fp);
    let pool = sheet_of(&after, me).map_or(0, |sheet| i32::from(sheet.force_points()));
    // 60 / 90 a second for 3 s: 200 hundredths (a step's rounding aside).
    let want = pool * 100 * 3 / 90;
    check(
        (fp(me) - 1000 - want).abs() <= 2 && fp(jedi) == 1000,
        format!(
            "player {} (pool {pool}, +{want} due), NPC {}",
            fp(me),
            fp(jedi)
        ),
    )
}

pub fn probes() -> Checks {
    vec![
        ("form_numbers", form_numbers()),
        ("power_costs", costs()),
        ("room_ratings", rooms()),
        ("fp_regeneration", regeneration()),
        ("granted_powers", granted()),
        ("spell_book", book_routines()),
        ("form_choice", form_choice()),
        ("bonus_force_points", bonus_points()),
        ("stat_adjusters", adjusters()),
        ("queue_and_gait", queue_and_gait()),
        ("fake_combat", fake_combat()),
        ("force_heartbeat", forced_heartbeat()),
        ("spawned_mines", spawned_mines()),
        ("party_regenerates", party_regenerates()),
        ("sight_and_heal_targets", sight_and_heal_targets()),
        ("random_destination", random_destinations()),
        ("forfeit_on_power", forfeit_on_power()),
    ]
}

/// The player knowing only Choke (`KNOWN`, 10 FP) with `fp` (hundredths),
/// under the forfeit conditions `rules`, presses `button` with Choke
/// chosen: the violation the party table keeps after the step.
fn ordered(rules: u32, fp: i32, button: u16) -> Result<u32, String> {
    let mut world = world(false)?;
    let me = player(&idle(&mut world)).ok_or("no player")?;
    reshape(&mut world, me, |sheet| {
        sheet.powers = crate::combat::sheet::Rows::default().with(KNOWN);
    });
    fight(&mut world, me, |fighter| fighter.fp = fp);
    let mut state = world.script_state().clone();
    state.party.forfeit = rules;
    world.set_script_state(state);
    let mut input = TickInput::default();
    input.insert(
        Seat(0),
        crate::input::Command {
            buttons: button,
            force: KNOWN as u8,
            ..crate::input::Command::default()
        },
    );
    let tick = world.tick() + 1;
    crate::step(&mut world, tick, &input, MSEC, StepReason::Authority);
    Ok(world.script_state().party.violation)
}

/// A player's order of a power it knows and can pay for breaks
/// `FORFEIT_NO_FORCE_POWERS` (1), by the power button or the grip's own;
/// one it cannot pay for, and other rules, keep none.
fn forfeit_on_power() -> Result<String, String> {
    use crate::input::buttons::{FORCE_GRIP, FORCE_POWER};
    use crate::script::party::forfeit::{FORCE_POWERS, ITEMS};
    let cases = [
        (FORCE_POWERS, 6000, FORCE_POWER, FORCE_POWERS),
        (FORCE_POWERS, 6000, FORCE_GRIP, FORCE_POWERS),
        (FORCE_POWERS, 0, FORCE_POWER, 0),
        (ITEMS, 6000, FORCE_POWER, 0),
    ];
    let mut seen = Vec::new();
    for (rules, fp, button, _) in cases {
        seen.push(ordered(rules, fp, button)?);
    }
    let want: Vec<u32> = cases.iter().map(|case| case.3).collect();
    check(seen == want, format!("violations {seen:?}, want {want:?}"))
}
