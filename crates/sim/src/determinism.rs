//! The determinism run: the same scripted inputs stepped several ways,
//! whose snapshot hashes must agree tick for tick, and a final hash every
//! platform must reach (`kjk sim determinism`, the approved test
//! `sim_determinism`, CI's pinned hash; `docs/SIM.md`).
//!
//! The world starts in the placeholder arena (boxes, wanderers, the
//! authority's spawner) and, after a third of the run, installs the
//! proving ground (`proving_ground`): a module whose collision is a built
//! `collision::Level` of off-axis triangles far from the origin, with a
//! door the players open and a trigger that asks for a transition.
//!
//! The ways, each against the authority run A (which records itself,
//! keeps every tick's hash and saves at the halfway tick):
//! - B: a fresh world, the same inputs;
//! - C: each tick's commands handed over in reverse seat order;
//! - D: the halfway save, through bytes, into a new world;
//! - E: the recording, through bytes, played back, installs and all;
//! - F: another world seed, which must change the result;
//! - G: rollbacks: predictions from kept snapshots (installing as a client
//!   would), which must match the authority exactly when no authority-only
//!   event (a spawn, a transition ask) falls in their window.

use std::collections::BTreeMap;
use std::sync::Arc;

use collision::{Contents, LevelBuilder, Surface, Tag};

use crate::input::{Command, Seat, TickInput, buttons};
use crate::install::Announcement;
use crate::module::{Link, ModuleData, ObjectData, ObjectKind, Opens, Placement, Spot};
use crate::motion::AnimTable;
use crate::rate::TickRate;
use crate::replay::{Divergence, Recorder, Recording};
use crate::rng::SimRng;
use crate::save::Save;
use crate::snapshot::{Kind, Snapshot, SnapshotHash};
use crate::world::{Level, SimWorld, StepReason, WorldConfig, step};

/// What a run starts from.
#[derive(Debug, Clone, Copy)]
pub struct Setup {
    pub world: WorldConfig,
    pub rate: TickRate,
    /// Seeds the pilot's commands, apart from the world's own seed.
    pub pilot: u64,
}

impl Setup {
    /// The run CI pins: world seed 1, pilot seed 7, 4 seats, 32 wanderers,
    /// the default tick rate (`kjk sim determinism` without options).
    pub fn pinned() -> Setup {
        Setup {
            world: WorldConfig {
                seed: 1,
                seats: 4,
                wanderers: 32,
                spawn: [0.0; 3],
                yaw: 90.0,
                sheet: crate::combat::Sheet::dev_soldier(),
                guests: crate::Guests::Avatars,
            },
            rate: TickRate::default(),
            pilot: 7,
        }
    }
}

/// Ticks the pinned run steps.
pub const PINNED_TICKS: u64 = 10_000;

/// Ticks a scripted seat holds one stick direction.
const HOLD_TICKS: u64 = 45;
/// Ticks between a rollback's start and the authority tick it is compared at.
pub const ROLLBACK_TICKS: u64 = 30;
/// Ticks between rollback starts: a prime, so the windows fall at every
/// phase of the spawner's period, whatever the rate.
const ROLLBACK_EVERY: u64 = 97;
/// Ticks between the hashes a recording keeps.
pub const RECORD_EVERY: u64 = 100;

