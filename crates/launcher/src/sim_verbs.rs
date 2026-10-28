//! `kjk sim …`: the sim funnel's evidence from the command line, with no
//! window. Every verb steps the placeholder world with scripted commands
//! (the pilot), prints what it found and writes it to `facts.json` in a run
//! folder under `kjk-artifacts/runs/`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use sim::{
    Accumulator, Level, Recorder, Recording, Save, ScriptVmState, SimRng, SimWorld, StepReason,
    TickRate, WorldConfig,
};

use crate::runtime::write_facts;
use sim::determinism::{RECORD_EVERY, ROLLBACK_TICKS, Run};
pub use sim::determinism::{Setup, pilot};

fn verdict(same: bool) -> &'static str {
    if same { "same" } else { "DIFFERENT" }
}

/// Steps the same inputs several ways and compares snapshot hashes tick by
/// tick (`sim::determinism`); exits 1 if any way that must agree does not.
/// With `expect`, the final hash must also start with it: the check that
/// another platform reaches the very same state.
pub fn determinism(setup: &Setup, ticks: u64, expect: Option<&str>) -> ExitCode {
    let started = Instant::now();
    let seats = setup.world.seats;
    let hz = setup.rate.hz();
    outln!(
        "determinism: {ticks} ticks at {hz} Hz; world seed {}, pilot seed {}, {seats} seats, {} wanderers",
        setup.world.seed,
        setup.pilot,
        setup.world.wanderers
    );
    let report = sim::determinism::run(setup, ticks);

    outln!("  {:>6}  {:<16}  {:<16}", "tick", "run A", "run B");
    for checkpoint in &report.checkpoints {
        outln!(
            "  {:>6}  {}  {}",
            checkpoint.tick,
            checkpoint.a.short(),
            checkpoint.b.short()
        );
    }
    outln!("  final  {}", report.final_hash);
    let total = report.total;
    outln!(
        "  B  fresh world, same inputs:            {} ({} of {total} tick hashes equal)",
        verdict(report.fresh_same()),
        report.fresh_equal
    );
    outln!(
        "  C  inputs handed over in reverse order: {} ({} of {ticks})",
        verdict(report.reversed_same()),
        report.reversed_equal
    );
    outln!(
        "  D  saved at tick {}, loaded, stepped on: {} ({} of {}); save {} bytes",
        report.middle,
        verdict(report.loaded_same()),
        report.loaded_equal,
        report.loaded_total,
        report.save_bytes
    );
    outln!(
        "  E  recording replayed:                  {}; recording {} bytes",
        match &report.replay {
            Ok(_) if report.replay_same() => "same".to_string(),
            Ok(_) => "DIFFERENT".to_string(),
            Err(divergence) => format!("DIVERGED at tick {}", divergence.tick),
        },
        report.recording_bytes
    );
    outln!(
        "  F  another world seed:                  {}",
        if report.other_seed_differs {
            "differs, as it must"
        } else {
            "SAME (the hash does not see the seed)"
        }
    );
    outln!(
        "  G  {} rollbacks predicting {ROLLBACK_TICKS} ticks: {} match the authority; \
         {} windows hold an authority-only event; unexpected: {}",
        report.rollbacks,
        report.rollbacks_matched,
        report.rollbacks - report.rollbacks_matched,
        report.rollbacks_unexpected.len()
    );
    let ground = &report.ground;
    outln!(
        "  {} installed after tick {} ({} brushes, collision {}): door opened {}; {} transition asks",
        sim::determinism::GROUND,
        ground.installed_after,
        ground.brushes,
        &ground.collision_digest[..16],
        ground
            .door_opened_at
            .map_or_else(|| "NEVER".to_string(), |tick| format!("at tick {tick}")),
        ground.asks
    );
    outln!(
        "  {} entities at the end; {:.1} s",
        report.entities_end,
        started.elapsed().as_secs_f64()
    );

    let final_hash = report.final_hash.to_string();
    if let Some(prefix) = expect {
        outln!(
            "  expected final {prefix}: {}",
            if report.reaches(prefix) {
                "reached"
            } else {
                "NOT reached"
            }
        );
    }
    let passed = report.passed(expect);
    write_facts(serde_json::json!({
        "verb": "sim determinism",
        "ticks": ticks,
        "hz": hz,
        "world_seed": setup.world.seed,
        "pilot_seed": setup.pilot,
        "seats": seats,
        "wanderers": setup.world.wanderers,
        "checkpoints": report.checkpoints.iter().map(|checkpoint| serde_json::json!({
            "tick": checkpoint.tick,
            "a": checkpoint.a.to_string(),
            "b": checkpoint.b.to_string(),
        })).collect::<Vec<_>>(),
        "final": final_hash,
        "expected": expect,
        "fresh_equal": report.fresh_equal,
        "reversed_equal": report.reversed_equal,
        "loaded_equal": report.loaded_equal,
        "loaded_total": report.loaded_total,
        "replay_same": report.replay_same(),
        "other_seed_differs": report.other_seed_differs,
        "rollbacks": report.rollbacks,
        "rollbacks_matched": report.rollbacks_matched,
        "rollbacks_unexpected": report.rollbacks_unexpected,
        "save_bytes": report.save_bytes,
        "recording_bytes": report.recording_bytes,
        "entities_end": report.entities_end,
        "ground": {
            "installed_after": report.ground.installed_after,
            "brushes": report.ground.brushes,
            "collision_digest": report.ground.collision_digest,
            "door_opened_at": report.ground.door_opened_at,
            "asks": report.ground.asks,
        },
        "passed": passed,
    }));
    if passed {
        outln!("determinism: passed");
        ExitCode::SUCCESS
    } else {
        outln!("determinism: FAILED");
        ExitCode::FAILURE
    }
}

