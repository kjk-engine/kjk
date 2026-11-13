//! Stealth and the skill buttons on the authority
//! (`docs/COMBAT.md`).
//!
//! Each seat's new presses of `buttons::STEALTH` and `buttons::LAY_MINE`
//! this step are kept for the scripts' step (`Presses`, `world::traps`
//! lays a mine). A seat's keys act through the body it controls
//! (`keyed_by`): for the first seat the companion leading the party, else
//! the seat's own body. The seat is told what happened. A companion that
//! stays in stealth after its player switched away keeps the rules that
//! end stealth, and the seat it follows is told when it ends.
//! A stealth press turns stealth mode off, or on: refused,
//! in the game's line, with no base Stealth ranks, no stealth unit worn
//! (`flags::STEALTH_UNIT`), in combat, or in the player restrict mode
//! (`SetPlayerRestrictMode`). Stealth ends on an attack, a power or a
//! grenade (opening `stealth::OPENER_MS`'s opener), on combat, on the unit
//! taken off, and on falling; a conversation beginning ends it too
//! (`script::dialogue`, `Change::Unstealth`). While it holds, the body walks
//! (KOTOR's "Run Disabled": `move_actors`), and a lightsaber in hand stays
//! unlit (`motion::weapon`'s `dark`). The weapon step runs before this one
//! in a step, so the attack that ends stealth here has already lit the
//! blade and begun its swing.

use bevy_ecs::prelude::*;

#[cfg(feature = "probes")]
pub(crate) mod probes;

use super::locks::lines;
use super::{Avatar, Clock, Fighting, Id, Object, PlayerSheet, keyed_by, scripts, sheet_of};
use crate::combat::sheet::{flags, skills};
use crate::input::{Seat, buttons};
use crate::notice::{Notice, Notices};
use crate::snapshot::SimId;

/// The skill buttons a press of which does something.
const SKILL_BUTTONS: u16 = buttons::STEALTH | buttons::LAY_MINE;
/// What ends stealth as an attack does.
const ATTACKS: u16 = buttons::ATTACK
    | buttons::ALT_ATTACK
    | buttons::FORCE_POWER
    | buttons::FORCE_GRIP
    | buttons::FORCE_LIGHTNING
    | buttons::FORCE_DRAIN
    | buttons::GRENADE;

/// This step's new presses of the skill buttons: by seat, its body, the
/// buttons newly down.
#[derive(Resource, Default)]
pub(crate) struct Presses(pub Vec<(Seat, SimId, u16)>);

/// See the module documentation.
#[allow(clippy::type_complexity)]
pub(super) fn covert(
    clock: Res<Clock>,
    scripts: Res<scripts::Scripts>,
    mut presses: ResMut<Presses>,
    mut notices: ResMut<Notices>,
    mut bodies: Query<(
        &Id,
        Option<&Avatar>,
        Option<&Object>,
        Option<&PlayerSheet>,
        &mut Fighting,
    )>,
) {
    presses.0.clear();
    let now = clock.time_ms;
    let restricted = scripts.state.party.restrict;
    let lent = super::lent(&scripts.state.party);
    // Each body with the seat whose keys act through it (`keyed`), and the
    // seat that is told about it.
    let mut bodies: Vec<_> = bodies
        .iter_mut()
        .filter_map(|(id, avatar, object, sheet, fighting)| {
            let avatar = avatar.map(|avatar| avatar.0);
            let keyed = keyed_by(avatar, id.0, lent);
            let follows = fighting.0.mind.leader.filter(|_| fighting.0.covert.stealth);
            let seat = keyed.or(avatar).or(follows)?;
            Some((seat, keyed.is_some(), id, object, sheet, fighting))
        })
        .collect();
    bodies.sort_by_key(|(seat, _, id, ..)| (*seat, id.0));
    for (seat, keyed, id, object, sheet, mut fighting) in bodies {
        let sheet = sheet_of(sheet, object);
        let fighter = &mut fighting.0;
        let command = fighter.last_command.buttons;
        let held = command & SKILL_BUTTONS;
        // Only a seat presses a key: a body no seat controls goes by its
        // mind, which presses neither.
        let pressed = if keyed {
            held & !fighter.covert.held
        } else {
            0
        };
        fighter.covert.held = held;
        if !fighter.alive() {
            fighter.covert.stealth = false;
            continue;
        }
        let mut tell = |line: u32| notices.0.push((seat, Notice::Line(line)));
        if fighter.covert.stealth {
            let attacked = command & ATTACKS != 0;
            if attacked {
                fighter.covert.opener_until = now + crate::stealth::OPENER_MS;
            }
            if attacked
                || fighter.combat_until > now
                || sheet.flags & flags::STEALTH_UNIT == 0
                || pressed & buttons::STEALTH != 0
            {
                fighter.covert.stealth = false;
                tell(lines::STEALTH_OFF);
            }
        } else if pressed & buttons::STEALTH != 0 {
            if fighter.combat_until > now {
                tell(lines::STEALTH_IN_COMBAT);
            } else if sheet.skills[skills::STEALTH] == 0
                || sheet.flags & flags::STEALTH_UNIT == 0
                || restricted
            {
                tell(lines::CANNOT_USE_SKILL);
            } else {
                fighter.covert.stealth = true;
                tell(lines::STEALTH_ON);
            }
        }
        if pressed & buttons::LAY_MINE != 0 {
            presses.0.push((seat, id.0, pressed));
        }
    }
}
