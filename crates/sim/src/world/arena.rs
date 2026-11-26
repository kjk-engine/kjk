//! The combat arena (`kjk play --stage arena`): a test floor where the
//! party fights whatever its players call in. A level is an arena when it
//! has `Rules` (`Level::with_arena`); there:
//!
//! - a player's `Action::Arena` orders are carried out on the authority, in
//!   the scripts' step (`orders`): a creature of a template made ahead of
//!   the seat's body, where its whole body fits the level, and turned
//!   hostile, every creature not of the party
//!   taken away, or the body's god mode (`sheet::flags::IMMORTAL`) turned
//!   on or off;
//! - one whose body falls below the pit's floor dies there (a party member
//!   goes down), and a party member down is back on its feet at the
//!   respawn point, whole, after `Rules::respawn_ms`: nobody revives and
//!   the arena is never lost (`falls`);
//! - the Debug Mode terminal (the placeable tagged `TERMINAL_TAG`) used by
//!   a seat's body opens the character page for that seat alone
//!   (`Screen::DebugTerminal`), and the page's orders change a party
//!   member (`edits`). A seat changes its own character or a
//!   companion no other seat plays, never another seat's character.

pub mod edits;

use script_ncs::vm::ObjectId;

use crate::action::{Action, ArenaOrder};
use crate::combat::Sheet;
use crate::combat::sheet::{factions, flags};
use crate::input::{Seat, TickInput};
use crate::module::ObjectKind;
use crate::script::{self, Change, Library, Out, State, View};
use crate::snapshot::SimId;

/// The tag of the Debug Mode map's terminal.
pub const TERMINAL_TAG: &str = "kjk_debug_terminal";

/// An arena's rules (metres, degrees and sim ms).
#[derive(Debug, Clone, PartialEq)]
pub struct Rules {
    /// Where the party comes back, and the way it then faces.
    pub respawn: [f32; 3],
    pub yaw: f32,
    /// The height under which a body has fallen into the pit.
    pub pit: f32,
    /// How long a party member lies down before it comes back.
    pub respawn_ms: u64,
    /// The Debug Mode terminal, on a level that has one.
    pub terminal: Option<Terminal>,
}

/// The Debug Mode terminal of an arena: where it stands, the way it faces,
/// and its facts (made from `TERMINAL_TEMPLATE`, tagged `TERMINAL_TAG`).
/// The arena's step makes it there when the level has none
/// (`place_terminal`), and the level's templates answer for it
/// (`Level::template`).
#[derive(Debug, Clone, PartialEq)]
pub struct Terminal {
    pub at: [f32; 3],
    pub yaw: f32,
    pub facts: std::sync::Arc<crate::module::ObjectData>,
}

impl Rules {
    pub(super) fn digest(&self, hasher: &mut blake3::Hasher) {
        for value in self.respawn.iter().chain([&self.yaw, &self.pit]) {
            hasher.update(&value.to_le_bytes());
        }
        hasher.update(&self.respawn_ms.to_le_bytes());
        if let Some(terminal) = &self.terminal {
            for value in terminal.at.iter().chain([&terminal.yaw]) {
                hasher.update(&value.to_le_bytes());
            }
            let mut facts = crate::wire::Writer::default();
            terminal.facts.digest_into(&mut facts);
            hasher.update(&facts.0);
        }
    }
}

/// The template the Debug Mode terminal is made from: KOTOR 1's Rakatan
/// computer, the Star Forge's own.
pub const TERMINAL_TEMPLATE: &str = "plc_rakatcom";

/// How far ahead of the seat's body a creature called in is made (metres).
pub const SPAWN_AHEAD: f32 = rules::world::arena::SPAWN_AHEAD_M;

/// A seat of the step: the seat, its own character, and the body it acts
/// through (a companion it leads, else its own character).
pub(super) type SeatBody = (Seat, ObjectId, ObjectId);

