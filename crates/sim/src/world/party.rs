//! The party as one (`docs/PARTY.md`): whether it is in combat, a break a
//! player called, and the ready check for an exit a player walked into.
//! All three ride in the snapshot (`Snapshot::in_combat`, `paused_by`,
//! `ready`).
//!
//! - **In combat** (`combat`, after the hurts, every step): a living
//!   character outside the party is fighting (a target, engaging or
//!   chasing, or a blow within `COMBAT_MS`) within `ranges.2da`'s
//!   PartyCombat of a party member who is not dead and to whom it is
//!   hostile (`keepers`); or, with friendly fire on, a party member struck
//!   another within `COMBAT_MS` (a duel is a fight) (`in_combat`).
//! - **A break** (`breaks`, the step's first system): any seat calls one
//!   (`Action::Break`) unless the party is in combat or one holds (a break
//!   called during another is dropped); its caller or seat 0 ends it
//!   (`Resume`). While it holds, a step moves its tick and nothing else
//!   (`running` gates the rest).
//! - **The ready check** (`ready`, after `interact`): a walk into an exit
//!   (or a use of an exit door) proposes the move; a seat is ready by
//!   walking into or using the exit, standing at it, or with `Ready`, and
//!   a seat with no body counts as ready; the party goes when
//!   everyone is, or when seat 0 forces it after `FORCE_AFTER_MS`. In
//!   combat a walk in is refused (told once) and a standing proposal waits.
//!   The asker gone, the lowest seat still at the exit asks; with no one
//!   there it lapses. A script's new module skips it. A seat whose body
//!   stands in an unlocked conversation, trades or watches a match leaves
//!   it as it readies (R11, R9a); one in a locked conversation or a match
//!   cannot ready, and, while more than one seat plays a body, stays
//!   unready (`kept`).

use bevy_ecs::prelude::*;
use collision::UNIT;

use super::{Avatar, Clock, Fighting, Id, InCombat, Object, Player, StepReason};
use crate::action::Action;
use crate::combat::FriendlyFire;
use crate::input::Seat;
use crate::math::{self, Vec3};
use crate::module::ObjectKind;
use crate::motion::MotionState;
use crate::notice::Notice;
use crate::snapshot::{ReadyCheck, SimId};
use crate::world::Level;

/// The party's calls but the fight (`super::InCombat`): restored from a
/// snapshot, written by the step.
#[derive(Resource, Default)]
pub(super) struct PartyState {
    pub(super) paused_by: Option<Seat>,
    pub(super) ready: Option<ReadyCheck>,
}

/// A character as the in-combat test sees it.
#[derive(Debug, Clone, Copy)]
pub struct Combatant {
    pub id: SimId,
    /// Units.
    pub at: Vec3,
    /// A player, or a companion of one (`world::in_party`).
    pub party: bool,
    pub faction: u8,
    pub life: crate::combat::fighter::Life,
    pub target: Option<SimId>,
    pub task: crate::combat::ai::Task,
    pub combat_until: u64,
    /// The last to hurt it, and when (sim ms: `Fighter::last_attacker`).
    pub struck_by: Option<(SimId, u64)>,
    /// It cannot fall (its sheet's `PLOT`).
    pub plot: bool,
}

impl Combatant {
    pub(super) fn side(&self) -> crate::module::Side {
        crate::module::Side {
            id: self.id.0,
            faction: self.faction,
            party: self.party,
        }
    }
}

/// What the in-combat test reads of the level: the module's factions (none
/// without a module), whether the party may hurt itself, and how far a
/// fight reaches the party (units: `ranges.2da` PartyCombat,
/// `ModuleData::ranges`).
#[derive(Debug, Clone, Copy)]
pub struct Rules<'a> {
    pub repute: Option<crate::module::Standing<'a>>,
    pub friendly_fire: FriendlyFire,
    pub reach: f32,
}

impl Rules<'_> {
    /// How far a fight reaches the party where a level has no table (K1's
    /// PartyCombat, units).
    pub const REACH: f32 = crate::module::Ranges::K1.party_combat_m / UNIT;
}

