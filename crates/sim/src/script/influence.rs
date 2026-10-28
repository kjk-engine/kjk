//! TSL's influence on its companions' alignment, as its executable's
//! `CSWSCreature::HandleAlignmentInfluence` works it out (I35,
//! `docs/PARTY.md`, "Influence"): a companion's alignment is its base
//! alignment pulled toward the player's side while its influence is over
//! the middle and pushed away from it under, by how far the player's
//! alignment (the `G_PC_Align_Val` global, not the live one) stands from
//! neutral; the player's Charisma adds to the influence counted, and a
//! Sith Lord's or Jedi Master's presence multiplies the pull with their
//! level. The arithmetic is the executable's, single floats in its order.

use script_ncs::vm::ObjectId;

use super::state::global;
use super::{Change, Library, Out, State, View};
use crate::combat::Sheet;
use crate::combat::sheet::{Ability, Rows};
use crate::module::ObjectData;
use crate::snapshot::StatusKind;
use rules::progression::influence;

/// The companions' tags by `NPC_*` number: the table's `GetNPCID` by
/// creature, else these (the executable's own list).
pub const NPC_TAGS: [&str; 12] = [
    "atton",
    "baodur",
    "mand",
    "g0t0",
    "handmaiden",
    "hk47",
    "kreia",
    "mira",
    "t3m4",
    "visasmarr",
    "hanharr",
    "disciple",
];

/// Kreia's alignment never follows the player's: her base, and her
/// influence is not started by the table's base influence.
pub const KREIA: &str = "kreia";

/// The global the player's alignment is kept in (TSL's
/// `CSWSCreatureStats::ModifyAlignment` writes it), which the companions'
/// alignment follows.
pub const PC_ALIGNMENT: &str = "G_PC_Align_Val";

/// The feats and prestige classes whose presence pushes the companions
/// harder (`feat.2da` rows 149 and 167, `classes.2da` rows 15 and 12),
/// each only while the player's alignment is on its side.
const DARK_SIDE_CORRUPTION: u16 = 149;
const LIGHT_SIDE_ENLIGHTENMENT: u16 = 167;
const SITH_LORD: u8 = 15;
const JEDI_MASTER: u8 = 12;
const DARK_AT_MOST: u8 = rules::progression::influence::DARK_AT_MOST;
const LIGHT_AT_LEAST: u8 = rules::progression::influence::LIGHT_AT_LEAST;

/// The middle of influence and alignment, and their top.
const MIDDLE: f32 = 50.0;
const TOP: i32 = 100;

/// What of the player's character counts: its alignment, Charisma
/// modifier, feats and class slots (`classes.2da` row, levels).
#[derive(Debug, Clone, Copy)]
pub struct Presence {
    pub good_evil: u8,
    pub charisma: i32,
    pub feats: Rows<4>,
    pub classes: [(u8, u8); 2],
}

impl Presence {
    /// How hard the player pushes (1.0, or `influence::PRESTIGE_PUSH` by the
    /// level of a Sith Lord on the dark side with Dark Side Corruption, of a
    /// Jedi Master on the light side with Light Side Enlightenment), and
    /// what its Charisma adds to the influence counted (its modifier
    /// `influence::CHARISMA_TIMES` over, as a signed byte).
    pub fn push(&self) -> (f32, i32) {
        let level = |class: u8| {
            self.classes
                .iter()
                .find(|(of, _)| *of == class)
                .map_or(0, |(_, levels)| *levels)
        };
        let multiplier = if self.feats.has(DARK_SIDE_CORRUPTION) {
            match self.good_evil <= DARK_AT_MOST {
                true => by_level(level(SITH_LORD)),
                false => 1.0,
            }
        } else if self.feats.has(LIGHT_SIDE_ENLIGHTENMENT) && self.good_evil >= LIGHT_AT_LEAST {
            by_level(level(JEDI_MASTER))
        } else {
            1.0
        };
        (
            multiplier,
            i32::from((influence::CHARISMA_TIMES * self.charisma) as i8),
        )
    }
}

