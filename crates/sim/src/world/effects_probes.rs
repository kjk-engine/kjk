//! The probes of TSL's effects and actions (`kjk sim probes "tsl effects"`),
//! on the proving ground, no install, with the executable's numbers:
//! - KOTOR's states (`combat::effects::state`): a cutscene's paralysis holds
//!   its 4 s whole on one immune to paralysis and with Force Immunity:
//!   Paralysis, which keep the plain one off; a cutscene's horror holds past
//!   10 s until `ClearAllEffects`, its end running the end-of-round script;
//!   the highest state holds (a mind trick over a stun, the stun again once
//!   the trick is over), the stun's visual (2002) shown as it takes hold;
//!   Force Crush's state holds its 2 s; a mind trick is refused by mind
//!   immunity, a droid's scramble by stun immunity, a droid's confusion by
//!   droid confusion immunity, a cutscene's stun by none;
//! - a mind trick breaks on a hostile perceived within √10 m (not at 3.5 m)
//!   and on damage;
//! - the routines (`script::engine`): `GetEffectType` (80, 15, 38, 67, 3,
//!   64, 0, 29), `GetIsImmune` (an immunity effect, no feats, the type's
//!   range), `RemoveEffectByExactMatch` (the first of two),
//!   `FaceObjectAwayFromObject`;
//! - Fury's kills add to damage up to 6, Force Sight's and Fury's video
//!   rows (4; 7 to 9 by spell), Force Body's split of a cost (50 % from FP,
//!   as much from HP; refused short of either), the vitality regeneration
//!   modifier doubling healing however many percent, a regeneration giving
//!   its hit points each interval, a faction modifier turning a creature
//!   and turning it back with its power's global reset;
//! - the actions: a fake cast holds the hand for its cast time and costs
//!   nothing, a cheat's real cast runs its impact at its end, one the caster
//!   does not know does nothing; a queued surrender comes after the wait
//!   before it; `ActionUseSkill` stealth turns stealth on.

use super::{Fighting, Id, Player, SimWorld};
use crate::combat::Sheet;
use crate::combat::effects::{self, Lasting, Modifies, state};
use crate::combat::force::crowd::immunity;
use crate::input::{Seat, TickInput};
use crate::module::{ObjectKind, Placement, Repute};
use crate::probe::{Checks, check};
use crate::script::actions::{Action, Target};
use crate::script::effects::{Applied, Effect};
use crate::script::{Pending, Talent, Talents};
use crate::snapshot::{SimId, Snapshot};
use crate::{Level, StepReason};
use fmt_odyssey::ncs::{Const, Ncs, Op};
use script_ncs::probes::Asm;
use script_ncs::vm::ObjectId;

/// A step's length (ms).
const MSEC: u32 = 100;

/// The creatures' tags, the one the scripts aim at, and where each stands
/// from the entry (metres, across the ground's floor).
const TARGET: &str = "probe_target";
const OTHER: &str = "probe_other";
const PLACES: [(&str, f32); 2] = [(TARGET, 3.5), (OTHER, -2.5)];

/// The `spells.2da` rows the probes cast: one whose impact sets token 30, a
/// cast taking `CAST_MS`.
const CAST_ROW: u16 = 3;
const CAST_MS: u16 = 1500;

const CATALOG: &str = r#"
int EFFECT_TYPE_IMMUNITY = 15;
int EFFECT_TYPE_ABILITY_INCREASE = 38;
int EFFECT_TYPE_BLINDNESS = 67;
int EFFECT_TYPE_REGENERATE = 3;
int EFFECT_TYPE_TRUESEEING = 64;
int EFFECT_TYPE_INVALIDEFFECT = 0;
int SKILL_STEALTH = 2;
// 0
void SetCustomToken(int nCustomTokenNumber, string sTokenValue);
// 1
string IntToString(int nInteger);
// 2
object GetObjectByTag(string sTag, int nNth = 0);
// 3
void ApplyEffectToObject(int nDurationType, effect eEffect, object oTarget, float fDuration = 0.0f);
// 4
effect EffectMindTrick();
// 5
effect GetFirstEffect(object oCreature);
// 6
effect GetNextEffect(object oCreature);
// 7
int GetEffectType(effect eEffect);
// 8
effect EffectImmunity(int nImmunityType);
// 9
int GetIsImmune(object oCreature, int nImmunityType, object oVersus = OBJECT_INVALID);
// 10
effect EffectAbilityIncrease(int nAbilityToIncrease, int nModifyBy);
// 11
void RemoveEffectByExactMatch(object oCreature, effect eEffect);
// 12
void FaceObjectAwayFromObject(object oFacer, object oObjectToFaceAwayFrom);
// 13
object GetFirstPC();
// 14
effect EffectCutSceneStunned();
// 15
effect EffectCrush();
// 16
effect EffectBlind();
// 17
effect EffectRegenerate(int nAmount, float fIntervalSeconds);
// 18
effect EffectTrueSeeing();
// 19
void ActionUseSkill(int nSkill, object oTarget, int nSubSkill = 0, object oItemUsed = OBJECT_INVALID);
// 20
effect EffectAssuredHit();
"#;

fn call(routine: u16, args: u8) -> Op {
    Op::Action { routine, args }
}