/// Scripted commands for `seats` seats: each holds a random stick
/// direction for `HOLD_TICKS` while turning at a random rate, lets go now
/// and then (sends nothing), walks or crouches some stretches, holds use
/// some (opening a door it looks at), and jumps every 97 ticks. A pure
/// function of its arguments, so any run can regenerate any tick's input.
pub fn pilot(seed: u64, tick: u64, seats: u8) -> TickInput {
    let mut input = TickInput::default();
    for seat in 0..seats {
        let segment = tick / HOLD_TICKS;
        let mut rng = SimRng::new(seed, (segment << 8) | u64::from(seat));
        if rng.below(8) == 0 {
            continue;
        }
        let mut axis = || (rng.below(255) as i32 - 127) as i8;
        let (forward, right) = (axis(), axis());
        let heading = rng.below(65_536) as i32;
        let turn = rng.below(401) as i32 - 200;
        let crouch = rng.below(6) == 0;
        let walk = rng.below(4) == 0;
        let using = rng.below(3) == 0;
        let yaw = heading + turn * (tick % HOLD_TICKS) as i32;
        let jump = (tick + u64::from(seat) * 13) % 97 < 3;
        let up = if jump {
            127
        } else if crouch {
            -127
        } else {
            0
        };
        let (forward, right) = if walk {
            (forward / 2, right / 2)
        } else {
            (forward, right)
        };
        input.insert(
            Seat(seat),
            Command {
                angles: [0, (yaw & 0xffff) as u16 as i16, 0],
                forward,
                right,
                up,
                buttons: if walk { buttons::WALKING } else { 0 }
                    | if using { buttons::USE } else { 0 },
                weapon: 0,
                force: 0,
                reply: 0,
                aim: [0; 3],
            },
        );
    }
    input
}

/// The proving ground's module name.
pub const GROUND: &str = "proving_ground";
/// Where its middle is (metres): far from the origin, as a KOTOR area's
/// rooms are, so traces there work at floats' coarser spacing.
const ORIGIN: [f32; 3] = [-452.25, 317.75, 12.5];
/// How far it is turned from the axes (degrees): no wall or edge lies
/// along X or Y.
pub(crate) const TURN_DEGREES: f32 = 34.0;
/// The door's object index, which is also its entity in the collision.
const DOOR: u32 = 0;

/// Local ground coordinates (x along the room, y across, z up, metres)
/// into the level's.
struct Frame {
    sin: f32,
    cos: f32,
}

impl Frame {
    fn at(&self, x: f32, y: f32, z: f32) -> [f32; 3] {
        [
            ORIGIN[0] + self.cos * x - self.sin * y,
            ORIGIN[1] + self.sin * x + self.cos * y,
            ORIGIN[2] + z,
        ]
    }

    /// Two triangles over four corners in order.
    fn quad(&self, builder: &mut LevelBuilder, corners: [[f32; 3]; 4], tag: Tag) {
        let [a, b, c, d] = corners.map(|[x, y, z]| self.at(x, y, z));
        builder.triangle([a, b, c], tag);
        builder.triangle([a, c, d], tag);
    }

    /// An upright quad from `from` to `to` (x, y) between heights `low`
    /// and `high`, facing the right of the way from `from` to `to`.
    fn wall(
        &self,
        builder: &mut LevelBuilder,
        from: [f32; 2],
        to: [f32; 2],
        (low, high): (f32, f32),
        tag: Tag,
    ) {
        let ([x0, y0], [x1, y1]) = (from, to);
        self.quad(
            builder,
            [[x0, y0, low], [x1, y1, low], [x1, y1, high], [x0, y0, high]],
            tag,
        );
    }

    /// A ramp rising along +x from the floor to `rise` over
    /// `x0..x1` × `y0..y1`: its top, its end and its sides.
    fn ramp(&self, builder: &mut LevelBuilder, [x0, x1, y0, y1]: [f32; 4], rise: f32) {
        let tag = Tag::level(Contents::SOLID, 3);
        self.quad(
            builder,
            [[x0, y0, 0.0], [x1, y0, rise], [x1, y1, rise], [x0, y1, 0.0]],
            tag,
        );
        self.wall(builder, [x1, y0], [x1, y1], (0.0, rise), tag);
        let side = |x: f32, y: f32, z: f32| self.at(x, y, z);
        builder.triangle(
            [side(x0, y0, 0.0), side(x1, y0, 0.0), side(x1, y0, rise)],
            tag,
        );
        builder.triangle(
            [side(x0, y1, 0.0), side(x1, y1, rise), side(x1, y1, 0.0)],
            tag,
        );
    }
}

