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
//!   most armour class (its properties' in) it may wear;
//! - TSLRCM's last ones and the actions on items, on a walkable half of the
//!   ground: `ActionForceFollowObject` runs to its object and follows on,
//!   holds near it, and keeps through a save; `ActionPickUpItem` walks to an
//!   item, plays the reach-low animation and takes it, `ActionPutDownItem`
//!   lays it at the feet, and an item another holds is not taken; the
//!   module's `Mod_OnPlrDeath` runs as the player dies, `GetLastPlayerDied`
//!   naming it, and its resurrection keeps the game from being lost;
//!   `DoSinglePlayerAutoSave` and `ShowSwoopUpgradeScreen` tell the player;
//! - the three combat effects: their constructors' clamps, an invisible
//!   creature unseen, damage reduction's highest-amount, power and limit
//!   rule, and a damage shield's blow back with its bonus;
//! - the routines K1's own scripts called that were the last unbound:
//!   what has an inventory, no item activated, no metamagic or cheat code
//!   before any, the button mash check kept, `ChangeItemCost` on every item
//!   of the base item (cut off), a blaster deflection of no effect type, the
//!   music's volume clamped to 1, sonic damage read by type (-1 before it
//!   and for another type, 0 for no single type), a visual at a place, a
//!   party slot's creature set (not past slot 11), a container's item taken
//!   (`INVENTORY_DISTURB_TYPE_REMOVED`) and put back (`ADDED`), and an
//!   in-game page shown and turned running `k_pend_screenchg` (closed, not).
//! - every `SWMG_` routine `script::minigame` matches on is the one its
//!   list counts bound (`script::bound`, `kjk script check`), no more.

use std::collections::BTreeMap;

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
    checks.extend(left_checks());
    checks.extend(k1_checks());
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

/// The leftover routines' probe's creatures: one following from afar, one
/// following from near, the one they follow, and one picking up and
/// putting down an item.
const FAR: &str = "probe_far";
const NEAR: &str = "probe_near";
const LEADER: &str = "probe_leader";
const PICKER: &str = "probe_picker";
/// The item lying on the floor, and the one the leader carries.
const LYING: &str = "probe_datapad";
const HELD: &str = "probe_credits";
/// How far the followers follow, and where each creature stands in the
/// proving ground's frame (metres), all in the half the player arrives in.
const FOLLOW_FAR_M: f32 = 2.0;
const SPOTS: [(&str, [f32; 2]); 4] = [
    (FAR, [-7.0, 3.0]),
    (NEAR, [-2.5, 3.0]),
    (LEADER, [-2.0, 3.0]),
    (PICKER, [-6.0, -3.0]),
];
/// Where the item lies: 3 m from the picker.
const LYING_AT: [f32; 2] = [-3.0, -3.0];
/// The creatures' walk and run (m/s), and how far they see and hear (m).
const WALK_RUN: (f32, f32) = (1.7, 5.4);
const SENSE_M: f32 = 20.0;

/// The leftover routines' probe's routines, each after its number.
const LEFT_CATALOG: &str = r#"
// 0
void SetCustomToken(int nCustomTokenNumber, string sTokenValue);
// 1
string IntToString(int nInteger);
// 2
object GetObjectByTag(string sTag, int nNth = 0);
// 3
object GetFirstPC();
// 4
void ActionForceFollowObject(object oFollow, float fFollowDistance = 0.0f);
// 5
void ActionPickUpItem(object oItem);
// 6
void ActionPutDownItem(object oItem);
// 7
object GetLastPlayerDied();
// 8
void DoSinglePlayerAutoSave();
// 9
void ShowSwoopUpgradeScreen();
// 10
int GetIsObjectValid(object oObject);
// 11
void ApplyEffectToObject(int nDurationType, effect eEffect, object oTarget, float fDuration = 0.0f);
// 12
effect EffectDeath(int nSpectacularDeath = FALSE, int nDisplayFeedback = TRUE, int nNoFadeAway = FALSE);
// 13
effect EffectResurrection(int nHPPercent = 0);
// 14
effect EffectInvisibility(int nInvisibilityType);
// 15
effect EffectDamageReduction(int nAmount, int nDamagePower, int nLimit = 0);
// 16
effect EffectDamageShield(int nDamageAmount, int nRandomAmount, int nDamageType);
// 17
int GetIsEffectValid(effect eEffect);
int INVISIBILITY_TYPE_NORMAL = 1;
int INVISIBILITY_TYPE_DARKNESS = 2;
int INVISIBILITY_TYPE_IMPROVED = 4;
int DAMAGE_POWER_NORMAL = 0;
int DAMAGE_POWER_ENERGY = 6;
int DAMAGE_TYPE_UNIVERSAL = 8;
"#;

/// `probe_follow_far`, `probe_follow_near`: the caller follows the leader.
fn follow(range: f32) -> Ncs {
    let mut asm = Asm::default();
    asm.op(float(range));
    push(&mut asm, &[int(0), text(LEADER), call(2, 2)]);
    asm.op(call(4, 2)).op(Op::Retn);
    asm.build()
}