fn int(value: i32) -> Op {
    Op::Const(Const::Int(value))
}

fn target() -> [Op; 3] {
    [
        int(0),
        Op::Const(Const::String(TARGET.as_bytes().to_vec())),
        call(2, 2),
    ]
}

/// Custom token `n` set to what `value` leaves (an int, as a string).
fn token(asm: &mut Asm, n: i32, value: &[Op]) {
    for op in value {
        asm.op(op.clone());
    }
    asm.op(call(1, 1)).op(int(n)).op(call(0, 2));
}

/// Applies, permanently, the effect `effect` leaves to the target.
fn apply(asm: &mut Asm, effect: &[Op]) {
    asm.op(Op::Const(Const::Float(0.0)));
    for op in target() {
        asm.op(op);
    }
    for op in effect {
        asm.op(op.clone());
    }
    asm.op(int(2)).op(call(3, 4));
}

/// `probe_apply`: on the target, permanently, a mind trick, immunity to
/// poison, +4 STR twice, blindness, a regeneration of 4 every 6 s, true
/// seeing, Force Crush's state and a cutscene's stun.
fn applies() -> Ncs {
    let mut asm = Asm::default();
    apply(&mut asm, &[call(4, 0)]);
    apply(&mut asm, &[int(i32::from(immunity::POISON)), call(8, 1)]);
    for _ in 0..2 {
        apply(&mut asm, &[int(4), int(0), call(10, 2)]);
    }
    apply(&mut asm, &[call(16, 0)]);
    apply(
        &mut asm,
        &[Op::Const(Const::Float(6.0)), int(4), call(17, 2)],
    );
    apply(&mut asm, &[call(18, 0)]);
    apply(&mut asm, &[call(15, 0)]);
    apply(&mut asm, &[call(14, 0)]);
    asm.op(Op::Retn);
    asm.build()
}

/// `probe_assure_now`: an assured hit on the target, at once
/// (`DURATION_TYPE_INSTANT`).
fn assures_now() -> Ncs {
    let mut asm = Asm::default();
    asm.op(Op::Const(Const::Float(0.0)));
    for op in target() {
        asm.op(op);
    }
    asm.op(call(20, 0)).op(int(0)).op(call(3, 4)).op(Op::Retn);
    asm.build()
}

/// `probe_read`: tokens 1 to 9 the target's first nine effects' types;
/// 20 to 22 `GetIsImmune` to poison, fear and type 34; then one +4 STR
/// taken off by its exact match, and the target turned away from the PC.
fn reads() -> Ncs {
    let mut asm = Asm::default();
    for n in 1..=9 {
        let mut walk = target().to_vec();
        walk.push(call(if n == 1 { 5 } else { 6 }, 1));
        walk.push(call(7, 1));
        token(&mut asm, n, &walk);
    }
    for (n, kind) in [(20, immunity::POISON), (21, immunity::FEAR), (22, 34)] {
        let mut asks = vec![
            Op::Const(Const::Object(ObjectId::INVALID.0 as i32)),
            int(i32::from(kind)),
        ];
        asks.extend(target());
        asks.push(call(9, 3));
        token(&mut asm, n, &asks);
    }
    asm.op(int(4)).op(int(0)).op(call(10, 2));
    for op in target() {
        asm.op(op);
    }
    asm.op(call(11, 2));
    asm.op(call(13, 0));
    for op in target() {
        asm.op(op);
    }
    asm.op(call(12, 2));
    asm.op(Op::Retn);
    asm.build()
}

/// `probe_stealth`, run by the target: stealth used on itself.
fn stealth() -> Ncs {
    let mut asm = Asm::default();
    asm.op(Op::Const(Const::Object(ObjectId::INVALID.0 as i32)))
        .op(int(0))
        .op(Op::Const(Const::Object(0)))
        .op(int(2))
        .op(call(19, 4))
        .op(Op::Retn);
    asm.build()
}

/// `probe_impact` (`CAST_ROW`'s impact) and `probe_end_round` (the
/// target's end-of-round script): tokens 30 and 31 set to 1.
fn marks(n: i32) -> Ncs {
    let mut asm = Asm::default();
    token(&mut asm, n, &[int(1)]);
    asm.op(Op::Retn);
    asm.build()
}

