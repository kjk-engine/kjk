//! What the headless probes of `kjk combat …` and `kjk collision walk`
//! share: an installer for a game, a module installed for one seat
//! (creatures turned hostile or allied first, `stage::with_factions`), and
//! the scenes a probe steps from it: seat 0 with a sheet facing one
//! creature, stepped on the authority by a script, recorded and replayed,
//! and the report of checks they end in.

use std::sync::Arc;

use collision::{Contents, Extents, UNIT};
use sim::combat::{Fighter, Sheet, force, mind};
use sim::module::ObjectKind;
use sim::motion::{self, MotionState};
use sim::{
    Command, Kind, Recorder, Seat, SimWorld, Snapshot, StepReason, TickInput, TickRate, step,
};
use vfs::{Discovery, Game};

use crate::lifecycle::Installer;
pub use crate::runtime::Report;

/// An installer for `game` (`k1` or `tsl`), timing moves by JA's skeleton.
pub fn installer(discovery: &Discovery, game: &str) -> Result<Installer, String> {
    let game = Game::from_key(game)
        .filter(|game| *game != Game::JediAcademy)
        .ok_or("use k1 or tsl")?;
    let (anims, _) = crate::stage::anims(discovery);
    Installer::new(discovery, game, Arc::new(anims))
}

/// A world of one seat with `level` installed, the seat arriving at
/// `arrive` (the module's entry without one).
pub fn one_seat(level: sim::Level, arrive: Option<&str>) -> Result<SimWorld, String> {
    let config = sim::WorldConfig::one_seat();
    let mut world =
        SimWorld::new(config, sim::Level::arena()).map_err(|error| error.to_string())?;
    world
        .install(level, arrive)
        .map_err(|error| format!("install refused: {error}"))?;
    Ok(world)
}

/// `module` of `game` planned and installed for one seat arriving at
/// `arrive`, the creatures tagged `hostile` and `allies` turned so first.
pub fn installed(
    discovery: &Discovery,
    game: &str,
    module: &str,
    arrive: Option<&str>,
    hostile: &[String],
    allies: &[String],
) -> Result<SimWorld, String> {
    let installer = installer(discovery, game)?;
    let prepared = installer.prepare_one(module, arrive)?;
    let level = crate::stage::with_factions(&prepared.plan.level, hostile, allies);
    one_seat(level, arrive)
}

pub use kjk_game::by_hand::door_states;

/// The creatures of `snapshot` (made in `level`) tagged `tag`, by their
/// places in its entities, in id order.
pub fn tagged(level: &sim::Level, snapshot: &Snapshot, tag: &str) -> Vec<usize> {
    let objects = sim::Instances::in_snapshot(level, snapshot);
    (0..snapshot.entities.len())
        .filter(|&at| {
            let entity = &snapshot.entities[at];
            matches!(entity.kind, Kind::Object(ObjectKind::Creature, _))
                && crate::cast::is_tagged(entity, &objects, tag)
        })
        .collect()
}

/// Where a probe's scenes start: a module installed for one seat, and the
/// scripts' state every scene and its replay start from (none unless
/// `with_script` gives the install's).
pub struct Stage {
    pub level: sim::Level,
    pub start: Snapshot,
    pub script: sim::ScriptVmState,
    pub rate: TickRate,
}

/// What a scene's script asks for at a sim time since its start: buttons,
/// the selected power, forward movement.
pub type Script<'a> = dyn FnMut(u64) -> (u16, u8, i8) + 'a;

/// A scene's result: every snapshot after the start, and whether the
/// recording replayed to its end.
pub struct Run {
    pub snapshots: Vec<Snapshot>,
    pub replayed: bool,
}

impl Stage {
    /// `module` installed for one seat with `hostile` turned so.
    pub fn new(
        discovery: &Discovery,
        game: &str,
        module: &str,
        hostile: &[String],
    ) -> Result<Stage, String> {
        Ok(Stage::of(&mut installed(
            discovery,
            game,
            module,
            None,
            hostile,
            &[],
        )?))
    }