/// A point of the proving ground on its own axes: metres from the
/// doorway's middle (x through the door, y along the wall across), `z`
/// above the floor.
pub fn ground_point(x: f32, y: f32, z: f32) -> [f32; 3] {
    let (sin, cos) = collision::trig::sin_cos(TURN_DEGREES * (std::f32::consts::PI / 180.0));
    Frame { sin, cos }.at(x, y, z)
}

/// The module the run installs: a walled room 16 m by 12 m of triangles,
/// turned 34° and some 550 m from the origin, split across by a wall whose
/// doorway holds a door (solid closed, swung aside open: brushes of each
/// state, as a KOTOR door's walkmeshes are), with a gentle ramp and a
/// wedge too steep to stand on in the half players arrive in, and a
/// trigger that leads out of the module in the other half. Built as a
/// KOTOR area is (`collision_odyssey::build_area`): triangles into a
/// `LevelBuilder`, the door's owned by its object's index, the level with
/// the module's objects.
pub fn proving_ground() -> Level {
    proving_ground_named(GROUND)
}

/// The proving ground as a module named `name`: a second one to leave
/// behind (`fixtures::kitchen_sink`).
pub(crate) fn proving_ground_named(name: &str) -> Level {
    let (sin, cos) = collision::trig::sin_cos(TURN_DEGREES * (std::f32::consts::PI / 180.0));
    let frame = Frame { sin, cos };
    let mut builder = LevelBuilder::new();
    let floor = Tag::level(Contents::SOLID, 1);
    let wall = Tag::level(Contents::SOLID, 2);
    let door = |state: u8| Tag {
        contents: Contents::SOLID,
        surface: Surface { material: 4 },
        entity: Some(DOOR),
        states: 1 << state,
    };
    // The floor, 2 m cells.
    for i in -4..4 {
        for j in -3..3 {
            let (x0, y0) = (i as f32 * 2.0, j as f32 * 2.0);
            let (x1, y1) = (x0 + 2.0, y0 + 2.0);
            frame.quad(
                &mut builder,
                [[x0, y0, 0.0], [x1, y0, 0.0], [x1, y1, 0.0], [x0, y1, 0.0]],
                floor,
            );
        }
    }
    // The walls round it, 3 m high in 2 m panels, facing in.
    let corners = [[-8.0f32, 6.0], [8.0, 6.0], [8.0, -6.0], [-8.0, -6.0]];
    for k in 0..4 {
        let (from, to) = (corners[k], corners[(k + 1) % 4]);
        let panels = ((to[0] - from[0]).abs() + (to[1] - from[1]).abs()) as i32 / 2;
        for p in 0..panels {
            let along = |t: f32| std::array::from_fn(|i| from[i] + (to[i] - from[i]) * t);
            let (a, b) = (p as f32 / panels as f32, (p + 1) as f32 / panels as f32);
            frame.wall(&mut builder, along(a), along(b), (0.0, 3.0), wall);
        }
    }
    // The wall across, its doorway 2 m wide and 2.5 m high, and the door.
    frame.wall(&mut builder, [0.0, -6.0], [0.0, -1.0], (0.0, 3.0), wall);
    frame.wall(&mut builder, [0.0, 1.0], [0.0, 6.0], (0.0, 3.0), wall);
    frame.wall(&mut builder, [0.0, -1.0], [0.0, 1.0], (2.5, 3.0), wall);
    frame.wall(&mut builder, [0.0, -1.0], [0.0, 1.0], (0.0, 2.5), door(0));
    frame.wall(&mut builder, [0.0, 1.0], [2.0, 1.0], (0.0, 2.5), door(1));
    // A ramp of 17° and a wedge of 56°.
    frame.ramp(&mut builder, [-7.0, -3.0, -5.5, -3.5], 1.2);
    frame.ramp(&mut builder, [-7.5, -6.0, 3.0, 5.0], 2.2);

    let position = |x: f32, y: f32| frame.at(x, y, 0.0);
    let object = |kind: ObjectKind, tag: &str, at: [f32; 3]| ObjectData {
        kind,
        tag: tag.to_string(),
        cased_tag: String::new(),
        template: String::new(),
        position: at,
        yaw: TURN_DEGREES,
        link: None,
        geometry: Vec::new(),
        opens: Opens::Never,
        state: 0,
        ways: crate::module::Ways::One,
        scripts: Vec::new(),
        conversation: String::new(),
        sheet: None,
        senses: None,
        classes: Vec::new(),
        gender: 0,
        subrace: 0,
        personal_space: 0.0,
        strike: None,
        challenge: 0.0,
        name: String::new(),
        inventory: Vec::new(),
        container: false,
        useable: false,
        store: None,
        lock: None,
        hit_points: None,
        toughness: crate::module::Toughness::default(),
        faction: 0,
        body: None,
        map_note: None,
        trap: None,
        interruptable: true,
        pitch_variation: 0.0,
        encounter: None,
    };
    let trigger_at = position(5.5, 0.0);
    let gate = ObjectData {
        opens: Opens::Yes,
        ..object(ObjectKind::Door, "gate", position(0.0, 0.0))
    };
    let way_out = ObjectData {
        link: Some(Link {
            module: "elsewhere".to_string(),
            tag: String::new(),
            destination: None,
        }),
        geometry: [[4.0f32, -2.0], [7.0, -2.0], [7.0, 2.0], [4.0, 2.0]]
            .map(|[x, y]| {
                let p = position(x, y);
                [p[0] - trigger_at[0], p[1] - trigger_at[1], 0.0]
            })
            .to_vec(),
        ..object(ObjectKind::Trigger, "way_out", trigger_at)
    };
    let module = ModuleData {
        minutes_per_hour: 0,
        game: "kjk".to_string(),
        name: name.to_string(),
        title: String::new(),
        area: name.to_string(),
        area_title: String::new(),
        entry: Spot {
            position: position(-4.0, 0.0),
            yaw: TURN_DEGREES,
        },
        placements: vec![Placement::Own(gate), Placement::Own(way_out)],
        templates: Default::default(),
        repute: crate::module::Repute::default(),
        force_costs: Vec::new(),
        globals: BTreeMap::new(),
        module_scripts: Vec::new(),
        area_scripts: Vec::new(),
        players: 1,
        map: None,
        traps: Vec::new(),
        bags: Vec::new(),
        area_stealth: Default::default(),
        ranges: Default::default(),
        force: Default::default(),
        disable_transit: false,
        encounter_tables: Default::default(),
    };
    Level::new(
        name,
        Arc::new(builder.build()),
        Arc::new(AnimTable::stand_in()),
    )
    .with_module(module)
}