/// The arena's orders this step (see the module documentation), in the
/// order they came, and the terminal's uses (`used`: object, user).
/// Whether a character page's order changed a character, so that what the
/// players see of the party is gathered again.
pub(super) fn orders(
    library: &Library,
    view: &View,
    state: &mut State,
    (inputs, used): (&TickInput, &[(SimId, SimId)]),
    seats: &[SeatBody],
    (rules, out): (&Rules, &mut Out),
) -> bool {
    place_terminal(library, view, state, rules, out);
    terminal_uses(view, used, seats, out);
    let mut edited = false;
    for (seat, action) in inputs.actions() {
        let Action::Arena(order) = action else {
            continue;
        };
        let Some(&(_, _, acting)) = seats.iter().find(|(of, ..)| of == seat) else {
            continue;
        };
        let Some(body) = view.thing(acting) else {
            continue;
        };
        if edit(library, view, state, *seat, order, out) {
            edited = true;
            continue;
        }
        match order {
            ArenaOrder::Spawn(template) => {
                let template = script::key(template);
                let Some(mut sheet) = library.creature(&template).and_then(|facts| facts.sheet)
                else {
                    continue;
                };
                let (sin, cos) = crate::math::sin_cos_deg(body.yaw);
                let [x, y, z] = body.position;
                let at = [x + cos * SPAWN_AHEAD, y + sin * SPAWN_AHEAD, z];
                let yaw = body.yaw + 180.0;
                let Some(made) = script::make(library, view, state, out, &template, at, yaw) else {
                    continue;
                };
                if let Some(put) = out.made.iter_mut().find(|put| put.id == made) {
                    put.reach = crate::install::ARENA;
                }
                // A foe of the party whatever its template's side, killable:
                // its sheet as made (which the snapshot keeps).
                sheet.faction = factions::HOSTILE_1;
                sheet.flags &= !(flags::PLOT | flags::MIN_ONE_HP | flags::IMMORTAL);
                out.changes.push(Change::Sheet { who: made, sheet });
            }
            ArenaOrder::Clear => {
                let foes = view.things.iter().filter(|thing| {
                    thing.kind == Some(ObjectKind::Creature)
                        && thing.character.is_some_and(|character| !character.party)
                });
                for thing in foes {
                    out.changes.push(Change::Destroy {
                        who: thing.id,
                        ms: 0,
                        going: None,
                    });
                }
            }
            ArenaOrder::God => {
                let Some(character) = body.character else {
                    continue;
                };
                let mut sheet = out.sheet(body.id, character.worn);
                sheet.flags ^= flags::IMMORTAL;
                out.changes.push(Change::Sheet {
                    who: body.id,
                    sheet,
                });
            }
            // A fall of the body's own (`falls::arena_falls`). The
            // terminal's orders are `edit`'s.
            _ => {}
        }
    }
    edited
}

/// The Debug Mode terminal made where the rules place it, on a level that
/// has one and has not made it yet. It is made once: a save keeps it, as
/// it keeps every object play made.
fn place_terminal(library: &Library, view: &View, state: &mut State, rules: &Rules, out: &mut Out) {
    let Some(terminal) = &rules.terminal else {
        return;
    };
    let made = view.things.iter().any(|thing| thing.tag() == TERMINAL_TAG)
        || out.made.iter().any(|made| made.facts.tag == TERMINAL_TAG);
    if !made {
        let facts = &terminal.facts;
        let yaw = facts.kind.yaw_facing(terminal.yaw);
        script::make_placed(library, view, state, out, facts, terminal.at, yaw);
    }
}

