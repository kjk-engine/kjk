//! The probes of `charting` (the approved test `map_fog`), on the proving
//! ground given a map 120 m square about it (15 by 8 cells: 8 m across,
//! 15 m down) and panels whose scripts the player uses:
//!
//! - pins: a waypoint whose note shows as placed and one whose does not;
//!   one panel pins the first off, another the second on;
//! - fog: standing explores the cells within reach, not the far ones;
//!   standing across the room explores more; leaving the module and coming
//!   back keeps them, as do a save and load; `RevealMap` round a point
//!   explores its cell and the four beside it; `ExploreAreaForPlayer` the
//!   whole; two runs end the same.

use fmt_odyssey::ncs::{Const, Op};

use crate::action::{Action, Verb};
use crate::fog::{Axis, Chart, Explored, REVEAL_M};
use crate::input::Seat;
use crate::module::{MapNote, ObjectData, ObjectKind, Placement};
use crate::snapshot::{ObjectState, Snapshot, pin};

/// The probe's routines, by catalog number.
const CATALOG: &str = r#"
// 0
void SetMapPinEnabled(object oMapPin, int nEnabled);
// 1
object GetObjectByTag(string sTag, int nNth=0);
// 2
void ExploreAreaForPlayer(object oArea, object oPlayer);
// 3
void RevealMap(vector vPoint=[0.0,0.0,0.0], int nRadius=-1);
"#;

/// The ground's map: this many metres square about its middle, the ARE's
/// `MapResX` this many cells across.
const SPAN: f32 = 120.0;
const COLUMNS: i32 = 15;

fn chart() -> Chart {
    let [x, y, _] = crate::determinism::ground_point(0.0, 0.0, 0.0);
    Chart::new(
        [
            Axis {
                world: 0,
                scale: 1.0 / SPAN,
                offset: 0.5 - x / SPAN,
            },
            // North up: down the picture is down world Y.
            Axis {
                world: 1,
                scale: -1.0 / SPAN,
                offset: 0.5 + y / SPAN,
            },
        ],
        COLUMNS,
    )
}

/// The world point (metres) at the middle of cell `[column, row]`.
fn middle(chart: &Chart, cell: [usize; 2]) -> [f32; 2] {
    let mut at = [0.0; 2];
    for (on, axis) in chart.axes.iter().enumerate() {
        let fraction = (cell[on] as f32 + 0.5) / f32::from(chart.cells[on]);
        at[usize::from(axis.world)] = (fraction - axis.offset) / axis.scale;
    }
    at
}

/// The cell `RevealMap` explores round, with a radius of one cell.
const REVEALED: [usize; 2] = [1, 6];