/// The run's two levels, built once.
struct Levels {
    arena: Level,
    ground: Level,
}

impl Levels {
    /// The level `snapshot` was taken in.
    fn of(&self, snapshot: &Snapshot) -> &Level {
        if snapshot.place.is_some() {
            &self.ground
        } else {
            &self.arena
        }
    }
}

/// A world and its latest snapshot, stepped one tick at a time.
pub struct Run {
    pub world: SimWorld,
    pub rate: TickRate,
    pub last: Snapshot,
}

impl Run {
    /// A new world of `setup` in the placeholder level.
    pub fn new(setup: &Setup) -> Self {
        let mut world = SimWorld::new(setup.world, Level::arena())
            .unwrap_or_else(|error| panic!("the run's world: {error}"));
        let last = world.snapshot();
        Run {
            world,
            rate: setup.rate,
            last,
        }
    }

    /// A world restored from `snapshot`, which was taken in `level`.
    pub fn resume(level: &Level, rate: TickRate, snapshot: &Snapshot) -> Self {
        Run {
            world: SimWorld::from_snapshot(snapshot, level.clone())
                .expect("a snapshot of the run restores in its level"),
            rate,
            last: snapshot.clone(),
        }
    }

    pub fn step(&mut self, input: &TickInput, reason: StepReason) -> &Snapshot {
        let tick = self.last.tick + 1;
        self.last = step(&mut self.world, tick, input, self.rate.msec(tick), reason);
        &self.last
    }

