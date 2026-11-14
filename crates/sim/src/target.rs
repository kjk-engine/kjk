//! What the player is looking at (`docs/UI.md`),
//! found render side each frame from the presented snapshot: the object
//! the screen's middle (the crosshair) rests on within `RANGE` of the
//! player, else the one nearest the crosshair within `CONE_DEGREES` and in
//! sight, else the nearest within `containers::REACH` in front of the
//! player (JA's camera sits high behind: what lies at the character's feet
//! is far below the crosshair). A placeable with action points is in sight
//! where its use would be (`seen_at_action_point`). A target holds
//! `HOLD_MS` after nothing picks it, so it does not flicker at an edge.
//!
//! Targetable: doors but static ones (a door leading out is one too),
//! useable placeables (one hung up a wall, as a door's panel, picked where
//! its console is drawn: `hung`), living creatures, a trap the party
//! detected, and a
//! downed party member (a seat's avatar or a companion, I30 P11), which a
//! standing one may revive (`Choice::Revive`, `Reviving`). A living
//! companion is one whoever drives it, a player or its AI, as KOTOR's party
//! members are clicked and talked to; never the looker's own body, and,
//! following close behind, only in front of the player (`Candidate::ahead`).
//! Seats' avatars are not: they have no conversation.

use collision::{Contents, Extents, UNIT};

use crate::instance::Instances;
use crate::math::{self, Vec3};
use crate::module::{ObjectKind, Opens};
use crate::snapshot::{EntityState, SimId};
use crate::world::Level;
use crate::{Kind, Snapshot};

/// How far from the player a target may be (metres; measured on Taris).
pub const RANGE: f32 = rules::targeting::pick::RANGE;
/// How far off the crosshair (degrees) a target may be when it meets
/// nothing.
pub const CONE_DEGREES: f32 = rules::targeting::pick::CONE_DEGREES;
/// How long a target holds after nothing picks it (ms).
pub const HOLD_MS: u64 = rules::targeting::pick::HOLD_MS;

/// How high a standing player's eyes are above its feet (metres), for the
/// near rule's sight.
pub const EYES: f32 = rules::targeting::pick::EYES;
/// How near the point it looks at a line of sight may stop (metres; `seen`).
const SIGHT_SLACK: f32 = rules::targeting::pick::SIGHT_SLACK;
/// A creature's pick volume: a standing cylinder (metres).
const CREATURE_RADIUS: f32 = rules::targeting::volumes::CREATURE_RADIUS;
const CREATURE_HEIGHT: f32 = rules::targeting::volumes::CREATURE_HEIGHT;
/// A placeable's, or a door's when the trace misses its brushes.
pub(crate) const PLACEABLE_RADIUS: f32 = rules::targeting::volumes::PLACEABLE_RADIUS;
pub(crate) const PLACEABLE_HEIGHT: f32 = rules::targeting::volumes::PLACEABLE_HEIGHT;
pub(crate) const DOOR_RADIUS: f32 = rules::targeting::volumes::DOOR_RADIUS;
pub(crate) const DOOR_HEIGHT: f32 = rules::targeting::volumes::DOOR_HEIGHT;
/// A downed body's: lying on the floor.
const BODY_RADIUS: f32 = rules::targeting::volumes::BODY_RADIUS;
const BODY_HEIGHT: f32 = rules::targeting::volumes::BODY_HEIGHT;
/// A trap's: a mine on the floor.
const TRAP_RADIUS: f32 = rules::targeting::volumes::TRAP_RADIUS;
const TRAP_HEIGHT: f32 = rules::targeting::volumes::TRAP_HEIGHT;

/// The numbers seats' avatars go by as targets, past any area's objects:
/// seat n's is `AVATARS + n`.
pub const AVATARS: u32 = u32::MAX - crate::world::MAX_SEATS as u32;

/// An entity's number as a target: an object's among the area's objects,
/// an avatar's `AVATARS` past its seat.
pub fn number(entity: &EntityState) -> Option<u32> {
    match entity.kind {
        Kind::Object(_, index) => Some(index),
        Kind::Avatar(seat) => Some(AVATARS + u32::from(seat.0)),
        _ => None,
    }
}

/// The entity of `snapshot` a target's number names.
pub fn entity(snapshot: &Snapshot, number: u32) -> Option<&EntityState> {
    snapshot
        .entities
        .iter()
        .find(|entity| self::number(entity) == Some(number))
}

/// Whether `entity` is a downed party member: a seat's avatar, or a
/// companion (one with a leader, driven or not).
pub fn revivable(entity: &EntityState) -> bool {
    entity.fighter.is_some_and(|fighter| {
        fighter.downed()
            && (matches!(entity.kind, Kind::Avatar(_)) || fighter.mind.leader.is_some())
    })
}

/// What may be done to a party member's body: revived while it is down.
pub fn body_choices(entity: &EntityState) -> Vec<Choice> {
    if revivable(entity) {
        vec![Choice::Revive]
    } else {
        Vec::new()
    }
}

/// A targetable object: its number among the area's objects, its kind,
/// where its feet are (metres) and its pick volume.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Candidate {
    pub index: u32,
    pub kind: ObjectKind,
    pub feet: Vec3,
    pub radius: f32,
    pub height: f32,
    /// Picked only in front of the player, its feet more than its radius
    /// ahead of the player's along its facing (a living companion, whose body the player's
    /// passes through): one at its heels or inside its body, between it and
    /// the camera, takes no look meant past it.
    pub ahead: bool,
    /// A placeable's action points in the world (metres), at most two
    /// (`ObjectData::action_points`): where a use from beside it looks
    /// (`uses::Target::aimed`), so the crosshair sees it there too.
    pub points: [Option<Vec3>; 2],
}

impl Candidate {
    fn middle(&self) -> Vec3 {
        self.up(0.5)
    }

