//! KOTOR's encounters ("UTE encounters"): the GIT's `Encounter List`
//! objects, each made from a UTE, that spawn creatures once something
//! hostile to them walks into their outline, as both games' executables
//! run them (`CSWSEncounter`). Not our own per-module creature overrides
//! (`session::encounters`, I28), which change what a module places.
//!
//! An encounter is `Active` until it exhausts. When a living creature
//! hostile to its faction (with `PlayerOnly`, a player or party member)
//! stands in its outline while it is active and not yet `Started`, it
//! builds its spawn list once (`spawn_if_appropriate`), and the world's
//! step makes one creature of that list each step until it is empty
//! (`world::spawners`). The games pick differently:
//! - TSL: a count uniform in `RecCreatures..=MaxCreatures`, every
//!   `SingleSpawn` creature once, `GuaranteedCount` copies of each other
//!   one, the rest drawn from the others (`tsl_list`);
//! - K1: NWN's challenge budget, from the levels of those near whoever last
//!   entered it (`area_points`, `party_cr`), picked greedily by challenge,
//!   split and topped up to `RecCreatures` (`k1_list`).
//!
//! When every creature it made has died or gone it signals `OnExhausted`
//! and turns inactive; with `Reset` and `SpawnOption` 1 it turns active
//! again `ResetTime` seconds later, `Respawns` times at most (-1: always).
//! Every draw is the authority stream's. What the executables save is
//! saved (`Spawn`); the creature's link to its encounter is not, as in the
//! games (QUIRKS.md Q-0261).

use std::collections::BTreeMap;

use collision::nav::NavMesh;
use script_ncs::vm::ObjectId;

use crate::module::{ModuleData, ObjectData, inside_outline};
use crate::rng::SimRng;
use crate::script::{State, View};
use crate::wire::{DecodeError, Reader, Writer};
use rules::encounters::making;

/// The game whose executable's own rules (`ModuleData::game`): TSL picks
/// and places as its executable does, every other game as K1's.
const TSL: &str = "tsl";

/// What an encounter's UTE and GIT entry give it (the outline and faction
/// are its `ObjectData`'s).
#[derive(Debug, Clone, PartialEq)]
pub struct Encounter {
    /// Spawns only while active (`Active`).
    pub active: bool,
    /// Exhausted, it may turn active again (`Reset`), after
    /// `reset_time_s` seconds, when `spawn_option` is 1, `respawns` times
    /// at most (-1: always).
    pub reset: bool,
    pub reset_time_s: i32,
    pub respawns: i32,
    pub spawn_option: i32,
    /// How many it makes: TSL's range, K1's cap and recommended least.
    pub max_creatures: i32,
    pub rec_creatures: i32,
    /// Only a player or party member starts it (`PlayerOnly`).
    pub player_only: bool,
    /// `encdifficulty.2da`'s value for its `DifficultyIndex` (the UTE's
    /// `Difficulty` without the table): K1's budget adds it.
    pub difficulty: i32,
    /// Its `CreatureList`, by challenge rating, highest first (the
    /// executables sort it so as they load it; equals keep their order).
    pub creatures: Vec<Creature>,
    /// Its `SpawnPointList` (absolute places).
    pub points: Vec<Point>,
}

/// One creature an encounter may make.
#[derive(Debug, Clone, PartialEq)]
pub struct Creature {
    pub template: String,
    /// Its challenge rating as the encounter counts it (`CR`).
    pub cr: f32,
    /// Made once a list at most (`SingleSpawn`).
    pub single: bool,
    /// TSL: made this many times a list (`GuaranteedCount`).
    pub guaranteed: i32,
}

/// A spawn point: where (metres) and its `Orientation` (radians, as the
/// GIT has it: the executables face a creature made there along
/// `(sin o, cos o)`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub at: [f32; 3],
    pub orientation: f32,
}

impl Point {
    /// The facing of a creature made here (degrees from +X): the angle of
    /// `(cos(π/2 − o), sin(π/2 − o))`, which is `π/2 − o`.
    pub fn yaw(&self) -> f32 {
        90.0 - self.orientation.to_degrees()
    }
}

impl Encounter {
    /// `creatures` in the order the executables keep them: by challenge
    /// rating, highest first, equals in their own order.
    pub fn sorted(mut creatures: Vec<Creature>) -> Vec<Creature> {
        creatures.sort_by(|a, b| b.cr.partial_cmp(&a.cr).unwrap_or(std::cmp::Ordering::Equal));
        creatures
    }

