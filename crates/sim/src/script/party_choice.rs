//! Which companions a player may choose, for the co-op lobby and for the
//! in-game party selection screen. Both call this module, so the two can
//! never disagree.
//!
//! What the games' scripts set, and what this module reads:
//!
//! - A companion is recruited when a script makes it available
//!   (`AddAvailableNPCByTemplate` or `AddAvailableNPCByObject`). The party
//!   table lists it in `Party::available`. A companion the table does not
//!   list is not offered at all.
//! - A script can make a companion unselectable (`SetNPCSelectability`). The
//!   party table lists it in `Party::unselectable`. Both games' selection
//!   screens refuse to move an unselectable companion. An unselectable
//!   companion outside the party therefore cannot join, and an unselectable
//!   companion in the party cannot leave. KOTOR 2's scripts use this to keep
//!   a companion in the party for a part of the story, and both games use it
//!   to keep a companion away.
//! - A script that opens the party selection screen (`ShowPartySelectionGUI`)
//!   can force companions into the party. They are listed in the open
//!   screen's `Selection::forced`. A forced companion cannot be removed.
//! - A part of the story with a fixed character is one where a script has
//!   switched the first player to a companion (`SwitchPlayerCharacter`). The
//!   party table names that companion in `Party::switched`. The first player
//!   plays that companion, so no other player can choose it.
//! - An area can forbid changing the party. The area's file marks it as
//!   unescapable, or a script calls `SetAreaUnescapable`. The caller reads
//!   this from the script state (`State::unescapable`) and passes it as
//!   `party_locked`. In such an area only the companions already in the
//!   party can be chosen.
//!
//! The answers come in the order of the party selection screen, which is the
//! order of the companions' party numbers.

use super::party::{Party, Selection};
use crate::notice::{NPCS, Roster};

/// What a player may do with one recruited companion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// A player may choose the companion.
    Open,
    /// The story has taken the companion away, so no player may choose it.
    Unavailable,
    /// The story keeps the companion in the party. A player may choose it.
    /// When no player does, the game controls it.
    Required,
}

/// The rule for one companion. `member` is whether it is in the party now.
/// `unselectable` is whether a script made it unselectable. `forced` is
/// whether an open selection screen forces it in. `fixed` is whether the
/// first player plays it in a part of the story with a fixed character.
/// `locked` is whether the area forbids changing the party.
pub fn choice(member: bool, unselectable: bool, forced: bool, fixed: bool, locked: bool) -> Choice {
    if fixed {
        Choice::Unavailable
    } else if forced || (unselectable && member) {
        Choice::Required
    } else if unselectable || (locked && !member) {
        Choice::Unavailable
    } else {
        Choice::Open
    }
}

/// One answer for each recruited companion of `party`, by party number.
/// `party_locked` is whether the area forbids changing the party.
pub fn choices(party: &Party, party_locked: bool) -> Vec<(u8, Choice)> {
    let none = Selection::in_play();
    let selection = party.selection.as_ref().unwrap_or(&none);
    party
        .available
        .keys()
        .map(|&npc| {
            let answer = choice(
                party.has(npc),
                party.unselectable.contains(&npc),
                selection.forced.contains(&npc),
                party.switched == Some(npc),
                party_locked,
            );
            (npc, answer)
        })
        .collect()
}

/// One answer for each companion a party selection screen's table lists.
/// The screen handles a refused party change itself, so the area's lock
/// is not applied here.
pub fn of_roster(roster: &Roster) -> Vec<(u8, Choice)> {
    (0..NPCS)
        .filter(|&npc| roster.template(npc).is_some())
        .filter_map(|npc| {
            let answer = choice(
                Roster::has(roster.members, npc),
                Roster::has(roster.unselectable, npc),
                Roster::has(roster.forced, npc),
                false,
                false,
            );
            Some((u8::try_from(npc).ok()?, answer))
        })
        .collect()
}

#[cfg(feature = "probes")]
pub use probes::probes;

#[cfg(feature = "probes")]
mod probes {
    use super::*;
    use crate::probe::{Checks, check};
    use crate::script::party::Npc;

    /// A party with the companions `recruited`, of whom `members` are in
    /// the party and `unselectable` were made unselectable.
    fn party(recruited: &[u8], members: &[u8], unselectable: &[u8]) -> Party {
        let npc = |template: String| Npc {
            template,
            object: None,
            kept: None,
            template_data: None,
        };
        Party {
            available: (recruited.iter())
                .map(|&n| (n, npc(format!("companion{n}"))))
                .collect(),
            members: members.to_vec(),
            unselectable: unselectable.iter().copied().collect(),
            ..Party::default()
        }
    }

