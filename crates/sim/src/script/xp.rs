//! Experience (I19, `docs/WORLD-STATE.md`). The party's is KOTOR's pool:
//! what one member gains every player and every available companion has
//! (`inventory::PARTY`'s); a creature outside the party keeps its own. Kept
//! in the scripts' state (a save carries it).
//!
//! Scripts give and set it (`GiveXPToCreature`, `SetXP`, `GivePlotXP`).
//! The story gives plot XP (`plot.2da`): a journal entry's share of its
//! quest's as the quest reaches it, a conversation node's share of its
//! plot's as it plays. A death gives the party what the creature's
//! challenge rating is worth at the party's level (`xptable.2da`): the
//! engine's, no script gives it.

use script_ncs::vm::{ObjectId, Value};

use super::engine::{Bound, Engine, int, object, text};
use super::state::State;
use super::{Library, Out, View, items};
use crate::inventory::PARTY;
use crate::notice::Notice;
use crate::snapshot::StatusKind;
use crate::wire::Writer;

/// The XP tables: `plot.2da` (label, XP) by row, and `xptable.2da`'s XP a
/// death gives by the party's level (row) and the challenge rating
/// (column `c0`, `c1`, …).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Experience {
    pub plots: Vec<(String, i32)>,
    pub kills: Vec<Vec<i32>>,
}

impl Experience {
    /// Plot row `row`'s XP.
    pub fn plot(&self, row: i32) -> Option<i32> {
        let row = usize::try_from(row).ok()?;
        self.plots.get(row).map(|(_, xp)| *xp)
    }

    /// The plot labelled `label`'s XP (any case).
    pub fn plot_named(&self, label: &str) -> Option<i32> {
        self.plots
            .iter()
            .find(|(of, _)| of.eq_ignore_ascii_case(label))
            .map(|(_, xp)| *xp)
    }

    /// What a death of challenge rating `challenge` gives a party of level
    /// `level`: the rows end at the table's last level, the columns at its
    /// highest rating.
    pub fn kill(&self, level: u8, challenge: f32) -> i32 {
        let row = usize::from(level.max(1) - 1).min(self.kills.len().saturating_sub(1));
        let Some(row) = self.kills.get(row) else {
            return 0;
        };
        let column = (challenge.max(0.0) as usize).min(row.len().saturating_sub(1));
        row.get(column).copied().unwrap_or(0)
    }

    pub(crate) fn digest_into(&self, out: &mut Writer) {
        out.u32(self.plots.len() as u32);
        for (label, xp) in &self.plots {
            out.string(label);
            out.u32(*xp as u32);
        }
        out.u32(self.kills.len() as u32);
        for row in &self.kills {
            out.u32(row.len() as u32);
            for xp in row {
                out.u32(*xp as u32);
            }
        }
    }
}

impl Library {
    /// The XP quest `tag` reaching entry `entry` gives: the entry's share
    /// of its quest's plot XP.
    pub fn quest_xp(&self, tag: &str, entry: i32) -> i32 {
        let Some(quest) = self.quests().get(tag) else {
            return 0;
        };
        let share = u32::try_from(entry)
            .ok()
            .and_then(|entry| quest.entry(entry))
            .map_or(0.0, |entry| entry.xp);
        let xp = self.experience().plot(quest.plot).unwrap_or(0);
        (xp as f32 * share) as i32
    }
}

/// Whose XP `who`'s is: the party's for a player, a companion in the
/// party or one available to it; else its own.
pub fn holder(view: &View, state: &State, who: ObjectId) -> u32 {
    let available = state
        .party
        .available
        .values()
        .any(|npc| npc.object == Some(who.0));
    if available {
        PARTY
    } else {
        items::holder(view, who)
    }
}

/// The global TSL's scripts read the player's level from.
pub const PC_LEVEL: &str = "G_PC_LEVEL";

