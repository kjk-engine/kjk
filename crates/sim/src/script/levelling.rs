//! Levels taken in play (I19, `sim::levels`). The party's XP (its pool,
//! `script::xp`) decides when a member may take another level
//! (`exptable.2da`). A companion no seat drives takes every level it may at
//! once, with the game's picks (KOTOR's automatic level up, the owner's
//! choice); a player, and a companion a guest drives, takes one when it
//! asks, with its own picks (`State::levelling`, the
//! level-up screen's; the feats and powers it leaves unpicked the game's),
//! checked. A level taken is a new sheet riding in the
//! snapshot (`Change::Sheet`) and the character's career kept in the
//! scripts' state (`State::careers`).

use script_ncs::vm::ObjectId;

use super::state::State;
use super::{Change, Step, Thing, View};
use crate::combat::Sheet;
use crate::inventory::PARTY;
use crate::levels::{Career, Choices};
use crate::module::ObjectKind;

/// `who`'s career: the levels it took here, else its template's classes;
/// a player without one has its sheet's level in `class` (the class its
/// level-up asks for: character creation's, which the sim does not hold
/// yet [verify]).
pub fn career(view: &View, state: &State, who: ObjectId, sheet: &Sheet, class: u8) -> Career {
    if let Some(career) = state.careers.get(&who.0) {
        return career.clone();
    }
    let template = view
        .facts(who)
        .map(|facts| facts.classes.clone())
        .filter(|classes| !classes.is_empty());
    template.unwrap_or_else(|| vec![(class, sheet.level)])
}

/// A companion the game levels itself (KOTOR's automatic level up, the
/// owner's choice): a creature in the party no seat drives. One a guest
/// drives waits for its guest's `Action::LevelUp`, as a player does (I30
/// P15); the step its guest drops (its driver cleared), it takes the
/// levels it waited for, with the game's picks.
fn levels_itself(thing: &Thing) -> bool {
    thing.kind == Some(ObjectKind::Creature)
        && thing
            .character
            .is_some_and(|character| character.party && character.driver.is_none())
}

/// Whether a level is asked for, or a companion that levels itself may
/// take one now.
pub fn due(library: &super::Library, state: &State, view: &View) -> bool {
    let xp = state.xp(PARTY);
    let classes = library.classes();
    !state.levelling.is_empty()
        || view
            .things
            .iter()
            .filter(|thing| levels_itself(thing))
            .any(|thing| {
                let Some(c) = thing.character else {
                    return false;
                };
                let career = career(view, state, thing.id, &c.worn, 0);
                classes.may_level(c.worn.level, xp)
                    && career
                        .first()
                        .is_some_and(|(class, _)| classes.offer(&c.worn, &career, *class).is_some())
            })
}

impl super::engine::Engine<'_> {
    /// `AddMultiClass`: `who` takes up `class` (a player becoming a Jedi on
    /// Dantooine, TSL's Exile a prestige class) with no levels in it yet
    /// (`CSWSCreatureStats::AddClass`). A player's party is given the XP
    /// its next level asks (both games' `ExecuteCommandAddMultiClass`:
    /// `DistributeExperience` of `GetExpNeededForLevelUp` less its XP), so
    /// its next level-up is of the new class. K1's does that for any
    /// creature; TSL's gives one outside the party that XP itself and levels
    /// it at once with the game's picks (`AutoLevelup`).
    pub(super) fn multiclass(&mut self, who: ObjectId, class: Option<u8>) {
        let classes = self.library.classes();
        let Some((class, character)) = class.zip(self.view.thing(who).and_then(|t| t.character))
        else {
            return;
        };
        let sheet = self.out.sheet(who, character.worn);
        let mut career = career(self.view, self.state, who, &sheet, 0);
        if career.iter().any(|(had, _)| *had == class) {
            return;
        }
        career.push((class, 0));
        let in_party = self.view.party.contains(&who);
        let tsl = crate::combat::passive::Game::named(&self.view.module.game)
            == crate::combat::passive::Game::Tsl;
        if in_party || !tsl {
            self.report
                .line(format!("#{} takes up class {class} ({career:?})", who.0));
            self.state.careers.insert(who.0, career);
            let holder = super::xp::holder(self.view, self.state, who);
            let needed = classes.next(sheet.level).unwrap_or(0) - self.state.xp(holder);
            if needed > 0 {
                self.out.reward(self.state, needed);
                super::xp::tell_pc_level(self.library, self.view, self.state);
            }
            return;
        }
        let holder = super::xp::holder(self.view, self.state, who);
        let needed = classes.next(sheet.level).unwrap_or(0) - self.state.xp(holder);
        self.state.give_xp(holder, needed.max(0));
        let banked = banked(self.state, who);
        if let Some(choices) = classes.auto(&sheet, &career, class, banked)
            && let Some((after, career, unspent)) = classes.take(&sheet, &career, &choices, banked)
        {
            self.report.line(format!(
                "#{} takes up class {class}: level {} ({career:?})",
                who.0, after.level
            ));
            levelled(&mut self.out.changes, who, sheet, after);
            self.state.careers.insert(who.0, career);
            bank(self.state, who, unspent);
        }
    }
}

