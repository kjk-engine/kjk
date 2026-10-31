//! Locks (Security): doors and placeables locked as placed
//! (`ObjectData::opens`) or as a script left them (`ObjectState::lock`),
//! their keys and DCs (`ObjectData::lock`).
//!
//! On the authority, after the uses and the containers, each step
//! (`locks`): a locked object a player used (`interact`'s failed opening)
//! opens with its key, when the party carries an item tagged as the key:
//! it is unlocked, a door opens, a container opens for that seat; the key
//! is used up when the lock says so. Without the key the opening fails
//! (its `OnFailToOpen`): the player is told it is locked, and every
//! player hears its locked sound and sees "Locked" over it (K1's feedback
//! for a locked object). A door tells it once its `OnFailToOpen` has run
//! (`refused`), and only if it is still locked then and has no opening of
//! its own queued, as K1's exe reads the lock again after the script: the
//! Endar Spire's doors unlock and open themselves for Trask, saying
//! nothing. A K1 container with a use script (`OnUsed`, which its every
//! use runs: `world::uses::answers_lock`) is answered by that instead,
//! saying nothing of the lock itself: Taris's sealed strongbox puts its
//! password questions [verify in the game: no "Locked" over it].
//!
//! Security (`Action::Unlock`) is a skill's use in the scripts' step
//! (`world::traps::skill_uses`, which breaks one off when its user moves,
//! attacks or falls): `begin` refuses a lock not within reach or not
//! locked, one only its key opens ("This object cannot be opened through
//! conventional means."), a character with no ranks in Security (the
//! game's description: "Characters with no points invested cannot attempt
//! to use this skill") and a spike the party does not carry; else the user
//! turns to the lock, plays K1's unlock animation for `UNLOCK_MS` and says
//! its sound set's line for it ("On it."). Then `unlock`: the check's die,
//! the total (`security`) and a spike's bonus against the lock's
//! `OpenLockDC`, taking `rules::interaction::checks::TAKEN` out of combat,
//! and the line for how it went ("It's open.",
//! "Damn!"); a spike is used up only when the try fails (owner,
//! 2026-10-09: as K1 plays).
//! Unlocked, a door opens; a container stays shut until it is used.
//!
//! What unlocked an object is the scripts' `OnUnlock` (`GetLastUnlocked`);
//! a door opening is its `OnOpen`.

use bevy_ecs::prelude::*;
use fmt_odyssey::ssf::slots;
use script_ncs::vm::ObjectId;

use super::{Avatar, Clock, Id, Object, Player, Static, containers, scripts};
use crate::combat::Sheet;
use crate::combat::sheet::skills;
use crate::input::Seat;
use crate::inventory::{Catalog, PARTY};
use crate::math;
use crate::module::{ObjectData, ObjectKind};
use crate::notice::{Notice, Notices, Sound};
use crate::rng::SimRng;
use crate::script::actions::Action;
use crate::script::{Change, Library, Out, SkillUse, State, Thing, UseKind, View, last};
use crate::snapshot::{ObjectState, SimId, lock};

/// How long Security takes (ms).
pub const UNLOCK_MS: u64 = rules::interaction::security::UNLOCK_MS;

/// K1's own animations for Security (its open-lock action's; no script
/// constant names them): on a door, drawn as `unlockdr`; on a placeable,
/// `unlockcntr`.
pub mod animations {
    pub const DOOR: u16 = 10128;
    pub const PLACEABLE: u16 = 10131;
}

