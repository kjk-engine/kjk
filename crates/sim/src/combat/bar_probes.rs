//! The probes of the action bar's stored state (`combat::bar`), no install:
//! - putting a content in a slot moves it there, and a content already on
//!   the bar swaps places with what the slot held;
//! - a slot past the bar's end is refused and nothing changes;
//! - clearing a slot empties it;
//! - every content that is not a power has its own bit in a set of contents;
//! - the bar comes back whole from its bytes, and an empty bar takes one
//!   byte a slot, two bytes a set and one byte for its first fill;
//! - a sheet from a snapshot older than the bar reads an empty bar;
//! - the first fill puts attack powers, then feat attacks that fit the
//!   weapon in hand, then grenades, then medpacs in the first four slots,
//!   and a second fill changes nothing;
//! - a power learned later takes the first empty slot of the four, or the
//!   slot of the lower tier it follows, and is left off a full bar; a slot
//!   the player cleared is not filled again;
//! - the wheel lists defensive powers before attack powers, leaves out what
//!   is on the bar, what is hidden and lower tiers not shown, and lists
//!   mines only for a character with a rank in Demolitions;
//! - a player's bar is filled by the world's first step.

use std::collections::BTreeMap;

use crate::combat::Sheet;
use crate::combat::bar::{Bar, Content, Contents, FILLED, FeatLine, Rules, SLOTS, wheel};
use crate::combat::items::{Carried, KINDS, Shelf};
use crate::combat::sheet::{Rows, feats, races, skills};
use crate::combat::spells::{
    FORCE_POWER_CHOKE as CHOKE, FORCE_POWER_CURE as CURE, FORCE_POWER_FORCE_PUSH as PUSH,
    FORCE_POWER_FORCE_STORM as STORM, FORCE_POWER_HEAL as HEAL, FORCE_POWER_LIGHTNING as LIGHTNING,
    FORCE_POWER_SHOCK as SHOCK, FORCE_POWER_STUN as STUN, FORCE_POWER_WOUND as WOUND,
};
use crate::probe::{Checks, check};
use crate::wire::{Reader, Versions, Writer};

fn put_and_swap() -> Result<String, String> {
    let mut bar = Bar::default();
    let placed = bar.put(0, Content::Power(10))
        && bar.put(1, Content::Item(Shelf::Medpac))
        && bar.put(0, Content::Item(Shelf::Medpac));
    let slots = *bar.slots();
    check(
        placed
            && slots[0] == Some(Content::Item(Shelf::Medpac))
            && slots[1] == Some(Content::Power(10))
            && bar.slot_of(Content::Power(10)) == Some(1),
        format!("placed {placed}, slots {slots:?}"),
    )
}

fn put_out_of_range() -> Result<String, String> {
    let mut bar = Bar::default();
    let placed = bar.put(SLOTS, Content::Stealth);
    check(
        !placed && bar == Bar::default(),
        format!("placed {placed}, bar {bar:?}"),
    )
}

fn clear_empties() -> Result<String, String> {
    let mut bar = Bar::default();
    bar.put(0, Content::Mine);
    let cleared = bar.clear(0);
    let past_end = bar.clear(SLOTS);
    check(
        cleared && !past_end && bar.slots()[0].is_none(),
        format!("cleared {cleared}, past the end {past_end}, bar {bar:?}"),
    )
}

/// Every content that is not a power.
fn others() -> Vec<Content> {
    let items = Shelf::ALL.into_iter().map(Content::Item);
    let feats = FeatLine::ALL.into_iter().map(Content::Feat);
    items
        .chain([Content::Mine])
        .chain(feats)
        .chain([Content::Stealth])
        .collect()
}

fn content_bits() -> Result<String, String> {
    let mut seen = Contents::default();
    let shared: Vec<Content> = others()
        .into_iter()
        .filter(|content| {
            let had = seen.has(*content);
            seen.insert(*content);
            had || !seen.has(*content)
        })
        .collect();
    check(
        shared.is_empty(),
        format!("contents without their own bit {shared:?}"),
    )
}