    pub(crate) fn digest_into(&self, out: &mut Writer) {
        out.u8(u8::from(self.active) | u8::from(self.reset) << 1 | u8::from(self.player_only) << 2);
        for value in [
            self.reset_time_s,
            self.respawns,
            self.spawn_option,
            self.max_creatures,
            self.rec_creatures,
            self.difficulty,
        ] {
            out.u32(value as u32);
        }
        out.u32(self.creatures.len() as u32);
        for creature in &self.creatures {
            out.string(&creature.template);
            out.f32(creature.cr);
            out.u8(u8::from(creature.single));
            out.u32(creature.guaranteed as u32);
        }
        out.u32(self.points.len() as u32);
        for point in &self.points {
            point.at.iter().for_each(|axis| out.f32(*axis));
            out.f32(point.orientation);
        }
    }
}

/// The 2DAs encounters read (identical in both games): `encdifficulty.2da`'s
/// `VALUE` by row (`SetEncounterDifficulty`) and `fractionalcr.2da`'s `min`
/// by row (K1's points of a challenge under 1).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Tables {
    pub difficulties: Vec<i32>,
    pub fractional: Vec<f32>,
}

impl Tables {
    pub(crate) fn digest_into(&self, out: &mut Writer) {
        out.u32(self.difficulties.len() as u32);
        self.difficulties.iter().for_each(|v| out.u32(*v as u32));
        out.u32(self.fractional.len() as u32);
        self.fractional.iter().for_each(|v| out.f32(*v));
    }
}

/// `fractionalcr.2da`'s first four `min` as both games ship them, for a
/// level without the table.
const FRACTIONAL: [f32; 4] = [0.40, 0.30, 0.20, 0.15];

/// What an encounter keeps as it runs, by its object id
/// (`script::State::encounters`). The first fields are what the
/// executables save (`SaveEncounter`); the rest they keep out of their
/// save, and so does ours.
#[derive(Debug, Clone, PartialEq)]
pub struct Spawn {
    pub active: bool,
    /// Its spawn list built since it last exhausted (`Started`).
    pub started: bool,
    /// Most resets and resets done (`Respawns`, `CurrentSpawns`).
    pub respawns: i32,
    pub current_spawns: i32,
    /// `Difficulty` as scripts left it (a value, not a row).
    pub difficulty: i32,
    /// Its creatures alive (`NumberSpawned`): one more each made, one less
    /// each that died or went.
    pub number_spawned: i32,
    /// The creatures still to make, first first (`SpawnList`): template
    /// and challenge rating.
    pub list: Vec<(String, f32)>,
    /// The last to come in and to leave (`LastEntered`, `LastLeft`).
    pub last_entered: Option<u32>,
    pub last_left: Option<u32>,
    /// When it last exhausted with `Reset` (sim ms; `LastSpawnDay`,
    /// `LastSpawnTime`), until it resets.
    pub last_spawn_ms: Option<u64>,
    /// When its heartbeat was last stamped (`HeartbeatTime`); none yet,
    /// and the first is skipped.
    pub heartbeat_ms: Option<u64>,
    /// K1: its challenge budget's pool, grown by each budget computed and
    /// spent by nothing (the executable's spawning takes the entry's
    /// zero); not saved.
    pub pool: f32,
    /// TSL: which spawn point the next creature takes; not saved.
    pub cursor: Cursor,
}

/// TSL's round of its spawn points (`SpawnCreatureFromSpawnList`): the
/// point, how many took it, and how many each takes (a list's count over
/// its points, whole: 0 when the points outnumber the list, QUIRKS.md
/// Q-0262).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Cursor {
    pub index: i32,
    pub count: i32,
    pub per_point: i32,
}

/// A creature an encounter made: which, and whether its death or going
/// was counted (`RemoveFromActiveCreatureCount`, once a creature). Not
/// saved (`script::State::encounter_creatures`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Link {
    pub encounter: u32,
    pub counted: bool,
}

impl Spawn {
    /// An encounter as its UTE starts it.
    pub fn new(facts: &Encounter) -> Spawn {
        Spawn {
            active: facts.active,
            started: false,
            respawns: facts.respawns,
            current_spawns: 0,
            difficulty: facts.difficulty,
            number_spawned: 0,
            list: Vec::new(),
            last_entered: None,
            last_left: None,
            last_spawn_ms: None,
            heartbeat_ms: None,
            pool: 0.0,
            cursor: Cursor::default(),
        }
    }

