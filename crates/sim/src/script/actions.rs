//! Objects' action queues (I17): what `Action…` routines append to the
//! calling object's queue, and how a step moves each queue on
//! (`advance`).
//!
//! The front action runs; when it completes the next starts, the same
//! step, up to `ACTIONS_PER_STEP`:
//! - **code** (`ActionDoCommand`): the deferred code runs for the object,
//!   and its turn ends for the step (KOTOR starts one action a frame: code
//!   that queues itself again loops once a step);
//! - **wait** (`ActionWait`): until it is up;
//! - **animation** (`ActionPlayAnimation`): a wait for its time, the object
//!   playing it from its start (`Change::Animate`; a clear stops it): one
//!   play of its clip at its speed, or a looping one's duration
//!   (`engine::animation_ms`); one looping on (a duration under 0) plays at
//!   once instead, never queued;
//! - **move** (`ActionMoveToObject`, `…ToLocation`, the forced ones): done
//!   within its range (2D); until then the object's route goal
//!   (`Goal`, which its mind walks, `world::minds`); a forced move that
//!   runs out of time jumps there;
//! - **jump** (`ActionJumpTo…`): the object is put there;
//! - **door** (`ActionOpenDoor`, `ActionCloseDoor`): walks to within
//!   `DOOR_REACH` of the door, then opens or closes it;
//! - **use** (`ActionUseObject`, `ActionInteractObject`): walks to
//!   within `DOOR_REACH` of the object, then uses it: a door opens (as
//!   `ActionOpenDoor`), anything else runs its `OnUsed` with the object
//!   using it as its last user;
//! - **lock** (`ActionUnlockObject`, `ActionLockObject`): walks to within
//!   `DOOR_REACH` of the door or placeable, then unlocks or locks it, with
//!   no check (the scripts decide), its `OnUnlock` or `OnLock` running;
//! - **attack** (`ActionAttack`; I22 C14): an intent on a target to the
//!   end of the object's combat round (`State::rounds`; one `ROUND_MS`
//!   before its first), done early when the target dies or is gone; meanwhile the object's mind fights that target (`Intent`,
//!   `world::minds`), a passive one facing it without swinging; a blade
//!   or fists with the target in reach are in combat (`Change::InCombat`),
//!   as after a blow (a gun is, by its shots), so its rounds run whether
//!   or not blows land;
//! - **talent** (`ActionUseTalentOnObject`, `…AtLocation`; C14): a power
//!   or feat used on a target, to the end of the object's round as an
//!   attack: its intent's `talent`, which the mind uses once within the
//!   power's reach (a power pressed or held, a ranged feat's special, a
//!   melee feat's saber style);
//! - **random walk** (`ActionRandomWalk`): to random points within
//!   `WANDER_M` of where the walker stood as the walk began, one after
//!   another, until the queue is cleared;
//! - **converse** (`ActionStartConversation`, after its move): asks for
//!   the conversation (`script::dialogue`) and completes;
//! - **hold** (`ActionResumeConversation`): the conversation its object
//!   is in goes on, and completes (`ActionPauseConversation` holds at
//!   once and queues nothing);
//! - **switch weapons** (TSL's `ActionSwitchWeapons`): the object's whole
//!   queue cleared (this one with it), its weapon sets exchanged at once
//!   (`items::State::switch_weapons`), and, in combat with a target, an
//!   attack on that target queued again (the executable's
//!   `AIActionSwitchWeapons`: no animation, done the update it starts);
//! - **cast** (`ActionCastSpellAtObject`, `…AtLocation`, the fake ones):
//!   turned to its target, the hand out, for the row's conjure and cast
//!   time; then a real one is paid and runs its impact script, a fake one
//!   nothing (`Step::cast`);
//! - **surrender** (`ActionSurrenderToEnemies`): `SurrenderToEnemies`, and
//!   completes;
//! - **bark** (`ActionBarkString`): the object barks the line as
//!   `BarkString` does, and completes;
//! - **follow owner** (TSL's `ActionFollowOwner`, a puppet's): while its
//!   owner (`GetPUPOwner`) stands within its range (2D), or it has none, it
//!   holds the queue; farther, a run to where the owner stands, within the
//!   range, and the follow again are queued behind what waits, and it
//!   completes (the executable's `AIActionFollowOwner`: it never ends but
//!   by a clear); one that is no puppet in the party, or dead, fails;
//! - **instant**: a routine queued as an action the sim cannot do yet (a
//!   gap), which completes at once.
//!
//! A player's queue moves on too, but its moves are dropped: a player's
//! legs are its seat's (I22 C14), except while it is held in a
//! conversation (a cinematic's), when a script walks it as a creature
//! (`world::Driven`).

use script_ncs::vm::{Deferred, ObjectId};