/// Runs the pilot for `ticks` ticks and writes the recording to `file`.
pub fn record(setup: &Setup, ticks: u64, file: &Path) -> ExitCode {
    let mut run = Run::new(setup);
    let mut recorder = Recorder::new(
        setup.rate.hz(),
        run.last.clone(),
        Level::arena().id(),
        RECORD_EVERY,
    );
    for tick in 1..=ticks {
        let input = pilot(setup.pilot, tick, setup.world.seats);
        let snapshot = run.step(&input, StepReason::Authority);
        recorder.record(setup.rate.msec(tick), &input, snapshot);
    }
    let recording = recorder.finish(&run.last);
    let bytes = recording.encode();
    if let Err(error) = std::fs::write(file, &bytes) {
        eprintln!("{}: {error}", file.display());
        return ExitCode::FAILURE;
    }
    outln!(
        "recorded {ticks} ticks at {} Hz to {} ({} bytes); ends at {}",
        setup.rate.hz(),
        file.display(),
        bytes.len(),
        recording.end
    );
    write_facts(serde_json::json!({
        "verb": "sim record",
        "file": file.display().to_string(),
        "ticks": ticks,
        "hz": setup.rate.hz(),
        "bytes": bytes.len(),
        "end": recording.end.to_string(),
    }));
    ExitCode::SUCCESS
}

