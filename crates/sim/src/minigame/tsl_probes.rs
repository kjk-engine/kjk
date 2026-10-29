//! TSL's own minigame rules on courses of our own making, no game files
//! (`kjk sim probes "minigame tsl"`), with its executable's numbers:
//! - the swoop's keys: fire raises `OnAccelerate` where the player has
//!   one (else `OnFire`, K1's and the turret's), jump raises `OnBrake`;
//! - the jump: 30 m/s up, `JUMP_GRAVITY` down, held in the tunnel: about
//!   9.8 m high and down again in about 1.31 s; none in K1's races;
//!   `SWMG_PlayerApplyForce`;
//! - the hits: a pad's own `OnHitFollower` after the player's, an
//!   obstacle's own instead of the player's `OnHitObstacle`, an obstacle
//!   met only where the player's middle passes into its box, the bumps
//!   (both hooks, `Bump_Damage` each way, the push out, the invulnerable
//!   periods, the held speed; a jump clears a mine);
//! - hit points: `SetFollowerHitPoints` only above none, the adjustment
//!   through the damage hook; an obstacle destroyed; the course's new
//!   state round its encoding;
//! - the routines in a world: `SWMG_GetObjectName`, the Dexterity-scaled
//!   lateral acceleration (TSL's) and the speeds refused, upgrades 0, the
//!   script stopped at `SWMG_SetPlayerInvincibility`; the forfeit globals;
//!   107per's bolts on the area's creatures and walls; a conversation the
//!   scripts start while it plays answered by the player.

use fmt_odyssey::ncs::{Const, Op};
use script_ncs::probes::Asm;
use script_ncs::vm::ObjectId;

use super::probes::{MSEC, again, command, run, track};
use super::{
    Bank, Course, Follower, Hook, Hooks, JUMP_GRAVITY, Kind, Obstacle, Path, Raised, Rules,
};
use crate::action::Action;
use crate::combat::Sheet;
use crate::input::{Command, Seat, TickInput};
use crate::module::{ObjectData, Placement};
use crate::probe::{Checks, check};
use crate::script::{Pending, global};
use crate::{Level, SimWorld, StepReason, WorldConfig};

pub fn probes() -> Checks {
    vec![
        ("keys_by_hooks", keys()),
        ("jump_arc", jump()),
        ("apply_force", force()),
        ("pad_own_hook", pad()),
        ("obstacle_one_hook", one_hook()),
        ("obstacle_heights", obstacle_heights()),
        ("bump", bump()),
        ("jump_clears_mine", over_mine()),
        ("hit_points", hit_points()),
        ("obstacle_destroyed", destroyed()),
        ("tsl_course_round_trip", round_trip()),
        ("routines", routines().unwrap_or_else(Err)),
        ("forfeit_globals", forfeit().unwrap_or_else(Err)),
        ("world_turret", world_turret().unwrap_or_else(Err)),
        (
            "talk_answered_while_playing",
            talk_while_playing().unwrap_or_else(Err),
        ),
    ]
}

/// The obstacles' and the enemies' places along the race's track.
const PAD_Y: f32 = 10.0;
const MINE: [f32; 3] = [1.5, 30.0, 0.0];
const SPECIAL_Y: f32 = 50.0;
const WALL_Y: f32 = 70.0;
/// The mine's and the player's `Bump_Damage`, the player's period (ms).
const MINE_BUMP: i32 = 5;
const PLAYER_BUMP: i32 = 3;
const PERIOD_MS: u32 = 300;

/// A follower standing still at `at`, its hooks named.
fn still(at: [f32; 3], hooks: &[(&str, &str)]) -> Follower {
    Follower {
        path: Path {
            origin: at,
            ..track(0.0, 0.0)
        },
        radius: 2.0,
        hooks: labelled(hooks),
        ..Follower::default()
    }
}

fn labelled(hooks: &[(&str, &str)]) -> Hooks {
    let pairs: Vec<(String, String)> = hooks
        .iter()
        .map(|(label, script)| (label.to_string(), script.to_string()))
        .collect();
    Hooks::from_labels(&pairs)
}

