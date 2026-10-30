//! A minigame's co-op (`docs/MINIGAMES.md`) on courses of
//! our own making, no game files: a straight track with a pad for the
//! race, a gun on a still hull with a looping fighter for the turret. For
//! `kjk sim probes` (group "minigame").

use fmt_odyssey::ncs::{Const, Op, Pair};

use super::{Bank, Course, Follower, Hook, Hooks, Kind, LEAVE_WAIT_MS, Leaving, Path, Rules};
use crate::input::{Command, buttons};
use crate::probe::{Checks, check};
use crate::wire::{Reader, Writer};
use crate::{Seat, SimWorld, StepReason, TickInput, WorldConfig};

/// One step's length (ms).
pub(super) const MSEC: u32 = 8;

/// A track `length` seconds long running `metres` along +y.
pub(super) fn track(metres: f32, length: f32) -> Path {
    Path {
        origin: [0.0; 3],
        rest: [0.0, 0.0, 0.0, 1.0],
        positions: vec![(0.0, [0.0; 3]), (length, [0.0, metres, 0.0])],
        handles: Vec::new(),
        orientations: Vec::new(),
        length,
    }
}

/// A race: the player down a 100 m track, a pad 10 m along it, its hooks
/// named (`OnHeartbeat`, `OnHitFollower`) so they are raised.
pub(crate) fn race() -> Rules {
    let hooks = Hooks::from_labels(&[
        ("OnHeartbeat".into(), "heart".into()),
        ("OnHitFollower".into(), "pad".into()),
    ]);
    let mut rules = Rules::bare(Kind::Swoop);
    rules.movement_per_sec = 10.0;
    rules.lateral_accel = 20.0;
    rules.player = Follower {
        path: track(100.0, 10.0),
        radius: 1.0,
        hp: 1,
        max_hp: 1,
        max_speed: 10.0,
        tunnel_pos: [5.0, 0.0, 0.0],
        tunnel_neg: [-5.0, 0.0, 0.0],
        hooks,
        ..Follower::default()
    };
    rules.enemies = vec![Follower {
        path: Path {
            origin: [0.0, 10.0, 0.0],
            ..track(0.0, 0.0)
        },
        radius: 1.0,
        hp: 1,
        max_hp: 1,
        trigger: true,
        ..Follower::default()
    }];
    rules
}

/// The turret: a gun firing along +y on a still hull, a fighter 50 m out
/// flying a loop, fighters scaled half again per gunner.
fn turret() -> Rules {
    let mut rules = Rules::bare(Kind::Turret);
    rules.per_player_hp = 0.5;
    rules.mouse = [2, 0];
    rules.player = Follower {
        path: track(0.0, 0.0),
        radius: 2.0,
        hp: 3000,
        max_hp: 3000,
        tunnel_infinite: [false, true, true],
        tunnel_pos: [30.0, 0.0, 0.0],
        tunnel_neg: [-30.0, 0.0, 0.0],
        banks: vec![Bank {
            damage: 30,
            lifespan_ms: 2000,
            rate_ms: 300,
            speed: 300.0,
            target: super::targets::ENEMIES,
            way: [0.0, 1.0, 0.0],
            ..Bank::default()
        }],
        ..Follower::default()
    };
    rules.enemies = vec![Follower {
        path: Path {
            origin: [0.0, 50.0, 0.0],
            ..track(10.0, 4.0)
        },
        loops: -1,
        radius: 3.0,
        hp: 100,
        max_hp: 100,
        ..Follower::default()
    }];
    rules
}

/// A seat's command: strafing `right`, turned `yaw` (JA shorts), firing.
pub(super) fn command(right: i8, yaw: i16, fire: bool) -> Command {
    Command {
        right,
        angles: [0, yaw, 0],
        buttons: if fire { buttons::ATTACK } else { 0 },
        ..Command::default()
    }
}

/// `course` stepped `steps` times with `commands`, its hooks raised.
pub(super) fn run(
    course: &mut Course,
    rules: &Rules,
    commands: &[(u8, Command)],
    steps: u64,
) -> Vec<super::Raised> {
    (0..steps)
        .flat_map(|tick| course.step(rules, commands, MSEC, tick))
        .collect()
}

/// `course` encoded and decoded again.
pub(super) fn again(course: &Course) -> Result<Course, String> {
    let mut out = Writer::default();
    course.encode(&mut out);
    let mut input = Reader::new("minigame probe", &out.0);
    let back = Course::decode(&mut input).map_err(|why| why.to_string())?;
    input.finish().map_err(|why| why.to_string())?;
    Ok(back)
}