/// The talk table's feedback lines a lock gives (the same strrefs in K1
/// and TSL).
pub mod lines {
    /// "That object is not locked."
    pub const NOT_LOCKED: u32 = 1430;
    /// "This object cannot be opened through conventional means."
    pub const KEY_ONLY: u32 = 1431;
    /// "You used a key."
    pub const KEY_USED: u32 = 1432;
    /// "You cannot use that item."
    pub const CANNOT_USE_ITEM: u32 = 1434;
    /// "You cannot use this skill."
    pub const CANNOT_USE_SKILL: u32 = 1435;
    /// "This object is locked."
    pub const LOCKED: u32 = 1437;
    /// "Locked": over an object that failed to open.
    pub const LOCKED_WORD: u32 = 1439;
    /// A skill check's line: "<CUSTOM0> <CUSTOM1>. <CUSTOM2>! <CUSTOM3>
    /// (roll <CUSTOM4><CUSTOM5>) vs. DC <CUSTOM6>", with the skill's name,
    /// "success", "failure" or "success not possible", and "Take 20".
    pub const CHECK: u32 = 1406;
    pub const SECURITY: u32 = 329;
    pub const SUCCESS: u32 = 1392;
    pub const FAILURE: u32 = 1393;
    pub const NOT_POSSIBLE: u32 = 1397;
    pub const TAKE_20: u32 = 1409;
    /// What floats over a skill's user as its check ends (`floats`): "Skill
    /// Too Low", "Impossible", besides "success" and "failure".
    pub const SKILL_TOO_LOW: u32 = 1519;
    pub const IMPOSSIBLE: u32 = 1370;
    /// What a trap check tries (the 1406 line's skill): "Disable Mine",
    /// "Set Mine", "Recover Mine".
    pub const DISARM_MINE: u32 = 1529;
    pub const SET_MINE: u32 = 1530;
    pub const RECOVER_MINE: u32 = 1531;
    /// "This mine appears to be a: <CUSTOM0>", the trap's name: told when
    /// the party finds one.
    pub const MINE_SEEN: u32 = 1517;
    /// "You triggered a Mine!"
    pub const MINE_TRIGGERED: u32 = 1461;
    /// TSL's: "Attempt to blast lock <CUSTOM1>: <CUSTOM2> ( <CUSTOM3> +
    /// <CUSTOM4>/2 + mine strength(<CUSTOM5>)) VS Lock DC of <CUSTOM6>",
    /// with "Succeeded" or "Failed".
    pub const BLAST: u32 = 128535;
    pub const BLAST_SUCCEEDED: u32 = 128534;
    pub const BLAST_FAILED: u32 = 128533;
    /// "You cannot enter stealth mode while in combat.", "Stealth Mode
    /// Activated (Run Disabled)", "Stealth Mode Deactivated (Run Enabled)".
    pub const STEALTH_IN_COMBAT: u32 = 1452;
    pub const STEALTH_ON: u32 = 1464;
    pub const STEALTH_OFF: u32 = 1465;
    /// "Sneak Attack!": told to whoever lands one.
    pub const SNEAK_ATTACK: u32 = 1459;
}

/// The words K1's client floats over the user of Security, Disable Mine or
/// Recover Mine as its check ends (from the result its server sends with
/// the check's line): a mine's success; a failure in combat (`roll`), but a
/// mine's that set it off (`sprung`); out of combat, where taking 20 did not
/// reach the DC, "Skill Too Low", or for a lock of `IMPOSSIBLE_DC` or more
/// "Impossible". Nothing else floats: not Security's success, nor setting a
/// mine. The client draws them starred (`*failure*`).
pub fn floats(skill: u32, roll: Option<u8>, dc: u8, success: bool, sprung: bool) -> Option<u32> {
    let mine = matches!(skill, lines::DISARM_MINE | lines::RECOVER_MINE);
    if !mine && skill != lines::SECURITY {
        return None;
    }
    match (success, roll) {
        (true, _) => mine.then_some(lines::SUCCESS),
        (false, None) if !mine && dc >= IMPOSSIBLE_DC => Some(lines::IMPOSSIBLE),
        (false, None) => Some(lines::SKILL_TOO_LOW),
        (false, Some(_)) => (!sprung).then_some(lines::FAILURE),
    }
}

/// The lock DC from which a failure taking 20 floats "Impossible".
pub const IMPOSSIBLE_DC: u8 = rules::interaction::security::IMPOSSIBLE_DC;

/// What the step's keys did, for the scripts: each object unlocked and
/// who unlocked it; whether a key was used up (the goods change).
#[derive(Resource, Default)]
pub(crate) struct Locks {
    pub unlocked: Vec<(SimId, SimId)>,
    pub used_items: bool,
    /// The doors that failed to open for want of their key, told after
    /// their scripts (`refused`).
    refused: Vec<Refusal>,
}

/// A locked object that failed to open for a seat's player (`seat`), every
/// player there is (`seats`) hearing it.
struct Refusal {
    object: SimId,
    seat: Seat,
    seats: Vec<Seat>,
}