    use Choice::{Open, Required, Unavailable};

    /// Each case: its name, the party, whether the area is locked, and the
    /// answers wanted.
    type Case = (&'static str, Party, bool, Vec<(u8, Choice)>);

    fn cases() -> Vec<Case> {
        let forced = Party {
            selection: Some(Selection {
                exit: String::new(),
                forced: vec![2],
                cancel: false,
            }),
            ..party(&[0, 2], &[], &[])
        };
        let switched = Party {
            switched: Some(1),
            ..party(&[0, 1, 2], &[0], &[])
        };
        vec![
            (
                "open",
                party(&[0, 1], &[0], &[]),
                false,
                vec![(0, Open), (1, Open)],
            ),
            (
                "taken_away",
                party(&[0, 1], &[], &[1]),
                false,
                vec![(0, Open), (1, Unavailable)],
            ),
            (
                "kept_in",
                party(&[0, 1], &[1], &[1]),
                false,
                vec![(0, Open), (1, Required)],
            ),
            ("forced", forced, false, vec![(0, Open), (2, Required)]),
            (
                "locked",
                party(&[0, 1, 3], &[3], &[]),
                true,
                vec![(0, Unavailable), (1, Unavailable), (3, Open)],
            ),
            (
                "fixed_character",
                switched,
                false,
                vec![(0, Open), (1, Unavailable), (2, Open)],
            ),
            ("no_companions", party(&[], &[], &[]), false, vec![]),
        ]
    }

    /// The party selection screen's table of `party`, as the host sends it.
    fn roster(party: &Party) -> Roster {
        party.roster(None, &crate::inventory::Items::default(), &|_| None)
    }

    /// The checks (`kjk sim probes --only "party choice"`). Each case runs
    /// through `choices` and through `of_roster`, which the in-game screen
    /// uses. The two must agree wherever the area is not locked and no
    /// character is fixed, since the screen sees neither.
    pub fn probes() -> Checks {
        let mut checks: Checks = cases()
            .into_iter()
            .map(|(name, party, locked, want)| {
                let got = choices(&party, locked);
                let screen = of_roster(&roster(&party));
                let agree = locked || party.switched.is_some() || screen == got;
                (
                    name,
                    check(
                        got == want && agree,
                        format!("{got:?}, the screen's {screen:?}"),
                    ),
                )
            })
            .collect();
        checks.push(("alignment_shown", alignment_shown()));
        checks
    }

    /// The screen's table tells each companion's own alignment, not its
    /// template's (issue 778). Companion 0 is away and its creature is gone,
    /// so the table's kept sheet (12) is told. Companion 1's creature stands
    /// in the module at 90, though the table kept it at 40 earlier, so 90 is
    /// told. Companion 2 was never made, so nothing is told and the screen
    /// falls back on its template. The table reads back from its bytes.
    fn alignment_shown() -> Result<String, String> {
        const STANDING: u32 = 500;
        let kept = |from, good_evil| crate::script::party::Kept {
            from,
            sheet: crate::combat::Sheet {
                good_evil,
                ..crate::combat::Sheet::commoner()
            },
            hp: 0,
            fp: 0,
            worn: None,
            look: crate::combat::Look::default(),
        };
        let mut party = party(&[0, 1, 2], &[1], &[]);
        for (npc, object, from, good_evil) in
            [(0, None, 400, 12), (1, Some(STANDING), STANDING, 40)]
        {
            let slot = party.available.get_mut(&npc).ok_or("a companion")?;
            slot.object = object;
            slot.kept = Some(kept(from, good_evil));
        }
        let standing = |id| (id == STANDING).then_some(90);
        let roster = party.roster(None, &crate::inventory::Items::default(), &standing);
        let told: Vec<Option<u8>> = (0..3).map(|npc| roster.good_evil(npc)).collect();
        let screen = crate::notice::Screen::PartySelection(roster);
        let again = crate::notice::Screen::from_bytes(&screen.to_bytes()) == Ok(screen);
        check(
            told == [Some(12), Some(90), None] && again,
            format!("told {told:?}, read back from its bytes: {again}"),
        )
    }
}