pub fn probes() -> Checks {
    vec![
        ("solo_has_no_runs", solo()),
        ("race_run_per_seat", runs_per_seat()),
        ("race_hooks_by_run", hooks_by_run()),
        ("race_leave_held", leave_held()),
        ("race_leave_waits_at_most", leave_waits()),
        ("race_voices_apart", voices_apart()),
        ("turret_gunner_per_seat", gunners()),
        ("turret_fighters_scaled", scaled()),
        ("turret_late_gunner", late_gunner()),
        ("coop_course_round_trip", round_trip()),
        ("coop_course_twice_alike", twice_alike()),
        ("race_globals_apart", globals_apart().unwrap_or_else(Err)),
        ("turret_lens_compass", lens_compass().unwrap_or_else(Err)),
    ]
}

/// One seat: no run, no gunner, the course as it always was.
fn solo() -> Result<String, String> {
    let (race, turret) = (race(), turret());
    let mut swoop = Course::new(&race);
    run(&mut swoop, &race, &[(0, command(0, 0, false))], 10);
    let mut gun = Course::new(&turret);
    run(&mut gun, &turret, &[(0, command(0, 0, true))], 10);
    check(
        swoop.runs.is_empty() && gun.gunners.is_empty() && gun.movers[1].max_hp == 100,
        format!(
            "runs {}, gunners {}, fighter {}",
            swoop.runs.len(),
            gun.gunners.len(),
            gun.movers[1].max_hp
        ),
    )
}

/// Three seats racing: two runs past the first, each steered by its own
/// seat's strafe (the first right, the second left, the third straight).
fn runs_per_seat() -> Result<String, String> {
    let rules = race();
    let mut course = Course::new(&rules);
    let commands = [
        (0, command(127, 0, false)),
        (1, command(-127, 0, false)),
        (2, command(0, 0, false)),
    ];
    run(&mut course, &rules, &commands, 100);
    let offsets: Vec<f32> = std::iter::once(&course)
        .chain(&course.runs)
        .map(|run| run.movers[0].offset[0])
        .collect();
    let seats: Vec<u8> = course.runs.iter().map(|run| run.seat).collect();
    check(
        seats == [1, 2]
            && offsets[0] > 0.1
            && offsets[1] < -0.1
            && offsets[2].abs() < 1e-6
            && course.fits(&rules),
        format!("seats {seats:?}, offsets {offsets:?}"),
    )
}

/// Each run's heartbeat raised as its own run's, and each run touches its
/// own pad: the hooks' callers' ids tell the runs apart.
fn hooks_by_run() -> Result<String, String> {
    let rules = race();
    let mut course = Course::new(&rules);
    let commands = [(0, command(0, 0, false)), (1, command(0, 0, false))];
    // The runs made, each fast enough to reach its pad in two seconds.
    run(&mut course, &rules, &commands, 1);
    course.movers[0].accel = 100.0;
    course.runs[0].movers[0].accel = 100.0;
    let raised = run(&mut course, &rules, &commands, 250);
    let count = |run: u8, hook: Hook| {
        raised
            .iter()
            .filter(|r| r.span == run && r.hook == hook)
            .count()
    };
    let ids: Vec<u32> = raised
        .iter()
        .filter(|r| r.hook == Hook::HitFollower)
        .filter_map(|r| super::pending(&rules, *r))
        .map(|pending| pending.caller.0)
        .collect();
    let heartbeats = [count(0, Hook::Heartbeat), count(1, Hook::Heartbeat)];
    let pads = [count(0, Hook::HitFollower), count(1, Hook::HitFollower)];
    let runs: Vec<Option<usize>> = ids.iter().map(|id| super::run(*id)).collect();
    check(
        heartbeats[0] > 0
            && heartbeats[0] == heartbeats[1]
            && pads == [1, 1]
            && runs == [Some(0), Some(1)],
        format!("heartbeats {heartbeats:?}, pads {pads:?}, their runs {runs:?}"),
    )
}