    /// Where a use by a player with its feet at `user` (metres) looks at
    /// this placeable: `uses::ACTION_SIGHT_M` above its action point
    /// nearest the player, as `uses::Target::aimed` aims; `None` without
    /// action points.
    fn action_sight(&self, user: Vec3) -> Option<Vec3> {
        let far = |p: &Vec3| math::length(math::sub(*p, user));
        let point = self
            .points
            .iter()
            .flatten()
            .min_by(|a, b| far(a).total_cmp(&far(b)))?;
        Some([point[0], point[1], point[2] + crate::uses::ACTION_SIGHT_M])
    }

    /// The point of its axis `share` of the way up its volume.
    pub fn up(&self, share: f32) -> Vec3 {
        [
            self.feet[0],
            self.feet[1],
            self.feet[2] + self.height * share,
        ]
    }
}

/// Where the crosshair looks from (metres) and along (a unit vector).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    pub from: Vec3,
    pub forward: Vec3,
}

/// The player looking: where it stands (feet, metres) and its facing
/// across the floor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Looker {
    pub feet: Vec3,
    pub facing: [f32; 2],
}

/// The targetable objects of `snapshot` (see the module documentation) for
/// the player whose body is `me`.
pub fn candidates(level: &Level, snapshot: &Snapshot, me: Option<SimId>) -> Vec<Candidate> {
    let instances = Instances::in_snapshot(level, snapshot);
    snapshot
        .entities
        .iter()
        .filter(|entity| Some(entity.id) != me)
        .filter_map(|entity| {
            if revivable(entity) {
                return Some(Candidate {
                    index: number(entity)?,
                    kind: ObjectKind::Creature,
                    feet: feet_of(&entity.player?),
                    radius: BODY_RADIUS,
                    height: BODY_HEIGHT,
                    ahead: false,
                    points: [None; 2],
                });
            }
            let Kind::Object(kind, index) = entity.kind else {
                return None;
            };
            let facts = instances.of_entity(entity)?;
            let (radius, height) = match kind {
                // An open door is picked by its brushes or at the feet: a
                // volume of its own would fill its doorway.
                ObjectKind::Door if entity.object.is_some_and(|object| object.state != 0) => {
                    (0.0, DOOR_HEIGHT)
                }
                ObjectKind::Door if facts.opens != Opens::Never || facts.exit().is_some() => {
                    (DOOR_RADIUS, DOOR_HEIGHT)
                }
                ObjectKind::Placeable if facts.useable => (PLACEABLE_RADIUS, PLACEABLE_HEIGHT),
                ObjectKind::Creature => {
                    entity.fighter.as_ref()?.alive().then_some(())?;
                    (CREATURE_RADIUS, CREATURE_HEIGHT)
                }
                ObjectKind::Trigger if seen_trap(facts, entity) => {
                    return Some(Candidate {
                        index,
                        kind,
                        feet: crate::traps::centre(&facts.geometry, entity.position),
                        radius: TRAP_RADIUS,
                        height: TRAP_HEIGHT,
                        ahead: false,
                        points: [None; 2],
                    });
                }
                _ => return None,
            };
            // A character's position is its body's origin, above its soles;
            // an object's is where it stands (its facts' for one placed, but
            // one play made, as remains, stands where it was made: its
            // template's facts are not there).
            let mut feet = entity.player.as_ref().map_or(entity.position, feet_of);
            // A creature JA does not pose is picked where its model is
            // drawn (`world::body`): as far across as it reaches, from its
            // lowest to its highest (a probe droid floats).
            let (radius, mut height) = match crate::world::bulk_of(Some(facts)) {
                Some(bulk) if kind == ObjectKind::Creature => {
                    feet[2] += bulk.min[2];
                    (bulk.reach(), bulk.max[2] - bulk.min[2])
                }
                _ => (radius, height),
            };
            let mut points = [None; 2];
            if kind == ObjectKind::Placeable {
                (feet[2], height) = hung(level, feet, height);
                for (slot, point) in points.iter_mut().zip(facts.action_points(feet)) {
                    *slot = Some(point);
                }
            }
            Some(Candidate {
                index,
                kind,
                feet,
                radius,
                height,
                ahead: kind == ObjectKind::Creature && crate::world::party_member(entity),
                points,
            })
        })
        .collect()
}

/// Where a placeable standing at `at` (metres) with a volume `height` tall
/// is picked from, and how tall: from its own height on the floor; one
/// hung over the floor (Yavin Station's door panels, invisible before
/// their consoles, 1.1 to 1.7 m up) from the floor up past it; one over
/// no floor (in a wall's alcove) about where it hangs. Its height and
/// the volume's as they are with no navigation mesh to tell.
fn hung(level: &Level, at: Vec3, height: f32) -> (f32, f32) {
    let Some(nav) = level.nav() else {
        return (at[2], height);
    };
    match nav.height(at, height, height) {
        Some(floor) if floor < at[2] => (floor, height + at[2] - floor),
        Some(_) => (at[2], height),
        None => (at[2] - height * 0.5, height),
    }
}

/// Whether `entity` is a trap the party detected that still springs.
fn seen_trap(facts: &crate::module::ObjectData, entity: &EntityState) -> bool {
    use crate::snapshot::trap;
    facts.trap.is_some()
        && entity.object.is_some_and(|object| {
            object.trap & trap::PARTY_SEES != 0 && object.trap & trap::DISABLED == 0
        })
}

/// Whether nothing solid stands between `from` and `to` (metres), doors as
/// `states` has them, but object `index` itself.
pub fn in_sight(level: &Level, states: Option<&[u8]>, from: Vec3, to: Vec3, index: u32) -> bool {
    let trace = level.line(states, from, to);
    crate::world::unobstructed(&trace) || trace.entity == Some(index)
}