/// The terminal used by a seat's body this step: its screen opens for that
/// seat's player alone.
fn terminal_uses(view: &View, used: &[(SimId, SimId)], seats: &[SeatBody], out: &mut Out) {
    let mut told = Vec::new();
    for &(object, user) in used {
        let terminal = view
            .thing(ObjectId(object.0))
            .is_some_and(|thing| thing.tag() == TERMINAL_TAG);
        let user = ObjectId(user.0);
        let player = seats
            .iter()
            .find(|(_, own, acting)| *own == user || *acting == user)
            .map(|(_, own, _)| *own);
        if let Some(player) = player.filter(|player| terminal && !told.contains(player)) {
            told.push(player);
            let screen = crate::notice::Screen::DebugTerminal;
            out.notices
                .push((Some(player), crate::notice::Notice::Show(screen)));
        }
    }
}

/// Whether `seat` may change party member `who`: its own character or the
/// companion it leads, or a companion no other seat plays.
fn may_change(view: &View, seat: Seat, who: ObjectId) -> bool {
    view.thing(who)
        .and_then(|thing| thing.character)
        .is_some_and(|character| {
            character.party && character.driver.is_none_or(|driver| driver == seat)
        })
}

/// The terminal's order `order` from `seat` carried out (`edits`): whether
/// it was one of the terminal's. An order naming a character the seat may
/// not change, or numbers no character can have, changes nothing, and so
/// does a restore naming anyone but a player's own character.
fn edit(
    library: &Library,
    view: &View,
    state: &mut State,
    seat: Seat,
    order: &ArenaOrder,
    out: &mut Out,
) -> bool {
    let who = match order {
        ArenaOrder::Rebuild { who, .. }
        | ArenaOrder::Ability { who, .. }
        | ArenaOrder::Skill { who, .. }
        | ArenaOrder::Alignment { who, .. }
        | ArenaOrder::Restore { who, .. } => ObjectId(who.0),
        ArenaOrder::Spawn(_) | ArenaOrder::Clear | ArenaOrder::God | ArenaOrder::Down(_) => {
            return false;
        }
    };
    let Some(character) = view.thing(who).and_then(|thing| thing.character) else {
        return true;
    };
    if !may_change(view, seat, who) {
        return true;
    }
    let sheet = out.sheet(who, character.worn);
    let changed = match order {
        ArenaOrder::Ability { ability, score, .. } => edits::ability(&sheet, *ability, *score),
        ArenaOrder::Skill { skill, ranks, .. } => edits::skill(&sheet, *skill, *ranks),
        ArenaOrder::Alignment { good_evil, .. } => Some(edits::alignment(&sheet, *good_evil)),
        ArenaOrder::Rebuild { class, level, .. } => {
            let classes = library.classes();
            let Some((made, career, banked)) = edits::rebuild(classes, &sheet, *class, *level)
            else {
                return true;
            };
            state.careers.insert(who.0, career);
            if banked > 0 {
                state.unspent.insert(who.0, banked);
            } else {
                state.unspent.remove(&who.0);
            }
            let needed = classes.next(made.level.saturating_sub(1)).unwrap_or(0);
            let party = crate::inventory::PARTY;
            if state.xp(party) < needed {
                state.set_xp(party, needed);
            }
            whole(library, state, out, who, made);
            return true;
        }
        // The starting character is a player's: a companion has none the
        // terminal keeps, so it is left as it is.
        ArenaOrder::Restore { .. } if view.thing(who).is_some_and(|thing| thing.kind.is_some()) => {
            return true;
        }
        ArenaOrder::Restore { sheet: start, .. } => {
            // The starting character has no career of its own.
            state.careers.remove(&who.0);
            state.unspent.remove(&who.0);
            whole(library, state, out, who, edits::kept(**start, &sheet));
            return true;
        }
        _ => None,
    };
    if let Some(sheet) = changed {
        out.changes.push(Change::Sheet { who, sheet });
    }
    true
}

/// `who` as `made`, dressed again by what it wears, with full hit points
/// and Force points.
fn whole(library: &Library, state: &State, out: &mut Out, who: ObjectId, made: Sheet) {
    let sheet = state.dressed(library.items(), who.0, made);
    out.changes.push(Change::Sheet { who, sheet });
    out.changes.push(Change::Whole { who, sheet });
}
