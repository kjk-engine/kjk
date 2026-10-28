//! Shots as a view sees them, and a joined player's own shots at once
//! (I22 C20; QUIRKS.md Q-0070).
//!
//! - A shot: a character with a blaster out whose torso starts its attack
//!   (`BOTH_ATTACK2` for the Bryar, `BOTH_ATTACK3` for the rest), or starts
//!   it over, from one snapshot to the next. Its flash leaves the muzzle
//!   (`sim::muzzle`, `CalcMuzzlePoint`) along the view; a pistol in each
//!   hand, one shot for each hand the shot left (`Shooting::hands`), each
//!   from its own (`sim::hand_muzzle`).
//! - A joined player's own shots are found in its predictions, at its own
//!   time, ahead of the host's snapshots: the flash plays then, and each
//!   bolt it fired flies from the muzzle at once (`OwnBolt`), as the client
//!   draws its own character. The host fires the same bolt advanced by the
//!   player's view lag (C18), so the host's copy, drawn at the remote view's
//!   time, stands where this one does at own time. When the host's copy
//!   shows, it takes over (`Handover`): any distance left between them (a
//!   view lag past C18's bound, a clock's drift) is closed over
//!   `HANDOVER_HALF_LIFE_MS`. A bolt the host's snapshots never have
//!   (the host saw no such shot) stops where it is and vanishes; one
//!   that met a body in the host's first step (its view-lag advance, or
//!   held there for a remote player's grace) flies on until it meets that
//!   body as drawn. Every own bolt stops at the first body or wall it meets
//!   as drawn; a body's impact plays then, and the authority's hit, when it
//!   comes, plays none again.

use bevy::prelude::*;
use collision::{Bodies, Body, CollisionWorld as _, Extents, UNIT};
use sim::combat::sheet::Weapon;
use sim::combat::specials::Special;
use sim::combat::{Delivery, Missile, weapons};
use sim::motion::Anim;
use sim::{Instances, SimId, Snapshot};

use crate::cast;

/// How fast the distance between an own bolt and the host's copy closes
/// (ms of the view's time): the half-life the client smooths its own
/// character's corrections with.
pub const HANDOVER_HALF_LIFE_MS: f32 = 50.0;
/// How far the host's copy of an own bolt may start from the predicted one
/// (units): a few ticks' walk.
const SAME_BASE: f32 = 12.0;
/// How many ticks the host's snapshots may lack an own bolt before it
/// counts as not fired (the host may fire it a tick or two late, its
/// command late or lost).
const LATE_TICKS: u64 = 4;
/// How long an own bolt whose hit came early flies on to the body as drawn
/// before it stops anyway (ms).
const DOOMED_MS: f64 = 400.0;

/// A shot: who fired which blaster (its base item, the feat special the
/// shot carried) from which hand's gun (`cast::Guns`), when (sim ms), from
/// where (units) and which way.
#[derive(Debug, Clone, Copy)]
pub struct Shot {
    pub by: SimId,
    pub hand: u8,
    pub base_item: u16,
    pub special: Option<Special>,
    pub ms: f64,
    pub muzzle: Vec3,
    pub forward: Vec3,
}

/// The shots `now` shows that `before` did not; `skip` leaves out a
/// character (the own one, found in the predictions instead).
pub fn shots(
    before: &Snapshot,
    now: &Snapshot,
    level: &sim::Level,
    skip: Option<SimId>,
) -> Vec<Shot> {
    let objects = Instances::in_snapshot(level, now);
    now.entities
        .iter()
        .filter(|entity| Some(entity.id) != skip)
        .flat_map(|entity| {
            let ps = entity.player?;
            let sheet = cast::sheet(entity, &objects);
            let weapon = cast::weapon(entity, &objects);
            let Delivery::Blaster(blaster) = weapon.delivery else {
                return None;
            };
            let torso = ps.torso;
            if !matches!(torso.anim, Anim::BOTH_ATTACK2 | Anim::BOTH_ATTACK3) {
                return None;
            }
            let earlier = cast::entity(before, entity.id)?;
            let was = earlier.player?.torso;
            if was.anim == torso.anim && was.flip == torso.flip {
                return None;
            }
            let shooting =
                |e: &sim::EntityState| e.fighter.map_or_else(Default::default, |f| f.shooting);
            let (then, shooting_now) = (shooting(earlier), shooting(entity));
            // The special its trigger was under: a charge let go, a burst
            // going, an aim held.
            let special = Special::of(&sheet, blaster).filter(|special| match special {
                Special::PowerBlast(_) => then.charging_from.is_some(),
                Special::RapidShot(_) => then.burst > 0 || shooting_now.burst > 0,
                Special::SniperShot(_) => shooting_now.aiming,
            });
            // A pair's guns each their own; one gun's shots leave the gun out.
            let pair = sheet.pistol_pair();
            let hands: Vec<u8> = if pair {
                [0, 1]
                    .into_iter()
                    .filter(|hand| shooting_now.hands & 1 << hand != 0)
                    .collect()
            } else {
                vec![0]
            };
            let shots: Vec<Shot> = hands
                .into_iter()
                .map(|hand| {
                    let held = if pair { sheet.held(hand) } else { weapon };
                    let own = match held.delivery {
                        Delivery::Blaster(own) => own,
                        _ => blaster,
                    };
                    let muzzle = weapons::primary(own).muzzle;
                    Shot {
                        by: entity.id,
                        hand,
                        base_item: held.base_item,
                        special,
                        ms: now.time_ms as f64,
                        muzzle: Vec3::from_array(sim::hand_muzzle(&ps, muzzle, hand)),
                        forward: Vec3::from_array(sim::math::angle_vectors(ps.view).0),
                    }
                })
                .collect();
            Some(shots)
        })
        .flatten()
        .collect()
}