/// The characters keeping the party in combat (KOTOR's "enemies nearby",
/// `docs/PARTY.md`): each living one outside the party that is fighting
/// (it has a target, engages or chases one, or struck or was struck within
/// `COMBAT_MS`) within `Rules::reach` of a party member who is not dead
/// and hostile to that member (`module::relation`). Never one that cannot
/// fall (plot: the Sand People's turrets on their enclave's walls): no
/// fight the party could end [verify: KOTOR's own test].
pub fn keepers<'a>(
    characters: &'a [Combatant],
    rules: Rules<'_>,
    now: u64,
) -> impl Iterator<Item = &'a Combatant> {
    use crate::combat::ai::Task;
    use crate::combat::fighter::Life;
    let members: Vec<&Combatant> = characters
        .iter()
        .filter(|c| c.party && c.life != Life::Dead)
        .collect();
    characters.iter().filter(move |c| {
        let fighting = c.target.is_some()
            || matches!(c.task, Task::Engage | Task::Chase)
            || c.combat_until > now;
        !c.party
            && !c.plot
            && c.life == Life::Alive
            && fighting
            && rules.repute.is_some()
            && members.iter().any(|member| threatens(rules, c, member))
    })
}

/// Whether `foe` is hostile to party member `member` and within
/// `Rules::reach` of it.
fn threatens(rules: Rules<'_>, foe: &Combatant, member: &Combatant) -> bool {
    crate::module::relation(rules.repute, foe.side(), member.side())
        == crate::combat::Relation::Hostile
        && math::length(math::sub(member.at, foe.at)) <= rules.reach
}

/// The party members a fight is near (R34, `script::pazaak`'s watching):
/// each that one of `keepers` (the characters keeping the party in combat,
/// as `keepers` finds them) threatens, hostile to it within
/// `Rules::reach`. The party's combat, member by member.
pub fn embattled(characters: &[Combatant], keepers: &[&Combatant], rules: Rules<'_>) -> Vec<SimId> {
    use crate::combat::fighter::Life;
    let mut near: Vec<SimId> = characters
        .iter()
        .filter(|c| c.party && c.life != Life::Dead)
        .filter(|member| keepers.iter().any(|foe| threatens(rules, foe, member)))
        .map(|member| member.id)
        .collect();
    near.sort();
    near
}

/// Whether the party is in combat, one rule for the whole party: someone
/// keeps it in combat (`keepers`); or, with friendly fire on, a party
/// member another party member struck within `COMBAT_MS` (a duel).
pub fn in_combat(characters: &[Combatant], rules: Rules<'_>, now: u64) -> bool {
    keepers(characters, rules, now).next().is_some() || duel(characters, rules, now)
}

/// With friendly fire on, a party member another party member struck
/// within `COMBAT_MS` (`in_combat`'s duel).
fn duel(characters: &[Combatant], rules: Rules<'_>, now: u64) -> bool {
    use crate::combat::fighter::Life;
    rules.friendly_fire != FriendlyFire::Off
        && (characters.iter())
            .filter(|c| c.party && c.life != Life::Dead)
            .any(|member| {
                member.struck_by.is_some_and(|(by, at)| {
                    by != member.id
                        && now < at + super::COMBAT_MS
                        && characters.iter().any(|c| c.id == by && c.party)
                })
            })
}

/// The characters as the in-combat test sees them.
#[allow(clippy::type_complexity)]
pub(super) fn combatants<'a>(
    actors: impl Iterator<
        Item = (
            &'a Id,
            Option<&'a Avatar>,
            Option<&'a Object>,
            Option<&'a super::PlayerSheet>,
            &'a Player,
            &'a Fighting,
        ),
    >,
) -> Vec<Combatant> {
    actors
        .map(|(id, avatar, object, sheet, player, fighting)| {
            let fighter = &fighting.0;
            let sheet = super::sheet_of(sheet, object);
            Combatant {
                id: id.0,
                at: player.0.origin,
                party: super::in_party(avatar, fighter),
                faction: super::faction_of(fighter, &sheet),
                life: fighter.life,
                target: fighter.target,
                task: fighter.mind.task,
                combat_until: fighter.combat_until,
                struck_by: fighter.last_attacker,
                plot: sheet.flags & crate::combat::sheet::flags::PLOT != 0,
            }
        })
        .collect()
}

/// The in-combat test's rules for `level` as `scripts` left the factions.
pub(super) fn rules<'a>(level: &'a Level, scripts: &'a super::scripts::Scripts) -> Rules<'a> {
    let module = level.module();
    Rules {
        repute: module.map(|module| module.repute.with(&scripts.state.reputation)),
        friendly_fire: level.friendly_fire(),
        reach: module.map_or(Rules::REACH, |module| module.ranges.party_combat_m / UNIT),
    }
}

/// The party members a fight is near (`embattled`), by id, as the step's
/// in-combat test leaves them: not state.
#[derive(Resource, Default)]
pub(super) struct Embattled(pub(super) Vec<SimId>);

