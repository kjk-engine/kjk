//! The probes of `DestroyObject` (`kjk sim probes destroys`), on the
//! proving ground, no install. As K1's executable has it, a destroyed
//! object leaves the world at its `fDelay` and the view fades the model it
//! still holds (`Snapshot::departing`).
//!
//! The main run destroys, each by a script as K1's `nwscript.nss` lets one
//! ask: a creature with `bNoFade` (gone at once, never drawn fading); one
//! with `fDelay` 0.5 s and `fDelayUntilFade` 1 s (in the world for 0.5 s,
//! then drawn whole another second, then fading out over the engine's
//! second); one with the defaults (out at once, fading over a second), and
//! a placeable likewise; one destroyed twice in one step, at once and
//! after 10 s (the sooner kept); a container (opening it then is refused,
//! while one kept opens). A script asking `GetIsObjectValid(
//! GetObjectByTag(…))` sees the delayed one before its delay and not
//! after, while it is still drawn; one destroying the fading one again
//! changes nothing (its fade never goes back). A snapshot taken mid-fade
//! round-trips, the delayed one in it carrying how it will depart, and a
//! world loaded from it is the same.
//!
//! The fight run: a beast fighting a dummy, destroyed with a fade once it
//! has struck, strikes no more and is no one's target. The walk run: the
//! player walking into a creature is stopped by it, and walks through
//! where it stood once it is destroyed, while it is still drawn fading.
//! Each run twice ends the same.

use super::{Id, Object, SimWorld};
use crate::action::Action;
use crate::combat::Sheet;
use crate::combat::sheet::factions::{FRIENDLY_1, HOSTILE_1};
use crate::input::{Command, Seat};
use crate::module::{ObjectData, ObjectKind, Placement, Repute};
use crate::notice::Notice;
use crate::probe::{Checks, check};
use crate::script::Pending;
use crate::snapshot::{Kind, SimId, Snapshot};
use crate::{Level, StepReason, TickInput};
use fmt_odyssey::ncs::{Const, Op};
use script_ncs::probes::Asm;
use script_ncs::vm::ObjectId;

/// The steps' length (ms), the step the destroys run in, and how many run.
const MSEC: u32 = 8;
const DESTROY_STEP: u64 = 4;
const STEPS: u64 = 400;
/// The steps (after the destroys') the delayed one is looked for, the
/// fading one destroyed again, the containers opened, and the snapshot
/// taken mid-fade.
const SEEN_BEFORE: u64 = 30;
const SEEN_AFTER: u64 = 90;
const AGAIN: u64 = 40;
const OPEN: u64 = 2;
const MID_FADE: u64 = 60;

/// The destroyed, by tag.
const NO_FADE: &str = "no_fade";
const DELAYED: &str = "delayed";
const PLAIN: &str = "plain";
const PROP: &str = "prop";
const TWICE: &str = "twice";
const GONE_BOX: &str = "gone_box";
/// The container left standing.
const KEPT_BOX: &str = "kept_box";

/// The routines the probe's scripts call.
const CATALOG: &str = r#"
// 0
void DestroyObject(object oDestroy, float fDelay=0.0f, int bNoFade = FALSE, float fDelayUntilFade = 0.0f);
// 1
object GetObjectByTag(string sTag, int nNth=0);
// 2
int GetIsObjectValid(object oObject);
// 3
void SetGlobalNumber(string sIdentifier, int nValue);
// 4
void ActionAttack(object oAttackee, int bPassive = FALSE);
"#;

/// The global the looking script sets.
const SEEN: &str = "probe_seen";