/// `probe_pick_up`, `probe_put_down`: the caller's action on the item.
fn on_item(routine: u16) -> Ncs {
    let mut asm = Asm::default();
    push(
        &mut asm,
        &[int(0), text(LYING), call(2, 2), call(routine, 1)],
    );
    asm.op(Op::Retn);
    asm.build()
}

/// `probe_lift_held`: the caller tries to pick up the leader's own item.
fn lift_held() -> Ncs {
    let mut asm = Asm::default();
    push(&mut asm, &[int(0), text(HELD), call(2, 2), call(5, 1)]);
    asm.op(Op::Retn);
    asm.build()
}

/// `probe_kill_pc`: the player killed; the module's `probe_plr_death`
/// (`Mod_OnPlrDeath`) sets token 40 to whether `GetLastPlayerDied` names
/// the first PC and raises it; `probe_screens` asks for an autosave and
/// the swoop upgrade screen.
fn kill_pc() -> Ncs {
    let mut asm = Asm::default();
    at_once(
        &mut asm,
        &[int(1), int(0), int(0), call(12, 3)],
        &[call(3, 0)],
    );
    asm.op(Op::Retn);
    asm.build()
}

/// `effect` applied to `on` at once (`DURATION_TYPE_INSTANT`), by
/// `LEFT_CATALOG`'s `ApplyEffectToObject`.
fn at_once(asm: &mut Asm, effect: &[Op], on: &[Op]) {
    asm.op(float(0.0));
    push(asm, on);
    push(asm, effect);
    asm.op(int(0)).op(call(11, 4));
}

fn plr_death() -> Ncs {
    let mut asm = Asm::default();
    let same = [
        call(7, 0),
        call(3, 0),
        Op::Equal(fmt_odyssey::ncs::Pair::ObjectObject),
        call(1, 1),
    ];
    token(&mut asm, 40, &same);
    at_once(&mut asm, &[int(100), call(13, 1)], &[call(7, 0)]);
    asm.op(Op::Retn);
    asm.build()
}

/// `probe_effects`: token 41 whether an invisibility of type 3 is an
/// effect (none), then on the leader a normal invisibility, a damage
/// reduction (5, a power past energy's, a limit below 0) and a damage shield
/// (points past the most, a bonus past 2d6, a type below 0), permanent.
fn effects_script() -> Ncs {
    let mut asm = Asm::default();
    token(
        &mut asm,
        41,
        &[int(3), call(14, 1), call(17, 1), call(1, 1)],
    );
    let leader = [int(0), text(LEADER), call(2, 2)];
    for effect in [
        vec![int(1), call(14, 1)],
        vec![int(-3), int(99), int(5), call(15, 3)],
        vec![int(-1), int(11), int(20_000), call(16, 3)],
    ] {
        asm.op(float(0.0));
        push(&mut asm, &leader);
        push(&mut asm, &effect);
        asm.op(int(2)).op(call(11, 4));
    }
    asm.op(Op::Retn);
    asm.build()
}

fn screens() -> Ncs {
    let mut asm = Asm::default();
    asm.op(call(8, 0)).op(call(9, 0)).op(Op::Retn);
    asm.build()
}