/// The multiplier a prestige class's level gives (the executable's steps):
/// none without a level, else the push of the first step it reaches.
fn by_level(level: u8) -> f32 {
    if level == 0 {
        return 1.0;
    }
    let steps = influence::PRESTIGE_LEVELS;
    influence::PRESTIGE_PUSH[steps.iter().filter(|most| level > **most).count()]
}

/// A companion's alignment from its base `base`, its influence
/// `influence`, the player's alignment `pc` (the global) and the push
/// (`Presence::push`; none without a player: 1.0, 0).
pub fn alignment(base: u8, influence: i32, pc: i32, (multiplier, bonus): (f32, i32)) -> u8 {
    let counted = influence + bonus;
    let lean = if counted > TOP {
        MIDDLE
    } else if counted < 0 {
        -MIDDLE
    } else {
        (counted - 50) as f32
    };
    let pull = multiplier * lean / MIDDLE;
    let shift = (pc - 50) as f32 / MIDDLE * pull;
    let base = i32::from(base as i8);
    let moved = if shift > 0.0 {
        base as f32 + (TOP - base) as f32 * shift
    } else {
        base as f32 * (shift + 1.0)
    };
    if moved > TOP as f32 {
        100
    } else if moved < 0.0 {
        0
    } else {
        (moved as i32).clamp(0, TOP) as u8
    }
}

/// The data of `who`: the world's object, or one made this step.
pub(super) fn facts<'a>(view: &'a View, out: &'a Out, who: ObjectId) -> Option<&'a ObjectData> {
    match view.thing(who) {
        Some(_) => view.facts(who),
        None => out
            .made
            .iter()
            .find(|made| made.id == who)
            .map(|made| made.facts.as_ref()),
    }
}

/// `who`'s sheet as this step leaves it: a character's, or one made this
/// step's (the party table's kept one, else its template's).
pub(super) fn sheet(view: &View, out: &Out, who: ObjectId) -> Option<Sheet> {
    let worn = match view.thing(who) {
        Some(thing) => thing.character?.worn,
        None => {
            let made = out.made.iter().find(|made| made.id == who)?;
            made.kept.map(|kept| kept.sheet).or(made.facts.sheet)?
        }
    };
    Some(out.sheet(who, worn))
}

/// `who`'s alignment set to `good_evil`, where it moves.
pub(super) fn set_alignment(view: &View, out: &mut Out, who: ObjectId, good_evil: u8) {
    if let Some(mut now) = sheet(view, out, who)
        && now.good_evil != good_evil
    {
        now.good_evil = good_evil;
        out.changes.push(Change::Sheet { who, sheet: now });
    }
}

/// The companion `who` is: the party table's slot whose creature it is,
/// else the one its tag names (`NPC_TAGS`).
fn npc_of(state: &State, who: ObjectId, tag: &str) -> Option<u8> {
    state
        .party
        .available
        .iter()
        .find(|(_, slot)| slot.object == Some(who.0))
        .map(|(npc, _)| *npc)
        .or_else(|| NPC_TAGS.iter().position(|of| *of == tag).map(|n| n as u8))
}

/// Companion `npc`'s base alignment (the executable's
/// `BaseCNPCAlignment`): as an alignment shift on it as a party member
/// left it, else the alignment its creature `who` was made with.
pub(super) fn base(view: &View, out: &Out, state: &State, npc: u8, who: ObjectId) -> Option<u8> {
    state
        .party
        .bases
        .get(&npc)
        .copied()
        .or_else(|| Some(facts(view, out, who)?.sheet?.good_evil))
}

/// Companion `npc`'s influence started where unset, as the table's base
/// influence for the companion tagged `tag` (no notice): every companion
/// but Kreia reads its base from when it is first made available.
pub(super) fn start(library: &Library, state: &mut State, npc: u8, tag: &str) -> Option<i32> {
    let starts = library.base_influence(tag)?;
    let now = state.party.influence(npc);
    if now != -1 {
        return Some(now);
    }
    state.party.influence.insert(npc, starts);
    Some(i32::from(starts))
}

