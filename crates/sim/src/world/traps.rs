//! Traps in play (`docs/COMBAT.md`), on the
//! authority within the scripts' step (`world::scripts`).
//!
//! Springing (S6): a creature coming into a trap's area (`scripts::triggers`
//! tells who came in), or opening or using a trapped door or placeable,
//! springs it unless it is of the trap's faction, not hostile to it (its
//! creator's then, for a laid mine; else its own), or carries its key (which
//! disarms it instead). Springing runs the trap's `OnTrapTriggered`, else its
//! `traps.2da` row's `trapscript`, as the trap with the creature entering,
//! plays the row's sound there, and takes a one-shot trap away. Whether the
//! party detected it is not asked: a detected trap still springs.
//!
//! Detecting (S5): every creature, each `traps::DETECT_EVERY_MS`, looks for
//! the armed, detectable traps within `traps::DETECT_RANGE_M` of their
//! `traps::RADIUS_M` circle that are not its faction's nor friendly to it,
//! and finds each by `traps::detects` (Awareness, a die and a base, less
//! while running: `rules::traps::detect`; a flagged trap is found anyway). A party member's find is the
//! party's: every member has it, the players see it and are told.
//!
//! Disarming and recovering (S7, S8): a seat's `Action::Disarm` or
//! `Recover` on a trap the party sees, within reach, with Demolitions,
//! takes `traps::DISARM_MS`; moving off, attacking or falling ends it with
//! no check. Then `traps::disarm` or `recover` (out of combat taking
//! `rules::interaction::checks::TAKEN`) decides: done, the trap gone (a disarm runs its `OnDisarm`, a recover
//! gives the party its kit); failed; or sprung on whoever tried.
//!
//! Laying (S9): a seat's `buttons::LAY_MINE` press sets the best kit the
//! party carries (`traps::best_kit`) at its feet over `traps::LAY_MS`, up
//! to `traps::LAID_MOST` of the party's in the area; then `traps::lay`
//! decides: a mine made (a trigger of its row's template,
//! `traps::laid_template`), the layer's and its faction's, its DCs from the
//! check, seen by the party, the kit used up; a failure keeps the kit.
//! TSL's mines on doors: a seat's `Action::Mine` on a door sets a kit there
//! (`traps::door_kit`) over `traps::LAY_MS`, never rolling; set, the mine
//! stands at the door and `traps::BLAST_MS` later blasts its lock
//! (`traps::blasts`, told as TSL's line 128535); a blasted door, plot or
//! not, is unlocked and opens `traps::OPEN_AFTER_MS` after that.
//!
//! A trap taken away in a step (sprung, disarmed, recovered) is spent for
//! the rest of it: no second spring, no use resolved on it.
//!
//! A seat's use is its body's, the companion leading the party's for the
//! first seat; Security is a use alike (`world::locks`).

use script_ncs::vm::ObjectId;

use super::Level;
use super::locks::lines;
use crate::action::Action;
use crate::combat::sheet::skills;
use crate::input::{Seat, TickInput};
use crate::inventory::{Catalog, PARTY};
use crate::math::flat_distance as across;
use crate::module::ObjectData;
use crate::notice::{Notice, Sound};
use crate::rng::SimRng;
use crate::script::{
    Change, Library, Made, Out, Pending, SkillUse, State, Thing, TrapState, UseKind, View, last,
};
use crate::snapshot::trap;
use crate::traps::Outcome;

/// How far a skill's user may move from where it began (metres) before
/// the use breaks off.
const STAY_M: f32 = rules::interaction::skill_uses::STAY_M;

#[cfg(feature = "probes")]
pub(crate) mod probes;

/// Whether `thing` is a trap that still springs: one with a trap's facts,
/// not disabled.
pub(crate) fn armed(thing: &Thing) -> bool {
    thing
        .facts
        .as_ref()
        .is_some_and(|facts| facts.trap.is_some())
        && thing.trap & trap::DISABLED == 0
}