/// `in_sight`, or the line gets within `SIGHT_SLACK` of `to`: a point on or
/// just inside the object's own body, the bench it stands on or the wall it
/// is fixed to does not hide it, while a wall in front still does. The
/// crosshair picks by it (`seen`), and a use goes by it (`uses::may_use`),
/// so what the crosshair offers to use is used: the Ebon Hawk's lift
/// controls, flush with the wall, were offered and never used (issue 237).
/// Placeables never hide anything from it (`Level::sight_states`): the
/// Ithorian held in the force cage of the Exchange on Telos is talked to
/// through the cage's walls, as in TSL.
pub fn in_sight_near(
    level: &Level,
    states: Option<&[u8]>,
    from: Vec3,
    to: Vec3,
    index: u32,
) -> bool {
    let sight = level.sight_states(states);
    clear_near(level, sight.as_deref().or(states), from, to, index)
}

/// `in_sight_near` with `states` already the line of sight's
/// (`Level::sight_states`).
fn clear_near(level: &Level, states: Option<&[u8]>, from: Vec3, to: Vec3, index: u32) -> bool {
    let trace = level.line(states, from, to);
    crate::world::unobstructed(&trace)
        || trace.entity == Some(index)
        || (!trace.start_solid && math::length(math::sub(to, trace.end)) <= SIGHT_SLACK)
}

/// Whether `candidate` is in sight of `from` (metres) for the crosshair of a
/// player with its feet at `user`: the line to its middle or its top
/// (`in_sight_near`, Peragus' consoles), or the line a use looks along
/// (`seen_at_action_point`); `states` are the line of sight's
/// (`Level::sight_states`).
fn seen(
    level: &Level,
    states: Option<&[u8]>,
    from: Vec3,
    user: Vec3,
    candidate: &Candidate,
) -> bool {
    body_seen(level, states, from, candidate)
        || seen_at_action_point(level, states, from, user, candidate)
}

/// Whether the line from `from` (metres) to `candidate`'s middle or its top
/// is clear (`in_sight_near`).
fn body_seen(level: &Level, states: Option<&[u8]>, from: Vec3, candidate: &Candidate) -> bool {
    [0.5, 1.0]
        .into_iter()
        .any(|share| clear_near(level, states, from, candidate.up(share), candidate.index))
}

/// Whether the line from `from` to where a use by a player with its feet at
/// `user` looks at `candidate` (`Candidate::action_sight`) is clear. A use
/// goes by that line (`uses::may_use`), so the crosshair offers what is
/// used from its action point though a solid stands round its body: the
/// force cage round Lonna Vash's body on Korriban, which a player could
/// search only by jumping over the cage. One with its action point behind
/// a wall stays hidden.
fn seen_at_action_point(
    level: &Level,
    states: Option<&[u8]>,
    from: Vec3,
    user: Vec3,
    candidate: &Candidate,
) -> bool {
    candidate
        .action_sight(user)
        .is_some_and(|to| clear_near(level, states, from, to, candidate.index))
}

/// Where along `look` (metres from its start) it enters `candidate`'s
/// cylinder, if it does.
fn enters(look: &Look, candidate: &Candidate) -> Option<f32> {
    let (o, d) = (look.from, look.forward);
    let (px, py) = (o[0] - candidate.feet[0], o[1] - candidate.feet[1]);
    let a = d[0] * d[0] + d[1] * d[1];
    let b = 2.0 * (px * d[0] + py * d[1]);
    let c = px * px + py * py - candidate.radius * candidate.radius;
    let inside_z = |t: f32| {
        let z = o[2] + d[2] * t - candidate.feet[2];
        (0.0..=candidate.height).contains(&z)
    };
    if a < 1e-6 {
        // Straight up or down: inside the circle, through its top or foot.
        return (c <= 0.0).then(|| {
            let t = if d[2] > 0.0 {
                candidate.feet[2] - o[2]
            } else {
                o[2] - candidate.feet[2] - candidate.height
            };
            t.max(0.0)
        });
    }
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
        return None;
    }
    let root = disc.sqrt();
    [(-b - root) / (2.0 * a), (-b + root) / (2.0 * a)]
        .into_iter()
        .find(|&t| t >= 0.0 && inside_z(t))
}

/// The object `look` picks among `candidates` (see the module
/// documentation), by its number.
pub fn pick(
    level: &Level,
    states: Option<&[u8]>,
    look: &Look,
    looker: &Looker,
    candidates: &[Candidate],
) -> Option<u32> {
    // Clear of the player's body where it must be (`Candidate::ahead`).
    let clear = |candidate: &&Candidate| {
        let (dx, dy) = (
            candidate.feet[0] - looker.feet[0],
            candidate.feet[1] - looker.feet[1],
        );
        !candidate.ahead || dx * looker.facing[0] + dy * looker.facing[1] > candidate.radius
    };
    let near = |candidate: &&Candidate| {
        math::flat_distance(candidate.feet, looker.feet) <= RANGE && clear(candidate)
    };
    // The crosshair's ray, as far as the range reaches past the camera.
    let length = RANGE + math::flat_distance(look.from, looker.feet);
    let end = math::mad(look.from, length, look.forward);
    let ray = |states: Option<&[u8]>| {
        level.with(states, |world| {
            world.trace(look.from, end, &Extents::POINT, Contents::SOLID)
        })
    };
    // The ray as it meets objects (a placeable is picked by its own body),
    // and as far as the player sees along it, past every placeable.
    let trace = ray(states);
    let sight = level.sight_states(states);
    let states = sight.as_deref().or(states);
    let solid = ray(states).fraction * length;
    let hit = candidates
        .iter()
        .filter(near)
        .filter_map(|candidate| {
            let at = if trace.fraction < 1.0 && trace.entity == Some(candidate.index) {
                trace.fraction * length
            } else {
                // A volume poking through a wall is not hit through it, but
                // one used from an action point in sight is.
                enters(look, candidate).filter(|t| {
                    (*t <= solid && body_seen(level, states, look.from, candidate))
                        || seen_at_action_point(level, states, look.from, looker.feet, candidate)
                })?
            };
            Some((at, candidate.index))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0));
    if let Some((_, index)) = hit {
        return Some(index);
    }
    // Soft aim: the nearest the crosshair, in sight.
    let cone = math::sin_cos_deg(CONE_DEGREES).1;
    let soft = candidates
        .iter()
        .filter(near)
        .filter_map(|candidate| {
            let to = math::sub(candidate.middle(), look.from);
            let cos = math::dot(to, look.forward) / math::length(to).max(1e-6);
            (cos >= cone && seen(level, states, look.from, looker.feet, candidate))
                .then_some((cos, candidate.index))
        })
        .max_by(|a, b| a.0.total_cmp(&b.0));
    if let Some((_, index)) = soft {
        return Some(index);
    }
    // At its feet: the nearest within reach in front, in sight of its
    // eyes.
    let eyes = [looker.feet[0], looker.feet[1], looker.feet[2] + EYES];
    candidates
        .iter()
        .filter(clear)
        .filter(|candidate| seen(level, states, eyes, looker.feet, candidate))
        .filter_map(|candidate| {
            crate::containers::ahead(looker.feet, looker.facing, candidate.feet)
                .map(|distance| (distance, candidate.index))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, index)| index)
}

