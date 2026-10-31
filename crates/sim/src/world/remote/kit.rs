//! The droids' kit (the spec's three buttons), on the authority after the
//! droids fly: attack fires the shock bolt, alternate attack the Sonic
//! Blast, the medpac key the stim dart. Every effect on a creature goes
//! through the gate (`gate`), and a hit is credited to the party's leader
//! (`super::credit`). No dice: fixed numbers (`rules::remote`), so a droid
//! never shifts what the story rolls next.

use bevy_ecs::prelude::*;
use collision::UNIT;

use super::super::{
    Avatar, Clock, Fighting, Heals, Hit, Hits, Id, Inputs, Object, Player, PlayerSheet, Static,
    falls, in_party, sheet_of,
};
use super::{Remotes, gate};
use crate::combat::damage::Source;
use crate::input::{Command, buttons};
use crate::math::{self, Vec3, scale, short_to_degrees};
use crate::remote::{KIT_BOLT, KIT_DART, KIT_SONIC};
use crate::snapshot::SimId;

pub(crate) const BOLT_DAMAGE: u16 = rules::remote::bolt::DAMAGE;
const BOLT_CADENCE_MS: u64 = rules::remote::bolt::CADENCE_MS;
const BOLT_RANGE_M: f32 = rules::remote::bolt::RANGE_M;
const SONIC_REACH_M: f32 = rules::remote::sonic::REACH_M;
const SONIC_CONE_DEG: f32 = rules::remote::sonic::CONE_DEG;
const SONIC_STRENGTH: f32 = rules::remote::sonic::STRENGTH;
const SONIC_KNOCKDOWN_MS: u64 = rules::remote::sonic::KNOCKDOWN_MS;
const SONIC_KNOCKDOWN_SIZE: u8 = rules::remote::sonic::KNOCKDOWN_SIZE;
const SONIC_COOLDOWN_MS: u64 = rules::remote::sonic::COOLDOWN_MS;
const DART_RANGE_M: f32 = rules::remote::dart::RANGE_M;
const DART_REVIVE_COOLDOWN_MS: u64 = rules::remote::dart::REVIVE_COOLDOWN_MS;
const DART_HEAL_COOLDOWN_MS: u64 = rules::remote::dart::HEAL_COOLDOWN_MS;
pub(crate) const HEAL_PERMILLE: i32 = rules::remote::dart::HEAL_PERMILLE;
pub(crate) const REVIVE_PERMILLE: i32 = rules::remote::dart::REVIVE_PERMILLE;

/// What a shock bolt strikes with: its fixed points, untyped, the sim's
/// own (not a story's staged blow).
pub(crate) fn bolt_source() -> Source {
    Source::Script {
        damage: BOLT_DAMAGE,
        damage_type: None,
        cast: false,
        shot: None,
        staged: false,
    }
}

/// A character as the kit sees it.
struct Body {
    id: SimId,
    motion: crate::motion::MotionState,
    alive: bool,
    downed: bool,
    party: bool,
    /// Hundredths of an HP.
    hp: i32,
    max_hp: i32,
    /// `appearance.2da` sizecategory (3, medium, where unknown).
    size: u8,
    /// Where its model is drawn, if JA does not pose it (`world::body`).
    bulk: Option<crate::module::Bulk>,
}

impl Body {
    /// Its box (m).
    fn bounds(&self) -> (Vec3, Vec3) {
        let body = super::super::body(self.id, &self.motion, self.bulk.as_ref());
        (body.min, body.max)
    }

    fn centre(&self) -> Vec3 {
        let (min, max) = self.bounds();
        std::array::from_fn(|axis| (min[axis] + max[axis]) * 0.5)
    }

    fn needs(&self) -> bool {
        self.downed || (self.alive && self.hp < self.max_hp)
    }
}

/// What the step's kit does, decided before any of it is done.
#[derive(Default)]
struct Deeds {
    hits: Vec<Hit>,
    heals: Vec<(SimId, i32)>,
    /// Thrown from (units), knocked down or not.
    thrown: Vec<(SimId, Vec3, bool)>,
    /// Up again with hundredths of an HP.
    revived: Vec<(SimId, i32)>,
}