/// The leftover routines' world: the four creatures in a row from the entry
/// (`SPOTS`), the leader carrying `HELD`, `LYING` on the floor at
/// `LYING_AT`, a walk from the picker; the module's `Mod_OnPlrDeath` `probe_plr_death` when
/// `raised`.
fn left_world(raised: bool) -> Result<(SimWorld, BTreeMap<&'static str, u32>), String> {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    data.repute = crate::module::Repute {
        rows: vec![vec![50; 8]; 8],
    };
    let body = crate::module::Body {
        appearance: 0,
        size: 3,
        movement: 4,
        walk_mps: WALK_RUN.0,
        run_mps: WALK_RUN.1,
        head_arc_h: 0.0,
        bag: 0,
        destroy_ms: 0,
        fade_ms: None,
        bulk: None,
    };
    for (tag, [x, y]) in SPOTS {
        data.placements
            .push(Placement::Own(crate::module::ObjectData {
                sheet: Some(Sheet::dev_soldier()),
                template: tag.to_string(),
                body: Some(body),
                senses: Some(crate::module::Senses {
                    sight: SENSE_M,
                    hearing: SENSE_M,
                }),
                ..crate::module::ObjectData::bare(
                    ObjectKind::Creature,
                    tag,
                    crate::determinism::ground_point(x, y, 0.0),
                )
            }));
    }
    if raised {
        data.module_scripts
            .push(("Mod_OnPlrDeath".into(), "probe_plr_death".into()));
    }
    let module = data.name.clone();
    let library = crate::probe::library(
        LEFT_CATALOG,
        [
            ("probe_follow_far", follow(FOLLOW_FAR_M)),
            ("probe_follow_near", follow(FOLLOW_FAR_M)),
            ("probe_follow_none", follow(0.0)),
            ("probe_pick_up", on_item(5)),
            ("probe_put_down", on_item(6)),
            ("probe_lift_held", lift_held()),
            ("probe_kill_pc", kill_pc()),
            ("probe_plr_death", plr_death()),
            ("probe_screens", screens()),
            ("probe_effects", effects_script()),
        ],
    )
    .with_items(Catalog::new([
        ItemFacts::plain(LYING),
        ItemFacts::plain(HELD),
    ]))
    .with_move_rates(crate::script::MoveRates {
        player: Some(WALK_RUN),
        default: Some(WALK_RUN),
    });
    // The navigation mesh over that half (`pace_probes`'s).
    let [a, b, c, d] = [[-7.5, -5.5], [-0.5, -5.5], [-0.5, 5.5], [-7.5, 5.5]]
        .map(|[x, y]| crate::determinism::ground_point(x, y, 0.0));
    let mut nav = collision::nav::NavMesh::build(
        &[[a, b, c], [a, c, d]],
        crate::walk::CLEARANCE * collision::UNIT,
    );
    if let Some(level) = ground.collision().level() {
        nav.mark(level);
    }
    let level = ground.with_module(data).with_scripts(library).with_nav(nav);
    let mut world =
        SimWorld::new(crate::WorldConfig::one_seat(), level).map_err(|error| error.to_string())?;
    let start = step(&mut world);
    let objects = crate::Instances::in_snapshot(world.level(), &start);
    let mut ids = BTreeMap::new();
    for entity in &start.entities {
        if let Some(data) = objects.of_entity(entity)
            && let Some((tag, _)) = SPOTS.iter().find(|(tag, _)| *tag == data.tag)
        {
            ids.insert(*tag, entity.id.0);
        }
    }
    let leader = *ids.get(LEADER).ok_or("no leader")?;
    let catalog = Catalog::new([ItemFacts::plain(LYING), ItemFacts::plain(HELD)]);
    let mut state = world.script_state().clone();
    let lying = crate::determinism::ground_point(LYING_AT[0], LYING_AT[1], 0.0);
    state
        .lay_item(&catalog, LYING, &module, (lying, 0.0))
        .ok_or("no item laid")?;
    state
        .make_item(&catalog, HELD, leader, 1, None)
        .ok_or("no item held")?;
    world.set_script_state(state);
    Ok((world, ids))
}

/// `script` queued for `caller`.
fn run_as(world: &mut SimWorld, script: &str, caller: u32) {
    let mut state = world.script_state().clone();
    state.pending.push(Pending::new(script, ObjectId(caller)));
    world.set_script_state(state);
}

/// `who`'s queue now.
fn queue_of(world: &SimWorld, who: u32) -> Vec<Action> {
    world
        .script_state()
        .actions
        .get(&who)
        .map_or_else(Vec::new, |queue| {
            queue.iter().map(|queued| queued.action.clone()).collect()
        })
}

/// `ActionForceFollowObject`: the far follower queues a run to the leader
/// within its range and the follow again behind it; the near one holds its
/// follow step after step; a distance of 0 follows at the least range;
/// the queues come back from a save as they were.
fn force_follow() -> Result<String, String> {
    use crate::script::actions::Target;
    let (mut world, ids) = left_world(false)?;
    let (far, near, leader) = (ids[FAR], ids[NEAR], ids[LEADER]);
    run_as(&mut world, "probe_follow_far", far);
    run_as(&mut world, "probe_follow_near", near);
    step(&mut world);
    step(&mut world);
    step(&mut world);
    let follow = Action::Follow {
        object: ObjectId(leader),
        range: FOLLOW_FAR_M,
    };
    let (far_queue, near_queue) = (queue_of(&world, far), queue_of(&world, near));
    let ran = matches!(
        far_queue.as_slice(),
        [Action::Move { to: Target::Object(to), run: true, range, .. }, again]
            if *to == ObjectId(leader) && *range == FOLLOW_FAR_M && *again == follow
    );
    let held = near_queue == [follow];
    let state = world.script_state();
    let mut out = crate::wire::Writer::default();
    state.encode(&mut out);
    let kept = crate::script::State::decode(&mut crate::wire::Reader::new("state", &out.0))
        .is_ok_and(|back| back.actions == state.actions);
    run_as(&mut world, "probe_follow_none", near);
    step(&mut world);
    let least = queue_of(&world, near).iter().any(|action| {
        *action
            == Action::Follow {
                object: ObjectId(leader),
                range: rules::scripts::moves::FOLLOW_LEAST,
            }
    });
    check(
        ran && held && kept && least,
        format!(
            "far {far_queue:?}; near {near_queue:?}; kept {kept}; a distance of 0 at the least {least}"
        ),
    )
}