/// A race's module held while a run is still going, given once all have
/// finished.
fn leave_held() -> Result<String, String> {
    let rules = race();
    let mut course = Course::new(&rules);
    run(
        &mut course,
        &rules,
        &[(0, command(0, 0, false)), (1, command(0, 0, false))],
        1,
    );
    course.leaving = Some(Leaving {
        run: 0,
        movies: Vec::new(),
        by: super::id(0, 0),
        module: "tar_m03af".into(),
        tag: "pits".into(),
        since: course.time_ms,
    });
    course.finished = Some(course.time_ms);
    let held = course.leave_due().is_none();
    course.runs[0].finished = Some(course.time_ms);
    let given = course.leave_due().map(|leaving| leaving.module);
    check(
        held && given.as_deref() == Some("tar_m03af") && course.leaving.is_none(),
        format!("held {held}, then {given:?}"),
    )
}

/// One run that never finishes holds the others `LEAVE_WAIT_MS` at most.
fn leave_waits() -> Result<String, String> {
    let rules = race();
    let mut course = Course::new(&rules);
    let commands = [(0, command(0, 0, false)), (1, command(0, 0, false))];
    run(&mut course, &rules, &commands, 1);
    course.leaving = Some(Leaving {
        run: 0,
        movies: Vec::new(),
        by: super::id(0, 0),
        module: "tar_m03af".into(),
        tag: String::new(),
        since: course.time_ms,
    });
    course.finished = Some(course.time_ms);
    let steps = LEAVE_WAIT_MS / u64::from(MSEC);
    run(&mut course, &rules, &commands, steps - 1);
    let early = course.leave_due().is_none();
    run(&mut course, &rules, &commands, 1);
    let late = course.leave_due().is_some();
    check(
        early && late,
        format!("held before {early}, given at the wait {late}"),
    )
}

/// Two seats at the turret: one gunner, aiming on its own (turned 90° while
/// the first seat looks ahead), firing its own bolts.
fn gunners() -> Result<String, String> {
    let rules = turret();
    let mut course = Course::new(&rules);
    run(
        &mut course,
        &rules,
        &[(0, command(0, 0, false)), (1, command(0, 0, false))],
        1,
    );
    // A quarter turn left (JA's yaw: 0x4000).
    let commands = [(0, command(0, 0, true)), (1, command(0, 0x4000, true))];
    run(&mut course, &rules, &commands, 2);
    let (first, second) = (course.aim_of(&rules, 0), course.aim_of(&rules, 1));
    let bolts = |gunner: u8| course.bullets.iter().filter(|b| b.gunner == gunner).count();
    let ways: Vec<[f32; 3]> = course.bullets.iter().map(|b| b.velocity).collect();
    check(
        course.gunners.len() == 1
            && bolts(0) == 1
            && bolts(1) == 1
            && first[1] > 0.99
            && second[1].abs() < 0.01
            && course.bullets.iter().all(|b| b.shooter == 0),
        format!(
            "gunners {}, aims {first:?} {second:?}, bolts {ways:?}",
            course.gunners.len()
        ),
    )
}

/// Three gunners at the start: the fighter has 1 + 0.5 × 2 of its hit
/// points.
fn scaled() -> Result<String, String> {
    let rules = turret();
    let mut course = Course::new(&rules);
    let commands = [
        (0, command(0, 0, false)),
        (1, command(0, 0, false)),
        (2, command(0, 0, false)),
    ];
    run(&mut course, &rules, &commands, 3);
    let fighter = &course.movers[1];
    check(
        fighter.hp == 200 && fighter.max_hp == 200 && course.movers[0].hp == 3000,
        format!(
            "fighter {}/{}, hull {}",
            fighter.hp, fighter.max_hp, course.movers[0].hp
        ),
    )
}

/// A seat coming after the start mans a gun, and adds its share to each
/// fighter still flying: one hurt keeps its wounds.
fn late_gunner() -> Result<String, String> {
    let rules = turret();
    let mut course = Course::new(&rules);
    run(&mut course, &rules, &[(0, command(0, 0, false))], 5);
    course.movers[1].hp = 40;
    run(
        &mut course,
        &rules,
        &[(0, command(0, 0, false)), (1, command(0, 0, false))],
        5,
    );
    let fighter = &course.movers[1];
    check(
        course.gunners.len() == 1 && fighter.max_hp == 150 && fighter.hp == 90,
        format!(
            "gunners {}, fighter {}/{}",
            course.gunners.len(),
            fighter.hp,
            fighter.max_hp
        ),
    )
}