/// Whether a body at `point` (metres) is in a trap's area: a laid mine's is
/// the square round where it stands (`at`), as high as a trigger reaches;
/// any other trigger's its outline.
pub(crate) fn holds(level: &Level, facts: &ObjectData, at: [f32; 3], point: [f32; 3]) -> bool {
    if !facts.geometry.is_empty() || crate::traps::laid_row(&facts.template).is_none() {
        return level.in_trigger(facts, point);
    }
    let feet = point[2] + crate::motion::SOLE_Z * collision::UNIT;
    let half = crate::traps::LAID_HALF_M;
    (point[0] - at[0]).abs() <= half
        && (point[1] - at[1]).abs() <= half
        && (feet - at[2]).abs() <= crate::module::TRIGGER_REACH_M
}

/// The faction a trap spares and is hostile by: its creator's when it was
/// laid, else its own.
fn faction(state: &State, id: ObjectId, facts: &ObjectData) -> u8 {
    state
        .traps
        .get(&id.0)
        .and_then(|kept| kept.faction)
        .unwrap_or(facts.faction)
}

/// Takes a trap away: destroyed. Its state stays until the step's scripts
/// have run (`forget`): a sprung mine's script asks its faction.
fn gone(out: &mut Out, id: ObjectId) {
    out.changes.push(Change::destroy(id));
}

/// Whether this step already took trap `id` away (sprung, disarmed or
/// recovered): nothing more happens to it.
fn spent(out: &Out, id: ObjectId) -> bool {
    out.changes
        .iter()
        .any(|change| matches!(change, Change::Destroy { who, .. } if *who == id))
}

/// The state of the traps this step took away, forgotten once its
/// scripts have run.
pub(crate) fn forget(state: &mut State, changes: &[Change]) {
    for change in changes {
        if let Change::Destroy { who, .. } = change {
            state.traps.remove(&who.0);
        }
    }
}

/// `by` disarmed the trap `id`: its `OnDisarm` runs with `by` its last
/// disarmer (`GetLastDisarmed`), and it is taken away.
pub(crate) fn disarmed(view: &View, state: &mut State, out: &mut Out, id: ObjectId, by: ObjectId) {
    state.last.insert((id.0, last::DISARMED), by.0);
    super::scripts::raise(view, state, id, crate::script::Hook::Disarm, None);
    gone(out, id);
}

/// `creature` springs the trap `trap` (see the module documentation):
/// coming into it, or opening or using it; `forced` (a failed disarm) asks
/// no faction.
pub(crate) fn spring(
    view: &View,
    catalog: &Catalog,
    state: &mut State,
    out: &mut Out,
    trap: &Thing,
    creature: &Thing,
    forced: bool,
) {
    let (Some(facts), Some(character)) = (trap.facts.as_deref(), creature.character) else {
        return;
    };
    let Some(found) = facts
        .trap
        .as_ref()
        .filter(|_| armed(trap) && !spent(out, trap.id))
    else {
        return;
    };
    let faction = faction(state, trap.id, facts);
    let standing = view
        .module
        .repute
        .with(&state.reputation)
        .of(character.faction, faction);
    // Immunity to traps is not in the effects engine (no K1 item or power
    // gives it).
    if !forced && !crate::traps::springs(standing, character.faction == faction, false) {
        return;
    }
    if !found.key.is_empty() {
        let holder = if character.party {
            PARTY
        } else {
            creature.id.0
        };
        if let Some(key) = super::locks::carried_tagged(state, catalog, holder, &found.key) {
            if found.auto_remove_key {
                state.use_one(key);
            }
            disarmed(view, state, out, trap.id, creature.id);
            return;
        }
    }
    let row = view.module.traps.get(usize::from(found.row));
    let script = view
        .hook(trap.id, crate::script::Hook::TrapTriggered)
        .filter(|script| !script.is_empty() && script != "default")
        .or_else(|| row.map(|row| row.script.clone()))
        .filter(|script| !script.is_empty());
    if let Some(script) = script {
        let mut pending = Pending::new(&script, trap.id);
        pending.entering = Some(creature.id);
        state.pending.push(pending);
    }
    if let Some(row) = row.filter(|row| !row.sound.is_empty()) {
        out.notices
            .push((None, Notice::Sound(Sound::new(trap.id.0, &row.sound))));
    }
    if character.party {
        out.notices
            .push((Some(creature.id), Notice::Line(lines::MINE_TRIGGERED)));
    }
    if found.one_shot {
        gone(out, trap.id);
    }
}

