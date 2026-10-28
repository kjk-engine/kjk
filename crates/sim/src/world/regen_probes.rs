//! The probes of natural healing (`kjk sim probes regeneration`), on the
//! proving ground with two seats (a co-op party), no install:
//! - K1's (`world::powers::regenerate_k1`, its `regeneration.2da`'s
//!   `healthregen`): as K1 ships it (0 in combat and out), a whole point
//!   every 10,000 s, so nothing in a minute; at 1 % a second out of combat,
//!   both players heal that share of their most, whole points at a time;
//!   in combat its in-combat row (0: the floor) holds;
//! - TSL's (`world::powers::regenerate`, its `regeneration.2da` as shipped):
//!   both players heal a point every `TimePerHP` (6 s) and more, never past
//!   their most.
//!
//! Each check says the hit points it saw second by second.

use super::{Fighting, Id, SimWorld};
use crate::input::{Seat, TickInput};
use crate::probe::{Checks, check};
use crate::script::HealthRegeneration;
use crate::snapshot::{Kind, SimId, Snapshot};
use crate::{StepReason, WorldConfig};

const MSEC: u32 = 8;
/// The seconds each run stands.
const SECONDS: u64 = 60;

/// K1's `regeneration.2da` as the Steam install ships it.
const K1: HealthRegeneration = HealthRegeneration {
    in_combat: 0.0,
    out_of_combat: 0.0,
};

/// Where the healing comes from.
#[derive(Clone, Copy)]
enum Rule {
    K1(HealthRegeneration),
    Tsl,
}

/// Each player's hit points (hundredths) every second of `SECONDS`, from a
/// third of its most, standing (in a fight throughout when `fighting`);
/// and its most.
fn healing(rule: Rule, fighting: bool) -> Result<Vec<(Vec<i32>, i32)>, String> {
    let library = crate::probe::library("", std::iter::empty::<(String, _)>());
    let library = match rule {
        Rule::K1(rates) => library.with_health_regeneration(Some(rates)),
        Rule::Tsl => library.with_regeneration(Some(super::tsl_probes::TSL)),
    };
    let level = crate::determinism::proving_ground().with_scripts(library);
    let config = WorldConfig {
        seats: 2,
        ..WorldConfig::one_seat()
    };
    let mut world = SimWorld::new(config, level).map_err(|error| error.to_string())?;
    let first = step(&mut world);
    let players: Vec<(SimId, i32)> = first
        .entities
        .iter()
        .filter(|entity| matches!(entity.kind, Kind::Avatar(_)))
        .filter_map(|entity| Some((entity.id, i32::from(entity.sheet?.max_hp) * 100)))
        .collect();
    if players.len() != 2 {
        return Err(format!("{} players", players.len()));
    }
    let mut runs: Vec<(Vec<i32>, i32)> = players
        .iter()
        .map(|(_, max)| (vec![max / 3], *max))
        .collect();
    for (id, mut fighter) in world
        .world
        .query::<(&Id, &mut Fighting)>()
        .iter_mut(&mut world.world)
    {
        if let Some((_, max)) = players.iter().find(|(me, _)| *me == id.0) {
            fighter.0.hp = max / 3;
            if fighting {
                fighter.0.combat_until = u64::MAX;
            }
        }
    }
    for _ in 0..SECONDS {
        let mut last = first.clone();
        for _ in 0..1000 / MSEC {
            last = step(&mut world);
        }
        for ((id, _), (hp, _)) in players.iter().zip(runs.iter_mut()) {
            hp.push(last.get(*id).and_then(|e| e.fighter).map_or(0, |f| f.hp));
        }
    }
    Ok(runs)
}

fn step(world: &mut SimWorld) -> Snapshot {
    let mut input = TickInput::default();
    for seat in 0..2 {
        input.insert(Seat(seat), crate::input::Command::default());
    }
    let tick = world.tick() + 1;
    crate::step(world, tick, &input, MSEC, StepReason::Authority)
}

/// Each run's whole hit points by second, for the check's words.
fn told(runs: &[(Vec<i32>, i32)]) -> String {
    runs.iter()
        .map(|(hp, max)| {
            let whole: Vec<String> = hp.iter().map(|hp| (hp / 100).to_string()).collect();
            format!("[{}] of {}", whole.join(" "), max / 100)
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// Whether every run heals as `heals` says from its start to its end, and
/// never past its most.
fn checked(
    runs: Result<Vec<(Vec<i32>, i32)>, String>,
    heals: impl Fn(i32, i32, i32) -> bool,
) -> Result<String, String> {
    let runs = runs?;
    let ok = runs.iter().all(|(hp, max)| {
        let (start, end) = (hp[0], hp[hp.len() - 1]);
        hp.iter().all(|hp| hp <= max) && heals(start, end, *max)
    });
    check(ok, told(&runs))
}

pub fn probes() -> Checks {
    let one_percent = HealthRegeneration {
        out_of_combat: 1.0,
        ..K1
    };
    vec![
        (
            "k1_shipped_rate",
            check(
                K1.every_ms(30, false) == 10_000_000 && K1.every_ms(300, true) == 10_000_000,
                format!(
                    "a point every {} ms out of combat, {} in it",
                    K1.every_ms(30, false),
                    K1.every_ms(300, true)
                ),
            ),
        ),
        (
            "k1_shipped_none_in_a_minute",
            checked(healing(Rule::K1(K1), false), |start, end, _| start == end),
        ),
        (
            "k1_percent_heals",
            // 1 % of the most a second: 60 % of it in a minute, give or
            // take the whole point at each end.
            checked(healing(Rule::K1(one_percent), false), |start, end, max| {
                let healed = (end - start) / 100;
                let expected = max * 60 / 100 / 100;
                (expected - 1..=expected + 1).contains(&healed)
            }),
        ),
        (
            "k1_in_combat_row",
            checked(healing(Rule::K1(one_percent), true), |start, end, _| {
                start == end
            }),
        ),
        (
            "tsl_heals",
            // A point (and the CON and Treat Injury shares) every 6 s: at
            // least ten points in a minute, or whole.
            checked(healing(Rule::Tsl, false), |start, end, max| {
                end >= (start + 1000).min(max)
            }),
        ),
    ]
}