/// A TSL race (`tsl`; else K1's): the player down a 100 m track at 10 m/s,
/// a pad with its own hook, a mine to bump, an obstacle with its own hook
/// and a wall without.
fn race(tsl: bool) -> Rules {
    let mut rules = Rules::bare(Kind::Swoop);
    rules.tsl = tsl;
    rules.do_bumping = true;
    rules.use_inertia = true;
    rules.movement_per_sec = 10.0;
    let mut player = still(
        [0.0; 3],
        &[
            ("OnAccelerate", "accel"),
            ("OnBrake", "jump"),
            ("OnFire", "fire"),
            ("OnHitFollower", "pad"),
            ("OnHitObstacle", "crash"),
        ],
    );
    player.path = track(100.0, 10.0);
    player.name = "PROBE_MGT01".into();
    player.hp = 600;
    player.max_hp = 600;
    player.invince_ms = PERIOD_MS;
    player.bump_damage = PLAYER_BUMP;
    player.max_speed = 10.0;
    player.tunnel_pos = [5.0, 0.0, 10.0];
    player.tunnel_neg = [-5.0, 0.0, 0.0];
    player.tunnel_infinite = [false, true, false];
    rules.player = player;
    let pad = Follower {
        trigger: true,
        hp: 1,
        max_hp: 1,
        ..still([0.0, PAD_Y, 0.0], &[("OnHitFollower", "own_pad")])
    };
    let mine = Follower {
        name: "PROBE_MGM01".into(),
        hp: 99,
        max_hp: 99,
        invince_ms: PERIOD_MS,
        bump_damage: MINE_BUMP,
        ..still(MINE, &[("OnHitFollower", "mine")])
    };
    rules.enemies = vec![pad, mine];
    rules.obstacles = vec![
        obstacle([0.0, SPECIAL_Y, 0.0], &[("OnHitFollower", "spec")]),
        obstacle([0.0, WALL_Y, 0.0], &[]),
    ];
    rules
}

/// An obstacle whose box, 2 m a side, stands at `at`.
fn obstacle(at: [f32; 3], hooks: &[(&str, &str)]) -> Obstacle {
    Obstacle {
        model: "probe_obstacle".into(),
        name: "PROBE_OBSTACLE".into(),
        position: at,
        centre: at,
        extent: [1.0; 3],
        hooks: labelled(hooks),
    }
}

/// The pad, the mine and the special obstacle by their numbers.
const PAD: usize = 1;
const MINE_N: usize = 2;
const SPECIAL: usize = 3;

/// A course of `rules` at full speed, the player standing `y` metres down
/// its track (`y / 10` seconds in).
fn at(rules: &Rules, y: f32) -> Course {
    let mut course = Course::new(rules);
    course.started = true;
    course.movers[0].speed = 10.0;
    course.movers[0].track = y / 10.0;
    course
}

/// Steps `course` `steps` times with `command` (one seat).
fn drive(course: &mut Course, rules: &Rules, command: Command, steps: u64) -> Vec<Raised> {
    run(course, rules, &[(0, command)], steps)
}

/// The hooks among `raised` on thing `who`.
fn on(raised: &[Raised], who: usize) -> Vec<Hook> {
    raised
        .iter()
        .filter(|r| usize::from(r.who) == who)
        .map(|r| r.hook)
        .collect()
}

/// Fire pressed then jump pressed: a TSL race's player raises
/// `OnAccelerate` and `OnBrake`; one without `OnAccelerate` (K1's race)
/// `OnFire`; the turret `OnFire`, never `OnBrake`.
fn keys() -> Result<String, String> {
    let jump = Command {
        up: 127,
        ..Command::default()
    };
    let pressed = |rules: &Rules| {
        let mut course = at(rules, 0.0);
        let mut raised = drive(&mut course, rules, command(0, 0, true), 1);
        raised.extend(drive(&mut course, rules, Command::default(), 1));
        raised.extend(drive(&mut course, rules, jump, 1));
        on(&raised, 0)
            .into_iter()
            .filter(|hook| matches!(hook, Hook::Accelerate | Hook::Brake | Hook::Fire))
            .collect::<Vec<_>>()
    };
    let tsl = race(true);
    let mut k1 = race(false);
    k1.player.hooks.0[Hook::Accelerate as usize].clear();
    k1.player.hooks.0[Hook::Brake as usize].clear();
    let mut turret = race(true);
    turret.kind = Kind::Turret;
    let (a, b, c) = (pressed(&tsl), pressed(&k1), pressed(&turret));
    check(
        a == [Hook::Accelerate, Hook::Brake] && b == [Hook::Fire] && c == [Hook::Fire],
        format!("TSL race {a:?}, K1 race {b:?}, turret {c:?}"),
    )
}