/// Those of `candidates` that `pick` may choose for `looker`: within its
/// range, or within reach in front (its last resort). It traces a line of
/// sight to each it weighs, and the rest could never be chosen.
pub fn weighed(candidates: &[Candidate], looker: &Looker) -> Vec<Candidate> {
    candidates
        .iter()
        .filter(|candidate| {
            math::flat_distance(candidate.feet, looker.feet) <= RANGE
                || crate::containers::ahead(looker.feet, looker.facing, candidate.feet).is_some()
        })
        .copied()
        .collect()
}

/// What may be done to a target (T8), the first the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// A shut door, a container (a locked one tries the party's key).
    Open,
    /// A useable placeable; an open door (its click script).
    Use,
    /// An open door the player could open again, closed (QUIRKS.md
    /// Q-0330: KOTOR's player never closes a door).
    Close,
    /// A creature that speaks.
    Talk,
    /// Security, with a spike the party carries (by script object id) or
    /// none.
    Unlock {
        spike: Option<u32>,
    },
    /// A downed party member, revived by holding use on it (`Reviving`).
    Revive,
    /// A hostile creature, an item the player's body wears used on it (a
    /// droid's utility item: `Action::UseItem`), from within its spell's
    /// reach (decimetres).
    Item {
        item: u32,
        reach_dm: u16,
    },
    /// A trap the party detected: disarmed, or recovered for its kit.
    Disarm,
    Recover,
    /// TSL's: a mine the party carries set on a locked door or placeable,
    /// to blast its lock (`Action::Mine`).
    Mine,
}

/// What the character choosing brings to a lock: offered Security only
/// with ranks in it (K1 lists the skill for no one else), with the best
/// spike the party carries, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Security {
    pub spike: Option<u32>,
}

/// What the character choosing brings to a mine: offered only with ranks in
/// Demolitions (K1's menu asks the skill's use as it does Security's), with
/// whether the mine's row has a kit to recover (`recover`) and, in TSL,
/// whether the party carries a kit to set on a lock (`lock_mine`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Demolitions {
    pub recover: bool,
    pub lock_mine: bool,
}

/// What may be done to an object of `kind` with `facts` in `state`: a
/// locked door or container is opened (tried with the party's key, else
/// its fail-to-open script) or unlocked by a character with `security`
/// (and the spike carried, if any) unless only its key opens it;
/// a shut door or a container opened; an open door closed where it may be
/// (`uses::closable`), and used; a useable placeable used; a creature not
/// hostile that speaks (a conversation or a dialogue hook) talked to; by a
/// character with `demolitions`, a disarmable trap the party detected
/// disarmed, or recovered where its row has a kit, and in TSL a mine set on
/// a locked door or placeable a mine may blast (`traps::blastable`) where
/// the party carries one. Nothing else.
pub fn choices(
    kind: ObjectKind,
    facts: &crate::module::ObjectData,
    state: crate::snapshot::ObjectState,
    hostile: bool,
    security: Option<Security>,
    demolitions: Option<Demolitions>,
) -> Vec<Choice> {
    let Demolitions { recover, lock_mine } = demolitions.unwrap_or(Demolitions {
        recover: false,
        lock_mine: false,
    });
    let skilled = demolitions.is_some();
    let locked = state.locked(facts.opens);
    let key_only = facts.lock.as_ref().is_some_and(|lock| lock.key_required);
    let unlock = || {
        let mut all = vec![Choice::Open];
        if let Some(Security { spike }) = security.filter(|_| !key_only) {
            all.push(Choice::Unlock { spike: None });
            all.extend(spike.map(|spike| Choice::Unlock { spike: Some(spike) }));
        }
        all
    };
    let mined = |mut all: Vec<Choice>| {
        all.extend(
            (skilled && lock_mine && crate::traps::blastable(facts, locked))
                .then_some(Choice::Mine),
        );
        all
    };
    match kind {
        ObjectKind::Door if locked && state.state == 0 => mined(unlock()),
        ObjectKind::Door if state.state == 0 => vec![Choice::Open],
        ObjectKind::Door if crate::uses::closable(facts, &state) => {
            vec![Choice::Close, Choice::Use]
        }
        ObjectKind::Door => vec![Choice::Use],
        ObjectKind::Placeable if facts.container && locked => mined(unlock()),
        ObjectKind::Placeable if locked => mined(vec![Choice::Use]),
        ObjectKind::Placeable if facts.container => vec![Choice::Open],
        ObjectKind::Placeable => vec![Choice::Use],
        ObjectKind::Trigger
            if skilled
                && state.trap & crate::snapshot::trap::PARTY_SEES != 0
                && facts.trap.as_ref().is_some_and(|trap| trap.disarmable) =>
        {
            let mut all = vec![Choice::Disarm];
            if recover {
                all.push(Choice::Recover);
            }
            all
        }
        ObjectKind::Creature => {
            if crate::script::hooks::speaks(facts) && !hostile {
                vec![Choice::Talk]
            } else {
                Vec::new()
            }
        }
        _ => Vec::new(),
    }
}

