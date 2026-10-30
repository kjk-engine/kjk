//! Seats to bodies (I30 P1–P5, I31; `docs/COOP.md`): which party member
//! each guest drives. A seat past the first plays a companion: its
//! creature stays an object and also carries `Avatar(seat)`, so what is
//! keyed by seat (moving, Use, containers, locks, touches, transitions)
//! works for it. The truth is the companion's `combat::ai::Mind::driver`,
//! which the snapshot carries; `SimWorld::from_snapshot` derives `Avatar`
//! from it.
//!
//! `assign`, first in each authority step (after the first seat's switch,
//! `switch::sync`, whose creature the first seat drives and no guest does):
//! 1. a creature loses its driver and its `Avatar`, and its mind takes
//!    over, when it left the party (`Mind::leader` none), is dead, or leads
//!    the party (`world::lent`: the host's portrait click or a script's
//!    `SetPartyLeader` makes it seat 0's body); or when its seat is not
//!    below the seats the session holds, plays an avatar of its own,
//!    already drives another creature (the first by id keeps it), or sent
//!    no command this step (the host's network gives a seat a command
//!    every tick while its connection lasts, its idle one through
//!    silences, holds, installs and loading, so a missing one is a seat
//!    that dropped: P5);
//! 2. then each seat past the first, below the seats the session holds,
//!    that sent a command and plays no body takes the first party creature
//!    (`Party::creatures`, joining order) no one drives and that does not
//!    lead, a standing one before a downed one (P2, P4), looking where it
//!    looks; with none free it watches (P3). The seats whose creatures left
//!    the party this step choose first (a companion benched and its
//!    replacement joining: P4), then the rest in seat order;
//! 3. a seat keeps the companion it drives (`Kept`, by `NPC_*` number, in
//!    the snapshot) for `KEEP_MS` after its last command: away, its
//!    companion's mind has it and no other seat takes it; back, the seat
//!    takes it first when it is free (a network blip, guests dropping and
//!    coming back in another order, a cutscene adding the companions again
//!    in another order). Planning counts the seat meanwhile
//!    (`Snapshot::players_driving`).
//!
//! `probes` checks it on the proving ground with two companions in the
//! party (`kjk sim probes`), and that a save made alone, hosted for three
//! (`Save::world`), gives its companions to the guests who join.

use bevy_ecs::prelude::*;

use super::{Avatar, Fighting, Id, Inputs, Object, Player, SeatsHeld};
use crate::combat::fighter::Life;
use crate::input::Seat;
use crate::script::dialogue::Release;
use crate::snapshot::SimId;

/// How long a companion stays kept for the seat that drove it after the
/// seat's last command (sim ms): the session's reconnect window (`net`'s
/// seat reservation).
pub const KEEP_MS: u64 = rules::party::seats::KEEP_MS;

/// The companions kept for their seats (`Snapshot::kept`).
#[derive(Resource, Default)]
pub(crate) struct Kept(pub Vec<crate::snapshot::Kept>);

/// A party creature as `assign` weighs it.
struct Member {
    entity: Entity,
    id: SimId,
    /// Its `NPC_*` number, if it is a companion of the party table's.
    npc: Option<u8>,
    driver: Option<Seat>,
    /// It follows the party (`Mind::leader`).
    follows: bool,
    /// It follows the party, is not dead and does not lead.
    able: bool,
    dead: bool,
    /// It leads the party (`super::lent`).
    leads: bool,
    /// It is not down.
    standing: bool,
}