/// Plays a recording back and compares it with the hashes it carries.
pub fn replay(discovery: &vfs::Discovery, file: &Path) -> ExitCode {
    let recording = match std::fs::read(file)
        .map_err(|error| error.to_string())
        .and_then(|bytes| Recording::decode(&bytes).map_err(|error| error.to_string()))
    {
        Ok(recording) => recording,
        Err(error) => {
            eprintln!("{}: {error}", file.display());
            return ExitCode::FAILURE;
        }
    };
    let started = Instant::now();
    // A recording made in a module replays in it, and in each module it
    // went to, planned here from its announcement.
    let mut installers = Installers::default();
    let level = match installers.level_named(
        discovery,
        &recording.level,
        recording.start.place.as_ref(),
        recording.start.seats(),
    ) {
        Ok(level) => level,
        Err(error) => {
            eprintln!("{}: refused: {error}", file.display());
            return ExitCode::FAILURE;
        }
    };
    let mut seats = BTreeMap::new();
    let result = recording.play_watched(
        &level,
        &mut |announcement| {
            installers
                .get(discovery, &announcement.game)?
                .level_for(announcement)
        },
        &mut |snapshot| watch_seats(&mut seats, snapshot),
    );
    let seconds = started.elapsed().as_secs_f64();
    let ticks = recording.ticks.len();
    let commands: usize = recording.ticks.iter().map(|tick| tick.input.len()).sum();
    let sim_ms: u64 = recording
        .ticks
        .iter()
        .map(|tick| u64::from(tick.msec))
        .sum();
    outln!(
        "replay: {ticks} ticks ({:.1} s of sim time at {} Hz, {commands} commands) from tick {}, {} checkpoints",
        sim_ms as f64 / 1000.0,
        recording.hz,
        recording.start.tick,
        recording.checkpoints.len()
    );
    for (seat, seen) in &seats {
        let taken: Vec<String> = seen
            .taken
            .iter()
            .map(|(result, count)| format!("{result} {count}"))
            .collect();
        outln!(
            "  seat {seat}: lowest HP {:.2}, lowest bar {:.2}; seen: {}; taken: {}; blows held for its grace: {}",
            f64::from(seen.lowest_hp) / 100.0,
            f64::from(seen.lowest_bar) / 100.0,
            if seen.seen.is_empty() {
                "nothing".to_string()
            } else {
                seen.seen.iter().cloned().collect::<Vec<_>>().join(", ")
            },
            if taken.is_empty() {
                "nothing".to_string()
            } else {
                taken.join(", ")
            },
            seen.held.len()
        );
    }
    let (verdict, code) = match &result {
        Ok(last) => {
            outln!("  ends at {} as recorded; {seconds:.2} s", last.hash());
            ("same", ExitCode::SUCCESS)
        }
        Err(divergence) => {
            outln!(
                "  DIVERGED by tick {}: recorded {}, replayed {}{}",
                divergence.tick,
                divergence.recorded,
                divergence.replayed,
                divergence
                    .reason
                    .as_ref()
                    .map_or_else(String::new, |reason| format!(" ({reason})"))
            );
            ("diverged", ExitCode::FAILURE)
        }
    };
    write_facts(serde_json::json!({
        "verb": "sim replay",
        "file": file.display().to_string(),
        "ticks": ticks,
        "hz": recording.hz,
        "commands": commands,
        "result": verdict,
        "end": recording.end.to_string(),
        "divergence_tick": result.as_ref().err().map(|divergence| divergence.tick),
        "seats": seats.iter().map(|(seat, seen)| serde_json::json!({
            "seat": seat, "lowest_hp": seen.lowest_hp, "lowest_bar": seen.lowest_bar,
            "seen": seen.seen, "taken": seen.taken, "held": seen.held.len(),
        })).collect::<Vec<_>>(),
    }));
    code
}

/// What a seat did in a replayed fight: its lowest hit points and bar
/// (hundredths), the powers and specials seen running, what was done to it
/// (its events by result, each once by sequence and tick), and the blows
/// held for its grace (I25, C19; by striker and tick).
struct Seen {
    lowest_hp: i32,
    lowest_bar: i32,
    seen: BTreeSet<String>,
    taken: BTreeMap<String, u32>,
    events: BTreeSet<(u16, u64)>,
    held: BTreeSet<(u32, u64)>,
}