/// See the module documentation.
pub(crate) fn kit(world: &mut World) {
    if world.resource::<Remotes>().0.is_empty() {
        return;
    }
    let now = world.resource::<Clock>().time_ms;
    let lost = world.resource::<falls::Lost>().0.is_some();
    let level = world.resource::<Static>().0.clone();
    let states = {
        let mut objects = world.query::<&Object>();
        level.states(
            objects
                .iter(world)
                .map(|object| (object.index, &object.state)),
        )
    };
    let bodies = bodies(world);
    let mut remotes = world.resource::<Remotes>().0.clone();
    let inputs = world.resource::<Inputs>().0.clone();
    let deeds = {
        let gate = gate::of(world);
        let credit = gate.leader();
        let whole_party_down = !bodies.iter().any(|body| body.party && body.alive);
        let mut deeds = Deeds::default();
        let mut darted: Vec<SimId> = Vec::new();
        let scene = Scene {
            level: &level,
            states: states.as_deref(),
            bodies: &bodies,
        };
        for remote in remotes.iter_mut().filter(|remote| !remote.hidden) {
            let Some(command) = inputs.get(remote.seat) else {
                continue;
            };
            let aim = aim(command);
            let ready = remote.ready_ms;
            let pressed =
                |button: u16, kit: usize| command.buttons & button != 0 && now >= ready[kit];
            if pressed(buttons::ATTACK, KIT_BOLT) {
                remote.ready_ms[KIT_BOLT] = now + BOLT_CADENCE_MS;
                let met = scene.first(remote.at, aim, BOLT_RANGE_M, |_| true);
                if let (Some((at, point)), Some(credit)) = (met, credit)
                    && gate.fair(bodies[at].id) == gate::Fair::Yes
                {
                    deeds.hits.push(Hit {
                        attacker: credit,
                        target: bodies[at].id,
                        source: bolt_source(),
                        point: Some(scale(point, 1.0 / UNIT)),
                        direction: Some(aim),
                    });
                }
            }
            if pressed(buttons::ALT_ATTACK, KIT_SONIC) {
                remote.ready_ms[KIT_SONIC] = now + SONIC_COOLDOWN_MS;
                for at in scene.cone(remote.at, aim) {
                    let body = &bodies[at];
                    if gate.fair(body.id) == gate::Fair::Yes {
                        let from = scale(remote.at, 1.0 / UNIT);
                        deeds
                            .thrown
                            .push((body.id, from, body.size <= SONIC_KNOCKDOWN_SIZE));
                    }
                }
            }
            if pressed(buttons::MEDPAC, KIT_DART) && !lost && !whole_party_down {
                let Some(at) = scene.dart(remote.at, aim) else {
                    continue;
                };
                let body = &bodies[at];
                if darted.contains(&body.id) || gate.dueling(body.id) {
                    continue;
                }
                darted.push(body.id);
                if body.downed {
                    deeds
                        .revived
                        .push((body.id, body.max_hp * REVIVE_PERMILLE / 1000));
                    remote.ready_ms[KIT_DART] = now + DART_REVIVE_COOLDOWN_MS;
                } else {
                    deeds
                        .heals
                        .push((body.id, body.max_hp * HEAL_PERMILLE / 1000));
                    remote.ready_ms[KIT_DART] = now + DART_HEAL_COOLDOWN_MS;
                }
            }
        }
        deeds
    };
    world.resource_mut::<Remotes>().0 = remotes;
    apply(world, deeds, now);
}

/// Where a seat's view points: its full direction.
fn aim(command: &Command) -> Vec3 {
    let view = [
        short_to_degrees(command.angles[0]),
        short_to_degrees(command.angles[1]),
        0.0,
    ];
    math::angle_vectors(view).0
}

/// Every character, in id order.
#[allow(clippy::type_complexity)]
fn bodies(world: &mut World) -> Vec<Body> {
    let mut query = world.query::<(
        &Id,
        Option<&Avatar>,
        Option<&Object>,
        Option<&PlayerSheet>,
        &Player,
        &Fighting,
    )>();
    let mut bodies: Vec<Body> = query
        .iter(world)
        .map(|(id, avatar, object, sheet, player, fighting)| {
            let fighter = &fighting.0;
            Body {
                id: id.0,
                motion: player.0,
                alive: fighter.alive(),
                downed: fighter.downed(),
                party: in_party(avatar, fighter),
                hp: fighter.hp,
                max_hp: i32::from(sheet_of(sheet, object).max_hp) * 100,
                size: object
                    .and_then(|object| object.facts.as_ref())
                    .and_then(|facts| facts.body)
                    .map_or(3, |body| body.size),
                bulk: super::super::object_bulk(object).copied(),
            }
        })
        .collect();
    bodies.sort_by_key(|body| body.id);
    bodies
}

/// What a droid's kit looks through.
struct Scene<'a> {
    level: &'a crate::Level,
    states: Option<&'a [u8]>,
    bodies: &'a [Body],
}

