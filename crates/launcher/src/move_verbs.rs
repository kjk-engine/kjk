//! `kjk move …`: movement's evidence without a window. `run` steps a
//! scripted walk through a stage and writes its track; `measure` prints
//! the measurement scenarios (`sim::measure`: speeds, jumps, steps,
//! slopes, crouching, rolls, drops); `trig` checks the sim's sine and
//! cosine against `f64` over every angle a command can carry. Each writes
//! `facts.json` into a run folder.

use std::process::ExitCode;
use std::time::Instant;

use collision::UNIT;
use sim::measure::player;
use sim::moves::Script;
use sim::motion::AnimTable;
use sim::{Census, Recorder, Seat, SimWorld, Snapshot, StepReason, TickInput, TickRate, step};

use crate::lifecycle::Installer;
use crate::stage::Stage;

/// One player's state after each tick of a run, from tick 0, and the
/// modules it went through.
pub struct Track {
    pub path: sim::measure::Track,
    pub installs: Vec<Installed>,
    /// Doors that changed state: (ms, module, tag, state).
    pub doors: Vec<(u64, String, String, u8)>,
    /// The run as a recording, and the hash of its last snapshot.
    pub recording: sim::Recording,
}

/// Each door's state in `snapshot`: (entity, object index, state).
fn door_states(snapshot: &Snapshot) -> Vec<(sim::SimId, u32, u8)> {
    snapshot
        .entities
        .iter()
        .filter_map(|entity| match (entity.kind, entity.object) {
            (sim::Kind::Object(sim::module::ObjectKind::Door, index), Some(object)) => {
                Some((entity.id, index, object.state))
            }
            _ => None,
        })
        .collect()
}

/// A module installed on the way: when, where to, and the entities before
/// and after.
pub struct Installed {
    pub ms: u64,
    pub tick: u64,
    pub from: String,
    pub module: String,
    pub arrival: String,
    pub before: Census,
    pub after: Census,
    pub prepare_ms: f64,
    pub hash: String,
    /// Where the player stands after it (box origin, units).
    pub origin: [f32; 3],
}

/// Steps `script` for seat 0 of `world` for `ms` of sim time; each module
/// transition it walks into is planned by `installer` and committed on the
/// spot, as a host would after its loading screen.
pub fn drive_world(
    mut world: SimWorld,
    installer: Option<&Installer>,
    script: &Script,
    ms: u64,
) -> Result<Track, String> {
    let rate = TickRate::default();
    let first = world.snapshot();
    let mut recorder = Recorder::new(rate.hz(), first.clone(), world.level().id(), 100)
        .with_script(world.script_state().clone());
    let mut states = vec![(0, player(&first))];
    let mut installs = Vec::new();
    let mut doors = Vec::new();
    let mut seen = door_states(&first);
    let mut last = first;
    let mut tick = last.tick;
    while rate.time_ms(tick) < ms {
        tick += 1;
        let start = rate.time_ms(tick - 1);
        let mut input = TickInput::default();
        input.insert(Seat(0), script.command(start));
        last = step(
            &mut world,
            tick,
            &input,
            rate.msec(tick),
            StepReason::Authority,
        );
        recorder.record(rate.msec(tick), &input, &last);
        let now = door_states(&last);
        if let Some(module) = world.level().module() {
            let objects = sim::Instances::in_snapshot(world.level(), &last);
            for (&(_, index, state), &(_, _, before)) in now.iter().zip(&seen) {
                if state != before {
                    doors.push((
                        last.time_ms,
                        module.name.clone(),
                        objects
                            .get(index)
                            .map_or_else(String::new, |door| door.tag.clone()),
                        state,
                    ));
                }
            }
        }
        seen = now;
        if let (Some(transition), Some(installer)) = (&last.transition, installer) {
            let started = Instant::now();
            let request = installer.request_from(&last, &transition.module, Some(&transition.tag));
            let prepared = installer.prepare(&request, false)?;
            let prepare_ms = started.elapsed().as_secs_f64() * 1000.0;
            let before = last.census();
            let from = last
                .place
                .as_ref()
                .map_or_else(String::new, |place| place.module.clone());
            let announcement = prepared
                .plan
                .commit(&mut world)
                .map_err(|error| error.to_string())?;
            last = world.snapshot();
            seen = door_states(&last);
            recorder.install(announcement.clone());
            installs.push(Installed {
                ms: last.time_ms,
                tick,
                from,
                module: announcement.module.clone(),
                arrival: announcement.arrival.clone(),
                before,
                after: last.census(),
                prepare_ms,
                hash: announcement.after.to_string(),
                origin: player(&last).origin,
            });
        }
        states.push((last.time_ms, player(&last)));
    }
    Ok(Track {
        path: sim::measure::Track {
            hz: rate.hz(),
            states,
            // A module installed on the way is a move, not a flight.
            moved_at: installs.iter().map(|install| install.ms).collect(),
        },
        installs,
        doors,
        recording: recorder.finish(&last),
    })
}

