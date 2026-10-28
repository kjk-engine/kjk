//! Items used and upgraded, on the proving ground, for the approved test
//! `item_uses`: consumables the party carries (`consumables`), a droid's
//! worn utility items and shield (`droids`) and a lightsaber's crystals
//! (`upgrades`), each scenario stepped twice to end alike.

use std::collections::BTreeMap;

use crate::action::Action;
use crate::combat::Sheet;
use crate::combat::items::Carried;
use crate::input::Seat;
use crate::inventory::{Catalog, ItemFacts, ItemSpell};
use crate::module::{ObjectKind, Placement};
use crate::notice::Notice;
use crate::probe::check;
use crate::script::{Library, State, Talent, Talents};
use crate::snapshot::{EntityState, SimId};
use crate::{Command, Snapshot, StepReason, TickInput, WorldConfig};

use super::super::SimWorld;

mod consumables;
mod droids;
mod upgrades;

type Checks = Vec<(&'static str, Result<String, String>)>;

/// An item casting `row` at `cost` (`iprp_chargecost.2da`), stacking to
/// ten.
fn casting(template: &str, row: u16, cost: u8) -> ItemFacts {
    ItemFacts {
        stacking: 10,
        spell: Some(ItemSpell { row, cost }),
        ..super::super::containers::plain(template)
    }
}

/// `spells.2da` rows up to `last`, those `hostile` used on a foe, each
/// reaching `reach_dm`.
fn spells(last: u16, hostile: impl Fn(u16) -> bool, reach_dm: u16) -> Vec<Talent> {
    (0..=last)
        .map(|row| Talent {
            hostile: hostile(row),
            reach_dm,
            ..Talent::default()
        })
        .collect()
}

/// A world on the proving ground: one seat whose body has `sheet`, the
/// creatures tagged `creatures` placed in a row ahead of the entry (1.5 m
/// apart), and the scripts' library of `catalog` and `spells`, the rows
/// `scripted` with an impact script that does nothing. The host's id and
/// the creatures'.
fn stage(
    sheet: Sheet,
    catalog: Catalog,
    spells: Vec<Talent>,
    scripted: &[u16],
    creatures: &[&str],
) -> Result<(SimWorld, u32, Vec<u32>), String> {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    let entry = data.entry.position;
    for (at, tag) in creatures.iter().enumerate() {
        let x = entry[0] + 1.5 * (at as f32 + 1.0);
        data.placements
            .push(Placement::Own(crate::module::ObjectData {
                sheet: Some(Sheet::dev_soldier()),
                ..crate::module::ObjectData::bare(
                    ObjectKind::Creature,
                    tag,
                    [x, entry[1], entry[2]],
                )
            }));
    }
    let routines = script_ncs::catalog::Catalog::parse("").map_err(|error| error.to_string())?;
    const IMPACT: &str = "impact";
    let impacts = (0..spells.len() as u16)
        .map(|row| {
            if scripted.contains(&row) {
                IMPACT.to_string()
            } else {
                String::new()
            }
        })
        .collect();
    let talents = Talents {
        spells,
        impacts,
        ..Talents::default()
    };
    let mut nothing = script_ncs::probes::Asm::default();
    nothing.op(fmt_odyssey::ncs::Op::Retn);
    let programs = BTreeMap::from([(IMPACT.to_string(), nothing.build())]);
    let library = Library::new(routines, programs, talents).with_items(catalog);
    let mut world = SimWorld::new(
        WorldConfig {
            sheet,
            ..WorldConfig::one_seat()
        },
        ground.with_module(data).with_scripts(library),
    )
    .map_err(|error| error.to_string())?;
    let start = world.snapshot();
    let objects = crate::Instances::in_snapshot(world.level(), &start);
    let host = start
        .body(Seat(0))
        .map(|entity| entity.id.0)
        .ok_or("no host")?;
    let placed = creatures
        .iter()
        .map(|tag| {
            start
                .entities
                .iter()
                .find(|entity| {
                    objects
                        .of_entity(entity)
                        .is_some_and(|data| data.tag == *tag)
                })
                .map(|entity| entity.id.0)
                .ok_or(format!("no {tag}"))
        })
        .collect::<Result<_, _>>()?;
    Ok((world, host, placed))
}

/// Each step's snapshot and scripts' state, and every notice with its
/// step, of the host `host`; the scenario's items' ids.
struct Ran {
    host: u32,
    items: Vec<u32>,
    snapshots: Vec<Snapshot>,
    states: Vec<State>,
    notices: Vec<(usize, Seat, Notice)>,
}

/// `world` stepped through `steps`: what the host holds down and the
/// actions its seat sends, a step each (1.6 s: a grenade's weapon wait and
/// a medpac's use run out between steps).
fn run(mut world: SimWorld, host: u32, steps: Vec<(u16, Vec<Action>)>) -> Ran {
    let (mut snapshots, mut states, mut notices) = (Vec::new(), Vec::new(), Vec::new());
    for (at, (held, actions)) in steps.into_iter().enumerate() {
        let mut input = TickInput::default();
        input.insert(
            Seat(0),
            Command {
                buttons: held,
                ..Command::default()
            },
        );
        for action in actions {
            input.act(Seat(0), action);
        }
        snapshots.push(crate::step(
            &mut world,
            at as u64 + 1,
            &input,
            1600,
            StepReason::Authority,
        ));
        states.push(world.script_state().clone());
        notices.extend(
            world
                .take_notices()
                .into_iter()
                .map(|(seat, notice)| (at, seat, notice)),
        );
    }
    Ran {
        host,
        items: Vec::new(),
        snapshots,
        states,
        notices,
    }
}

impl Ran {
    fn host(&self, step: usize) -> Option<&EntityState> {
        self.snapshots[step].get(SimId(self.host))
    }

    /// The host's usable kinds after `step`, and what the HUD shows left
    /// of each (`count - used`).
    fn kinds(&self, step: usize) -> Vec<(u16, u8)> {
        let Some(host) = self.host(step) else {
            return Vec::new();
        };
        let used = host.fighter.map_or([0; 4], |fighter| fighter.pack.used);
        host.sheet
            .map_or([None; 4], |sheet| sheet.items)
            .iter()
            .zip(used)
            .filter_map(|(kind, used)| {
                kind.map(|Carried { row, count }| (row, count.saturating_sub(used)))
            })
            .collect()
    }

    /// The stack and charges of `item` after `step` (none: gone).
    fn held(&self, step: usize, item: u32) -> Option<(u16, u8)> {
        let item = self.states[step].items.get(&item)?;
        Some((item.stack, item.charges))
    }

    fn told(&self, step: usize, notice: &Notice) -> bool {
        self.notices
            .iter()
            .any(|(at, seat, told)| *at == step && *seat == Seat(0) && told == notice)
    }

    fn quiet(&self, step: usize) -> bool {
        self.notices.iter().all(|(at, ..)| *at != step)
    }
}

/// A scenario's checks, and that two runs of it end alike.
fn twice(
    name: &'static str,
    scenario: impl Fn() -> Result<Ran, String>,
    cases: impl Fn(&Ran) -> Checks,
) -> Checks {
    let ran = match scenario() {
        Ok(ran) => ran,
        Err(why) => return vec![(name, Err(why))],
    };
    let again = scenario().map(|again| again.snapshots.last().map(Snapshot::hash));
    let mut checks = cases(&ran);
    let last = ran.snapshots.last().map(Snapshot::hash);
    checks.push((
        name,
        check(
            again.as_ref().is_ok_and(|hash| *hash == last),
            format!(
                "the same every run: {:?} then {:?}",
                last.map(|hash| hash.short()),
                again.map(|hash| hash.map(|hash| hash.short()))
            ),
        ),
    ));
    checks
}

pub fn probes() -> Checks {
    let mut checks = twice(
        "consumables_same_every_run",
        consumables::scenario,
        consumables::cases,
    );
    checks.extend(twice(
        "droids_same_every_run",
        droids::scenario,
        droids::cases,
    ));
    checks.extend(twice(
        "upgrades_same_every_run",
        upgrades::scenario,
        upgrades::cases,
    ));
    checks
}