    /// Its saved fields.
    pub(crate) fn encode(&self, out: &mut Writer) {
        out.u8(u8::from(self.active) | u8::from(self.started) << 1);
        for value in [
            self.respawns,
            self.current_spawns,
            self.difficulty,
            self.number_spawned,
        ] {
            out.u32(value as u32);
        }
        out.u32(self.list.len() as u32);
        for (template, cr) in &self.list {
            out.string(template);
            out.f32(*cr);
        }
        for id in [self.last_entered, self.last_left] {
            out.u32(id.unwrap_or(u32::MAX));
        }
        for at in [self.last_spawn_ms, self.heartbeat_ms] {
            out.u64(at.unwrap_or(u64::MAX));
        }
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Spawn, DecodeError> {
        let flags = input.u8()?;
        let mut int = || -> Result<i32, DecodeError> { Ok(input.u32()? as i32) };
        let (respawns, current_spawns, difficulty, number_spawned) =
            (int()?, int()?, int()?, int()?);
        let list = (0..input.u32()?)
            .map(|_| Ok((input.string()?, input.f32()?)))
            .collect::<Result<_, DecodeError>>()?;
        let mut id = || -> Result<Option<u32>, DecodeError> {
            Ok(Some(input.u32()?).filter(|id| *id != u32::MAX))
        };
        let (last_entered, last_left) = (id()?, id()?);
        let mut at = || -> Result<Option<u64>, DecodeError> {
            Ok(Some(input.u64()?).filter(|at| *at != u64::MAX))
        };
        let (last_spawn_ms, heartbeat_ms) = (at()?, at()?);
        Ok(Spawn {
            active: flags & 1 != 0,
            started: flags & 2 != 0,
            respawns,
            current_spawns,
            difficulty,
            number_spawned,
            list,
            last_entered,
            last_left,
            last_spawn_ms,
            heartbeat_ms,
            pool: 0.0,
            cursor: Cursor::default(),
        })
    }
}

pub(crate) fn encode_spawns(out: &mut Writer, spawns: &BTreeMap<u32, Spawn>) {
    out.u32(spawns.len() as u32);
    for (id, spawn) in spawns {
        out.u32(*id);
        spawn.encode(out);
    }
}

pub(crate) fn decode_spawns(input: &mut Reader) -> Result<BTreeMap<u32, Spawn>, DecodeError> {
    (0..input.u32()?)
        .map(|_| Ok((input.u32()?, Spawn::decode(input)?)))
        .collect()
}

/// Whether `module` runs TSL's executable's rules.
pub fn tsl(module: &ModuleData) -> bool {
    module.game == TSL
}

/// 1.5 to the power `n`, by whole steps in double precision (the
/// executables' table, `m_sPowerLookup`, is `pow(1.5, i)` stored as floats).
fn power64(n: i32) -> f64 {
    let whole = (0..n.unsigned_abs()).fold(1.0f64, |p, _| p * 1.5);
    if n < 0 { 1.0 / whole } else { whole }
}

/// K1's points of challenge rating `cr`: a fraction of 1 by
/// `fractionalcr.2da`'s rows (`fractional`) for one under 1, 1.5 to the
/// whole part for one from 0 to 50, else 1.5 to it (only the party's
/// whole challenges reach there: by whole steps).
pub fn points(cr: f32, fractional: &[f32]) -> f32 {
    if cr > 0.0 && cr < 1.0 {
        let min = |row: usize| fractional.get(row).copied().unwrap_or(FRACTIONAL[row]);
        return if cr >= min(0) {
            1.0
        } else if cr >= min(1) {
            2.0 / 3.0
        } else if cr >= min(2) {
            4.0 / 9.0
        } else if cr >= min(3) {
            8.0 / 27.0
        } else {
            16.0 / 81.0
        };
    }
    // Whole parts by truncation, as the executable converts.
    power64(cr as i32) as f32
}

/// K1's party challenge from the points near the one who entered:
/// `ln(points / 4) / ln 1.5 + 0.5`, truncated toward zero, as the
/// executable works it out in doubles. Found here by the powers of 1.5 it
/// lies between, so no logarithm is needed (the libraries' differ in their
/// last bits): a tie on a boundary may round the other way [verify].
pub fn party_cr(area_points: f32) -> i32 {
    let x = f64::from(area_points) * 0.25;
    let edge = |n: i32| power64(n) / 1.5f64.sqrt();
    const FURTHEST: i32 = 400;
    let mut n = 0;
    if x >= edge(0) {
        while n < FURTHEST && x >= edge(n + 1) {
            n += 1;
        }
    } else {
        while n > -FURTHEST && x <= edge(n - 1) {
            n -= 1;
        }
    }
    n
}

/// TSL's spawn list (`BuildInitialSpawnList`), by index into the creature
/// list: a count drawn in `RecCreatures..=MaxCreatures` (one draw, even
/// when the range is one), then each single creature once in order, then
/// each other one its `GuaranteedCount` times, then the rest drawn from
/// the others. Nothing when the count is not positive or the list empty;
/// only the singles when every creature is one.
pub fn tsl_list(facts: &Encounter, rng: &mut SimRng) -> Vec<usize> {
    let span = facts.max_creatures + 1 - facts.rec_creatures;
    // A range the data never has (its maximum under its least) would be the
    // executable's division by nothing; the least then.
    let drawn = u32::try_from(span)
        .ok()
        .filter(|span| *span > 0)
        .map(|span| rng.below(span));
    let mut count = facts.rec_creatures + drawn.map_or(0, |drawn| drawn as i32);
    let mut out = Vec::new();
    if count <= 0 || facts.creatures.is_empty() {
        return out;
    }
    for (index, creature) in facts.creatures.iter().enumerate() {
        if creature.single {
            out.push(index);
            count -= 1;
            if count == 0 {
                return out;
            }
        }
    }
    let others: Vec<usize> = (0..facts.creatures.len())
        .filter(|index| !facts.creatures[*index].single)
        .collect();
    if others.is_empty() {
        return out;
    }
    for &index in &others {
        let guaranteed = facts.creatures[index].guaranteed;
        for made in 0..guaranteed.max(0) {
            out.push(index);
            if made + 1 == count {
                return out;
            }
        }
        if guaranteed > 0 {
            count -= guaranteed;
        }
    }
    while count > 0 {
        out.push(others[rng.below(others.len() as u32) as usize]);
        count -= 1;
    }
    out
}

/// What K1's list is built against: the most it may hold, the least it is
/// topped up to (both less those alive), its pool and the 2DA's rows.
struct Budget<'a> {
    room: i32,
    least: i32,
    most: i32,
    pool: f32,
    fractional: &'a [f32],
}

