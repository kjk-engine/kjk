//! The probes of the routines override mods call that the games' own
//! scripts rarely do (`kjk sim probes "mod routines"`), on the proving
//! ground, no install:
//! - the maths routines (`script::maths`) give the bits both executables'
//!   handler gives (its degrees, its `f32` steps, its zeros out of a
//!   function's domain), the expected bits worked from the same formula on
//!   macOS's C library, which the Mac executables call;
//! - the string routines (`GetStringUpperCase`, `GetStringLowerCase`,
//!   `InsertString`, `StringToFloat`, `IntToHexString`) as `CExoString`
//!   and C's `atof` work them;
//! - a script calling each of them reads what the routine gives (bound,
//!   its arguments in their order);
//! - in a world of three creatures (two of one faction, challenge ratings 2
//!   and 5; one of another): the faction's leader is the higher rated, its
//!   members walk in order, none of them a player's; `ChangeFaction` moves
//!   the third over; `GetAC`, `GetWillSavingThrow` and
//!   `GetChallengeRating` read the sheet and template, -1 armour class for
//!   no object; `GetGoldPieceValue` is at least 1 and 0 for a plot item;
//!   `FloatingTextStringOnCreature` tells the player only when not
//!   broadcast; the effects: haste's speed, Defense and own part (+4 to
//!   `GetAC`), concealment and miss chance one kind, out-of-range percents
//!   and a drain of nothing no effect, a spell immunity making
//!   `ResistForce` 2, a knockdown throwing its bearer down;
//!   `ActionUseFeat` queues an attack feat it has and nothing for one it
//!   lacks; `ActionEquipMostEffectiveArmor` wears the carried armour of the
//!   most armour class (its properties' in) it may wear.

use super::{Fighting, Id, SimWorld};
use crate::StepReason;
use crate::combat::sheet::{Rows, Sheet, feats};
use crate::input::{Seat, TickInput};
use crate::inventory::{Armour, Catalog, ItemFacts, slots};
use crate::module::{ObjectKind, Placement};
use crate::notice::Notice;
use crate::probe::{Checks, check};
use crate::script::Pending;
use crate::script::actions::{Action, Use};
use crate::snapshot::Snapshot;
use fmt_odyssey::ncs::{Const, Ncs, Op};
use script_ncs::probes::Asm;
use script_ncs::vm::ObjectId;

const MSEC: u32 = 8;

/// The routines the probe's scripts call, each after its number.
const CATALOG: &str = r#"
// 0
void SetCustomToken(int nCustomTokenNumber, string sTokenValue);
// 1
string IntToString(int nInteger);
// 2
string FloatToString(float fFloat, int nWidth = 18, int nDecimals = 9);
// 3
float sin(float fValue);
// 4
float acos(float fValue);
// 5
float pow(float fValue, float fExponent);
// 6
float sqrt(float fValue);
// 7
string GetStringUpperCase(string sString);
// 8
string GetStringLowerCase(string sString);
// 9
string InsertString(string sDestination, string sString, int nPosition);
// 10
float StringToFloat(string sNumber);
// 11
string IntToHexString(int nInteger);
"#;

fn call(routine: u16, args: u8) -> Op {
    Op::Action { routine, args }
}

fn int(value: i32) -> Op {
    Op::Const(Const::Int(value))
}

fn float(value: f32) -> Op {
    Op::Const(Const::Float(value))
}

fn text(value: &str) -> Op {
    Op::Const(Const::String(value.as_bytes().to_vec()))
}

/// Pushes `ops` (a routine's arguments, the last first, then its call).
fn push(asm: &mut Asm, ops: &[Op]) {
    for op in ops {
        asm.op(op.clone());
    }
}

/// Custom token `n` set to the string `value` leaves.
fn token(asm: &mut Asm, n: i32, value: &[Op]) {
    push(asm, value);
    asm.op(int(n)).op(call(0, 2));
}

/// Custom token `n` set to the float `value` leaves, to six decimals.
fn float_token(asm: &mut Asm, n: i32, value: &[Op]) {
    asm.op(int(6)).op(int(0));
    push(asm, value);
    asm.op(call(2, 3));
    asm.op(int(n)).op(call(0, 2));
}