/// `ActionPickUpItem` and `ActionPutDownItem`: the picker walks to the item,
/// plays the reach-low animation, then holds it (the floor without it); put
/// down, it lies at the picker's feet again, the area's; an item the
/// leader holds is not taken.
fn pick_up_put_down() -> Result<String, String> {
    use crate::script::actions::{GET_LOW, PICK_UP_MS};
    let (mut world, ids) = left_world(false)?;
    let (picker, leader) = (ids[PICKER], ids[LEADER]);
    let module = world
        .level()
        .module()
        .map(|m| m.name.clone())
        .ok_or("the ground has no module")?;
    let item_of = |world: &SimWorld, template: &str| {
        let state = world.script_state();
        state
            .items
            .iter()
            .find(|(_, held)| held.template == template)
            .map(|(id, held)| (*id, held.holder))
    };
    let (item, _) = item_of(&world, LYING).ok_or("no item")?;
    let animation = Action::Animate {
        animation: GET_LOW,
        ms: PICK_UP_MS,
    };
    let mut animated = false;
    run_as(&mut world, "probe_pick_up", picker);
    let mut steps = 0;
    let mut began = Vec::new();
    while steps < 2000
        && world
            .script_state()
            .lying(&module, ObjectId(item))
            .is_some()
    {
        step(&mut world);
        animated |= queue_of(&world, picker).first() == Some(&animation);
        steps += 1;
        if steps == 5 {
            began = queue_of(&world, picker);
        }
    }
    let holder = item_of(&world, LYING).map(|(_, holder)| holder);
    let picked = holder == Some(picker);
    let reach = rules::scripts::moves::PICK_UP_REACH;
    let off_floor = !world.script_state().floor.contains_key(&item);
    run_as(&mut world, "probe_put_down", picker);
    let mut dropped_animated = false;
    for _ in 0..400 {
        step(&mut world);
        dropped_animated |= queue_of(&world, picker).first() == Some(&animation);
    }
    let at = world
        .script_state()
        .lying(&module, ObjectId(item))
        .map(|lying| lying.at);
    let feet = world
        .snapshot()
        .get(crate::snapshot::SimId(picker))
        .map(|entity| entity.position);
    let put = at
        .zip(feet)
        .is_some_and(|(at, feet)| crate::script::mod_routines::distance(at, feet) < reach);
    run_as(&mut world, "probe_lift_held", picker);
    for _ in 0..40 {
        step(&mut world);
    }
    let kept = item_of(&world, HELD).map(|(_, holder)| holder) == Some(leader)
        && queue_of(&world, picker).is_empty();
    check(
        picked && off_floor && animated && dropped_animated && put && kept,
        format!(
            "picked {picked} in {steps} steps (animated {animated}; queued {began:?}); put down at {at:?}, \
             feet {feet:?} (animated {dropped_animated}); the leader's kept {kept}"
        ),
    )
}

/// The module's `Mod_OnPlrDeath` runs as the player dies, `GetLastPlayerDied`
/// the player; its resurrection keeps the game from being lost; without
/// it, the game is lost. `DoSinglePlayerAutoSave` and
/// `ShowSwoopUpgradeScreen` tell the player.
fn player_died() -> Result<String, String> {
    let mut outcomes = Vec::new();
    for raised in [true, false] {
        let (mut world, _) = left_world(raised)?;
        run_as(&mut world, "probe_kill_pc", crate::script::MODULE.0);
        let mut last = step(&mut world);
        for _ in 0..20 {
            last = step(&mut world);
        }
        let named = read(&last, 40);
        outcomes.push((raised, named, last.lost.is_some()));
    }
    let (mut world, _) = left_world(false)?;
    run_as(&mut world, "probe_screens", crate::script::MODULE.0);
    step(&mut world);
    let told: Vec<Notice> = world
        .take_notices()
        .into_iter()
        .map(|(_, notice)| notice)
        .collect();
    let screens = told.contains(&Notice::AutoSave)
        && told.contains(&Notice::Show(crate::notice::Screen::SwoopUpgrade));
    check(
        outcomes == [(true, "1".into(), false), (false, "(unset)".into(), true)] && screens,
        format!("(raised, the player named, lost): {outcomes:?}; told {told:?}"),
    )
}

/// The three effects as the constructors clamp them: no invisibility of an
/// unknown type; a reduction's power past energy's none's, its limit at
/// least 0; a shield's points past the most 1, its bonus past 2d6 none, its
/// type below 0 universal. The leader, invisible, is not seen by the near
/// follower, which saw it before.
fn three_effects() -> Result<String, String> {
    use crate::combat::effects::Modifies;
    let (mut world, ids) = left_world(false)?;
    let (near, leader) = (ids[NEAR], ids[LEADER]);
    let seen = |world: &SimWorld| {
        world
            .script_state()
            .perceived
            .get(&near)
            .is_some_and(|perceived| perceived.seen.contains(&leader))
    };
    for _ in 0..300 {
        step(&mut world);
    }
    let before = seen(&world);
    run_as(&mut world, "probe_effects", crate::script::MODULE.0);
    let made = step(&mut world);
    for _ in 0..300 {
        step(&mut world);
    }
    let after = seen(&world);
    let lasting: Vec<(Modifies, i16, u8, u32)> = world
        .world
        .query::<(&Id, &Fighting)>()
        .iter(&world.world)
        .filter(|(id, _)| id.0.0 == leader)
        .flat_map(|(_, fighting)| fighting.0.effects.lasting.iter().flatten().copied())
        .map(|l| (l.modifies, l.amount, l.of, l.third))
        .collect();
    let want = [
        (Modifies::Invisibility, 1, 0, 0),
        (Modifies::DamageReduction, 5, 0, 0),
        (Modifies::DamageShield, 1, 0, 8),
    ];
    let unknown = read(&made, 41);
    check(
        want.iter().all(|w| lasting.contains(w)) && unknown == "0" && before && !after,
        format!(
            "on the leader {lasting:?}; type 3 an effect {unknown}; seen before {before}, \
             invisible {after}"
        ),
    )
}