/// Doors and containers opened and placeables used this step that are
/// trapped spring on whoever opened or used them, once (a container used
/// as it opens is both).
pub(crate) fn opened_and_used(
    view: &View,
    catalog: &Catalog,
    state: &mut State,
    out: &mut Out,
    opened: &[(crate::SimId, Option<crate::SimId>, bool)],
    used: &[(crate::SimId, crate::SimId)],
) {
    let doors = opened
        .iter()
        .filter(|(_, _, ok)| *ok)
        .filter_map(|(door, by, _)| Some((*door, (*by)?)));
    let users = used.iter().copied();
    let mut sprung = Vec::new();
    for (object, by) in doors.chain(users) {
        if sprung.contains(&(object, by)) {
            continue;
        }
        sprung.push((object, by));
        if let (Some(trap), Some(creature)) =
            (view.thing(ObjectId(object.0)), view.thing(ObjectId(by.0)))
            && armed(trap)
        {
            spring(view, catalog, state, out, trap, creature, false);
        }
    }
}

/// A trap's middle (metres): its outline's, else where it stands.
fn middle(thing: &Thing) -> [f32; 3] {
    let geometry = thing
        .facts
        .as_ref()
        .map_or(&[][..], |facts| &facts.geometry[..]);
    crate::traps::centre(geometry, thing.position)
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    crate::math::length(crate::math::sub(a, b))
}

/// A skill's total for a character: its ranks as effects and items leave
/// them, its key ability, its feats (`stealth::total`).
fn total(library: &Library, thing: &Thing, skill: usize) -> Option<i16> {
    let character = thing.character?;
    crate::stealth::total(Some(library), &character.worn, &character.sheet, skill)
}

/// The traps that still spring, in id order.
fn armed_traps<'a>(view: &'a View) -> impl Iterator<Item = &'a Thing> {
    view.things.iter().filter(|thing| armed(thing))
}

/// Every creature's look for traps this step (see the module
/// documentation).
pub(crate) fn detect(
    view: &View,
    library: &Library,
    state: &mut State,
    out: &mut Out,
    rng: &mut SimRng,
) {
    let msec =
        (view.now_ms - state.last_detect_ms.min(view.now_ms)).min(u64::from(u32::MAX)) as u32;
    state.last_detect_ms = view.now_ms;
    let party: Vec<u32> = view
        .things
        .iter()
        .filter(|thing| thing.character.is_some_and(|c| c.party))
        .map(|thing| thing.id.0)
        .collect();
    let standing = view.module.repute.with(&state.reputation);
    for looker in view.things.iter().filter(|thing| {
        thing.hp.is_some_and(|(hp, _)| hp > 0) && thing.character.is_some_and(|c| !c.downed)
    }) {
        let Some(character) = looker.character else {
            continue;
        };
        if !super::scripts::moment(
            view.now_ms,
            msec.max(1),
            crate::traps::DETECT_EVERY_MS,
            u64::from(looker.id.0) * 37,
        ) {
            continue;
        }
        let awareness = total(library, looker, skills::AWARENESS).unwrap_or(0);
        for found in armed_traps(view) {
            let (Some(facts), id) = (found.facts.as_deref(), found.id) else {
                continue;
            };
            let Some(kind) = facts.trap.as_ref().filter(|trap| trap.detectable) else {
                continue;
            };
            let kept = state.traps.get(&id.0);
            let faction = kept.and_then(|kept| kept.faction).unwrap_or(facts.faction);
            if kept.is_some_and(|kept| kept.detected_by.contains(&looker.id.0))
                || faction == character.faction
                || standing.of(character.faction, faction)
                    >= rules::world::factions::FRIENDLY_AT_LEAST
                || distance(middle(found), looker.position) - view.module.ranges.trap_radius_m
                    > crate::traps::DETECT_RANGE_M
            {
                continue;
            }
            let dc = kept
                .and_then(|kept| kept.laid)
                .map_or(kind.detect_dc, |laid| laid.detect_dc);
            let d10 = 1 + rng.below(rules::traps::detect::DIE) as u8;
            if !(kind.flagged || crate::traps::detects(awareness, d10, character.running, dc)) {
                continue;
            }
            state.last.insert((looker.id.0, last::TRAP_DETECTED), id.0);
            let kept = state.traps.entry(id.0).or_default();
            if !character.party {
                kept.detected_by.push(looker.id.0);
                continue;
            }
            for member in &party {
                if !kept.detected_by.contains(member) {
                    kept.detected_by.push(*member);
                }
            }
            if found.trap & trap::PARTY_SEES == 0 {
                out.changes.push(Change::Trap {
                    object: id,
                    bits: found.trap | trap::PARTY_SEES,
                });
                let name = view
                    .module
                    .traps
                    .get(usize::from(kind.row))
                    .map_or(u32::MAX, |row| row.name);
                out.notices.push((None, Notice::TrapSeen { name }));
            }
        }
    }
}