/// What each token of `probe_strings_maths` should read.
const SCRIPTED: [(&str, &str); 10] = [
    ("sin(30)", "0.500000"),
    ("acos(0.5)", "60.000000"),
    ("pow(2, 10)", "1024.000000"),
    ("sqrt(-4) is 0", "0.000000"),
    ("upper case", "BASTILA SHAN"),
    ("lower case", "darth revan"),
    ("insert at the start", "Darth Malak"),
    ("insert at the end does nothing", "Malak"),
    ("atof's prefix", "-25.000000"),
    ("hex", "0xffffffd6"),
];

/// `probe_strings_maths`: tokens 1 to 10 as `SCRIPTED` lists them.
fn strings_maths() -> Ncs {
    let mut asm = Asm::default();
    float_token(&mut asm, 1, &[float(30.0), call(3, 1)]);
    float_token(&mut asm, 2, &[float(0.5), call(4, 1)]);
    float_token(&mut asm, 3, &[float(10.0), float(2.0), call(5, 2)]);
    float_token(&mut asm, 4, &[float(-4.0), call(6, 1)]);
    token(&mut asm, 5, &[text("Bastila Shan"), call(7, 1)]);
    token(&mut asm, 6, &[text("Darth REVAN"), call(8, 1)]);
    token(
        &mut asm,
        7,
        &[int(0), text("Darth "), text("Malak"), call(9, 3)],
    );
    token(&mut asm, 8, &[int(5), text("!"), text("Malak"), call(9, 3)]);
    float_token(&mut asm, 9, &[text(" \t-2.5e1xyz"), call(10, 1)]);
    token(&mut asm, 10, &[int(-42), call(11, 1)]);
    asm.op(Op::Retn);
    asm.build()
}

fn library() -> crate::script::Library {
    crate::probe::library(CATALOG, [("probe_strings_maths", strings_maths())])
}

fn step(world: &mut SimWorld) -> Snapshot {
    let mut input = TickInput::default();
    input.insert(Seat(0), crate::input::Command::default());
    let tick = world.tick() + 1;
    crate::step(world, tick, &input, MSEC, StepReason::Authority)
}

/// `script` run once by the module on the proving ground: the snapshot
/// after it.
fn ran(script: &str) -> Result<Snapshot, String> {
    let level = crate::determinism::proving_ground().with_scripts(library());
    let mut world =
        SimWorld::new(crate::WorldConfig::one_seat(), level).map_err(|error| error.to_string())?;
    step(&mut world);
    let mut state = world.script_state().clone();
    state
        .pending
        .push(Pending::new(script, crate::script::MODULE));
    world.set_script_state(state);
    Ok(step(&mut world))
}

/// The maths routines' cases: routine, its argument(s), the `f32` bits the
/// executables' handler gives (NaN: any NaN).
const MATHS: [(&str, f32, f32, u32); 27] = [
    ("cos", 0.0, 0.0, 0x3f80_0000),
    // 90° in the f32's radians falls short of π/2.
    ("cos", 90.0, 0.0, 0x3251_0b46),
    ("cos", 60.0, 0.0, 0x3f00_0000),
    ("sin", 30.0, 0.0, 0x3f00_0000),
    ("sin", 90.0, 0.0, 0x3f80_0000),
    ("sin", -45.0, 0.0, 0xbf35_04f3),
    // A large angle, reduced exactly.
    ("sin", 1e9, 0.0, 0xbf7f_cd50),
    ("tan", 45.0, 0.0, 0x3f80_0000),
    ("tan", 90.0, 0.0, 0x4c9c_c06d),
    ("acos", 0.5, 0.0, 0x4270_0000),
    ("acos", 0.1, 0.0, 0x42a8_858a),
    ("acos", 1.5, 0.0, 0),
    ("asin", 1.0, 0.0, 0x42b4_0000),
    ("asin", -0.5, 0.0, 0xc1f0_0000),
    ("atan", 1.0, 0.0, 0x4234_0000),
    ("atan", -1e30, 0.0, 0xc2b4_0000),
    ("log", 10.0, 0.0, 0x4013_5d8e),
    ("log", 0.0, 0.0, 0),
    ("log", -1.0, 0.0, 0),
    ("pow", 2.0, 10.0, 0x4480_0000),
    ("pow", 2.0, 0.5, 0x3fb5_04f3),
    ("pow", 0.0, -1.0, 0),
    ("pow", -8.0, 1.0 / 3.0, f32::NAN.to_bits()),
    // 2488³ lies halfway between two f32s: the exact cube rounds to even.
    ("pow", 2488.0, 3.0, 0x5065_7e74),
    ("pow", -3.0, 3.0, 0xc1d8_0000),
    ("sqrt", 2.0, 0.0, 0x3fb5_04f3),
    ("sqrt", -4.0, 0.0, 0),
];

