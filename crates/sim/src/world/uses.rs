//! Using an object, KOTOR's click on it: a door opens or closes, a
//! container opens, a placeable is used, what speaks is talked to. The
//! per-kind meaning is shared by a player's use key (`interact`'s trace)
//! and a seat's `Action::Use`, which is first checked here: the verb one
//! the object takes (`applies`), within the verb's reach, and nothing solid
//! between the player's eye and the object but the object itself
//! (`may_use`); the player up (its caller). A refused Use does nothing.

use collision::UNIT;

use super::{Object, scripts};
use crate::action::Verb;
use crate::input::Seat;
use crate::math;
use crate::module::{Link, ObjectData, ObjectKind, Opens};
use crate::motion::MotionState;
use crate::snapshot::SimId;

/// How far (metres, across the floor) a player reaches with `verb`: a
/// conversation's range, a door's reach, a container's otherwise.
pub fn reach(kind: ObjectKind, verb: Verb) -> f32 {
    match (kind, verb) {
        (_, Verb::Talk) => crate::script::CONVERSATION_RANGE,
        (ObjectKind::Door, _) => crate::script::DOOR_REACH,
        _ => super::containers::REACH,
    }
}

/// How high above where an object stands the line to it ends (metres): a
/// door's or a container's middle.
pub const MIDDLE_M: f32 = rules::interaction::using::MIDDLE_M;

/// How far above a placeable's action point its line of sight ends.
pub const ACTION_SIGHT_M: f32 = rules::interaction::using::ACTION_SIGHT_M;

/// How far above or below a player (metres) what it uses may stand.
pub const HEIGHT_REACH_M: f32 = rules::interaction::using::HEIGHT_REACH_M;

/// What a Use is aimed at: its object, where it stands (metres) and the
/// point a line to it ends at; a door's action point on the user's side
/// (`target::action_point`), if it has them.
pub(super) struct Target {
    pub index: u32,
    pub at: [f32; 3],
    pub middle: [f32; 3],
    pub point: Option<[f32; 3]>,
}

impl Target {
    /// This target as a use from `from` (metres) sees it: a placeable with
    /// action points (`ObjectData::use_points`) at the nearest of them,
    /// seen `ACTION_SIGHT_M` up it and reached to it where it is nearer
    /// than where the placeable stands, as TSL's
    /// `CSWSCreature::GetIsInUseRange` looks and measures
    /// (`ObjectData::use_distance`; Korriban's Lonna Vash, searched from
    /// beside the force cage round her body; Peragus's broken droid in the
    /// hangar, its action point 3 m from where it stands); anything else
    /// at its middle. A door's action point is where it may also be used
    /// from (`may_use`).
    pub fn aimed(
        mut self,
        level: &super::Level,
        kind: ObjectKind,
        facts: &ObjectData,
        from: [f32; 3],
    ) -> Target {
        match kind {
            ObjectKind::Placeable => {
                if let Some(point) = facts.nearest_use_point(from) {
                    if math::flat_distance(point, from) < math::flat_distance(self.at, from) {
                        self.at = point;
                    }
                    self.middle = [point[0], point[1], point[2] + ACTION_SIGHT_M];
                }
            }
            ObjectKind::Door => self.point = crate::target::action_point(level, facts, from),
            _ => {}
        }
        self
    }

    /// A creature at its body's origin (metres), aimed at its eyes (over
    /// a counter it stands behind); anything else where it was placed,
    /// aimed at its middle.
    pub fn new(index: u32, creature: Option<[f32; 3]>, placed: [f32; 3]) -> Target {
        match creature {
            Some(at) => Target {
                index,
                at,
                middle: [at[0], at[1], at[2] + MIDDLE_M],
                point: None,
            },
            None => Target {
                index,
                at: placed,
                middle: [placed[0], placed[1], placed[2] + MIDDLE_M],
                point: None,
            },
        }
    }
}