/// K1's spawn list (`BuildSpawnList`), by index into the creature list:
/// the greedy pick within the pool by challenge, highest first
/// (`k1_initial`), dear picks split into cheaper ones toward
/// `RecCreatures` while the pool is over the cheapest creature's points
/// (`k1_split`), then a top-up from the cheapest (`k1_top_up`).
pub fn k1_list(
    facts: &Encounter,
    fractional: &[f32],
    pool: f32,
    number_spawned: i32,
    rng: &mut SimRng,
) -> Vec<usize> {
    let creatures = &facts.creatures;
    let worth: Vec<f32> = creatures.iter().map(|c| points(c.cr, fractional)).collect();
    let Some(&cheapest) = worth.last() else {
        return Vec::new();
    };
    let budget = Budget {
        room: facts.max_creatures - number_spawned,
        least: facts.rec_creatures - number_spawned,
        most: facts.max_creatures,
        pool,
        fractional,
    };
    let mut used = vec![false; creatures.len()];
    let mut out = k1_initial(creatures, &worth, &budget, &mut used, rng);
    if (out.len() as i32) < budget.least && pool > cheapest {
        for at in (0..out.len()).rev() {
            if out.len() as i32 >= budget.least {
                break;
            }
            k1_split(
                creatures, &worth, cheapest, &budget, &mut used, &mut out, at,
            );
        }
    }
    if (out.len() as i32) < budget.least {
        k1_top_up(creatures, &worth, &budget, &mut used, &mut out, rng);
    }
    out
}