pub fn probes() -> Checks {
    let (ran, fought, walked) = match (scenario(), fight(), walk(true)) {
        (Ok(ran), Ok(fought), Ok(walked)) => (ran, fought, walked),
        (Err(why), ..) | (_, Err(why), _) | (.., Err(why)) => {
            return vec![("destroys_scenario", Err(why))];
        }
    };
    let blocked = walk(false);
    let again = [
        scenario().map(|again| again.last.hash()),
        fight().map(|again| again.last.hash()),
        walk(true).map(|again| again.last.hash()),
    ];
    let at = ran.destroyed_ms;
    // When each was last drawn, and last in the world, after the destroys
    // (ms; none: never), and its alpha `ms` after them (none: not drawn).
    let until = |tag: &str, in_world: bool| {
        ran.steps
            .iter()
            .rfind(|step| {
                step.of(tag)
                    .is_some_and(|drawn| drawn.in_world || !in_world)
            })
            .map(|step| step.ms - at)
    };
    let alpha = |tag: &str, ms: u64| {
        ran.steps
            .iter()
            .find(|step| step.ms - at >= ms)
            .and_then(|step| step.of(tag))
            .map(|drawn| drawn.alpha)
    };
    let alphas = |tag: &str| -> Vec<f32> {
        ran.steps
            .iter()
            .filter_map(|step| step.of(tag))
            .map(|drawn| drawn.alpha)
            .collect()
    };
    let near = |got: Option<u64>, want: u64| {
        got.is_some_and(|got| got + 2 * u64::from(MSEC) >= want && got <= want)
    };
    let about = |got: Option<f32>, want: f32| got.is_some_and(|got| (got - want).abs() < 0.03);
    let out_at_once = [NO_FADE, PLAIN, PROP, TWICE]
        .iter()
        .all(|tag| until(tag, true) == Some(0));
    vec![
        (
            "no_fade_gone_at_once",
            check(
                until(NO_FADE, false).is_none_or(|ms| ms <= u64::from(MSEC))
                    && alphas(NO_FADE).iter().all(|alpha| *alpha == 1.0),
                format!(
                    "last drawn {:?} ms after the destroy, alphas {:?}",
                    until(NO_FADE, false),
                    alphas(NO_FADE)
                ),
            ),
        ),
        (
            "out_of_the_world_at_its_delay",
            check(
                out_at_once && near(until(DELAYED, true), 500),
                format!(
                    "the undelayed out at once {out_at_once}, the delayed in the world until \
                     {:?} ms",
                    until(DELAYED, true)
                ),
            ),
        ),
        (
            "default_fades_a_second",
            check(
                near(until(PLAIN, false), 1000)
                    && about(alpha(PLAIN, 0), 1.0)
                    && about(alpha(PLAIN, 500), 0.5),
                format!(
                    "drawn until {:?} ms, alpha {:?} at once, {:?} at 0.5 s",
                    until(PLAIN, false),
                    alpha(PLAIN, 0),
                    alpha(PLAIN, 500)
                ),
            ),
        ),
        (
            "delayed_fade_waits",
            check(
                near(until(DELAYED, false), 2500)
                    && about(alpha(DELAYED, 1400), 1.0)
                    && about(alpha(DELAYED, 2000), 0.5),
                format!(
                    "drawn until {:?} ms, alpha {:?} at 1.4 s, {:?} at 2 s",
                    until(DELAYED, false),
                    alpha(DELAYED, 1400),
                    alpha(DELAYED, 2000)
                ),
            ),
        ),
        (
            "gone_to_scripts_at_its_delay",
            check(
                ran.seen == (Some(1), Some(0)) && ran.drawn_unseen,
                format!(
                    "seen {:?} before its delay and {:?} after, drawn then {}",
                    ran.seen.0, ran.seen.1, ran.drawn_unseen
                ),
            ),
        ),
        (
            "placeable_fades",
            check(
                near(until(PROP, false), 1000) && about(alpha(PROP, 500), 0.5),
                format!(
                    "drawn until {:?} ms, alpha {:?} at 0.5 s",
                    until(PROP, false),
                    alpha(PROP, 500)
                ),
            ),
        ),
        (
            "two_destroys_keep_the_sooner",
            check(
                until(TWICE, true) == Some(0) && near(until(TWICE, false), 1000),
                format!(
                    "destroyed at once then after 10 s in one step: in the world until {:?} \
                     ms, drawn until {:?} ms",
                    until(TWICE, true),
                    until(TWICE, false)
                ),
            ),
        ),
        (
            "fading_destroyed_again_does_not_pop_back",
            check(
                alphas(PLAIN).windows(2).all(|pair| pair[1] <= pair[0]),
                format!(
                    "destroyed again {} ms in: alpha never rose",
                    u64::from(MSEC) * AGAIN
                ),
            ),
        ),
        (
            "destroyed_container_not_opened",
            check(
                ran.opened == (false, true) && ran.refused,
                format!(
                    "opened: the destroyed {}, the kept {}; the destroyed refused {}",
                    ran.opened.0, ran.opened.1, ran.refused
                ),
            ),
        ),
        (
            "destroyed_no_longer_acts_nor_targeted",
            check(
                fought.struck_before
                    && fought.hp_after.0 == fought.hp_after.1
                    && fought.targeted == (true, false),
                format!(
                    "the dummy struck before {}, its hp after the destroy {:?}, the beast its \
                     target before {} and after {}",
                    fought.struck_before, fought.hp_after, fought.targeted.0, fought.targeted.1
                ),
            ),
        ),
        (
            "destroyed_does_not_collide",
            check(
                blocked
                    .as_ref()
                    .is_ok_and(|blocked| blocked.passed.is_none())
                    && walked.passed == Some(true),
                format!(
                    "undestroyed, walked past it {:?}; destroyed, walked past it {:?} (still \
                     drawn then)",
                    blocked.as_ref().map(|blocked| blocked.passed),
                    walked.passed
                ),
            ),
        ),
        (
            "mid_fade_round_trip",
            check(ran.round_trip.is_ok(), format!("{:?}", ran.round_trip)),
        ),
        (
            "same_every_run",
            check(
                again[0].as_ref().is_ok_and(|hash| *hash == ran.last.hash())
                    && again[1]
                        .as_ref()
                        .is_ok_and(|hash| *hash == fought.last.hash())
                    && again[2]
                        .as_ref()
                        .is_ok_and(|hash| *hash == walked.last.hash()),
                format!(
                    "{} {} {} then {:?}",
                    ran.last.hash().short(),
                    fought.last.hash().short(),
                    walked.last.hash().short(),
                    again.map(|hash| hash.map(|hash| hash.short()))
                ),
            ),
        ),
    ]
}