/// The string helpers' cases: `InsertString`'s and `StringToFloat`'s.
fn string_checks() -> Checks {
    use crate::script::mod_routines::{inserted, leading_float};
    let inserts = [
        ("Malak", "Darth ", 0, "Darth Malak"),
        ("Malak", "!", 4, "Mala!k"),
        ("Malak", "!", 5, "Malak"),
        ("Malak", "!", -1, "Malak"),
        ("", "x", 0, ""),
        ("Malak", "", 2, "Malak"),
    ];
    let floats = [
        ("1.5", 1.5),
        ("  +7.", 7.0),
        (".25e2m", 25.0),
        ("1e", 1.0),
        ("e5", 0.0),
        ("-.", 0.0),
        ("INFINITY", f32::INFINITY),
        ("0.1", 0.1),
        ("1e40", f32::INFINITY),
    ];
    let mut checks: Checks = Vec::new();
    let bad: Vec<String> = inserts
        .iter()
        .filter(|(d, s, at, want)| inserted(d, s, *at) != *want)
        .map(|(d, s, at, want)| {
            format!("{d:?}+{s:?}@{at}: {:?}, want {want:?}", inserted(d, s, *at))
        })
        .collect();
    checks.push((
        "insert_string",
        check(bad.is_empty(), format!("{} cases; {bad:?}", inserts.len())),
    ));
    let bad: Vec<String> = floats
        .iter()
        .filter(|(text, want)| leading_float(text).to_bits() != want.to_bits())
        .map(|(text, want)| format!("{text:?}: {}, want {want}", leading_float(text)))
        .collect();
    checks.push((
        "string_to_float",
        check(bad.is_empty(), format!("{} cases; {bad:?}", floats.len())),
    ));
    checks
}

pub fn probes() -> Checks {
    let mut checks = Vec::new();
    let bad: Vec<String> = MATHS
        .iter()
        .filter_map(|&(name, x, y, want)| {
            let got = crate::script::mod_routines::maths(name, x, y)?;
            let same = got.to_bits() == want || (got.is_nan() && f32::from_bits(want).is_nan());
            (!same).then(|| {
                format!(
                    "{name}({x}, {y}) = {got:e} ({:#x}), want {want:#x}",
                    got.to_bits()
                )
            })
        })
        .collect();
    checks.push((
        "maths_routines",
        check(bad.is_empty(), format!("{} cases; {bad:?}", MATHS.len())),
    ));
    checks.extend(string_checks());
    checks.extend(world_checks());
    checks.push((
        "strings_and_maths_scripted",
        ran("probe_strings_maths").and_then(|snapshot| {
            let read: Vec<(&str, String, &str)> = SCRIPTED
                .iter()
                .zip(1..)
                .map(|(&(what, want), n)| {
                    let got = snapshot.talk.tokens.get(&n).cloned();
                    (what, got.unwrap_or_else(|| "(unset)".into()), want)
                })
                .collect();
            let bad: Vec<_> = read.iter().filter(|(_, got, want)| got != want).collect();
            check(
                bad.is_empty(),
                format!("{} tokens; wrong: {bad:?}", read.len()),
            )
        }),
    ));
    checks
}

/// The world probe's creatures: tag, faction, challenge rating.
const BRUTE: (&str, u8, f32) = ("probe_brute", 3, 2.0);
const BOSS: (&str, u8, f32) = ("probe_boss", 3, 5.0);
const OTHER: (&str, u8, f32) = ("probe_other", 4, 1.0);