/// The damage reduction's rule (`harm::reduced`): 10 off a physical hit
/// below power 2, none at power 2 or on another type; a limit of 5 is
/// drained by a 3-point blow (all taken off) to 2, then a 8-point blow
/// spends it, taking 2 off and ending it. A shield of 4 points and a bonus
/// of 3, slashing, strikes back 7 of slashing; 2d6 more lies in 2 to 12.
fn reduction_and_shield() -> Result<String, String> {
    use crate::combat::effects::{Lasting, Modifies};
    use crate::combat::sheet::type_rows::SLASHING;
    let lasting = |modifies, amount, of, third| Lasting {
        modifies,
        amount,
        of,
        until: u64::MAX,
        spell: u16::MAX,
        by: 0,
        id: 7,
        at: 0,
        third,
    };
    let mut fighter = crate::combat::Fighter::fresh(20, 0);
    fighter.effects.lasting[0] = Some(lasting(Modifies::DamageReduction, 10, 2, 0));
    let physical = Some(SLASHING);
    let cases = [
        super::harm::reduced(&mut fighter, 1500, (physical, 0), 1),
        super::harm::reduced(&mut fighter, 1500, (physical, 2), 1),
        super::harm::reduced(&mut fighter, 1500, (Some(SLASHING + 1), 0), 1),
    ];
    fighter.effects.lasting[0] = Some(lasting(Modifies::DamageReduction, 10, 2, 5));
    let first = super::harm::reduced(&mut fighter, 300, (physical, 0), 1);
    let left = fighter.effects.lasting[0].map(|l| l.third);
    let second = super::harm::reduced(&mut fighter, 800, (physical, 0), 1);
    let ended = fighter.effects.reduction(1).is_none();
    let slashing_flag = 1u32 << SLASHING;
    let hit = super::Hit {
        attacker: crate::snapshot::SimId(1),
        target: crate::snapshot::SimId(2),
        source: crate::combat::damage::Source::Death,
        point: None,
        direction: None,
    };
    let mut rng = crate::rng::SimRng::new(1, 2);
    let flat = super::harm::shield_blow(
        &lasting(Modifies::DamageShield, 4, 3, slashing_flag),
        hit,
        &mut rng,
    );
    let rolled: Vec<u16> = (0..50)
        .filter_map(|_| {
            let blow =
                super::harm::shield_blow(&lasting(Modifies::DamageShield, 0, 10, 0), hit, &mut rng);
            match blow.source {
                crate::combat::damage::Source::Script { damage, .. } => Some(damage),
                _ => None,
            }
        })
        .collect();
    let back = matches!(
        flat.source,
        crate::combat::damage::Source::Script { damage: 7, damage_type: Some(row), .. } if row == SLASHING
    ) && flat.target == hit.attacker
        && flat.attacker == hit.target;
    let dice = rolled.iter().all(|d| (2..=12).contains(d)) && rolled.len() == 50;
    check(
        cases == [500, 1500, 1500]
            && first == 0
            && left == Some(2)
            && second == 600
            && ended
            && back
            && dice,
        format!(
            "{cases:?}; limited: {first} then {left:?} left, {second} (ended {ended}); \
             shield back {flat:?}; 2d6 {dice}"
        ),
    )
}

/// The leftover routines' checks (TSLRCM's, the item actions, the three
/// effects).
fn left_checks() -> Checks {
    vec![
        ("force_follow", force_follow()),
        ("pick_up_put_down", pick_up_put_down()),
        ("player_died", player_died()),
        ("invisibility_reduction_shield_made", three_effects()),
        ("reduction_and_shield_rules", reduction_and_shield()),
    ]
}

/// The K1 probe's creature, container and plain placeable.
const TARGET: &str = "probe_k1_target";
const CHEST: &str = "probe_k1_chest";
const SIGN: &str = "probe_k1_sign";
/// Two templates of one base item, and their costs.
const MEDKIT: (&str, u32) = ("probe_k1_medkit", 100);
const BANDAGE: (&str, u32) = ("probe_k1_bandage", 7);
/// The visual `PlayVisualAreaEffect` plays.
const AREA_VISUAL: i32 = 1000;