impl Refusal {
    /// The player is told it is locked; every player hears its locked
    /// sound and sees "Locked" over it.
    fn tell(&self, state: &mut State, notices: &mut Vec<(Seat, Notice)>, now: u64) {
        notices.push((self.seat, Notice::Line(lines::LOCKED)));
        state
            .talk
            .bark_line(ObjectId(self.object.0), lines::LOCKED_WORD, now);
        let sound = Sound::new(self.object.0, crate::notice::LOCKED_SOUND);
        notices.extend(self.seats.iter().map(|seat| (*seat, Notice::Sound(sound))));
    }
}

/// Security's total for a character (`stealth::total`): its ranks as its
/// effects and items leave them with what its feats add (`now`,
/// `Sheet::skill`), its key ability's modifier; `None` for one with no
/// ranks of its own (`worn`, its sheet without effects).
pub(crate) fn security(library: &Library, worn: &Sheet, now: &Sheet) -> Option<i16> {
    crate::stealth::total(Some(library), worn, now, skills::SECURITY)
}

/// A seat's player as a failed opening sees it.
struct Picker {
    seat: Seat,
    id: SimId,
    /// Metres.
    at: [f32; 3],
}

/// The item tagged `tag` that `holder` (the party: `PARTY`) carries, if it
/// carries one.
pub(crate) fn carried_tagged(
    state: &State,
    catalog: &Catalog,
    holder: u32,
    tag: &str,
) -> Option<u32> {
    state
        .items
        .held_by(holder)
        .find(|(_, item)| {
            state
                .item_facts(catalog, &item.template)
                .is_some_and(|facts| facts.tag.eq_ignore_ascii_case(tag))
        })
        .map(|(id, _)| id)
}

