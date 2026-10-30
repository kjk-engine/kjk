//! The first seat playing another character (`SwitchPlayerCharacter`, K1
//! and TSL): the companion the party table names (`Party::switched`)
//! becomes the scripts' PC and the body the first seat moves, and the
//! seat's own character stays where it was, a character nobody plays,
//! until `NPC_PLAYER` switches back (K1's Leviathan escape: the cell's
//! prisoner stays behind while the one chosen escapes).
//!
//! The table is the truth; `sync`, first in `drivers::assign`, makes the
//! world follow it, so a switch a script makes moves the seat from the next
//! step on:
//! - the creature is driven by the first seat as a guest's companion is
//!   (`Mind::driver`, which the snapshot carries, and `Avatar`): the seat's
//!   commands move it, its mind stands down (but for the moves its
//!   scripts give it, `Mind::steered`), and what is keyed by seat (use,
//!   containers, locks, touches, exits) works through it. It follows the
//!   party's first seat and fights in the player's faction meanwhile;
//! - the seat's own character is `Benched`: no `Avatar`, so its mind keeps
//!   it, out of the party, still the snapshot's `Kind::Avatar` (a world
//!   made from a snapshot benches the avatar of a seat that drives a
//!   creature, so clients and saves need no more than the driver);
//! - switching back, or the creature gone, the creature's mind takes over
//!   again (in the party, or on its own if it is not a member) and the
//!   seat's own character is its body again; the table forgets a creature
//!   gone.
//!
//! Scripts see the switch at once, in the step that made it: `GetFirstPC`,
//! `GetIsPC`, `GetPCSpeaker` and the party's index 0 answer the creature
//! (`script::View::pcs`). A companion with no creature in the module is
//! made from its template beside the PC, as the party selection screen
//! makes those it calls (`script::engine`).
//!
//! `probes` checks it on the proving ground (`kjk sim probes --only
//! switch`).

use bevy_ecs::prelude::*;

use super::{Avatar, Benched, Fighting, Id, Inputs, LEADER_SEAT, Object, Player};
use crate::combat::sheet::factions;
use crate::snapshot::SimId;

/// Whether a body carrying `avatar` is a scripts' PC: a seat's own
/// character, or the creature the first seat is switched to (no guest
/// drives as the first seat), not a companion a guest drives.
pub(super) fn pc(avatar: &Avatar, object: Option<&Object>) -> bool {
    object.is_none() || avatar.0 == LEADER_SEAT
}

/// The world made to follow the party table's switch (see the module
/// documentation): exclusive, as it adds and removes `Avatar`.
pub(super) fn sync(world: &mut World) {
    let table = &world.resource::<super::scripts::Scripts>().state.party;
    let wanted = table.switched_body().map(SimId);
    // The companions' creatures, with whether each is in the party.
    let members: Vec<(u32, bool)> = table
        .available
        .iter()
        .filter_map(|(npc, slot)| Some((slot.object?, table.has(*npc))))
        .collect();
    let mut creatures =
        world.query_filtered::<(Entity, &Id, &Fighting), (With<Object>, With<Player>)>();
    let mut body = None;
    let mut riding = Vec::new();
    for (entity, id, fighting) in creatures.iter(world) {
        if Some(id.0) == wanted {
            body = Some(entity);
        }
        if fighting.0.mind.driver == Some(LEADER_SEAT) {
            riding.push((entity, id.0));
        }
    }
    // A switch to a companion with no creature (gone, or its slot gone)
    // is forgotten: nothing switches the seat into one made for it later.
    if body.is_none() {
        world
            .resource_mut::<super::scripts::Scripts>()
            .state
            .party
            .switched = None;
    }
    let angles = world
        .resource::<Inputs>()
        .0
        .get(LEADER_SEAT)
        .map(|command| command.angles);
    // Given back: its mind again, in the party or on its own.
    for (entity, id) in riding {
        if Some(entity) == body {
            continue;
        }
        let member = members.iter().any(|(of, member)| *of == id.0 && *member);
        let mut creature = world.entity_mut(entity);
        creature.remove::<Avatar>();
        if let Some(mut fighting) = creature.get_mut::<Fighting>() {
            fighting.0.mind.driver = None;
            super::scripts::join(&mut fighting.0, member);
        }
    }
    // Taken (from a guest driving it too) into the party and the player's
    // faction, and held in the party meanwhile (a script's
    // `RemovePartyMember` does not take the PC out of it; its
    // `ChangeToStandardFaction` stands). Driven even dead: KOTOR's game is
    // over then.
    if let Some(entity) = body {
        let mut creature = world.entity_mut(entity);
        let taken = creature
            .get::<Fighting>()
            .is_some_and(|fighting| fighting.0.mind.driver != Some(LEADER_SEAT));
        if let Some(mut fighting) = creature.get_mut::<Fighting>() {
            let fighter = &mut fighting.0;
            if taken {
                super::scripts::join(fighter, true);
            }
            fighter.mind.driver = Some(LEADER_SEAT);
            fighter.mind.leader = Some(LEADER_SEAT);
        }
        if taken {
            creature.insert(Avatar(LEADER_SEAT));
            look_on(&mut creature, angles);
            let npc = world
                .resource::<super::scripts::Scripts>()
                .state
                .party
                .switched;
            let mut kept = world.resource_mut::<super::drivers::Kept>();
            kept.0.retain(|kept| Some(kept.npc) != npc);
        }
    }
    // The seat's own character benched while it plays another, its body
    // again after.
    let mut own = world.query_filtered::<(Entity, Option<&Avatar>, Option<&Benched>), (Without<Object>, With<Player>)>();
    let seats: Vec<(Entity, bool)> = own
        .iter(world)
        .filter_map(|(entity, avatar, benched)| match (avatar, benched) {
            (Some(Avatar(seat)), _) if *seat == LEADER_SEAT => Some((entity, false)),
            (_, Some(Benched(seat))) if *seat == LEADER_SEAT => Some((entity, true)),
            _ => None,
        })
        .collect();
    for (entity, benched) in seats {
        let mut character = world.entity_mut(entity);
        match (body.is_some(), benched) {
            (true, false) => {
                character.remove::<Avatar>();
                character.insert(Benched(LEADER_SEAT));
                // It stays on the player's side, though nobody plays it.
                if let Some(mut fighting) = character.get_mut::<Fighting>() {
                    let faction = &mut fighting.0.faction;
                    *faction = faction.or(Some(factions::PLAYER));
                }
            }
            (false, true) => {
                character.remove::<Benched>();
                character.insert(Avatar(LEADER_SEAT));
                look_on(&mut character, angles);
            }
            _ => {}
        }
    }
}

impl super::SimWorld {
    /// The party table's switch made the world's at once, not on the next
    /// step: a module installed in the step a script switched back (TSL's
    /// prologue ends `SwitchPlayerCharacter(NPC_PLAYER)`, takes T3-M4 off
    /// the table and starts Peragus at once) carries the seat's own
    /// character, and the one given back stays if it is no member.
    pub(crate) fn settle_switch(&mut self) {
        sync(&mut self.world);
    }
}

/// A body a seat takes goes on looking where it looks; the seat's view
/// turns it from there (JA's `SetClientViewAngle` on a spawn).
pub(super) fn look_on(body: &mut EntityWorldMut, angles: Option<[i16; 3]>) {
    if let (Some(angles), Some(mut player)) = (angles, body.get_mut::<Player>()) {
        let ps = &mut player.0;
        ps.command_angles = angles;
        ps.set_view(ps.view);
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