/// The K1 probe's routines, each after its number.
const K1_CATALOG: &str = r#"
// 0
void SetCustomToken(int nCustomTokenNumber, string sTokenValue);
// 1
string IntToString(int nInteger);
// 2
object GetObjectByTag(string sTag, int nNth = 0);
// 3
int GetHasInventory(object oObject);
// 4
object GetItemActivated();
// 5
object GetItemActivator();
// 6
int GetIsObjectValid(object oObject);
// 7
int GetMetaMagicFeat();
// 8
int GetCheatCode(int nCode);
// 9
int GetSubScreenID();
// 10
void SetButtonMashCheck(int nCheck);
// 11
int GetButtonMashCheck();
// 12
void ChangeItemCost(string sItem, float fCostMultiplier);
// 13
object CreateItemOnObject(string sItemTemplate, object oTarget = OBJECT_SELF, int nStackSize = 1);
// 14
int GetGoldPieceValue(object oItem);
// 15
effect EffectBlasterDeflectionIncrease(int nChange);
// 16
int GetIsEffectValid(effect eEffect);
// 17
int GetEffectType(effect eEffect);
// 18
void SetMusicVolume(float fVolume = 1.0f);
// 19
void ApplyEffectToObject(int nDurationType, effect eEffect, object oTarget, float fDuration = 0.0f);
// 20
effect EffectDamage(int nDamageAmount, int nDamageType = DAMAGE_TYPE_UNIVERSAL, int nDamagePower = DAMAGE_POWER_NORMAL);
// 21
int GetDamageDealtByType(int nDamageType);
// 22
void PlayVisualAreaEffect(int nEffectID, location lTarget);
// 23
location GetLocation(object oObject);
// 24
void SetAvailableNPCId(int nNPC, object oidNPC);
// 25
int GetInventoryDisturbType();
// 26
void AddToParty(object oPC, object oPartyLeader);
// 27
void RemoveFromParty(object oPC);
// 28
object GetFirstPC();
int DURATION_TYPE_INSTANT = 0;
int DURATION_TYPE_PERMANENT = 2;
int EFFECT_TYPE_INVALIDEFFECT = 0;
int DAMAGE_TYPE_UNIVERSAL = 8;
int DAMAGE_TYPE_SONIC = 1024;
int DAMAGE_TYPE_ENERGY = 4096;
int DAMAGE_POWER_NORMAL = 0;
"#;

/// What `probe_k1_world`'s tokens should read.
const K1_TOKENS: [(i32, &str, &str); 15] = [
    (40, "a creature has an inventory", "1"),
    (41, "a container has one", "1"),
    (42, "a plain placeable has none", "0"),
    (43, "nothing has none", "0"),
    (44, "no item activated", "0"),
    (45, "no activator", "0"),
    (46, "no cast's metamagic", "255"),
    (47, "no cheat code", "0"),
    (48, "no in-game page", "0"),
    (49, "the button mash check kept", "5"),
    (50, "a medkit's worth", "100"),
    (51, "its base item's other template, by 2.5 cut off", "17"),
    (52, "the medkit by 2.5", "250"),
    (53, "a blaster deflection is an effect", "1"),
    (54, "of no effect type", "0"),
];

/// The object of tag `tag`, by the K1 catalog's `GetObjectByTag`.
fn k1_tagged(tag: &str) -> Vec<Op> {
    vec![int(0), text(tag), call(2, 2)]
}

/// `probe_k1_world`, run by the module: tokens 40 to 54, the music's volume
/// set past 1, sonic damage on the target, a visual at its place, party slot
/// 2 (and 12, past the table) given the target, and NWN's party calls.
fn k1_world() -> Ncs {
    let mut asm = Asm::default();
    let target = k1_tagged(TARGET);
    let has = |object: Vec<Op>| number([object, vec![call(3, 1)]].concat());
    token(&mut asm, 40, &has(target.clone()));
    token(&mut asm, 41, &has(k1_tagged(CHEST)));
    token(&mut asm, 42, &has(k1_tagged(SIGN)));
    token(&mut asm, 43, &has(vec![Op::Const(Const::Object(-1))]));
    token(&mut asm, 44, &number(vec![call(4, 0), call(6, 1)]));
    token(&mut asm, 45, &number(vec![call(5, 0), call(6, 1)]));
    token(&mut asm, 46, &number(vec![call(7, 0)]));
    token(&mut asm, 47, &number(vec![int(0), call(8, 1)]));
    token(&mut asm, 48, &number(vec![call(9, 0)]));
    asm.op(int(5)).op(call(10, 1));
    token(&mut asm, 49, &number(vec![call(11, 0)]));
    let worth = |template: &str| {
        number(
            [
                vec![int(1)],
                target.clone(),
                vec![text(template), call(13, 3), call(14, 1)],
            ]
            .concat(),
        )
    };
    token(&mut asm, 50, &worth(MEDKIT.0));
    asm.op(float(2.5)).op(text(MEDKIT.0)).op(call(12, 2));
    token(&mut asm, 51, &worth(BANDAGE.0));
    token(&mut asm, 52, &worth(MEDKIT.0));
    token(
        &mut asm,
        53,
        &number(vec![int(5), call(15, 1), call(16, 1)]),
    );
    token(
        &mut asm,
        54,
        &number(vec![int(5), call(15, 1), call(17, 1)]),
    );
    asm.op(float(1.5)).op(call(18, 1));
    asm.op(float(0.0));
    push(&mut asm, &target);
    asm.op(int(0)).op(int(1024)).op(int(10)).op(call(20, 3));
    asm.op(int(0)).op(call(19, 4));
    push(&mut asm, &target);
    asm.op(call(23, 1)).op(int(AREA_VISUAL)).op(call(22, 2));
    for slot in [2, 12] {
        push(&mut asm, &target);
        asm.op(int(slot)).op(call(24, 2));
    }
    asm.op(call(28, 0)).op(call(28, 0)).op(call(26, 2));
    push(&mut asm, &target);
    asm.op(call(27, 1)).op(Op::Retn);
    asm.build()
}