/// K1's greedy pick: `remaining` the pool, with a share of it as
/// tolerance, walking the creatures highest first while some remains and
/// there is room. An unused single is taken free (the executable marks only
/// its copy used); one of no challenge or dearer than what remains is
/// passed by; one whose challenge the next does not share is taken again
/// and again while it fits; a run sharing a challenge is drawn from at
/// random, each draw taken unless it is a used single, until room is out,
/// every one of an all-single run was drawn, or the draw no longer fits.
fn k1_initial(
    creatures: &[Creature],
    worth: &[f32],
    budget: &Budget,
    used: &mut [bool],
    rng: &mut SimRng,
) -> Vec<usize> {
    let mut out = Vec::new();
    let mut remaining = budget.pool;
    let tolerance = remaining / rules::encounters::starting::TOLERANCE;
    let mut picked = vec![false; creatures.len()];
    let mut i = 0;
    while i < creatures.len() {
        if remaining <= 0.0 || out.len() as i32 >= budget.room {
            break;
        }
        let creature = &creatures[i];
        if creature.single && !used[i] {
            out.push(i);
            i += 1;
            continue;
        }
        if creature.cr == 0.0 || worth[i] > tolerance + remaining {
            i += 1;
            continue;
        }
        let run = creatures[i..]
            .iter()
            .take_while(|other| other.cr == creature.cr)
            .count();
        if run == 1 {
            // A used single is never taken again.
            while !creature.single && (out.len() as i32) < budget.room {
                out.push(i);
                remaining -= worth[i];
                if tolerance + remaining < worth[i] {
                    break;
                }
            }
            i += 1;
            continue;
        }
        let singles = 1 + creatures[i + 1..i + run]
            .iter()
            .filter(|other| other.single)
            .count();
        for flag in &mut picked[i + 1..i + run] {
            *flag = false;
        }
        // The executable compares the pool with a float read from before
        // the creature array (its heap header): taken as passing.
        let mut drawn = 0;
        while (out.len() as i32) < budget.room {
            let k = i + rng.below(run as u32) as usize;
            if !(creatures[k].single && used[k]) {
                out.push(k);
                used[k] = true;
                remaining -= worth[k];
            }
            if !picked[k] {
                picked[k] = true;
                drawn += 1;
            }
            if (run == singles && drawn == run) || tolerance + remaining < worth[k] {
                break;
            }
        }
        i += run;
    }
    out
}

/// K1's split of pick `at` (`out[at]`): unless it is worth the cheapest
/// creature's points, it is taken out and its points filled again from the
/// creatures worth less (by more than `encounters.starting.split_margin`), in order, each again and
/// again while it fits and the list is under the most (a single once),
/// stopping at the first creature once the list is past the least.
fn k1_split(
    creatures: &[Creature],
    worth: &[f32],
    cheapest: f32,
    budget: &Budget,
    used: &mut [bool],
    out: &mut Vec<usize>,
    at: usize,
) {
    let Some(&split) = out.get(at) else { return };
    let whole = points(creatures[split].cr, budget.fractional);
    if whole == cheapest {
        return;
    }
    out.remove(at);
    let under = whole + -rules::encounters::starting::SPLIT_MARGIN;
    let mut left = whole;
    for (j, creature) in creatures.iter().enumerate() {
        if out.len() as i32 > budget.least {
            return;
        }
        let w = worth[j];
        if left < w || under < w {
            continue;
        }
        while (out.len() as i32) < budget.most && !(creature.single && used[j]) {
            out.push(j);
            used[j] = true;
            left -= w;
            if left < w {
                break;
            }
        }
    }
}

/// The most draws one worth's top-up makes: a guard on a worth whose span
/// holds only used singles of another worth, which the sorted lists of the
/// games never have (the executable would draw on for good).
const TOP_UP_DRAWS: usize = 4096;

/// K1's top-up toward `RecCreatures`: from the cheapest worth that is
/// dearer than the last tried and not a used single, scanning from the
/// end, draws among every creature from the first to the last of that
/// worth, each taken unless a used single, until enough; an all-single
/// worth once each was drawn, then the next dearer.
fn k1_top_up(
    creatures: &[Creature],
    worth: &[f32],
    budget: &Budget,
    used: &mut [bool],
    out: &mut Vec<usize>,
    rng: &mut SimRng,
) {
    let mut need = budget.least - out.len() as i32;
    let mut picked = vec![false; creatures.len()];
    let mut last = 0.0f32;
    while need > 0 {
        let prev = last;
        let found = (0..creatures.len()).rev().find(|&j| {
            last = worth[j];
            last > prev && !(creatures[j].single && used[j])
        });
        if found.is_none() {
            return;
        }
        let of_worth: Vec<usize> = (0..creatures.len()).filter(|&j| worth[j] == last).collect();
        let (Some(&first), Some(&end)) = (of_worth.first(), of_worth.last()) else {
            return;
        };
        let singles = of_worth.iter().filter(|&&j| creatures[j].single).count();
        for &j in &of_worth {
            picked[j] = false;
        }
        let run = end - first + 1;
        let mut drawn = 0;
        for _ in 0..TOP_UP_DRAWS {
            let k = first + rng.below(run as u32) as usize;
            if !(creatures[k].single && used[k]) {
                out.push(k);
                used[k] = true;
                need -= 1;
            }
            if !picked[k] {
                picked[k] = true;
                drawn += 1;
            }
            if need <= 0 || (run == singles && drawn == singles) {
                break;
            }
        }
    }
}