/// The world probe's routines, each after its number.
const WORLD_CATALOG: &str = r#"
// 0
void SetCustomToken(int nCustomTokenNumber, string sTokenValue);
// 1
string IntToString(int nInteger);
// 2
string FloatToString(float fFloat, int nWidth = 18, int nDecimals = 9);
// 3
object GetObjectByTag(string sTag, int nNth = 0);
// 4
string GetTag(object oObject);
// 5
object GetFactionLeader(object oMemberOfFaction);
// 6
object GetFirstFactionMember(object oMemberOfFaction, int bPCOnly = TRUE);
// 7
object GetNextFactionMember(object oMemberOfFaction, int bPCOnly = TRUE);
// 8
int GetIsObjectValid(object oObject);
// 9
int GetAC(object oObject, int nForFutureUse = 0);
// 10
int GetWillSavingThrow(object oTarget);
// 11
float GetChallengeRating(object oCreature);
// 12
object CreateItemOnObject(string sItemTemplate, object oTarget = OBJECT_SELF, int nStackSize = 1);
// 13
int GetGoldPieceValue(object oItem);
// 14
void ChangeFaction(object oObjectToChangeFaction, object oMemberOfFactionToJoin);
// 15
int GetStandardFaction(object oObject);
// 16
void ApplyEffectToObject(int nDurationType, effect eEffect, object oTarget, float fDuration = 0.0f);
// 17
effect EffectHaste();
// 18
effect EffectConcealment(int nPercentage);
// 19
effect EffectMissChance(int nPercentage);
// 20
int GetIsEffectValid(effect eEffect);
// 21
int GetEffectType(effect eEffect);
// 22
effect EffectForceDrain(int nDamage);
// 23
effect EffectSpellImmunity(int nImmunityToSpell = -1);
// 24
int ResistForce(object oSource, object oTarget);
// 25
effect EffectKnockdown();
// 26
void FloatingTextStringOnCreature(string sStringToDisplay, object oCreatureToFloatAbove, int bBroadcastToFaction = TRUE);
// 27
object GetFirstPC();
// 28
void ActionUseFeat(int nFeat, object oTarget);
// 29
void ActionEquipMostEffectiveArmor();
// 30
effect GetFirstEffect(object oCreature);
// 31
effect GetNextEffect(object oCreature);
int EFFECT_TYPE_HASTE = 36;
int EFFECT_TYPE_CONCEALMENT = 72;
int EFFECT_TYPE_SPELL_IMMUNITY = 73;
"#;

/// The object of tag `tag`.
fn tagged(tag: &str) -> Vec<Op> {
    vec![int(0), text(tag), call(3, 2)]
}

/// An int routine's value as a string.
fn number(value: Vec<Op>) -> Vec<Op> {
    [value, vec![call(1, 1)]].concat()
}

/// `effect` applied to `on` for `seconds` (`DURATION_TYPE_TEMPORARY`, or
/// `DURATION_TYPE_PERMANENT` when `None`).
fn apply(asm: &mut Asm, effect: &[Op], on: &[Op], seconds: Option<f32>) {
    asm.op(float(seconds.unwrap_or(0.0)));
    push(asm, on);
    push(asm, effect);
    asm.op(int(if seconds.is_some() { 1 } else { 2 }))
        .op(call(16, 4));
}

/// What the world probe's first script's tokens should read (the armour
/// class and save after the sheets', `armour`, filled in).
fn first_tokens(defence: i32, will: i32) -> Vec<(i32, &'static str, String)> {
    [
        (11, "faction leader: the higher rated", BOSS.0.to_string()),
        (12, "first non-player member", BRUTE.0.to_string()),
        (13, "next member", BOSS.0.to_string()),
        (14, "no third member", "0".to_string()),
        (15, "no player in the faction", "0".to_string()),
        (16, "armour class", defence.to_string()),
        (17, "will save", will.to_string()),
        (18, "challenge rating", "5.0".to_string()),
        (19, "armour class of nothing", "-1".to_string()),
        (20, "a cost of 0 is worth 1", "1".to_string()),
        (21, "a cost of 250", "250".to_string()),
        (22, "a plot item is worth 0", "0".to_string()),
        (23, "concealment past 100 is none", "0".to_string()),
        (24, "concealment of 30", "1".to_string()),
        (25, "a drain of nothing is none", "0".to_string()),
    ]
    .into_iter()
    .collect()
}