/// A script setting token `n` to `GetDamageDealtByType` of each of `types`
/// in turn (`n`, `n + 1`, …), run by the target.
fn dealt(n: i32, types: &[i32]) -> Ncs {
    let mut asm = Asm::default();
    for (at, kind) in (n..).zip(types) {
        token(&mut asm, at, &number(vec![int(*kind), call(21, 1)]));
    }
    asm.op(Op::Retn);
    asm.build()
}

/// A script setting token `n` to the int routine `routine` (no arguments).
fn reads(n: i32, routine: u16) -> Ncs {
    let mut asm = Asm::default();
    token(&mut asm, n, &number(vec![call(routine, 0)]));
    asm.op(Op::Retn);
    asm.build()
}

/// What the K1 probe saw.
struct K1 {
    tokens: Snapshot,
    before: String,
    dealt: Vec<String>,
    music: Vec<Notice>,
    area_visual: bool,
    slots: (Option<u32>, Option<u32>, u32),
    disturbed: Vec<String>,
    pages: Vec<(u8, String)>,
}

/// One step with `actions` of seat 0.
fn act(world: &mut SimWorld, actions: &[crate::action::Action]) -> Snapshot {
    let mut input = TickInput::default();
    input.insert(Seat(0), crate::input::Command::default());
    for action in actions {
        input.act(Seat(0), action.clone());
    }
    let tick = world.tick() + 1;
    crate::step(world, tick, &input, MSEC, StepReason::Authority)
}

fn k1() -> Result<K1, String> {
    use crate::action::Action as SeatAction;
    use crate::inventory::Held;
    use crate::module::ObjectData;
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    let entry = data.entry.position;
    let held = |template: &str| Held {
        template: template.to_string(),
        stack: 1,
        slot: None,
        droppable: true,
        infinite: false,
    };
    data.placements.extend(
        [
            ObjectData {
                sheet: Some(Sheet::dev_soldier()),
                template: TARGET.to_string(),
                ..ObjectData::bare(
                    ObjectKind::Creature,
                    TARGET,
                    [entry[0] + 2.0, entry[1] + 3.0, entry[2]],
                )
            },
            ObjectData {
                inventory: vec![held(BANDAGE.0)],
                container: true,
                useable: true,
                scripts: vec![("OnInvDisturbed".into(), "probe_k1_disturbed".into())],
                ..ObjectData::bare(ObjectKind::Placeable, CHEST, entry)
            },
            ObjectData::bare(
                ObjectKind::Placeable,
                SIGN,
                [entry[0] - 3.0, entry[1] + 3.0, entry[2]],
            ),
        ]
        .map(Placement::Own),
    );
    let worth = |(template, cost): (&str, u32)| ItemFacts {
        cost,
        base_item: 53,
        ..ItemFacts::plain(template)
    };
    let library = crate::probe::library(
        K1_CATALOG,
        [
            ("probe_k1_world", k1_world()),
            ("probe_k1_before", dealt(64, &[1024])),
            ("probe_k1_reads", dealt(60, &[1024, 4096, 3, 0])),
            ("probe_k1_disturbed", reads(70, 25)),
            (crate::script::presentation::SCREEN_CHANGED, reads(80, 9)),
        ],
    )
    .with_items(Catalog::new([worth(MEDKIT), worth(BANDAGE)]));
    let mut world = SimWorld::new(
        crate::WorldConfig::one_seat(),
        ground.with_module(data).with_scripts(library),
    )
    .map_err(|error| error.to_string())?;
    let start = step(&mut world);
    let objects = crate::Instances::in_snapshot(world.level(), &start);
    let entity_of = |tag: &str| {
        start
            .entities
            .iter()
            .find(|entity| {
                objects
                    .of_entity(entity)
                    .is_some_and(|data| data.tag == tag)
            })
            .ok_or(format!("no {tag}"))
    };
    let target = entity_of(TARGET)?.id.0;
    let chest = entity_of(CHEST)?;
    let (chest_id, chest_index) = match chest.kind {
        crate::Kind::Object(ObjectKind::Placeable, index) => (chest.id.0, index),
        _ => return Err("the chest is no placeable".into()),
    };
    let mut state = world.script_state().clone();
    for slot in [2, 12] {
        state.party.available.insert(
            slot,
            crate::script::party::Npc {
                template: format!("probe_k1_npc{slot}"),
                object: None,
                kept: None,
                template_data: None,
            },
        );
    }
    state
        .pending
        .push(Pending::new("probe_k1_before", ObjectId(target)));
    state
        .pending
        .push(Pending::new("probe_k1_world", crate::script::MODULE));
    world.set_script_state(state);
    let tokens = step(&mut world);
    let area_visual = world.script_state().applied.iter().any(|applied| {
        applied.at.is_some()
            && matches!(
                applied.effect,
                crate::script::effects::Effect::Visual { row: Some(row), .. }
                    if i32::from(row) == AREA_VISUAL
            )
    });
    let available = &world.script_state().party.available;
    let slots = (
        available.get(&2).and_then(|npc| npc.object),
        available.get(&12).and_then(|npc| npc.object),
        target,
    );
    let mut music: Vec<Notice> = world.take_notices().into_iter().map(|(_, n)| n).collect();
    // The damage lands, then the target reads it.
    step(&mut world);
    music.extend(world.take_notices().into_iter().map(|(_, n)| n));
    music.retain(|notice| matches!(notice, Notice::MusicVolume(_)));
    let mut state = world.script_state().clone();
    state
        .pending
        .push(Pending::new("probe_k1_reads", ObjectId(target)));
    world.set_script_state(state);
    let after = step(&mut world);
    let dealt = (60..=63).map(|n| read(&after, n)).collect();
    // Taken out of the chest, then put back.
    let mut disturbed = Vec::new();
    act(&mut world, &[SeatAction::Open(chest_index)]);
    let item = world
        .script_state()
        .loot(chest_id)
        .map(|(item, _)| item)
        .next()
        .ok_or("the chest holds nothing")?;
    let taken = act(
        &mut world,
        &[SeatAction::Take {
            container: chest_index,
            item,
            count: None,
        }],
    );
    disturbed.push(read(&taken, 70));
    let back = world
        .script_state()
        .items
        .iter()
        .find(|(_, held)| held.template == BANDAGE.0 && held.holder == crate::inventory::PARTY)
        .map(|(id, _)| *id)
        .ok_or("the party holds no bandage")?;
    let put = act(
        &mut world,
        &[SeatAction::Put {
            container: chest_index,
            item: back,
            count: None,
        }],
    );
    disturbed.push(read(&put, 70));
    // An in-game page shown, turned, closed.
    let mut pages = Vec::new();
    for page in [1, 5, 0] {
        let shown = act(&mut world, &[SeatAction::SubScreen(page)]);
        pages.push((world.script_state().subscreen, read(&shown, 80)));
    }
    Ok(K1 {
        before: read(&tokens, 64),
        tokens,
        dealt,
        music,
        area_visual,
        slots,
        disturbed,
        pages,
    })
}