/// The ground with the player, the target and another creature (soldiers;
/// the target hostile to the player, its end-of-round script
/// `probe_end_round`, its sheet `sheet`), the scripts, `CAST_ROW` castable.
fn world(sheet: Sheet) -> Result<(SimWorld, SimId, SimId, SimId), String> {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    let entry = data.entry.position;
    for (tag, ahead) in PLACES {
        let target = tag == TARGET;
        data.placements
            .push(Placement::Own(crate::module::ObjectData {
                sheet: Some(if target { sheet } else { Sheet::dev_soldier() }),
                faction: 0,
                scripts: if target {
                    vec![("ScriptEndRound".into(), "probe_end_round".into())]
                } else {
                    Vec::new()
                },
                ..crate::module::ObjectData::bare(
                    ObjectKind::Creature,
                    tag,
                    [entry[0] + ahead, entry[1], entry[2]],
                )
            }));
    }
    // Hostile 1 and the player at war, the rest neutral.
    let mut rows = vec![vec![50u8; 7]; 7];
    for (at, row) in rows.iter_mut().enumerate() {
        row[at] = 100;
    }
    rows[0][1] = 0;
    rows[1][0] = 0;
    data.repute = Repute { rows };
    data.force_costs = vec![20; 8];
    let mut spells = vec![Talent::default(); 8];
    spells[usize::from(CAST_ROW)].cast_ms = CAST_MS;
    let mut impacts = vec![String::new(); 8];
    impacts[usize::from(CAST_ROW)] = "probe_impact".into();
    let library = crate::probe::library_with(
        CATALOG,
        [
            ("probe_apply", applies()),
            ("probe_read", reads()),
            ("probe_stealth", stealth()),
            ("probe_assure_now", assures_now()),
            ("probe_impact", marks(30)),
            ("probe_end_round", marks(31)),
        ],
        Talents {
            spells,
            impacts,
            ..Talents::default()
        },
    )
    .with_regeneration(Some(super::tsl_probes::TSL));
    let level: Level = ground.with_module(data).with_scripts(library);
    let mut world =
        SimWorld::new(crate::WorldConfig::one_seat(), level).map_err(|error| error.to_string())?;
    let first = step(&mut world);
    let objects = crate::Instances::in_snapshot(world.level(), &first);
    let tagged = |tag: &str| {
        first
            .entities
            .iter()
            .find(|entity| {
                objects
                    .of_entity(entity)
                    .is_some_and(|data| data.tag == tag)
            })
            .map(|entity| entity.id)
            .ok_or(format!("no {tag}"))
    };
    let player = first.body(Seat(0)).map(|e| e.id).ok_or("no player")?;
    Ok((world, player, tagged(TARGET)?, tagged(OTHER)?))
}

/// The target's sheet: a soldier with `feats`, and stealth.
fn soldier(feats: &[u16]) -> Sheet {
    let mut sheet = Sheet::dev_soldier();
    for feat in feats {
        sheet.feats.insert(*feat);
    }
    sheet.faction = crate::combat::sheet::factions::HOSTILE_1;
    sheet.skills[crate::combat::sheet::skills::STEALTH] = 4;
    sheet.flags |= crate::combat::sheet::flags::STEALTH_UNIT;
    sheet
}

fn step(world: &mut SimWorld) -> Snapshot {
    let mut input = TickInput::default();
    input.insert(Seat(0), crate::input::Command::default());
    let tick = world.tick() + 1;
    crate::step(world, tick, &input, MSEC, StepReason::Authority)
}

/// `world` stepped for `ms`: the last snapshot.
fn wait(world: &mut SimWorld, ms: u64) -> Snapshot {
    let mut last = step(world);
    for _ in 1..ms / u64::from(MSEC) {
        last = step(world);
    }
    last
}

/// The script state changed by `change`.
fn script(world: &mut SimWorld, change: impl FnOnce(&mut crate::script::State)) {
    let mut state = world.script_state().clone();
    change(&mut state);
    world.set_script_state(state);
}

/// `effect` applied to `on` for `ms` (`u64::MAX`: permanently), its id
/// `id`, as a script's would be, from the next step.
fn put(world: &mut SimWorld, on: SimId, effect: Effect, ms: u64, id: u32) {
    let now = world.snapshot().time_ms;
    script(world, |state| {
        state.applied.push(Applied {
            target: ObjectId(on.0),
            by: ObjectId(on.0),
            effect,
            until: Some(if ms == u64::MAX { ms } else { now + ms }),
            spell: u16::MAX,
            id,
            at: None,
        })
    });
}

fn held_state(state: u8, cutscene: bool) -> Effect {
    Effect::State { state, cutscene }
}

/// `who`'s fight as it stands.
fn fighter(world: &mut SimWorld, who: SimId) -> Option<crate::combat::Fighter> {
    world
        .world
        .query::<(&Id, &Fighting)>()
        .iter(&world.world)
        .find(|(id, _)| id.0 == who)
        .map(|(_, fighting)| fighting.0)
}