/// A bar holding each kind of content, two hidden powers, one shown lower
/// tier, every content seen by the fill, and the first fill done.
fn full_bar() -> Bar {
    let contents = [
        Content::Power(10),
        Content::Item(Shelf::Grenade),
        Content::Mine,
        Content::Feat(FeatLine::SniperShot),
        Content::Stealth,
    ];
    let mut bar = Bar::default();
    for (slot, content) in contents.into_iter().enumerate() {
        bar.put(slot, content);
        bar.seen.insert(content);
    }
    bar.hidden.insert(12);
    bar.hidden.insert(300);
    bar.unhidden.insert(7);
    for content in others() {
        bar.seen.insert(content);
    }
    bar.filled = true;
    bar
}

/// Both the full bar and an empty one come back from their bytes.
fn round_trip() -> Result<String, String> {
    let empty_size = SLOTS + 4 * 2 + 1;
    let mut sizes = Vec::new();
    for bar in [full_bar(), Bar::default()] {
        let mut out = Writer::default();
        bar.encode(&mut out);
        sizes.push(out.0.len());
        let mut input = Reader::new("probe", &out.0);
        let read = Bar::decode(&mut input);
        let whole = input.finish();
        if read.as_ref() != Ok(&bar) || whole.is_err() {
            return Err(format!("sent {bar:?}, read {read:?}, rest {whole:?}"));
        }
    }
    check(
        sizes[1] == empty_size,
        format!("bytes {sizes:?}, an empty bar should take {empty_size}"),
    )
}

/// A sheet written before the bar came in (the sheet, then its picks)
/// reads an empty bar; one written now reads its bar back.
fn old_snapshot_has_empty_bar() -> Result<String, String> {
    let mut sheet = Sheet::dev_soldier();
    sheet.bar = full_bar();
    let mut old = Writer::default();
    sheet.encode(&mut old);
    sheet.picks.encode(&mut old);
    let versions = Versions {
        snapshot: crate::since::snapshot::FEAT_ACTION_BAR.1 - 1,
        ..Versions::THIS_BUILD
    };
    let mut input = Reader::new("probe", &old.0).with_versions(versions);
    let read_old = crate::snapshot::decode_sheet(&mut input).map(|sheet| sheet.bar);
    let old_whole = input.finish();
    let mut now = Writer::default();
    crate::snapshot::encode_sheet(&sheet, &mut now);
    let read_now = crate::snapshot::decode_sheet(&mut Reader::new("probe", &now.0));
    let now_whole = read_now.as_ref() == Ok(&sheet);
    check(
        read_old == Ok(Bar::default()) && old_whole.is_ok() && now_whole,
        format!(
            "old reads {read_old:?} ({old_whole:?}), new reads its sheet back {now_whole}, bar {:?}",
            read_now.map(|sheet| sheet.bar)
        ),
    )
}

/// A medpac's and a fragmentation grenade's `spells.2da` rows.
const MEDPAC: u16 = 64;
const GRENADE: u16 = 87;

/// Runs `run` with the probes' own game data: Stun and the powers of
/// Shock's, Push's and Wound's chains are used on enemies, and each tier
/// of Shock's, Wound's and Cure's chains needs the one below it. `mines` says whether the party carries a mine kit.
fn with_rules<R>(mines: bool, run: impl FnOnce(&Rules) -> R) -> R {
    let hostile = |row: u16| [SHOCK, LIGHTNING, STORM, PUSH, WOUND, CHOKE, STUN].contains(&row);
    let prerequisites = BTreeMap::from(
        [
            (LIGHTNING, SHOCK),
            (STORM, LIGHTNING),
            (CHOKE, WOUND),
            (HEAL, CURE),
        ]
        .map(|(power, needs)| (power, [Some(needs), None])),
    );
    run(&Rules {
        hostile: &hostile,
        prerequisites: &prerequisites,
        library: None,
        mines,
    })
}

/// What a character carries: so many of each `spells.2da` row.
fn carrying(rows: &[u16]) -> [Option<Carried>; KINDS] {
    let mut items = [None; KINDS];
    for (held, row) in items.iter_mut().zip(rows) {
        *held = Some(Carried {
            row: *row,
            count: 2,
        });
    }
    items
}

/// A Jedi with Shock, Lightning, Push, Choke and Heal, a single lightsaber
/// and medpacs.
fn jedi() -> Sheet {
    let mut sheet = Sheet::dev_jedi();
    sheet.powers = [SHOCK, LIGHTNING, PUSH, CHOKE, HEAL]
        .into_iter()
        .fold(Rows::default(), |rows, row| rows.with(row));
    sheet.weapons[1] = None;
    sheet.items = carrying(&[MEDPAC]);
    sheet
}