/// A race with runs, globals, a finish and a held module, and a turret with
/// a gunner, come back from their encoding as they were.
fn round_trip() -> Result<String, String> {
    let (race, turret) = (race(), turret());
    let mut swoop = Course::new(&race);
    run(
        &mut swoop,
        &race,
        &[(0, command(64, 0, false)), (1, command(-64, 0, false))],
        20,
    );
    swoop.runs[0]
        .overlay
        .numbers
        .insert("TAR_SWOOP_SEC".into(), 31);
    swoop.runs[0].overlay.booleans.insert("RUN".into(), true);
    swoop.runs[0]
        .overlay
        .strings
        .insert("WHO".into(), "b".into());
    swoop.runs[0].finished = Some(100);
    swoop.runs[0].voice(
        9,
        super::Voice {
            object: 9,
            state: Some(crate::snapshot::sound::PLAYING),
            volume: None,
            pitch: Some(151),
            plays: 0,
            fade: Some(500),
        },
        true,
    );
    swoop.leaving = Some(Leaving {
        run: 1,
        movies: vec!["probe_movie".into()],
        by: super::id(1, 0),
        module: "tar_m03af".into(),
        tag: "pits".into(),
        since: 100,
    });
    let mut gun = Course::new(&turret);
    run(
        &mut gun,
        &turret,
        &[(0, command(0, 0, true)), (1, command(0, 0x1000, true))],
        20,
    );
    let back = (again(&swoop)?, again(&gun)?);
    check(
        back.0 == swoop && back.1 == gun,
        format!(
            "race {} runs, turret {} gunners, {} bolts",
            back.0.runs.len(),
            back.1.gunners.len(),
            back.1.bullets.len()
        ),
    )
}

/// The same seats' commands step two courses alike: a race with runs and a
/// turret with gunners end equal, bolts and hits and all.
fn twice_alike() -> Result<String, String> {
    let play = |rules: &Rules| {
        let mut course = Course::new(rules);
        for tick in 0..600u64 {
            let wobble = ((tick / 50) % 3) as i8 - 1;
            let commands = [
                (0, command(wobble * 100, (tick * 40) as i16, tick % 7 < 4)),
                (
                    1,
                    command(-wobble * 90, -((tick * 30) as i16), tick % 5 < 3),
                ),
            ];
            course.step(rules, &commands, MSEC, tick);
        }
        course
    };
    let (race, turret) = (race(), turret());
    let (a, b) = (play(&race), play(&race));
    let (c, d) = (play(&turret), play(&turret));
    check(
        a == b && c == d && (!c.bullets.is_empty() || c.facts.len() > 1),
        format!(
            "race alike {}, turret alike {}, {} hits heard",
            a == b,
            c == d,
            c.facts.len()
        ),
    )
}

/// Where the race's next module is (`StartNewModule` from the pad's hook).
const NEXT: &str = "probe_next";

/// The scripts the world probe's race runs: its player's heartbeat counts
/// `LAPS` and sets its acceleration; its pad hook starts `NEXT`.
fn race_scripts() -> crate::script::Library {
    const CATALOG: &str = r#"
// 0
void StartNewModule(string sModuleName, string sWayPoint = "", string sMovie1 = "", string sMovie2 = "", string sMovie3 = "", string sMovie4 = "", string sMovie5 = "", string sMovie6 = "");
// 1
int GetGlobalNumber(string sIdentifier);
// 2
void SetGlobalNumber(string sIdentifier, int nValue);
// 3
void SWMG_SetPlayerAccelerationPerSecond(float fAPS);
"#;
    let laps = || Op::Const(Const::String(b"LAPS".to_vec()));
    let mut heart = script_ncs::probes::Asm::default();
    heart
        .op(laps())
        .op(Op::Action {
            routine: 1,
            args: 1,
        })
        .op(Op::Const(Const::Int(1)))
        .op(Op::Add(Pair::IntInt))
        .op(laps())
        .op(Op::Action {
            routine: 2,
            args: 2,
        })
        .op(Op::Const(Const::Float(5.0)))
        .op(Op::Action {
            routine: 3,
            args: 1,
        })
        .op(Op::Retn);
    let mut pad = script_ncs::probes::Asm::default();
    for _ in 0..7 {
        pad.op(Op::Const(Const::String(Vec::new())));
    }
    pad.op(Op::Const(Const::String(NEXT.as_bytes().to_vec())))
        .op(Op::Action {
            routine: 0,
            args: 8,
        })
        .op(Op::Retn);
    crate::probe::library(CATALOG, [("heart", heart.build()), ("pad", pad.build())])
}

