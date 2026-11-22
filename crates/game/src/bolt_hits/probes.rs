//! Checks of which bolt hits leave a burn and how large it is, and of which
//! bolt as drawn made a hit, on hits and bolts made here (no install).

use asset_ja::UNIT;
use bevy::prelude::*;
use sim::SimId;
use sim::combat::Location;
use sim::combat::sheet::Weapon;

use super::{
    BURN_GAP_MS, BoltHit, BurnSize, FLINCH_DEG, FLINCH_MOST_DEG, FLINCH_RISE_MS, Trail, blow_way,
    burn_size, burns, flinch_deg, flinch_turn, line_of, nearest_on_line, pick_trail,
};
use crate::combat_fx::CONVERGE;
use crate::probe_kit::{Checks, check};

/// The checks, for `bolt_hits` in the approved tests.
pub fn probes() -> Checks {
    vec![
        ("burn_rule_table", burn_rule_table()),
        ("head_never", head_never()),
        ("armoured_head_burns", armoured_head_burns()),
        ("gate_same_snapshot", gate_same_snapshot()),
        ("zero_max_hp", zero_max_hp()),
        ("burn_sizes", burn_sizes()),
        ("pick_nearest", pick_nearest()),
        ("pick_reach", pick_reach()),
        ("pick_stale", pick_stale()),
        ("line_as_drawn", line_as_drawn()),
        ("pick_past", pick_past()),
        ("light_on_body", light_on_body()),
        ("flinch_curve", flinch_curve()),
        ("flinch_away", flinch_away()),
        ("flinch_capped", flinch_capped()),
        ("blow_follows_bolt", blow_follows_bolt()),
    ]
}

/// A flinch turns its node not at all as the bolt hits, 6° 40 ms later, a
/// little less at every 10 ms from 40 ms to 300 ms, and not at all from
/// 300 ms on.
fn flinch_curve() -> Result<String, String> {
    let at = |ms: f32| flinch_deg(ms);
    let falling = (4..30).all(|step| at(step as f32 * 10.0 + 10.0) < at(step as f32 * 10.0));
    let after = [300.0, 350.0, 1000.0].map(at);
    check(
        at(0.0) == 0.0
            && (at(FLINCH_RISE_MS) - FLINCH_DEG).abs() < 1e-4
            && falling
            && after.iter().all(|&deg| deg == 0.0),
        format!(
            "{:.2}° at 0 ms, {:.2}° at 40 ms, falling at every 10 ms from 40 to 300 ms \
             {falling}, {after:?}° at 300, 350 and 1000 ms",
            at(0.0),
            at(FLINCH_RISE_MS)
        ),
    )
}

/// A point 1 m above a node at the origin, a bolt along +X: 40 ms after the
/// hit, the turn has moved the point toward +X.
fn flinch_away() -> Result<String, String> {
    let turned = flinch_turn(&[(Vec3::X, 0.0)], FLINCH_RISE_MS) * Vec3::Z;
    check(
        turned.x > 0.05,
        format!("the point 1 m above the node is now at {turned}"),
    )
}

/// Five hits in the same ms turn the node 12°, not 30°.
fn flinch_capped() -> Result<String, String> {
    let degrees = flinch_turn(&[(Vec3::X, 0.0); 5], FLINCH_RISE_MS)
        .to_scaled_axis()
        .length()
        .to_degrees();
    check(
        (degrees - FLINCH_MOST_DEG).abs() < 1e-3,
        format!("five hits at once turn the node {degrees:.2}°"),
    )
}