/// What a choice asks of the host (`crate::action`): none for a revive,
/// whose body goes as `Action::Revive` when its hold begins and which the
/// player's held use does (`Reviving`), nor for an item, whose user and
/// target go as `Action::UseItem`.
pub fn ask(choice: Choice, object: u32, kind: ObjectKind) -> Option<crate::action::Action> {
    use crate::action::{Action, Verb};
    let verb = match (choice, kind) {
        (Choice::Unlock { spike }, _) => return Some(Action::Unlock { object, spike }),
        (Choice::Revive | Choice::Item { .. }, _) => return None,
        (Choice::Disarm, _) => return Some(Action::Disarm(object)),
        (Choice::Recover, _) => return Some(Action::Recover(object)),
        (Choice::Mine, _) => return Some(Action::Mine(object)),
        (Choice::Open, _) | (Choice::Use, ObjectKind::Door) => Verb::Open,
        (Choice::Close, _) => Verb::Close,
        (Choice::Use, _) => Verb::Use,
        (Choice::Talk, _) => Verb::Talk,
    };
    Some(Action::Use { object, verb })
}

/// How far (metres, across the floor) a choice reaches: its verb's
/// (`world::uses::reach`); an item its spell's; a mine's disarm or recover
/// `traps::REACH_M` from its middle; Security and a revive a container's.
pub fn reach(choice: Choice, kind: ObjectKind) -> f32 {
    match choice {
        Choice::Item { reach_dm, .. } => return f32::from(reach_dm) / 10.0,
        Choice::Disarm | Choice::Recover => return crate::traps::REACH_M,
        _ => {}
    }
    match ask(choice, 0, kind) {
        Some(crate::action::Action::Use { verb, .. }) => crate::uses::reach(kind, verb),
        _ => crate::containers::REACH,
    }
}

/// How far from a door's action point (metres, across the floor) a
/// character uses it, past its personal space (K1's and TSL's
/// `CSWSCreature::GetUseRange`).
pub const DOOR_POINT_REACH: f32 = rules::interaction::using::DOOR_POINT_REACH_M;

/// How far from its action point (metres, across the floor) a player uses
/// a door: `DOOR_POINT_REACH` past a humanoid's personal space.
pub fn door_point_reach() -> f32 {
    DOOR_POINT_REACH + crate::world::strikes::humanoid_space(0.0)
}

/// A door's action point for a user at `from` (feet, metres), on the
/// floor (feet, metres): of its two (`ObjectData::action_points`) the
/// nearer, unless only the other stands on the walkable floor, as the
/// exes' `CSWSDoor::GetNearestActionPoint` picks it; `None` for a door
/// without them (or anything else).
pub fn action_point(level: &Level, facts: &crate::module::ObjectData, from: Vec3) -> Option<Vec3> {
    if facts.kind != ObjectKind::Door {
        return None;
    }
    let origin = |at: Vec3| {
        let [x, y, z] = math::scale(at, 1.0 / UNIT);
        [x, y, z - crate::motion::SOLE_Z]
    };
    // A point is on the floor by its x and y, as `walk::on_floor` stands it:
    // a door's points keep its own height, under a ramp rising to it (Taris'
    // Upper Sewers door before the rancor, 0.5 m under its floor).
    let floored: Vec<Vec3> = facts
        .action_points(from)
        .into_iter()
        .map(|point| crate::walk::on_floor(level, origin(point)))
        .collect();
    let point = floored
        .iter()
        .find(|point| crate::walk::on_mesh(level, **point))
        .or(floored.first())?;
    Some(feet_at(*point))
}

/// Where a walk to do a choice heads (feet, metres), how near it must
/// come (metres, across the floor) for the choice to be done there, and
/// the farthest the choice reaches from it (`reach`), where a walk that
/// can come no nearer does it (`Walk::stride`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spot {
    pub at: Vec3,
    pub within: f32,
    pub reach: f32,
}

impl Spot {
    /// `choice` on an object of `kind` standing at `feet`: within the
    /// choice's `arrival`.
    pub fn plain(feet: Vec3, choice: Choice, kind: ObjectKind) -> Spot {
        Spot {
            at: feet,
            within: arrival(choice, kind),
            reach: reach(choice, kind),
        }
    }

    /// `choice` on the object of `facts` standing at `feet`, by a player
    /// at `from` (feet, metres): a door with action points from the one on
    /// the player's side (`action_point`), within `ARRIVE` of
    /// `door_point_reach`, as KOTOR walks a creature to it, never into a
    /// wide door's face; anything else `plain`.
    pub fn of(
        level: &Level,
        choice: Choice,
        facts: &crate::module::ObjectData,
        feet: Vec3,
        from: Vec3,
    ) -> Spot {
        match action_point(level, facts, from) {
            Some(at) => Spot {
                at,
                within: door_point_reach() * ARRIVE,
                reach: door_point_reach(),
            },
            None => Spot::plain(feet, choice, facts.kind),
        }
    }