/// The in-combat test, after the step's hurts, every step (prediction
/// too: it is a function of the state), and who of the party a fight is
/// near. Order does not matter: it is an `any`.
#[allow(clippy::type_complexity)]
pub(super) fn combat(
    clock: Res<Clock>,
    level: Res<super::Static>,
    scripts: Res<super::scripts::Scripts>,
    (mut fight, mut embattled): (ResMut<InCombat>, ResMut<Embattled>),
    actors: Query<(
        &Id,
        Option<&Avatar>,
        Option<&Object>,
        Option<&super::PlayerSheet>,
        &Player,
        &Fighting,
    )>,
) {
    let characters = combatants(actors.iter());
    let (rules, now) = (rules(&level.0, &scripts), clock.time_ms);
    // `in_combat` and `embattled` share one pass over the keepers.
    let keepers: Vec<&Combatant> = keepers(&characters, rules, now).collect();
    fight.0 = !keepers.is_empty() || duel(&characters, rules, now);
    let near = self::embattled(&characters, &keepers, rules);
    if embattled.0 != near {
        embattled.0 = near;
    }
}

impl super::SimWorld {
    /// The characters keeping the party in combat now (`keepers`), by id:
    /// what `Snapshot::in_combat` is true for, for a driver or a report to
    /// name them.
    pub fn combat_keepers(&self) -> Vec<SimId> {
        let time_ms = self.world.resource::<Clock>().time_ms;
        let Some(mut query) = self.world.try_query::<(
            &Id,
            Option<&Avatar>,
            Option<&Object>,
            Option<&super::PlayerSheet>,
            &Player,
            &Fighting,
        )>() else {
            return Vec::new();
        };
        let mut characters = combatants(query.iter(&self.world));
        characters.sort_by_key(|c| c.id);
        let level = &self.world.resource::<super::Static>().0;
        let scripts = self.world.resource::<super::scripts::Scripts>();
        keepers(&characters, rules(level, scripts), time_ms)
            .map(|c| c.id)
            .collect()
    }
}

/// Whether `seat` may end a break `by` called: the caller, or the host's
/// seat.
pub fn may_resume(by: Seat, seat: Seat) -> bool {
    seat == by || seat == Seat(0)
}

/// The step runs: no break holds.
pub(super) fn running(party: Res<PartyState>) -> bool {
    party.paused_by.is_none()
}

/// Breaks called and ended (the authority's actions, in seat order), as
/// the previous step left the fight. While one holds, the step's clock
/// stands still and the rest of the step does not run (`running`): a
/// paused step changes its tick and nothing else, but that the module it
/// asked for last step is asked no more (an ask lasts one step). A break
/// called while one held when the step began (pressed during it, stepped
/// with its resume) is dropped: the world runs on after the resume.
pub(super) fn breaks(
    mut clock: ResMut<Clock>,
    inputs: Res<super::Inputs>,
    fight: Res<InCombat>,
    mut party: ResMut<PartyState>,
    mut module: ResMut<super::Module>,
    mut notices: ResMut<crate::notice::Notices>,
) {
    let held = party.paused_by.is_some();
    if clock.reason != StepReason::Predict {
        for (seat, action) in inputs.0.actions() {
            let seat = *seat;
            match (action, party.paused_by) {
                (Action::Break, _) if held => {}
                (Action::Break, None) if fight.0 => notices.0.push((seat, Notice::InCombat)),
                (Action::Break, None) => party.paused_by = Some(seat),
                (Action::Resume, Some(by)) if may_resume(by, seat) => party.paused_by = None,
                (Action::Resume, Some(_)) => notices.0.push((seat, Notice::NotYourBreak)),
                _ => {}
            }
        }
    }
    if party.paused_by.is_some() {
        clock.time_ms -= u64::from(clock.msec);
        clock.msec = 0;
        module.pending = None;
    }
}

/// How long a proposal stands before seat 0 may force it (ms; P16).
pub const FORCE_AFTER_MS: u64 = rules::party::exits::FORCE_AFTER_MS;

/// What a player's body is busy with, in the words the screens use (R11):
/// "Carth is in a conversation.", "waiting for player 2 (playing pazaak)".
pub const IN_CONVERSATION: &str = "in a conversation";
pub const PLAYING_PAZAAK: &str = "playing pazaak";

