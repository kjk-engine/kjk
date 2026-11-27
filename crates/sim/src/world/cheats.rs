//! The console's cheats (`docs/CONSOLE.md`).
//!
//! A console command that changes the game is an order to the simulation:
//! one of the arena's orders (`world::arena`) or a `CheatOrder`. The orders
//! are carried out in the scripts' step, after the players' own actions.
//!
//! - The cheat rule (`Rule`) says who may cheat. Only the first seat may
//!   set it (`CheatOrder::Policy`). The host's game sends it when the
//!   session starts. The rule is checked again here for every order, so a
//!   changed guest program cannot cheat against the host's wishes.
//! - A level with arena rules (the Debug Mode map and the other combat
//!   test levels) takes every order whatever the rule, because those levels
//!   exist for them.
//! - A seat changes only its own character, the companion it leads, or a
//!   companion that no other seat plays.
//! - The first order carried out outside an arena level marks the game as
//!   cheated (`Cheats::used`). The mark never goes away again, and a save
//!   keeps it.
//! - An order that is refused sends its seat's player a notice
//!   (`Notice::Cheat`) that says why. An order carried out sends none: the
//!   console has already printed what the command does.

use script_ncs::vm::ObjectId;

use super::arena::SeatBody;
use crate::action::{Action, CheatOrder, Mark};
use crate::combat::sheet::flags;
use crate::input::{Seat, TickInput};
use crate::notice::Notice;
use crate::script::{self, Change, Library, Out, State, View};
use crate::wire::{DecodeError, Reader, Writer};

/// The largest amounts the console's orders set (the rulebook's
/// `world.console` section).
pub mod limits {
    pub const CREDITS_MOST: i32 = rules::world::console::CREDITS_MOST;
    pub const XP_MOST: i32 = rules::world::console::XP_MOST;
    pub const ITEMS_MOST: u16 = rules::world::console::ITEMS_MOST;
}

/// Who may cheat.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Rule {
    /// No player.
    #[default]
    Off,
    /// The host alone (the first seat).
    Host,
    /// Every player.
    All,
}

impl Rule {
    pub const ALL: [Rule; 3] = [Rule::Off, Rule::Host, Rule::All];

    /// The rule's number on the wire.
    pub fn code(self) -> u8 {
        match self {
            Rule::Off => 0,
            Rule::Host => 1,
            Rule::All => 2,
        }
    }

    pub fn from_code(code: u8) -> Option<Rule> {
        Rule::ALL.into_iter().find(|rule| rule.code() == code)
    }
}

/// The session's cheat rule, and whether a cheat was ever carried out
/// outside an arena level (`Snapshot::cheats`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cheats {
    pub rule: Rule,
    pub used: bool,
}

impl Cheats {
    pub(crate) fn encode(&self, out: &mut Writer) {
        out.u8(self.rule.code());
        out.u8(u8::from(self.used));
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Cheats, DecodeError> {
        let at = input.at();
        let rule = Rule::from_code(input.u8()?).ok_or_else(|| input.invalid("cheat rule", at))?;
        let at = input.at();
        let used = match input.u8()? {
            0 => false,
            1 => true,
            _ => return Err(input.invalid("cheated flag", at)),
        };
        Ok(Cheats { rule, used })
    }

    /// Why `seat` may not cheat now, or `None` when it may.
    pub(super) fn refusal(&self, seat: Seat) -> Option<Outcome> {
        match self.rule {
            Rule::Off => Some(Outcome::Off),
            Rule::Host if seat != super::LEADER_SEAT => Some(Outcome::HostOnly),
            Rule::Host | Rule::All => None,
        }
    }
}

/// What became of a cheat order (`Notice::Cheat`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The order was carried out.
    Done,
    /// The cheat rule is Off.
    Off,
    /// The cheat rule lets only the host cheat.
    HostOnly,
    /// The order names another player's character, or a companion that
    /// another player leads.
    NotYours,
    /// The order names a template, a script, a tag or an object that does
    /// not exist here.
    Unknown,
    /// The order would kill a party member or a creature the story needs.
    Protected,
    /// The order works only on the Debug Mode map and the other combat
    /// test levels.
    ArenaOnly,
}

/// Tells the player of `own` what became of an order.
pub(super) fn tell(out: &mut Out, own: ObjectId, outcome: Outcome) {
    out.notices.push((Some(own), Notice::Cheat(outcome)));
}

/// Whether `seat` may change party member `who`: its own character or the
/// companion it leads, or a companion no other seat plays.
pub(super) fn may_change(view: &View, seat: Seat, who: ObjectId) -> bool {
    view.thing(who)
        .and_then(|thing| thing.character)
        .is_some_and(|character| {
            character.party && character.driver.is_none_or(|driver| driver == seat)
        })
}

/// The step's `CheatOrder::Policy` orders, before any other order of the
/// step, so a rule sent with a cheat in the same step already counts. The
/// first seat's set the rule without a notice, because the host's game
/// sends one each time a session starts. Any other seat's is refused.
pub(super) fn policy(inputs: &TickInput, seats: &[SeatBody], cheats: &mut Cheats, out: &mut Out) {
    for (seat, action) in inputs.actions() {
        let Action::Cheat(CheatOrder::Policy(rule)) = action else {
            continue;
        };
        if *seat == super::LEADER_SEAT {
            cheats.rule = *rule;
        } else if let Some(&(_, own, _)) = seats.iter().find(|(of, ..)| of == seat) {
            tell(out, own, Outcome::HostOnly);
        }
    }
}

