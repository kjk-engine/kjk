//! KOTOR's minigames' routines (`SWMG_*`: the swoop race, the turret;
//! `crate::minigame`). Every routine whose name starts with `SWMG_` comes
//! here first (`engine::Engine`'s `Host`); one answered here is bound,
//! `None` leaves it a gap. They read the course as the step's scripts left
//! it so far and change a copy of it (`Out::minigame`), which the world
//! keeps after them; the hooks a change raises (a death) queue as events.
//!
//! The things are numbered: 0 the player, the enemies from 1, then the
//! obstacles; each is the object `minigame::id(run, n)`. A co-op race's
//! scripts each run as their own run's thing and see that run (M10): its
//! player, its pads, its globals (`overlaid`, `overlay`), its finish
//! (`leave`); a script that is no thing's sees the first seat's.
//! Meanings from the install's `nwscript.nss` and the games' own use of
//! them (K1 calls 25 of the 105, docs/MINIGAMES.md).

use std::collections::BTreeMap;

use script_ncs::catalog::Routine;
use script_ncs::vm::{ObjectId, Value};

use super::engine::{Engine, int, object, string, text, truth};
use super::state::{global, set_global};
use crate::minigame::{self, Anim, Course, Fact, Leaving, Overlay, Raised, Rules, Voice};

fn float(args: &[Value], at: usize) -> f32 {
    match args.get(at) {
        Some(Value::Float(v)) => *v,
        _ => 0.0,
    }
}

fn vector(args: &[Value], at: usize) -> [f32; 3] {
    match args.get(at) {
        Some(Value::Vector(v)) => *v,
        _ => [0.0; 3],
    }
}

/// Thing `n` of run `run` as an object.
fn id(run: usize, n: usize) -> Value {
    Value::Object(ObjectId(minigame::id(run, n)))
}

/// Thing `n`'s object, if any, else none.
fn id_of(span: usize, n: Option<u16>) -> Value {
    n.map_or(Value::Object(ObjectId::INVALID), |n| {
        id(span, usize::from(n))
    })
}

/// The run `caller`'s scripts see: its own, a thing's; else the first.
fn run_of(caller: ObjectId) -> usize {
    minigame::run(caller.0).unwrap_or(0)
}

/// Run `run` of the course as the scripts have it so far, and its rules.
fn read<'e>(engine: &'e Engine, run: usize) -> Option<(&'e Course, &'e Rules)> {
    let (course, rules) = engine.view.minigame?;
    let course = engine.out.minigame.as_ref().unwrap_or(course);
    (course.fits(rules))
        .then(|| course.run(run))
        .flatten()
        .map(|course| (course, rules))
}

/// Run `run` of the course to change (copied from the step's on the first
/// change).
fn write<'e>(engine: &'e mut Engine, run: usize) -> Option<(&'e mut Course, &'e Rules)> {
    let (course, rules) = engine.view.minigame?;
    let course = engine.out.minigame.get_or_insert_with(|| course.clone());
    if !course.fits(rules) {
        return None;
    }
    Some((course.run_mut(run)?, rules))
}

/// Queues the hooks a change in span `span` raised.
fn queue(engine: &mut Engine, span: usize, raised: Vec<Raised>) {
    let Some((_, rules)) = engine.view.minigame else {
        return;
    };
    let queued = raised.into_iter().filter_map(|raised| {
        minigame::pending(
            rules,
            Raised {
                span: span as u8,
                ..raised
            },
        )
    });
    engine.state.pending.extend(queued);
}