/// The assign system (see the module documentation): it adds and removes
/// `Avatar` itself, so it runs exclusively, before every other system. The
/// first seat's switch (`switch::sync`) first: the creature it plays is no
/// guest's to drive or keep.
pub(super) fn assign(world: &mut World) {
    super::switch::sync(world);
    let held = world.resource::<SeatsHeld>().0;
    let now = world.resource::<super::Clock>().time_ms;
    let inputs = world.resource::<Inputs>().0.clone();
    let table = &world.resource::<super::scripts::Scripts>().state.party;
    let party: Vec<u32> = table.creatures().collect();
    // The companions' creatures, with their `NPC_*` numbers.
    let npcs: Vec<(u32, u8)> = table
        .available
        .iter()
        .filter_map(|(npc, slot)| Some((slot.object?, *npc)))
        .collect();
    let npc_of = |id: SimId| npcs.iter().find(|(of, _)| *of == id.0).map(|(_, npc)| *npc);
    // The companion leading the party is seat 0's body (`super::lent`).
    let lent = super::lent(table);
    let players: Vec<Seat> = world
        .query_filtered::<&Avatar, Without<Object>>()
        .iter(world)
        .map(|avatar| avatar.0)
        .collect();
    let mut members: Vec<Member> = world
        .query_filtered::<(Entity, &Id, &Fighting), (With<Object>, With<Player>)>()
        .iter(world)
        .map(|(entity, id, fighting)| {
            let follows = fighting.0.mind.leader.is_some();
            Member {
                entity,
                id: id.0,
                npc: npc_of(id.0),
                driver: fighting.0.mind.driver,
                follows,
                able: follows && fighting.0.life != Life::Dead && lent != Some(id.0),
                dead: fighting.0.life == Life::Dead,
                leads: lent == Some(id.0),
                standing: fighting.0.alive(),
            }
        })
        .collect();
    members.sort_by_key(|member| member.id);
    // Kept while the window lasts, for a seat the session holds.
    let mut kept = std::mem::take(&mut world.resource_mut::<Kept>().0);
    kept.retain(|kept| kept.seat.0 < held && now < kept.at_ms + KEEP_MS);
    // A driver stays while its creature can be driven and its seat, held
    // and playing no player of its own, spoke this step; one body a seat.
    let mut keeping: Vec<Seat> = Vec::new();
    let mut released = Vec::new();
    // The seats whose creatures left the party, in seat order.
    let mut benched: Vec<Seat> = Vec::new();
    for member in &mut members {
        let Some(seat) = member.driver.filter(|seat| *seat != super::LEADER_SEAT) else {
            continue;
        };
        let keeps = member.able
            && seat.0 < held
            && !players.contains(&seat)
            && !keeping.contains(&seat)
            && inputs.get(seat).is_some();
        if keeps {
            keeping.push(seat);
        } else {
            if !member.follows {
                benched.push(seat);
            }
            member.driver = None;
            // Why the seat let it go, for its conversation (R12): one the
            // host now leads it through goes on, the host's.
            let why = if member.dead {
                Some(Release::Died)
            } else if !member.follows {
                Some(Release::Left)
            } else if member.leads {
                None
            } else {
                Some(Release::Dropped)
            };
            released.push((member.entity, member.id, why));
        }
    }
    benched.sort();
    benched.dedup();
    let rest = (1..held).map(Seat).filter(|seat| !benched.contains(seat));
    let order: Vec<Seat> = benched.iter().copied().chain(rest).collect();
    let mut taken = Vec::new();
    for seat in order {
        let Some(command) = inputs.get(seat) else {
            continue;
        };
        if seat.0 >= held || players.contains(&seat) || keeping.contains(&seat) {
            continue;
        }
        let mine = kept.iter().find(|k| k.seat == seat).map(|k| k.npc);
        // Free: in the party, able, driven by nobody, and kept for no
        // other seat.
        let free = |member: &Member| {
            member.able
                && member.driver.is_none()
                && member
                    .npc
                    .is_none_or(|npc| !kept.iter().any(|k| k.npc == npc && k.seat != seat))
        };
        let pick = |wanted: &dyn Fn(&Member) -> bool| {
            party.iter().find_map(|object| {
                members
                    .iter()
                    .position(|member| member.id.0 == *object && free(member) && wanted(member))
            })
        };
        let at = pick(&|member| mine.is_some() && member.npc == mine)
            .or_else(|| pick(&|member| member.standing))
            .or_else(|| pick(&|_| true));
        if let Some(at) = at {
            members[at].driver = Some(seat);
            taken.push((members[at].entity, seat, command.angles));
        }
    }
    // The seats driving now keep their companions from now.
    for member in &members {
        if let (Some(seat), Some(npc)) = (member.driver, member.npc)
            && seat != super::LEADER_SEAT
        {
            kept.retain(|kept| kept.seat != seat);
            kept.push(crate::snapshot::Kept {
                seat,
                npc,
                at_ms: now,
            });
        }
    }
    kept.sort_by_key(|kept| kept.seat);
    world.resource_mut::<Kept>().0 = kept;
    // A released body's conversation is interrupted, its match paused or
    // ended and its trade ended (R12) in the step's dialogue and pazaak
    // passes.
    let mut scripts = world.resource_mut::<super::scripts::Scripts>();
    let state = &mut scripts.state;
    for &(_, id, why) in &released {
        let body = script_ncs::vm::ObjectId(id.0);
        let busy = state.talk.holding(body).is_some()
            || state.plays_pazaak(body)
            || state.trades.contains_key(&id.0);
        if let Some(why) = why
            && busy
        {
            state.talk.released.push((body, why));
        }
    }
    for (entity, ..) in released {
        let mut body = world.entity_mut(entity);
        body.remove::<Avatar>();
        if let Some(mut fighting) = body.get_mut::<Fighting>() {
            fighting.0.mind.driver = None;
        }
    }
    for (entity, seat, angles) in taken {
        let mut body = world.entity_mut(entity);
        body.insert(Avatar(seat));
        if let Some(mut fighting) = body.get_mut::<Fighting>() {
            fighting.0.mind.driver = Some(seat);
        }
        super::switch::look_on(&mut body, Some(angles));
    }
}

#[cfg(feature = "probes")]
#[allow(
    clippy::disallowed_methods,
    reason = "checks and traces set up their own cases on one machine"
)]
mod probes;
#[cfg(feature = "probes")]
pub use probes::probes;