    /// `choice` on target `index` of `snapshot`, of `kind` standing at
    /// `feet`, by `player`: `of` with its facts, else `plain`.
    pub fn in_snapshot(
        level: &Level,
        snapshot: &Snapshot,
        (index, kind): (u32, ObjectKind),
        choice: Choice,
        feet: Vec3,
        player: &crate::motion::MotionState,
    ) -> Spot {
        let instances = Instances::in_snapshot(level, snapshot);
        match entity(snapshot, index).and_then(|entity| instances.of_entity(entity)) {
            Some(facts) => Spot::of(level, choice, facts, feet, feet_of(player)),
            None => Spot::plain(feet, choice, kind),
        }
    }

    /// How far from `at` (metres, across the floor) a walk to do `choice`
    /// here heads: halfway into the arrival; a trap's `TRAP_CLEAR_M`
    /// outside the trap's circle (`traps::RADIUS_M`), as KOTOR disarms from
    /// beside a mine, never on it: the stop of a run slides on.
    pub fn stand_off(&self, choice: Choice) -> f32 {
        match choice {
            Choice::Disarm | Choice::Recover => trap_stand_off(),
            _ => self.within * rules::targeting::walk::SHARE,
        }
    }
}

/// How far from a mine (metres, across the floor) a walk to disarm or
/// recover it stops.
fn trap_stand_off() -> f32 {
    crate::traps::RADIUS_M + TRAP_CLEAR_M
}

/// How near its target (metres, across the floor) a walk to do `choice`
/// has arrived: `ARRIVE` of its reach; a trap's `TRAP_ARRIVE_M` beyond
/// where it stops (`trap_stand_off`), still outside the trap.
fn arrival(choice: Choice, kind: ObjectKind) -> f32 {
    match choice {
        Choice::Disarm | Choice::Recover => trap_stand_off() + TRAP_ARRIVE_M,
        _ => reach(choice, kind) * ARRIVE,
    }
}

/// Whether `choice` is a skill's use the host breaks off when its user
/// moves (Security, a mine disarmed, recovered or set on a door:
/// `world::traps::skill_uses`), so a walk to do it ends standing still.
fn stands_for(choice: Choice) -> bool {
    matches!(
        choice,
        Choice::Unlock { .. } | Choice::Disarm | Choice::Recover | Choice::Mine
    )
}

/// How long a walk to a target may last (ms), and how many times its
/// walker may stall.
pub const WALK_MS: u64 = rules::targeting::walk::WALK_MS;
const STALLS: u8 = rules::targeting::walk::STALLS;
/// What share of a choice's reach a walk ends within.
const ARRIVE: f32 = rules::targeting::walk::ARRIVE;
/// How far outside a mine's circle a walk to it stops, and how much
/// farther it has arrived (metres).
const TRAP_CLEAR_M: f32 = rules::targeting::walk::TRAP_CLEAR_M;
const TRAP_ARRIVE_M: f32 = rules::targeting::walk::TRAP_ARRIVE_M;
/// How far (metres) a target may move before its walk is planned again.
const MOVED: f32 = rules::targeting::walk::MOVED;

/// A walk to a target to do a choice there (T16, T17): a walker to a point
/// before the target inside the choice's reach, or, where no route leads
/// (a door that leads out stands on the faces before it), straight at that
/// point; planned again when the target moves.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Walk {
    pub object: u32,
    pub choice: Choice,
    walker: crate::walk::Walker,
    /// Where the target stood when the walk was planned (metres).
    toward: Vec3,
    direct: bool,
    started_ms: u64,
    /// The nearest the walk has come to its target (metres, across the
    /// floor), and since when.
    nearest: f32,
    nearest_ms: u64,
}

/// What a walk does this step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Stride {
    /// The player's command in place of its own.
    Steer(crate::Command),
    /// Within reach (and, for a skill's use, standing still): the choice
    /// is asked now, the walk over.
    Arrived,
    /// Stalled or out of time: the walk is over.
    Unreachable,
}

/// Where the player stands for a walk: its body (`origin`, units) and
/// feet (metres).
fn feet_of(player: &crate::motion::MotionState) -> Vec3 {
    feet_at(player.origin)
}

/// The feet (metres) of a body whose origin is `origin` (units).
pub fn feet_at(origin: Vec3) -> Vec3 {
    math::scale(
        [origin[0], origin[1], origin[2] + crate::motion::SOLE_Z],
        UNIT,
    )
}

impl Walk {
    /// Whether the target still offers the walk's choice (`choices`): a
    /// door opened, a lock picked, a creature turned hostile on the way
    /// ends it.
    pub fn wanted(&self, choices: &[Choice]) -> bool {
        choices.contains(&self.choice)
    }

    /// Whether `player` is near enough `spot` to do its choice without
    /// walking.
    pub fn near(player: &crate::motion::MotionState, spot: Spot) -> bool {
        math::flat_distance(feet_of(player), spot.at) <= spot.within
    }

    /// A walk from `player` to object `object` to do `choice` at `spot`,
    /// from `now_ms`.
    pub fn new(
        level: &Level,
        ways: &dyn crate::walk::Ways,
        player: &crate::motion::MotionState,
        object: u32,
        spot: Spot,
        choice: Choice,
        now_ms: u64,
    ) -> Walk {
        let mut walk = Walk {
            object,
            choice,
            walker: crate::walk::Walker::new(player.origin),
            toward: spot.at,
            direct: false,
            started_ms: now_ms,
            nearest: f32::MAX,
            nearest_ms: now_ms,
        };
        walk.plan(level, ways, player, spot);
        walk
    }

    fn plan(
        &mut self,
        level: &Level,
        ways: &dyn crate::walk::Ways,
        player: &crate::motion::MotionState,
        spot: Spot,
    ) {
        let to = spot.at;
        let feet = feet_of(player);
        let (dx, dy) = (feet[0] - to[0], feet[1] - to[1]);
        let length = (dx * dx + dy * dy).sqrt().max(1e-3);
        let stand = spot.stand_off(self.choice);
        let goal = math::scale(
            [
                to[0] + dx / length * stand,
                to[1] + dy / length * stand,
                to[2],
            ],
            1.0 / UNIT,
        );
        self.walker = crate::walk::Walker::new([goal[0], goal[1], goal[2] - crate::motion::SOLE_Z]);
        self.direct =
            crate::walk::route_ways(level, player.origin, self.walker.goal, ways).is_none();
        self.toward = to;
    }