/// Adds `snapshot`'s seats to what they were seen doing.
fn watch_seats(seats: &mut BTreeMap<u8, Seen>, snapshot: &sim::Snapshot) {
    let now = snapshot.time_ms;
    for entity in &snapshot.entities {
        let (sim::Kind::Avatar(seat), Some(fighter)) = (entity.kind, entity.fighter) else {
            continue;
        };
        let seen = seats.entry(seat.0).or_insert(Seen {
            lowest_hp: fighter.hp,
            lowest_bar: fighter.fp,
            seen: BTreeSet::new(),
            taken: BTreeMap::new(),
            events: BTreeSet::new(),
            held: BTreeSet::new(),
        });
        seen.lowest_hp = seen.lowest_hp.min(fighter.hp);
        seen.lowest_bar = seen.lowest_bar.min(fighter.fp);
        for event in fighter.events.iter().flatten() {
            if seen.events.insert((event.sequence, event.tick)) {
                let result = format!("{:?}", event.result).to_lowercase();
                *seen.taken.entry(result).or_insert(0) += 1;
            }
        }
        seen.held.extend(
            fighter
                .grace
                .iter()
                .flatten()
                .map(|blow| (blow.by.0, blow.tick)),
        );
        let force = &fighter.force;
        let shooting = &fighter.shooting;
        let running = [
            (force.speed_until > now, "speed"),
            (force.protect_until > now, "protect"),
            (force.absorb_until > now, "absorb"),
            (force.trick_until > now, "mind trick"),
            (force.raging(now) > 0, "rage"),
            (force.sight_until > now, "sight"),
            (force.jumping, "force jump"),
            (shooting.charging_from.is_some(), "Power Blast charge"),
            (shooting.burst > 0, "Rapid Shot burst"),
            (shooting.aiming, "Sniper Shot aim"),
            (fighter.saber.in_special(), "saber special"),
        ];
        seen.seen.extend(
            running
                .iter()
                .filter(|(on, _)| *on)
                .map(|(_, name)| (*name).to_string()),
        );
        if let Some(channel) = force.channel {
            seen.seen
                .insert(format!("{:?}", channel.power).to_lowercase());
        }
        if let Some((gesture, _)) = force.hand.filter(|(_, until)| *until > now) {
            seen.seen.insert(format!("{gesture:?} hand").to_lowercase());
        }
    }
}

/// Runs the pilot for `ticks` ticks and saves; then steps `then` more ticks
/// and prints the hash a load of this save must reach after as many.
pub fn save(setup: &Setup, ticks: u64, then: u64, file: &Path) -> ExitCode {
    let mut run = Run::new(setup);
    for tick in 1..=ticks {
        run.step(
            &pilot(setup.pilot, tick, setup.world.seats),
            StepReason::Authority,
        );
    }
    let save = Save {
        hz: setup.rate.hz(),
        level: Level::arena().id(),
        snapshot: run.last.clone(),
        script: ScriptVmState::default(),
    };
    let bytes = save.encode();
    if let Err(error) = std::fs::write(file, &bytes) {
        eprintln!("{}: {error}", file.display());
        return ExitCode::FAILURE;
    }
    let saved = run.last.hash();
    for tick in ticks + 1..=ticks + then {
        run.step(
            &pilot(setup.pilot, tick, setup.world.seats),
            StepReason::Authority,
        );
    }
    outln!(
        "saved tick {ticks} to {} ({} bytes): {saved}",
        file.display(),
        bytes.len()
    );
    outln!("  {then} ticks later, without a load: {}", run.last.hash());
    write_facts(serde_json::json!({
        "verb": "sim save",
        "file": file.display().to_string(),
        "tick": ticks,
        "bytes": bytes.len(),
        "saved": saved.to_string(),
        "then": then,
        "then_hash": run.last.hash().to_string(),
    }));
    ExitCode::SUCCESS
}

/// Installers by game, each made once (its base archives opened once),
/// with the JA skeleton's animation lengths loaded once for all of them.
#[derive(Default)]
pub(crate) struct Installers {
    anims: Option<std::sync::Arc<sim::motion::AnimTable>>,
    made: Vec<std::sync::Arc<crate::lifecycle::Installer>>,
}