/// A two-seat race on the proving ground, the games' way: each run's
/// heartbeat counts `LAPS` in its own globals, the real one untouched
/// while they race; the first seat runs onto its pad and finishes, its
/// `StartNewModule` held; the second steers wide of its pad and never
/// finishes, so the race leaves `LEAVE_WAIT_MS` after the first finished,
/// not before, the first's count made the real one as it leaves.
fn globals_apart() -> Result<Result<String, String>, String> {
    let rules = race();
    let level = crate::determinism::proving_ground()
        .with_minigame(rules.clone())
        .with_scripts(race_scripts());
    let config = WorldConfig {
        seats: 2,
        ..WorldConfig::one_seat()
    };
    let mut start = SimWorld::new(config, level.clone())
        .map_err(|why| why.to_string())?
        .snapshot();
    start.minigame = Some(Course::new(&rules));
    let mut world = SimWorld::from_snapshot(&start, level).map_err(|why| why.to_string())?;
    let mut input = TickInput::default();
    input.insert(Seat(0), command(0, 0, false));
    input.insert(Seat(1), command(-127, 0, false));
    let real = |world: &SimWorld| world.script_state().numbers.get("LAPS").copied();
    let own = |snapshot: &crate::Snapshot, run: usize| {
        let course = snapshot.minigame.as_ref()?.run(run)?;
        course.overlay.numbers.get("LAPS").copied()
    };
    let mut tick = start.tick;
    let mut step = |world: &mut SimWorld| {
        tick += 1;
        crate::step(world, tick, &input, MSEC, StepReason::Authority)
    };
    // Racing, before anyone finishes (a second in).
    let mut racing = step(&mut world);
    for _ in 0..124 {
        racing = step(&mut world);
    }
    let (real_racing, own_racing) = (real(&world), [own(&racing, 0), own(&racing, 1)]);
    // On until the first finishes.
    let mut finished = None;
    for _ in 0..500 {
        let snapshot = step(&mut world);
        if snapshot
            .minigame
            .as_ref()
            .is_some_and(|c| c.finished.is_some())
        {
            finished = Some(snapshot);
            break;
        }
    }
    let finished = finished.ok_or("the first seat never reached its pad")?;
    let course = finished.minigame.as_ref().ok_or("no course")?;
    let held = real(&world);
    let second = course.runs.first().and_then(|run| run.finished);
    let since = course.leaving.as_ref().map(|leaving| leaving.since);
    // Held until the wait is out.
    let steps = LEAVE_WAIT_MS / u64::from(MSEC);
    let mut went = None;
    let mut committed = (None, None);
    for k in 0..steps + 4 {
        let snapshot = step(&mut world);
        if snapshot.transition.is_some() {
            committed = (real(&world), own(&snapshot, 0));
            went = Some((k, snapshot.transition.map(|t| t.module)));
            break;
        }
    }
    let on_time = went
        .as_ref()
        .is_some_and(|(k, module)| *k + 4 >= steps && module.as_deref() == Some(NEXT));
    Ok(check(
        real_racing.is_none()
            && own_racing[0].is_some_and(|n| n >= 120)
            && own_racing[0] == own_racing[1]
            && held.is_none()
            // The winner's count (a byte global, wrapped many laps on)
            // made the real one as it leaves.
            && committed.0.is_some()
            && committed.0 == committed.1
            && second.is_none()
            && since.is_some()
            && finished.transition.is_none()
            && on_time,
        format!(
            "racing: real {real_racing:?}, own {own_racing:?}; first finished: real {held:?}, second {second:?}; left {went:?} steps after (wait {steps}), real {:?}, own {:?}",
            committed.0, committed.1
        ),
    ))
}

/// The turret's scripts for the lens probe: its player's heartbeat counts
/// `BEATS`, plays `compass` on `SWMG_GetPlayer()` and takes a hit point
/// from it.
fn turret_scripts() -> crate::script::Library {
    const CATALOG: &str = r#"
// 0
object SWMG_GetPlayer();
// 1
void SWMG_PlayAnimation(object oObject, string sAnimName, int bLooping = 1, int bQueue = 0, int bOverlay = 0);
// 2
void SetGlobalNumber(string sIdentifier, int nValue);
// 3
int GetGlobalNumber(string sIdentifier);
// 4
void SWMG_AdjustFollowerHitPoints(object oFollower, int nHP, int nAbsolute = 0);
"#;
    let beats = || Op::Const(Const::String(b"BEATS".to_vec()));
    let mut heart = script_ncs::probes::Asm::default();
    heart
        .op(beats())
        .op(Op::Action {
            routine: 3,
            args: 1,
        })
        .op(Op::Const(Const::Int(1)))
        .op(Op::Add(Pair::IntInt))
        .op(beats())
        .op(Op::Action {
            routine: 2,
            args: 2,
        })
        .op(Op::Const(Const::Int(0)))
        .op(Op::Const(Const::Int(0)))
        .op(Op::Const(Const::Int(1)))
        .op(Op::Const(Const::String(b"compass".to_vec())))
        .op(Op::Action {
            routine: 0,
            args: 0,
        })
        .op(Op::Action {
            routine: 1,
            args: 5,
        })
        .op(Op::Const(Const::Int(0)))
        .op(Op::Const(Const::Int(-1)))
        .op(Op::Action {
            routine: 0,
            args: 0,
        })
        .op(Op::Action {
            routine: 4,
            args: 3,
        })
        .op(Op::Retn);
    crate::probe::library(CATALOG, [("heart", heart.build())])
}