    /// Installs `level`'s module at its entry.
    pub fn install(&mut self, level: &Level) -> &Snapshot {
        self.last = self
            .world
            .install(level.clone(), None)
            .expect("the players have room at the module's entry");
        &self.last
    }

    /// One tick of the scenario: the step, and the install after
    /// `install_at`.
    fn advance(
        &mut self,
        input: &TickInput,
        reason: StepReason,
        levels: &Levels,
        install_at: u64,
    ) -> &Snapshot {
        self.step(input, reason);
        if self.last.tick == install_at {
            self.install(&levels.ground);
        }
        &self.last
    }
}

/// Runs A and B's hashes at a tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Checkpoint {
    pub tick: u64,
    pub a: SnapshotHash,
    pub b: SnapshotHash,
}

/// What run A did in the proving ground.
#[derive(Debug, Clone, PartialEq)]
pub struct Ground {
    /// Installed after this tick.
    pub installed_after: u64,
    pub brushes: usize,
    /// `collision::Level::digest`.
    pub collision_digest: String,
    /// The first tick a player had opened the door by.
    pub door_opened_at: Option<u64>,
    /// Ticks whose step asked for a transition (the trigger that leads
    /// out; the host declines them).
    pub asks: usize,
}

/// What a determinism run found.
#[derive(Debug, Clone)]
pub struct Report {
    pub setup: Setup,
    pub ticks: u64,
    /// A and B at every tenth of the run.
    pub checkpoints: Vec<Checkpoint>,
    /// A's last snapshot's hash: what `--expect` and CI pin.
    pub final_hash: SnapshotHash,
    /// Tick hashes compared (tick 0 to the last).
    pub total: usize,
    /// B: of `total`, equal.
    pub fresh_equal: usize,
    /// C: of `ticks`, equal.
    pub reversed_equal: usize,
    /// D: saved at this tick; of `loaded_total`, equal.
    pub middle: u64,
    pub loaded_equal: usize,
    pub loaded_total: usize,
    pub save_bytes: usize,
    /// E: the replay's last snapshot's hash, or where it diverged.
    pub replay: Result<SnapshotHash, Divergence>,
    pub recording_bytes: usize,
    /// F.
    pub other_seed_differs: bool,
    /// G: rollbacks run, matching the authority, and the starts whose
    /// verdict was not the expected one.
    pub rollbacks: usize,
    pub rollbacks_matched: usize,
    pub rollbacks_unexpected: Vec<u64>,
    pub ground: Ground,
    pub entities_end: usize,
}

impl Report {
    pub fn fresh_same(&self) -> bool {
        self.fresh_equal == self.total
    }

    pub fn reversed_same(&self) -> bool {
        self.reversed_equal as u64 == self.ticks
    }

    pub fn loaded_same(&self) -> bool {
        self.loaded_equal == self.loaded_total
    }

    pub fn replay_same(&self) -> bool {
        self.replay
            .as_ref()
            .is_ok_and(|last| *last == self.final_hash)
    }

    /// Whether the final hash starts with `prefix` (hex, any case); an
    /// empty prefix never matches.
    pub fn reaches(&self, prefix: &str) -> bool {
        !prefix.is_empty()
            && self
                .final_hash
                .to_string()
                .starts_with(&prefix.to_ascii_lowercase())
    }