/// Encounter `id` of the view: its facts and its UTE's.
pub(crate) fn of<'a>(view: &'a View, id: ObjectId) -> Option<(&'a ObjectData, &'a Encounter)> {
    let facts = view.facts(id)?;
    Some((facts, facts.encounter.as_deref()?))
}

/// Encounter `id`'s state, made from its UTE the first time it is asked.
pub(crate) fn spawn_of<'a>(state: &'a mut State, id: ObjectId, facts: &Encounter) -> &'a mut Spawn {
    state
        .encounters
        .entry(id.0)
        .or_insert_with(|| Spawn::new(facts))
}

/// Whether a character of `faction` is hostile to an encounter of
/// `encounter_faction`: the encounter's standing toward it under 11 (the
/// executables' `GetAIStateReputation` of 2), as scripts left the factions.
fn hostile(view: &View, state: &State, encounter_faction: u8, faction: u8) -> bool {
    view.module
        .repute
        .with(&state.reputation)
        .hostile(encounter_faction, faction)
}

/// Whether `at` (metres) is in `facts`' outline seen from above (the
/// executables' `GetInActivateArea`: z is not read).
pub(crate) fn inside(facts: &ObjectData, at: [f32; 3]) -> bool {
    inside_outline(
        &facts.geometry,
        [at[0] - facts.position[0], at[1] - facts.position[1]],
    )
}

/// The executables' `UpdateActivateAreaList`'s verdict: a living
/// creature hostile to the encounter stands in it (`enterer` counts as in
/// once within the outline's span across x, as the executables take it
/// unchecked), and it is a player or party member where `PlayerOnly` asks.
fn qualified(
    view: &View,
    state: &State,
    facts: &ObjectData,
    encounter: &Encounter,
    enterer: Option<ObjectId>,
) -> bool {
    let (low, high) = facts
        .geometry
        .iter()
        .fold((f32::MAX, f32::MIN), |(low, high), p| {
            (
                low.min(p[0] + facts.position[0]),
                high.max(p[0] + facts.position[0]),
            )
        });
    view.things.iter().any(|thing| {
        let Some(c) = thing.character else {
            return false;
        };
        let alive = thing.hp.is_some_and(|(hp, _)| hp > 0);
        let x = thing.position[0];
        let within =
            x >= low && x <= high && (Some(thing.id) == enterer || inside(facts, thing.position));
        alive
            && within
            && hostile(view, state, facts.faction, c.faction)
            && (!encounter.player_only || c.party)
    })
}

/// K1's area points (`ComputeSpawnPool`'s): the points of the total level
/// of each living creature hostile to the encounter within `REACH_M` across x
/// and y of whoever last entered it, a party member's whole, another's
/// halved; none without one who entered.
pub fn area_points(
    view: &View,
    state: &State,
    facts: &ObjectData,
    last_entered: Option<u32>,
) -> f32 {
    const REACH_M: f32 = rules::encounters::starting::REACH_M;
    let Some(origin) = last_entered.and_then(|id| view.thing(ObjectId(id))) else {
        return 0.0;
    };
    let [x0, y0, _] = origin.position;
    let fractional = &view.module.encounter_tables.fractional;
    view.things
        .iter()
        .filter_map(|thing| {
            let c = thing.character?;
            let [x, y, _] = thing.position;
            let near =
                x >= x0 - REACH_M && x <= x0 + REACH_M && y >= y0 - REACH_M && y <= y0 + REACH_M;
            let alive = thing.hp.is_some_and(|(hp, _)| hp > 0);
            (near && alive && hostile(view, state, facts.faction, c.faction)).then(|| {
                let level = if c.party { c.level } else { c.level >> 1 };
                points(f32::from(level), fractional)
            })
        })
        .fold(0.0, |sum, p| sum + p)
}

