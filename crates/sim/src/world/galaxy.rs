//! The galaxy map and the game's end, on the proving ground with two
//! seats, each a body, and scripts written here:
//!
//! - a creature's spawn script makes Taris (row 1) available and Dantooine
//!   (row 3) available and selectable, and shows the map on Dantooine:
//!   both seats are shown it, with those planets;
//! - the guest's travel to Dantooine is the host's to make (I30 P6), and
//!   the host's to Taris is refused (not selectable): neither runs the
//!   travel script; the host's to Dantooine runs it, which hears the
//!   planet from `GetSelectedPlanet`;
//! - `EndGame(TRUE)`, and `EndGame()` as K1's older scripts call it, lose
//!   the game (`Snapshot::lost`) and show no end; `EndGame(FALSE)` (Manaan's
//!   sentence) shows both seats the end and loses nothing, and after
//!   `StartCreditSequence` (K1's ending) the credits come first.
//!
//! For the approved test `galaxy_and_end`.

use fmt_odyssey::ncs::{Const, Ncs, Op};

use super::SimWorld;
use crate::action::Action;
use crate::input::Seat;
use crate::module::{ObjectData, Placement};
use crate::notice::{Notice, Screen};
use crate::probe::{Checks, check};
use crate::{Snapshot, StepReason, TickInput, WorldConfig};

const CATALOG: &str = r#"
int TRUE = 1;
// 0
void SendMessageToPC(object oPlayer, string szMessage);
// 1
object GetFirstPC();
// 2
string IntToString(int nInteger);
// 3
int GetSelectedPlanet();
// 4
void SetPlanetAvailable(int nPlanet, int bAvailable);
// 5
void SetPlanetSelectable(int nPlanet, int bSelectable);
// 6
void ShowGalaxyMap(int nPlanet);
// 7
void EndGame(int nShowEndGameGui=TRUE);
// 8
void StartCreditSequence(int bTransparentBackground);
"#;

const TARIS: u8 = 1;
const DANTOOINE: u8 = 3;
/// The steps the map's scenario takes, and those the end's.
const STEPS: usize = 8;

fn int(asm: &mut script_ncs::probes::Asm, value: u8) -> &mut script_ncs::probes::Asm {
    asm.op(Op::Const(Const::Int(i32::from(value))))
}

/// `routine(planet, on)` for each.
fn planets(asm: &mut script_ncs::probes::Asm, calls: &[(u16, u8)]) {
    for &(routine, planet) in calls {
        int(asm, 1);
        int(asm, planet).op(Op::Action { routine, args: 2 });
    }
}

/// The map's spawn script.
fn map() -> Ncs {
    let mut asm = script_ncs::probes::Asm::default();
    planets(&mut asm, &[(4, TARIS), (4, DANTOOINE), (5, DANTOOINE)]);
    int(&mut asm, DANTOOINE).op(Op::Action {
        routine: 6,
        args: 1,
    });
    asm.op(Op::Retn);
    asm.build()
}

/// The travel script: tells the player the planet selected.
fn travel() -> Ncs {
    let mut asm = script_ncs::probes::Asm::default();
    for (routine, args) in [(3, 0), (2, 1), (1, 0), (0, 2)] {
        asm.op(Op::Action { routine, args });
    }
    asm.op(Op::Retn);
    asm.build()
}

/// `EndGame` with `shown` as its argument, or none; the credits started
/// first when `credits`.
fn end(credits: bool, shown: Option<u8>) -> Ncs {
    let mut asm = script_ncs::probes::Asm::default();
    if credits {
        int(&mut asm, 0).op(Op::Action {
            routine: 8,
            args: 1,
        });
    }
    if let Some(shown) = shown {
        int(&mut asm, shown);
    }
    asm.op(Op::Action {
        routine: 7,
        args: u8::from(shown.is_some()),
    })
    .op(Op::Retn);
    asm.build()
}

/// Each step's snapshot, and the notices it told by seat.
type Ran = (Vec<Snapshot>, Vec<Vec<(Seat, Notice)>>);