    /// Whether the walk has come no nearer `spot` for a walker's stall
    /// (`walk::STALL_MS`) while within the choice's own reach of it: a
    /// counter or a desk between, where no way leads round (Greeta Holda
    /// in Anchorhead's Czerka office, 3.4 m off across hers), as KOTOR's
    /// walk to talk ends where its path does and the conversation starts
    /// there, within range [verify in the exe].
    fn no_nearer(&mut self, player: &crate::motion::MotionState, spot: Spot, now_ms: u64) -> bool {
        let off = math::flat_distance(feet_of(player), spot.at);
        if off < self.nearest - rules::walking::route::HEADWAY * UNIT {
            self.nearest = off;
            self.nearest_ms = now_ms;
        }
        off <= spot.reach && now_ms >= self.nearest_ms + crate::walk::STALL_MS
    }

    /// This step of the walk for `player`, its target's `spot` now, at
    /// `now_ms`.
    pub fn stride(
        &mut self,
        level: &Level,
        ways: &dyn crate::walk::Ways,
        player: &crate::motion::MotionState,
        spot: Spot,
        now_ms: u64,
    ) -> Stride {
        let late = now_ms >= self.started_ms + WALK_MS;
        if Walk::near(player, spot) || self.no_nearer(player, spot, now_ms) {
            return self.arrived(player, late);
        }
        if late || self.walker.stalls >= STALLS {
            return Stride::Unreachable;
        }
        if math::flat_distance(spot.at, self.toward) > MOVED {
            self.plan(level, ways, player, spot);
        }
        let command = if self.direct {
            crate::walk::toward(player, self.walker.goal)
        } else {
            self.walker.command(level, player, now_ms, ways)
        };
        // KOTOR's walk to a clicked target never leaves the area (only a
        // click on the transition does): before a way out it stops, and,
        // stopped, does the choice there if it reaches (C8-42 knocked back
        // behind the Crystal Cave's exit on Dantooine, #255), else is out
        // of reach. Stopped short by its run's slide, it walks on.
        if leaves(ways, player, &command) {
            if moving(player) {
                self.walker.keep_headway(now_ms, player);
                return Stride::Steer(still(player));
            }
            return if math::flat_distance(feet_of(player), spot.at) <= spot.reach {
                self.arrived(player, late)
            } else {
                Stride::Unreachable
            };
        }
        Stride::Steer(command)
    }

    /// The walk arrived: the choice is done, but a skill's use first stops
    /// a run (the slide would carry its user past where it breaks off on
    /// moving).
    fn arrived(&self, player: &crate::motion::MotionState, late: bool) -> Stride {
        if moving(player) && stands_for(self.choice) && !late {
            return Stride::Steer(still(player));
        }
        Stride::Arrived
    }
}

/// Whether `player` moves across the floor.
fn moving(player: &crate::motion::MotionState) -> bool {
    math::length([player.velocity[0], player.velocity[1], 0.0])
        >= rules::movement::ground::STILL_BELOW
}

/// The command that lets `player`'s run slide to a stop, looking as it
/// does.
fn still(player: &crate::motion::MotionState) -> crate::Command {
    let [pitch, yaw, _] = player.view;
    crate::combat::mind::looking(player, pitch, yaw)
}

/// How far past where its run's slide would end (metres) a walk looks for a
/// way out it is not to take.
const EXIT_MARGIN_M: f32 = rules::targeting::walk::EXIT_MARGIN_M;
/// How often along the way it looks (metres): a trigger may be thin.
const EXIT_STEP_M: f32 = 0.25;

/// Whether `command` (or, let go, its run's slide) would carry `player`
/// into a way out `ways` keeps it from (`Ways::leads_out`), looked for
/// every `EXIT_STEP_M` as far as the slide at its speed reaches and
/// `EXIT_MARGIN_M` on: never one it stands in already (where it arrived),
/// whose way the sim has taken or not.
fn leaves(
    ways: &dyn crate::walk::Ways,
    player: &crate::motion::MotionState,
    command: &crate::Command,
) -> bool {
    let body = math::scale(player.origin, UNIT);
    if ways.leads_out(body) {
        return false;
    }
    let speed = math::length([player.velocity[0], player.velocity[1], 0.0]);
    let ahead = speed / crate::motion::FRICTION * UNIT + EXIT_MARGIN_M;
    let steps = (ahead / EXIT_STEP_M).ceil() as usize;
    let out_along = |way: [f32; 2]| {
        let length = math::length([way[0], way[1], 0.0]);
        length > f32::EPSILON
            && (1..=steps).any(|step| {
                let d = ahead * step as f32 / steps as f32;
                ways.leads_out([
                    body[0] + way[0] / length * d,
                    body[1] + way[1] / length * d,
                    body[2],
                ])
            })
    };
    let yaw = math::short_to_degrees(command.angles[1].wrapping_add(player.view_offset[1]));
    let (ahead_of, side, _) = math::angle_vectors([0.0, yaw, 0.0]);
    let (f, r) = (f32::from(command.forward), f32::from(command.right));
    let driven = [ahead_of[0] * f + side[0] * r, ahead_of[1] * f + side[1] * r];
    out_along(driven) || out_along([player.velocity[0], player.velocity[1]])
}