/// `routine` called by `caller` with `args`: its value, or `None` for a
/// routine the minigames do not answer.
pub(super) fn call(
    engine: &mut Engine,
    routine: &Routine,
    caller: ObjectId,
    args: &[Value],
) -> Option<Value> {
    let name = routine.name.strip_prefix("SWMG_")?;
    // The thing an object argument names, else the caller.
    let thing = |at: usize| minigame::thing(object(args, at).0);
    let me = minigame::thing(caller.0);
    let run = run_of(caller);
    // A gunner's lens (M11): its ids its own, its aim and clips the
    // gunner's.
    let span = minigame::span(caller.0).unwrap_or(run);
    let lens = minigame::gunner(caller.0);
    let fact_number = engine.event.number;
    if let Some(value) = read_only(engine, (run, span, lens), name, args, thing(0), fact_number) {
        return Some(value);
    }
    // Through a lens only the gunner's own clips change: the hull's speed,
    // hit points and the rest are the player's heartbeat's, run already.
    if lens.is_some() && !matches!(name, "PlayAnimation" | "RemoveAnimation") {
        return Some(Value::Int(0));
    }
    let mut raised = Vec::new();
    let dexterity = dexterity_modifier(engine);
    let mut value = Value::Int(0);
    {
        let (course, rules) = write(engine, run)?;
        let now = course.time_ms;
        // K1's executable refuses a negative minimum speed, maximum speed
        // and acceleration; TSL's only a negative maximum.
        let refused = |v: f32, by_both: bool| v < 0.0 && (by_both || !rules.tsl);
        let player = &mut course.movers[0];
        match name {
            "SetPlayerSpeed" => player.speed = float(args, 0),
            "SetPlayerMinSpeed" if !refused(float(args, 0), false) => {
                player.min_speed = float(args, 0)
            }
            "SetPlayerMaxSpeed" if !refused(float(args, 0), true) => {
                player.max_speed = float(args, 0)
            }
            "SetPlayerAccelerationPerSecond" if !refused(float(args, 0), false) => {
                player.accel = float(args, 0)
            }
            "SetPlayerMinSpeed" | "SetPlayerMaxSpeed" | "SetPlayerAccelerationPerSecond" => {}
            "SetPlayerTunnelPos" => player.tunnel_pos = vector(args, 0),
            "SetPlayerTunnelNeg" => player.tunnel_neg = vector(args, 0),
            "SetPlayerOffset" => player.offset = vector(args, 0),
            "StartInvulnerability" => {
                let period = thing(0)
                    .and_then(|n| rules.follower(n))
                    .map(|f| f.invince_ms);
                if let (Some(mover), Some(period)) =
                    (thing(0).and_then(|n| course.movers.get_mut(n)), period)
                {
                    mover.invulnerable_until = now + u64::from(period);
                }
            }
            // TSL's executable scales it by the player's Dexterity
            // modifier, three tenths a point; K1's keeps it as given.
            "SetLateralAccelerationPerSecond" if rules.tsl => {
                let given = float(args, 0);
                course.lateral_accel = given + dexterity as f32 * given * LATERAL_PER_DEX;
            }
            "SetLateralAccelerationPerSecond" => course.lateral_accel = float(args, 0),
            "SetJumpSpeed" => course.jump = float(args, 0),
            "PlayerApplyForce" => course.apply_force(vector(args, 0)),
            "DestroyMiniGameObject" => {
                if let Some(n) = thing(0) {
                    course.destroy(rules, n);
                }
            }
            "SetFollowerPosition" => {
                course.restart();
                // Declared a vector, it gives none back in the executable.
                value = Value::Vector([0.0; 3]);
            }
            "SetSpeedBlurEffect" => {
                course.blur = if int(args, 0) != 0 {
                    float(args, 1)
                } else {
                    0.0
                }
            }
            "SetCameraClip" => course.clip = [float(args, 0), float(args, 1)],
            "PlayAnimation" => {
                let anim = Anim {
                    name: text(args, 1).to_string(),
                    looping: int(args, 2) != 0,
                    overlay: int(args, 4) != 0,
                    since: now,
                };
                match (thing(0), lens.and_then(|g| course.gunners.get_mut(g - 1))) {
                    (Some(0), Some(gunner)) => {
                        minigame::play_into(rules, 0, &mut gunner.anims, anim);
                    }
                    (Some(n), _) => course.play(rules, n, anim),
                    (None, _) => {}
                }
            }
            "RemoveAnimation" => {
                let name = text(args, 1);
                match (thing(0), lens.and_then(|g| course.gunners.get_mut(g - 1))) {
                    (Some(0), Some(gunner)) => gunner
                        .anims
                        .retain(|playing| !playing.name.eq_ignore_ascii_case(name)),
                    (Some(n), _) => course.stop(rules, n, name),
                    (None, _) => {}
                }
            }
            "SetFollowerHitPoints" => {
                if let Some(n) = thing(0) {
                    course.set_hp(n, int(args, 1));
                }
            }
            // Both executables read no third argument: always a change,
            // heard through the follower's damage hook (QUIRKS.md Q-0258).
            "AdjustFollowerHitPoints" => {
                if let Some(n) = thing(0).filter(|n| *n < course.movers.len()) {
                    course.adjust_hp(rules, n, int(args, 1), &mut raised);
                }
            }
            "SetMaxHitPoints" => {
                if let Some(mover) = thing(0).and_then(|n| course.movers.get_mut(n)) {
                    mover.max_hp = int(args, 1);
                }
            }
            "SetSphereRadius" => {
                if let Some(mover) = thing(0).and_then(|n| course.movers.get_mut(n)) {
                    mover.radius = float(args, 1);
                }
            }
            "SetNumLoops" => {
                if let Some(mover) = thing(0).and_then(|n| course.movers.get_mut(n)) {
                    mover.loops = int(args, 1);
                }
            }
            // The default handlers, for the hook's own thing and hit.
            "OnDamage" => {
                if let Some(n) = me {
                    course.on_damage(rules, n, fact_number, &mut raised);
                }
            }
            "OnBulletHit" => {
                if let Some(n) = me {
                    course.damage(rules, n, fact_number, &mut raised);
                }
            }
            "OnDeath" => {
                if let Some(n) = me {
                    course.on_death(n);
                }
            }
            "OnObstacleHit" => {}
            _ => return None,
        }
    }
    queue(engine, span, raised);
    Some(value)
}