/// Why `body` cannot ready for an exit (R11), in the screens' words: it
/// stands in a locked conversation of its own (or joined), a party one
/// holds it (every seat but one standing in its own, R5; never its
/// unlocked listener, who may still leave it, R6), or it plays a match.
/// One in an unlocked conversation or a store leaves it as it readies
/// (`ready`).
pub fn kept(
    conversations: &[crate::script::dialogue::Conversation],
    pazaak: &[crate::pazaak::Match],
    body: SimId,
) -> Option<&'static str> {
    let id = script_ncs::vm::ObjectId(body.0);
    let own = conversations.iter().find(|c| !c.party && c.holds(id));
    let talking = match own {
        Some(c) => c.locked,
        None => conversations
            .iter()
            .any(|c| c.party && (c.locked || c.listener != id)),
    };
    if talking {
        Some(IN_CONVERSATION)
    } else if pazaak.iter().any(|table| table.player == body.0) {
        Some(PLAYING_PAZAAK)
    } else {
        None
    }
}

/// The seats whose ready this step leaves the unlocked conversation or the
/// store their bodies are in (R11), with those bodies: the scripts' step
/// leaves them (`world::scripts`). Not state.
#[derive(Resource, Default)]
pub(super) struct Leaving(pub(super) Vec<(Seat, SimId)>);

/// A walk into an exit this step (`interact`, in seat order): the move it
/// asks for, and the exit's object index when entering made the seat its
/// tenant (a trigger, an open doorway), so it can be undone; none for a
/// door run into or used.
pub(super) struct Ask {
    pub(super) transition: crate::snapshot::Transition,
    pub(super) entered: Option<u32>,
}

/// The step's walks into exits. Not state.
#[derive(Resource, Default)]
pub(super) struct Asked(pub(super) Vec<Ask>);

/// The seats told this stay that an exit is refused in combat: told once
/// while they keep walking into it. Not state: a restore tells again at
/// worst.
#[derive(Resource, Default)]
pub(super) struct Refused(u64);

/// Seat `seat`'s bit in a set of seats (none past `MAX_SEATS`: a garbage
/// seat in an action names nobody).
pub fn bit(seat: Seat) -> u64 {
    1u64.checked_shl(u32::from(seat.0)).unwrap_or(0)
}

/// Whether `body`, seat `seat`'s, stands at exit `object` in `level`: a
/// trigger's or an open doorway's tenant, or within a door's reach
/// (`script::DOOR_REACH`) of the door's shape, open or closed, wherever
/// along it (a door is run into or used, not stood in).
fn at_exit(level: &Level, object: &Object, seat: Seat, body: &MotionState) -> bool {
    let tenant = object.state.tenants & bit(seat) != 0;
    match object.kind {
        ObjectKind::Trigger => tenant,
        ObjectKind::Door => {
            tenant
                || super::near_door(
                    level,
                    object.index,
                    body.origin,
                    body.maxs_z,
                    crate::script::DOOR_REACH / UNIT,
                )
        }
        _ => false,
    }
}

/// Undoes `seat`'s entry into exit `index` (its tenancy): it asks again
/// next step while it stays.
fn leave(objects: &mut Query<(&Id, &mut Object)>, index: u32, seat: Seat) {
    if let Some((_, mut object)) = objects.iter_mut().find(|(_, object)| object.index == index) {
        object.state.tenants &= !bit(seat);
    }
}