/// `probe_mod_world`, run by the module: tokens 11 to 25, then the effects
/// on the boss (haste, a miss chance, an immunity to every power) and the
/// brute (a knockdown), the third creature to the brute's faction, and two
/// floating lines to the player (one broadcast).
fn mod_world() -> Ncs {
    let mut asm = Asm::default();
    let boss = tagged(BOSS.0);
    let brute = tagged(BRUTE.0);
    let tag_of = |object: Vec<Op>| [object, vec![call(4, 1)]].concat();
    let valid = |object: Vec<Op>| number([object, vec![call(8, 1)]].concat());
    let member =
        |routine: u16, pcs: i32| [vec![int(pcs)], brute.clone(), vec![call(routine, 2)]].concat();
    token(
        &mut asm,
        11,
        &tag_of([brute.clone(), vec![call(5, 1)]].concat()),
    );
    token(&mut asm, 12, &tag_of(member(6, 0)));
    token(&mut asm, 13, &tag_of(member(7, 0)));
    token(&mut asm, 14, &valid(member(7, 0)));
    token(&mut asm, 15, &valid(member(6, 1)));
    token(
        &mut asm,
        16,
        &number([vec![int(0)], boss.clone(), vec![call(9, 2)]].concat()),
    );
    token(
        &mut asm,
        17,
        &number([boss.clone(), vec![call(10, 1)]].concat()),
    );
    let rating = [
        vec![int(1), int(0)],
        boss.clone(),
        vec![call(11, 1), call(2, 3)],
    ]
    .concat();
    token(&mut asm, 18, &rating);
    token(
        &mut asm,
        19,
        &number(vec![int(0), Op::Const(Const::Object(-1)), call(9, 2)]),
    );
    let worth = |template: &str| {
        number(
            [
                vec![int(1)],
                boss.clone(),
                vec![text(template), call(12, 3), call(13, 1)],
            ]
            .concat(),
        )
    };
    token(&mut asm, 20, &worth("probe_cheap"));
    token(&mut asm, 21, &worth("probe_gem"));
    token(&mut asm, 22, &worth("probe_relic"));
    token(
        &mut asm,
        23,
        &number(vec![int(150), call(18, 1), call(20, 1)]),
    );
    token(
        &mut asm,
        24,
        &number(vec![int(30), call(18, 1), call(20, 1)]),
    );
    token(
        &mut asm,
        25,
        &number(vec![int(0), call(22, 1), call(20, 1)]),
    );
    apply(&mut asm, &[call(17, 0)], &boss, Some(60.0));
    apply(&mut asm, &[int(30), call(19, 1)], &boss, Some(60.0));
    apply(&mut asm, &[int(-1), call(23, 1)], &boss, None);
    apply(&mut asm, &[call(25, 0)], &brute, Some(3.0));
    push(&mut asm, &brute);
    push(&mut asm, &tagged(OTHER.0));
    asm.op(call(14, 2));
    for (line, broadcast) in [("probe line", 0), ("probe broadcast", 1)] {
        asm.op(int(broadcast))
            .op(call(27, 0))
            .op(text(line))
            .op(call(26, 3));
    }
    asm.op(Op::Retn);
    asm.build()
}

/// `probe_mod_world_after`: token 30 the third creature's faction, 31 the
/// boss's armour class (haste's +4 in), 32 `ResistForce` on the boss, 33 to
/// 37 the types of its five effects.
fn mod_world_after() -> Ncs {
    let mut asm = Asm::default();
    let boss = tagged(BOSS.0);
    token(
        &mut asm,
        30,
        &number([tagged(OTHER.0), vec![call(15, 1)]].concat()),
    );
    token(
        &mut asm,
        31,
        &number([vec![int(0)], boss.clone(), vec![call(9, 2)]].concat()),
    );
    token(
        &mut asm,
        32,
        &number([boss.clone(), vec![call(27, 0), call(24, 2)]].concat()),
    );
    for (n, routine) in [(33, 30), (34, 31), (35, 31), (36, 31), (37, 31)] {
        token(
            &mut asm,
            n,
            &number([boss.clone(), vec![call(routine, 1), call(21, 1)]].concat()),
        );
    }
    asm.op(Op::Retn);
    asm.build()
}

/// `probe_mod_boss`, the boss's own: an attack feat it has (Power Attack)
/// and one it lacks (Rapid Shot) used on the player, then its best armour.
fn mod_boss() -> Ncs {
    let mut asm = Asm::default();
    for feat in [feats::POWER_ATTACK[0], feats::RAPID_SHOT[0]] {
        asm.op(call(27, 0)).op(int(i32::from(feat))).op(call(28, 2));
    }
    asm.op(call(29, 0)).op(Op::Retn);
    asm.build()
}