/// Whether the trap `id` was made by a party member.
fn own(view: &View, state: &State, id: ObjectId) -> bool {
    state
        .traps
        .get(&id.0)
        .and_then(|kept| kept.creator)
        .and_then(|creator| view.thing(ObjectId(creator)))
        .is_some_and(|creator| creator.character.is_some_and(|c| c.party))
}

/// The trap kits the party carries: item id and `traps.2da` row.
fn kits(state: &State, catalog: &Catalog) -> Vec<(u32, u8)> {
    state
        .items
        .held_by(PARTY)
        .filter(|(_, item)| item.slot.is_none())
        .filter_map(|(id, item)| Some((id, state.item_facts(catalog, &item.template)?.trap?)))
        .collect()
}

/// How many mines the party laid that stand in the area.
fn laid_standing(view: &View, state: &State) -> usize {
    state
        .traps
        .iter()
        .filter(|(id, kept)| {
            kept.laid.is_some()
                && view.thing(ObjectId(**id)).is_some()
                && kept
                    .creator
                    .and_then(|creator| view.thing(ObjectId(creator)))
                    .is_some_and(|creator| creator.character.is_some_and(|c| c.party))
        })
        .count()
}

fn refuse(out: &mut Out, body: ObjectId) {
    out.notices
        .push((Some(body), Notice::Line(lines::CANNOT_USE_SKILL)));
}

/// A use begun: its user turned to face `toward` (metres; a lay faces
/// where it stands) and playing KOTOR's animation for it for `ms` (a
/// trap's here, Security's in `world::locks`).
pub(super) fn act(out: &mut Out, user: &Thing, toward: Option<[f32; 3]>, animation: u16, ms: u64) {
    if let Some(at) = toward {
        let yaw = crate::math::atan2_deg(at[1] - user.position[1], at[0] - user.position[0]);
        out.changes.push(Change::Face { who: user.id, yaw });
    }
    out.changes.push(Change::Animate {
        who: user.id,
        play: (animation, ms as u32),
    });
}

/// The lay key's presses this step: a use begun, or refused.
fn begin_laying(
    view: &View,
    library: &Library,
    state: &mut State,
    presses: &[(Seat, crate::SimId, u16)],
    out: &mut Out,
) {
    for (seat, body, pressed) in presses {
        if pressed & crate::input::buttons::LAY_MINE == 0 {
            continue;
        }
        let body = ObjectId(body.0);
        let Some(user) = view.thing(body) else {
            continue;
        };
        let kits = kits(state, library.items());
        let best = crate::traps::best_kit(kits.iter().map(|(_, row)| *row));
        let kit = best.and_then(|best| kits.iter().find(|(_, row)| *row == best));
        let (Some(&(item, _)), Some(_)) = (kit, total(library, user, skills::DEMOLITIONS)) else {
            refuse(out, body);
            continue;
        };
        if state.skill_uses.contains_key(&seat.0)
            || laid_standing(view, state) >= crate::traps::LAID_MOST
        {
            refuse(out, body);
            continue;
        }
        state.skill_uses.insert(
            seat.0,
            SkillUse {
                kind: UseKind::Lay,
                by: body.0,
                object: item,
                tool: None,
                from: user.position,
                until: view.now_ms + crate::traps::LAY_MS,
            },
        );
        act(
            out,
            user,
            None,
            crate::traps::animations::SET,
            crate::traps::LAY_MS,
        );
    }
}