/// The ready check (P16; the module docs). Every step, every reason: the
/// proposal follows the bodies as prediction sees them; only the authority
/// takes actions, tells seats and asks for the transition.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(super) fn ready(
    clock: Res<Clock>,
    inputs: Res<super::Inputs>,
    fight: Res<InCombat>,
    (mut asked, mut refused): (ResMut<Asked>, ResMut<Refused>),
    mut party: ResMut<PartyState>,
    mut module: ResMut<super::Module>,
    mut notices: ResMut<crate::notice::Notices>,
    level: Res<super::Static>,
    (scripts, talking, holding, tabled, mut leaving): (
        Res<super::scripts::Scripts>,
        Res<super::Talking>,
        Res<super::Holding>,
        Res<super::Tabled>,
        ResMut<Leaving>,
    ),
    bodies: Query<(&Avatar, &Id, &Player, &Fighting)>,
    mut objects: Query<(&Id, &mut Object)>,
) {
    let authority = clock.reason != StepReason::Predict;
    let (now, in_combat) = (clock.time_ms, fight.0);
    // The seats with a living body, in seat order, with the body each acts
    // through (`acting_body`): the rest are watching, and count as ready.
    let lent = super::lent(&scripts.state.party);
    let mut standing: Vec<(Seat, SimId, MotionState)> = bodies
        .iter()
        .filter(|(.., fighting)| fighting.0.alive())
        .map(|(avatar, id, player, _)| {
            let body = super::acting_body(avatar.0, id.0, lent);
            (avatar.0, body, player.0)
        })
        .collect();
    standing.sort_by_key(|(seat, ..)| *seat);
    let needed = standing
        .iter()
        .fold(0, |bits, (seat, ..)| bits | bit(*seat));
    // The seats whose bodies cannot ready (`kept`), and whose ready leaves
    // what they are in, only while more than one seat plays a body (R11):
    // one player goes as before. Read as the last step's scripts left them.
    let together = standing.len() > 1;
    let talk = &talking.0;
    let kept_seats = standing
        .iter()
        .filter(|(_, body, _)| together && kept(&talk.conversations, &tabled.0, *body).is_some())
        .fold(0, |bits, (seat, ..)| bits | bit(*seat));
    let mut check = party.ready.take();
    // Walks into exits: the first proposes; one into the proposal's exit
    // is readiness (below); one into another exit, or any in combat, is
    // undone so it asks again (in combat told once).
    let mut asking = 0;
    // The exits walked into (or used) and not undone, by whom: each is at
    // that exit this step, wherever its body stands.
    let mut walked: Vec<(SimId, Seat)> = Vec::new();
    for Ask {
        transition,
        entered,
    } in std::mem::take(&mut asked.0)
    {
        let seat = transition.seat;
        asking |= bit(seat);
        if check
            .as_ref()
            .is_some_and(|check| check.via == transition.via)
        {
            walked.push((transition.via, seat));
            continue;
        }
        if in_combat || check.is_some() {
            if let Some(index) = entered {
                leave(&mut objects, index, seat);
            }
            if in_combat && authority && refused.0 & bit(seat) == 0 {
                notices.0.push((seat, Notice::InCombat));
            }
            continue;
        }
        walked.push((transition.via, seat));
        check = Some(ReadyCheck {
            module: transition.module,
            tag: transition.tag,
            via: transition.via,
            asked_by: seat,
            ready: bit(seat),
            since_ms: now,
        });
    }
    refused.0 = if in_combat { asking } else { 0 };
    // Everyone at the exit is ready; the asker gone, the lowest still there
    // asks; with no one there it lapses.
    check = check.and_then(|mut proposal| {
        let exit = objects
            .iter()
            .find(|(id, _)| id.0 == proposal.via)
            .map(|(_, object)| object)?;
        let there: Vec<Seat> = standing
            .iter()
            .filter(|(seat, _, body)| {
                walked.contains(&(proposal.via, *seat)) || at_exit(&level.0, exit, *seat, body)
            })
            .map(|(seat, ..)| *seat)
            .collect();
        let first = *there.first()?;
        for seat in &there {
            proposal.ready |= bit(*seat);
        }
        if !there.contains(&proposal.asked_by) {
            proposal.asked_by = first;
        }
        Some(proposal)
    });
    // Whether `body` stands in a conversation, trades or watches a match
    // (R9a): its ready leaves what it is in.
    let busy = |body: SimId| {
        together
            && (talk.holding(script_ncs::vm::ObjectId(body.0)).is_some()
                || holding.0.trade(body.0).is_some()
                || tabled
                    .0
                    .iter()
                    .any(|table| table.watchers.contains(&body.0)))
    };
    let mut forced = false;
    if authority {
        for (seat, action) in inputs.0.actions() {
            let seat = *seat;
            match (action, check.as_mut()) {
                (Action::Ready, Some(_)) if kept_seats & bit(seat) != 0 => {}
                (Action::Ready, Some(proposal)) => {
                    proposal.ready |= bit(seat);
                    let body = standing
                        .iter()
                        .find(|(of, body, _)| *of == seat && busy(*body));
                    if let Some(&(_, body, _)) = body {
                        leaving.0.push((seat, body));
                    }
                }
                (Action::Force, _) if in_combat => notices.0.push((seat, Notice::InCombat)),
                (Action::Force, Some(proposal))
                    if seat == Seat(0) && now >= proposal.since_ms + FORCE_AFTER_MS =>
                {
                    forced = true;
                }
                (Action::Force, _) => notices.0.push((seat, Notice::CannotForce)),
                _ => {}
            }
        }
    }
    // A seat that cannot ready stays unready however it was.
    if let Some(proposal) = check.as_mut() {
        proposal.ready &= !kept_seats;
    }
    // Everyone ready (or forced) and no fight: the party goes.
    party.ready = match check {
        Some(proposal) if !in_combat && (forced || needed & !proposal.ready == 0) => {
            if authority {
                module.pending = Some(crate::snapshot::Transition {
                    seat: proposal.asked_by,
                    via: proposal.via,
                    module: proposal.module,
                    tag: proposal.tag,
                });
            }
            None
        }
        check => check,
    };
}

#[cfg(feature = "probes")]
mod probes;
#[cfg(feature = "probes")]
pub use probes::probes;