/// XP the party gains is the player's character's too (TSL's
/// `CSWSCreature::ReceiveExperience`, for its player character): it tells
/// the campaign's scripts the level that XP reaches, its own and each after
/// it the XP is enough for (`CSWSCreatureStats::GetPotentialLevel`), in
/// `G_PC_LEVEL` where the global catalogue declares it (K1's does not; a
/// module with none, any).
/// Read by the quest rewards and random loot (`a_give_quest_ls`,
/// `k_inc_treas_k2`).
pub(crate) fn tell_pc_level(library: &Library, view: &View, state: &mut State) {
    let declared = super::engine::declared(view.module, PC_LEVEL, crate::module::Global::Number);
    let level = view
        .party
        .first()
        .and_then(|pc| view.thing(*pc))
        .and_then(|thing| thing.character)
        .map(|character| character.worn.level);
    if let (true, Some(level)) = (declared, level) {
        let reached = library.classes().potential_level(level, state.xp(PARTY));
        super::state::set_global(&mut state.numbers, PC_LEVEL, i32::from(reached));
    }
}

impl State {
    /// `holder`'s XP.
    pub fn xp(&self, holder: u32) -> i32 {
        self.xp.get(&holder).copied().unwrap_or(0)
    }

    /// `holder` gains `amount` (loses, if negative; never below 0); what
    /// it gained.
    pub(crate) fn give_xp(&mut self, holder: u32, amount: i32) -> i32 {
        let was = self.xp(holder);
        let now = was.saturating_add(amount).max(0);
        self.xp.insert(holder, now);
        now - was
    }

    /// The party gains `amount`, every player told.
    pub(crate) fn reward(&mut self, amount: i32, notices: &mut Vec<(Option<ObjectId>, Notice)>) {
        let gained = self.give_xp(PARTY, amount);
        if gained != 0 {
            notices.push((None, Notice::Experience(gained)));
        }
    }
}

impl Out {
    /// The party gains `amount` (`State::reward`).
    pub(super) fn reward(&mut self, state: &mut State, amount: i32) {
        state.reward(amount, &mut self.notices);
    }

    /// The party gains `amount` from the story (a quest's, a plot's, a
    /// script's gift to a player's character), which KOTOR's status
    /// summary tells; a kill's or a trap's it does not.
    pub(super) fn story_xp(&mut self, state: &mut State, amount: i32) {
        let was = state.xp(PARTY);
        self.reward(state, amount);
        let gained = state.xp(PARTY) - was;
        if gained != 0 {
            self.statuses.push(StatusKind::Xp(gained));
        }
    }
}

impl Engine<'_> {
    /// The XP routines, and `GetJournalQuestExperience`.
    pub(super) fn xp_call(&mut self, bound: Bound, args: &[Value]) -> Value {
        let experience = self.library.experience();
        match bound {
            Bound::GiveXPToCreature | Bound::SetXP => {
                let holder = holder(self.view, self.state, object(args, 0));
                let amount = int(args, 1);
                let amount = match bound {
                    Bound::SetXP => amount.max(0) - self.state.xp(holder),
                    _ => amount,
                };
                if holder == PARTY && bound == Bound::GiveXPToCreature {
                    // Told only when it is given a player's character.
                    if self.is_pc(object(args, 0)) {
                        self.out.story_xp(self.state, amount);
                    } else {
                        self.out.reward(self.state, amount);
                    }
                    tell_pc_level(self.library, self.view, self.state);
                } else {
                    self.state.give_xp(holder, amount);
                }
                Value::Int(0)
            }
            Bound::GetXP => {
                let holder = holder(self.view, self.state, object(args, 0));
                Value::Int(self.state.xp(holder))
            }
            Bound::GivePlotXP => {
                let xp = experience.plot_named(text(args, 0)).unwrap_or(0);
                let share = i64::from(xp) * i64::from(int(args, 1)) / 100;
                let share = share.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
                self.out.story_xp(self.state, share);
                tell_pc_level(self.library, self.view, self.state);
                Value::Int(0)
            }
            _ => {
                let plot = self.library.quests().get(text(args, 0)).map(|q| q.plot);
                Value::Int(plot.and_then(|row| experience.plot(row)).unwrap_or(0))
            }
        }
    }
}