/// A jump at 30 m/s in a TSL race: up to about 9.83 m (30² / 2 / 45.8) and
/// down again in about 1.31 s (2 × 30 / 45.8), held at the tunnel's floor;
/// in K1's race the same speed lifts nothing.
fn jump() -> Result<String, String> {
    let arc = |rules: &Rules| {
        let mut course = at(rules, 0.0);
        course.jump = 30.0;
        let (mut peak, mut landed) = (0.0f32, None);
        for step in 1..=400u64 {
            drive(&mut course, rules, Command::default(), 1);
            let z = course.movers[0].offset[2];
            peak = peak.max(z);
            if peak > 0.0 && z <= 0.0 && landed.is_none() {
                landed = Some(step as f32 * MSEC as f32 / 1000.0);
            }
        }
        (peak, landed, course.movers[0].offset[2])
    };
    let (peak, landed, rest) = arc(&race(true));
    let (k1_peak, _, _) = arc(&race(false));
    let expected = (
        30.0f32 * 30.0 / (2.0 * -JUMP_GRAVITY),
        2.0 * 30.0 / -JUMP_GRAVITY,
    );
    check(
        (peak - expected.0).abs() < 0.2
            && landed.is_some_and(|t| (t - expected.1).abs() < 0.03)
            && rest == 0.0
            && k1_peak == 0.0,
        format!(
            "peak {peak:.3} (about {:.3}), landed at {landed:?} s (about {:.3}), resting at {rest}; K1's race peaks at {k1_peak}",
            expected.0, expected.1
        ),
    )
}

/// `SWMG_PlayerApplyForce((30, 0, 0))`: the lateral acceleration reads 30
/// and the player drifts right; `(0, 0, 30)` is a jump.
fn force() -> Result<String, String> {
    let rules = race(true);
    let mut course = at(&rules, 0.0);
    course.apply_force([30.0, 5.0, 0.0]);
    let accel = course.lateral_accel;
    drive(&mut course, &rules, Command::default(), 1);
    let across = course.movers[0].offset[0];
    course.apply_force([0.0, 0.0, 30.0]);
    let jump = course.jump;
    check(
        accel == 30.0 && across > 0.0 && jump == 30.0,
        format!("lateral acceleration {accel}, moved across {across}, jump speed {jump}"),
    )
}

/// The player onto the pad: its `OnHitFollower` hearing of the pad, then
/// the pad's own hearing of the player, once; K1's pad, with no hook of its
/// own, keeps no second hit.
fn pad() -> Result<String, String> {
    let met = |rules: &Rules| {
        let mut course = at(rules, PAD_Y - 3.0);
        let raised = drive(&mut course, rules, Command::default(), 50);
        let hits: Vec<(usize, Option<u16>)> = raised
            .iter()
            .filter(|r| r.hook == Hook::HitFollower)
            .map(|r| (usize::from(r.who), course.fact_of(r.fact).follower))
            .collect();
        (hits, course.facts.len())
    };
    let tsl = race(true);
    let mut k1 = race(false);
    k1.enemies[0].hooks = Hooks::default();
    let ((hits, _), (k1_hits, k1_facts)) = (met(&tsl), met(&k1));
    check(
        hits == [(0, Some(PAD as u16)), (PAD, Some(0))]
            && k1_hits == [(0, Some(PAD as u16))]
            && k1_facts == 1,
        format!("TSL's pad heard by {hits:?}; K1's by {k1_hits:?}, {k1_facts} hit kept"),
    )
}