/// Two seats at the turret, the games' way: the player's heartbeat runs for
/// the first seat and again through the gunner's lens, each playing its
/// `compass` where that seat sees it (the player's clips, the gunner's) and
/// counting `BEATS` where that seat keeps it (the real global, the lens's);
/// the hull's hit points go once a beat, the lens writing nothing but its
/// clips.
fn lens_compass() -> Result<Result<String, String>, String> {
    let mut rules = turret();
    rules.player.hooks = Hooks::from_labels(&[("OnHeartbeat".into(), "heart".into())]);
    let level = crate::determinism::proving_ground()
        .with_minigame(rules.clone())
        .with_scripts(turret_scripts());
    let config = WorldConfig {
        seats: 2,
        ..WorldConfig::one_seat()
    };
    let mut start = SimWorld::new(config, level.clone())
        .map_err(|why| why.to_string())?
        .snapshot();
    start.minigame = Some(Course::new(&rules));
    let hull = start
        .minigame
        .as_ref()
        .map_or(0, |course| course.movers[0].hp);
    let mut world = SimWorld::from_snapshot(&start, level).map_err(|why| why.to_string())?;
    let mut input = TickInput::default();
    input.insert(Seat(0), command(0, 0, false));
    input.insert(Seat(1), command(0, 0x2000, false));
    let mut last = None;
    for tick in 1..=10 {
        last = Some(crate::step(
            &mut world,
            start.tick + tick,
            &input,
            MSEC,
            StepReason::Authority,
        ));
    }
    let course = last
        .and_then(|snapshot| snapshot.minigame)
        .ok_or("no course")?;
    let named = |anims: &[super::Anim]| anims.iter().any(|anim| anim.name == "compass");
    let gunner = course.gunners.first().ok_or("no gunner")?;
    let player = named(&course.movers[0].anims);
    let lens = named(&gunner.anims);
    let real = world.script_state().numbers.get("BEATS").copied();
    let kept = gunner.overlay.numbers.get("BEATS").copied();
    let seen = named(&course.lens_anims(&rules, 1));
    let lost = hull - course.movers[0].hp;
    Ok(check(
        player
            && lens
            && seen
            && real.is_some_and(|beats| beats > 0 && beats == lost)
            && kept.is_some(),
        format!(
            "compass on the player {player}, on the lens {lens}, seen by the gunner {seen}; BEATS real {real:?}, the lens's {kept:?}; hull lost {lost}"
        ),
    ))
}

/// A sound object a run's scripts started and pitched is so for that run
/// only, over the world's own (its start counted on from the world's).
fn voices_apart() -> Result<String, String> {
    let rules = race();
    let mut course = Course::new(&rules);
    run(
        &mut course,
        &rules,
        &[(0, command(0, 0, false)), (1, command(0, 0, false))],
        1,
    );
    let playing = crate::snapshot::sound::PLAYING;
    course.runs[0].voice(
        4,
        super::Voice {
            object: 4,
            state: Some(playing),
            volume: None,
            pitch: Some(130),
            plays: 0,
            fade: None,
        },
        true,
    );
    let world = crate::ObjectState {
        plays: 3,
        ..crate::ObjectState::default()
    };
    let (own, other) = (course.runs[0].voiced(4, world), course.voiced(4, world));
    check(
        own.state == playing && own.pitch == 130 && own.plays == 4 && other == world,
        format!(
            "the run's: state {}, pitch {}, plays {}; the other's as the world's {}",
            own.state,
            own.pitch,
            own.plays,
            other == world
        ),
    )
}
