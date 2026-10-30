//! What the player is looking at (`docs/UI.md`),
//! found render side each frame from the presented snapshot: the object
//! the screen's middle (the crosshair) rests on within `RANGE` of the
//! player, else the one nearest the crosshair within `CONE_DEGREES` and in
//! sight, else the nearest within `containers::REACH` in front of the
//! player (JA's camera sits high behind: what lies at the character's feet
//! is far below the crosshair). A target holds `HOLD_MS` after nothing
//! picks it, so it does not flicker at an edge.
//!
//! Targetable: doors but static ones (a door leading out is one too),
//! useable placeables, living creatures but the party's, a trap the
//! party detected, and a downed party member (a seat's avatar or a companion, I30 P11), which a
//! standing one may revive (`Choice::Revive`, `Reviving`).

use collision::{Contents, Extents, UNIT};

use crate::instance::Instances;
use crate::math::{self, Vec3};
use crate::module::{ObjectKind, Opens};
use crate::snapshot::EntityState;
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
const EYES: f32 = rules::targeting::pick::EYES;
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
}

impl Candidate {
    fn middle(&self) -> Vec3 {
        [self.feet[0], self.feet[1], self.feet[2] + self.height * 0.5]
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

/// The targetable objects of `snapshot` (see the module documentation).
pub fn candidates(level: &Level, snapshot: &Snapshot) -> Vec<Candidate> {
    let instances = Instances::in_snapshot(level, snapshot);
    snapshot
        .entities
        .iter()
        .filter_map(|entity| {
            if revivable(entity) {
                return Some(Candidate {
                    index: number(entity)?,
                    kind: ObjectKind::Creature,
                    feet: feet_of(&entity.player?),
                    radius: BODY_RADIUS,
                    height: BODY_HEIGHT,
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
                    let fighter = entity.fighter.as_ref()?;
                    (fighter.alive() && fighter.mind.leader.is_none()).then_some(())?;
                    (CREATURE_RADIUS, CREATURE_HEIGHT)
                }
                ObjectKind::Trigger if seen_trap(facts, entity) => {
                    return Some(Candidate {
                        index,
                        kind,
                        feet: crate::traps::centre(&facts.geometry, entity.position),
                        radius: TRAP_RADIUS,
                        height: TRAP_HEIGHT,
                    });
                }
                _ => return None,
            };
            // A character's position is its body's origin, above its soles;
            // an object's is where it stands (its facts' for one placed, but
            // one play made, as remains, stands where it was made: its
            // template's facts are not there).
            let feet = entity.player.as_ref().map_or(entity.position, feet_of);
            Some(Candidate {
                index,
                kind,
                feet,
                radius,
                height,
            })
        })
        .collect()
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
    let near = |candidate: &&Candidate| math::flat_distance(candidate.feet, looker.feet) <= RANGE;
    // The crosshair's ray, as far as the range reaches past the camera.
    let length = RANGE + math::flat_distance(look.from, looker.feet);
    let end = math::mad(look.from, length, look.forward);
    let trace = level.with(states, |world| {
        world.trace(look.from, end, &Extents::POINT, Contents::SOLID)
    });
    let solid = trace.fraction * length;
    let hit = candidates
        .iter()
        .filter(near)
        .filter_map(|candidate| {
            let at = if trace.fraction < 1.0 && trace.entity == Some(candidate.index) {
                solid
            } else {
                // A volume poking through a wall is not hit through it.
                enters(look, candidate).filter(|t| {
                    *t <= solid
                        && in_sight(
                            level,
                            states,
                            look.from,
                            candidate.middle(),
                            candidate.index,
                        )
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
            (cos >= cone
                && in_sight(
                    level,
                    states,
                    look.from,
                    candidate.middle(),
                    candidate.index,
                ))
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
        .filter(|candidate| in_sight(level, states, eyes, candidate.middle(), candidate.index))
        .filter_map(|candidate| {
            crate::containers::ahead(looker.feet, looker.facing, candidate.feet)
                .map(|distance| (distance, candidate.index))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, index)| index)
}

/// What may be done to a target (T8), the first the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// A shut door, a container (a locked one tries the party's key).
    Open,
    /// A useable placeable; an open door (its click script).
    Use,
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
    /// TSL's: a mine the party carries set on a shut door, to blast its
    /// lock (`Action::Mine`).
    Mine,
}

/// What the character choosing brings to a lock: offered Security only
/// with ranks in it (K1 lists the skill for no one else), with the best
/// spike the party carries, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Security {
    pub spike: Option<u32>,
}

/// What may be done to an object of `kind` with `facts` in `state`: a
/// locked door or container is opened (tried with the party's key, else
/// its fail-to-open script) or unlocked by a character with `security`
/// (and the spike carried, if any) unless only its key opens it;
/// a shut door or a container opened; an open door
/// or a useable placeable used; a creature not hostile that speaks (a
/// conversation or a dialogue hook) talked to; a disarmable trap the party
/// detected disarmed, or recovered where its row has a kit (`recoverable`);
/// in TSL, a mine set on a shut door where the party carries one (`mine`).
/// Nothing else.
pub fn choices(
    kind: ObjectKind,
    facts: &crate::module::ObjectData,
    state: crate::snapshot::ObjectState,
    hostile: bool,
    security: Option<Security>,
    (recoverable, mine): (bool, bool),
) -> Vec<Choice> {
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
    let shut_door = |mut all: Vec<Choice>| {
        all.extend(mine.then_some(Choice::Mine));
        all
    };
    match kind {
        ObjectKind::Door if locked && state.state == 0 => shut_door(unlock()),
        ObjectKind::Door if state.state == 0 => shut_door(vec![Choice::Open]),
        ObjectKind::Door => vec![Choice::Use],
        ObjectKind::Placeable if facts.container && locked => unlock(),
        ObjectKind::Placeable if facts.container => vec![Choice::Open],
        ObjectKind::Placeable => vec![Choice::Use],
        ObjectKind::Trigger
            if state.trap & crate::snapshot::trap::PARTY_SEES != 0
                && facts.trap.as_ref().is_some_and(|trap| trap.disarmable) =>
        {
            let mut all = vec![Choice::Disarm];
            if recoverable {
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
        (Choice::Use, _) => Verb::Use,
        (Choice::Talk, _) => Verb::Talk,
    };
    Some(Action::Use { object, verb })
}

/// How far (metres, across the floor) a choice reaches: its verb's
/// (`world::uses::reach`); an item its spell's; Security and a revive a
/// container's.
pub fn reach(choice: Choice, kind: ObjectKind) -> f32 {
    if let Choice::Item { reach_dm, .. } = choice {
        return f32::from(reach_dm) / 10.0;
    }
    match ask(choice, 0, kind) {
        Some(crate::action::Action::Use { verb, .. }) => crate::uses::reach(kind, verb),
        _ => crate::containers::REACH,
    }
}

/// How far from its target (metres, across the floor) a walk to do
/// `choice` ends: halfway into the choice's arrival; a trap's at the
/// arrival's edge, outside the trap's circle (`traps::RADIUS_M`), as KOTOR
/// disarms from beside a mine, not on it.
pub fn stand_off(choice: Choice, kind: ObjectKind) -> f32 {
    let arrive = reach(choice, kind) * ARRIVE;
    match choice {
        Choice::Disarm | Choice::Recover => arrive * rules::targeting::walk::TRAP_SHARE,
        _ => arrive * rules::targeting::walk::SHARE,
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
    let o = player.origin;
    math::scale([o[0], o[1], o[2] + crate::motion::SOLE_Z], UNIT)
}

impl Walk {
    /// Whether the target still offers the walk's choice (`choices`): a
    /// door opened, a lock picked, a creature turned hostile on the way
    /// ends it.
    pub fn wanted(&self, choices: &[Choice]) -> bool {
        choices.contains(&self.choice)
    }

    /// Whether `player` is near enough `to` (feet, metres) to do `choice`
    /// on an object of `kind` without walking.
    pub fn near(
        player: &crate::motion::MotionState,
        to: Vec3,
        choice: Choice,
        kind: ObjectKind,
    ) -> bool {
        math::flat_distance(feet_of(player), to) <= reach(choice, kind) * ARRIVE
    }

    /// A walk from `player` to object `object` of `kind` standing at `to`
    /// (feet, metres) to do `choice`, from `now_ms`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        level: &Level,
        ways: &dyn crate::walk::Ways,
        player: &crate::motion::MotionState,
        object: u32,
        kind: ObjectKind,
        to: Vec3,
        choice: Choice,
        now_ms: u64,
    ) -> Walk {
        let mut walk = Walk {
            object,
            choice,
            walker: crate::walk::Walker::new(player.origin),
            toward: to,
            direct: false,
            started_ms: now_ms,
        };
        walk.plan(level, ways, player, kind, to);
        walk
    }

    fn plan(
        &mut self,
        level: &Level,
        ways: &dyn crate::walk::Ways,
        player: &crate::motion::MotionState,
        kind: ObjectKind,
        to: Vec3,
    ) {
        let feet = feet_of(player);
        let (dx, dy) = (feet[0] - to[0], feet[1] - to[1]);
        let length = (dx * dx + dy * dy).sqrt().max(1e-3);
        let stand = stand_off(self.choice, kind);
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

    /// This step of the walk for `player`, its target of `kind` at `to`
    /// (feet, metres), at `now_ms`.
    pub fn stride(
        &mut self,
        level: &Level,
        ways: &dyn crate::walk::Ways,
        player: &crate::motion::MotionState,
        kind: ObjectKind,
        to: Vec3,
        now_ms: u64,
    ) -> Stride {
        let late = now_ms >= self.started_ms + WALK_MS;
        if Walk::near(player, to, self.choice, kind) {
            // Arrived at a run, it stops first where the choice breaks off
            // on its user moving: the slide would carry it past.
            let moving = math::length([player.velocity[0], player.velocity[1], 0.0])
                >= rules::movement::ground::STILL_BELOW;
            if moving && stands_for(self.choice) && !late {
                let [pitch, yaw, _] = player.view;
                return Stride::Steer(crate::combat::mind::looking(player, pitch, yaw));
            }
            return Stride::Arrived;
        }
        if late || self.walker.stalls >= STALLS {
            return Stride::Unreachable;
        }
        if math::flat_distance(to, self.toward) > MOVED {
            self.plan(level, ways, player, kind, to);
        }
        Stride::Steer(if self.direct {
            crate::walk::toward(player, self.walker.goal)
        } else {
            self.walker.command(level, player, now_ms, ways)
        })
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
pub use probes::probes;