/// The bolts `me` fired in `now`'s tick (a prediction's: not advanced).
pub fn fired_bolts(now: &Snapshot, me: SimId) -> Vec<Missile> {
    now.entities
        .iter()
        .filter_map(|entity| entity.missile)
        .filter(|m| m.owner == me && m.lob.is_none() && m.from_ms == now.time_ms)
        .filter(|m| matches!(m.weapon.delivery, Delivery::Blaster(_)))
        .collect()
}

/// A joined player's own shots and bolts found in its predictions since
/// the last frame (`play_view`), how far ahead of the view's time they are
/// shown, the others' bodies as drawn now (`combat_fx` meets the own bolts
/// with them), and the own character at own time (`force_view`).
#[derive(Resource, Default)]
pub struct OwnFire {
    pub me: Option<SimId>,
    /// Own time minus the view's (ms); `None` while nothing is predicted.
    pub shift: Option<f64>,
    pub shots: Vec<Shot>,
    /// Each with the tick it was fired in.
    pub bolts: Vec<(u64, Missile)>,
    pub bodies: Vec<Body>,
    /// The own character at own time, as its newest prediction by then has
    /// it (its Force effects show from it).
    pub own: Option<sim::EntityState>,
}

/// Where an own bolt is in its life.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Fate {
    /// Flying, the host's snapshots not yet past its tick.
    Flying,
    /// The host's snapshots have it: it flies until its copy shows.
    Confirmed,
    /// The host's first step met a body with it: it flies on to the body
    /// as drawn, until then at the latest (own ms).
    Doomed(f64),
    /// Stopped at this own time.
    Ended(f64),
    /// The host's copy took over.
    Handed(SimId),
}

/// A joined player's own bolt, drawn from its prediction until the host's
/// copy shows.
#[derive(Debug, Clone)]
pub struct OwnBolt {
    pub tick: u64,
    pub weapon: Weapon,
    pub base: Vec3,
    pub velocity: Vec3,
    /// When it left the muzzle (own ms).
    pub fired: f64,
    pub fate: Fate,
    /// Where its head was last drawn (units).
    pub head: Vec3,
}

impl OwnBolt {
    pub fn new(tick: u64, missile: &Missile) -> Option<OwnBolt> {
        if !matches!(missile.weapon.delivery, Delivery::Blaster(_)) {
            return None;
        }
        let base = Vec3::from_array(missile.base);
        Some(OwnBolt {
            tick,
            weapon: missile.weapon,
            base,
            velocity: Vec3::from_array(missile.velocity),
            fired: missile.from_ms as f64,
            fate: Fate::Flying,
            head: base,
        })
    }

    /// Where it is at own time `ms` (units), held where it stopped.
    pub fn at(&self, ms: f64) -> Vec3 {
        let ms = match self.fate {
            Fate::Ended(end) => ms.min(end),
            _ => ms,
        };
        self.base + self.velocity * ((ms - self.fired).max(0.0) / 1000.0) as f32
    }