fn locked(state: &ObjectState, facts: &ObjectData) -> bool {
    matches!(facts.kind, ObjectKind::Door | ObjectKind::Placeable) && state.locked(facts.opens)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn locks(
    clock: Res<Clock>,
    level: Res<Static>,
    mut scripts: ResMut<scripts::Scripts>,
    mut opened: ResMut<scripts::Opened>,
    mut notices: ResMut<Notices>,
    mut locks: ResMut<Locks>,
    players: Query<(&Id, Option<&Avatar>, &Player)>,
    // Doors and placeables: what has a body is a character above.
    mut objects: Query<(&Id, &mut Object), Without<Player>>,
) {
    locks.unlocked.clear();
    locks.used_items = false;
    locks.refused.clear();
    let failed = opened.0.iter().any(|(_, by, ok)| !ok && by.is_some());
    let Some(library) = level.0.scripts().cloned().filter(|_| failed) else {
        return;
    };
    let now = clock.time_ms;
    // The companion leading picks for its player (its body, its Security),
    // ahead of the player's own.
    let lent = super::lent(&scripts.state.party);
    let mut pickers: Vec<Picker> = players
        .iter()
        .filter_map(|(id, avatar, player)| {
            let seat = match avatar {
                Some(avatar) => avatar.0,
                None if lent == Some(id.0) => super::LEADER_SEAT,
                None => return None,
            };
            Some(Picker {
                seat,
                id: id.0,
                at: math::scale(player.0.origin, collision::UNIT),
            })
        })
        .collect();
    pickers.sort_by_key(|picker| (picker.seat, Some(picker.id) != lent));
    let mut seats: Vec<Seat> = pickers.iter().map(|picker| picker.seat).collect();
    seats.dedup();
    let mut objects: Vec<_> = objects.iter_mut().collect();
    objects.sort_by_key(|(_, object)| object.index);
    let state = &mut scripts.state;
    let catalog = library.items();
    let notices = &mut notices.0;

    // Opening a locked object: its key, else it stays shut. One another
    // seat's key opened this step is open for this one too.
    let fails: Vec<(usize, SimId, SimId)> = opened
        .0
        .iter()
        .enumerate()
        .filter(|(_, (_, _, ok))| !ok)
        .filter_map(|(at, (object, by, _))| Some((at, *object, (*by)?)))
        .collect();
    let mut dropped = Vec::new();
    for (at, object, by) in fails {
        let Some(picker) = pickers.iter().find(|picker| picker.id == by) else {
            continue;
        };
        let Some((id, object)) = objects.iter_mut().find(|(id, _)| id.0 == object) else {
            continue;
        };
        let Some(facts) = object.facts.clone() else {
            continue;
        };
        if !locked(&object.state, &facts) {
            dropped.push(at);
            continue;
        }
        let key = facts
            .lock
            .as_ref()
            .filter(|lock| !lock.key.is_empty())
            .and_then(|lock| {
                Some((
                    carried_tagged(state, catalog, PARTY, &lock.key)?,
                    lock.auto_remove_key,
                ))
            });
        let Some((key, used_up)) = key else {
            let refusal = Refusal {
                object: id.0,
                seat: picker.seat,
                seats: seats.clone(),
            };
            if facts.kind == ObjectKind::Door {
                locks.refused.push(refusal);
            } else if !super::uses::answers_lock(&facts) {
                refusal.tell(state, notices, now);
            }
            continue;
        };
        notices.push((picker.seat, Notice::Line(lines::KEY_USED)));
        if used_up {
            state.use_one(key);
            locks.used_items = true;
        }
        object.state.lock = lock::UNLOCKED;
        locks.unlocked.push((id.0, by));
        dropped.push(at);
        if facts.kind == ObjectKind::Door {
            object.state.state = facts.opening(Some(picker.at));
            opened.0.push((id.0, Some(by), true));
        } else if facts.container {
            containers::hold(id.0, &mut object.state, picker.seat, by, &mut opened);
        }
    }
    for at in dropped.into_iter().rev() {
        opened.0.remove(at);
    }
}

/// The doors that failed to open this step for want of their key
/// (`locks`), told once their `OnFailToOpen` scripts have run and their
/// changes stand (`scripts::apply`): only a door still locked, neither
/// opening nor with its own opening queued.
pub(super) fn refused(
    clock: Res<Clock>,
    mut scripts: ResMut<scripts::Scripts>,
    mut locks: ResMut<Locks>,
    mut notices: ResMut<Notices>,
    mut talking: ResMut<super::Talking>,
    objects: Query<(&Id, &Object)>,
) {
    if locks.refused.is_empty() {
        return;
    }
    let state = &mut scripts.state;
    for refusal in std::mem::take(&mut locks.refused) {
        let Some((_, object)) = objects.iter().find(|(id, _)| id.0 == refusal.object) else {
            continue;
        };
        let Some(facts) = object.facts.as_deref() else {
            continue;
        };
        let door = ObjectId(refusal.object.0);
        let opening = object.state.state != 0
            || state.actions.get(&door.0).is_some_and(|queue| {
                queue.iter().any(|queued| {
                    matches!(queued.action, Action::Door { door: of, open: true } if of == door)
                })
            });
        if locked(&object.state, facts) && !opening {
            refusal.tell(state, &mut notices.0, clock.time_ms);
        }
    }
    let told = state.talk.told();
    if talking.0 != told {
        talking.0 = told;
    }
}

/// The bonus a security spike the party carries (`item`) gives, if it
/// carries it and it is one.
fn spike_bonus(state: &State, catalog: &Catalog, item: u32) -> Option<i16> {
    state
        .items
        .get(&item)
        .filter(|carried| carried.holder == PARTY && carried.slot.is_none())
        .and_then(|carried| state.item_facts(catalog, &carried.template))
        .map(|facts| i16::from(facts.tools))
        .filter(|tools| *tools > 0)
}

/// The lock of area object `index`, if it is a door or placeable.
pub(super) fn lock_of<'a>(view: &'a View, index: u32) -> Option<(&'a Thing, &'a ObjectData)> {
    view.things.iter().find_map(|thing| {
        let facts = thing.facts.as_deref()?;
        (thing.index == Some(index)
            && matches!(facts.kind, ObjectKind::Door | ObjectKind::Placeable))
        .then_some((thing, facts))
    })
}

/// `user` says line `slot` of its sound set, heard by every player.
fn say(out: &mut Out, user: &Thing, slot: usize) {
    out.notices
        .push((None, Notice::Sound(Sound::line(user.id.0, slot))));
}