impl Scene<'_> {
    /// Whether nothing solid stands between `from` and `to` (m), nor
    /// around `from`.
    fn clear(&self, from: Vec3, to: Vec3) -> bool {
        let line = self.level.line(self.states, from, to);
        !line.start_solid && line.fraction >= 1.0
    }

    /// How far (m) a line from `from` along `aim` goes within `range`
    /// before the level stops it.
    fn open(&self, from: Vec3, aim: Vec3, range: f32) -> f32 {
        let to = math::mad(from, range, aim);
        let line = self.level.line(self.states, from, to);
        if line.start_solid {
            0.0
        } else {
            line.fraction * range
        }
    }

    /// The first body of those `taking` that a line from `from` (m) along
    /// `aim` meets within `range` before the level stops it: its index and
    /// where (m). The living only, and the downed: a corpse lets it by.
    fn first(
        &self,
        from: Vec3,
        aim: Vec3,
        range: f32,
        taking: impl Fn(&Body) -> bool,
    ) -> Option<(usize, Vec3)> {
        let open = self.open(from, aim, range);
        (self.bodies.iter().enumerate())
            .filter(|(_, body)| (body.alive || body.downed) && taking(body))
            .filter_map(|(at, body)| {
                let (min, max) = body.bounds();
                entry(from, aim, min, max)
                    .filter(|far| *far <= open)
                    .map(|far| (far, at))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(far, at)| (at, math::mad(from, far, aim)))
    }

    /// The living a Sonic Blast from `from` (m) along `aim` reaches: within
    /// `SONIC_REACH_M` (to the nearest point of its box), within half the
    /// cone of the aim and in clear sight.
    fn cone(&self, from: Vec3, aim: Vec3) -> Vec<usize> {
        let half = crate::math::sin_cos_deg(SONIC_CONE_DEG * 0.5).1;
        (0..self.bodies.len())
            .filter(|&at| self.bodies[at].alive)
            .filter(|&at| {
                let body = &self.bodies[at];
                let centre = body.centre();
                let mut toward = math::sub(centre, from);
                math::normalize(&mut toward);
                let (min, max) = body.bounds();
                let nearest: Vec3 = std::array::from_fn(|i| from[i].clamp(min[i], max[i]));
                math::dot(toward, aim) >= half
                    && math::length(math::sub(nearest, from)) <= SONIC_REACH_M
                    && self.clear(from, centre)
            })
            .collect()
    }

    /// A stim dart's party member: the one under the crosshair if it needs
    /// one, else the nearest downed, else the nearest hurt, in range and
    /// in clear sight.
    fn dart(&self, from: Vec3, aim: Vec3) -> Option<usize> {
        let aimed = self
            .first(from, aim, DART_RANGE_M, |_| true)
            .map(|(at, _)| at)
            .filter(|&at| self.bodies[at].party && self.bodies[at].needs());
        aimed.or_else(|| {
            let away = |body: &Body| math::length(math::sub(body.centre(), from));
            (self.bodies.iter().enumerate())
                .filter(|(_, body)| body.party && body.needs())
                .filter(|(_, body)| away(body) <= DART_RANGE_M)
                .filter(|(_, body)| self.clear(from, body.centre()))
                .min_by(|(_, a), (_, b)| {
                    (!a.downed, away(a))
                        .partial_cmp(&(!b.downed, away(b)))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(at, _)| at)
        })
    }
}

/// How far along a line from `from` along unit `aim` it enters the box
/// `min`..`max` (0 when it starts inside), if it does going forward.
fn entry(from: Vec3, aim: Vec3, min: Vec3, max: Vec3) -> Option<f32> {
    let (mut near, mut far) = (0.0f32, f32::INFINITY);
    for i in 0..3 {
        if aim[i] == 0.0 {
            if from[i] < min[i] || from[i] > max[i] {
                return None;
            }
            continue;
        }
        let (a, b) = ((min[i] - from[i]) / aim[i], (max[i] - from[i]) / aim[i]);
        near = near.max(a.min(b));
        far = far.min(a.max(b));
    }
    (near <= far).then_some(near)
}

/// Does the step's deeds: the hits and heals for `hurt`, the throws and
/// revives on the bodies now.
fn apply(world: &mut World, deeds: Deeds, now: u64) {
    world.resource_mut::<Hits>().0.extend(deeds.hits);
    world.resource_mut::<Heals>().0.extend(deeds.heals);
    if deeds.thrown.is_empty() && deeds.revived.is_empty() {
        return;
    }
    let anims = world.resource::<Static>().0.anims.clone();
    let strength = crate::combat::force::ja::PUSH_PULL_STRENGTH * SONIC_STRENGTH;
    let mut query = world.query::<(&Id, &mut Player, &mut Fighting)>();
    for (id, mut player, mut fighting) in query.iter_mut(world) {
        for (_, from, knocks) in deeds.thrown.iter().filter(|(of, ..)| *of == id.0) {
            let knockdown = knocks.then_some(SONIC_KNOCKDOWN_MS);
            super::super::thrown(
                &mut player.0,
                &mut fighting.0,
                *from,
                now,
                strength,
                knockdown,
            );
        }
        for (_, hp) in deeds.revived.iter().filter(|(of, _)| *of == id.0) {
            falls::get_up(&mut player.0, &mut fighting.0, &anims, *hp, now);
        }
    }
}