/// A track's states as JSON, every `every`th.
fn track_json(track: &sim::measure::Track, every: usize) -> serde_json::Value {
    serde_json::Value::Array(
        track
            .states
            .iter()
            .step_by(every.max(1))
            .map(|(ms, s)| {
                let r = |v: f32| (v * 100.0).round() / 100.0;
                serde_json::json!({
                    "ms": ms,
                    "origin": s.origin.map(r),
                    "velocity": s.velocity.map(r),
                    "view": s.view.map(r),
                    "ground": s.ground.is_some(),
                    "ducked": s.ducked(),
                    "legs": s.legs.anim.name(),
                    "torso": s.torso.anim.name(),
                    "legs_timer": s.legs.timer,
                    "move_dir": s.push_dir,
                    "events": s.events,
                })
            })
            .collect(),
    )
}

/// `kjk move run`: the scripted walk through a stage, through every module
/// transition it walks into; its track, flights, animations and installs,
/// and a replay of it that must end the same.
pub fn run(stage: &Stage, anims_note: &str, script_text: &str) -> ExitCode {
    let script = match Script::parse(script_text) {
        Ok(script) => script,
        Err(error) => {
            eprintln!("--moves: {error}");
            return ExitCode::from(2);
        }
    };
    outln!("move run: {} ({}), {}", stage.name, script_text, anims_note);
    let track = stage.world(1).and_then(|(world, _)| {
        drive_world(
            world,
            stage.installer.as_deref(),
            &script,
            script.length_ms() + 500,
        )
    });
    let track = match track {
        Ok(track) => track,
        Err(error) => {
            eprintln!("move run: {error}");
            return ExitCode::FAILURE;
        }
    };
    for install in &track.installs {
        outln!(
            "  {} ms (tick {}): {} -> {} at {:?}, prepared in {:.0} ms; entities {} live + {} left behind -> {} + {}; origin ({:.1}, {:.1}, {:.1}) units",
            install.ms,
            install.tick,
            install.from,
            install.module,
            install.arrival,
            install.prepare_ms,
            install.before.live_total(),
            install.before.stashed_total(),
            install.after.live_total(),
            install.after.stashed_total(),
            install.origin[0],
            install.origin[1],
            install.origin[2],
        );
    }
    // The run again from its recording: every install replanned from its
    // announcement, every checkpoint and the end the same.
    let replay = track
        .recording
        .play_with(&stage.level, &mut |announcement| {
            stage
                .installer
                .as_ref()
                .ok_or("no installer".to_string())
                .and_then(|installer| installer.level_for(announcement))
        });
    let replayed = match &replay {
        Ok(end) if end.hash() == track.recording.end => "same".to_string(),
        Ok(_) => "DIFFERENT".to_string(),
        Err(divergence) => format!(
            "DIVERGED at tick {}{}",
            divergence.tick,
            divergence
                .reason
                .as_ref()
                .map_or_else(String::new, |reason| format!(" ({reason})"))
        ),
    };
    outln!(
        "  replayed from its recording ({} installs): {replayed}",
        track.recording.installs.len()
    );
    for (ms, module, tag, state) in &track.doors {
        outln!(
            "  {ms} ms: {module} door {tag} {}",
            if *state == 0 { "closed" } else { "opened" }
        );
    }
    let last = track.path.states.len() - 1;
    let end = track.path.feet(last);
    outln!(
        "  {} ticks; ends at ({:.1}, {:.1}, {:.1}) units, ({:.2}, {:.2}, {:.2}) m",
        last,
        end[0],
        end[1],
        end[2],
        end[0] * UNIT,
        end[1] * UNIT,
        end[2] * UNIT
    );
    for (take_off, land, apex, distance) in track.path.flights() {
        outln!(
            "  in the air {take_off}..{land} ms ({} ms): highest {apex:.1} units over take-off, {distance:.0} units along",
            land - take_off
        );
    }
    let legs = track.path.legs();
    outln!(
        "  legs: {}",
        legs.iter()
            .map(|(ms, name)| format!("{ms} {name}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let census = |census: &Census| {
        serde_json::json!({
            "live": census.live,
            "left_behind": census.stashed,
        })
    };
    write_facts(serde_json::json!({
        "verb": "move run",
        "stage": stage.name,
        "moves": script_text,
        "anims": anims_note,
        "hz": track.path.hz,
        "installs": track.installs.iter().map(|i| serde_json::json!({
            "ms": i.ms,
            "tick": i.tick,
            "from": i.from,
            "module": i.module,
            "arrival": i.arrival,
            "prepare_ms": i.prepare_ms,
            "before": census(&i.before),
            "after": census(&i.after),
            "hash": i.hash,
            "origin_units": i.origin,
        })).collect::<Vec<_>>(),
        "replay": replayed,
        "doors": track.doors.iter().map(|(ms, module, tag, state)| serde_json::json!({
            "ms": ms, "module": module, "tag": tag, "state": state,
        })).collect::<Vec<_>>(),
        "end_hash": track.recording.end.to_string(),
        "end_units": end,
        "flights": track.path.flights().iter().map(|f| serde_json::json!({
            "take_off_ms": f.0, "land_ms": f.1, "apex_units": f.2, "distance_units": f.3,
        })).collect::<Vec<_>>(),
        "legs": legs.iter().map(|(ms, n)| serde_json::json!([ms, n])).collect::<Vec<_>>(),
        "track": track_json(&track.path, 1),
    }));
    ExitCode::SUCCESS
}

/// `kjk move measure`: speeds, jumps, steps, slopes, crouching, rolls and
/// drops, each on a world of its own (`sim::measure`).
pub fn measure(anims: (AnimTable, String)) -> ExitCode {
    let table = anims.0;
    outln!("move measure: {}", anims.1);
    let measured = sim::measure::measure(&table);
    let mut facts = serde_json::Map::new();

    outln!("speeds (units/s): top, mean over the last second held; ms to 95%; units to stop");
    let mut speeds = Vec::new();
    for s in &measured.speeds {
        outln!(
            "  {:<14} top {:6.1}  mean {:6.1}  (JA's number {:6.1})  95% at {:4} ms  stops in {:5.1}",
            s.name,
            s.top,
            s.mean,
            s.expected,
            s.rise_ms,
            s.stop
        );
        speeds.push(serde_json::json!({
            "move": s.name, "top": s.top, "mean_last_second": s.mean, "expected": s.expected,
            "ms_to_95": s.rise_ms, "stop_units": s.stop, "curve": s.curve,
        }));
    }
    facts.insert("speeds".into(), speeds.into());

    let (ja_apex, ja_air) = (measured.ja_apex, measured.ja_air_ms);
    outln!(
        "jumps: JA's steps give an apex of {ja_apex:.2} units ({:.3} m) and {ja_air} ms in the air",
        ja_apex * UNIT
    );
    let mut jumps = Vec::new();
    for jump in &measured.jumps {
        outln!(
            "  {:<9} apex {:6.2} units ({:.3} m), {} ms in the air, {:6.1} units along",
            jump.name,
            jump.apex,
            jump.apex * UNIT,
            jump.air_ms,
            jump.distance
        );
        jumps.push(serde_json::json!({
            "jump": jump.name, "apex_units": jump.apex, "air_ms": jump.air_ms,
            "distance_units": jump.distance,
            "ja_apex_units": ja_apex, "ja_air_ms": ja_air,
        }));
    }
    facts.insert(
        "standing_jump_arc_units".into(),
        measured.standing_jump_arc.clone().into(),
    );
    facts.insert("jumps".into(), jumps.into());

    outln!("steps (run at a step 2 m ahead):");
    let mut steps = Vec::new();
    for s in &measured.steps {
        outln!(
            "  {:5.1} units: {} (highest soles {:.1} units; step event {}; ends at y {:.2} m)",
            s.rise,
            if s.walked_up { "walked up" } else { "blocked" },
            s.highest,
            s.step_event,
            s.end_y,
        );
        steps.push(serde_json::json!({
            "rise_units": s.rise, "walked_up": s.walked_up, "highest_units": s.highest,
            "step_event": s.step_event,
        }));
    }
    facts.insert("steps".into(), steps.into());

    outln!("slopes (run 3 s at a ramp rising 3 m ahead over 4 m, then back):");
    let mut slopes = Vec::new();
    for s in &measured.slopes {
        outln!(
            "  {:4.1}°: {} (highest {:.2} m of {:.2}); up the ramp {:5.1} units/s level; {} ms off the ground coming down",
            s.degrees,
            if s.reached_top {
                "reached the top"
            } else {
                "slid back"
            },
            s.top,
            s.rise,
            s.up_speed,
            s.air_down_ms
        );
        slopes.push(serde_json::json!({
            "degrees": s.degrees, "rise_m": s.rise, "reached_top": s.reached_top,
            "highest_m": s.top, "level_speed_up": s.up_speed,
            "air_ms_coming_down": s.air_down_ms,
        }));
    }
    facts.insert("slopes".into(), slopes.into());

    for roll in &measured.rolls {
        outln!(
            "roll: {}..{} ms ({} ms held, BOTH_ROLL_F {} ms), {:.0} units ({:.2} m)",
            roll.from,
            roll.to,
            roll.to - roll.from,
            measured.roll_anim_ms,
            roll.distance,
            roll.distance * UNIT
        );
        facts.insert(
            "roll".into(),
            serde_json::json!({ "from_ms": roll.from, "to_ms": roll.to, "units": roll.distance }),
        );
    }
    facts.insert("roll_legs".into(), serde_json::json!(measured.roll_legs));

    outln!("drops (off a ledge, running on or letting go at the edge):");
    let mut drops = Vec::new();
    for d in &measured.drops {
        let landed_with = d.landed_with.map(|(ms, name)| format!("{name} at {ms} ms"));
        outln!(
            "  {:5.1} units ({}): legs {}; {}; hard (fall event) {}",
            d.height,
            d.keys,
            d.legs
                .iter()
                .map(|(_, n)| *n)
                .collect::<Vec<_>>()
                .join(" > "),
            landed_with.as_deref().unwrap_or("no landing animation"),
            d.fall_event
        );
        drops.push(serde_json::json!({
            "height_units": d.height,
            "moves": d.keys,
            "legs": d.legs,
            "fall_event": d.fall_event,
        }));
    }
    facts.insert("drops".into(), drops.into());

    let tunnel = &measured.tunnel;
    outln!(
        "tunnel (43 units): let go inside, stays crouched until out: {}; ends at y {:.2} m, standing: {}",
        tunnel.stays_crouched_inside,
        tunnel.end[1] * UNIT,
        tunnel.standing_at_end
    );
    facts.insert(
        "tunnel".into(),
        serde_json::json!({
            "stays_crouched_inside": tunnel.stays_crouched_inside,
            "end_units": tunnel.end,
        }),
    );

    let mut facts = serde_json::Value::Object(facts);
    facts["verb"] = "move measure".into();
    facts["anims"] = anims.1.into();
    write_facts(facts);
    ExitCode::SUCCESS
}

/// `kjk move trig`: the sim's sine and cosine against `f64` over all
/// 65 536 command angles, and a million other angles.
pub fn trig() -> ExitCode {
    let error = |degrees: f32| {
        let (s, c) = sim::math::sin_cos_deg(degrees);
        let r = f64::from(degrees).to_radians();
        let error = (f64::from(s) - r.sin())
            .abs()
            .max((f64::from(c) - r.cos()).abs());
        (error, degrees)
    };
    let most = |all: &mut dyn Iterator<Item = (f64, f32)>| {
        all.fold((0.0f64, 0.0f32), |a, b| if b.0 > a.0 { b } else { a })
    };
    let shorts = most(&mut (i16::MIN..=i16::MAX).map(|s| error(sim::math::short_to_degrees(s))));
    let worst =
        most(&mut (0..1_000_000u32).map(|i| error(-720.0 + i as f32 * 1440.0 / 1_000_000.0)));
    outln!(
        "trig: sin and cos within {:.2e} of f64 over every command angle (worst at {}°); within {:.2e} over a million angles in ±720° (worst at {}°)",
        shorts.0,
        shorts.1,
        worst.0,
        worst.1
    );
    write_facts(serde_json::json!({
        "verb": "move trig",
        "command_angles_max_error": shorts.0,
        "command_angles_worst_degrees": shorts.1,
        "wide_max_error": worst.0,
        "wide_worst_degrees": worst.1,
    }));
    ExitCode::SUCCESS
}

/// Writes `facts` to `facts.json` in a new run folder, and says where.
fn write_facts(facts: serde_json::Value) {
    let folder = match crate::runtime::create_run_folder() {
        Ok(folder) => folder,
        Err(error) => {
            eprintln!("cannot create the run folder: {error}");
            return;
        }
    };
    let mut facts = facts;
    facts["git"] = env!("KJK_GIT_SHA").into();
    let path = folder.join("facts.json");
    let text = serde_json::to_string_pretty(&facts).expect("json values serialize");
    match std::fs::write(&path, text) {
        Ok(()) => eprintln!("facts: {}", path.display()),
        Err(error) => eprintln!("{}: {error}", path.display()),
    }
}