    /// A world's level and snapshot as a stage.
    pub fn of(world: &mut SimWorld) -> Stage {
        Stage {
            level: world.level().clone(),
            start: world.snapshot(),
            script: sim::ScriptVmState::default(),
            rate: TickRate::default(),
        }
    }

    /// This stage with its scenes starting from `script` (an install's
    /// state: the items its creatures were stocked with, its globals).
    pub fn with_script(self, script: sim::ScriptVmState) -> Stage {
        Stage { script, ..self }
    }

    /// The start with seat 0 as `sheet` at full pools (the regeneration
    /// clock as a full pool keeps it: a spawn's is due at once, as JA's is;
    /// a scene starts steady).
    pub fn with_sheet(&self, sheet: Sheet) -> Snapshot {
        let mut start = self.start.clone();
        let me = seat0(&start);
        let mut fighter = Fighter::of(&sheet);
        fighter.force.regen_at = start.time_ms + force::regen_every(sheet.pool());
        start.entities[me].sheet = Some(sheet);
        start.entities[me].fighter = Some(fighter);
        start
    }

    /// This stage with the creature at `at` reading `sheet` (its module's
    /// data changed, and the start's place with it).
    pub fn with_creature_sheet(&self, at: usize, sheet: Sheet) -> Option<Stage> {
        let Kind::Object(_, index) = self.start.entities.get(at)?.kind else {
            return None;
        };
        let mut module = (**self.level.module()?).clone();
        module.object_mut(index as usize)?.sheet = Some(sheet);
        let level = self.level.clone().with_module(module);
        let mut start = self.start.clone();
        if let Some(place) = start.place.as_mut() {
            place.digest = level.digest();
        }
        Some(Stage {
            level,
            start,
            script: self.script.clone(),
            rate: self.rate,
        })
    }

    /// The open arena (`sim::Level::arena`) with one seat.
    pub fn arena() -> Stage {
        let config = sim::WorldConfig::one_seat();
        let mut world = SimWorld::new(config, sim::Level::arena()).expect("the arena");
        Stage::of(&mut world)
    }

    /// The creature tagged `tag` (the first of them).
    pub fn creature(&self, tag: &str) -> Option<usize> {
        tagged(&self.level, &self.start, tag).first().copied()
    }

    /// The start of a scene: seat 0 with `sheet` at full pools, `distance`
    /// units from `target` facing it; everyone else but the target dead,
    /// so only the two act, and the target's mind casting nothing.
    pub fn scene(&self, target: usize, distance: f32, sheet: Sheet) -> Option<Snapshot> {
        let mut start = self.with_sheet(sheet);
        let them = start.entities[target].player?;
        let spot = spot_near(&self.level, &them, distance)?;
        let (_, yaw) = mind::aim(spot, them.origin);
        let me = seat0(&start);
        start.entities[me].player = Some(MotionState::standing(spot, yaw));
        for (at, entity) in start.entities.iter_mut().enumerate() {
            if !matches!(entity.kind, Kind::Object(ObjectKind::Creature, _)) {
                continue;
            }
            if let (Some(fighter), Some(player)) = (entity.fighter.as_mut(), entity.player.as_mut())
            {
                // The target's mind fights on, but casts nothing: a scene
                // measures seat 0 alone.
                fighter.mind.cast_after = u64::MAX;
                if at != target {
                    fighter.life = sim::combat::Life::Dead;
                    player.flags |= motion::motion_flags::DEAD;
                }
            }
        }
        Some(start)
    }

    /// Steps a scene from `start` for `ms`, seat 0 looking at the entity at
    /// `target` and doing what `script` says; records it and replays it.
    pub fn run(&self, start: &Snapshot, target: usize, ms: u64, script: &mut Script) -> Run {
        let me = seat0(start);
        self.drive(start, ms, &mut |last, since| {
            aim(last, me, target, script(since))
        })
    }