/// A killing bolt along (0.6, 0.8, −0.3) throws the body along (0.6, 0.8, 0)
/// made a unit vector, whatever way the striker stands. With no bolt, the
/// body is thrown along the striker's offset made level, as before.
fn blow_follows_bolt() -> Result<String, String> {
    let striker = Some(Vec3::new(3.0, 0.0, 1.0));
    let bolt = blow_way(Some(Vec3::new(0.6, 0.8, -0.3)), striker, Vec3::ZERO);
    let none = blow_way(None, striker, Vec3::Y);
    let level = Vec3::new(0.6, 0.8, 0.0).normalize();
    check(
        bolt.is_some_and(|way| way.distance(level) < 1e-5) && none == Some(Vec3::X),
        format!("with the bolt it is {bolt:?}; with no bolt it is {none:?}"),
    )
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
        armoured_head: false,
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

/// Every row of the table, on a bare head, leaves no burn.
fn head_never() -> Result<String, String> {
    run_rows(|hit| hit.location = Location::Head, |_| false)
}

/// Every row of the table, on an armoured head, answers as it does on the
/// chest. The models named as helmets are found whatever their case, and a
/// bare-faced Republic soldier's helmet is not one of them.
fn armoured_head_burns() -> Result<String, String> {
    let named = ["N_SithSoldier", "n_mandalorian", "N_SithAssnH"]
        .into_iter()
        .all(crate::bolt_hits::armoured_head)
        && !crate::bolt_hits::armoured_head("rep_soldier");
    let rows = run_rows(
        |hit| {
            hit.location = Location::Head;
            hit.armoured_head = true;
        },
        |row| row.burns,
    );
    match rows {
        Ok(said) if named => Ok(said),
        Ok(_) => Err("the helmet models are not named as the list says".to_string()),
        Err(wrong) => Err(wrong),
    }
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

/// The shooter of the bolts made here, and the snapshot time of their hit.
const SHOOTER: SimId = SimId(2);
const HIT_MS: f32 = 1000.0;

/// A bolt of `owner` fired at time 0 from `base` (units) along the x axis at
/// 1000 units a second, with no launch, last drawn `before` ms before the
/// hit.
fn bolt(owner: SimId, base: Vec3, before: f32) -> Trail {
    Trail {
        owner,
        base,
        velocity: Vec3::new(1000.0, 0.0, 0.0),
        launch: Vec3::ZERO,
        fired: 0.0,
        last_ms: HIT_MS - before,
        weapon: Weapon::UNARMED,
        power: false,
        sniper: false,
    }
}

/// Where a bolt made here starts, along the x axis (units), so that it is
/// beside the node at the origin at the hit.
const START_X: f32 = -1000.0;

/// A bolt of the shooter whose line runs `metres` from the node at the
/// origin, last drawn one frame before the hit.
fn passing(metres: f32) -> Trail {
    bolt(SHOOTER, Vec3::new(START_X, metres / UNIT, 0.0), 16.0)
}

/// How far from the node at the origin the picked trail's line runs
/// (metres), or `None` when none was picked.
fn picked(trails: &[Trail], by: SimId) -> Option<f32> {
    pick_trail(trails, by, HIT_MS, Vec3::ZERO).map(|trail| trail.base.y * UNIT)
}

/// Of two bolts of one shooter, 0.2 m and 0.9 m from the node, the nearer
/// one is picked, in either order.
fn pick_nearest() -> Result<String, String> {
    let orders = [[passing(0.9), passing(0.2)], [passing(0.2), passing(0.9)]];
    let got = orders.map(|trails| picked(&trails, SHOOTER));
    let near = |found: Option<f32>| found.is_some_and(|metres| (metres - 0.2).abs() < 1e-3);
    check(
        got.iter().all(|&found| near(found)),
        format!("picked the lines {got:?} m from the node"),
    )
}

/// A bolt 2 m from the node is out of reach, and another shooter's bolt is
/// never picked.
fn pick_reach() -> Result<String, String> {
    let far = picked(&[passing(2.0)], SHOOTER);
    let other = picked(&[passing(0.2)], SimId(3));
    check(
        far.is_none() && other.is_none(),
        format!("a bolt 2 m away picks {far:?}; another shooter's bolt picks {other:?}"),
    )
}

/// A bolt last drawn 300 ms before the hit is not picked. The same bolt
/// drawn 200 ms before it is.
fn pick_stale() -> Result<String, String> {
    let at = Vec3::new(START_X, 0.2 / UNIT, 0.0);
    let stale = picked(&[bolt(SHOOTER, at, 300.0)], SHOOTER);
    let fresh = picked(&[bolt(SHOOTER, at, 200.0)], SHOOTER);
    check(
        stale.is_none() && fresh.is_some(),
        format!("drawn 300 ms before it picks {stale:?}; drawn 200 ms before it picks {fresh:?}"),
    )
}

/// A bolt that is two ticks' flight past the node at the hit missed and flew
/// on, so it is not picked. A bolt half a tick's flight past the node is
/// picked, since the hit came within the tick.
fn pick_past() -> Result<String, String> {
    let one_tick = 1000.0 / sim::TICK_HZ as f32;
    let past = |ticks: f32| {
        let start = Vec3::new(START_X + one_tick * ticks, 0.2 / UNIT, 0.0);
        picked(&[bolt(SHOOTER, start, 16.0)], SHOOTER)
    };
    let (far, near) = (past(2.0), past(0.5));
    check(
        far.is_none() && near.is_some(),
        format!("two ticks past it picks {far:?}; half a tick past it picks {near:?}"),
    )
}

/// The light of a hit sits where the bolt's line passes the node. A bolt
/// drawn 5 cm past the node, on a line 10 cm from it, puts the light 10 cm
/// from the node, not 5 cm beyond it.
fn light_on_body() -> Result<String, String> {
    let node = Vec3::ZERO;
    let way = Vec3::X;
    let from = Vec3::new(0.05, 0.1, 0.0);
    let spot = nearest_on_line((from, way), node);
    let off = spot.distance(node);
    check(
        (off - 0.1).abs() < 1e-4,
        format!("the light sits {:.1} cm from the node", off * 100.0),
    )
}

/// A bolt flying past `CONVERGE` units is drawn on its own way: its point is
/// `base + velocity × t`. Before that it is drawn off its way by the part of
/// its launch still left, as the drawn bolt is.
fn line_as_drawn() -> Result<String, String> {
    let trail = Trail {
        launch: Vec3::new(0.0, 0.0, 20.0),
        velocity: Vec3::new(2000.0, 0.0, 0.0),
        fired: 100.0,
        ..bolt(SHOOTER, Vec3::new(5.0, 6.0, 7.0), 0.0)
    };
    let own_way = |t: f32| trail.base + trail.velocity * (t / 1000.0);
    // 500 ms flies 1000 units, past `CONVERGE`; 50 ms flies 100 units.
    let (late, early) = (500.0, 50.0);
    let left = 1.0 - trail.velocity.length() * early / 1000.0 / CONVERGE;
    let expected = [own_way(late), own_way(early) + trail.launch * left];
    let got = [late, early].map(|t| line_of(&trail, trail.fired + t));
    let off_mm = [0, 1].map(|i| got[i].0.distance(expected[i]) * UNIT * 1000.0);
    let ways_unit = got.iter().all(|(_, way)| (way.length() - 1.0).abs() < 1e-4);
    check(
        off_mm.iter().all(|&mm| mm < 1.0) && ways_unit,
        format!(
            "past CONVERGE the point is {:.3} mm off base + velocity × t; before it {:.3} mm \
             off the launched line; ways of length 1: {ways_unit}",
            off_mm[0], off_mm[1]
        ),
    )
}
