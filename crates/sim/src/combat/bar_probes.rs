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
//!   the player cleared takes the next power learned, never one the fill
//!   had already seen;
//! - the wheel lists offensive, then defensive powers, then
//!   the item kinds, then mines, leaves out what
//!   is on the bar, what is hidden and lower tiers not shown, and lists
//!   mines only for a character with a rank in Demolitions;
//! - a player's bar is filled by the host's first step, and a client's
//!   predicted step leaves it unfilled;
//! - with no game data, the fill and the wheel go by Jedi Academy's own
//!   powers: their chains and which of them are used on enemies;
//! - choosing a feat attack sets it for the weapon in hand, and choosing it
//!   again turns it off, but only for a line the character knows and the
//!   weapon can use;
//! - the bar edit and feat attack actions come back whole from their bytes;
//! - a seat edits only the bars of the party members it may control, and
//!   puts on them only what the character may put there.

use std::collections::BTreeMap;

use crate::action::{Action, BarEdit};
use crate::combat::Sheet;
use crate::combat::bar::{Bar, Content, Contents, FILLED, FeatLine, Rules, SLOTS, wheel};
use crate::combat::force::PULL;
use crate::combat::items::{Carried, KINDS, Shelf};
use crate::combat::picks::{Picks, Shown};
use crate::combat::sheet::{Blaster, Delivery, Rows, SaberShape, feats, races, skills};
use crate::combat::spells::{
    FORCE_POWER_CHOKE as CHOKE, FORCE_POWER_CURE as CURE, FORCE_POWER_FORCE_PUSH as PUSH,
    FORCE_POWER_FORCE_STORM as STORM, FORCE_POWER_HEAL as HEAL, FORCE_POWER_LIGHTNING as LIGHTNING,
    FORCE_POWER_SHOCK as SHOCK, FORCE_POWER_SPEED_BURST as SPEED, FORCE_POWER_STUN as STUN,
    FORCE_POWER_WOUND as WOUND,
};
use crate::inventory::{Catalog, ItemFacts, PARTY};
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
            sheet.powers.insert(STUN);
        },
        [
            Some(Content::Power(STUN)),
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

/// The Jedi with Burst of Speed, a rank in Demolitions and a mine kit in
/// the party. The wheel lists its offensive powers first: Push, Pull,
/// Lightning and Choke. Pull is listed because the Jedi knows Push, and it
/// is offensive because Push is. Shock is left out as Lightning's lower
/// tier. Then come its defensive powers (Burst of Speed and Heal), then the
/// medpacs, then mines. Within a group the powers keep the game's order.
fn wheel_groups() -> Result<String, String> {
    use crate::combat::bar::Group::{Defensive, Items, Offensive, Other};
    let mut sheet = jedi();
    sheet.powers = sheet.powers.with(SPEED);
    sheet.skills[skills::DEMOLITIONS] = 1;
    let (listed, groups): (Vec<Content>, Vec<_>) = with_rules(true, |rules| {
        let listed = wheel(&sheet, rules, 0, &|_| false);
        let groups = listed.iter().map(|content| rules.group(*content)).collect();
        (listed, groups)
    });
    let want = [PUSH, PULL, LIGHTNING, CHOKE, SPEED, HEAL]
        .map(Content::Power)
        .into_iter()
        .chain([Content::Item(Shelf::Medpac), Content::Mine]);
    let want_groups = [
        Offensive, Offensive, Offensive, Offensive, Defensive, Defensive, Items, Other,
    ];
    check(
        listed.iter().copied().eq(want) && groups == want_groups,
        format!("listed {listed:?}, groups {groups:?}"),
    )
}

/// The Jedi's wheel with Lightning alone on the bar: Push comes before
/// Heal, and neither Lightning nor Shock is listed. Shock is listed once
/// shown, and Push is not once hidden.
fn wheel_lists() -> Result<String, String> {
    let mut sheet = jedi();
    sheet.bar.put(0, Content::Power(LIGHTNING));
    sheet.bar.filled = true;
    with_rules(false, |rules| {
        let listed = |sheet: &Sheet| wheel(sheet, rules, 0, &|_| false);
        let first = listed(&sheet);
        let at = |content| {
            first
                .iter()
                .position(|listed| *listed == Content::Power(content))
        };
        let ordered = matches!((at(HEAL), at(PUSH)), (Some(heal), Some(push)) if push < heal);
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
            let listed = with_rules(true, |rules| wheel(&sheet, rules, 0, &|_| false));
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
    let listed = with_rules(true, |rules| wheel(&sheet, rules, 0, &|_| false));
    check(listed.is_empty(), format!("listed {listed:?}"))
}

/// The soldier as a player on the proving ground, after one step. The
/// host's step gives the world's sheet the soldier's first fill. A
/// client's predicted step leaves the bar unfilled, for the host's
/// snapshot to bring.
fn filled_in_the_world() -> Result<String, String> {
    use crate::StepReason;
    use crate::input::{Command, Seat, TickInput};
    let cases = [
        (StepReason::Authority, Some((slots(CHARACTERS[1].2), true))),
        (StepReason::Predict, Some((slots([None; FILLED]), false))),
    ];
    let mut seen = Vec::new();
    for (reason, want) in cases {
        let config = crate::WorldConfig {
            sheet: soldier(),
            ..crate::WorldConfig::one_seat()
        };
        let level = crate::determinism::proving_ground();
        let mut world = crate::SimWorld::new(config, level).map_err(|error| error.to_string())?;
        let mut input = TickInput::default();
        input.insert(Seat(0), Command::default());
        let snapshot = crate::step(&mut world, 1, &input, 8, reason);
        let bar = snapshot
            .body(Seat(0))
            .and_then(|body| body.sheet)
            .map(|sheet| (*sheet.bar.slots(), sheet.bar.filled));
        if bar != want {
            return Err(format!("{reason:?}: slots and filled {bar:?}"));
        }
        seen.push(format!("{reason:?} {bar:?}"));
    }
    check(true, seen.join("; "))
}

/// The Jedi with no game data, as on the combat arena: its first fill
/// takes its attack powers, Shock left out as Lightning's lower tier, and
/// the wheel lists Pull before Heal.
fn no_library() -> Result<String, String> {
    let mut sheet = jedi();
    let (bar, listed) = Rules::with(None, false, |rules| {
        let mut bar = sheet.bar;
        bar.settle(&sheet, rules, 0);
        sheet.bar = bar;
        (*bar.slots(), wheel(&sheet, rules, 0, &|_| false))
    });
    let at = |row| {
        listed
            .iter()
            .position(|listed| *listed == Content::Power(row))
    };
    let ordered = matches!((at(HEAL), at(PULL)), (Some(heal), Some(pull)) if pull < heal);
    check(
        bar == slots(CHARACTERS[0].2) && ordered,
        format!("slots {bar:?}, wheel {listed:?}"),
    )
}

/// A feat attack chosen twice for the weapon in the right hand: how the
/// weapon was held, the feats the character knows, the line chosen, and
/// what each of the two choices returns, leaves the HUD showing and leaves
/// in use (`Picks::in_use`).
type Toggle = (
    &'static str,
    (Delivery, usize),
    &'static [u16],
    FeatLine,
    [(bool, Option<Shown>, Option<FeatLine>); 2],
);

const TWO_BLADES: (Delivery, usize) = (Delivery::Saber(SaberShape::Single), 2);
const ONE_BLADE: (Delivery, usize) = (Delivery::Saber(SaberShape::Single), 1);
const PISTOL: (Delivery, usize) = (Delivery::Blaster(Blaster::Pistol), 1);
const BLAST: u16 = feats::POWER_BLAST[0];
const RAPID: u16 = feats::RAPID_SHOT[0];

const TOGGLES: [Toggle; 6] = [
    (
        "two blades",
        TWO_BLADES,
        &[feats::FLURRY[0]],
        FeatLine::Flurry,
        [
            (
                true,
                Some(Shown::Feat(feats::FLURRY[0])),
                Some(FeatLine::Flurry),
            ),
            (true, Some(Shown::Nothing), None),
        ],
    ),
    (
        "one blade",
        ONE_BLADE,
        &[feats::FLURRY[0]],
        FeatLine::Flurry,
        [(false, None, None), (false, None, None)],
    ),
    (
        "two blades, Flurry not known",
        TWO_BLADES,
        &[feats::POWER_ATTACK[0]],
        FeatLine::Flurry,
        [
            (false, Some(Shown::Nothing), None),
            (false, Some(Shown::Nothing), None),
        ],
    ),
    (
        "pistol",
        PISTOL,
        &[BLAST, RAPID],
        FeatLine::RapidShot,
        [
            (true, Some(Shown::Feat(RAPID)), Some(FeatLine::RapidShot)),
            (false, Some(Shown::Feat(RAPID)), Some(FeatLine::RapidShot)),
        ],
    ),
    (
        "pistol, the line it uses before any choice",
        PISTOL,
        &[BLAST, RAPID],
        FeatLine::PowerBlast,
        [
            (false, Some(Shown::Feat(BLAST)), Some(FeatLine::PowerBlast)),
            (false, Some(Shown::Feat(BLAST)), Some(FeatLine::PowerBlast)),
        ],
    ),
    (
        "pistol, a melee line",
        PISTOL,
        &[BLAST, RAPID, feats::FLURRY[0]],
        FeatLine::Flurry,
        [
            (false, Some(Shown::Feat(BLAST)), Some(FeatLine::PowerBlast)),
            (false, Some(Shown::Feat(BLAST)), Some(FeatLine::PowerBlast)),
        ],
    ),
];

/// Choosing a feat attack for blades or empty hands sets it, and choosing
/// it again turns it off. A gun always uses one of the lines the character
/// knows, so choosing the line it uses already changes nothing. A single
/// blade, a line the character does not know and a line the weapon cannot
/// use are refused.
fn feat_set_and_toggle() -> Result<String, String> {
    let mut seen = Vec::new();
    for (name, (delivery, hands), known, line, want) in TOGGLES {
        let mut sheet = crate::world::picks_probes::holding(delivery, hands, known);
        let got = [(); 2].map(|()| {
            let set = Picks::set(&mut sheet, 0, line);
            (set, Picks::shown(&sheet, 0), Picks::in_use(&sheet, 0))
        });
        if got != want {
            return Err(format!("{name}: {got:?}"));
        }
        seen.push(format!("{name} {got:?}"));
    }
    check(true, seen.join("; "))
}

/// Both actions come back whole from their bytes, and a feat line past the
/// last is refused.
fn action_round_trip() -> Result<String, String> {
    let bar = |edit| Action::Bar { who: 7, edit };
    let put = |slot, content| bar(BarEdit::Put { slot, content });
    let actions = [
        put(5, Content::Power(PUSH)),
        put(0, Content::Item(Shelf::Grenade)),
        put(1, Content::Mine),
        put(2, Content::Feat(FeatLine::SniperShot)),
        put(3, Content::Stealth),
        bar(BarEdit::Clear { slot: 4 }),
        bar(BarEdit::Hide {
            row: 300,
            hide: true,
        }),
        bar(BarEdit::Hide {
            row: CHOKE,
            hide: false,
        }),
        Action::Feat {
            hand: 1,
            line: FeatLine::Flurry,
        },
    ];
    let wrong: Vec<&Action> = actions
        .iter()
        .filter(|action| Action::from_bytes(&action.to_bytes()).as_ref() != Ok(*action))
        .collect();
    let mut past = Action::Feat {
        hand: 0,
        line: FeatLine::SniperShot,
    }
    .to_bytes();
    if let Some(line) = past.last_mut() {
        *line = FeatLine::ALL.len() as u8;
    }
    let refused = Action::from_bytes(&past).is_err();
    check(
        wrong.is_empty() && refused,
        format!("not read back {wrong:?}, a line past the last refused {refused}"),
    )
}

/// The template of the mine kit the edit probes give the party.
const KIT: &str = "probe_kit";

/// Two players on the proving ground, each the soldier with a rank in
/// Demolitions, after the host's first step has filled their bars. With
/// `kits`, the party carries a mine kit. Also the two players' ids.
fn edit_world(kits: bool) -> Result<(crate::SimWorld, [u32; 2]), String> {
    use crate::input::Seat;
    let kit = ItemFacts {
        trap: Some(3),
        stacking: 99,
        ..ItemFacts::plain(KIT)
    };
    // Seats' actions run with the scripts' step: a library, here an empty one.
    let library = crate::probe::library::<String>("void ActionWait(float fSeconds);", [])
        .with_items(Catalog::new([kit]));
    let catalog = library.items().clone();
    let level = crate::determinism::proving_ground().with_scripts(library);
    let mut sheet = soldier();
    sheet.skills[skills::DEMOLITIONS] = 1;
    let config = crate::WorldConfig {
        sheet,
        seats: 2,
        ..crate::WorldConfig::one_seat()
    };
    let mut world = crate::SimWorld::new(config, level).map_err(|error| error.to_string())?;
    if kits {
        let mut state = world.script_state().clone();
        state.make_item(&catalog, KIT, PARTY, 1, None);
        world.set_script_state(state);
    }
    let snapshot = edit_step(&mut world, None);
    let body = |seat| snapshot.body(Seat(seat)).map(|body| body.id.0);
    match (body(0), body(1)) {
        (Some(first), Some(second)) => Ok((world, [first, second])),
        _ => Err("no players".into()),
    }
}

/// One host step, a seat doing an action if one is given.
fn edit_step(
    world: &mut crate::SimWorld,
    action: Option<(u8, Action)>,
) -> crate::snapshot::Snapshot {
    use crate::input::{Command, Seat, TickInput};
    let mut input = TickInput::default();
    for seat in 0..2 {
        input.insert(Seat(seat), Command::default());
    }
    if let Some((seat, action)) = action {
        input.act(Seat(seat), action);
    }
    let tick = world.tick() + 1;
    crate::step(world, tick, &input, 8, crate::StepReason::Authority)
}

/// An edit through the world: its name, the seat that asks, the action
/// made from the two players' ids, whether the party carries a mine kit,
/// and whether the first player's bar or feat choices change.
type Edit = (&'static str, u8, fn([u32; 2]) -> Action, bool, bool);

const EDITS: [Edit; 9] = [
    (
        "a feat attack moved to the last slot",
        0,
        |[me, _]| Action::Bar {
            who: me,
            edit: BarEdit::Put {
                slot: 5,
                content: Content::Feat(FeatLine::PowerBlast),
            },
        },
        false,
        true,
    ),
    (
        "a slot cleared",
        0,
        |[me, _]| Action::Bar {
            who: me,
            edit: BarEdit::Clear { slot: 0 },
        },
        false,
        true,
    ),
    (
        "a slot past the end cleared",
        0,
        |[me, _]| Action::Bar {
            who: me,
            edit: BarEdit::Clear { slot: SLOTS as u8 },
        },
        false,
        false,
    ),
    (
        "a power the character does not know",
        0,
        |[me, _]| Action::Bar {
            who: me,
            edit: BarEdit::Put {
                slot: 5,
                content: Content::Power(PUSH),
            },
        },
        false,
        false,
    ),
    (
        "mines without a kit",
        0,
        |[me, _]| Action::Bar {
            who: me,
            edit: BarEdit::Put {
                slot: 5,
                content: Content::Mine,
            },
        },
        false,
        false,
    ),
    (
        "mines with a kit",
        0,
        |[me, _]| Action::Bar {
            who: me,
            edit: BarEdit::Put {
                slot: 5,
                content: Content::Mine,
            },
        },
        true,
        true,
    ),
    (
        "another seat's player",
        1,
        |[other, _]| Action::Bar {
            who: other,
            edit: BarEdit::Clear { slot: 0 },
        },
        false,
        false,
    ),
    (
        "the feat attack the pistol uses already",
        0,
        |_| Action::Feat {
            hand: 0,
            line: FeatLine::PowerBlast,
        },
        false,
        false,
    ),
    (
        "a melee feat attack chosen for the pistol",
        0,
        |_| Action::Feat {
            hand: 0,
            line: FeatLine::PowerAttack,
        },
        false,
        false,
    ),
];

/// A seat edits the bar of a party member it may control, with what the
/// character may put on the bar. A power the character does not know,
/// mines without a kit, a slot past the end and another seat's player are
/// refused. A feat attack the weapon in hand cannot use is refused too.
fn edit_refused() -> Result<String, String> {
    use crate::input::Seat;
    let mut seen = Vec::new();
    for (name, seat, action, kits, want) in EDITS {
        let (mut world, bodies) = edit_world(kits)?;
        let state = |snapshot: &crate::snapshot::Snapshot| {
            snapshot
                .body(Seat(0))
                .and_then(|body| body.sheet)
                .map(|sheet| (sheet.bar, sheet.picks))
        };
        let before = state(&world.snapshot());
        let after = state(&edit_step(&mut world, Some((seat, action(bodies)))));
        let changed = before.is_some() && before != after;
        if changed != want {
            return Err(format!("{name}: changed {changed}, bar {after:?}"));
        }
        seen.push(format!("{name} {changed}"));
    }
    check(true, seen.join("; "))
}

pub fn probes() -> Checks {
    type Case = fn() -> Result<String, String>;
    let cases: [(&'static str, Case); 17] = [
        ("bar_put_and_swap", put_and_swap),
        ("bar_put_out_of_range", put_out_of_range),
        ("bar_clear_empties", clear_empties),
        ("bar_content_bits", content_bits),
        ("bar_round_trip", round_trip),
        ("bar_old_snapshot_has_empty_bar", old_snapshot_has_empty_bar),
        ("bar_first_fill", first_fill),
        ("bar_settle_is_idempotent", settle_is_idempotent),
        ("bar_wheel_lists", wheel_lists),
        ("bar_wheel_groups", wheel_groups),
        (
            "bar_wheel_mines_need_demolitions",
            wheel_mines_need_demolitions,
        ),
        ("bar_wheel_empty", wheel_empty),
        ("bar_filled_in_the_world", filled_in_the_world),
        ("bar_no_library", no_library),
        ("bar_feat_set_and_toggle", feat_set_and_toggle),
        ("bar_action_round_trip", action_round_trip),
        ("bar_edit_refused", edit_refused),
    ];
    let later = LATER.into_iter().map(|case| (case.0, later(case)));
    cases
        .into_iter()
        .map(|(name, run)| (name, run()))
        .chain(later)
        .collect()
}