/// The player into an obstacle with a hook of its own: that hook alone; into
/// one without: the player's `OnHitObstacle` alone.
fn one_hook() -> Result<String, String> {
    let rules = race(true);
    let hooks = |y: f32| {
        let mut course = at(&rules, y);
        drive(&mut course, &rules, Command::default(), 50)
            .into_iter()
            .filter(|r| matches!(r.hook, Hook::HitFollower | Hook::HitObstacle))
            .map(|r| (usize::from(r.who), r.hook))
            .collect::<Vec<_>>()
    };
    let (special, wall) = (hooks(SPECIAL_Y - 4.0), hooks(WALL_Y - 4.0));
    check(
        special == [(SPECIAL, Hook::HitFollower)] && wall == [(0, Hook::HitObstacle)],
        format!("into the special {special:?}, into the wall {wall:?}"),
    )
}

/// An obstacle 5 m under the track and 2 m across from it: a TSL race's
/// player passes over and beside it (its middle never in the box); K1's
/// meets it across the ground, its sphere within reach of the box.
fn obstacle_heights() -> Result<String, String> {
    let met = |tsl: bool| {
        let mut rules = race(tsl);
        rules.obstacles[1] = obstacle([0.0, WALL_Y, -5.0], &[]);
        rules
            .obstacles
            .push(obstacle([2.5, WALL_Y + 20.0, 0.0], &[]));
        let mut course = at(&rules, WALL_Y - 4.0);
        drive(&mut course, &rules, Command::default(), 300)
            .into_iter()
            .filter(|r| r.hook == Hook::HitObstacle)
            .count()
    };
    let (tsl, k1) = (met(true), met(false));
    check(
        tsl == 0 && k1 == 2,
        format!("TSL's race meets {tsl}, K1's {k1}"),
    )
}

/// The player at the mine (1.5 m across): on the step their ways come
/// within the two radii, the mine takes the player's `Bump_Damage` (99 to
/// 96) and the player the mine's (600 to 595), each hears of the other, the
/// player is pushed to `BUMP_ROOM` past touching (5.1 m from the mine),
/// invulnerable twice its period, its speed held a step.
fn bump() -> Result<String, String> {
    let rules = race(true);
    let mut course = at(&rules, MINE[1] - 6.0);
    let mut raised = Vec::new();
    let mut steps = 0;
    while steps < 100 && !raised.iter().any(|r: &Raised| r.hook == Hook::HitFollower) {
        raised = drive(&mut course, &rules, Command::default(), 1);
        steps += 1;
    }
    let met = course.time_ms;
    let hooks: Vec<(usize, Hook)> = raised
        .iter()
        .filter(|r| r.hook == Hook::HitFollower)
        .map(|r| (usize::from(r.who), r.hook))
        .collect();
    let (player, mine) = (&course.movers[0], &course.movers[MINE_N]);
    let apart = course
        .position(&rules, 0)
        .zip(course.position(&rules, MINE_N))
        .map_or(0.0, |(a, b)| crate::math::length(crate::math::sub(a, b)));
    let speed = player.speed;
    let held = course.bumped;
    let (hp, mine_hp) = (player.hp, mine.hp);
    let invulnerable = player.invulnerable_until.saturating_sub(met);
    check(
        hooks == [(0, Hook::HitFollower), (MINE_N, Hook::HitFollower)]
            && hp == 600 - MINE_BUMP
            && mine_hp == 99 - PLAYER_BUMP
            && (apart - (2.0 + 2.0 + 1.1)).abs() < 0.01
            && invulnerable == 2 * u64::from(PERIOD_MS)
            && held
            && speed == 10.0,
        format!(
            "after {steps} steps: hooks {hooks:?}, hp {hp}, the mine's {mine_hp}, {apart:.3} m apart, invulnerable {invulnerable} ms, speed held {held}"
        ),
    )
}

/// Jumping as it passes the mine, the player clears it: no bump.
fn over_mine() -> Result<String, String> {
    let rules = race(true);
    let mut course = at(&rules, MINE[1] - 6.0);
    course.jump = 30.0;
    let raised = drive(&mut course, &rules, Command::default(), 120);
    let bumps = raised
        .iter()
        .filter(|r| usize::from(r.who) == MINE_N || r.hook == Hook::HitFollower)
        .count();
    check(
        bumps == 0 && course.movers[0].hp == 600,
        format!("{bumps} bump hooks, hp {}", course.movers[0].hp),
    )
}