/// Whether `object` takes `verb`: a door opens, and closes where it may
/// (`closable`), a container opens, a useable placeable is used, a living
/// creature is talked to. Nothing else, and nothing without facts.
pub(super) fn applies(object: &Object, verb: Verb, alive: bool) -> bool {
    let Some(facts) = object.facts.as_deref() else {
        return false;
    };
    match (object.kind, verb) {
        (ObjectKind::Door, Verb::Open) => true,
        (ObjectKind::Door, Verb::Close) => closable(facts, &object.state),
        (ObjectKind::Placeable, Verb::Open) => facts.container,
        (ObjectKind::Placeable, Verb::Use) => facts.useable && !facts.container,
        (ObjectKind::Creature, Verb::Talk) => alive,
        _ => false,
    }
}

/// Whether a player may close a door with `facts` in `state` (QUIRKS.md
/// Q-0330, the owner's: KOTOR's player closes none): open, not broken
/// open, not leading out, and one it could open again itself (not locked,
/// not a door only scripts open), so a close never shuts it out of a way
/// the story opened. Who stands in its doorway stops it too (the host,
/// `interaction::in_doorway`).
pub fn closable(facts: &ObjectData, state: &crate::snapshot::ObjectState) -> bool {
    facts.kind == ObjectKind::Door
        && state.state != 0
        && state.opens(facts.opens)
        && !state.broken(facts)
        && facts.exit().is_none()
}

/// Whether `player` may use `target` (an object of `kind`) with `verb`:
/// within the verb's reach across the floor (a door's also within
/// `target::door_point_reach` of its action point on the player's side,
/// where KOTOR uses it from) and `HEIGHT_REACH_M` in
/// height, and its eye in sight of the target (`target::in_sight`, doors
/// as `states` has them). A creature is talked to within reach across the
/// floor whatever stands between and however far above or below, as
/// KOTOR's conversation action asks only that its speaker come within
/// range on the ground's plane (reone's `navigateTo`, a flat distance)
/// [verify in the exe]: Dantooine's Administrator across a desk taller than
/// the player's eye, Dxun's Kumus on his ridge 6 m above the path.
pub(super) fn may_use(
    level: &super::Level,
    states: Option<&[u8]>,
    player: &MotionState,
    target: &Target,
    kind: ObjectKind,
    verb: Verb,
) -> bool {
    let me = math::scale(player.origin, UNIT);
    let (dx, dy, dz) = (
        target.at[0] - me[0],
        target.at[1] - me[1],
        target.at[2] - me[2],
    );
    let reach = reach(kind, verb);
    let at_point = target
        .point
        .is_some_and(|point| math::flat_distance(me, point) <= crate::target::door_point_reach());
    if dx * dx + dy * dy > reach * reach && !at_point {
        return false;
    }
    if kind == ObjectKind::Creature && verb == Verb::Talk {
        return true;
    }
    if dz.abs() > HEIGHT_REACH_M {
        return false;
    }
    let mut eye = me;
    eye[2] += player.eye_height * UNIT;
    crate::target::in_sight(level, states, eye, target.middle, target.index)
}

/// What the step's uses tell the scripts.
pub(super) struct Events<'a> {
    pub opened: &'a mut scripts::Opened,
    pub closed: &'a mut scripts::Closed,
    pub clicked: &'a mut scripts::Clicked,
    pub used: &'a mut scripts::Used,
}

/// What a use came to, for its caller.
pub(super) enum Outcome {
    /// Done, or nothing to do.
    Done,
    /// Locked: it fails to open (`world::locks`: its key, or the line).
    Locked,
    /// A door that leads out: the seat asks for the transition.
    Leave(Link),
}

/// Whether a container has a script for its use (`OnUsed`), which its
/// every use runs (`use_object`). K1's placeables have no fail-to-open
/// script (its loader reads none), so a use of one locked runs its
/// `OnUsed` too, which answers the lock itself (`world::locks`): Taris's
/// puzzle boxes start their conversation while `GetLocked`, the Ebon
/// Hawk's compartment unlocks itself once its code is known. TSL's that
/// have an `OnFailToOpen` run that instead, as its doors do. Its
/// conversation is only what its scripts start (the sewers' bone pile
/// speaks when the bait is put in, `OnInvDisturbed`).
pub(super) fn used_script(facts: &ObjectData) -> bool {
    crate::script::Hook::Used.of(facts).is_some()
}