    /// Every way agrees (and, with `expect`, the final hash is the one
    /// expected).
    pub fn passed(&self, expect: Option<&str>) -> bool {
        expect.is_none_or(|prefix| self.reaches(prefix))
            && self.fresh_same()
            && self.reversed_same()
            && self.loaded_same()
            && self.replay_same()
            && self.other_seed_differs
            && self.rollbacks_unexpected.is_empty()
    }
}

fn count_equal(a: &[SnapshotHash], b: &[SnapshotHash]) -> usize {
    a.iter().zip(b).filter(|(a, b)| a == b).count()
}

/// Steps `setup`'s world `ticks` ticks every way above and compares.
pub fn run(setup: &Setup, ticks: u64) -> Report {
    let seats = setup.world.seats;
    let hz = setup.rate.hz();
    let levels = Levels {
        arena: Level::arena(),
        ground: proving_ground(),
    };
    let install_at = ticks / 3;
    let input = |tick: u64| pilot(setup.pilot, tick, seats);

    // A: the authority run everything else is compared with. It records
    // itself, keeps every tick's hash, and saves at the halfway tick.
    let middle = ticks / 2;
    let mut a = Run::new(setup);
    let mut recorder = Recorder::new(hz, a.last.clone(), levels.arena.id(), RECORD_EVERY);
    let mut hashes = vec![a.last.hash()];
    let mut kept: BTreeMap<u64, Snapshot> = BTreeMap::new();
    kept.insert(0, a.last.clone());
    let mut save = Vec::new();
    // Ids are never reused, so a spawn is a step that shows a new highest
    // id (an install's new objects are not spawns).
    let newest = |snapshot: &Snapshot| snapshot.entities.iter().map(|entity| entity.id).max();
    let mut highest = newest(&a.last);
    let mut spawned_at = Vec::new();
    let mut asked_at = Vec::new();
    let mut door_opened_at = None;
    for tick in 1..=ticks {
        let input = input(tick);
        let stepped = a.step(&input, StepReason::Authority);
        recorder.record(setup.rate.msec(tick), &input, stepped);
        if newest(stepped) > highest {
            spawned_at.push(tick);
        }
        if stepped.transition.is_some() {
            asked_at.push(tick);
        }
        if door_opened_at.is_none() && door_open(stepped) {
            door_opened_at = Some(tick);
        }
        if tick == install_at {
            let after = a.install(&levels.ground);
            recorder.install(
                Announcement::new(&levels.ground, None, after).expect("the ground is a module"),
            );
        }
        let snapshot = &a.last;
        highest = newest(snapshot);
        hashes.push(snapshot.hash());
        if tick.is_multiple_of(ROLLBACK_EVERY) {
            kept.insert(tick, snapshot.clone());
        }
        if tick == middle {
            save = Save {
                hz,
                level: levels.of(snapshot).id(),
                snapshot: snapshot.clone(),
                script: a.world.script_state().clone(),
            }
            .encode();
        }
    }
    let end = a.last.clone();
    let recording = recorder.finish_encoded(&end);

    // B: a fresh world, the same inputs.
    let mut b = Run::new(setup);
    let mut b_hashes = vec![b.last.hash()];
    for tick in 1..=ticks {
        b_hashes.push(
            b.advance(&input(tick), StepReason::Authority, &levels, install_at)
                .hash(),
        );
    }
    let fresh_equal = count_equal(&hashes, &b_hashes);

    // C: each tick's commands handed over in reverse seat order.
    let mut c = Run::new(setup);
    let mut reversed_equal = 0;
    for tick in 1..=ticks {
        let mut reversed = TickInput::default();
        for (seat, command) in input(tick).iter().collect::<Vec<_>>().into_iter().rev() {
            reversed.insert(seat, *command);
        }
        reversed_equal += usize::from(
            c.advance(&reversed, StepReason::Authority, &levels, install_at)
                .hash()
                == hashes[tick as usize],
        );
    }

    // D: the halfway save, through bytes, into a new world in the level
    // it names.
    let loaded = Save::decode(&save).expect("a save this build wrote decodes");
    let level = levels.of(&loaded.snapshot);
    loaded
        .level
        .check(level)
        .expect("the save names the level it was made in");
    let mut d = Run::resume(level, setup.rate, &loaded.snapshot);
    d.world.set_script_state(loaded.script.clone());
    let mut loaded_equal = usize::from(d.last.hash() == hashes[middle as usize]);
    for tick in middle + 1..=ticks {
        loaded_equal += usize::from(
            d.advance(&input(tick), StepReason::Authority, &levels, install_at)
                .hash()
                == hashes[tick as usize],
        );
    }
    let loaded_total = (ticks - middle + 1) as usize;

    // E: the recording, through bytes, played back; its install builds
    // the ground again from the announcement's name.
    let replay = Recording::decode(&recording)
        .expect("a recording this build wrote decodes")
        .play_with(&levels.arena, &mut |announcement| {
            if announcement.module == GROUND {
                Ok(proving_ground())
            } else {
                Err(format!("{}: not this run's module", announcement.module))
            }
        })
        .map(|last| last.hash());

    // F: another world seed must change the result.
    let mut f_setup = *setup;
    f_setup.world.seed ^= 0x5eed;
    let mut f = Run::new(&f_setup);
    for tick in 1..=ticks {
        f.advance(&input(tick), StepReason::Authority, &levels, install_at);
    }
    let other_seed_differs = f.last.hash() != end.hash();

    // G: rollbacks. From each kept snapshot, predict ROLLBACK_TICKS ahead
    // with the same inputs. Prediction skips what only the authority runs
    // (the spawner, transition asks), so it must match exactly when no
    // spawn falls in the window and no ask on its last tick (an ask lasts
    // one step), and differ when one does.
    let mut rollbacks = 0;
    let mut rollbacks_matched = 0;
    let mut rollbacks_unexpected = Vec::new();
    for (&from, snapshot) in &kept {
        let to = from + ROLLBACK_TICKS;
        if to > ticks {
            continue;
        }
        let mut g = Run::resume(levels.of(snapshot), setup.rate, snapshot);
        for tick in from + 1..=to {
            g.advance(&input(tick), StepReason::Predict, &levels, install_at);
        }
        let same = g.last.hash() == hashes[to as usize];
        let authority_only = spawned_at.iter().any(|&at| at > from && at <= to)
            || (to != install_at && asked_at.contains(&to));
        rollbacks += 1;
        rollbacks_matched += usize::from(same);
        if same == authority_only {
            rollbacks_unexpected.push(from);
        }
    }

    let every = (ticks / 10).max(1);
    let checkpoints = (every..=ticks)
        .step_by(every as usize)
        .map(|tick| Checkpoint {
            tick,
            a: hashes[tick as usize],
            b: b_hashes[tick as usize],
        })
        .collect();
    let built = levels
        .ground
        .collision()
        .level()
        .expect("the ground is a built level");
    Report {
        setup: *setup,
        ticks,
        checkpoints,
        final_hash: end.hash(),
        total: hashes.len(),
        fresh_equal,
        reversed_equal,
        middle,
        loaded_equal,
        loaded_total,
        save_bytes: save.len(),
        replay,
        recording_bytes: recording.len(),
        other_seed_differs,
        rollbacks,
        rollbacks_matched,
        rollbacks_unexpected,
        ground: Ground {
            installed_after: install_at,
            brushes: built.brush_count(),
            collision_digest: built.digest(),
            door_opened_at,
            asks: asked_at.len(),
        },
        entities_end: end.entities.len(),
    }
}

/// Whether the proving ground's door stands open in `snapshot`.
fn door_open(snapshot: &Snapshot) -> bool {
    snapshot.entities.iter().any(|entity| {
        matches!(entity.kind, Kind::Object(ObjectKind::Door, DOOR))
            && entity.object.is_some_and(|object| object.state != 0)
    })
}