/// Armour of `defence` (and `bonus` from its properties) needing `needs`.
fn armour(template: &str, defence: i8, bonus: i8, needs: &[u16]) -> ItemFacts {
    let mut facts = ItemFacts::plain(template);
    facts.slots = 1 << slots::BODY;
    facts.armour = Some(Armour {
        defence,
        dex_cap: None,
    });
    facts.gear.defence = bonus;
    facts.needs = needs.to_vec();
    facts
}

/// The armour the boss carries, the one `ActionEquipMostEffectiveArmor`
/// should put on, and the items whose worth is asked.
fn items() -> Catalog {
    let worth = |template: &str, cost: u32, plot: bool| ItemFacts {
        cost,
        plot,
        ..ItemFacts::plain(template)
    };
    Catalog::new([
        armour("probe_light", 2, 0, &[]),
        armour("probe_medium", 4, 0, &[]),
        armour("probe_padded", 2, 3, &[]),
        // Better, but the boss lacks its proficiency.
        armour("probe_heavy", 8, 0, &[feats::PROFICIENCIES[2]]),
        worth("probe_cheap", 0, false),
        worth("probe_gem", 250, false),
        worth("probe_relic", 500, true),
    ])
}
const BEST_ARMOUR: &str = "probe_padded";
const CARRIED: [&str; 4] = ["probe_light", "probe_medium", "probe_padded", "probe_heavy"];

/// The boss's sheet: Power Attack and nothing else of note.
fn boss_sheet() -> Sheet {
    Sheet {
        feats: Rows::default().with(feats::POWER_ATTACK[0]),
        ..Sheet::dev_soldier()
    }
}

/// What the world probe saw: the tokens, the notices told seat 0, the
/// boss's queue and worn armour, and whether the brute lies knocked down.
struct World {
    tokens: Snapshot,
    after: Snapshot,
    notices: Vec<Notice>,
    queue: Vec<Action>,
    worn: Option<String>,
    brute_down: bool,
}

fn world() -> Result<World, String> {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    data.repute = crate::module::Repute {
        rows: vec![vec![50; 8]; 8],
    };
    let entry = data.entry.position;
    for (n, (tag, faction, challenge)) in [BRUTE, BOSS, OTHER].into_iter().enumerate() {
        let at = [entry[0] + 2.0 + n as f32, entry[1] + 3.0, entry[2]];
        data.placements
            .push(Placement::Own(crate::module::ObjectData {
                sheet: Some(Sheet {
                    faction,
                    ..if tag == BOSS.0 {
                        boss_sheet()
                    } else {
                        Sheet::dev_soldier()
                    }
                }),
                template: tag.to_string(),
                faction,
                challenge,
                ..crate::module::ObjectData::bare(ObjectKind::Creature, tag, at)
            }));
    }
    let library = crate::probe::library(
        WORLD_CATALOG,
        [
            ("probe_mod_world", mod_world()),
            ("probe_mod_world_after", mod_world_after()),
            ("probe_mod_boss", mod_boss()),
        ],
    )
    .with_items(items());
    let mut world = SimWorld::new(
        crate::WorldConfig::one_seat(),
        ground.with_module(data).with_scripts(library),
    )
    .map_err(|error| error.to_string())?;
    let start = step(&mut world);
    let objects = crate::Instances::in_snapshot(world.level(), &start);
    let id_of = |tag: &str| {
        start
            .entities
            .iter()
            .find(|entity| {
                objects
                    .of_entity(entity)
                    .is_some_and(|data| data.tag == tag)
            })
            .map(|entity| entity.id.0)
            .ok_or(format!("no {tag}"))
    };
    let (boss, brute) = (id_of(BOSS.0)?, id_of(BRUTE.0)?);
    let mut state = world.script_state().clone();
    let catalog = items();
    for template in CARRIED {
        state
            .make_item(&catalog, template, boss, 1, None)
            .ok_or(format!("no {template}"))?;
    }
    state
        .pending
        .push(Pending::new("probe_mod_world", crate::script::MODULE));
    world.set_script_state(state);
    let tokens = step(&mut world);
    let mut notices: Vec<Notice> = Vec::new();
    for _ in 0..2 {
        step(&mut world);
        notices.extend(
            world
                .take_notices()
                .into_iter()
                .filter(|(seat, _)| *seat == Seat(0))
                .map(|(_, notice)| notice),
        );
    }
    let brute_down = world
        .world
        .query::<(&Id, &Fighting)>()
        .iter(&world.world)
        .any(|(id, fighting)| id.0.0 == brute && fighting.0.saber.floored());
    let mut state = world.script_state().clone();
    state
        .pending
        .push(Pending::new("probe_mod_world_after", crate::script::MODULE));
    state
        .pending
        .push(Pending::new("probe_mod_boss", ObjectId(boss)));
    world.set_script_state(state);
    let after = step(&mut world);
    let state = world.script_state();
    let queue = state.actions.get(&boss).map_or_else(Vec::new, |queue| {
        queue.iter().map(|queued| queued.action.clone()).collect()
    });
    let worn = state
        .in_slot(boss, slots::BODY)
        .and_then(|id| state.items.get(&id.0))
        .map(|held| held.template.clone());
    Ok(World {
        tokens,
        after,
        notices,
        queue,
        worn,
        brute_down,
    })
}