/// Whether a placeable has a fail-to-open script (TSL's `OnFailToOpen`).
fn fails_to_open(facts: &ObjectData) -> bool {
    crate::script::Hook::FailToOpen.of(facts).is_some()
}

/// Whether a locked container's use is its `OnUsed`'s to answer
/// (`used_script`), the lock saying nothing itself.
pub(super) fn answers_lock(facts: &ObjectData) -> bool {
    used_script(facts) && !fails_to_open(facts)
}

/// What the use key does to an object: opens a door or a container, uses
/// anything else.
pub(super) fn default_verb(object: &Object) -> Verb {
    let container = object.facts.as_ref().is_some_and(|facts| facts.container);
    if object.kind == ObjectKind::Door || container {
        Verb::Open
    } else {
        Verb::Use
    }
}

/// `seat`'s player `who`, standing at `from` (metres), uses `object`
/// (entity `id`) with `verb`. A door opens away from it (or asks the way
/// out, or is locked, a locked one with a conversation starting it; an
/// open one runs its `OnClick`); a door closes; a container is held open
/// (or is locked), and used as well when it has a use script
/// (`used_script`); a creature or placeable is talked to or used
/// (`scripts::talk_to`). A verb that does not apply does nothing.
pub(super) fn use_object(
    id: SimId,
    object: &mut Object,
    verb: Verb,
    seat: Seat,
    (who, from): (SimId, [f32; 3]),
    events: &mut Events,
) -> Outcome {
    let talks = matches!(verb, Verb::Talk | Verb::Use);
    let Some(facts) = object.facts.clone() else {
        if talks {
            events.used.0.push((id, who));
        }
        return Outcome::Done;
    };
    let locked = object.state.locked(facts.opens);
    match (object.kind, verb) {
        (ObjectKind::Door, Verb::Open) => {
            if let Some(link) = facts.exit().filter(|_| !locked) {
                Outcome::Leave(link.clone())
            } else if object.state.state == 0 && object.state.opens(facts.opens) {
                object.state.state = facts.opening(Some(from));
                events.opened.0.push((id, Some(who), true));
                Outcome::Done
            } else if object.state.state == 0 && facts.opens != Opens::Never {
                // A locked door used may speak its `Conversation` once its
                // fail-to-open script has run (`scripts::door_speaks`); one
                // that opens only opens.
                if !facts.conversation.is_empty() {
                    events.used.0.push((id, who));
                }
                Outcome::Locked
            } else {
                // An open door used: its click script (KOTOR's doors are
                // not closed by a click).
                if object.state.state != 0 {
                    events.clicked.0.push((id, who));
                }
                Outcome::Done
            }
        }
        // A door broken open stays open (`world::objects`).
        (ObjectKind::Door, Verb::Close) => {
            if object.state.state != 0 && !object.state.broken(&facts) {
                object.state.state = 0;
                events.closed.0.push((id, who));
            }
            Outcome::Done
        }
        (ObjectKind::Placeable, Verb::Open) if facts.container => {
            let used = if locked {
                answers_lock(&facts)
            } else {
                used_script(&facts)
            };
            if used {
                events.used.0.push((id, who));
            }
            if locked {
                Outcome::Locked
            } else {
                super::containers::hold(id, &mut object.state, seat, who, events.opened);
                Outcome::Done
            }
        }
        (_, Verb::Talk | Verb::Use) => {
            events.used.0.push((id, who));
            Outcome::Done
        }
        _ => Outcome::Done,
    }
}

#[cfg(feature = "probes")]
#[allow(
    clippy::disallowed_methods,
    reason = "checks and traces set up their own cases on one machine"
)]
pub(crate) mod probes;
#[cfg(feature = "probes")]
pub use probes::{probes, script_probes, security_walk_probes, walk_probes};