/// `who`'s fight changed by `change`.
fn alter(world: &mut SimWorld, who: SimId, change: impl FnOnce(&mut crate::combat::Fighter)) {
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

/// Whether `who` is held still at the world's time.
fn held(world: &mut SimWorld, who: SimId) -> bool {
    let now = world.snapshot().time_ms;
    fighter(world, who).is_some_and(|f| f.force.stunned(now))
}

/// An immunity lasting on `who` from now, as `EffectImmunity` gives it.
fn immune(world: &mut SimWorld, who: SimId, kind: u8, id: u32) {
    let effect = Effect::Modifier {
        modifies: Modifies::Immunity,
        amount: 1,
        of: kind,
    };
    put(world, who, effect, u64::MAX, id);
}

fn tokens(snapshot: &Snapshot, numbers: impl Iterator<Item = i32>) -> Vec<String> {
    numbers
        .map(|n| {
            snapshot
                .talk
                .tokens
                .get(&n)
                .cloned()
                .unwrap_or_else(|| "-".into())
        })
        .collect()
}

pub fn probes() -> Checks {
    vec![
        ("cutscene_paralysis", cutscene_paralysis()),
        ("cutscene_horror", cutscene_horror()),
        (
            "state_end_unheard_by_the_dead",
            state_end_unheard_by_the_dead(),
        ),
        ("highest_state", highest_state()),
        ("crush", crush()),
        ("state_immunities", state_immunities()),
        ("mind_trick_breaks", mind_trick_breaks()),
        ("routines", routines()),
        ("fury", fury()),
        ("video_rows", video_rows()),
        ("force_body", force_body()),
        ("healing_doubled", healing_doubled()),
        ("regenerate", regenerate()),
        ("faction_modifier", faction_modifier()),
        ("casts", casts()),
        ("player_casts", player_casts()),
        ("surrender_queued", surrender_queued()),
        ("use_skill_stealth", use_skill_stealth()),
        ("lasting_at_once_stays", lasting_at_once_stays()),
    ]
}

/// On one immune to paralysis with Force Immunity: Paralysis, the plain
/// state refused; a cutscene's for 4 s held at 3.9 s, free at 4.1 s.
fn cutscene_paralysis() -> Result<String, String> {
    use crate::combat::sheet::feats::FORCE_IMMUNITY_PARALYSIS;
    let (mut world, _, target, _) = world(soldier(&[FORCE_IMMUNITY_PARALYSIS]))?;
    immune(&mut world, target, immunity::PARALYSIS, 1);
    step(&mut world);
    put(
        &mut world,
        target,
        held_state(state::PARALYZED, false),
        4000,
        2,
    );
    step(&mut world);
    let plain = held(&mut world, target);
    put(
        &mut world,
        target,
        held_state(state::PARALYZED, true),
        4000,
        3,
    );
    wait(&mut world, 3900);
    let at_39 = held(&mut world, target);
    wait(&mut world, 300);
    let at_42 = held(&mut world, target);
    check(
        !plain && at_39 && !at_42,
        format!("plain held {plain}; cutscene held at 3.9 s {at_39}, at 4.2 s {at_42}"),
    )
}

/// A permanent cutscene horror on one with Force Immunity: Fear: held at
/// 10 s, freed by `ClearAllEffects`, its end running the end-of-round
/// script.
fn cutscene_horror() -> Result<String, String> {
    use crate::combat::sheet::feats::FORCE_IMMUNITY_FEAR;
    let (mut world, _, target, _) = world(soldier(&[FORCE_IMMUNITY_FEAR]))?;
    put(
        &mut world,
        target,
        held_state(state::HORRIFIED, true),
        u64::MAX,
        1,
    );
    let late = wait(&mut world, 10_000);
    let at_10 = held(&mut world, target);
    let before = tokens(&late, 31..=31);
    script(&mut world, |state| {
        state.removed.push((target.0, None, 0));
    });
    step(&mut world);
    let after = step(&mut world);
    let freed = !held(&mut world, target);
    let ran = tokens(&after, 31..=31);
    check(
        at_10 && freed && before == ["-"] && ran == ["1"],
        format!("held at 10 s {at_10}; freed {freed}; end-of-round token {before:?} → {ran:?}"),
    )
}

/// A cutscene horror ending on a body that died under it runs no
/// end-of-round script: the dead have no round, as they hear no heartbeat.
fn state_end_unheard_by_the_dead() -> Result<String, String> {
    let (mut world, _, target, _) = world(soldier(&[]))?;
    put(
        &mut world,
        target,
        held_state(state::HORRIFIED, true),
        u64::MAX,
        1,
    );
    step(&mut world);
    alter(&mut world, target, |f| {
        f.hp = 0;
        f.life = crate::combat::fighter::Life::Dead;
    });
    step(&mut world);
    script(&mut world, |state| {
        state.removed.push((target.0, None, 0));
    });
    step(&mut world);
    let after = step(&mut world);
    let freed = !held(&mut world, target);
    let ran = tokens(&after, 31..=31);
    let dead = fighter(&mut world, target).is_some_and(|f| f.dead());
    check(
        freed && dead && ran == ["-"],
        format!("freed {freed}; dead {dead}; end-of-round token {ran:?}"),
    )
}

/// A stun for 6 s, then a mind trick for 3 s: the trick holds (its visual
/// 2002 shown), then the stun again, then nothing.
fn highest_state() -> Result<String, String> {
    let (mut world, _, target, _) = world(soldier(&[]))?;
    put(
        &mut world,
        target,
        held_state(state::STUNNED, false),
        6000,
        1,
    );
    step(&mut world);
    put(
        &mut world,
        target,
        held_state(state::MIND_TRICK, false),
        3000,
        2,
    );
    step(&mut world);
    let states = |world: &mut SimWorld| fighter(world, target).map_or(0, |f| f.force.state);
    let first = states(&mut world);
    let now = world.snapshot().time_ms;
    let visual = fighter(&mut world, target)
        .is_some_and(|f| f.effects.visuals(now).any(|(row, _, _)| row == 2002));
    wait(&mut world, 3000);
    let second = states(&mut world);
    wait(&mut world, 3000);
    let last = states(&mut world);
    check(
        (first, second, last) == (state::MIND_TRICK, state::STUNNED, 0) && visual,
        format!("states {first}, {second}, {last}; stun's visual shown {visual}"),
    )
}

/// Force Crush's state for 2 s on one immune to mind effects: held at
/// 1.9 s, free at 2.1 s.
fn crush() -> Result<String, String> {
    let (mut world, _, target, _) = world(soldier(&[]))?;
    immune(&mut world, target, immunity::MIND_SPELLS, 1);
    step(&mut world);
    put(&mut world, target, held_state(state::CRUSH, false), 2000, 2);
    wait(&mut world, 1900);
    let at_19 = held(&mut world, target);
    wait(&mut world, 200);
    let at_21 = held(&mut world, target);
    check(
        at_19 && !at_21,
        format!("held at 1.9 s {at_19}, at 2.1 s {at_21}"),
    )
}

/// Each state on one with an immunity: mind trick by mind immunity, a
/// droid's scramble by stun immunity, a droid's confusion by droid
/// confusion immunity (33) refused; a cutscene's stun on one immune to
/// stuns taken.
fn state_immunities() -> Result<String, String> {
    let cases = [
        (immunity::MIND_SPELLS, state::MIND_TRICK, false, false),
        (immunity::STUN, state::DROID_SCRAMBLE, false, false),
        (33, state::DROID_CONFUSED, false, false),
        (immunity::STUN, state::STUNNED, true, true),
    ];
    let mut seen = Vec::new();
    for (kind, held_by, cutscene, want) in cases {
        let (mut world, _, target, _) = world(soldier(&[]))?;
        immune(&mut world, target, kind, 1);
        step(&mut world);
        put(&mut world, target, held_state(held_by, cutscene), 5000, 2);
        step(&mut world);
        let now = world.snapshot().time_ms;
        let took = fighter(&mut world, target).is_some_and(|f| {
            f.effects
                .running(now)
                .any(|l| l.modifies == Modifies::State && l.of == held_by)
        });
        seen.push((held_by, took, want));
    }
    check(
        seen.iter().all(|(_, took, want)| took == want),
        format!("(state, taken, wanted): {seen:?}"),
    )
}

/// A tricked hostile perceiving the player: tricked with the player 3.5 m
/// off, woken once the player stands 2 m off; tricked again, woken by a
/// point of damage.
fn mind_trick_breaks() -> Result<String, String> {
    let (mut world, player, target, _) = world(soldier(&[]))?;
    put(
        &mut world,
        target,
        held_state(state::MIND_TRICK, false),
        30_000,
        1,
    );
    script(&mut world, |state| {
        state.perceived.insert(
            target.0,
            crate::script::Perceived {
                seen: vec![player.0],
                heard: Vec::new(),
            },
        );
    });
    wait(&mut world, 500);
    let far = held(&mut world, target);
    let there = world
        .snapshot()
        .get(target)
        .map_or([0.0; 3], |e| e.position);
    for (id, mut body) in world
        .world
        .query::<(&Id, &mut Player)>()
        .iter_mut(&mut world.world)
    {
        if id.0 == player {
            let near = [there[0] - 2.0, there[1], there[2]];
            body.0.origin = near.map(|v| v / collision::UNIT);
        }
    }
    wait(&mut world, 300);
    let near = held(&mut world, target);
    script(&mut world, |state| state.perceived.clear());
    put(
        &mut world,
        target,
        held_state(state::MIND_TRICK, false),
        30_000,
        2,
    );
    wait(&mut world, 300);
    let again = held(&mut world, target);
    put(
        &mut world,
        target,
        Effect::Damage { amount: 1, kind: 0 },
        0,
        3,
    );
    wait(&mut world, 300);
    let hurt = held(&mut world, target);
    check(
        far && !near && again && !hurt,
        format!("at 3.5 m tricked {far}, at 2 m {near}; again {again}, after damage {hurt}"),
    )
}

/// The routines on the target (`probe_apply`, then `probe_read`).
fn routines() -> Result<String, String> {
    let (mut world, player, target, _) =
        world(soldier(&[crate::combat::sheet::feats::FORCE_IMMUNITY_FEAR]))?;
    script(&mut world, |state| {
        state
            .pending
            .push(Pending::new("probe_apply", crate::script::MODULE));
    });
    wait(&mut world, 300);
    // Turned across first, so that turning away is seen.
    for (id, mut body) in world
        .world
        .query::<(&Id, &mut Player)>()
        .iter_mut(&mut world.world)
    {
        if id.0 == target {
            body.0.face(90.0);
        }
    }
    script(&mut world, |state| {
        state
            .pending
            .push(Pending::new("probe_read", crate::script::MODULE));
    });
    let read = wait(&mut world, 300);
    let types = tokens(&read, 1..=9);
    let immune = tokens(&read, 20..=22);
    let now = read.time_ms;
    let strength = fighter(&mut world, target).map_or(0, |f| {
        f.effects
            .running(now)
            .filter(|l| l.modifies == Modifies::Ability)
            .count()
    });
    let at = |id: SimId| read.get(id).map_or([0.0; 3], |e| e.position);
    let (from, to) = (at(target), at(player));
    let away = crate::math::atan2_deg(from[1] - to[1], from[0] - to[0]);
    let facing = read
        .get(target)
        .and_then(|e| e.player)
        .map_or(f32::NAN, |ps| ps.view[1]);
    let turned = crate::combat::mind::turn(facing, away).abs() < 1.0;
    let want = ["80", "15", "38", "38", "67", "3", "64", "0", "29"];
    check(
        types == want && immune == ["1", "0", "0"] && strength == 1 && turned,
        format!(
            "types {types:?} (want {want:?}); immune to poison, fear, 34: {immune:?}; \
             +4 STR left {strength}; facing {facing:.1}° from 90° (away {away:.1}°)"
        ),
    )
}

/// Fury on the target: its kills counted from 0 (its weapon's blow killing
/// the other creature counts one) add to its weapons' damage, never past 6;
/// none counted once Fury ends.
fn fury() -> Result<String, String> {
    let (mut world, _, target, other) = world(soldier(&[]))?;
    let fury = Effect::Modifier {
        modifies: Modifies::Fury,
        amount: 0,
        of: 0,
    };
    put(&mut world, target, fury, 60_000, 1);
    step(&mut world);
    let sheet = soldier(&[]);
    // Its blows on the other, at 1 HP, until one lands and kills.
    alter(&mut world, other, |f| f.hp = 100);
    let weapon = sheet.main_weapon();
    for _ in 0..20 {
        if fighter(&mut world, other).is_none_or(|f| !f.alive()) {
            break;
        }
        world
            .world
            .resource_mut::<super::Hits>()
            .0
            .push(super::Hit {
                attacker: target,
                target: other,
                source: crate::combat::damage::Source::Weapon {
                    weapon,
                    cadence_ms: 1000,
                    share: 1000,
                    area: false,
                },
                point: None,
                direction: None,
            });
        step(&mut world);
    }
    let killed = fighter(&mut world, target).and_then(|f| f.effects.fury);
    alter(&mut world, target, |f| f.effects.fury = Some(0));
    let bonus = |f: &crate::combat::Fighter, now| {
        effects::sheet_now(sheet, f, now)
            .weapons
            .iter()
            .flatten()
            .map(|w| i32::from(w.damage_bonus))
            .next()
            .unwrap_or(0)
    };
    let now = world.snapshot().time_ms;
    let start = fighter(&mut world, target).map(|f| (f.effects.fury, bonus(&f, now)));
    alter(&mut world, target, |f| f.effects.fury = Some(3));
    let three = fighter(&mut world, target).map(|f| bonus(&f, now));
    alter(&mut world, target, |f| {
        for _ in 0..5 {
            f.effects.fury = f.effects.fury.map(|k| (k + 1).min(effects::FURY_MOST));
        }
    });
    let most = fighter(&mut world, target).map(|f| (f.effects.fury, bonus(&f, now)));
    let base = bonus(&crate::combat::Fighter::fresh(10, 0), now);
    script(&mut world, |state| {
        state.removed.push((target.0, Some(1), 0))
    });
    step(&mut world);
    let ended = fighter(&mut world, target).map(|f| f.effects.fury);
    check(
        killed == Some(1)
            && start == Some((Some(0), base))
            && three == Some(base + 3)
            && most == Some((Some(6), base + 6))
            && ended == Some(None),
        format!(
            "kills after a killing blow {killed:?}; (kills, damage bonus) from 0 {start:?}, \
             three kills {three:?}, eight {most:?} (base {base}); ended {ended:?}"
        ),
    )
}

/// The video rows: Force Sight 4; Fury by its spell (165: 8); a Fury of no
/// such spell none; Force Sight over Fury.
fn video_rows() -> Result<String, String> {
    let lasting = |modifies, spell| Lasting {
        modifies,
        amount: 0,
        of: 0,
        until: u64::MAX,
        spell,
        by: 0,
        id: 1,
        at: 0,
    };
    let row = |parts: &[(Modifies, u16)]| {
        let mut on = effects::Effects::default();
        for (modifies, spell) in parts {
            on.add(lasting(*modifies, *spell), 0);
        }
        on.video_row(1)
    };
    let seen = [
        row(&[(Modifies::ForceSight, u16::MAX)]),
        row(&[(Modifies::Fury, 165)]),
        row(&[(Modifies::Fury, 12)]),
        row(&[(Modifies::Fury, 166), (Modifies::ForceSight, u16::MAX)]),
    ];
    check(
        seen == [Some(4), Some(8), None, Some(4)],
        format!("sight, fury 165, fury 12, both: {seen:?}"),
    )
}

/// Force Body (level 0) and a 20-point power: 10 from FP, 10 from HP; with
/// 5 FP, or 11 HP, not paid; Improved (1): 8 and 8. A cast landing charges
/// whatever is left (`effects::charge`): with 5 FP, 0 FP and 10 HP off.
fn force_body() -> Result<String, String> {
    let body = |level: u8, (fp, hp): (i32, i32), landing: bool| {
        let mut f = crate::combat::Fighter::fresh(100, 100);
        f.effects.add(
            Lasting {
                modifies: Modifies::ForceBody,
                amount: 0,
                of: level,
                until: u64::MAX,
                spell: 150,
                by: 0,
                id: 1,
                at: 0,
            },
            0,
        );
        (f.fp, f.hp) = (fp * 100, hp * 100);
        let paid = if landing {
            effects::charge(&mut f, 2000, 1);
            true
        } else {
            effects::pay(&mut f, 2000, 1)
        };
        (paid, f.fp / 100, f.hp / 100)
    };
    let seen = [
        body(0, (30, 50), false),
        body(0, (5, 50), false),
        body(0, (30, 11), false),
        body(1, (30, 50), false),
        body(0, (5, 50), true),
    ];
    check(
        seen == [
            (true, 20, 40),
            (false, 5, 50),
            (false, 30, 11),
            (true, 22, 42),
            (true, 0, 40),
        ],
        format!("(paid, FP, HP): {seen:?}"),
    )
}

/// Natural healing (TSL's `regeneration.2da`) with one vitality modifier
/// of 50 % and with two: double and four times the healing without.
fn healing_doubled() -> Result<String, String> {
    let healed = |modifiers: u32| -> Result<i32, String> {
        let (mut world, player, _, _) = world(soldier(&[]))?;
        for n in 0..modifiers {
            let effect = Effect::Modifier {
                modifies: Modifies::VpRegen,
                amount: 50,
                of: 0,
            };
            put(&mut world, player, effect, u64::MAX, 10 + n);
        }
        step(&mut world);
        alter(&mut world, player, |f| f.hp = 100);
        let before = fighter(&mut world, player).map_or(0, |f| f.hp);
        wait(&mut world, 6200);
        Ok(fighter(&mut world, player).map_or(0, |f| f.hp) - before)
    };
    let seen = [healed(0)?, healed(1)?, healed(2)?];
    check(
        seen[0] > 0 && seen[1] == 2 * seen[0] && seen[2] == 4 * seen[0],
        format!("healed in 6.2 s with 0, 1, 2 modifiers: {seen:?} hundredths"),
    )
}

/// A regeneration of 4 every 6 s on the target at 1 HP of 12: none at
/// 5.9 s, 4 at 6.2 s, 8 at 12.2 s.
fn regenerate() -> Result<String, String> {
    let (mut world, _, target, _) = world(soldier(&[]))?;
    alter(&mut world, target, |f| f.hp = 100);
    let effect = Effect::Modifier {
        modifies: Modifies::Regenerate,
        amount: 6000,
        of: 4,
    };
    put(&mut world, target, effect, u64::MAX, 1);
    step(&mut world);
    let hp = |world: &mut SimWorld| fighter(world, target).map_or(0, |f| f.hp / 100);
    wait(&mut world, 5800);
    let early = hp(&mut world);
    wait(&mut world, 300);
    let first = hp(&mut world);
    wait(&mut world, 6000);
    let second = hp(&mut world);
    check(
        (early, first, second) == (1, 5, 9),
        format!("HP at 5.9 s {early}, 6.2 s {first}, 12.2 s {second} (from 1)"),
    )
}

/// Force Confusion's faction modifier (spell 200, friendly 1) on the
/// hostile target for 3 s, its global set: friendly meanwhile, hostile
/// after, the global 0; refused on the player.
fn faction_modifier() -> Result<String, String> {
    use crate::combat::sheet::factions::{FRIENDLY_1, HOSTILE_1};
    let (mut world, player, target, _) = world(soldier(&[]))?;
    script(&mut world, |state| {
        crate::script::set_global(&mut state.numbers, "000_Human_Conf_Active", 1);
    });
    let now = world.snapshot().time_ms;
    for (on, id) in [(target, 1), (player, 2)] {
        script(&mut world, |state| {
            state.applied.push(Applied {
                target: ObjectId(on.0),
                by: ObjectId(player.0),
                effect: Effect::Modifier {
                    modifies: Modifies::Faction,
                    amount: i16::from(FRIENDLY_1),
                    of: 0,
                },
                until: Some(now + 3000),
                spell: 200,
                id,
                at: None,
            })
        });
    }
    step(&mut world);
    let faction = |world: &mut SimWorld, who| fighter(world, who).and_then(|f| f.faction);
    let during = (faction(&mut world, target), faction(&mut world, player));
    wait(&mut world, 3200);
    let after = faction(&mut world, target);
    let global = world
        .script_state()
        .numbers
        .get("000_Human_Conf_Active")
        .copied();
    check(
        during == (Some(FRIENDLY_1), None) && after == Some(HOSTILE_1) && global == Some(0),
        format!("(target, player) during {during:?}; target after {after:?}; global {global:?}"),
    )
}

/// The target's casts at the player: a fake one holds the hand and costs
/// nothing, its impact never run; a cheat's runs its impact after its cast
/// time (not before); one it does not know does nothing.
fn casts() -> Result<String, String> {
    let (mut world, player, target, _) = world(soldier(&[]))?;
    let queue = |world: &mut SimWorld, fake: bool, cheat: bool| {
        script(world, |state| {
            state.queue(
                ObjectId(target.0),
                Action::Cast {
                    row: CAST_ROW,
                    target: Target::Object(ObjectId(player.0)),
                    fake,
                    cheat,
                },
            )
        });
    };
    let before = fighter(&mut world, target).map(|f| (f.fp, f.hp));
    queue(&mut world, true, false);
    let started = wait(&mut world, 300);
    let hand = fighter(&mut world, target).is_some_and(|f| f.force.hand.is_some());
    let done = wait(&mut world, 1500);
    let fake = (
        hand,
        tokens(&started, 30..=30),
        tokens(&done, 30..=30),
        fighter(&mut world, target).map(|f| (f.fp, f.hp)) == before,
    );
    queue(&mut world, false, false);
    let unknown = tokens(&wait(&mut world, 2000), 30..=30);
    queue(&mut world, false, true);
    let early = tokens(&wait(&mut world, 1000), 30..=30);
    let late = tokens(&wait(&mut world, 800), 30..=30);
    let want = (true, vec!["-".to_string()], vec!["-".to_string()], true);
    check(
        fake == want && unknown == ["-"] && early == ["-"] && late == ["1"],
        format!(
            "fake (hand, impact at 0.3 s, at 1.8 s, unpaid) {fake:?}; unknown {unknown:?}; \
             cheat at 1 s {early:?}, at 1.8 s {late:?}"
        ),
    )
}

/// The player's casts from the power list (`action::Action::Cast`, as
/// bytes and back): one it does not know does nothing; one it knows runs
/// its impact after its cast time (not before) and is paid for.
fn player_casts() -> Result<String, String> {
    let (mut world, player, ..) = world(soldier(&[]))?;
    let cast = crate::action::Action::Cast {
        row: CAST_ROW,
        target: player.0,
    };
    let bytes = crate::action::Action::from_bytes(&cast.to_bytes()) == Ok(cast.clone());
    let press = |world: &mut SimWorld| {
        let mut input = TickInput::default();
        input.insert(Seat(0), crate::input::Command::default());
        input.act(Seat(0), cast.clone());
        let tick = world.tick() + 1;
        crate::step(world, tick, &input, MSEC, StepReason::Authority);
    };
    alter(&mut world, player, |f| f.fp = 100 * 100);
    press(&mut world);
    let unknown = tokens(&wait(&mut world, 2000), 30..=30);
    for (id, mut sheet) in world
        .world
        .query::<(&Id, &mut super::PlayerSheet)>()
        .iter_mut(&mut world.world)
    {
        if id.0 == player {
            sheet.0.powers.insert(CAST_ROW);
        }
    }
    press(&mut world);
    let early = tokens(&wait(&mut world, 1000), 30..=30);
    let before = fighter(&mut world, player).map_or(0, |f| f.fp);
    let late = tokens(&wait(&mut world, 800), 30..=30);
    // Its 20 FP paid as it lands (less the little regained meanwhile).
    let paid = before - fighter(&mut world, player).map_or(0, |f| f.fp);
    check(
        bytes && unknown == ["-"] && early == ["-"] && late == ["1"] && paid > 15 * 100,
        format!(
            "bytes {bytes}; unknown {unknown:?}; known at 1 s {early:?}, at 1.8 s {late:?}; \
             paid {paid} hundredths of FP"
        ),
    )
}

/// The target waits 1 s, then surrenders: still hostile at 0.5 s, neutral
/// at 1.5 s.
fn surrender_queued() -> Result<String, String> {
    use crate::combat::sheet::factions::NEUTRAL;
    let (mut world, _, target, _) = world(soldier(&[]))?;
    script(&mut world, |state| {
        state.queue(ObjectId(target.0), Action::Wait { ms: 1000 });
        state.queue(ObjectId(target.0), Action::Surrender);
    });
    wait(&mut world, 500);
    let early = fighter(&mut world, target).and_then(|f| f.faction);
    wait(&mut world, 1000);
    let late = fighter(&mut world, target).and_then(|f| f.faction);
    check(
        early.is_none() && late == Some(NEUTRAL),
        format!("faction at 0.5 s {early:?}, at 1.5 s {late:?}"),
    )
}

/// The target, with stealth ranks and a stealth unit, uses Stealth on
/// itself: in stealth after.
fn use_skill_stealth() -> Result<String, String> {
    let (mut world, _, target, _) = world(soldier(&[]))?;
    let before = fighter(&mut world, target).is_some_and(|f| f.covert.stealth);
    script(&mut world, |state| {
        state
            .pending
            .push(Pending::new("probe_stealth", ObjectId(target.0)));
    });
    wait(&mut world, 300);
    let after = fighter(&mut world, target).is_some_and(|f| f.covert.stealth);
    check(
        !before && after,
        format!("in stealth before {before}, after {after}"),
    )
}

/// An effect that lasts applied at once (`DURATION_TYPE_INSTANT`: Jolee's
/// assured hit on Kashyyyk, HK-50's sleep on Telos) stays as a permanent
/// one, as both executables keep any effect their own apply does not
/// discard and expire only a temporary one: the target's attacks are
/// assured at once and still 60 s on.
fn lasting_at_once_stays() -> Result<String, String> {
    let (mut world, _, target, _) = world(soldier(&[]))?;
    let assured = |world: &mut SimWorld| {
        let now = world.snapshot().time_ms;
        fighter(world, target).is_some_and(|f| f.effects.has(Modifies::AssuredHit, now))
    };
    let before = assured(&mut world);
    script(&mut world, |state| {
        state
            .pending
            .push(Pending::new("probe_assure_now", crate::script::MODULE));
    });
    wait(&mut world, 300);
    let at_once = assured(&mut world);
    wait(&mut world, 60_000);
    let later = assured(&mut world);
    let gaps = world.script_report().gaps.clone();
    check(
        !before && at_once && later && gaps.is_empty(),
        format!("assured before {before}, after {at_once}, 60 s on {later}; gaps {gaps:?}"),
    )
}