impl Installers {
    fn anims(&mut self, discovery: &vfs::Discovery) -> std::sync::Arc<sim::motion::AnimTable> {
        self.anims
            .get_or_insert_with(|| std::sync::Arc::new(crate::stage::anims(discovery).0))
            .clone()
    }

    /// The installer for `game` (`k1` or `tsl`).
    pub(crate) fn get(
        &mut self,
        discovery: &vfs::Discovery,
        game: &str,
    ) -> Result<std::sync::Arc<crate::lifecycle::Installer>, String> {
        let game = vfs::Game::from_key(game)
            .filter(|game| *game != vfs::Game::JediAcademy)
            .ok_or(format!("{game}: not a KOTOR game"))?;
        if let Some(made) = self.made.iter().find(|made| made.game == game) {
            return Ok(made.clone());
        }
        let anims = self.anims(discovery);
        let made = std::sync::Arc::new(crate::lifecycle::Installer::new(discovery, game, anims)?);
        self.made.push(made.clone());
        Ok(made)
    }

    /// The level a save or recording names, built here: the placeholder
    /// arena, the movement course, or its module, planned for `seats`
    /// players; refused unless it is the very level named
    /// (`LevelId::check`).
    pub(crate) fn level_named(
        &mut self,
        discovery: &vfs::Discovery,
        id: &sim::LevelId,
        place: Option<&sim::Place>,
        seats: u8,
    ) -> Result<Level, String> {
        let level = match place {
            None if id.name == "arena" => Level::arena(),
            None if id.name == "course" => Level::new(
                "course",
                std::sync::Arc::new(crate::course::course()),
                self.anims(discovery),
            ),
            None if id.name == "duel" => crate::stage::duel_level(self.anims(discovery)),
            None if id.name == "ring" => crate::stage::ring(&self.anims(discovery)),
            None => return Err(format!("{}: no such level without a module", id.name)),
            Some(place) => {
                let installer = self.get(discovery, &place.game)?;
                crate::lifecycle_verbs::level_of_place(&installer, place, seats)?
            }
        };
        id.check(&level).map_err(|error| error.to_string())?;
        Ok(level)
    }
}

/// What `kjk sim view --replay` runs a recording in: its own level, the
/// world at its start, and, when it is in a module or installs any, what
/// plans each module it went to (checked against the recording's
/// announcements as it replays).
pub fn replay_setup(
    discovery: &vfs::Discovery,
    recording: &Recording,
) -> Result<(Level, SimWorld, Option<crate::sim_host::Lifecycle>), String> {
    let mut installers = Installers::default();
    let level = installers.level_named(
        discovery,
        &recording.level,
        recording.start.place.as_ref(),
        recording.start.seats(),
    )?;
    let game = recording
        .start
        .place
        .as_ref()
        .map(|place| place.game.clone())
        .or_else(|| recording.installs.first().map(|a| a.game.clone()));
    let lifecycle = match game {
        None => None,
        Some(game) => {
            if recording.installs.iter().any(|a| a.game != game) {
                return Err("the recording installs modules of two games".into());
            }
            Some(crate::sim_host::Lifecycle {
                installer: installers.get(discovery, &game)?,
                body: None,
                first: None,
            })
        }
    };
    let start = SimWorld::from_snapshot(&recording.start, level.clone())
        .map_err(|error| error.to_string())?;
    Ok((level, start, lifecycle))
}