/// The step's other cheat orders carried out, in the order they came.
/// `arena` says whether the level has arena rules. Whether any order was
/// carried out, so that what the players see of the party is gathered
/// again.
pub(super) fn orders(
    library: &Library,
    view: &View,
    state: &mut State,
    (inputs, seats): (&TickInput, &[SeatBody]),
    (arena, cheats): (bool, &mut Cheats),
    out: &mut Out,
) -> bool {
    let mut carried = false;
    for (seat, action) in inputs.actions() {
        let Action::Cheat(order) = action else {
            continue;
        };
        if matches!(order, CheatOrder::Policy(_)) {
            continue;
        }
        let Some(&(_, own, acting)) = seats.iter().find(|(of, ..)| of == seat) else {
            continue;
        };
        let outcome = match cheats.refusal(*seat).filter(|_| !arena) {
            Some(refused) => refused,
            None => carry(library, view, state, (*seat, acting), order, out),
        };
        if outcome == Outcome::Done {
            carried = true;
            cheats.used |= !arena;
        } else {
            tell(out, own, outcome);
        }
    }
    carried
}

/// One order of `seat`, whose body is `acting`, carried out.
fn carry(
    library: &Library,
    view: &View,
    state: &mut State,
    (seat, acting): (Seat, ObjectId),
    order: &CheatOrder,
    out: &mut Out,
) -> Outcome {
    let party = crate::inventory::PARTY;
    match order {
        // Set before the step's other orders (`policy`).
        CheatOrder::Policy(_) => Outcome::Done,
        CheatOrder::Item { template, count } => {
            let catalog = library.items();
            if state.item_facts(catalog, template).is_none() {
                return Outcome::Unknown;
            }
            for _ in 0..(*count).clamp(1, limits::ITEMS_MOST) {
                state.make_item(catalog, template, party, 1, None);
            }
            Outcome::Done
        }
        CheatOrder::Credits(amount) => {
            let total = i64::from(state.gold) + i64::from(*amount);
            let total = total.clamp(0, i64::from(limits::CREDITS_MOST));
            state.gold = i32::try_from(total).unwrap_or(limits::CREDITS_MOST);
            Outcome::Done
        }
        CheatOrder::Xp(total) => {
            state.set_xp(party, (*total).clamp(0, limits::XP_MOST));
            Outcome::Done
        }
        CheatOrder::Heal { who } => {
            let healed: Vec<ObjectId> = match who {
                Some(who) => vec![ObjectId(who.0)],
                None => (view.things.iter())
                    .filter(|thing| may_change(view, seat, thing.id))
                    .map(|thing| thing.id)
                    .collect(),
            };
            if healed.is_empty() || !healed.iter().all(|&who| may_change(view, seat, who)) {
                return Outcome::NotYours;
            }
            for who in healed {
                if let Some(character) = view.thing(who).and_then(|thing| thing.character) {
                    let sheet = out.sheet(who, character.worn);
                    out.changes.push(Change::Whole { who, sheet });
                }
            }
            Outcome::Done
        }
        CheatOrder::Kill { who } => kill(view, state, acting, who),
        CheatOrder::Global { name, value } => {
            if state.booleans.contains_key(name)
                || (state.booleans.keys()).any(|key| key.eq_ignore_ascii_case(name))
            {
                script::set_global(&mut state.booleans, name, *value != 0);
            } else {
                script::set_global(&mut state.numbers, name, *value);
            }
            Outcome::Done
        }
        CheatOrder::Script { resref, who } => {
            if library.program(resref).is_none() {
                return Outcome::Unknown;
            }
            let caller = match who {
                None => script::MODULE,
                Some(who) => {
                    let who = ObjectId(who.0);
                    let Some(thing) = view.thing(who) else {
                        return Outcome::Unknown;
                    };
                    let party = thing.character.is_some_and(|character| character.party);
                    if party && !may_change(view, seat, who) {
                        return Outcome::NotYours;
                    }
                    who
                }
            };
            state
                .pending
                .push(script::Pending::new(&script::key(resref), caller));
            Outcome::Done
        }
    }
}

/// The creature `who` names killed by `acting`: an `EffectDeath` applied,
/// as a script applies one, so the creature dies in the normal way and its
/// death script runs. A party member and a creature the story needs (its
/// sheet's plot flag) are refused.
fn kill(view: &View, state: &mut State, acting: ObjectId, who: &Mark) -> Outcome {
    let thing = match who {
        Mark::Id(id) => view.thing(ObjectId(id.0)),
        Mark::Tag(tag) => {
            let from = view.thing(acting).map_or([0.0; 3], |body| body.position);
            // The square of the distance on the floor, which orders the same
            // way and needs no square root.
            let apart = |at: [f32; 3]| {
                let (x, y) = (at[0] - from[0], at[1] - from[1]);
                x * x + y * y
            };
            (view.things.iter())
                .filter(|thing| thing.tag().eq_ignore_ascii_case(tag) && !thing.dead())
                .min_by(|a, b| apart(a.position).total_cmp(&apart(b.position)))
        }
    };
    let Some((thing, character)) = thing
        .filter(|thing| !thing.dead())
        .and_then(|thing| Some((thing, thing.character?)))
    else {
        return Outcome::Unknown;
    };
    if character.party || character.sheet.flags & (flags::PLOT | flags::IMMORTAL) != 0 {
        return Outcome::Protected;
    }
    let id = state.next_effect;
    state.next_effect = state.next_effect.wrapping_add(1);
    state.applied.push(script::effects::Applied {
        target: thing.id,
        by: acting,
        effect: script::effects::Effect::Death,
        until: None,
        spell: u16::MAX,
        id,
        at: None,
        save: None,
        laid: false,
    });
    Outcome::Done
}