/// A lay done: the check; the mine made and the kit used up if laid (a
/// failure keeps the kit, S9).
#[allow(clippy::too_many_arguments)]
fn lay(
    view: &View,
    level: &Level,
    library: &Library,
    state: &mut State,
    out: &mut Out,
    rng: &mut SimRng,
    user: &Thing,
    item: u32,
) {
    let (Some(character), Some(total)) =
        (user.character, total(library, user, skills::DEMOLITIONS))
    else {
        return;
    };
    let row = state
        .items
        .get(&item)
        .filter(|kit| kit.holder == PARTY)
        .and_then(|kit| state.item_facts(library.items(), &kit.template))
        .and_then(|facts| facts.trap);
    let Some((row, trap_row)) =
        row.and_then(|row| Some((row, view.module.traps.get(usize::from(row))?.clone())))
    else {
        return;
    };
    let roll = character
        .in_combat
        .then(|| 1 + rng.below(rules::interaction::checks::DIE) as u8);
    let ranks = character.worn.skills[skills::DEMOLITIONS];
    let (got, laid) = crate::traps::lay(total, ranks, roll, &trap_row);
    if laid.is_some() {
        state.use_one(item);
    }
    out.notices.push((
        Some(user.id),
        Notice::Check {
            skill: lines::SET_MINE,
            roll,
            total: got - i16::from(roll.unwrap_or(rules::interaction::checks::TAKEN)),
            dc: trap_row.set_dc,
            success: got >= i16::from(trap_row.set_dc),
        },
    ));
    let Some(laid) = laid else {
        return;
    };
    let feet = [
        user.position[0],
        user.position[1],
        user.position[2] + crate::motion::SOLE_Z * collision::UNIT,
    ];
    make_mine(view, level, state, out, user, (row, laid, total), feet);
}

/// A mine of `traps.2da` row `row` made at `feet` (metres), `user`'s and
/// its faction's, with `laid`'s DCs and its Demolitions total as it set it
/// (`demolitions`, TSL's `OwnerDemolitionsSkill`), seen by the party: its
/// id.
fn make_mine(
    view: &View,
    level: &Level,
    state: &mut State,
    out: &mut Out,
    user: &Thing,
    (row, laid, demolitions): (u8, crate::traps::Laid, i16),
    feet: [f32; 3],
) -> Option<ObjectId> {
    let character = user.character?;
    let template = level.template(
        crate::module::ObjectKind::Trigger,
        &crate::traps::laid_template(row),
    )?;
    let mut facts = (*template).clone();
    facts.position = feet;
    let id = ObjectId(view.next_id + out.made.len() as u32);
    out.made.push(Made {
        id,
        facts: std::sync::Arc::new(facts),
        at: feet,
        yaw: 0.0,
        kept: None,
    });
    let party = view
        .things
        .iter()
        .filter(|thing| thing.character.is_some_and(|c| c.party))
        .map(|thing| thing.id.0)
        .collect();
    state.traps.insert(
        id.0,
        TrapState {
            detected_by: party,
            creator: Some(user.id.0),
            faction: Some(character.faction),
            laid: Some(laid),
            disabled: false,
            owner_demolitions: demolitions.clamp(-128, 127) as i8,
        },
    );
    out.changes.push(Change::Trap {
        object: id,
        bits: trap::PARTY_SEES,
    });
    Some(id)
}