/// `SWMG_SetFollowerHitPoints` 0 and -5 leave them, 42 sets them; an
/// adjustment adds, held at the maximum; -100 on the 1-hit-point pad kills
/// it (no death hook: gone); one on a follower with a damage hook raises it
/// and leaves the hit points to it.
fn hit_points() -> Result<String, String> {
    let mut rules = race(true);
    let mut course = at(&rules, 0.0);
    let mut raised = Vec::new();
    course.set_hp(0, 0);
    course.set_hp(0, -5);
    let kept = course.movers[0].hp;
    course.set_hp(0, 42);
    let set = course.movers[0].hp;
    course.adjust_hp(&rules, 0, 1000, &mut raised);
    let capped = course.movers[0].hp;
    course.adjust_hp(&rules, PAD, -100, &mut raised);
    let pad_gone = !course.movers[PAD].alive;
    rules.enemies[1].hooks = labelled(&[("OnDamage", "hurt")]);
    course.adjust_hp(&rules, MINE_N, -10, &mut raised);
    let hooked = on(&raised, MINE_N) == [Hook::Damage] && course.movers[MINE_N].hp == 99;
    let change = raised
        .last()
        .map_or(0, |r| course.fact_of(r.fact).hp_change);
    check(
        kept == 600 && set == 42 && capped == 600 && pad_gone && hooked && change == -10,
        format!(
            "set 0 and -5: {kept}; set 42: {set}; +1000: {capped}; the pad gone {pad_gone}; the hooked mine {hooked} (heard {change})"
        ),
    )
}

/// The special obstacle destroyed: gone, met no more; the mine "destroyed"
/// stays (an obstacle only).
fn destroyed() -> Result<String, String> {
    let rules = race(true);
    let mut course = at(&rules, SPECIAL_Y - 4.0);
    course.destroy(&rules, SPECIAL);
    course.destroy(&rules, MINE_N);
    let raised = drive(&mut course, &rules, Command::default(), 50);
    let met = on(&raised, SPECIAL).len();
    check(
        course.stills[0].gone && !course.stills[1].gone && course.movers[MINE_N].alive && met == 0,
        format!(
            "special gone {}, wall gone {}, mine there {}, special heard {met}",
            course.stills[0].gone, course.stills[1].gone, course.movers[MINE_N].alive
        ),
    )
}

/// A course mid-jump, its jump key held, its speed held by a bump and an
/// obstacle gone comes back from its encoding as it was.
fn round_trip() -> Result<String, String> {
    let rules = race(true);
    let mut course = at(&rules, 0.0);
    course.jump = 12.5;
    course.jump_held = true;
    course.bumped = true;
    course.stills[1].gone = true;
    let back = again(&course)?;
    check(
        back == course,
        format!(
            "jump {}, held {}, bumped {}, gone {}",
            back.jump, back.jump_held, back.bumped, back.stills[1].gone
        ),
    )
}

/// The probe's routines, as `nwscript.nss` declares them.
const CATALOG: &str = r#"
// 0
void SetGlobalString(string sIdentifier, string sValue);
// 1
void SetGlobalNumber(string sIdentifier, int nValue);
// 2
object SWMG_GetEnemy(int nEntry);
// 3
string SWMG_GetObjectName(object oid = OBJECT_SELF);
// 4
void SWMG_SetLateralAccelerationPerSecond(float fLAPS);
// 5
void SWMG_SetPlayerMinSpeed(float fMinSpeed);
// 6
void SWMG_SetPlayerMaxSpeed(float fMaxSpeed);
// 7
int SWMG_GetSwoopUpgrade(int nSlot);
// 8
void SWMG_SetPlayerInvincibility(float fInvincibility);
// 9
object SWMG_GetPlayer();
// 10
void SWMG_StartInvulnerability(object oFollower);
// 11
int GetUserDefinedEventNumber();
"#;

fn call(asm: &mut Asm, routine: u16, args: u8) {
    asm.op(Op::Action { routine, args });
}

fn text(value: &str) -> Op {
    Op::Const(Const::String(value.as_bytes().to_vec()))
}