/// The ways a walk to target `object` keeps out of (`Walk::stride`), over
/// the walk's own `ways`: every trigger and door of the snapshot that leads
/// out of the module but the target itself.
pub struct KeptIn<'a> {
    ways: &'a dyn crate::walk::Ways,
    level: &'a Level,
    triggers: Vec<std::sync::Arc<crate::module::ObjectData>>,
    doors: Vec<u32>,
}

impl<'a> KeptIn<'a> {
    pub fn new(
        level: &'a Level,
        ways: &'a dyn crate::walk::Ways,
        snapshot: &Snapshot,
        object: u32,
    ) -> Self {
        let instances = Instances::in_snapshot(level, snapshot);
        let (mut triggers, mut doors) = (Vec::new(), Vec::new());
        for entity in &snapshot.entities {
            let Kind::Object(kind, index) = entity.kind else {
                continue;
            };
            let Some(facts) = instances
                .facts(index)
                .filter(|facts| facts.exit().is_some())
            else {
                continue;
            };
            match kind {
                ObjectKind::Trigger => triggers.push(facts.clone()),
                ObjectKind::Door if index != object => doors.push(index),
                _ => {}
            }
        }
        KeptIn {
            ways,
            level,
            triggers,
            doors,
        }
    }
}

impl crate::walk::Ways for KeptIn<'_> {
    fn door(&self, index: u32) -> crate::ObjectState {
        self.ways.door(index)
    }

    fn walls(&self, face: u32) -> bool {
        self.ways.walls(face)
    }

    fn leads_out(&self, body: Vec3) -> bool {
        self.triggers
            .iter()
            .any(|facts| self.level.in_trigger(facts, body))
            || self
                .doors
                .iter()
                .any(|&index| self.level.at_door(index, body))
    }
}

/// What of the player's own cancels a walk besides moving and jumping: an
/// attack (T17).
pub const WALK_BREAKS: u16 = crate::buttons::ATTACK | crate::buttons::ALT_ATTACK;
/// What cancels a revive's hold besides moving and jumping: any action,
/// as the host breaks a revive off for one (`falls::ACTING`).
pub const HOLD_BREAKS: u16 = crate::falls::ACTING;

/// The command the player's tick takes: its own (`base`: the keys', a
/// script's), or a walk's or a revive's hold (`piloted`) in its place; and
/// whether `base` cancels it (it moves, jumps, or presses one of
/// `breaks`).
pub fn steer(
    piloted: Option<crate::Command>,
    base: crate::Command,
    breaks: u16,
) -> (crate::Command, bool) {
    let Some(command) = piloted else {
        return (base, false);
    };
    if base.forward != 0 || base.right != 0 || base.up > 0 || base.buttons & breaks != 0 {
        return (base, true);
    }
    (
        crate::Command {
            reply: base.reply,
            weapon: base.weapon,
            force: base.force,
            ..command
        },
        false,
    )
}

/// How long a chosen revive waits for the host to show it under way (ms
/// of the sim): past it, another player revives the body or it is not
/// ahead, and the hold ends.
pub const REVIVE_WAIT_MS: u64 = rules::targeting::revive::WAIT_MS;
/// How far down a reviver looks at the body (degrees).
const REVIVE_PITCH: f32 = rules::targeting::revive::PITCH;

/// A revive chosen in the target block (E on Revive, after any walk
/// there): the body is asked for (`Action::Revive`), and the player's
/// commands hold use (`buttons::USE_HELD`) facing it, standing still,
/// until it stands (`world::falls` does the revive
/// and keeps its rules). It ends on the player's own move, jump or action
/// (`steer` with `HOLD_BREAKS`), on the body standing or going, and when
/// the host's revive by this player breaks off (a hit, the body out of
/// reach) or begins again, or never begins (`REVIVE_WAIT_MS`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reviving {
    /// The body's number as a target.
    pub object: u32,
    began_ms: u64,
    /// When the host's revive by this player began, once seen.
    seen: Option<u64>,
}

impl Reviving {
    /// A hold on target `object` from `now_ms` (the sim's).
    pub fn new(object: u32, now_ms: u64) -> Reviving {
        Reviving {
            object,
            began_ms: now_ms,
            seen: None,
        }
    }

    /// This step's command for `me` reviving `body` (as the snapshot of
    /// `now_ms` has them), or `None` when the hold is over.
    pub fn keep(
        &mut self,
        body: Option<&EntityState>,
        me: &EntityState,
        now_ms: u64,
    ) -> Option<crate::Command> {
        let body = body.filter(|body| revivable(body))?;
        let (lies, player) = (body.player?, me.player?);
        let ours = body
            .fighter
            .and_then(|fighter| fighter.fallen?.revive)
            .filter(|revive| revive.by == me.id);
        match (ours, self.seen) {
            (Some(revive), None) => self.seen = Some(revive.from_ms),
            (Some(revive), Some(from)) if revive.from_ms != from => return None,
            (None, Some(_)) => return None,
            (None, None) if now_ms >= self.began_ms + REVIVE_WAIT_MS => return None,
            _ => {}
        }
        let (_, yaw) = crate::combat::mind::aim(player.origin, lies.origin);
        Some(crate::Command {
            buttons: crate::buttons::USE_HELD,
            ..crate::combat::mind::looking(&player, REVIVE_PITCH, yaw)
        })
    }
}

/// A target held `HOLD_MS` after nothing picks it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Hold {
    target: Option<u32>,
    seen_ms: u64,
}

impl Hold {
    /// The target now, given what was picked at `now_ms`.
    pub fn update(&mut self, picked: Option<u32>, now_ms: u64) -> Option<u32> {
        match picked {
            Some(index) => {
                self.target = Some(index);
                self.seen_ms = now_ms;
            }
            None if now_ms >= self.seen_ms + HOLD_MS => self.target = None,
            None => {}
        }
        self.target
    }
}

#[cfg(feature = "probes")]
mod probes;
#[cfg(feature = "probes")]
pub use probes::{action_point_probes, cage_probes, probes};