/// A soldier with Power Attack and Power Blast, a blaster pistol in the
/// right hand, grenades and medpacs.
fn soldier() -> Sheet {
    let mut sheet = Sheet::dev_soldier();
    sheet.feats = sheet
        .feats
        .with(feats::POWER_ATTACK[0])
        .with(feats::POWER_BLAST[0]);
    sheet.items = carrying(&[GRENADE, MEDPAC]);
    sheet
}

/// A droid with no powers, no feat attacks and no items.
fn droid() -> Sheet {
    Sheet {
        race: races::DROID,
        ..Sheet::commoner()
    }
}

/// Settles `sheet`'s bar with the right hand's weapon, as the world does.
fn settled(sheet: &mut Sheet) -> bool {
    let mut bar = sheet.bar;
    let changed = with_rules(false, |rules| bar.settle(sheet, rules, 0));
    sheet.bar = bar;
    changed
}

/// The first four slots, then two empty ones.
fn slots(first: [Option<Content>; FILLED]) -> [Option<Content>; SLOTS] {
    let mut slots = [None; SLOTS];
    slots[..FILLED].copy_from_slice(&first);
    slots
}

type Characters = [(&'static str, fn() -> Sheet, [Option<Content>; FILLED]); 3];

/// Each character and its first fill.
const CHARACTERS: Characters = [
    (
        "jedi",
        jedi,
        [
            Some(Content::Power(PUSH)),
            Some(Content::Power(LIGHTNING)),
            Some(Content::Power(CHOKE)),
            Some(Content::Item(Shelf::Medpac)),
        ],
    ),
    (
        "soldier",
        soldier,
        [
            Some(Content::Feat(FeatLine::PowerBlast)),
            Some(Content::Item(Shelf::Grenade)),
            Some(Content::Item(Shelf::Medpac)),
            None,
        ],
    ),
    ("droid", droid, [None; FILLED]),
];

fn first_fill() -> Result<String, String> {
    let mut seen = Vec::new();
    for (name, sheet, first) in CHARACTERS {
        let mut sheet = sheet();
        let changed = settled(&mut sheet);
        let got = *sheet.bar.slots();
        if !changed || got != slots(first) {
            return Err(format!("{name}: changed {changed}, slots {got:?}"));
        }
        seen.push(format!("{name} {got:?}"));
    }
    check(true, seen.join("; "))
}

fn settle_is_idempotent() -> Result<String, String> {
    let again: Vec<(&str, bool)> = CHARACTERS
        .iter()
        .map(|(name, sheet, _)| {
            let mut sheet = sheet();
            settled(&mut sheet);
            (*name, settled(&mut sheet))
        })
        .collect();
    check(
        again.iter().all(|(_, changed)| !changed),
        format!("changed by a second fill {again:?}"),
    )
}

/// What happens to the Jedi's filled bar, and its first four slots after
/// the next fill.
type Later = (&'static str, fn(&mut Sheet), [Option<Content>; FILLED]);

const LATER: [Later; 4] = [
    (
        "bar_learned_power_takes_empty_slot",
        |sheet| {
            sheet.bar.clear(3);
            sheet.powers.insert(STUN);
        },
        [
            Some(Content::Power(PUSH)),
            Some(Content::Power(LIGHTNING)),
            Some(Content::Power(CHOKE)),
            Some(Content::Power(STUN)),
        ],
    ),
    (
        "bar_learned_tier_replaces_in_place",
        |sheet| sheet.powers.insert(STORM),
        [
            Some(Content::Power(PUSH)),
            Some(Content::Power(STORM)),
            Some(Content::Power(CHOKE)),
            Some(Content::Item(Shelf::Medpac)),
        ],
    ),
    (
        "bar_full_bar_is_left_alone",
        |sheet| sheet.powers.insert(STUN),
        [
            Some(Content::Power(PUSH)),
            Some(Content::Power(LIGHTNING)),
            Some(Content::Power(CHOKE)),
            Some(Content::Item(Shelf::Medpac)),
        ],
    ),
    (
        "bar_cleared_slot_is_not_refilled_with_a_seen_power",
        |sheet| {
            sheet.bar.clear(0);
        },
        [
            None,
            Some(Content::Power(LIGHTNING)),
            Some(Content::Power(CHOKE)),
            Some(Content::Item(Shelf::Medpac)),
        ],
    ),
];

fn later((_, change, first): Later) -> Result<String, String> {
    let mut sheet = jedi();
    settled(&mut sheet);
    change(&mut sheet);
    settled(&mut sheet);
    let got = *sheet.bar.slots();
    check(got == slots(first), format!("slots {got:?}"))
}

/// The Jedi's wheel with Lightning alone on the bar: Heal comes before
/// Push, and neither Lightning nor Shock is listed. Shock is listed once
/// shown, and Push is not once hidden.
fn wheel_lists() -> Result<String, String> {
    let mut sheet = jedi();
    sheet.bar.put(0, Content::Power(LIGHTNING));
    sheet.bar.filled = true;
    with_rules(false, |rules| {
        let listed = |sheet: &Sheet| wheel(sheet, rules, 0);
        let first = listed(&sheet);
        let at = |content| {
            first
                .iter()
                .position(|listed| *listed == Content::Power(content))
        };
        let ordered = matches!((at(HEAL), at(PUSH)), (Some(heal), Some(push)) if heal < push);
        let left_out = at(LIGHTNING).is_none() && at(SHOCK).is_none();
        let mut bar = sheet.bar;
        bar.hide(&sheet, rules, SHOCK, false);
        bar.hide(&sheet, rules, PUSH, true);
        sheet.bar = bar;
        let after = listed(&sheet);
        let shock = after.contains(&Content::Power(SHOCK));
        let push = after.contains(&Content::Power(PUSH));
        check(
            ordered && left_out && shock && !push,
            format!("first {first:?}, after showing Shock and hiding Push {after:?}"),
        )
    })
}

fn wheel_mines_need_demolitions() -> Result<String, String> {
    let listed: Vec<(u8, bool)> = [0, 1]
        .into_iter()
        .map(|ranks| {
            let mut sheet = soldier();
            sheet.skills[skills::DEMOLITIONS] = ranks;
            let listed = with_rules(true, |rules| wheel(&sheet, rules, 0));
            (ranks, listed.contains(&Content::Mine))
        })
        .collect();
    check(
        listed == [(0, false), (1, true)],
        format!("ranks and mines listed {listed:?}"),
    )
}

fn wheel_empty() -> Result<String, String> {
    let mut sheet = droid();
    settled(&mut sheet);
    let listed = with_rules(true, |rules| wheel(&sheet, rules, 0));
    check(listed.is_empty(), format!("listed {listed:?}"))
}

/// The soldier as a player on the proving ground: after one step the
/// world's sheet has the soldier's first fill.
fn filled_in_the_world() -> Result<String, String> {
    use crate::input::{Command, Seat, TickInput};
    let config = crate::WorldConfig {
        sheet: soldier(),
        ..crate::WorldConfig::one_seat()
    };
    let level = crate::determinism::proving_ground();
    let mut world = crate::SimWorld::new(config, level).map_err(|error| error.to_string())?;
    let mut input = TickInput::default();
    input.insert(Seat(0), Command::default());
    let snapshot = crate::step(&mut world, 1, &input, 8, crate::StepReason::Authority);
    let bar = snapshot
        .body(Seat(0))
        .and_then(|body| body.sheet)
        .map(|sheet| *sheet.bar.slots());
    check(
        bar == Some(slots(CHARACTERS[1].2)),
        format!("slots {bar:?}"),
    )
}

pub fn probes() -> Checks {
    type Case = fn() -> Result<String, String>;
    let cases: [(&'static str, Case); 12] = [
        ("bar_put_and_swap", put_and_swap),
        ("bar_put_out_of_range", put_out_of_range),
        ("bar_clear_empties", clear_empties),
        ("bar_content_bits", content_bits),
        ("bar_round_trip", round_trip),
        ("bar_old_snapshot_has_empty_bar", old_snapshot_has_empty_bar),
        ("bar_first_fill", first_fill),
        ("bar_settle_is_idempotent", settle_is_idempotent),
        ("bar_wheel_lists", wheel_lists),
        (
            "bar_wheel_mines_need_demolitions",
            wheel_mines_need_demolitions,
        ),
        ("bar_wheel_empty", wheel_empty),
        ("bar_filled_in_the_world", filled_in_the_world),
    ];
    let later = LATER.into_iter().map(|case| (case.0, later(case)));
    cases
        .into_iter()
        .map(|(name, run)| (name, run()))
        .chain(later)
        .collect()
}