/// The probe's scripts: `probe_routines` (the mine's name into `NAME`, the
/// lateral acceleration 100, the speeds -5, upgrade 0 into `UPGRADE`, then
/// `SWMG_SetPlayerInvincibility`, after which nothing may run:
/// invulnerability started, `AFTER` set); `probe_struck` (its user-defined
/// event's number into `STRUCK`).
fn scripts() -> crate::script::Library {
    let mut routines = Asm::default();
    routines.op(Op::Const(Const::Int(1)));
    call(&mut routines, 2, 1);
    call(&mut routines, 3, 1);
    routines.op(text("NAME"));
    call(&mut routines, 0, 2);
    for (routine, value) in [(4, 100.0), (5, -5.0), (6, -5.0)] {
        routines.op(Op::Const(Const::Float(value)));
        call(&mut routines, routine, 1);
    }
    routines.op(Op::Const(Const::Int(0)));
    call(&mut routines, 7, 1);
    routines.op(text("UPGRADE"));
    call(&mut routines, 1, 2);
    routines.op(Op::Const(Const::Float(1.0)));
    call(&mut routines, 8, 1);
    call(&mut routines, 9, 0);
    call(&mut routines, 10, 1);
    routines.op(Op::Const(Const::Int(1)));
    routines.op(text("AFTER"));
    call(&mut routines, 1, 2);
    routines.op(Op::Retn);
    let mut struck = Asm::default();
    call(&mut struck, 11, 0);
    struck.op(text("STRUCK"));
    call(&mut struck, 1, 2);
    struck.op(Op::Retn);
    crate::probe::library(
        CATALOG,
        [
            ("probe_routines", routines.build()),
            ("probe_struck", struck.build()),
        ],
    )
}

/// A world on the proving ground as module `name` of `game`, with `placed`
/// added, `rules`' minigame started and the probe's scripts; one seat, the
/// developer's soldier (Dexterity 14).
fn world(
    rules: &Rules,
    (name, game): (&str, &str),
    placed: Vec<ObjectData>,
) -> Result<SimWorld, String> {
    world_in(rules, (name, game), placed, scripts())
}

/// `world` with `library` its scripts and conversations.
fn world_in(
    rules: &Rules,
    (name, game): (&str, &str),
    placed: Vec<ObjectData>,
    library: crate::script::Library,
) -> Result<SimWorld, String> {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    data.name = name.into();
    data.game = game.into();
    data.placements
        .extend(placed.into_iter().map(Placement::Own));
    let level: Level = ground
        .with_module(data)
        .with_minigame(rules.clone())
        .with_scripts(library);
    let config = WorldConfig {
        sheet: Sheet::dev_soldier(),
        ..WorldConfig::one_seat()
    };
    let mut start = SimWorld::new(config, level.clone())
        .map_err(|why| why.to_string())?
        .snapshot();
    start.minigame = Some(Course::new(rules));
    SimWorld::from_snapshot(&start, level).map_err(|why| why.to_string())
}

/// Steps `world` `steps` times from `tick` with seat 0's `input`; the tick
/// it reached.
fn advance(world: &mut SimWorld, tick: u64, steps: u64, input: &TickInput) -> u64 {
    for k in 1..=steps {
        crate::step(world, tick + k, input, MSEC, StepReason::Authority);
    }
    tick + steps
}

/// The course as the world has it.
fn course_of(world: &mut SimWorld) -> Result<Course, String> {
    world.snapshot().minigame.ok_or("no course".into())
}

