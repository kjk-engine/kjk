//! The lock routines (`world::locks` has the players' keys and Security):
//! a door's or placeable's lock facts (`ObjectData::lock`), the creatures'
//! unlock and lock actions, and the door actions KOTOR's AI asks about
//! (`k_ai_master`). Who last unlocked or locked an object is `GetLast…`'s
//! (`engine`).

use script_ncs::vm::{ObjectId, Value};

use super::actions::Action;
use super::engine::{Bound, Engine, int, object, truth};
use super::{Change, Hook, Pending, Step, last};
use crate::module::Lock;

/// The door actions KOTOR's AI asks about, by `DOOR_ACTION_*` (the same
/// values as `PLACEABLE_ACTION_*`); knocking and ignoring are none here.
#[derive(Clone, Copy, PartialEq)]
enum DoorAction {
    Open,
    Unlock,
    Bash,
}

const DOOR_ACTIONS: [(&str, DoorAction); 3] = [
    ("DOOR_ACTION_OPEN", DoorAction::Open),
    ("DOOR_ACTION_UNLOCK", DoorAction::Unlock),
    ("DOOR_ACTION_BASH", DoorAction::Bash),
];

/// A lock with none of its fields set.
static NO_LOCK: Lock = Lock {
    lockable: false,
    open_dc: 0,
    close_dc: 0,
    key: String::new(),
    key_required: false,
    auto_remove_key: false,
};

impl Engine<'_> {
    /// `target`'s lock facts, and whether it is locked now; `None` for
    /// what is not a door or placeable.
    fn lock_of(&self, target: ObjectId) -> Option<(&Lock, bool)> {
        let locked = self.view.thing(target)?.locked?;
        let facts = self.view.facts(target)?;
        Some((facts.lock.as_ref().unwrap_or(&NO_LOCK), locked))
    }

    /// Whether `who` may unlock `target` with Security: a lock not only
    /// its key opens, which its total reaches taking
    /// `rules::interaction::checks::TAKEN`.
    fn may_unlock(&self, who: ObjectId, target: ObjectId) -> bool {
        let Some((lock, true)) = self.lock_of(target) else {
            return false;
        };
        let reaches = self
            .view
            .thing(who)
            .and_then(|thing| thing.character)
            .and_then(|c| crate::world::locks::security(self.library, &c.worn, &c.sheet));
        let taken = i16::from(rules::interaction::checks::TAKEN);
        !lock.key_required && reaches.is_some_and(|total| taken + total >= i16::from(lock.open_dc))
    }

    /// Whether `who` is a door or a placeable.
    fn situated(&self, who: ObjectId) -> bool {
        self.view.thing(who).is_some_and(|thing| {
            matches!(
                thing.kind,
                Some(crate::module::ObjectKind::Door | crate::module::ObjectKind::Placeable)
            )
        })
    }

    /// Whether blows may break `target` open (`module::bashable`).
    fn bashable(&self, target: ObjectId) -> bool {
        self.view
            .thing(target)
            .is_some_and(|thing| thing.bashable())
    }

    /// The door action a `DOOR_ACTION_*` value names.
    fn door_action(&self, value: i32) -> Option<DoorAction> {
        DOOR_ACTIONS
            .iter()
            .find(|(name, _)| self.library.constant(name) == Some(value))
            .map(|(_, action)| *action)
    }

    pub(super) fn lock_call(&mut self, bound: Bound, caller: ObjectId, args: &[Value]) -> Value {
        use Bound::*;
        let target = object(args, 0);
        let lock = self.lock_of(target).map(|(lock, _)| lock);
        match bound {
            GetLockKeyRequired => truth(lock.is_some_and(|lock| lock.key_required)),
            GetLockLockable => truth(lock.is_some_and(|lock| lock.lockable)),
            GetLockUnlockDC => Value::Int(lock.map_or(0, |lock| i32::from(lock.open_dc))),
            GetLockLockDC => Value::Int(lock.map_or(0, |lock| i32::from(lock.close_dc))),
            // A creature's actions: a door or placeable told to lock or
            // unlock does nothing (it has no hands; scripts use `SetLocked`
            // for that). Citadel Station's Exchange door is told to lock
            // itself as Slusk's scene begins (`a_luxaslusk`); held to, it
            // would shut the party in his office for good [verify in TSL's
            // executable].
            ActionUnlockObject | ActionLockObject if self.situated(caller) => Value::Int(0),
            ActionUnlockObject | ActionLockObject => self.queue(
                caller,
                Action::Lock {
                    object: target,
                    locked: bound == ActionLockObject,
                },
            ),
            // Opening a closed door that is not locked; unlocking a lock the
            // caller's Security reaches.
            GetIsDoorActionPossible => truth(match self.door_action(int(args, 1)) {
                Some(DoorAction::Open) => {
                    let open = self.view.thing(target).and_then(|thing| thing.open);
                    open == Some(0) && self.lock_of(target).is_some_and(|(_, locked)| !locked)
                }
                Some(DoorAction::Unlock) => self.may_unlock(caller, target),
                Some(DoorAction::Bash) => self.bashable(target),
                None => false,
            }),
            DoDoorAction => match self.door_action(int(args, 1)) {
                Some(DoorAction::Open) => self.queue(
                    caller,
                    Action::Door {
                        door: target,
                        open: true,
                    },
                ),
                Some(DoorAction::Unlock) if self.may_unlock(caller, target) => self.queue(
                    caller,
                    Action::Lock {
                        object: target,
                        locked: false,
                    },
                ),
                Some(DoorAction::Bash) if self.bashable(target) => {
                    self.queue(caller, Action::Bash { door: target })
                }
                // Knocking: not yet.
                _ => Value::Int(0),
            },
            _ => Value::Int(0),
        }
    }
}

impl Step<'_> {
    /// `who`'s use action done: a door opens (as `ActionOpenDoor`'s),
    /// anything else runs its `OnUsed` with `who` its last user.
    pub(super) fn use_object(&mut self, who: ObjectId, object: ObjectId) {
        let kind = self.view.thing(object).and_then(|thing| thing.kind);
        if kind == Some(crate::module::ObjectKind::Door) {
            self.out.changes.push(Change::Door {
                door: object,
                open: true,
                by: who,
            });
            return;
        }
        self.state.last.insert((object.0, last::USED), who.0);
        if let Some(script) = self.view.hook(object, Hook::Used) {
            self.state.pending.push(Pending::new(&script, object));
        }
    }

    /// `who`'s lock action done: `object` unlocked or locked, its hook run
    /// with `who` its last unlocker or locker.
    pub(super) fn lock(&mut self, who: ObjectId, object: ObjectId, locked: bool) {
        self.out.changes.push(Change::Lock {
            door: object,
            locked,
        });
        let (kind, hook) = if locked {
            (last::LOCKED, Hook::Lock)
        } else {
            (last::UNLOCKED, Hook::Unlock)
        };
        self.state.last.insert((object.0, kind), who.0);
        if let Some(script) = self.view.hook(object, hook) {
            self.state.pending.push(Pending::new(&script, object));
        }
    }
}