    /// Whether `missile` (the host's, fired `lead` ms of own time before
    /// the view shows it: its view-lag advance) is this bolt: from about the
    /// same muzzle, the same way, within half a shot's cadence.
    pub fn is(&self, missile: &Missile, lead: f64) -> bool {
        let fired = missile.from_ms as f64 + lead.min(sim::REWIND_MS as f64);
        Vec3::from_array(missile.base).distance(self.base) <= SAME_BASE
            && self.same_way(missile)
            && (fired - self.fired).abs() <= f64::from(missile.cadence_ms) / 2.0
    }

    fn same_way(&self, missile: &Missile) -> bool {
        Vec3::from_array(missile.velocity)
            .normalize_or_zero()
            .dot(self.velocity.normalize_or_zero())
            > 0.9995
    }

    /// Whether `missile` is this bolt held on a body for its grace (its
    /// base moved there): the same way, met about when it was fired.
    fn held_here(&self, missile: &Missile) -> bool {
        missile.held.is_some_and(|held| {
            held.tick + LATE_TICKS >= self.tick && held.tick <= self.tick + LATE_TICKS
        }) && self.same_way(missile)
    }

    /// What the host's `snapshot` says of it, at own time `ms` (`lead` ahead
    /// of the view's): it has the bolt (confirmed), met a body with it on
    /// its tick (doomed: fly on), or, some ticks on, never fired it (it
    /// stops now: true).
    pub fn judge(&mut self, snapshot: &Snapshot, me: SimId, (ms, lead): (f64, f64)) -> bool {
        if !matches!(self.fate, Fate::Flying) || snapshot.tick < self.tick {
            return false;
        }
        let mine = snapshot
            .entities
            .iter()
            .filter_map(|entity| entity.missile)
            .filter(|missile| missile.owner == me);
        let (mut has, mut held) = (false, false);
        for missile in mine {
            has |= self.is(&missile, lead);
            held |= self.held_here(&missile);
        }
        if has {
            self.fate = Fate::Confirmed;
            return false;
        }
        let hit = held
            || snapshot
                .entities
                .iter()
                .filter_map(|entity| entity.fighter)
                .flat_map(|fighter| fighter.events.into_iter().flatten())
                .any(|event| {
                    event.by == Some(me)
                        && matches!(event.weapon, Some(Delivery::Blaster(_)))
                        && event.tick.abs_diff(self.tick) <= LATE_TICKS
                });
        if hit {
            self.fate = Fate::Doomed(ms + DOOMED_MS);
        } else if snapshot.tick >= self.tick + LATE_TICKS {
            self.fate = Fate::Ended(ms);
            return true;
        }
        false
    }

    /// Moves its head to own time `ms`, stopping at the first wall or body
    /// it meets on the way (the level with `doors`, `bodies` as drawn): the
    /// body met, if one was.
    pub fn fly(
        &mut self,
        ms: f64,
        (level, doors): (&sim::Level, Option<&[u8]>),
        bodies: &[Body],
        me: SimId,
    ) -> Option<(SimId, Vec3)> {
        if matches!(self.fate, Fate::Ended(_) | Fate::Handed(_)) {
            return None;
        }
        if let Fate::Doomed(until) = self.fate
            && ms > until
        {
            self.fate = Fate::Ended(until);
            return None;
        }
        let from = self.head;
        let to = self.at(ms);
        if from.distance(to) < 0.01 {
            return None;
        }
        let metres = |v: Vec3| (v * UNIT).to_array();
        let trace = level.with(doors, |world| {
            Bodies::new(world, bodies).skipping(me.0 | sim::BODY).trace(
                metres(from),
                metres(to),
                &Extents::POINT,
                sim::SHOT,
            )
        });
        if trace.fraction >= 1.0 {
            self.head = to;
            return None;
        }
        let travelled = from.distance(to) * trace.fraction;
        let stop = from + (to - from).normalize_or_zero() * travelled;
        let speed = self.velocity.length().max(1.0);
        let end = self.fired + f64::from(stop.distance(self.base) / speed * 1000.0);
        self.head = stop;
        self.fate = Fate::Ended(end);
        trace
            .entity
            .filter(|entity| entity & sim::BODY != 0)
            .map(|entity| (SimId(entity & !sim::BODY), stop))
    }
}

/// The host's copy of an own bolt took over at the view's `since` ms,
/// `offset` units from where the own bolt was: it is drawn that much off,
/// the offset closing.
#[derive(Debug, Clone, Copy)]
pub struct Handover {
    pub offset: Vec3,
    pub since: f32,
}

impl Handover {
    /// The offset left at the view's `now` ms.
    pub fn left(&self, now: f32) -> Vec3 {
        self.offset * 0.5f32.powf((now - self.since).max(0.0) / HANDOVER_HALF_LIFE_MS)
    }
}