/// The executables' `SpawnIfAppropriate` for encounter `id`, `enterer`
/// coming in (none for a script's activation or a reset): an active
/// encounter not yet started that a qualifying creature stands in builds
/// its spawn list and starts. K1 first adds the budget of those near the
/// last to enter to its pool, and builds only with a pool.
pub(crate) fn spawn_if_appropriate(
    view: &View,
    state: &mut State,
    rng: &mut SimRng,
    id: ObjectId,
    enterer: Option<ObjectId>,
) {
    let Some((facts, encounter)) = of(view, id) else {
        return;
    };
    let spawn = spawn_of(state, id, encounter);
    if !spawn.active || spawn.started {
        return;
    }
    if !qualified(view, state, facts, encounter, enterer) {
        return;
    }
    let tables = &view.module.encounter_tables;
    let tsl = tsl(view.module);
    let last_entered = state
        .encounters
        .get(&id.0)
        .and_then(|spawn| spawn.last_entered);
    let near = if tsl {
        0.0
    } else {
        area_points(view, state, facts, last_entered)
    };
    let spawn = spawn_of(state, id, encounter);
    let picks = if tsl {
        tsl_list(encounter, rng)
    } else {
        if near != 0.0 {
            let budget = party_cr(near) + spawn.difficulty - 1;
            spawn.pool += points(budget as f32, &tables.fractional);
        }
        if spawn.pool <= 0.0 {
            return;
        }
        let picks = k1_list(
            encounter,
            &tables.fractional,
            spawn.pool,
            spawn.number_spawned,
            rng,
        );
        let cheapest = encounter
            .creatures
            .last()
            .map_or(0.0, |c| points(c.cr, &tables.fractional));
        if cheapest > spawn.pool {
            spawn.pool = 0.0;
        }
        picks
    };
    spawn.list = picks
        .into_iter()
        .map(|index| {
            let creature = &encounter.creatures[index];
            (creature.template.clone(), creature.cr)
        })
        .collect();
    if tsl && !encounter.points.is_empty() {
        spawn.cursor.per_point = spawn.list.len() as i32 / encounter.points.len() as i32;
    }
    spawn.started = true;
}

/// `SetEncounterActive`: active or not; made active, it spawns at once if
/// a qualifying creature already stands in it (it is not un-started).
pub(crate) fn set_active(view: &View, state: &mut State, rng: &mut SimRng, id: ObjectId, on: bool) {
    let Some((_, encounter)) = of(view, id) else {
        return;
    };
    spawn_of(state, id, encounter).active = on;
    if on {
        spawn_if_appropriate(view, state, rng, id, None);
    }
}

/// One of encounter `id`'s creatures died or went (`RemoveFromActiveCreatureCount`):
/// one fewer alive; the last of them exhausts it (no longer active nor
/// started; with `Reset`, its reset's clock starts at `now`). Whether it
/// exhausted.
pub(crate) fn lost_one(state: &mut State, id: u32, reset: bool, now: u64) -> bool {
    let Some(spawn) = state.encounters.get_mut(&id) else {
        return false;
    };
    if spawn.number_spawned == 0 {
        return false;
    }
    spawn.number_spawned -= 1;
    if spawn.number_spawned != 0 {
        return false;
    }
    if reset {
        spawn.last_spawn_ms = Some(now);
    }
    spawn.active = false;
    spawn.started = false;
    true
}

/// A body standing in the way of a creature made (`TestSafeLocationPoint`'s
/// creature test): where its soles are (metres) and how wide it is.
#[derive(Debug, Clone, Copy)]
pub struct Body {
    pub soles: [f32; 3],
    pub radius: f32,
}

/// What a creature is placed by (`CSWSArea::ComputeSafeLocation`,
/// `ComputeNonVisibleLocation`): the walkmesh, the bodies standing, and
/// whether sight runs clear between two places.
pub struct Ground<'a> {
    pub nav: Option<&'a NavMesh>,
    pub bodies: Vec<Body>,
    pub sees: &'a dyn Fn([f32; 3], [f32; 3]) -> bool,
}