/// `probe_routines` run by the race's player, in TSL's race and K1's:
/// `SWMG_GetObjectName` is the mine's track's name; the lateral
/// acceleration 100 is 160 in TSL's (Dexterity 14: + 2 × 0.3) and 100 in
/// K1's; the minimum speed -5 is kept in TSL's only, the maximum -5 in
/// neither; every upgrade is 0; the script stops at
/// `SWMG_SetPlayerInvincibility` without a fault.
fn routines() -> Result<Result<String, String>, String> {
    /// What `probe_routines` left: the name, the course, `UPGRADE`,
    /// `AFTER`, the faults.
    type Ran = (Option<String>, Course, Option<i32>, Option<i32>, usize);
    let ran = |tsl: bool| -> Result<Ran, String> {
        let rules = race(tsl);
        let mut world = world(
            &rules,
            ("probe", if tsl { "tsl" } else { "k1" }),
            Vec::new(),
        )?;
        let mut state = world.script_state().clone();
        crate::script::set_global(&mut state.numbers, "UPGRADE", 7);
        state
            .pending
            .push(Pending::new("probe_routines", ObjectId(super::id(0, 0))));
        world.set_script_state(state);
        advance(&mut world, 0, 1, &TickInput::default());
        let course = course_of(&mut world)?;
        let state = world.script_state();
        let name = global(&state.strings, "NAME").cloned();
        let upgrade = global(&state.numbers, "UPGRADE").copied();
        let after = global(&state.numbers, "AFTER").copied();
        let faults = world.script_report().faults.len();
        Ok((name, course, upgrade, after, faults))
    };
    let (name, tsl, upgrade, after, faults) = ran(true)?;
    let (_, k1, _, k1_after, k1_faults) = ran(false)?;
    let speeds = |course: &Course| (course.movers[0].min_speed, course.movers[0].max_speed);
    let invulnerable = tsl.movers[0].invulnerable_until > tsl.time_ms;
    Ok(check(
        name.as_deref() == Some("PROBE_MGM01")
            && tsl.lateral_accel == 160.0
            && k1.lateral_accel == 100.0
            && speeds(&tsl) == (-5.0, 10.0)
            && speeds(&k1) == (0.0, 10.0)
            && upgrade == Some(0)
            && after.is_none()
            && k1_after.is_none()
            && !invulnerable
            && faults + k1_faults == 0,
        format!(
            "name {name:?}; lateral TSL {}, K1 {}; speeds TSL {:?}, K1 {:?}; upgrade {upgrade:?}; after the stop {after:?} {k1_after:?}, invulnerable {invulnerable}, faults {}",
            tsl.lateral_accel,
            k1.lateral_accel,
            speeds(&tsl),
            speeds(&k1),
            faults + k1_faults
        ),
    ))
}

/// A TSL race begins with `FORFEIT_SWOOPRACE` false and the first seat's
/// forfeit sets `GBL_QUIT_SWOOP`; K1's race sets neither.
fn forfeit() -> Result<Result<String, String>, String> {
    let ran = |tsl: bool| -> Result<(Option<bool>, Option<bool>), String> {
        let rules = race(tsl);
        let mut world = world(
            &rules,
            ("probe", if tsl { "tsl" } else { "k1" }),
            Vec::new(),
        )?;
        let tick = advance(&mut world, 0, 1, &TickInput::default());
        let mut quit = TickInput::default();
        quit.act(Seat(0), Action::Forfeit);
        advance(&mut world, tick, 1, &quit);
        let booleans = &world.script_state().booleans;
        Ok((
            global(booleans, "FORFEIT_SWOOPRACE").copied(),
            global(booleans, "GBL_QUIT_SWOOP").copied(),
        ))
    };
    let (tsl, k1) = (ran(true)?, ran(false)?);
    Ok(check(
        tsl == (Some(false), Some(true)) && k1 == (None, None),
        format!("TSL's race (forfeit, quit) {tsl:?}; K1's {k1:?}"),
    ))
}

/// The turret's bolt's damage (under 128: the probe keeps it in a global).
const BOLT_DAMAGE: i32 = 100;

/// The turret: one bolt fired 1 m above the proving ground's floor, 6 m
/// short of its dividing wall, straight at it (along the ground's x), at
/// 20 m/s.
fn turret() -> Rules {
    let (sin, cos) = collision::trig::sin_cos(crate::determinism::TURN_DEGREES.to_radians());
    let mut rules = Rules::bare(Kind::Turret);
    rules.player = Follower {
        path: Path {
            origin: crate::determinism::ground_point(-6.0, 0.0, 1.0),
            ..track(0.0, 0.0)
        },
        radius: 2.0,
        hp: 1,
        max_hp: 1,
        tunnel_infinite: [true; 3],
        banks: vec![Bank {
            damage: BOLT_DAMAGE,
            lifespan_ms: 2000,
            rate_ms: 5000,
            speed: 20.0,
            target: super::targets::ENEMIES,
            way: [cos, sin, 0.0],
            ..Bank::default()
        }],
        ..Follower::default()
    };
    rules
}