/// How much of a TSL `SWMG_SetLateralAccelerationPerSecond` each point of
/// the player's Dexterity modifier adds (its executable's constant).
const LATERAL_PER_DEX: f32 = rules::minigames::swoop::LATERAL_PER_DEX;

/// The player's Dexterity modifier, as `SWMG_SetLateralAccelerationPerSecond`
/// reads it: the first PC's.
fn dexterity_modifier(engine: &Engine) -> i32 {
    let first = engine.view.first_pc(&engine.state.party);
    engine
        .view
        .thing(first)
        .and_then(|thing| thing.character)
        .map_or(0, |c| (i32::from(c.sheet.abilities[1]) - 10).div_euclid(2))
}

/// What both executables' handlers for these do with TSL's and K1's own
/// prototypes: they take an object first, which the call never pushed, so
/// the typed stack refuses it and the script stops there (`mine.ncs`
/// calls `SWMG_SetPlayerInvincibility` just before
/// `SWMG_StartInvulnerability`, which never runs). QUIRKS.md Q-0257.
pub(super) fn aborts(name: &str) -> bool {
    matches!(
        name,
        "SWMG_SetPlayerInvincibility" | "SWMG_GetPlayerInvincibility"
    )
}

/// The host's error a script stops on at a routine that `aborts`: no fault.
pub(super) const ABORTED: &str = "the executable stops the script here";

