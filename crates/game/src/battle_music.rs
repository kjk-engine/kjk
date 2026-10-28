//! When the area's battle music plays (`docs/SOUND.md`, "What plays
//! when"), KOTOR's rule as its executable keeps it, one for both games and
//! every seat. The area has one battle flag:
//!
//! - **On** when a script plays it (`MusicBattlePlay`), or when a seat's
//!   body is **stirred** while a living creature hostile to it stands within
//!   `NEAR_M` (across the floor). A body is stirred for `STIRRED_MS`
//!   (`excitedduration.2da`'s `Damage` and `SpellCast` rows, alike in both
//!   games) when an attack or a power lands on it from another
//!   creature, when it lands one on another (a foe, a friend, a
//!   bystander), or when one of its party within `ALERT_M` (`ranges.2da`
//!   `PercepRngDefault`) is struck by, or strikes, one hostile to it. The
//!   blows are the fighters' events (`sim::combat::Event`), scripts' visual
//!   effects aside.
//! - **Off** when a script stops it (`MusicBattleStop`), or when the last
//!   stirred seat's time runs out: the executable's lapse stops a script's
//!   battle music too.
//!
//! So a shot or a swing at the air, a duel with a friend, a container
//! bashed (no fighter's event), or a fight in a cutscene that never reaches
//! a seat's body or its party plays none; a fight with a foe does, until `STIRRED_MS` after its
//! last blow. The executable stops the music when any player's body stops being
//! stirred; with more than one seat here it waits for the last.

use std::collections::BTreeMap;

use sim::combat::Relation;
use sim::combat::fighter::{Life, Outcome};
use sim::module::{Repute, Side, UNADJUSTED, relation};
use sim::{EntityState, Instances, Seat, Snapshot, sound};

/// How long a blow stirs a body (ms): `excitedduration.2da`'s `Damage` and
/// `SpellCast`.
const STIRRED_MS: u64 = rules::world::battle_music::STIRRED_MS;
/// How near a foe must stand for a stirred body to start the battle music
/// (metres, across the floor).
const NEAR_M: f32 = rules::world::battle_music::NEAR_M;
/// How far a blow on one of the party stirs the rest (metres):
/// `ranges.2da`'s `PercepRngDefault`.
const ALERT_M: f32 = rules::world::battle_music::ALERT_M;

/// A character as the rule sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Body {
    pub id: u32,
    /// Metres.
    pub at: [f32; 3],
    /// A seat's body: a player's (the executable's player creature).
    pub seat: bool,
    /// In the party: the seats' factions' other members.
    pub party: bool,
    pub alive: bool,
    /// The faction it fights for (a script's, else its sheet's), if known.
    pub faction: Option<u8>,
}

/// A blow that landed: an attack or a power `by` one on another, at sim
/// step `tick`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Blow {
    pub on: u32,
    pub by: u32,
    pub tick: u64,
}

/// What a step of the view shows the rule.
pub struct Moment<'a> {
    /// The sim's step and time (ms) the view presents.
    pub tick: u64,
    pub now_ms: u64,
    pub bodies: &'a [Body],
    pub blows: &'a [Blow],
    /// The scripts' say (`sim::sound::AS_PLACED`, `PLAYING`, `STOPPED`).
    pub scripted: u8,
}

/// The area's battle flag and what it is kept by.
#[derive(Debug, Clone, Default)]
pub struct Fight {
    pub on: bool,
    /// Each stirred seat body, and until when (ms).
    pub stirred: BTreeMap<u32, u64>,
    /// The newest step whose blows were read.
    seen: Option<u64>,
    scripted: u8,
}

