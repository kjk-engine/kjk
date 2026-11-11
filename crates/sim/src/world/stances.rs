//! The fighters' stance sets in the world: the set a fighter is made with,
//! the seats' picks (`Action::PickStance`), and what a change of the
//! session's rules does to every fighter's set (`SimWorld::restance`).
//!
//! The sets play only under MDA in a level that has them. There a fighter
//! is made with the set its row of the level's table gives it
//! (`StanceSets::row_for`). Under JA's rules every fighter has set 0, so
//! the sim is the same as without sets.

use bevy_ecs::prelude::*;
use bevy_ecs::query::Allow;

use super::{
    Avatar, Away, Benched, Fighting, Id, Inputs, LEADER_SEAT, Level, Module, Object, Rules,
    SimWorld, Static, in_party,
};
use crate::Instances;
use crate::action::Action;
use crate::combat::Fighter;
use crate::combat::saber::SaberRules;
use crate::skeleton::StanceSets;
use crate::snapshot::{Kind, Snapshot};
use crate::wire::{DecodeError, Reader, Writer};

/// A fighter as the sets' table reads it: its template, its tag, and
/// whether it is a seat's own character.
pub(crate) type Named<'a> = (&'a str, &'a str, bool);

/// A seat's own character, which has no template and no tag of its own.
pub(crate) const SEAT: Named<'static> = ("", "", true);

/// A fighter that is neither a seat's own character nor an object.
const NOBODY: Named<'static> = ("", "", false);

/// The stance sets the fighters play: the level's under MDA, and none
/// under JA's rules or in a level without sets.
fn playing(rules: SaberRules, level: &Level) -> Option<&StanceSets> {
    level
        .anims()
        .stances()
        .filter(|sets| rules.duel() && !sets.is_empty())
}

/// The first frame the stance sets lend to `level`'s skeleton, which is the
/// skeleton's count of its own frames. A level whose skeleton lends no
/// frames, or that has no skeleton, has no lent frame, and this is
/// `i32::MAX`.
fn first_lent(level: &Level) -> i32 {
    level
        .anims()
        .skeleton()
        .filter(|skeleton| skeleton.frame_count() > skeleton.own_frame_count())
        .map_or(i32::MAX, |skeleton| skeleton.own_frame_count() as i32)
}

/// The first frame past `level`'s skeleton's own frames, which is the
/// skeleton's count of its own frames, lent frames or not. A level without
/// a skeleton owns no frame, and this is 0. A save's fit drops every part
/// at or past it (`Snapshot::fit_stances`), because a save made with sets
/// may name lent frames that this level's skeleton lacks.
pub(crate) fn own_frames(level: &Level) -> i32 {
    level
        .anims()
        .skeleton()
        .map_or(0, |skeleton| skeleton.own_frame_count() as i32)
}

/// The stance sets a world under `rules` in `level` plays, each by its name
/// and its digest: the level's under MDA, and none under JA's rules.
/// A recorder without a world names the sets with it, from its start's
/// rules (`Recorder::new`).
pub fn played_ids(rules: SaberRules, level: &Level) -> Vec<(String, [u8; 32])> {
    playing(rules, level).map_or_else(Vec::new, StanceSets::ids)
}

/// Writes the names and digests of stance sets (`played_ids`), as a save
/// and a recording carry them: their count in a byte, then each name and
/// digest. A level holds at most `MAX_SETS` sets, which fits the byte.
pub(crate) fn write_ids(out: &mut Writer, ids: &[(String, [u8; 32])]) {
    out.u8(u8::try_from(ids.len()).expect("a level holds at most MAX_SETS stance sets"));
    for (name, digest) in ids {
        out.string(name);
        out.bytes(digest);
    }
}

/// Reads what `write_ids` wrote.
pub(crate) fn read_ids(input: &mut Reader) -> Result<Vec<(String, [u8; 32])>, DecodeError> {
    let count = input.u8()?;
    (0..count)
        .map(|_| Ok((input.string()?, input.array()?)))
        .collect()
}

/// Object `index` as the sets' table reads it. An object that play made
/// from a template the level lacks is named by that template.
fn object(instances: &Instances, index: u32) -> Named<'_> {
    let made = instances
        .made_from(index)
        .map(|made| made.template.as_str());
    let template = instances
        .get(index)
        .map(|facts| facts.template.as_str())
        .or(made)
        .unwrap_or("");
    (template, instances.tag(index).unwrap_or(""), false)
}

/// The set a fighter named `who` is made with under `rules` in `level`.
/// Under MDA in a level with sets it is the fighter's table row's set.
/// Otherwise it is 0.
pub(crate) fn made_with(rules: SaberRules, level: &Level, (template, tag, player): Named) -> u8 {
    playing(rules, level).map_or(0, |sets| sets.row_for(template, tag, player))
}

/// What `SimWorld::restance` does to one fighter. `sets` holds the sets
/// played and the fighter's name for their table, and is none when no
/// sets play. `lent` is the level's first lent frame (`first_lent`).
fn restance(f: &mut Fighter, sets: Option<(&StanceSets, Named)>, lent: i32) {
    match sets {
        Some((sets, (template, tag, player))) => {
            if f.stance_set == 0 || usize::from(f.stance_set) > sets.len() {
                f.stance_set = sets.row_for(template, tag, player);
            }
        }
        None => {
            f.stance_set = 0;
            f.stance_queued = None;
            f.pose.drop_played(lent);
        }
    }
}