/// The routines that only read the course.
fn read_only(
    engine: &Engine,
    (run, span, lens): (usize, usize, Option<usize>),
    name: &str,
    args: &[Value],
    thing: Option<usize>,
    fact_number: i32,
) -> Option<Value> {
    let (course, rules) = read(engine, run)?;
    let id = |n: usize| id(span, n);
    let id_of = |n: Option<u16>| id_of(span, n);
    let aim = lens
        .and_then(|g| course.gunners.get(g - 1))
        .map_or(course.movers[0].offset, |gunner| gunner.offset);
    let player = &course.movers[0];
    let mover = thing.and_then(|n| course.movers.get(n));
    let fact: Fact = course.fact_of(fact_number);
    let followers = rules.followers();
    Some(match name {
        "GetPlayer" => id(0),
        "GetEnemyCount" => Value::Int(rules.enemies.len() as i32),
        "GetEnemy" => {
            let n = int(args, 0);
            match usize::try_from(n).ok().filter(|n| *n < rules.enemies.len()) {
                Some(n) => id(1 + n),
                None => Value::Object(ObjectId::INVALID),
            }
        }
        "GetObstacleCount" => Value::Int(rules.obstacles.len() as i32),
        "GetObstacle" => {
            let n = int(args, 0);
            match usize::try_from(n)
                .ok()
                .filter(|n| *n < rules.obstacles.len())
            {
                Some(n) => id(followers + n),
                None => Value::Object(ObjectId::INVALID),
            }
        }
        "GetPosition" => Value::Vector(
            thing
                .and_then(|n| course.place(rules, n))
                .unwrap_or([0.0; 3]),
        ),
        "GetPlayerOffset" => Value::Vector(aim),
        "GetPlayerSpeed" => Value::Float(player.speed),
        "GetPlayerMinSpeed" => Value::Float(player.min_speed),
        "GetPlayerMaxSpeed" => Value::Float(player.max_speed),
        "GetPlayerAccelerationPerSecond" => Value::Float(player.accel),
        "GetPlayerTunnelPos" => Value::Vector(player.tunnel_pos),
        "GetPlayerTunnelNeg" => Value::Vector(player.tunnel_neg),
        "GetPlayerOrigin" => Value::Vector(rules.player.path.origin),
        "GetIsInvulnerable" => truth(mover.is_some_and(|m| m.invulnerable_until > course.time_ms)),
        "GetLateralAccelerationPerSecond" => Value::Float(course.lateral_accel),
        "GetCameraNearClip" => Value::Float(course.clip[0]),
        "GetCameraFarClip" => Value::Float(course.clip[1]),
        "GetHitPoints" => Value::Int(mover.map_or(0, |mover| mover.hp)),
        "GetMaxHitPoints" => Value::Int(mover.map_or(0, |mover| mover.max_hp)),
        "GetSphereRadius" => Value::Float(mover.map_or(0.0, |mover| mover.radius)),
        "GetNumLoops" => Value::Int(mover.map_or(0, |mover| mover.loops)),
        "GetLastFollowerHit" => id_of(fact.follower),
        "GetLastObstacleHit" => id_of(fact.obstacle),
        "GetLastBulletHitDamage" => Value::Int(fact.damage),
        "GetLastBulletHitTarget" => id_of(fact.target),
        "GetLastBulletHitShooter" => id_of(fact.shooter),
        "GetLastHPChange" => Value::Int(fact.hp_change),
        "GetObjectName" => string(thing.map_or(String::new(), |n| name_of(rules, n))),
        // Where its track has it, its offset left out (TSL's executable;
        // only dead code calls it).
        "GetTrackPosition" => Value::Vector(
            thing
                .and_then(|n| Some(rules.follower(n)?.path.at(course.movers.get(n)?.track)))
                .unwrap_or([0.0; 3]),
        ),
        // Swoop upgrades are cut from TSL (no upgrade item ships): every
        // slot's level is 0.
        "GetSwoopUpgrade" => Value::Int(0),
        "IsPlayer" => truth(thing == Some(0)),
        "IsEnemy" => truth(thing.is_some_and(|n| n >= 1 && n < followers)),
        "IsFollower" => truth(thing.is_some_and(|n| n < followers)),
        "IsTrigger" => truth(
            thing
                .and_then(|n| rules.follower(n))
                .is_some_and(|follower| follower.trigger),
        ),
        "IsObstacle" => {
            truth(thing.is_some_and(|n| n >= followers && n < followers + rules.obstacles.len()))
        }
        _ => return None,
    })
}

/// Thing `n`'s name (`SWMG_GetObjectName`, TSL's executable): a follower's
/// track model's, an obstacle's model's, as their MDLs spell them.
fn name_of(rules: &Rules, n: usize) -> String {
    match rules.follower(n) {
        Some(follower) => follower.name.clone(),
        None => rules
            .obstacles
            .get(n - rules.followers())
            .map_or_else(String::new, |obstacle| obstacle.name.clone()),
    }
}

/// `caller`'s run, if it is a thing of a co-op race (its globals kept
/// apart, M10).
fn racing_run(engine: &Engine, caller: ObjectId) -> Option<usize> {
    let run = minigame::run(caller.0)?;
    let (course, _) = engine.view.minigame?;
    let course = engine.out.minigame.as_ref().unwrap_or(course);
    (course.racing_together() && minigame::gunner(caller.0).is_none()).then_some(run)
}

