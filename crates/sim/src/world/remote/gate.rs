//! The one gate every effect of a droid's kit goes through (the spec's
//! Rule 2): its bolt and its blast touch only a creature hostile to the
//! party's leader, alive and keeping the party in combat (`party::keepers`),
//! that no script protects; its dart holds off a member fighting one a
//! script protects, so a duel is won by its fighter alone.

use super::super::party::{self, Combatant, Rules};
use crate::combat::Relation;
use crate::combat::fighter::Life;
use crate::combat::sheet::flags;
use crate::snapshot::SimId;

/// What the gate says of a target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fair {
    Yes,
    /// Hostile, but a script keeps it: Min1HP, plot, immortal, or in a
    /// conversation or a scene. Scripted duels end so.
    Protected,
    /// In the party, neutral or friendly: the droid never starts a fight.
    NotHostile,
    /// Hostile but not fighting the party, or not alive.
    NotFighting,
}

/// A character as the gate reads it: as the in-combat test does, with its
/// sheet's flags and whether it takes part in a conversation.
#[derive(Debug, Clone, Copy)]
pub struct Target {
    pub who: Combatant,
    pub flags: u8,
    pub talks: bool,
}

impl Target {
    /// A script keeps it from harm, or in a round it must end.
    fn protected(&self) -> bool {
        self.who.plot
            || self.flags & (flags::PLOT | flags::MIN_ONE_HP | flags::IMMORTAL) != 0
            || self.talks
    }
}

/// The gate over everyone, as the step left them.
pub struct Gate<'a> {
    targets: Vec<Target>,
    keepers: Vec<SimId>,
    leader: Option<SimId>,
    rules: Rules<'a>,
}

impl<'a> Gate<'a> {
    /// The gate over `targets`, the party led by `leader` (`super::credit`),
    /// at sim time `now`.
    pub fn new(targets: Vec<Target>, leader: Option<SimId>, rules: Rules<'a>, now: u64) -> Self {
        let characters: Vec<Combatant> = targets.iter().map(|target| target.who).collect();
        let keepers = party::keepers(&characters, rules, now)
            .map(|keeper| keeper.id)
            .collect();
        Gate {
            targets,
            keepers,
            leader,
            rules,
        }
    }

    fn find(&self, id: SimId) -> Option<&Target> {
        self.targets.iter().find(|target| target.who.id == id)
    }

    /// Whether `who` is hostile to the leader (KOTOR's `GetIsEnemy`), or,
    /// with no leader, to any party member.
    fn hostile(&self, who: &Combatant) -> bool {
        let against = |member: &Combatant| {
            crate::module::relation(self.rules.repute, who.side(), member.side())
                == Relation::Hostile
        };
        match self.leader.and_then(|leader| self.find(leader)) {
            Some(leader) => against(&leader.who),
            None => (self.targets.iter())
                .filter(|target| target.who.party)
                .any(|member| against(&member.who)),
        }
    }

    /// Whether a droid's bolt or blast may touch `id`.
    pub fn fair(&self, id: SimId) -> Fair {
        let Some(target) = self.find(id) else {
            return Fair::NotHostile;
        };
        if target.who.party || !self.hostile(&target.who) {
            Fair::NotHostile
        } else if target.who.life != Life::Alive {
            Fair::NotFighting
        } else if target.protected() {
            Fair::Protected
        } else if self.keepers.contains(&id) {
            Fair::Yes
        } else {
            Fair::NotFighting
        }
    }

    /// Whether party member `member` fights a creature the gate protects
    /// (it targets the member, or the member it): a duel, its fighter's
    /// alone.
    pub fn dueling(&self, member: SimId) -> bool {
        let Some(own) = self.find(member) else {
            return false;
        };
        self.targets.iter().any(|foe| {
            (foe.who.target == Some(member) || own.who.target == Some(foe.who.id))
                && self.fair(foe.who.id) == Fair::Protected
        })
    }
}