impl Step<'_> {
    /// The levels asked for, then the companions'.
    pub(super) fn levels(&mut self) {
        let classes = self.library.classes();
        let asked = std::mem::take(&mut self.state.levelling);
        if classes.classes.is_empty() {
            return;
        }
        let xp = self.state.xp(PARTY);
        for (who, choices) in asked {
            let who = ObjectId(who);
            let Some((sheet, career)) = self.now(who, choices.class) else {
                continue;
            };
            let choices = classes.complete(&sheet, &career, &choices);
            let banked = banked(self.state, who);
            let taken = classes
                .may_level(sheet.level, xp)
                .then(|| classes.take(&sheet, &career, &choices, banked))
                .flatten();
            match taken {
                Some(taken) => self.took(who, sheet, taken, &choices),
                None => {
                    self.report.line(format!("level refused for #{}", who.0));
                    let refused = crate::notice::Notice::LevelRefused;
                    self.out.notices.push((Some(who), refused));
                }
            }
        }
        let companions: Vec<ObjectId> = self
            .view
            .things
            .iter()
            .filter(|thing| levels_itself(thing))
            .map(|thing| thing.id)
            .collect();
        for who in companions {
            while let Some((sheet, career)) = self.now(who, 0)
                && classes.may_level(sheet.level, xp)
                && let Some(&(class, _)) = career.first()
                && let Some(choices) = classes.auto(&sheet, &career, class, banked(self.state, who))
                && let Some(taken) =
                    classes.take(&sheet, &career, &choices, banked(self.state, who))
            {
                self.took(who, sheet, taken, &choices);
            }
        }
    }

    /// `who`'s sheet as this step leaves it so far, and its career (`class`
    /// for a player's without one).
    fn now(&self, who: ObjectId, class: u8) -> Option<(Sheet, Career)> {
        let character = self.view.thing(who)?.character?;
        let sheet = self.out.sheet(who, character.worn);
        let career = career(self.view, self.state, who, &sheet, class);
        Some((sheet, career))
    }

    fn took(
        &mut self,
        who: ObjectId,
        before: Sheet,
        (after, career, unspent): (Sheet, Career, u8),
        choices: &Choices,
    ) {
        self.report.line(format!(
            "#{} takes level {} (class {}: {:?})",
            who.0, after.level, choices.class, career
        ));
        levelled(&mut self.out.changes, who, before, after);
        self.state.careers.insert(who.0, career);
        bank(self.state, who, unspent);
    }
}

/// The skill points `who` left unspent at its levels before.
pub fn banked(state: &State, who: ObjectId) -> u8 {
    state.unspent.get(&who.0).copied().unwrap_or(0)
}

/// `who` leaves `points` unspent for its next level.
fn bank(state: &mut State, who: ObjectId, points: u8) {
    if points > 0 {
        state.unspent.insert(who.0, points);
    } else {
        state.unspent.remove(&who.0);
    }
}

/// A level taken by `who`: its new sheet, and its hit points whole at their
/// new most, its Force points up by what their most rose (both games'
/// `CSWSCreatureStats::LevelUp`, whoever levels: a player's level-up, a
/// companion's automatic one, `AddMultiClass`).
fn levelled(changes: &mut Vec<Change>, who: ObjectId, before: Sheet, after: Sheet) {
    changes.push(Change::Sheet { who, sheet: after });
    changes.push(Change::Levelled { who, before, after });
}
