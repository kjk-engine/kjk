//! The probes of the action bar's stored state (`combat::bar`), no install:
//! - putting a content in a slot moves it there, and a content already on
//!   the bar swaps places with what the slot held;
//! - a slot past the bar's end is refused and nothing changes;
//! - clearing a slot empties it;
//! - the bar comes back whole from its bytes;
//! - a sheet from a snapshot older than the bar reads an empty bar.

use crate::combat::Sheet;
use crate::combat::bar::{Bar, Content, FeatLine};
use crate::combat::items::Shelf;
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
    let placed = bar.put(crate::combat::bar::SLOTS, Content::Stealth);
    check(
        !placed && bar == Bar::default(),
        format!("placed {placed}, bar {bar:?}"),
    )
}

fn clear_empties() -> Result<String, String> {
    let mut bar = Bar::default();
    bar.put(0, Content::Mine);
    let cleared = bar.clear(0);
    let past_end = bar.clear(crate::combat::bar::SLOTS);
    check(
        cleared && !past_end && bar.slots()[0].is_none(),
        format!("cleared {cleared}, past the end {past_end}, bar {bar:?}"),
    )
}

/// A bar holding each kind of content, two hidden powers, one shown lower
/// tier, what the fill has seen, and the first fill done.
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
    bar.filled = true;
    bar
}

fn round_trip() -> Result<String, String> {
    let bar = full_bar();
    let mut out = Writer::default();
    bar.encode(&mut out);
    let mut input = Reader::new("probe", &out.0);
    let read = Bar::decode(&mut input);
    let whole = input.finish();
    check(
        read.as_ref() == Ok(&bar) && whole.is_ok(),
        format!("sent {bar:?}, read {read:?}, rest {whole:?}"),
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

pub fn probes() -> Checks {
    type Case = fn() -> Result<String, String>;
    let cases: [(&'static str, Case); 5] = [
        ("bar_put_and_swap", put_and_swap),
        ("bar_put_out_of_range", put_out_of_range),
        ("bar_clear_empties", clear_empties),
        ("bar_round_trip", round_trip),
        ("bar_old_snapshot_has_empty_bar", old_snapshot_has_empty_bar),
    ];
    cases.into_iter().map(|(name, run)| (name, run())).collect()
}