/// TSL's mine set on door `index` asked by a seat (`Action::Mine`): the
/// kit a player would pick (`traps::door_kit`), set by `user` within reach
/// with Demolitions over `traps::LAY_MS`; refused outside TSL, on a door
/// already mined, or with no kit.
#[allow(clippy::too_many_arguments)]
fn begin_door_mine(
    view: &View,
    library: &Library,
    state: &mut State,
    out: &mut Out,
    seat: Seat,
    user: &Thing,
    tell: ObjectId,
    index: u32,
) {
    let door = super::locks::lock_of(view, index)
        .filter(|(_, facts)| facts.kind == crate::module::ObjectKind::Door);
    let demolitions = total(library, user, skills::DEMOLITIONS);
    let kits = kits(state, library.items());
    let (Some((door, facts)), Some(demolitions)) = (door, demolitions) else {
        return refuse(out, tell);
    };
    let dc = facts.lock.as_ref().map_or(0, |lock| lock.open_dc);
    let kit = crate::traps::door_kit(demolitions, dc, kits.iter().map(|(_, row)| *row))
        .and_then(|best| kits.iter().find(|(_, row)| *row == best));
    let Some(&(item, _)) = kit.filter(|_| {
        view.module.game == "tsl"
            && !state.skill_uses.contains_key(&seat.0)
            && !state.door_mines.contains_key(&door.id.0)
    }) else {
        return refuse(out, tell);
    };
    if across(facts.position, user.position) > super::containers::REACH {
        out.notices.push((Some(tell), Notice::OutOfReach));
        return;
    }
    state.skill_uses.insert(
        seat.0,
        SkillUse {
            kind: UseKind::MineDoor,
            by: user.id.0,
            object: door.id.0,
            tool: Some(item),
            from: user.position,
            until: view.now_ms + crate::traps::LAY_MS,
        },
    );
    act(
        out,
        user,
        Some(facts.position),
        crate::traps::animations::SET,
        crate::traps::LAY_MS,
    );
}

/// A mine set on a door done: the set check, never rolled; on a success the
/// kit used up, the mine made at the door and its blast to come
/// (`blasts`). Whether the party's goods changed.
#[allow(clippy::too_many_arguments)]
fn mine_door(
    view: &View,
    level: &Level,
    library: &Library,
    state: &mut State,
    out: &mut Out,
    user: &Thing,
    tell: ObjectId,
    used: &SkillUse,
) -> bool {
    let door = view.thing(ObjectId(used.object));
    let (Some(door), Some(item), Some(character), Some(total)) = (
        door,
        used.tool,
        user.character,
        total(library, user, skills::DEMOLITIONS),
    ) else {
        return false;
    };
    let row = state
        .items
        .get(&item)
        .filter(|kit| kit.holder == PARTY)
        .and_then(|kit| state.item_facts(library.items(), &kit.template))
        .and_then(|facts| facts.trap);
    let Some((row, trap_row)) =
        row.and_then(|row| Some((row, view.module.traps.get(usize::from(row))?.clone())))
    else {
        return false;
    };
    if state.door_mines.contains_key(&door.id.0) {
        return false;
    }
    let ranks = character.worn.skills[skills::DEMOLITIONS];
    let (got, laid) = crate::traps::lay(total, ranks, None, &trap_row);
    out.notices.push((
        Some(tell),
        Notice::Check {
            skill: lines::SET_MINE,
            roll: None,
            total: got - i16::from(rules::interaction::checks::TAKEN),
            dc: trap_row.set_dc,
            success: laid.is_some(),
        },
    ));
    let Some(laid) = laid else {
        return false;
    };
    state.use_one(item);
    let set = (row, laid, total);
    if let Some(mine) = make_mine(view, level, state, out, user, set, door.position) {
        state.door_mines.insert(
            door.id.0,
            crate::script::DoorMine {
                setter: user.id.0,
                mine: mine.0,
                strength: crate::traps::blast_strength(row),
                blast_at: view.now_ms + crate::traps::BLAST_MS,
                open_at: None,
            },
        );
    }
    true
}