/// Token `n` of `snapshot`.
fn read(snapshot: &Snapshot, n: i32) -> String {
    snapshot
        .talk
        .tokens
        .get(&n)
        .cloned()
        .unwrap_or_else(|| "(unset)".into())
}

fn world_checks() -> Checks {
    let ran = match world() {
        Ok(ran) => ran,
        Err(error) => return vec![("mod_routines_world", Err(error))],
    };
    let sheet = boss_sheet();
    let wanted = first_tokens(sheet.defence(), sheet.save(2));
    let bad: Vec<String> = wanted
        .iter()
        .filter(|(n, _, want)| read(&ran.tokens, *n) != *want)
        .map(|(n, what, want)| format!("{what}: {:?}, want {want:?}", read(&ran.tokens, *n)))
        .collect();
    let mut checks: Checks = vec![(
        "factions_sheet_worth_scripted",
        check(
            bad.is_empty(),
            format!("{} tokens; wrong: {bad:?}", wanted.len()),
        ),
    )];
    let faction = read(&ran.after, 30);
    checks.push((
        "change_faction",
        check(
            faction == BRUTE.1.to_string(),
            format!("the third creature's faction {faction}"),
        ),
    ));
    let lines: Vec<&Notice> = ran
        .notices
        .iter()
        .filter(|notice| matches!(notice, Notice::Said(line) if line.starts_with("probe")))
        .collect();
    checks.push((
        "floating_text_not_broadcast",
        check(
            lines == [&Notice::Said("probe line".into())],
            format!("told {lines:?}"),
        ),
    ));
    let hasted = read(&ran.after, 31);
    let want = (sheet.defence() + i32::from(rules::force_effects::haste::DEFENCE)).to_string();
    checks.push((
        "haste_defence",
        check(
            hasted == want,
            format!("armour class {hasted}, want {want}"),
        ),
    ));
    let resist = read(&ran.after, 32);
    checks.push((
        "spell_immunity_resists",
        check(resist == "2", format!("ResistForce {resist}")),
    ));
    // Haste's parts walk as its one kind.
    let mut types: Vec<String> = (33..=37).map(|n| read(&ran.after, n)).collect();
    types.sort();
    types.dedup();
    let want: Vec<String> = [36, 72, 73].map(|t: i32| t.to_string()).to_vec();
    checks.push((
        "effect_types",
        check(types == want, format!("{types:?}, want {want:?}")),
    ));
    checks.push((
        "knockdown",
        check(
            ran.brute_down,
            format!("the brute down: {}", ran.brute_down),
        ),
    ));
    let feats_queued: Vec<&Action> = ran
        .queue
        .iter()
        .filter(|action| matches!(action, Action::Talent { .. }))
        .collect();
    let power_attack = Use::Feat(feats::POWER_ATTACK[0]);
    let one = matches!(feats_queued.as_slice(), [Action::Talent { talent, .. }] if *talent == power_attack);
    checks.push((
        "use_feat_only_its_own",
        check(one, format!("queued {feats_queued:?}")),
    ));
    checks.push((
        "most_effective_armour",
        check(
            ran.worn.as_deref() == Some(BEST_ARMOUR),
            format!("worn {:?}, want {BEST_ARMOUR}", ran.worn),
        ),
    ));
    checks
}