/// The globals `caller` keeps apart from the real ones: its run's in a
/// co-op race (M10), a gunner's lens's at the turret (M11).
fn overlay_of(course: &mut Course, caller: ObjectId) -> Option<&mut Overlay> {
    match minigame::gunner(caller.0) {
        Some(gunner) => course
            .gunners
            .get_mut(gunner - 1)
            .map(|gunner| &mut gunner.overlay),
        None if course.racing_together() => course
            .run_mut(minigame::run(caller.0)?)
            .map(|run| &mut run.overlay),
        None => None,
    }
}

/// Global `name` as `caller` keeps it apart (`overlay_of`; `pick`: of
/// which kind); none where it has not set it, or keeps none (the real one).
pub(super) fn overlaid<T: Clone>(
    engine: &Engine,
    caller: ObjectId,
    pick: fn(&Overlay) -> &BTreeMap<String, T>,
    name: &str,
) -> Option<T> {
    minigame::thing(caller.0)?;
    let (course, rules) = engine.view.minigame?;
    let course = engine.out.minigame.as_ref().unwrap_or(course);
    if !course.fits(rules) {
        return None;
    }
    let overlay = match minigame::gunner(caller.0) {
        Some(gunner) => &course.gunners.get(gunner - 1)?.overlay,
        None if course.racing_together() => &course.run(minigame::run(caller.0)?)?.overlay,
        None => return None,
    };
    global(pick(overlay), name).cloned()
}

/// Sets global `name` where `caller` keeps it apart (`overlay_of`;
/// `pick`: of which kind); elsewhere, the value back to be set as the real
/// one.
pub(super) fn overlay<T>(
    engine: &mut Engine,
    caller: ObjectId,
    pick: fn(&mut Overlay) -> &mut BTreeMap<String, T>,
    name: &str,
    value: T,
) -> Result<(), T> {
    if minigame::thing(caller.0).is_none() {
        return Err(value);
    }
    let Some((course, _)) = write(engine, 0) else {
        return Err(value);
    };
    match overlay_of(course, caller) {
        Some(overlay) => {
            set_global(pick(overlay), name, value);
            Ok(())
        }
        None => Err(value),
    }
}

/// A sound object's `change` from `caller` kept in its run of a co-op race
/// (M10; `play`, a start), not the world's; a gunner's lens changes none.
/// Whether it was kept.
pub(super) fn voice(engine: &mut Engine, caller: ObjectId, change: Voice, play: bool) -> bool {
    if minigame::gunner(caller.0).is_some() {
        return true;
    }
    let Some(run) = racing_run(engine, caller) else {
        return false;
    };
    match write(engine, run) {
        Some((course, _)) => {
            course.voice(change.object, change, play);
            true
        }
        None => false,
    }
}

/// `StartNewModule` from `caller`'s run of a co-op race (M10): the run has
/// finished. The first to finish has its module and `movies` held for the
/// others (`Course::leaving`, `Course::leave_due`), its globals made the
/// real ones as the race leaves (`commit`). Whether it was held.
pub(super) fn leave(
    engine: &mut Engine,
    caller: ObjectId,
    (module, tag): (&str, &str),
    movies: &[String],
) -> bool {
    // A gunner's lens runs the player's heartbeat for its compass only: the
    // first seat's leaves.
    if minigame::gunner(caller.0).is_some() {
        return true;
    }
    let Some(run) = racing_run(engine, caller) else {
        return false;
    };
    let Some((course, _)) = write(engine, 0) else {
        return false;
    };
    let now = course.time_ms;
    let Some(own) = course.run_mut(run) else {
        return false;
    };
    own.finished.get_or_insert(now);
    course.leaving.get_or_insert_with(|| Leaving {
        run: run as u8,
        by: caller.0,
        module: module.to_string(),
        tag: tag.to_string(),
        movies: movies.to_vec(),
        since: now,
    });
    true
}

/// A co-op race's winning run's globals (`overlay`) made the real ones in
/// `state`, as the race leaves (M10).
pub(crate) fn commit(state: &mut super::State, overlay: &Overlay) {
    for (name, value) in &overlay.numbers {
        set_global(&mut state.numbers, name, *value);
    }
    for (name, value) in &overlay.booleans {
        set_global(&mut state.booleans, name, *value);
    }
    for (name, value) in &overlay.strings {
        set_global(&mut state.strings, name, value.clone());
    }
}