/// Security asked of `user` on area object `index` (with the party's
/// `spike`, if any), its refusals told to `tell` (its seat's player): the
/// use begun (see the module documentation), or `None`.
#[allow(clippy::too_many_arguments)]
pub(super) fn begin(
    view: &View,
    library: &Library,
    state: &State,
    out: &mut Out,
    user: &Thing,
    tell: ObjectId,
    index: u32,
    spike: Option<u32>,
) -> Option<SkillUse> {
    let mut refuse = |line| {
        out.notices.push((Some(tell), line));
        None
    };
    let Some((target, facts)) = lock_of(view, index) else {
        return refuse(Notice::Line(lines::NOT_LOCKED));
    };
    let up = user.hp.is_some_and(|(hp, _)| hp > 0) && user.character.is_some_and(|c| !c.downed);
    if !up || facts.use_distance(user.position) > containers::REACH {
        return refuse(Notice::OutOfReach);
    }
    if target.locked != Some(true) {
        return refuse(Notice::Line(lines::NOT_LOCKED));
    }
    if facts.lock.as_ref().is_some_and(|lock| lock.key_required) {
        return refuse(Notice::Line(lines::KEY_ONLY));
    }
    let character = user.character?;
    // A swing under way would break it off at once (`skill_uses`):
    // nothing said or played.
    if character.attacking {
        return None;
    }
    if security(library, &character.worn, &character.sheet).is_none() {
        return refuse(Notice::Line(lines::CANNOT_USE_SKILL));
    }
    if spike.is_some_and(|item| spike_bonus(state, library.items(), item).is_none()) {
        return refuse(Notice::Line(lines::CANNOT_USE_ITEM));
    }
    let animation = match facts.kind {
        ObjectKind::Door => animations::DOOR,
        _ => animations::PLACEABLE,
    };
    super::traps::act(out, user, Some(facts.position), animation, UNLOCK_MS);
    say(out, user, slots::BEGIN_UNLOCK);
    Some(SkillUse {
        kind: UseKind::Unlock,
        by: user.id.0,
        object: target.id.0,
        tool: spike,
        from: user.position,
        until: view.now_ms + UNLOCK_MS,
    })
}

/// `used`, Security begun by `user`, done: the check, told to `tell`; the
/// lock unlocked (`OnUnlock`) and a shut door opened on a success, else
/// the spike, if any, used up. Whether the party's goods changed.
#[allow(clippy::too_many_arguments)]
pub(super) fn unlock(
    view: &View,
    library: &Library,
    state: &mut State,
    out: &mut Out,
    rng: &mut SimRng,
    user: &Thing,
    tell: ObjectId,
    used: &SkillUse,
) -> bool {
    let target = view.thing(ObjectId(used.object));
    let Some((target, facts)) = target.and_then(|thing| Some((thing, thing.facts.as_deref()?)))
    else {
        return false;
    };
    if target.locked != Some(true) {
        out.notices
            .push((Some(tell), Notice::Line(lines::NOT_LOCKED)));
        return false;
    }
    let Some(character) = user.character else {
        return false;
    };
    let Some(total) = security(library, &character.worn, &character.sheet) else {
        return false;
    };
    let bonus = match used.tool {
        None => 0,
        Some(item) => match spike_bonus(state, library.items(), item) {
            Some(bonus) => bonus,
            None => {
                out.notices
                    .push((Some(tell), Notice::Line(lines::CANNOT_USE_ITEM)));
                return false;
            }
        },
    };
    let total = total + bonus;
    let dc = facts.lock.as_ref().map_or(0, |lock| lock.open_dc);
    let roll = character
        .in_combat
        .then(|| 1 + rng.below(rules::interaction::checks::DIE) as u8);
    let success =
        i16::from(roll.unwrap_or(rules::interaction::checks::TAKEN)) + total >= i16::from(dc);
    out.notices.push((
        Some(tell),
        Notice::Check {
            skill: lines::SECURITY,
            roll,
            total,
            dc,
            success,
        },
    ));
    out.feedback
        .extend(floats(lines::SECURITY, roll, dc, success, false).map(|words| (user.id, words)));
    say(
        out,
        user,
        if success {
            slots::UNLOCK_SUCCESS
        } else {
            slots::UNLOCK_FAILED
        },
    );
    if success {
        out.changes.push(Change::Lock {
            door: target.id,
            locked: false,
        });
        state.last.insert((target.id.0, last::UNLOCKED), user.id.0);
        scripts::raise(view, state, target.id, crate::script::Hook::Unlock, None);
        if facts.kind == ObjectKind::Door && target.open == Some(0) {
            out.changes.push(Change::Door {
                door: target.id,
                open: true,
                by: user.id,
            });
        }
        return false;
    }
    used.tool.inspect(|item| state.use_one(*item)).is_some()
}