impl Fight {
    /// The flag after `moment`; `hostile(a, b)`: whether body `a` stands
    /// hostile to body `b`.
    pub fn step(&mut self, moment: &Moment, hostile: impl Fn(&Body, &Body) -> bool) -> bool {
        if moment.scripted != self.scripted {
            self.scripted = moment.scripted;
            match moment.scripted {
                sound::PLAYING => self.on = true,
                sound::STOPPED => self.on = false,
                _ => {}
            }
        }
        let seen = self.seen.unwrap_or(moment.tick.saturating_sub(1));
        self.seen = Some(seen.max(moment.tick));
        let body = |id: u32| moment.bodies.iter().find(|body| body.id == id);
        let new = moment.blows.iter().filter(|blow| blow.tick > seen);
        let mut stir: Vec<&Body> = Vec::new();
        for blow in new {
            let (Some(on), Some(by)) = (body(blow.on), body(blow.by)) else {
                continue;
            };
            for me in moment.bodies.iter().filter(|body| body.seat) {
                if stirs(me, (on, by), &hostile) && !stir.contains(&me) {
                    stir.push(me);
                }
            }
        }
        for me in stir {
            self.stirred.insert(me.id, moment.now_ms + STIRRED_MS);
            let foe_near = moment.bodies.iter().any(|other| {
                other.alive && other.id != me.id && hostile(me, other) && floor(me, other) <= NEAR_M
            });
            if foe_near {
                self.on = true;
            }
        }
        let before = self.stirred.len();
        self.stirred.retain(|_, until| *until > moment.now_ms);
        if before > 0 && self.stirred.is_empty() {
            self.on = false;
        }
        self.on
    }
}

/// Whether blow `(on, by)` stirs seat body `me`: on it, by it, or on or by
/// one of its party near it in a fight with one hostile to `me`.
fn stirs(me: &Body, (on, by): (&Body, &Body), hostile: &impl Fn(&Body, &Body) -> bool) -> bool {
    if on.id == by.id {
        return false;
    }
    if on.id == me.id || by.id == me.id {
        return true;
    }
    [(on, by), (by, on)].into_iter().any(|(mate, other)| {
        me.party && mate.party && distance(me, mate) <= ALERT_M && hostile(me, other)
    })
}

fn distance(a: &Body, b: &Body) -> f32 {
    sim::math::length(sim::math::sub(a.at, b.at))
}

/// Across the floor: how the executable measures a foe's nearness.
fn floor(a: &Body, b: &Body) -> f32 {
    (a.at[0] - b.at[0]).hypot(a.at[1] - b.at[1])
}

/// The rule's view of `snapshot`: its characters, the blows its fighters
/// show (their events but scripts' visual effects), as `instances` (the
/// level's) has their sheets.
pub fn bodies_and_blows(snapshot: &Snapshot, instances: &Instances) -> (Vec<Body>, Vec<Blow>) {
    let seats: Vec<u32> = (0..snapshot.seats())
        .filter_map(|seat| snapshot.body(Seat(seat)).map(|body| body.id.0))
        .collect();
    let fighters = snapshot
        .entities
        .iter()
        .filter(|entity| entity.fighter.is_some());
    let bodies = fighters
        .clone()
        .map(|entity| Body {
            id: entity.id.0,
            at: entity.position.map(|v| v * collision::UNIT),
            seat: seats.contains(&entity.id.0),
            party: snapshot.in_party(entity),
            alive: entity.fighter.is_some_and(|f| f.life == Life::Alive),
            faction: faction(entity, instances),
        })
        .collect();
    let blows = fighters
        .flat_map(|entity| {
            let fighter = entity.fighter.as_ref().expect("filtered");
            fighter.events.iter().flatten().filter_map(|event| {
                let by = event.by?;
                (event.result != Outcome::Visual && by != entity.id).then_some(Blow {
                    on: entity.id.0,
                    by: by.0,
                    tick: event.tick,
                })
            })
        })
        .collect();
    (bodies, blows)
}

/// Whether `a` stands hostile to `b` by `repute` (`repute.2da`; none: no
/// one is), as `sim::module::relation` has it.
pub fn hostility(repute: Option<&Repute>) -> impl Fn(&Body, &Body) -> bool + '_ {
    move |a, b| {
        let side = |body: &Body, faction| Side {
            id: body.id,
            faction,
            party: body.party,
        };
        let standing = repute.map(|repute| repute.with(&UNADJUSTED));
        match (a.faction, b.faction) {
            (Some(fa), Some(fb)) => {
                relation(standing, side(a, fa), side(b, fb)) == Relation::Hostile
            }
            _ => false,
        }
    }
}

/// The faction `entity` fights for, as `instances` has its sheet: a
/// script's, else its sheet's.
fn faction(entity: &EntityState, instances: &Instances) -> Option<u8> {
    let fighter = entity.fighter.as_ref()?;
    let sheet = entity
        .sheet
        .or_else(|| instances.of_entity(entity).and_then(|facts| facts.sheet))?;
    Some(fighter.faction.unwrap_or(sheet.faction))
}

#[cfg(feature = "probes")]
pub fn probes() -> sim::probe::Checks {
    probes::checks()
}

#[cfg(feature = "probes")]
mod probes;