/// The mines set on doors whose time came: a blast on the lock
/// (`traps::blasts`, by the Demolitions its setter had as it set it, the
/// mine's `TrapState::owner_demolitions`), told to the setter's
/// player, the mine taken away; a blasted door unlocked and opened
/// `traps::OPEN_AFTER_MS` later, by itself (no opener). A mine no longer
/// there when its time comes (recovered, disarmed, set off) blasts nothing.
fn blasts(view: &View, state: &mut State, out: &mut Out) {
    let due: Vec<(u32, crate::script::DoorMine)> = state
        .door_mines
        .iter()
        .filter(|(_, mine)| mine.open_at.unwrap_or(mine.blast_at) <= view.now_ms)
        .map(|(door, mine)| (*door, *mine))
        .collect();
    for (id, mine) in due {
        state.door_mines.remove(&id);
        let Some((door, facts)) = view
            .thing(ObjectId(id))
            .and_then(|door| Some((door, door.facts.as_deref()?)))
        else {
            continue;
        };
        if mine.open_at.is_some() {
            out.changes.push(Change::Lock {
                door: door.id,
                locked: false,
            });
            if door.open == Some(0) {
                out.changes.push(Change::Door {
                    door: door.id,
                    open: true,
                    by: door.id,
                });
            }
            continue;
        }
        let laid = ObjectId(mine.mine);
        if view.thing(laid).is_none() || spent(out, laid) {
            continue;
        }
        let demolitions = state
            .traps
            .get(&laid.0)
            .map_or(0, |kept| i16::from(kept.owner_demolitions));
        let dc = facts.lock.as_ref().map_or(0, |lock| lock.open_dc);
        let (sum, success) = crate::traps::blasts(demolitions, mine.strength, dc);
        out.notices.push((
            Some(ObjectId(mine.setter)),
            Notice::Blast {
                sum,
                demolitions,
                strength: mine.strength,
                dc,
                success,
            },
        ));
        gone(out, laid);
        if success {
            let open_at = Some(view.now_ms + crate::traps::OPEN_AFTER_MS);
            state
                .door_mines
                .insert(id, crate::script::DoorMine { open_at, ..mine });
        }
    }
}