/// One destroyed thing drawn in a step: how opaque, and whether it is in
/// the world yet (else departing).
#[derive(Clone, Copy)]
struct Drawn {
    tag: &'static str,
    alpha: f32,
    in_world: bool,
}

/// A step as the scenario saw it: its time and each destroyed thing still
/// drawn.
struct Step {
    ms: u64,
    drawn: Vec<Drawn>,
}

impl Step {
    fn of(&self, tag: &str) -> Option<Drawn> {
        self.drawn.iter().copied().find(|drawn| drawn.tag == tag)
    }
}

/// What the main run saw: when the destroys ran (sim ms), each step, what
/// the looking script saw before and after the delayed one's delay,
/// whether it was still drawn when it was not seen, whether the destroyed
/// and the kept container opened and the destroyed one's opening was
/// refused, a mid-fade snapshot's round trip, and the last snapshot.
struct Ran {
    destroyed_ms: u64,
    steps: Vec<Step>,
    seen: (Option<i32>, Option<i32>),
    drawn_unseen: bool,
    opened: (bool, bool),
    refused: bool,
    round_trip: Result<(), String>,
    last: Snapshot,
}

/// Pushes `DestroyObject(oDestroy, fDelay, bNoFade, fDelayUntilFade)`,
/// the object pushed by `object`; `None` for the defaults.
fn push_destroy(asm: &mut Asm, object: &[Op], with: Option<(f32, i32, f32)>) {
    let args = match with {
        Some((delay, no_fade, until_fade)) => {
            asm.op(Op::Const(Const::Float(until_fade)))
                .op(Op::Const(Const::Int(no_fade)))
                .op(Op::Const(Const::Float(delay)));
            4
        }
        None => 1,
    };
    for op in object {
        asm.op(op.clone());
    }
    asm.op(Op::Action { routine: 0, args });
}

