//! The events objects run scripts for (`Hook`), and the field each kind of
//! holder names its script under: a creature's UTC, a door's UTD, a
//! placeable's UTP, a trigger's UTT, an encounter's UTE, a store's UTM, the
//! area's ARE and the module's IFO. The labels are the ones both games'
//! files carry (`kjk show`), as reone's and xoreos's readers name them; a
//! holder whose files have no field for an event has no hook for it.

use crate::module::{ObjectData, ObjectKind};

/// An event an object runs its script for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hook {
    Spawn,
    Heartbeat,
    /// Perception (`ScriptOnNotice`).
    Notice,
    Attacked,
    Damaged,
    Death,
    /// An inventory taken from or added to.
    Disturbed,
    Blocked,
    EndRound,
    /// A conversation's or a shout's (`ScriptDialogue`).
    Dialogue,
    EndDialogue,
    SpellCastAt,
    UserDefined,
    Rested,
    /// A trigger's or encounter's entered, the area's, the module's player
    /// arriving.
    Enter,
    Exit,
    /// An encounter's last creature died or went (`OnExhausted`,
    /// `crate::spawner`).
    Exhausted,
    Click,
    Open,
    Closed,
    FailToOpen,
    Lock,
    Unlock,
    Disarm,
    TrapTriggered,
    Used,
    OpenStore,
    ModuleLoad,
    ModuleStart,
    AcquireItem,
    UnacquireItem,
    ActivateItem,
}

/// Who holds a hook: an object of the area, the area, or the module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Holder {
    Object(ObjectKind),
    Area,
    Module,
}

impl Hook {
    /// The field `holder` names its script for this event under, if its
    /// files have one.
    pub fn label(self, holder: Holder) -> Option<&'static str> {
        use Hook::*;
        use ObjectKind::{Creature, Door, Encounter, Placeable, Store, Trigger};
        let object = |kind| holder == Holder::Object(kind);
        let (area, module) = (holder == Holder::Area, holder == Holder::Module);
        let creature = object(Creature);
        let trigger = object(Trigger);
        let encounter = object(Encounter);
        let situated = object(Door) || object(Placeable);
        let label = match self {
            Spawn if creature => "ScriptSpawn",
            Heartbeat if creature || trigger => "ScriptHeartbeat",
            Heartbeat if situated || encounter || area => "OnHeartbeat",
            Heartbeat if module => "Mod_OnHeartbeat",
            Notice if creature => "ScriptOnNotice",
            Attacked if creature => "ScriptAttacked",
            Attacked if situated => "OnMeleeAttacked",
            Damaged if creature => "ScriptDamaged",
            Damaged if situated => "OnDamaged",
            Death if creature => "ScriptDeath",
            Death if situated => "OnDeath",
            Disturbed if creature => "ScriptDisturbed",
            Disturbed if object(Placeable) => "OnInvDisturbed",
            Blocked if creature => "ScriptOnBlocked",
            EndRound if creature => "ScriptEndRound",
            Dialogue if creature => "ScriptDialogue",
            EndDialogue if creature => "ScriptEndDialogu",
            EndDialogue if object(Placeable) => "OnEndDialogue",
            SpellCastAt if creature => "ScriptSpellAt",
            SpellCastAt if situated => "OnSpellCastAt",
            UserDefined if creature || trigger => "ScriptUserDefine",
            UserDefined if situated || encounter || area => "OnUserDefined",
            UserDefined if module => "Mod_OnUsrDefined",
            Rested if creature => "ScriptRested",
            Enter if trigger => "ScriptOnEnter",
            Enter if encounter => "OnEntered",
            Enter if area => "OnEnter",
            Enter if module => "Mod_OnClientEntr",
            Exit if trigger => "ScriptOnExit",
            Exit if encounter || area => "OnExit",
            Exit if module => "Mod_OnClientLeav",
            Exhausted if encounter => "OnExhausted",
            Click if object(Door) || trigger => "OnClick",
            Open if situated => "OnOpen",
            Closed if situated => "OnClosed",
            FailToOpen if situated => "OnFailToOpen",
            Lock if situated => "OnLock",
            Unlock if situated => "OnUnlock",
            Disarm if situated || trigger => "OnDisarm",
            TrapTriggered if situated || trigger => "OnTrapTriggered",
            Used if object(Placeable) => "OnUsed",
            OpenStore if object(Store) => "OnOpenStore",
            ModuleLoad if module => "Mod_OnModLoad",
            ModuleStart if module => "Mod_OnModStart",
            AcquireItem if module => "Mod_OnAcquirItem",
            UnacquireItem if module => "Mod_OnUnAqreItem",
            ActivateItem if module => "Mod_OnActvtItem",
            _ => return None,
        };
        Some(label)
    }

    /// The script `scripts` (hook label, script) names for this event, as
    /// `holder` labels it.
    pub fn in_scripts(self, holder: Holder, scripts: &[(String, String)]) -> Option<&str> {
        let label = self.label(holder)?;
        scripts
            .iter()
            .find(|(hook, _)| hook == label)
            .map(|(_, script)| script.as_str())
    }

    /// The script an object's facts name for this event.
    pub fn of(self, facts: &ObjectData) -> Option<&str> {
        self.in_scripts(Holder::Object(facts.kind), &facts.scripts)
    }
}

/// Whether a player's use of an object is answered by talk: a creature or
/// placeable with a conversation, or with its script for being talked to
/// (a creature's dialogue hook, a placeable's use).
pub fn speaks(facts: &ObjectData) -> bool {
    let hook = match facts.kind {
        ObjectKind::Creature => Hook::Dialogue,
        ObjectKind::Placeable => Hook::Used,
        _ => return false,
    };
    !facts.conversation.is_empty() || hook.of(facts).is_some()
}