/// How far around a spawn point the safe place is looked for (metres; the
/// executables'), how far the rings around the encounter's own place go and
/// how far from it a place may be (`encounters.making`).
pub const SAFE_REACH_M: f32 = making::SAFE_REACH_M;
const RING_REACH_M: f32 = making::RING_REACH_M;
const ORIGIN_REACH_M: f32 = making::ORIGIN_REACH_M;
/// The least ring step: a guard on a body of no width (no appearance has
/// a personal space this small).
const LEAST_STEP_M: f32 = 0.1;
/// How high over the floor sight between two places is traced (metres)
/// [verify: the executables' `ClearLineOfSight` from the places as they
/// are].
const SIGHT_RISE_M: f32 = making::SIGHT_RISE_M;

impl Ground<'_> {
    /// Whether a body `radius` wide may stand at `at` (`TestSafeLocationPoint`):
    /// on the walkmesh, its floor there (nearest in height), with walkmesh
    /// a body's width round it at a step's height, and no body on it. The
    /// place on the floor, if so. Without a walkmesh, the place as it is.
    fn safe(&self, at: [f32; 3], radius: f32) -> Option<[f32; 3]> {
        let Some(nav) = self.nav else {
            return Some(at);
        };
        let [x, y, z] = at;
        let floor = nav.height([x, y, z], f32::MAX, f32::MAX)?;
        let step = collision::player::STEP_HEIGHT * collision::UNIT;
        let room = [[radius, 0.0], [-radius, 0.0], [0.0, radius], [0.0, -radius]]
            .iter()
            .all(|[dx, dy]| nav.height([x + dx, y + dy, floor], step, step).is_some());
        let height =
            (collision::player::STAND_HEIGHT - collision::player::SOLE_Z) * collision::UNIT;
        let clear = self.bodies.iter().all(|body| {
            let apart = ((body.soles[0] - x).powi(2) + (body.soles[1] - y).powi(2)).sqrt();
            apart >= radius + body.radius || (body.soles[2] - floor).abs() >= height
        });
        (room && clear).then_some([x, y, floor])
    }

    /// `ComputeSafeLocation(point, SAFE_REACH_M)`: the point itself if safe, else the
    /// first safe place of the rings round it.
    pub fn safe_location(&self, point: [f32; 3], radius: f32) -> Option<[f32; 3]> {
        self.safe(point, radius)
            .or_else(|| ring(point, radius, SAFE_REACH_M, |at| self.safe(at, radius)))
    }

    /// `ComputeNonVisibleLocation(origin)`: the origin if safe, else the
    /// first safe place of the rings round it within `ORIGIN_REACH_M` of it with sight
    /// clear from it. Its test of whom the players see never finds a player
    /// (QUIRKS.md Q-0263), so the first such place wins.
    pub fn out_of_sight(&self, origin: [f32; 3], radius: f32) -> Option<[f32; 3]> {
        let lift = |p: [f32; 3]| [p[0], p[1], p[2] + SIGHT_RISE_M];
        self.safe(origin, radius).or_else(|| {
            ring(origin, radius, RING_REACH_M, |at| {
                let place = self.safe(at, radius)?;
                let d = crate::math::length(crate::math::sub(place, origin));
                (d <= ORIGIN_REACH_M && (self.sees)(lift(origin), lift(place))).then_some(place)
            })
        })
    }
}

/// The executables' square rings round `center` (both placements): half
/// sizes 1, 1 + step, … while under `reach`; each ring's top and bottom
/// edges in x steps of `step`, then its sides in y steps, in single
/// precision as the executables add them; the first place `test` takes.
fn ring<T>(
    center: [f32; 3],
    step: f32,
    reach: f32,
    mut test: impl FnMut([f32; 3]) -> Option<T>,
) -> Option<T> {
    let step = step.max(LEAST_STEP_M);
    let [cx, cy, cz] = center;
    let mut r = making::FIRST_RING_M;
    while reach > r {
        let (x_lo, x_hi) = (cx - r, cx + r);
        let (y_lo, y_hi) = (cy - r, cy + r);
        let across = y_hi - y_lo;
        let mut x = x_lo;
        while x_hi >= x {
            let mut y = y_lo;
            while y_hi >= y {
                if let Some(found) = test([x, y, cz]) {
                    return Some(found);
                }
                y += across;
            }
            x += step;
        }
        let (side_lo, side_hi, along) = (y_lo + step, y_hi - step, x_hi - x_lo);
        let mut x = x_lo;
        while x_hi >= x {
            let mut y = side_lo;
            while side_hi >= y {
                if let Some(found) = test([x, y, cz]) {
                    return Some(found);
                }
                y += step;
            }
            x += along;
        }
        r += step;
    }
    None
}