/// A script of the destroys `withs`, in order, of the object `object`
/// pushes.
fn destroys(object: &[Op], withs: &[Option<(f32, i32, f32)>]) -> fmt_odyssey::ncs::Ncs {
    let mut asm = Asm::default();
    for with in withs {
        push_destroy(&mut asm, object, *with);
    }
    asm.op(Op::Retn);
    asm.build()
}

/// A string constant.
fn text(value: &str) -> Op {
    Op::Const(Const::String(value.as_bytes().to_vec()))
}

/// `GetObjectByTag(tag)`, as pushed.
fn by_tag(tag: &str) -> Vec<Op> {
    vec![
        Op::Const(Const::Int(0)),
        text(tag),
        Op::Action {
            routine: 1,
            args: 2,
        },
    ]
}

/// `ActionAttack(GetObjectByTag(tag))`.
fn attack(tag: &str) -> fmt_odyssey::ncs::Ncs {
    let mut asm = Asm::default();
    asm.op(Op::Const(Const::Int(0)));
    for op in by_tag(tag) {
        asm.op(op);
    }
    asm.op(Op::Action {
        routine: 4,
        args: 2,
    })
    .op(Op::Retn);
    asm.build()
}

/// The probe's scripts: each destroy, the look for the delayed one
/// (`SetGlobalNumber(SEEN, GetIsObjectValid(GetObjectByTag(DELAYED)))`),
/// and the fight's attacks.
fn scripts() -> crate::script::Library {
    let me = [Op::Const(Const::Object(0))];
    let mut look = Asm::default();
    for op in by_tag(DELAYED) {
        look.op(op);
    }
    look.op(Op::Action {
        routine: 2,
        args: 1,
    })
    .op(text(SEEN))
    .op(Op::Action {
        routine: 3,
        args: 2,
    })
    .op(Op::Retn);
    crate::probe::library(
        CATALOG,
        [
            ("probe_no_fade", destroys(&me, &[Some((0.0, 1, 0.0))])),
            ("probe_delayed", destroys(&me, &[Some((0.5, 0, 1.0))])),
            ("probe_plain", destroys(&me, &[None])),
            ("probe_prop", destroys(&by_tag(PROP), &[None])),
            ("probe_box", destroys(&by_tag(GONE_BOX), &[None])),
            ("probe_again", destroys(&by_tag(PLAIN), &[None])),
            ("probe_twice", destroys(&me, &[None, Some((10.0, 0, 0.0))])),
            ("probe_look", look.build()),
            ("probe_attack_dummy", attack(DUMMY)),
            ("probe_attack_beast", attack(BEAST)),
        ],
    )
}

/// The proving ground with `placed` added (the first's index returned),
/// `HOSTILE_1` and `FRIENDLY_1` at war, the probe's scripts; a world on
/// it, one seat.
fn world_with(placed: Vec<ObjectData>) -> Result<(SimWorld, Level, u32), String> {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    let first = data.placements.len() as u32;
    data.placements
        .extend(placed.into_iter().map(Placement::Own));
    data.repute = Repute::at_war([HOSTILE_1, FRIENDLY_1]);
    let level = ground.with_module(data).with_scripts(scripts());
    let world = SimWorld::new(
        crate::WorldConfig {
            seed: 1,
            seats: 1,
            wanderers: 0,
            spawn: [0.0; 3],
            yaw: 0.0,
            sheet: Sheet::dev_soldier(),
            guests: crate::Guests::Avatars,
        },
        level.clone(),
    )
    .map_err(|error| error.to_string())?;
    Ok((world, level, first))
}

/// The proving ground's entry (the player's spot), moved by `dx`, `dy`.
fn by_entry(dx: f32, dy: f32) -> [f32; 3] {
    let [x, y, z] = crate::determinism::proving_ground()
        .module()
        .map_or([0.0; 3], |module| module.entry.position);
    [x + dx, y + dy, z]
}

