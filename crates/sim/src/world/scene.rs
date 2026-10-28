//! Scene mode (`docs/DIALOGUE.md`): while a conversation holds
//! the world (S1), the area runs on KOTOR's rules rather than JA's. Every
//! creature fights as a scene's fighter, companions keep no formation, and
//! party members use nothing and roll no attacks (S4, `minds`, `interact`,
//! the scripts' queue). Nothing is paused, and nothing is restored when it
//! ends.

use bevy_ecs::prelude::*;
use script_ncs::vm::ObjectId;

use super::{Avatar, Id, Talking, acting_body};
use crate::input::{Seat, buttons};
use crate::script::dialogue::Told;
use crate::snapshot::SimId;

#[cfg(feature = "probes")]
pub mod probes;

/// Whether a conversation holds the world (S1): a party one, or one holding
/// every seat's body (`seat_bodies`, from `seat_bodies`).
pub(crate) fn held(talking: &Talking, seat_bodies: &[ObjectId]) -> bool {
    talking.0.holds_world(seat_bodies)
}

/// Whether a conversation holds the world this step (`held`), answered as
/// the step begins (`hold`) for every system that asks, the scripts' too
/// (`script::View::held`), and again after the scripts for their changes
/// (`scripts::apply`), so a jump made by a conversation's first line lands
/// by the held rule and one made by its last by the free one. Not state: a
/// client works it out from the snapshot's conversations as the authority
/// does.
#[derive(Resource)]
pub(crate) struct Held {
    pub world: bool,
    /// The host's two bodies (`host_bodies`), as the conversations count
    /// who takes part in them.
    host: [ObjectId; 2],
}

impl Default for Held {
    fn default() -> Held {
        Held {
            world: false,
            host: [ObjectId::INVALID; 2],
        }
    }
}

impl Held {
    /// Whether `id` takes part in a conversation under way (`Told::with`).
    pub(crate) fn talks(&self, talking: &Told, id: SimId) -> bool {
        talking.with(ObjectId(id.0), self.host).is_some()
    }

    /// Whether `id` plays by KOTOR's rules (S1, S4): every creature while
    /// a conversation holds the world, else those taking part in one, a
    /// private conversation's speakers too. The one gate of the minds'
    /// scene fighting, a blow's push, flinch and death in place: KOTOR
    /// pushes no one and plays its own dying clip, in a conversation or
    /// out of one, so a fighter in a conversation that holds no world
    /// (a co-op guest's, a creature's own) is never JA's.
    pub(crate) fn scene(&self, talking: &Told, id: SimId) -> bool {
        self.world || self.talks(talking, id)
    }
}

/// `Held` from the conversations as they stand and the bodies of the seats
/// (their avatars and the companion leading).
pub(super) fn hold(
    talking: Res<Talking>,
    scripts: Res<super::scripts::Scripts>,
    avatars: Query<(&Id, &Avatar)>,
    mut held: ResMut<Held>,
) {
    let lent = super::lent(&scripts.state.party);
    let seat_0 = avatars
        .iter()
        .find(|(_, avatar)| avatar.0 == super::LEADER_SEAT)
        .map(|(id, _)| id.0);
    held.host = super::host_bodies(seat_0, lent);
    if talking.0.conversations.is_empty() {
        held.world = false;
        return;
    }
    let seats = seat_bodies(avatars.iter().map(|(id, avatar)| (avatar.0, id.0)), lent);
    held.world = self::held(&talking, &seats);
}

/// Each seat's body as the scripts' `View::seats` has it: the one it acts
/// through (`acting_body`), from the bodies holding the seats' avatars and
/// the companion leading (`lent`). A watching seat has none.
pub(crate) fn seat_bodies(
    avatars: impl IntoIterator<Item = (Seat, SimId)>,
    lent: Option<SimId>,
) -> Vec<ObjectId> {
    avatars
        .into_iter()
        .map(|(seat, own)| ObjectId(acting_body(seat, own, lent).0))
        .collect()
}

/// What a party member's command loses while the world is held (S4): its
/// attacks, its powers and its throws (KOTOR rolls no party attack in a
/// conversation; its powers and throws [verify]). A script's
/// `CutsceneAttack` keeps them, its shots being the script's.
pub(crate) const ROLLS: u16 = buttons::ATTACK
    | buttons::ALT_ATTACK
    | buttons::FORCE_POWER
    | buttons::FORCE_GRIP
    | buttons::FORCE_LIGHTNING
    | buttons::FORCE_DRAIN
    | buttons::GRENADE;