/// 107per's bolts as TSL's executable flies them: one meets a creature (3 m
/// out) and is spent, the creature's user-defined event hearing its damage;
/// with none there it stops on the wall (6 m out) long before its life is
/// out. Any other module's bolt flies through both.
fn world_turret() -> Result<Result<String, String>, String> {
    let rules = turret();
    let ran = |module: &str, creature: bool| -> Result<(Option<i32>, usize), String> {
        let at = crate::determinism::ground_point(-3.0, 0.0, 0.0);
        let placed = creature
            .then(|| ObjectData {
                scripts: vec![("ScriptUserDefine".into(), "probe_struck".into())],
                ..ObjectData::creature("probe_trooper", "probe_trooper", at, Sheet::commoner())
            })
            .into_iter()
            .collect();
        let mut world = world(&rules, (module, "tsl"), placed)?;
        let mut fire = TickInput::default();
        fire.insert(Seat(0), command(0, 0, true));
        let tick = advance(&mut world, 0, 1, &fire);
        // 0.8 s: past the creature and the wall, inside the bolt's 2 s.
        advance(&mut world, tick, 100, &TickInput::default());
        let struck = global(&world.script_state().numbers, "STRUCK").copied();
        Ok((struck, course_of(&mut world)?.bullets.len()))
    };
    let (hit, wall, elsewhere) = (
        ran("107per", true)?,
        ran("107per", false)?,
        ran("probe", true)?,
    );
    Ok(check(
        hit == (Some(BOLT_DAMAGE), 0) && wall == (None, 0) && elsewhere == (None, 1),
        format!(
            "107per: on the creature {hit:?}, on the wall {wall:?}; elsewhere {elsewhere:?} (struck, bolts flying)"
        ),
    ))
}

/// A conversation the scripts start while the turret plays is the
/// player's to answer, as TSL's 107per ends: Atton's `turend`, its reply
/// loading the boarded Ebon Hawk. The minigame parks what moves the
/// player, never its answer: the last reply ("Goodbye.") ends it.
fn talk_while_playing() -> Result<Result<String, String>, String> {
    use crate::script::dialogue::{Line, Pick, Start, answer};
    let rules = turret();
    let at = crate::determinism::ground_point(-3.0, 2.0, 0.0);
    let placed = vec![ObjectData::creature(
        "probe_gunner",
        "probe_gunner",
        at,
        Sheet::commoner(),
    )];
    let dialogues = [(
        "probe_talk".to_string(),
        crate::script::dialogue_probes::talking(2, ""),
    )]
    .into();
    let library = scripts().with_dialogues(dialogues, std::collections::BTreeMap::new());
    let mut world = world_in(&rules, ("107per", "tsl"), placed, library)?;
    let start = world.snapshot();
    let player = start
        .body(Seat(0))
        .map(|body| body.id.0)
        .ok_or("no player")?;
    let owner = start
        .entities
        .iter()
        .find(|entity| entity.id.0 != player && entity.fighter.is_some())
        .map(|entity| entity.id.0)
        .ok_or("no gunner")?;
    let mut state = world.script_state().clone();
    state.talk.asked.push(Start {
        owner: ObjectId(owner),
        pc: ObjectId(player),
        asker: ObjectId(player),
        dialog: "probe_talk".into(),
        bark: false,
    });
    world.set_script_state(state);
    let mut still = TickInput::default();
    still.insert(Seat(0), Command::default());
    let tick = advance(&mut world, 0, 2, &still);
    let begun = world.snapshot().talk.conversations.first().map(|c| c.id);
    let mut goodbye = TickInput::default();
    goodbye.insert(
        Seat(0),
        Command {
            reply: answer(0, Pick::Reply(1)),
            ..Command::default()
        },
    );
    // 8 s: the line played and its last reply taken.
    advance(&mut world, tick, 1000, &goodbye);
    let ended = begun.is_some_and(|id| {
        world
            .script_report()
            .talk
            .iter()
            .any(|line| matches!(line, Line::Ended { id: of, aborted: false, .. } if *of == id))
    });
    let playing = world.snapshot().minigame.is_some();
    Ok(check(
        ended && playing,
        format!("begun {begun:?}, ended by its reply {ended}, the turret still playing {playing}"),
    ))
}