/// The ids of the objects placed `first` on, `count` of them.
fn ids(world: &mut SimWorld, first: u32, count: usize) -> Result<Vec<u32>, String> {
    (first..first + count as u32)
        .map(|index| {
            world
                .world
                .query::<(&Id, &Object)>()
                .iter(&world.world)
                .find(|(_, object)| object.index == index)
                .map(|(id, _)| id.0.0)
                .ok_or(format!("object {index} not placed"))
        })
        .collect()
}

/// The player's id.
fn player(world: &mut SimWorld) -> Result<u32, String> {
    world
        .snapshot()
        .entities
        .iter()
        .find(|entity| matches!(entity.kind, Kind::Avatar(_)))
        .map(|entity| entity.id.0)
        .ok_or("no player".into())
}

/// Runs `script` in `world`'s next step, called by `caller`.
fn run(world: &mut SimWorld, script: &str, caller: u32) {
    let mut state = world.script_state().clone();
    state.pending.push(Pending::new(script, ObjectId(caller)));
    world.set_script_state(state);
}

/// Steps `world` to `tick` with `input`.
fn step(world: &mut SimWorld, tick: u64, input: &TickInput) -> Snapshot {
    crate::step(world, tick, input, MSEC, StepReason::Authority)
}

/// The object placed `index`th in `snapshot`, as drawn: whether in the
/// world, and how opaque.
fn drawn_as(snapshot: &Snapshot, index: u32) -> Option<(bool, f32)> {
    let placed = |kind: &Kind| matches!(kind, Kind::Object(_, at) if *at == index);
    if snapshot.entities.iter().any(|entity| placed(&entity.kind)) {
        return Some((true, 1.0));
    }
    snapshot
        .departing
        .iter()
        .find(|going| placed(&going.entity.kind))
        .map(|going| (false, going.alpha()))
}

fn scenario() -> Result<Ran, String> {
    let creature =
        |tag: &str, dx, dy| ObjectData::creature(tag, tag, by_entry(dx, dy), Sheet::commoner());
    let tags = [NO_FADE, DELAYED, PLAIN, PROP, TWICE, GONE_BOX, KEPT_BOX];
    let (mut world, level, first) = world_with(vec![
        creature(NO_FADE, 2.0, 0.0),
        creature(DELAYED, 2.0, 2.0),
        creature(PLAIN, 2.0, -2.0),
        ObjectData {
            kind: ObjectKind::Placeable,
            tag: PROP.into(),
            ..ObjectData::unknown(PROP, by_entry(-2.0, 0.0), 0.0)
        },
        creature(TWICE, -2.0, 2.0),
        super::containers::container(GONE_BOX, by_entry(0.0, 1.0), &[]),
        super::containers::container(KEPT_BOX, by_entry(0.0, -1.0), &[]),
    ])?;
    let ids = ids(&mut world, first, tags.len())?;
    let player = player(&mut world)?;
    let drawn = |snapshot: &Snapshot| -> Vec<Drawn> {
        tags.iter()
            .zip(first..)
            .filter_map(|(&tag, index)| {
                let (in_world, alpha) = drawn_as(snapshot, index)?;
                Some(Drawn {
                    tag,
                    alpha,
                    in_world,
                })
            })
            .collect()
    };
    let open = |snapshot: &Snapshot, index: u32| {
        snapshot.entities.iter().any(|entity| {
            entity.kind == Kind::Object(ObjectKind::Placeable, index)
                && entity.object.is_some_and(|object| object.tenants != 0)
        })
    };
    let boxes = (first + 5, first + 6);
    let mut ran = Ran {
        destroyed_ms: 0,
        steps: Vec::new(),
        seen: (None, None),
        drawn_unseen: false,
        opened: (false, false),
        refused: false,
        round_trip: Err("no snapshot mid-fade".into()),
        last: world.snapshot(),
    };
    for tick in 1..=STEPS {
        let after = tick.checked_sub(DESTROY_STEP);
        if after == Some(0) {
            for (script, caller) in [
                ("probe_no_fade", ids[0]),
                ("probe_delayed", ids[1]),
                ("probe_plain", ids[2]),
                ("probe_prop", player),
                ("probe_twice", ids[4]),
                ("probe_box", player),
            ] {
                run(&mut world, script, caller);
            }
        }
        if after == Some(AGAIN) {
            run(&mut world, "probe_again", player);
        }
        let looking = after == Some(SEEN_BEFORE) || after == Some(SEEN_AFTER);
        if looking {
            run(&mut world, "probe_look", player);
        }
        let mut input = TickInput::default();
        if after == Some(OPEN) {
            input.act(Seat(0), Action::Open(boxes.0));
            input.act(Seat(0), Action::Open(boxes.1));
        }
        let snapshot = step(&mut world, tick, &input);
        if after == Some(0) {
            ran.destroyed_ms = snapshot.time_ms;
        }
        if after == Some(OPEN) {
            ran.opened = (open(&snapshot, boxes.0), open(&snapshot, boxes.1));
            ran.refused = world
                .take_notices()
                .iter()
                .any(|(_, notice)| *notice == Notice::NotOpen);
        }
        if looking {
            let number = crate::script::global(&world.script_state().numbers, SEEN).copied();
            if after == Some(SEEN_BEFORE) {
                ran.seen.0 = number;
            } else {
                ran.seen.1 = number;
                ran.drawn_unseen = drawn(&snapshot).iter().any(|drawn| drawn.tag == DELAYED);
            }
        }
        if after == Some(MID_FADE) {
            ran.round_trip = round_trip(&snapshot, &level, SimId(ids[1]));
        }
        if after.is_some() {
            ran.steps.push(Step {
                ms: snapshot.time_ms,
                drawn: drawn(&snapshot),
            });
        }
        ran.last = snapshot;
    }
    Ok(ran)
}