/// Where an action goes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Target {
    Object(ObjectId),
    /// Metres, game space; a facing (degrees) for a jump.
    Point([f32; 3], f32),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Code {
        script: String,
        deferred: Deferred,
    },
    Wait {
        ms: u32,
    },
    /// `ActionPlayAnimation`: a wait of `ms` while the object plays
    /// `animation` (an `ANIMATION_…` constant), which views draw.
    Animate {
        animation: u16,
        ms: u32,
    },
    Move {
        to: Target,
        run: bool,
        /// Metres.
        range: f32,
        /// A forced move's time before it jumps (ms).
        force_ms: Option<u32>,
    },
    Jump {
        to: Target,
    },
    Door {
        door: ObjectId,
        open: bool,
    },
    Lock {
        object: ObjectId,
        locked: bool,
    },
    /// `ActionUseObject`, `ActionInteractObject`.
    UseObject {
        object: ObjectId,
    },
    /// `DoDoorAction(DOOR_ACTION_BASH)`: walk to the door and strike it a
    /// blow a round with what is in hand until it breaks (`world::objects`).
    Bash {
        door: ObjectId,
    },
    Attack {
        target: ObjectId,
        passive: bool,
    },
    Talent {
        talent: Use,
        target: ObjectId,
    },
    /// `ActionRandomWalk`: where the walker stood as it began, and where
    /// it walks now (metres), picked afresh when reached or after
    /// `WANDER_MS`; it never completes.
    RandomWalk {
        from: Option<[f32; 3]>,
        to: Option<[f32; 3]>,
    },
    /// `ActionStartConversation`: with whom, the DLG (empty: the owner's
    /// own), and the body whose use queued it (`Event::asker`).
    Converse {
        with: ObjectId,
        dialog: String,
        asker: Option<ObjectId>,
    },
    /// `ActionResumeConversation`, and the conversation whose script queued
    /// it (`dialogue::running`).
    Hold {
        talk: Option<u32>,
    },
    /// `ActionSwitchWeapons`.
    SwitchWeapons,
    Instant {
        routine: String,
    },
    /// `ActionCastSpellAtObject`, `…AtLocation` and their fake forms: a
    /// `spells.2da` row cast at a target; a fake one only looks cast, a
    /// cheat's needs no knowing the power nor paying for it.
    Cast {
        row: u16,
        target: Target,
        fake: bool,
        cheat: bool,
    },
    /// `ActionSurrenderToEnemies`: `SurrenderToEnemies` when it is reached.
    Surrender,
    /// `ActionBarkString`: the object barks a talk-table line, as
    /// `BarkString` would, when its turn comes.
    Bark {
        strref: u32,
    },
    /// TSL's `ActionFollowOwner`: a puppet keeps within `range` (metres) of
    /// its owner.
    FollowOwner {
        range: f32,
    },
}

impl Action {
    /// Its kind, for reports.
    pub fn kind(&self) -> &'static str {
        match self {
            Action::Code { .. } => "code",
            Action::Wait { .. } => "wait",
            Action::Animate { .. } => "animation",
            Action::Move { .. } => "move",
            Action::Jump { .. } => "jump",
            Action::Door { .. } => "door",
            Action::Lock { .. } => "lock",
            Action::UseObject { .. } => "use",
            Action::Bash { .. } => "bash",
            Action::Attack { .. } => "attack",
            Action::Talent { .. } => "talent",
            Action::RandomWalk { .. } => "random walk",
            Action::Converse { .. } => "converse",
            Action::Hold { .. } => "hold",
            Action::SwitchWeapons => "switch weapons",
            Action::Instant { .. } => "instant",
            Action::Cast { .. } => "cast",
            Action::Surrender => "surrender",
            Action::Bark { .. } => "bark",
            Action::FollowOwner { .. } => "follow owner",
        }
    }

    /// `GetCurrentAction`'s name for it (the catalog's `ACTION_…`).
    pub fn constant(&self) -> &'static str {
        match self {
            Action::Move { .. } => "ACTION_MOVETOPOINT",
            Action::Wait { .. } | Action::Animate { .. } => "ACTION_WAIT",
            Action::Door { open: true, .. } => "ACTION_OPENDOOR",
            Action::Door { open: false, .. } => "ACTION_CLOSEDOOR",
            Action::Lock { locked: false, .. } => "ACTION_OPENLOCK",
            Action::Lock { locked: true, .. } => "ACTION_LOCK",
            Action::UseObject { .. } => "ACTION_USEOBJECT",
            Action::Attack { .. } | Action::Bash { .. } => "ACTION_ATTACKOBJECT",
            Action::Talent {
                talent: Use::Power(_),
                ..
            }
            | Action::Cast { .. } => "ACTION_CASTSPELL",
            Action::Talent { .. } => "ACTION_USETALENTONOBJECT",
            Action::RandomWalk { .. } => "ACTION_RANDOMWALK",
            Action::Converse { .. } => "ACTION_DIALOGOBJECT",
            Action::FollowOwner { .. } => "ACTION_FOLLOWOWNER",
            Action::Jump { .. }
            | Action::Code { .. }
            | Action::Hold { .. }
            | Action::SwitchWeapons
            | Action::Instant { .. }
            | Action::Surrender
            | Action::Bark { .. } => "ACTION_INVALID",
        }
    }
}

/// An action in a queue, and when it started (sim ms).
#[derive(Debug, Clone, PartialEq)]
pub struct Queued {
    pub action: Action,
    pub started_ms: Option<u64>,
}

/// Where a moving object's mind should route it this step (units).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Goal {
    pub point: [f32; 3],
    pub range: f32,
    pub run: bool,
}

/// A talent used (`Action::Talent`): a `spells.2da` row or a `feat.2da`
/// row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Use {
    Power(u16),
    Feat(u16),
}

/// Whom a creature's mind fights for its script (`Action::Attack`), and
/// the talent it uses on it now (`Action::Talent`), if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Intent {
    pub target: ObjectId,
    pub passive: bool,
    pub talent: Option<Use>,
}

/// KOTOR's combat round: an attack intent's length and the end-of-round
/// event's period.
pub use crate::combat::ROUND_MS;

/// A random walk's next point: within this far (metres), picked again
/// after this long (ms) if not reached.
pub const WANDER_M: f32 = rules::scripts::wander::REACH_M;
pub const WANDER_MS: u64 = rules::scripts::wander::REPICK_MS;

/// How near a door an object opens or closes it (metres).
pub const DOOR_REACH: f32 = rules::scripts::moves::DOOR_REACH;
/// The most actions one object's queue completes in a step.
pub const ACTIONS_PER_STEP: usize = 16;