/// The routines K1's own scripts called unbound until now.
fn k1_checks() -> Checks {
    let ran = match k1() {
        Ok(ran) => ran,
        Err(error) => return vec![("k1_routines", Err(error))],
    };
    let bad: Vec<String> = K1_TOKENS
        .iter()
        .filter(|(n, _, want)| read(&ran.tokens, *n) != *want)
        .map(|(n, what, want)| format!("{what}: {:?}, want {want:?}", read(&ran.tokens, *n)))
        .collect();
    let want_dealt = ["10", "-1", "0", "0"];
    let (slot, past, target) = ran.slots;
    let want_pages = [
        (1, "1".to_string()),
        (5, "5".to_string()),
        (0, "5".to_string()),
    ];
    let (unlisted, unanswered) = crate::script::answered_drift();
    vec![
        (
            "minigame_routines_counted_bound",
            check(
                unlisted.is_empty() && unanswered.is_empty(),
                format!(
                    "SWMG_ routines answered but not counted bound {unlisted:?}, counted but not answered {unanswered:?}"
                ),
            ),
        ),
        (
            "k1_routines_scripted",
            check(
                bad.is_empty(),
                format!("{} tokens; wrong: {bad:?}", K1_TOKENS.len()),
            ),
        ),
        (
            "music_volume_clamped",
            check(
                ran.music == [Notice::MusicVolume(1.0f32.to_bits())],
                format!("told {:?}", ran.music),
            ),
        ),
        (
            "damage_dealt_by_type",
            check(
                ran.before == "-1" && ran.dealt == want_dealt,
                format!(
                    "before {}, sonic/energy/two bits/none {:?}, want -1 and {want_dealt:?}",
                    ran.before, ran.dealt
                ),
            ),
        ),
        (
            "visual_at_a_place",
            check(
                ran.area_visual,
                format!("visual {AREA_VISUAL} put at a place: {}", ran.area_visual),
            ),
        ),
        (
            "available_npc_object",
            check(
                slot == Some(target) && past.is_none(),
                format!("slot 2 {slot:?}, slot 12 (past the table) {past:?}"),
            ),
        ),
        (
            "inventory_disturb_type",
            check(
                ran.disturbed == ["1", "0"],
                format!("taken, put back: {:?}", ran.disturbed),
            ),
        ),
        (
            "sub_screen_script",
            check(ran.pages == want_pages, format!("pages {:?}", ran.pages)),
        ),
    ]
}
