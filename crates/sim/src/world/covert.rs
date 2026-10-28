//! Stealth and the skill buttons on the authority
//! (`docs/COMBAT.md`).
//!
//! Each seat's new presses of `buttons::STEALTH` and `buttons::LAY_MINE`
//! this step are kept for the scripts' step (`Presses`, `world::traps`
//! lays a mine). A stealth press turns stealth mode off, or on: refused,
//! in the game's line, with no base Stealth ranks, no stealth unit worn
//! (`flags::STEALTH_UNIT`), in combat, or in the player restrict mode
//! (`SetPlayerRestrictMode`). Stealth ends on an attack, a power or a
//! grenade (opening `stealth::OPENER_MS`'s opener), on combat, on the unit
//! taken off, and on falling; a conversation beginning ends it too
//! (`script::dialogue`, `Change::Unstealth`). While it holds, the body walks
//! (KOTOR's "Run Disabled": `move_actors`).

use bevy_ecs::prelude::*;

#[cfg(feature = "probes")]
pub(crate) mod probes;

use super::locks::lines;
use super::{Avatar, Clock, Fighting, Id, Object, PlayerSheet, scripts, sheet_of};
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
        &Avatar,
        Option<&Object>,
        Option<&PlayerSheet>,
        &mut Fighting,
    )>,
) {
    presses.0.clear();
    let now = clock.time_ms;
    let restricted = scripts.state.party.restrict;
    let mut bodies: Vec<_> = bodies.iter_mut().collect();
    bodies.sort_by_key(|(id, avatar, ..)| (avatar.0, id.0));
    for (id, avatar, object, sheet, mut fighting) in bodies {
        let seat = avatar.0;
        let sheet = sheet_of(sheet, object);
        let fighter = &mut fighting.0;
        let command = fighter.last_command.buttons;
        let held = command & SKILL_BUTTONS;
        let pressed = held & !fighter.covert.held;
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
