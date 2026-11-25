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
//!    (`Snapshot::players_driving`);
//! 4. a seat past the first can take a companion no player controls
//!    (`Action::Play`, its portrait click or leader key; `plays` lists
//!    when it is refused). Its choice is written to `Kept` before the
//!    rules above, so in the same step the seat lets its body go (its
//!    mind takes over) and takes the companion first. Two seats asking for
//!    one companion in a step are weighed in seat order, and the first
//!    seat's leader switch onto it (`Action::Lead`) comes before them all.
//!    A seat is never moved off a downed body on its own: only its own
//!    take moves it. A guest flying a droid takes a companion the same
//!    way, and the companion takes it off the droid (`remote::launch`).
//!    The first seat cannot lead through a companion a guest plays either
//!    (`scripts`' leader change refuses it).
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
/// The seats that never drive a companion, by bit (`SimWorld::set_phantoms`):
/// a test's guests that only fly droids (`kjk chain --droids chaos`). Not
/// state: whoever steps the world sets it.
#[derive(Resource, Default)]
pub(crate) struct Phantoms(pub u64);

impl super::SimWorld {
    /// The seats (bit n for seat n) that fly droids and never take a
    /// companion: the chain's chaos droids. Kept until the world is
    /// replaced (an install makes a new one).
    pub fn set_phantoms(&mut self, seats: u64) {
        self.world.resource_mut::<Phantoms>().0 = seats;
    }

    /// Holds `seats` seats (`Snapshot::seats_held`), as a host would: a
    /// test's phantoms' room. An install keeps it.
    pub fn set_seats_held(&mut self, seats: u8) {
        self.world.resource_mut::<SeatsHeld>().0 = seats.min(crate::MAX_SEATS);
    }
}

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

/// The companions the seats take this step (`Action::Play`): each seat
/// with the place in `members` of the companion it gets. A seat gets the
/// companion it asks for when all of these hold:
/// - the seat is not the first, the session holds it, it sent a command
///   this step, and it plays no character of its own;
/// - the companion is in the party, not dead, a companion of the party
///   table's, and no seat drives it;
/// - the first seat does not lead through it this step (`Action::Lead`);
/// - no lower seat took it this step, and the seat took no other this step
///   (the seats are weighed in seat order, each seat's actions as it sent
///   them);
/// - neither the companion nor the body the seat leaves is in a
///   conversation, a pazaak match or a store.
///
/// A companion kept for a seat that is away is free to take: its mind
/// plays it meanwhile. A downed body may be left for a standing companion,
/// and a downed companion may be taken.
fn plays(
    world: &World,
    inputs: &crate::input::TickInput,
    members: &[Member],
    players: &[Seat],
    held: u8,
) -> Vec<(Seat, usize)> {
    use crate::action::Action;
    let state = &world.resource::<super::scripts::Scripts>().state;
    let busy = |id: SimId| {
        let body = script_ncs::vm::ObjectId(id.0);
        state.talk.holding(body).is_some()
            || state.plays_pazaak(body)
            || state.watching(body).is_some()
            || state.trades.contains_key(&id.0)
    };
    // A party conversation holds every player, whether or not its
    // participants list a guest's companion.
    if state.talk.party().is_some() {
        return Vec::new();
    }
    // The bodies the first seat leads through this step, by body (`Lead`)
    // or by number (`SetLeader`): its switch comes before any take.
    let led: Vec<SimId> = inputs
        .actions()
        .iter()
        .filter(|(seat, _)| *seat == super::LEADER_SEAT)
        .filter_map(|(_, action)| match action {
            Action::Lead(body) => Some(*body),
            Action::SetLeader(npc) => members
                .iter()
                .find(|member| member.npc == Some(*npc))
                .map(|member| member.id),
            _ => None,
        })
        .collect();
    let mut granted: Vec<(Seat, usize)> = Vec::new();
    for (seat, action) in inputs.actions() {
        let (seat, Action::Play(body)) = (*seat, action) else {
            continue;
        };
        let Some(at) = members.iter().position(|member| member.id == *body) else {
            continue;
        };
        let wanted = &members[at];
        let asks = seat != super::LEADER_SEAT
            && seat.0 < held
            && inputs.get(seat).is_some()
            && !players.contains(&seat);
        let free = wanted.able
            && wanted.npc.is_some()
            && wanted.driver.is_none()
            && !led.contains(body)
            && !busy(wanted.id);
        let first = granted
            .iter()
            .all(|&(taker, taken)| taker != seat && taken != at);
        let leaving = members.iter().find(|member| member.driver == Some(seat));
        if asks && free && first && leaving.is_none_or(|member| !busy(member.id)) {
            granted.push((seat, at));
        }
    }
    granted
}

/// The assign system (see the module documentation): it adds and removes
/// `Avatar` itself, so it runs exclusively, before every other system. The
/// first seat's switch (`switch::sync`) first: the creature it plays is no
/// guest's to drive or keep.
pub(super) fn assign(world: &mut World) {
    super::switch::sync(world);
    let held = world.resource::<SeatsHeld>().0;
    let now = world.resource::<super::Clock>().time_ms;
    // A phantom seat speaks for no body: it only flies a droid.
    let phantoms = world.resource::<Phantoms>().0;
    let mut inputs = world.resource::<Inputs>().0.clone();
    for seat in (1..held).filter(|seat| phantoms & (1 << seat) != 0) {
        inputs.remove(Seat(seat));
    }
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
    // A seat taking a free companion: its choice is kept for it from now,
    // and no longer for a seat that is away, so the pick below gives it.
    let plays = plays(world, &inputs, &members, &players, held);
    for &(seat, at) in &plays {
        if let Some(npc) = members[at].npc {
            kept.retain(|kept| kept.seat != seat && kept.npc != npc);
            kept.push(crate::snapshot::Kept {
                seat,
                npc,
                at_ms: now,
            });
        }
    }
    // A driver stays while its creature can be driven and its seat, held
    // and playing no player of its own, spoke this step; one body a seat.
    let mut keeping: Vec<Seat> = Vec::new();
    let mut released = Vec::new();
    // The seats that let a body go, for their droids to launch from it.
    let mut freed: Vec<(Seat, Entity)> = Vec::new();
    // The seats whose creatures left the party, in seat order.
    let mut benched: Vec<Seat> = Vec::new();
    for member in &mut members {
        let Some(seat) = member.driver.filter(|seat| *seat != super::LEADER_SEAT) else {
            continue;
        };
        let takes = plays.iter().any(|(taker, _)| *taker == seat);
        let keeps = member.able
            && seat.0 < held
            && !players.contains(&seat)
            && !keeping.contains(&seat)
            && inputs.get(seat).is_some()
            && !takes;
        if keeps {
            keeping.push(seat);
        } else {
            if !member.follows {
                benched.push(seat);
            }
            member.driver = None;
            freed.push((seat, member.entity));
            // Why the seat let it go, for its conversation (R12): one the
            // host now leads it through goes on, the host's. A seat taking
            // another companion is in none (`plays`).
            let why = if member.dead {
                Some(Release::Died)
            } else if !member.follows {
                Some(Release::Left)
            } else if member.leads || takes {
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
    let launched = freed
        .into_iter()
        .filter_map(|(seat, entity)| {
            let origin = world.get::<Player>(entity)?.0.origin;
            Some((seat, crate::math::scale(origin, collision::UNIT)))
        })
        .collect();
    world.resource_mut::<super::remote::Launched>().0 = launched;
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
pub(in crate::world) mod probes;
#[cfg(feature = "probes")]
pub use probes::{droid_take, led_party, probes};