/// `snapshot`, taken mid-fade, decoded back the same, its departing four
/// (the plain one, the placeable, the one destroyed twice, the destroyed
/// container), the delayed one (`delayed`) still in the world with how it
/// will depart, and a world loaded from it the same.
fn round_trip(snapshot: &Snapshot, level: &Level, delayed: SimId) -> Result<(), String> {
    let back = Snapshot::decode(&snapshot.encode()).map_err(|error| error.to_string())?;
    let loaded = SimWorld::from_snapshot(&back, level.clone())
        .map_err(|error| error.to_string())?
        .snapshot();
    let departs = back.get(delayed).and_then(|entity| entity.going);
    if back == *snapshot && back.departing.len() == 4 && departs.is_some() && loaded == back {
        Ok(())
    } else {
        Err(format!(
            "back the same {}, {} departing, the delayed one to depart {departs:?}, loaded the \
             same {}",
            back == *snapshot,
            back.departing.len(),
            loaded == back
        ))
    }
}

/// The fight's two.
const BEAST: &str = "beast";
const DUMMY: &str = "dummy";

/// How long the fight run runs at most (steps), and after the destroy.
const FIGHT_STEPS: u64 = 2000;
const FIGHT_AFTER: u64 = 300;

/// What the fight run saw: whether the dummy was struck before the beast
/// was destroyed, the dummy's hp the step after the destroy and at the
/// end, whether the beast was its target before and after, the last
/// snapshot.
struct Fought {
    struck_before: bool,
    hp_after: (i32, i32),
    targeted: (bool, bool),
    last: Snapshot,
}