/// The seats' Disarm, Recover and Security (`world::locks`) asked this
/// step, the lay key's presses, and the uses under way (see the module
/// documentation). A seat's use is its body's, the companion leading the
/// party's for the first seat (`acting_body`); its player is told. Whether
/// the party's goods changed.
#[allow(clippy::too_many_arguments)]
pub(crate) fn skill_uses(
    view: &View,
    level: &Level,
    library: &Library,
    state: &mut State,
    (input, presses): (&TickInput, &[(Seat, crate::SimId, u16)]),
    seats: &[(Seat, ObjectId)],
    out: &mut Out,
    rng: &mut SimRng,
) -> bool {
    begin_laying(view, library, state, presses, out);
    blasts(view, state, out);
    let lent = super::lent(&state.party);
    let body_of = |seat: Seat| {
        let &(_, own) = seats.iter().find(|(of, _)| *of == seat)?;
        let body = super::acting_body(seat, crate::SimId(own.0), lent);
        Some((own, ObjectId(body.0)))
    };
    for (seat, action) in input.actions() {
        let Some((tell, body)) = body_of(*seat) else {
            continue;
        };
        let Some(user) = view.thing(body) else {
            continue;
        };
        let busy = |state: &State, object: Option<ObjectId>| {
            state.skill_uses.contains_key(&seat.0)
                || object.is_some_and(|object| {
                    state
                        .skill_uses
                        .values()
                        .any(|used| used.object == object.0)
                })
        };
        let (kind, index) = match *action {
            Action::Disarm(index) => (UseKind::Disarm, index),
            Action::Recover(index) => (UseKind::Recover, index),
            Action::Mine(index) => {
                begin_door_mine(view, library, state, out, *seat, user, tell, index);
                continue;
            }
            Action::Unlock { object, spike } => {
                let lock = super::locks::lock_of(view, object);
                if busy(state, lock.map(|(lock, _)| lock.id)) {
                    refuse(out, tell);
                } else if let Some(used) =
                    super::locks::begin(view, library, state, out, user, tell, object, spike)
                {
                    state.skill_uses.insert(seat.0, used);
                }
                continue;
            }
            _ => continue,
        };
        let target = view
            .things
            .iter()
            .find(|thing| thing.index == Some(index) && armed(thing));
        let Some(target) = target.filter(|target| {
            target.trap & trap::PARTY_SEES != 0
                && target
                    .facts
                    .as_ref()
                    .and_then(|facts| facts.trap.as_ref())
                    .is_some_and(|trap| trap.disarmable)
        }) else {
            refuse(out, tell);
            continue;
        };
        if busy(state, Some(target.id)) || total(library, user, skills::DEMOLITIONS).is_none() {
            refuse(out, tell);
            continue;
        }
        if across(middle(target), user.position) > crate::traps::REACH_M {
            out.notices.push((Some(tell), Notice::OutOfReach));
            continue;
        }
        state.skill_uses.insert(
            seat.0,
            SkillUse {
                kind,
                by: body.0,
                object: target.id.0,
                tool: None,
                from: user.position,
                until: view.now_ms + crate::traps::DISARM_MS,
            },
        );
        let animation = match kind {
            UseKind::Recover => crate::traps::animations::RECOVER,
            _ => crate::traps::animations::DISARM,
        };
        act(
            out,
            user,
            Some(middle(target)),
            animation,
            crate::traps::DISARM_MS,
        );
    }
    let mut goods = false;
    let uses: Vec<(u8, SkillUse)> = state
        .skill_uses
        .iter()
        .map(|(seat, used)| (*seat, *used))
        .collect();
    for (seat, used) in uses {
        let user = view.thing(ObjectId(used.by));
        let stays = user.is_some_and(|user| {
            user.hp.is_some_and(|(hp, _)| hp > 0)
                && user.character.is_some_and(|c| !c.downed && !c.attacking)
                && across(user.position, used.from) <= STAY_M
        });
        if !stays {
            state.skill_uses.remove(&seat);
            // Broken off: its animation with it.
            out.changes.push(Change::Clear {
                who: ObjectId(used.by),
            });
            continue;
        }
        if view.now_ms < used.until {
            continue;
        }
        state.skill_uses.remove(&seat);
        let tell = body_of(Seat(seat)).map_or(ObjectId(used.by), |(tell, _)| tell);
        match (used.kind, user) {
            (UseKind::Lay, Some(user)) => {
                lay(view, level, library, state, out, rng, user, used.object);
                goods = true;
                continue;
            }
            (UseKind::Unlock, Some(user)) => {
                goods |= super::locks::unlock(view, library, state, out, rng, user, tell, &used);
                continue;
            }
            (UseKind::MineDoor, Some(user)) => {
                goods |= mine_door(view, level, library, state, out, user, tell, &used);
                continue;
            }
            (UseKind::Lay | UseKind::Unlock | UseKind::MineDoor, None) => continue,
            _ => {}
        }
        let (Some(user), Some(target)) = (user, view.thing(ObjectId(used.object))) else {
            continue;
        };
        let Some(found) = target.facts.as_ref().and_then(|facts| facts.trap.clone()) else {
            continue;
        };
        let (Some(total), Some(character)) =
            (total(library, user, skills::DEMOLITIONS), user.character)
        else {
            continue;
        };
        // Gone this step already: sprung by another, or used by a lower
        // seat.
        if !armed(target) || spent(out, target.id) {
            continue;
        }
        let roll = character
            .in_combat
            .then(|| 1 + rng.below(rules::interaction::checks::DIE) as u8);
        let dc = state
            .traps
            .get(&target.id.0)
            .and_then(|kept| kept.laid)
            .map_or(found.disarm_dc, |laid| laid.disarm_dc);
        let own = own(view, state, target.id);
        let (outcome, skill, shown) = match used.kind {
            UseKind::Recover => (
                crate::traps::recover(total, roll, dc, own),
                lines::RECOVER_MINE,
                dc.max(1).saturating_add(rules::traps::recover::HARDER),
            ),
            _ => (
                crate::traps::disarm(total, roll, dc, own),
                lines::DISARM_MINE,
                dc.max(1),
            ),
        };
        out.notices.push((
            Some(tell),
            Notice::Check {
                skill,
                roll,
                total,
                dc: shown,
                success: outcome == Outcome::Done,
            },
        ));
        let sprung = outcome == Outcome::Sprung;
        out.feedback.extend(
            crate::locks::floats(skill, roll, shown, outcome == Outcome::Done, sprung)
                .map(|words| (user.id, words)),
        );
        match (outcome, used.kind) {
            (Outcome::Done, UseKind::Recover) => {
                if let Some(row) = view.module.traps.get(usize::from(found.row))
                    && !row.kit.is_empty()
                {
                    state.make_item(library.items(), &row.kit, PARTY, 1, None);
                    goods = true;
                }
                gone(out, target.id);
            }
            (Outcome::Done, _) => disarmed(view, state, out, target.id, user.id),
            (Outcome::Sprung, _) => spring(view, library.items(), state, out, target, user, true),
            (Outcome::Failed, _) => {}
        }
    }
    goods
}
