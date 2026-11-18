//! Checks of which bolt hits leave a burn and how large it is, on hits made
//! here (no install).

use bevy::prelude::*;
use sim::SimId;
use sim::combat::Location;

use super::{BURN_GAP_MS, BoltHit, BurnSize, burn_size, burns};
use crate::probe_kit::{Checks, check};

/// The checks, for `bolt_hits` in the approved tests.
pub fn probes() -> Checks {
    vec![
        ("burn_rule_table", burn_rule_table()),
        ("head_never", head_never()),
        ("gate_same_snapshot", gate_same_snapshot()),
        ("zero_max_hp", zero_max_hp()),
        ("burn_sizes", burn_sizes()),
    ]
}

/// The snapshot time of every hit made here.
const AT_MS: u64 = 10_000;

/// A change made to the plain hit.
type Change = fn(&mut BoltHit);

/// One row of the burn rule's table. The hit is the plain hit with `change`
/// made to it. `last_burn` is how long before the hit the body's last bolt
/// burn was laid, or `None` when it has none.
struct Row {
    name: &'static str,
    change: Change,
    last_burn: Option<u64>,
    burns: bool,
}

/// A plain hit: a bolt from a pistol that deals 10.00 points to the chest of
/// a body with 100 points of maximum vitality, without a critical hit, a
/// Power Blast, a Sniper Shot or a kill. `change` is then made to it.
fn hit(change: impl FnOnce(&mut BoltHit)) -> BoltHit {
    let mut hit = BoltHit {
        victim: SimId(1),
        by: Some(SimId(2)),
        sequence: 0,
        ms: AT_MS,
        location: Location::Chest,
        critical: false,
        power_blast: false,
        sniper_shot: false,
        killing: false,
        damage: 1000,
        max_hp: 100,
        one_handed: true,
        from: Vec3::ZERO,
        way: Vec3::X,
        node: None,
        tint: Vec3::ONE,
    };
    change(&mut hit);
    hit
}

/// The rows of the burn rule, each with the last burn 100 ms before the hit
/// unless the row says otherwise.
fn rows() -> Vec<Row> {
    let row = |name, change: Change, last_burn, burns| Row {
        name,
        change,
        last_burn,
        burns,
    };
    let recent = Some(100);
    vec![
        row("plain 10.00 damage", |_| {}, recent, false),
        row("15.00 damage", |hit| hit.damage = 1500, recent, true),
        row("14.99 damage", |hit| hit.damage = 1499, recent, false),
        row("critical", |hit| hit.critical = true, recent, true),
        row("power blast", |hit| hit.power_blast = true, recent, true),
        row("sniper shot", |hit| hit.sniper_shot = true, recent, true),
        row("killing", |hit| hit.killing = true, recent, true),
        row("last burn 500 ms before", |_| {}, Some(500), true),
        row("last burn 499 ms before", |_| {}, Some(499), false),
        row("no last burn", |_| {}, None, true),
    ]
}

/// Runs every row with `extra` made to its hit after the row's own change,
/// and lists each row whose answer is not `expected(row)`.
fn run_rows(extra: Change, expected: impl Fn(&Row) -> bool) -> Result<String, String> {
    let rows = rows();
    let wrong: Vec<String> = rows
        .iter()
        .filter_map(|row| {
            let hit = hit(|hit| {
                (row.change)(hit);
                extra(hit);
            });
            let got = burns(&hit, row.last_burn.map(|before| AT_MS - before));
            (got != expected(row)).then(|| format!("{} burns {got}", row.name))
        })
        .collect();
    listed(
        wrong,
        format!("all {} rows answer as the rule says", rows.len()),
    )
}

/// The ruling's table on a body with 100 points of maximum vitality.
fn burn_rule_table() -> Result<String, String> {
    run_rows(|_| {}, |row| row.burns)
}

/// Every row of the table, on the head, leaves no burn.
fn head_never() -> Result<String, String> {
    run_rows(|hit| hit.location = Location::Head, |_| false)
}

/// Two hits on one body in the same snapshot, with no bolt burn on it
/// before. When the heavy hit comes first, it burns and the plain one is
/// gated by that burn. When the plain hit comes first, the gate lets it
/// burn and the heavy one burns as well.
fn gate_same_snapshot() -> Result<String, String> {
    let heavy = hit(|hit| hit.damage = 2000);
    let plain = hit(|_| {});
    let laid = |order: [&BoltHit; 2]| {
        let mut last = None;
        order.map(|hit| {
            let burnt = burns(hit, last);
            if burnt {
                last = Some(hit.ms);
            }
            burnt
        })
    };
    let heavy_first = laid([&heavy, &plain]);
    let plain_first = laid([&plain, &heavy]);
    check(
        heavy_first == [true, false] && plain_first == [true, true],
        format!(
            "heavy then plain burn {heavy_first:?}; plain then heavy burn {plain_first:?} \
             (the gate is {BURN_GAP_MS} ms)"
        ),
    )
}

/// A body with no maximum vitality counts no hit as heavy, so only the gate
/// decides.
fn zero_max_hp() -> Result<String, String> {
    let plain = hit(|hit| hit.max_hp = 0);
    let gated = burns(&plain, Some(AT_MS - 100));
    let first = burns(&plain, None);
    check(
        !gated && first,
        format!("with the last burn 100 ms before it burns {gated}; with none it burns {first}"),
    )
}

/// A pistol's burn is small, a rifle's larger, and a Power Blast's the
/// largest from either.
fn burn_sizes() -> Result<String, String> {
    let table = [
        ((true, false), BurnSize::Pistol),
        ((false, false), BurnSize::Rifle),
        ((true, true), BurnSize::PowerBlast),
        ((false, true), BurnSize::PowerBlast),
    ];
    let wrong: Vec<String> = table
        .iter()
        .filter_map(|&((one_handed, power_blast), size)| {
            let got = burn_size(one_handed, power_blast);
            (got != size)
                .then(|| format!("one-handed {one_handed}, Power Blast {power_blast} is {got:?}"))
        })
        .collect();
    listed(
        wrong,
        "a pistol's burn is Pistol, a rifle's Rifle, a Power Blast's PowerBlast".into(),
    )
}

/// Passes with `all_right` when nothing is `wrong`, and fails naming each
/// wrong case otherwise.
fn listed(wrong: Vec<String>, all_right: String) -> Result<String, String> {
    if wrong.is_empty() {
        check(true, all_right)
    } else {
        check(false, wrong.join("; "))
    }
}