/// Two seats on the ground, a creature by the entry running `spawn` as it
/// spawns, the travel script beside it; each step's actions by seat. Each
/// step's snapshot and notices.
fn run(spawn: Ncs, steps: &[Vec<(u8, Action)>]) -> Result<Ran, String> {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    let at = data.entry.position;
    data.placements.push(Placement::Own(ObjectData {
        scripts: vec![("ScriptSpawn".into(), "probe_spawn".into())],
        ..ObjectData::creature(
            "herald",
            "herald",
            [at[0] + 2.0, at[1], at[2]],
            crate::combat::Sheet::dev_soldier(),
        )
    }));
    let library = crate::probe::library(
        CATALOG,
        [
            ("probe_spawn", spawn),
            (crate::script::TRAVEL_SCRIPT, travel()),
        ],
    );
    let mut world = SimWorld::new(
        WorldConfig {
            seats: 2,
            guests: crate::Guests::Avatars,
            ..WorldConfig::one_seat()
        },
        ground.with_module(data).with_scripts(library),
    )
    .map_err(|error| error.to_string())?;
    let (mut snapshots, mut notices) = (Vec::new(), Vec::new());
    for (at, actions) in steps.iter().enumerate() {
        let mut input = TickInput::default();
        for seat in [0, 1] {
            input.insert(Seat(seat), crate::Command::default());
        }
        for (seat, action) in actions {
            input.act(Seat(*seat), action.clone());
        }
        snapshots.push(crate::step(
            &mut world,
            at as u64 + 1,
            &input,
            8,
            StepReason::Authority,
        ));
        notices.push(world.take_notices());
    }
    Ok((snapshots, notices))
}

/// The steps a script's notice is told at, and to which seats.
fn told(notices: &[Vec<(Seat, Notice)>], wanted: impl Fn(&Notice) -> bool) -> Vec<(usize, u8)> {
    notices
        .iter()
        .enumerate()
        .flat_map(|(at, step)| {
            step.iter()
                .filter(|(_, notice)| wanted(notice))
                .map(move |(seat, _)| (at, seat.0))
        })
        .collect()
}

fn galaxy() -> Checks {
    // The map shows in the first steps; travels from step 4, one a step.
    let mut steps = vec![Vec::new(); STEPS];
    steps[4] = vec![(1, Action::Travel(DANTOOINE)), (0, Action::Travel(TARIS))];
    steps[6] = vec![(0, Action::Travel(DANTOOINE))];
    let (_, notices) = match run(map(), &steps) {
        Ok(ran) => ran,
        Err(why) => return vec![("galaxy_scenario", Err(why))],
    };
    let shown = told(&notices, |notice| {
        *notice
            == Notice::Show(Screen::GalaxyMap {
                planet: DANTOOINE,
                available: 1 << TARIS | 1 << DANTOOINE,
                selectable: 1 << DANTOOINE,
            })
    });
    let travelled = told(&notices, |notice| {
        *notice == Notice::Said(DANTOOINE.to_string())
    });
    let shown_at = shown.first().map(|(at, _)| *at);
    vec![
        (
            "galaxy_shown_to_both",
            check(
                shown.len() == 2
                    && shown_at.is_some_and(|at| at < 4)
                    && shown.iter().all(|(at, _)| Some(*at) == shown_at),
                format!("shown {shown:?}"),
            ),
        ),
        (
            "galaxy_host_travels",
            check(
                !travelled.is_empty() && travelled.iter().all(|(at, _)| *at >= 6),
                format!("the travel script told {travelled:?}"),
            ),
        ),
    ]
}

fn ending() -> Checks {
    let steps = vec![Vec::new(); STEPS];
    let both = |screens: &[Screen]| -> Vec<(u8, Screen)> {
        [0, 1]
            .iter()
            .flat_map(|seat| screens.iter().map(move |screen| (*seat, screen.clone())))
            .collect()
    };
    [
        (
            "end_lost_with_death_screen",
            false,
            Some(1),
            true,
            Vec::new(),
        ),
        ("end_lost_by_default", false, None, true, Vec::new()),
        (
            "end_without_death_screen",
            false,
            Some(0),
            false,
            both(&[Screen::End]),
        ),
        (
            "end_after_credits",
            true,
            Some(0),
            false,
            both(&[Screen::Credits, Screen::End]),
        ),
    ]
    .into_iter()
    .map(|(name, credits, shown, loses, wanted)| {
        let result = run(end(credits, shown), &steps).and_then(|(snapshots, notices)| {
            let lost = snapshots.last().and_then(|last| last.lost);
            let mut screens: Vec<(u8, Screen)> = notices
                .iter()
                .flatten()
                .filter_map(|(seat, notice)| match notice {
                    Notice::Show(screen @ (Screen::End | Screen::Credits)) => {
                        Some((seat.0, screen.clone()))
                    }
                    _ => None,
                })
                .collect();
            screens.sort_by_key(|(seat, _)| *seat);
            check(
                lost.is_some() == loses && screens == wanted,
                format!("lost at {lost:?}, shown {screens:?}"),
            )
        });
        (name, result)
    })
    .collect()
}

/// See the module documentation.
pub fn probes() -> Checks {
    let mut checks = galaxy();
    checks.extend(ending());
    checks
}