/// Each panel: its tag (and script's name) and its script's code.
fn panels(chart: &Chart) -> [(&'static str, Vec<Op>); 4] {
    let call = |routine: u16, args: u8| Op::Action { routine, args };
    let pin = |tag: &str, on: i32| {
        vec![
            Op::Const(Const::Int(on)),
            Op::Const(Const::Int(0)),
            Op::Const(Const::String(tag.as_bytes().to_vec())),
            call(1, 2),
            call(0, 2),
        ]
    };
    let [x, y] = middle(chart, REVEALED);
    [
        ("pin_off", pin("shown_note", 0)),
        ("pin_on", pin("hidden_note", 1)),
        (
            "reveal",
            vec![
                Op::Const(Const::Int(1)),
                Op::Const(Const::Float(x)),
                Op::Const(Const::Float(y)),
                Op::Const(Const::Float(0.0)),
                call(3, 2),
            ],
        ),
        (
            "explore",
            vec![
                Op::Const(Const::Object(1)),
                Op::Const(Const::Object(1)),
                call(2, 2),
            ],
        ),
    ]
}

fn library(chart: &Chart) -> crate::script::Library {
    let scripts = panels(chart).map(|(name, ops)| {
        let mut asm = script_ncs::probes::Asm::default();
        for op in ops {
            asm.op(op);
        }
        asm.op(Op::Retn);
        (name.to_string(), asm.build())
    });
    crate::probe::library(CATALOG, scripts)
}

/// The objects added after the ground's door and trigger: two notes, then
/// the panels.
const SHOWN: u32 = 2;
const HIDDEN: u32 = 3;
const PIN_OFF: u32 = 4;
const PIN_ON: u32 = 5;
const REVEAL: u32 = 6;
const EXPLORE: u32 = 7;

/// Where the player stands, by the panels, and across the room (the
/// ground's own axes, metres).
const NEAR: [f32; 2] = [-1.2, 0.0];
const ACROSS: [f32; 2] = [7.0, 5.0];

fn floor_at([x, y]: [f32; 2]) -> [f32; 3] {
    crate::determinism::ground_point(x, y, 0.0)
}

fn added(chart: &Chart) -> Vec<ObjectData> {
    let note = |tag: &str, at, enabled| ObjectData {
        map_note: Some(MapNote {
            text: tag.into(),
            enabled,
        }),
        ..ObjectData::bare(ObjectKind::Waypoint, tag, floor_at(at))
    };
    let panel = |(script, _): (&str, Vec<Op>), at| ObjectData {
        useable: true,
        scripts: vec![("OnUsed".into(), script.into())],
        ..ObjectData::bare(ObjectKind::Placeable, script, floor_at(at))
    };
    let mut objects = vec![
        note("shown_note", [-3.0, -2.0], true),
        note("hidden_note", [-3.0, 2.0], false),
    ];
    let places = [[-2.0, -1.0], [-2.0, 1.0], [-2.2, -1.8], [-2.2, 1.8]];
    objects.extend(
        panels(chart)
            .into_iter()
            .zip(places)
            .map(|(p, at)| panel(p, at)),
    );
    objects
}

/// What a phase does: the player stands somewhere, or uses a panel; the
/// party leaves for another module and comes back; or the world is saved
/// and loaded.
#[derive(Clone, Copy)]
enum Phase {
    Stand([f32; 2]),
    Use(u32),
    Away,
    Reload,
}

const PHASES: [Phase; 8] = [
    Phase::Stand(NEAR),
    Phase::Use(PIN_OFF),
    Phase::Use(PIN_ON),
    Phase::Stand(ACROSS),
    Phase::Away,
    Phase::Reload,
    Phase::Use(REVEAL),
    Phase::Use(EXPLORE),
];
/// Steps a phase lasts (a use's script runs in them).
const SETTLE: u64 = 3;

struct Run {
    world: super::super::SimWorld,
    tick: u64,
}

impl Run {
    /// `SETTLE` steps with the player at `at`, the first using `used`.
    fn settle(&mut self, at: [f32; 2], used: Option<u32>) {
        use crate::{StepReason, TickInput};
        for settle in 0..SETTLE {
            let world = &mut self.world.world;
            let mut players = world.query::<(&super::super::Avatar, &mut super::super::Player)>();
            for (_, mut player) in players.iter_mut(world) {
                let [x, y, _] = floor_at(at);
                player.0.origin[0] = x / collision::UNIT;
                player.0.origin[1] = y / collision::UNIT;
            }
            let mut input = TickInput::default();
            if let (Some(object), 0) = (used, settle) {
                let verb = Verb::Use;
                input.act(Seat(0), Action::Use { object, verb });
            }
            self.tick += 1;
            crate::step(&mut self.world, self.tick, &input, 8, StepReason::Authority);
        }
    }
}

/// The snapshot after each phase.
fn scenario() -> Result<Vec<Snapshot>, String> {
    let chart = chart();
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    data.map = Some(chart);
    data.placements
        .extend(added(&chart).into_iter().map(Placement::Own));
    let elsewhere = crate::determinism::proving_ground().with_module(crate::module::ModuleData {
        name: "elsewhere".into(),
        map: None,
        ..data.clone()
    });
    let level = ground.with_module(data).with_scripts(library(&chart));
    let world = super::super::SimWorld::new(
        crate::WorldConfig {
            seed: 1,
            seats: 1,
            wanderers: 0,
            spawn: [0.0; 3],
            yaw: 0.0,
            sheet: crate::combat::Sheet::dev_soldier(),
            guests: crate::Guests::Avatars,
        },
        level.clone(),
    )
    .map_err(|error| error.to_string())?;
    let mut run = Run { world, tick: 0 };
    let mut after = Vec::new();
    for phase in PHASES {
        match phase {
            Phase::Stand(at) => run.settle(at, None),
            Phase::Use(panel) => run.settle(NEAR, Some(panel)),
            Phase::Away => {
                for level in [&elsewhere, &level] {
                    run.world
                        .install(level.clone(), None)
                        .map_err(|error| error.to_string())?;
                    run.settle(NEAR, None);
                }
            }
            Phase::Reload => {
                let saved = Snapshot::decode(&run.world.snapshot().encode())
                    .map_err(|error| error.to_string())?;
                run.world = super::super::SimWorld::from_snapshot(&saved, level.clone())
                    .map_err(|error| error.to_string())?;
            }
        }
        after.push(run.world.snapshot());
    }
    Ok(after)
}

/// Object `index`'s state in `snapshot`.
fn state(snapshot: &Snapshot, index: u32) -> Option<ObjectState> {
    snapshot
        .entities
        .iter()
        .find_map(|entity| match entity.kind {
            crate::Kind::Object(_, at) if at == index => entity.object,
            _ => None,
        })
}

/// The ground's explored cells in `snapshot`.
fn explored(snapshot: &Snapshot) -> Vec<[usize; 2]> {
    let grid: Option<&Explored> = snapshot
        .explored
        .iter()
        .find(|grid| grid.module != "elsewhere");
    let [columns, rows] = chart().cells.map(usize::from);
    (0..rows)
        .flat_map(|row| (0..columns).map(move |column| [column, row]))
        .filter(|&[column, row]| grid.is_some_and(|grid| grid.explored(column, row)))
        .collect()
}

/// Cell `cell`'s nearest point's distance (metres) from ground point `at`.
fn distance(cell: [usize; 2], at: [f32; 2]) -> f32 {
    let chart = chart();
    let [x, y, _] = floor_at(at);
    let point = chart.point([x, y]);
    let mut squared = 0.0;
    for (on, axis) in chart.axes.iter().enumerate() {
        let count = f32::from(chart.cells[on]);
        let (low, high) = (cell[on] as f32 / count, (cell[on] + 1) as f32 / count);
        let gap = (point[on] - point[on].clamp(low, high)) / axis.scale;
        squared += gap * gap;
    }
    squared.sqrt()
}

/// The checks of map pins and fog (see the module documentation).
pub fn probes() -> Vec<(&'static str, Result<String, String>)> {
    let check = |ok: bool, what: String| if ok { Ok(what) } else { Err(what) };
    let after = match scenario() {
        Ok(after) => after,
        Err(why) => return vec![("map_scenario", Err(why))],
    };
    let again = scenario().map(|after| after.last().map(Snapshot::hash));
    let facts = added(&chart());
    // Each note's pin and whether it shows, after each phase.
    let notes = |phase: usize| {
        [SHOWN, HIDDEN].map(|index| {
            let state = after.get(phase).and_then(|snapshot| state(snapshot, index));
            let facts = &facts[(index - SHOWN) as usize];
            state.map(|state| (state.state, state.map_note(facts).is_some()))
        })
    };
    let cells: Vec<Vec<[usize; 2]>> = after.iter().map(explored).collect();
    let within = |phase: usize, at| {
        cells[phase]
            .iter()
            .all(|cell| distance(*cell, at) <= REVEAL_M)
    };
    let far = [[0, 0], [14, 0], [14, 7], [0, 7]];
    let none_far = |phase: usize| far.iter().all(|cell| !cells[phase].contains(cell));
    let walked: Vec<&[usize; 2]> = cells[3]
        .iter()
        .filter(|cell| !cells[0].contains(cell))
        .collect();
    let kept = |phase: usize| cells[3].iter().all(|cell| cells[phase].contains(cell));
    let [column, row] = REVEALED;
    let beside = [
        [column, row],
        [column - 1, row],
        [column + 1, row],
        [column, row - 1],
        [column, row + 1],
    ];
    let corners = [[column - 1, row - 1], [column + 1, row + 1]];
    let revealed = beside.iter().all(|cell| cells[6].contains(cell))
        && corners
            .iter()
            .all(|cell| !cells[6].contains(cell) || cells[5].contains(cell));
    let whole = after[7]
        .explored
        .iter()
        .find(|grid| grid.module != "elsewhere")
        .is_some_and(Explored::whole);
    let round_trip = after
        .last()
        .is_some_and(|last| Snapshot::decode(&last.encode()).as_ref() == Ok(last));
    vec![
        (
            "pins_as_placed",
            check(
                notes(0) == [Some((pin::AS_PLACED, true)), Some((pin::AS_PLACED, false))],
                format!("{:?}", notes(0)),
            ),
        ),
        (
            "pin_off",
            check(
                notes(1) == [Some((pin::OFF, false)), Some((pin::AS_PLACED, false))],
                format!("{:?}", notes(1)),
            ),
        ),
        (
            "pin_on",
            check(
                notes(2) == [Some((pin::OFF, false)), Some((pin::ON, true))],
                format!("{:?}", notes(2)),
            ),
        ),
        (
            "fog_near_not_far",
            check(
                !cells[0].is_empty() && within(0, NEAR) && none_far(0),
                format!("standing by the panels: {:?}", cells[0]),
            ),
        ),
        (
            "fog_walked",
            check(
                !walked.is_empty()
                    && walked
                        .iter()
                        .all(|cell| distance(**cell, ACROSS) <= REVEAL_M)
                    && none_far(3),
                format!("across the room, besides: {walked:?}"),
            ),
        ),
        (
            "fog_kept_away",
            check(
                kept(4)
                    && after[4]
                        .explored
                        .iter()
                        .all(|grid| grid.module != "elsewhere"),
                format!("back from elsewhere: {:?}", cells[4]),
            ),
        ),
        (
            "fog_saved",
            check(
                cells[5] == cells[4] && after[5].explored == after[4].explored,
                format!("after a save and load: {:?}", cells[5]),
            ),
        ),
        (
            "reveal_map",
            check(revealed, format!("round {REVEALED:?}: {:?}", cells[6])),
        ),
        (
            "explore_area",
            check(whole, format!("{} cells explored", cells[7].len())),
        ),
        (
            "map_round_trip",
            check(round_trip, "the last snapshot encodes and decodes".into()),
        ),
        (
            "map_deterministic",
            check(
                again == Ok(after.last().map(Snapshot::hash)),
                format!("{again:?}"),
            ),
        ),
    ]
}