/// `SimWorld::restance` for a snapshot that is about to become a world in
/// `level`, under the snapshot's rules. A module's install and the duel
/// trace's start use it, so the fighters they place take their rows' sets.
/// It runs only when the snapshot's rules are MDA. Under JA's rules it
/// leaves every fighter alone, because a switch to JA has already dropped
/// what a fighter held of a set.
pub(crate) fn restance_snapshot(snapshot: &mut Snapshot, level: &Level) {
    if !snapshot.saber_rules.duel() {
        return;
    }
    let sets = playing(snapshot.saber_rules, level);
    let instances = sets.map(|_| Instances::in_snapshot(level, snapshot));
    let lent = first_lent(level);
    for entity in &mut snapshot.entities {
        let named = match entity.kind {
            Kind::Avatar(_) => SEAT,
            Kind::Object(_, index) => instances
                .as_ref()
                .map_or(NOBODY, |instances| object(instances, index)),
            _ => NOBODY,
        };
        if let Some(f) = entity.fighter.as_mut() {
            restance(f, sets.map(|sets| (sets, named)), lent);
        }
    }
}

/// The name of a fighter in the world for the sets' table.
fn named<'a>(instances: &'a Instances, seat: bool, object_index: Option<u32>) -> Named<'a> {
    match (seat, object_index) {
        (true, _) => SEAT,
        (false, Some(index)) => object(instances, index),
        (false, None) => NOBODY,
    }
}

/// The seats' picks of a stance set (`Action::PickStance`). A pick is
/// taken only under MDA in a level with sets, and only for a set from 0 to
/// the number of sets. A seat picks for its own character, and the first
/// seat also for any party member. A taken pick is queued, and set 0
/// queues the fighter's table row's set. The saber step makes the queued
/// set the fighter's once its weapon timer has run out. Every other pick
/// is dropped without an effect.
#[allow(clippy::type_complexity)]
pub(super) fn picks(
    inputs: Res<Inputs>,
    rules: Res<Rules>,
    level: Res<Static>,
    module: Res<Module>,
    mut fighters: Query<(
        &Id,
        Option<&Avatar>,
        Option<&Benched>,
        Option<&Object>,
        &mut Fighting,
    )>,
) {
    let Some(sets) = playing(rules.0, &level.0) else {
        return;
    };
    for (seat, action) in inputs.0.actions() {
        let Action::PickStance { whom, set } = *action else {
            continue;
        };
        if usize::from(set) > sets.len() {
            continue;
        }
        let Some((_, avatar, benched, object, mut fighting)) =
            fighters.iter_mut().find(|(id, ..)| id.0 == whom)
        else {
            continue;
        };
        let own = avatar
            .map(|avatar| avatar.0)
            .or(benched.map(|benched| benched.0));
        let allowed = own == Some(*seat) || (*seat == LEADER_SEAT && in_party(avatar, &fighting.0));
        if !allowed {
            continue;
        }
        let picked = match set {
            0 => {
                let (template, tag, player) = named(
                    &module.instances,
                    own.is_some(),
                    object.map(|object| object.index),
                );
                sets.row_for(template, tag, player)
            }
            set => set,
        };
        fighting.0.stance_queued = Some(picked);
    }
}

impl SimWorld {
    /// Gives every fighter the stance set the session's rules allow.
    /// `set_saber_rules` runs it only when the rules were MDA before the
    /// change or are MDA after it, so a session under JA's rules from its
    /// start never reaches it. Under MDA in a level with sets, a fighter
    /// whose set is 0 or past the level's sets takes its table row's set.
    /// Otherwise every fighter's set and queued set become 0. Every part of its pose that plays a set's
    /// sequence is dropped, and so is every part whose playback or blend
    /// names a frame the sets lend to the skeleton. The next pose step sets
    /// a dropped part afresh.
    pub fn restance(&mut self) {
        let level = self.level().clone();
        let sets = playing(self.saber_rules(), &level);
        let lent = first_lent(&level);
        self.world.resource_scope(|world, module: Mut<Module>| {
            let mut fighters = world.query_filtered::<(
                Option<&Avatar>,
                Option<&Benched>,
                Option<&Object>,
                &mut Fighting,
            ), Allow<Away>>();
            for (avatar, benched, object, mut fighting) in fighters.iter_mut(world) {
                let seat = avatar.is_some() || benched.is_some();
                let who = named(&module.instances, seat, object.map(|object| object.index));
                restance(&mut fighting.0, sets.map(|sets| (sets, who)), lent);
            }
        });
    }

    /// The stance sets this world plays, each by its name and its digest:
    /// the level's under MDA, and none under JA's rules.
    pub fn stance_ids(&self) -> Vec<(String, [u8; 32])> {
        played_ids(self.saber_rules(), self.level())
    }

    /// The names of the stance sets the save named and this world's level
    /// lacks, as the world loaded from a save (`Save::world`). A world that
    /// did not load from a save, or lost no set, gives none.
    pub fn stances_gone(&self) -> &[String] {
        &self.world.resource::<Module>().stances_gone
    }

    pub(crate) fn stances_gone_with(&mut self, names: Vec<String>) {
        self.world.resource_mut::<Module>().stances_gone = names;
    }
}