    /// Steps a scene from `start` for `ms`, seat 0's command each step
    /// `command` of the last snapshot and the time since the start;
    /// records it and replays it.
    pub fn drive(
        &self,
        start: &Snapshot,
        ms: u64,
        command: &mut dyn FnMut(&Snapshot, u64) -> Command,
    ) -> Run {
        let mut world =
            SimWorld::from_snapshot(start, self.level.clone()).expect("the scene starts");
        world.set_script_state(self.script.clone());
        let mut last = start.clone();
        let mut recorder = Recorder::new(self.rate.hz(), last.clone(), self.level.id(), 100)
            .with_script(self.script.clone());
        let mut snapshots = Vec::new();
        while self.rate.time_ms(last.tick) < start.time_ms + ms {
            let tick = last.tick + 1;
            let command = command(&last, last.time_ms - start.time_ms);
            let mut input = TickInput::default();
            input.insert(Seat(0), command);
            let msec = self.rate.msec(tick);
            last = step(&mut world, tick, &input, msec, StepReason::Authority);
            recorder.record(msec, &input, &last);
            snapshots.push(last.clone());
        }
        // `KJK_DEBUG_SCRIPTS=1`: what the scene's scripts ran and called.
        if std::env::var_os("KJK_DEBUG_SCRIPTS").is_some() {
            let report = world.script_report();
            eprintln!("DEBUG runs {:?} faults {:?}", report.runs, report.faults);
            for line in report.trace.iter().take(120) {
                eprintln!("DEBUG {line}");
            }
        }
        let recording = recorder.finish(&last);
        let replayed = recording
            .play(&self.level)
            .is_ok_and(|end| end.hash() == recording.end);
        Run {
            snapshots,
            replayed,
        }
    }
}

pub fn seat0(snapshot: &Snapshot) -> usize {
    snapshot
        .entities
        .iter()
        .position(|entity| entity.kind == Kind::Avatar(Seat(0)))
        .expect("seat 0 plays")
}

pub fn fighter(snapshot: &Snapshot, at: usize) -> Fighter {
    snapshot.entities[at].fighter.expect("a character")
}

pub fn player(snapshot: &Snapshot, at: usize) -> MotionState {
    snapshot.entities[at].player.expect("a character")
}

/// A spot `distance` units from `target` on its floor that a standing box
/// reaches from it in a straight line, the first of sixteen directions.
pub fn spot_near(level: &sim::Level, target: &MotionState, distance: f32) -> Option<[f32; 3]> {
    let half = motion::HALF_WIDTH * UNIT;
    let extents = Extents {
        mins: [-half, -half, motion::SOLE_Z * UNIT],
        maxs: [half, half, target.maxs_z * UNIT],
    };
    (0..16u8).find_map(|step| {
        let (sin, cos) = sim::math::sin_cos_deg(f32::from(step) * 22.5);
        let spot = sim::math::mad(target.origin, distance, [cos, sin, 0.0]);
        let clear = level.with(None, |world| {
            let trace = world.trace(
                sim::math::scale(target.origin, UNIT),
                sim::math::scale(spot, UNIT),
                &extents,
                Contents::SOLID,
            );
            trace.fraction >= 1.0 && !trace.start_solid
        });
        clear.then_some(spot)
    })
}

/// Seat 0's command in `last`: looking at the target's chest, with the
/// script's buttons, power and forward movement.
pub fn aim(
    last: &Snapshot,
    me: usize,
    target: usize,
    (held, power, forward): (u16, u8, i8),
) -> Command {
    let (Some(player), Some(them)) = (last.entities[me].player, last.entities[target].player)
    else {
        return Command::default();
    };
    let (pitch, yaw) = mind::aim(mind::crosshair(&player), mind::chest(&them));
    Command {
        buttons: held,
        force: power,
        forward,
        ..mind::looking(&player, pitch, yaw)
    }
}

/// Holds `held` from the start for `for_ms`.
pub fn hold(held: u16, for_ms: u64) -> impl FnMut(u64) -> (u16, u8, i8) {
    move |ms| (if ms < for_ms { held } else { 0 }, 0, 0)
}