/// What the player pushes the companions with: the first seat's own
/// character's (`Presence`), none without one.
fn push(view: &View, out: &Out) -> (f32, i32) {
    let pc = view.party.first().copied();
    let presence = pc.and_then(|pc| {
        let character = view.thing(pc)?.character?;
        let sheet = out.sheet(pc, character.worn);
        Some(Presence {
            good_evil: sheet.good_evil,
            charisma: sheet.modifier(Ability::Charisma),
            feats: character.feats,
            classes: character.classes,
        })
    });
    presence.map_or((1.0, 0), |presence| presence.push())
}

/// TSL's companion `who` takes the alignment its influence gives it (the
/// executable's `HandleAlignmentInfluence`): Kreia her base; a companion
/// `influence.2da` lists, its base pulled by its influence (started where
/// unset) and the player's alignment; any other creature is left as it
/// is.
pub(super) fn follow(
    library: &Library,
    view: &View,
    state: &mut State,
    out: &mut Out,
    who: ObjectId,
) {
    if !library.influences() {
        return;
    }
    let Some(tag) = facts(view, out, who).map(|facts| facts.tag.to_ascii_lowercase()) else {
        return;
    };
    let npc = npc_of(state, who, &tag);
    if tag == KREIA {
        if let Some(base) = npc.and_then(|npc| base(view, out, state, npc, who)) {
            set_alignment(view, out, who, base.min(100));
        }
        return;
    }
    if library.base_influence(&tag).is_none() {
        return;
    }
    let Some(npc) = npc else {
        return;
    };
    let pc = global(&state.numbers, PC_ALIGNMENT).copied().unwrap_or(0);
    let push = push(view, out);
    let Some(influence) = start(library, state, npc, &tag) else {
        return;
    };
    if let Some(base) = base(view, out, state, npc, who) {
        set_alignment(view, out, who, alignment(base, influence, pc, push));
    }
}

/// Companion `npc`'s influence changed by a script: told in the status
/// summary (gained or lost as `gained` says), its creature here following
/// it.
pub(super) fn moved(
    library: &Library,
    view: &View,
    state: &mut State,
    out: &mut Out,
    npc: u8,
    gained: bool,
) {
    out.statuses.push(StatusKind::Influence { npc, gained });
    let here = state.party.available.get(&npc).and_then(|slot| slot.object);
    if let Some(who) = here
        .map(ObjectId)
        .filter(|who| sheet(view, out, *who).is_some())
    {
        follow(library, view, state, out, who);
    }
}

/// The probes of the formula (`kjk sim probes influence`): the exact
/// numbers the executable gives.
#[cfg(feature = "probes")]
pub fn probes() -> crate::probe::Checks {
    use crate::probe::check;
    let plain = Presence {
        good_evil: 50,
        charisma: 0,
        feats: Rows::default(),
        classes: [(0, 1), (0, 0)],
    };
    let none = plain.push();
    let cases = [
        // Atton (base 50) against a fully dark player.
        ("full influence follows", alignment(50, 100, 0, none), 0),
        ("no influence opposes", alignment(50, 0, 0, none), 100),
        ("middle influence holds", alignment(50, 50, 0, none), 50),
        ("part way, truncated", alignment(50, 75, 25, none), 37),
        // Visas (base 35), a fully light player of Charisma 14.
        ("charisma counts", alignment(35, 90, 100, (1.0, 6)), 94),
    ];
    let mut checks: crate::probe::Checks = cases
        .into_iter()
        .map(|(name, got, want)| (name, check(got == want, format!("{got}, want {want}"))))
        .collect();
    let mut master = Presence {
        good_evil: 80,
        charisma: 2,
        classes: [(3, 6), (JEDI_MASTER, 12)],
        ..plain
    };
    master.feats.insert(LIGHT_SIDE_ENLIGHTENMENT);
    let push = master.push();
    checks.push((
        "jedi master pushes",
        check(
            push == (1.6, 6) && alignment(35, 90, 100, push) == 100,
            format!("push {push:?}, Visas {}", alignment(35, 90, 100, push)),
        ),
    ));
    let mut fallen = master;
    fallen.feats = Rows::default();
    fallen.feats.insert(DARK_SIDE_CORRUPTION);
    checks.push((
        "corruption needs the dark side",
        check(
            fallen.push().0 == 1.0,
            format!("a light Sith Lord's push {:?}", fallen.push()),
        ),
    ));
    checks
}
