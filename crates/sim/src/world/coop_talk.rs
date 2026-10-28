//! Conversations in co-op (`docs/DIALOGUE.md`): the party gathered to a
//! conversation as it begins (`gather`), and the probes of the whole, on
//! the proving ground with a host, a guest driving a
//! companion and a guest watching (later driving a second one). They
//! cover:
//! - every seat's markers, and who answers a party conversation: the host
//!   everything, the guest all but the story's replies, the first answer
//!   winning;
//! - a player's use talking alone: only its body held, its screen showing
//!   it, its own AI companions brought; another player walking up joins;
//!   the asker carried by each use (a console's `OnUsed`, two players at
//!   one NPC in a step);
//! - a party conversation (the game's, or a cutscene's) holding every body
//!   and every screen, cutting the players' own short;
//! - a bark holding nobody;
//! - a guest's use talking with the host's character as the PC;
//! - the store that conversation opens, opening for whoever asked; two
//!   players trading at one store in seat order; a store closed by damage
//!   or a party conversation's gather, and when its player's seat lets go;
//! - pazaak: a match for the guest alone, beside another's conversation,
//!   paused by a party conversation, a hurt or a drop (each pause ending
//!   with its cause, a paused match saved and loaded alike), quit by a
//!   death or by leaving the party (two matches at once and the module's
//!   quits are `pazaak::probes`');
//! - watching a match (R9a): free players near it as it begins (not one
//!   beside a fight), those joined to the conversation dealing it, its
//!   listener once it ends normally (a guest dealt in the host's), a use
//!   of its player (the companion the host leads too; one not free passes
//!   to the dealer) or `Action::Watch`, watchers held and not busy, kept
//!   while the match is paused, and what stops them (Leave, alone, the
//!   host's leading through a companion too, a hurt, a fight reaching
//!   them, a cutscene, a ready, a deal of their own); a joined player
//!   stepping out to watch taking its mark;
//! - a guest's pick of a story reply in its own conversation asking the
//!   host in, the host accepting or ending it, out of combat and outside
//!   a locked conversation of its own;
//! - leaving, and what locks a conversation;
//! - a companion's line bringing its player in where it stands, or
//!   waiting on its player's locked conversation, `INTERJECTION_WAIT_MS`
//!   at most;
//! - damage cutting a listener's own conversation short unless its owner
//!   is not interruptable; a dropped guest's cut short, or handed to the
//!   host when locked (waiting on the host's own locked one); a dead
//!   listener's cut short; a guest's own cut short by a module change; a
//!   busy companion not benched;
//! - an exit's ready check leaving an unlocked conversation or a store and
//!   waiting on a locked one or a match;
//! - what the screens read: each seat's busy icon, the Leave row's state,
//!   whether a story pick asks the host or a seat takes the story's
//!   replies, why the request keys wait;
//! - one seat alone playing as before: its companion gathered, talking on
//!   and shopping on when hurt, its match never paused nor watched.

use bevy_ecs::prelude::*;
use collision::UNIT;

use super::{Id, Level, Player};
use crate::math::{self, scale};
use crate::module::Spot;
use crate::motion::{self, MotionState};
use crate::snapshot::SimId;

/// How far from where a conversation is held a party member must stand
/// to be brought to it as it begins (metres). [verify] against KOTOR,
/// which brings its party into a conversation's scene. A free player this
/// near a match's player as it begins watches it (R9a).
pub(crate) const GATHER_BEYOND_M: f32 = rules::dialogue::coop::GATHER_BEYOND_M;

/// A conversation began around `around` (I30 P7, R1).
/// - Each of `members` (those it brings, `script::Gathered`) farther than
///   `GATHER_BEYOND_M` from it stands still at a free place near it:
///   `install::spots` around it, starting behind it (not between it and
///   whom it talks to), clear of every character and in its sight.
/// - Each of them is turned to it, in id order, nearest place first.
/// - Those already near stay where they are.
/// - Where there is no room for all of them, nobody moves.
pub(super) fn gather(
    level: &Level,
    states: Option<&[u8]>,
    around: SimId,
    members: &[SimId],
    players: &mut Query<(&Id, &mut Player)>,
) {
    let mut bodies: Vec<(SimId, MotionState)> = players
        .iter()
        .map(|(id, player)| (id.0, player.0))
        .collect();
    bodies.sort_by_key(|(id, _)| *id);
    let Some(centre) = bodies
        .iter()
        .find(|(id, _)| *id == around)
        .map(|(_, at)| *at)
    else {
        return;
    };
    let beyond = GATHER_BEYOND_M / UNIT;
    let coming: Vec<SimId> = bodies
        .iter()
        .filter(|(id, at)| {
            let d = math::sub(at.origin, centre.origin);
            members.contains(id) && d[0] * d[0] + d[1] * d[1] + d[2] * d[2] > beyond * beyond
        })
        .map(|(id, _)| *id)
        .collect();
    if coming.is_empty() {
        return;
    }
    let staying: Vec<([f32; 3], f32)> = bodies
        .iter()
        .filter(|(id, _)| !coming.contains(id))
        .map(|(_, at)| (at.origin, crate::install::APART))
        .collect();
    let mut soles = centre.origin;
    soles[2] += motion::SOLE_Z;
    let spot = Spot {
        position: scale(soles, UNIT),
        yaw: centre.view[1] + 180.0,
    };
    let places = level.with(states, |world| {
        crate::install::spots(
            world,
            &spot,
            coming.len(),
            &staying,
            Some(centre.origin),
            crate::install::RINGS,
        )
    });
    let Some(places) = places else {
        return;
    };
    for (id, place) in coming.iter().zip(places) {
        if let Some((_, mut player)) = players.iter_mut().find(|(of, _)| of.0 == *id) {
            let ps = &mut player.0;
            ps.origin = place;
            ps.velocity = [0.0; 3];
            ps.view[1] = math::atan2_deg(centre.origin[1] - place[1], centre.origin[0] - place[0]);
        }
    }
}

#[cfg(feature = "probes")]
mod probes;
#[cfg(feature = "probes")]
pub use probes::probes;