/// A beast and a dummy at war attacking each other, the beast destroyed
/// (the defaults: fading a second) the step after it first strikes the
/// dummy.
fn fight() -> Result<Fought, String> {
    let at = |x| crate::determinism::ground_point(x, 3.0, 0.0);
    let (mut world, _, first) = world_with(vec![
        ObjectData::creature(
            BEAST,
            BEAST,
            at(-4.5),
            Sheet {
                abilities: [16, 10, 10, 4, 6, 5],
                faction: HOSTILE_1,
                ..Sheet::commoner()
            },
        ),
        ObjectData::creature(
            DUMMY,
            DUMMY,
            at(-2.5),
            Sheet {
                faction: FRIENDLY_1,
                max_hp: 500,
                ..Sheet::commoner()
            },
        ),
    ])?;
    let ids = ids(&mut world, first, 2)?;
    let beast = SimId(ids[0]);
    let dummy = |snapshot: &Snapshot| {
        snapshot
            .get(SimId(ids[1]))
            .and_then(|entity| entity.fighter)
            .ok_or("the dummy has no fight".to_string())
    };
    let mut hp = dummy(&world.snapshot())?.hp;
    let mut fought = Fought {
        struck_before: false,
        hp_after: (0, 0),
        targeted: (false, false),
        last: world.snapshot(),
    };
    run(&mut world, "probe_attack_dummy", beast.0);
    run(&mut world, "probe_attack_beast", ids[1]);
    let mut destroyed = None;
    for tick in 1..=FIGHT_STEPS {
        let snapshot = step(&mut world, tick, &TickInput::default());
        let now = dummy(&snapshot)?;
        match destroyed {
            None => {
                fought.targeted.0 |= now.target == Some(beast);
                if now.hp < hp {
                    fought.struck_before = true;
                    run(&mut world, "probe_plain", beast.0);
                    destroyed = Some(tick);
                }
            }
            // Its destroy runs in the next step's scripts and it leaves the
            // world as the step after ends (`expire`): from then on.
            Some(at) => {
                if tick == at + 2 {
                    fought.hp_after.0 = now.hp;
                }
                fought.hp_after.1 = now.hp;
                fought.targeted.1 |= tick > at + 2 && now.target == Some(beast);
                if tick == at + FIGHT_AFTER {
                    fought.last = snapshot;
                    break;
                }
            }
        }
        hp = now.hp;
        fought.last = snapshot;
    }
    Ok(fought)
}

/// How long the walk runs (steps), and the step it starts in.
const WALK_STEPS: u64 = 150;
const WALK_FROM: u64 = DESTROY_STEP + 2;
/// How far ahead of the player the creature stands (m), and past it the
/// player is counted through.
const BLOCKER_AHEAD: f32 = 1.2;
const THROUGH: f32 = 0.5;

/// What the walk saw: whether the player got past where the creature
/// stood, and whether the creature was still drawn then; the last
/// snapshot.
struct Walked {
    passed: Option<bool>,
    last: Snapshot,
}

/// The player walking along the world's x into a creature standing ahead
/// of it, destroyed (the defaults: fading a second) as it starts if
/// `destroyed`.
fn walk(destroyed: bool) -> Result<Walked, String> {
    let ahead = by_entry(BLOCKER_AHEAD, 0.0);
    let (mut world, _, first) = world_with(vec![ObjectData::creature(
        "blocker",
        "blocker",
        ahead,
        Sheet::commoner(),
    )])?;
    let blocker = ids(&mut world, first, 1)?[0];
    // Its view turned to the world's x (a command's angles add to its
    // delta angles).
    let delta = world
        .snapshot()
        .entities
        .iter()
        .find_map(|entity| {
            entity
                .player
                .filter(|_| matches!(entity.kind, Kind::Avatar(_)))
        })
        .ok_or("no player")?
        .view_offset;
    let mut walking = TickInput::default();
    walking.insert(
        Seat(0),
        Command {
            angles: [0, 0i16.wrapping_sub(delta[1]), 0],
            forward: 127,
            ..Command::default()
        },
    );
    let mut walked = Walked {
        passed: None,
        last: world.snapshot(),
    };
    for tick in 1..=WALK_STEPS {
        if destroyed && tick == DESTROY_STEP {
            run(&mut world, "probe_plain", blocker);
        }
        let input = if tick >= WALK_FROM {
            walking.clone()
        } else {
            TickInput::default()
        };
        let snapshot = step(&mut world, tick, &input);
        let x = snapshot
            .entities
            .iter()
            .find(|entity| matches!(entity.kind, Kind::Avatar(_)))
            .map_or(f32::MIN, |entity| entity.position[0]);
        if walked.passed.is_none() && x > ahead[0] + THROUGH {
            walked.passed = Some(drawn_as(&snapshot, first).is_some());
        }
        walked.last = snapshot;
    }
    Ok(walked)
}