/// Loads a save, checks it encodes back to the same bytes, and steps on
/// with the pilot.
pub fn load(
    discovery: &vfs::Discovery,
    file: &Path,
    then: u64,
    pilot_seed: u64,
    seats: u8,
) -> ExitCode {
    let bytes = match std::fs::read(file) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("{}: {error}", file.display());
            return ExitCode::FAILURE;
        }
    };
    let save = match Save::decode(&bytes) {
        Ok(save) => save,
        Err(error) => {
            eprintln!("{}: {error}", file.display());
            return ExitCode::FAILURE;
        }
    };
    let Some(rate) = TickRate::new(save.hz) else {
        eprintln!("{}: tick rate {} is out of range", file.display(), save.hz);
        return ExitCode::FAILURE;
    };
    let round_trip = save.encode() == bytes;
    let level = match Installers::default().level_named(
        discovery,
        &save.level,
        save.snapshot.place.as_ref(),
        save.snapshot.seats(),
    ) {
        Ok(level) => level,
        Err(error) => {
            eprintln!("{}: refused: {error}", file.display());
            return ExitCode::FAILURE;
        }
    };
    let mut run = match SimWorld::from_snapshot(&save.snapshot, level) {
        Ok(world) => Run {
            world,
            rate,
            last: save.snapshot.clone(),
        },
        Err(error) => {
            eprintln!("{}: refused: {error}", file.display());
            return ExitCode::FAILURE;
        }
    };
    let loaded = run.last.hash();
    let from = run.last.tick;
    for tick in from + 1..=from + then {
        run.step(&pilot(pilot_seed, tick, seats), StepReason::Authority);
    }
    outln!(
        "loaded tick {from} at {} Hz from {} (level {}): {loaded}",
        save.hz,
        file.display(),
        save.level.name
    );
    outln!(
        "  encodes back to the same bytes: {}",
        if round_trip { "yes" } else { "NO" }
    );
    outln!("  {then} ticks later: {}", run.last.hash());
    write_facts(serde_json::json!({
        "verb": "sim load",
        "file": file.display().to_string(),
        "tick": from,
        "hz": save.hz,
        "loaded": loaded.to_string(),
        "round_trip": round_trip,
        "then": then,
        "then_hash": run.last.hash().to_string(),
    }));
    if round_trip {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Timing of candidate tick rates: what a step costs at several entity
/// counts, how many whole milliseconds each rate's ticks cover, and how the
/// fixed step meets display frames.
pub fn rates(rates: &[TickRate], entities: &[u32], seconds: u64, pilot_seed: u64) -> ExitCode {
    let seats = 4u8;
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    outln!("tick rates ({profile} build; {seconds} s of sim time per cell)");

    outln!("milliseconds per tick (the first 12 ticks):");
    let mut schedules = Vec::new();
    for rate in rates {
        let msecs: Vec<u32> = (1..=12).map(|tick| rate.msec(tick)).collect();
        let uneven = msecs.iter().any(|&msec| msec != msecs[0]);
        outln!(
            "  {:>4} Hz: {}{}",
            rate.hz(),
            msecs
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(" "),
            if uneven { "  (uneven)" } else { "" }
        );
        schedules.push(serde_json::json!({ "hz": rate.hz(), "msec": msecs, "uneven": uneven }));
    }

    outln!("step cost (step + snapshot), microseconds; core share = mean x rate:");
    outln!(
        "  {:>8} {:>5} {:>8} {:>8} {:>8} {:>7} {:>9} {:>11}",
        "entities",
        "Hz",
        "mean",
        "p99",
        "max",
        "core%",
        "snap B",
        "full kB/s"
    );
    let mut costs = Vec::new();
    for &count in entities {
        for rate in rates {
            let setup = Setup {
                world: WorldConfig {
                    seed: 1,
                    seats,
                    wanderers: count.saturating_sub(u32::from(seats)),
                    spawn: [0.0; 3],
                    yaw: 90.0,
                    sheet: sim::combat::Sheet::dev_soldier(),
                    guests: sim::Guests::Avatars,
                },
                rate: *rate,
                pilot: pilot_seed,
            };
            let mut run = Run::new(&setup);
            let ticks = u64::from(rate.hz()) * seconds;
            let mut times: Vec<Duration> = Vec::with_capacity(ticks as usize);
            let mut bytes = 0;
            for tick in 1..=ticks {
                let input = pilot(pilot_seed, tick, seats);
                let started = Instant::now();
                run.step(&input, StepReason::Authority);
                times.push(started.elapsed());
                bytes += run.last.encode().len();
            }
            times.sort();
            let micros = |duration: Duration| duration.as_secs_f64() * 1e6;
            let mean = times.iter().map(|time| micros(*time)).sum::<f64>() / times.len() as f64;
            let p99 = micros(times[times.len() * 99 / 100]);
            let max = micros(times[times.len() - 1]);
            let share = mean * f64::from(rate.hz()) / 1e4;
            let snapshot = bytes / ticks as usize;
            let rate_kb = snapshot as f64 * f64::from(rate.hz()) / 1000.0;
            outln!(
                "  {count:>8} {:>5} {mean:>8.1} {p99:>8.1} {max:>8.1} {share:>7.3} {snapshot:>9} {rate_kb:>11.1}",
                rate.hz()
            );
            costs.push(serde_json::json!({
                "entities": count,
                "hz": rate.hz(),
                "mean_us": mean,
                "p99_us": p99,
                "max_us": max,
                "core_percent": share,
                "snapshot_bytes": snapshot,
                "full_snapshot_kb_per_s": rate_kb,
            }));
        }
    }

    outln!("fixed step against display frames (10 s, frame times jittered +-20%):");
    outln!(
        "  {:>5} {:>7} {:>10} {:>10} {:>10} {:>10}",
        "Hz",
        "display",
        "0 ticks",
        "1 tick",
        "2+ ticks",
        "max/frame"
    );
    let mut frames_report = Vec::new();
    let mut jitter = SimRng::new(pilot_seed, 99);
    for rate in rates {
        for display in [30u32, 60, 144, 240] {
            let mut clock = Accumulator::new(*rate, 0);
            let mut counts = [0u64; 3];
            let mut most = 0;
            let frame = 1.0 / f64::from(display);
            let mut wall = 0.0;
            while wall < 10.0 {
                let elapsed = frame * f64::from(jitter.range(0.8, 1.2));
                wall += elapsed;
                let due = clock.advance(Duration::from_secs_f64(elapsed)).len();
                counts[due.min(2)] += 1;
                most = most.max(due);
            }
            let frames: u64 = counts.iter().sum();
            let percent = |count: u64| count as f64 * 100.0 / frames as f64;
            outln!(
                "  {:>5} {display:>7} {:>9.1}% {:>9.1}% {:>9.1}% {most:>10}",
                rate.hz(),
                percent(counts[0]),
                percent(counts[1]),
                percent(counts[2]),
            );
            frames_report.push(serde_json::json!({
                "hz": rate.hz(),
                "display_hz": display,
                "frames_without_tick_percent": percent(counts[0]),
                "frames_one_tick_percent": percent(counts[1]),
                "frames_two_plus_percent": percent(counts[2]),
                "max_ticks_in_a_frame": most,
            }));
        }
    }

    outln!(
        "per rate: tick length, mean wait for the next tick, ticks to re-predict at 100 / 200 ms:"
    );
    let mut derived = Vec::new();
    for rate in rates {
        let tick_ms = 1000.0 / f64::from(rate.hz());
        let resim = |rtt: f64| (rtt / tick_ms).ceil() as u32;
        outln!(
            "  {:>5} Hz: {tick_ms:>6.2} ms, {:>5.2} ms, {:>3} / {:>3}",
            rate.hz(),
            tick_ms / 2.0,
            resim(100.0),
            resim(200.0)
        );
        derived.push(serde_json::json!({
            "hz": rate.hz(),
            "tick_ms": tick_ms,
            "mean_input_wait_ms": tick_ms / 2.0,
            "repredict_ticks_100ms": resim(100.0),
            "repredict_ticks_200ms": resim(200.0),
        }));
    }

    write_facts(serde_json::json!({
        "verb": "sim rates",
        "profile": profile,
        "seconds": seconds,
        "schedules": schedules,
        "costs": costs,
        "frames": frames_report,
        "derived": derived,
    }));
    ExitCode::SUCCESS
}
