//! The probes of targeted blocks (`kjk sim probes --only targeted`). The
//! session's flag survives a snapshot, an older snapshot reads it as off, and
//! the world turns targeted blocks on only under the Advanced rules with the
//! flag set. The sides (`duel::aim`) follow the spec's tables: the side of
//! a swing by its start quadrant and the two fighters' facings, the side of
//! the crosshair by its offsets from the chest, and the crosshair line
//! agrees with the screen of every camera. In a fight, a player's perfect
//! block needs the crosshair on the swing's side, a swing with no side
//! needs no aim, the flag off asks for no aim, the window is twice as
//! long, each of two attackers is judged against its own side, a blow held
//! for a lagged player's grace is judged against the move it was struck
//! with, and a creature's guard does not change. The reticule's threat is the fighter
//! in front and within range whose swing began first, else the nearest.

use super::super::duels::play;
use crate::Seat;
use crate::combat::mind;
use crate::combat::saber::duel::exchange::Guard;
use crate::combat::saber::duel::tap::{self, Tapped};
use crate::combat::saber::duel::{self, aim, aim::Side};
use crate::combat::saber::{Move, Quad, SaberRules, grace};
use crate::determinism::{Run, Setup};
use crate::math::{self, Vec3};
use crate::module::ObjectKind;
use crate::motion::MotionState;
use crate::probe::{Checks, check};
use crate::snapshot::{Kind, SimId, Snapshot, decode_targeted};
use crate::wire::{Reader, Versions};
use crate::{Level, SimWorld};
use collision::UNIT;

pub fn probes() -> Checks {
    vec![
        ("flag_round_trip", flag_round_trip()),
        ("old_snapshot_off", old_snapshot_off()),
        ("ja_ignores_flag", ja_ignores_flag()),
        ("swing_side_table", swing_side_table()),
        ("crosshair_side_table", crosshair_side_table()),
        ("aimed_table", aimed_table()),
        (
            "crosshair_line_matches_shots",
            crosshair_line_matches_shots(),
        ),
    ]
    .into_iter()
    .chain(AIMED.iter().map(|(name, case)| (*name, aimed_block(case))))
    .chain([
        ("window_doubled", window_doubled()),
        ("two_attackers", two_attackers()),
        ("held_blow_keeps_its_move", held_blow_keeps_its_move()),
        ("creature_guard_unchanged", creature_guard_unchanged()),
        ("threat_order", threat_order()),
    ])
    .collect()
}

/// One blow that met a blocker, as the exchange tap told it, with the side
/// its swing came from and the side of the attacker the blocker's
/// crosshair was on, both on the tick before the blow.
#[derive(Debug, Clone, Copy)]
struct Blow {
    tick: u64,
    attacker: SimId,
    guard: Guard,
    perfect: bool,
    swing: Option<Side>,
    crosshair: Option<Side>,
}

/// `text` played: every tick, and every blow that met the entity of kind
/// `blocker`.
fn blows_at(text: &str, blocker: Kind) -> Result<(Vec<Snapshot>, Vec<Blow>), String> {
    tap::listen();
    let ticks = play(text);
    let tapped = tap::take();
    tap::stop();
    let ticks = ticks?;
    let body = ticks
        .last()
        .and_then(|tick| tick.entities.iter().find(|e| e.kind == blocker))
        .map(|entity| entity.id)
        .ok_or_else(|| format!("no {blocker:?}"))?;
    let blows = tapped
        .into_iter()
        .filter_map(|tapped| match tapped {
            Tapped::Exchange {
                tick,
                a,
                d,
                guard,
                perfect,
                ..
            } if d == body => Some((tick, a, guard, perfect)),
            _ => None,
        })
        .map(|(tick, attacker, guard, perfect)| {
            let before = ticks.iter().rfind(|t| t.tick + 1 == tick);
            let of = |id: SimId| before?.entities.iter().find(|e| e.id == id);
            let them = of(attacker).and_then(|e| Some((e.fighter?, e.player?)));
            let me = of(body).and_then(|e| e.player);
            let (swing, crosshair) = match (them, me) {
                (Some((fighter, ps)), Some(me)) => aim::sides(&me, &ps, fighter.saber.current_move),
                _ => (None, None),
            };
            Blow {
                tick,
                attacker,
                guard,
                perfect,
                swing,
                crosshair,
            }
        })
        .collect();
    Ok((ticks, blows))
}

/// The keys of 30 rounds in which seat `blocker` holds block and presses
/// attack afresh every 16 ticks, more often than its perfect window lasts,
/// while the other seats press `others`. The blocker aims its crosshair at
/// the chest of seat `at` and turns it by `look`.
fn pressing(blocker: u8, at: u8, others: &str, look: &str) -> String {
    let held = format!("{others} h{blocker}=block x{blocker}=1 s{blocker}={at} {look}");
    format!("tick 15 {held} b{blocker}=attack\ntick 1 {held}\n").repeat(30)
}

/// The scenario's word for targeted blocks `flag` (`targeted on|off`).
fn on_off(flag: bool) -> &'static str {
    if flag { "on" } else { "off" }
}

/// A duel under the Advanced rules with targeted blocks `flag`. Seat 0 is
/// a `kind` fighter at x=0. Seat 1 is a level 20 medium fighter at x=45
/// facing it, with the sheet keys `sheet` (`sheet 1 KEYS`). Seat 0 plays
/// `before` once and then holds `swing`, while seat 1 blocks
/// (`pressing`) with its crosshair turned by `look`.
#[derive(Clone, Copy)]
struct Fight {
    flag: bool,
    kind: &'static str,
    sheet: &'static str,
    before: &'static str,
    swing: &'static str,
    look: &'static str,
}

/// A fight with targeted blocks on, a medium seat 0 swinging from the
/// blocker's left, and the blocker's crosshair on the attacker's chest.
const FIGHT: Fight = Fight {
    flag: true,
    kind: "medium",
    sheet: "",
    before: "",
    swing: SWING_FROM_LEFT,
    look: CENTRE,
};

/// The fight `FIGHT` with seat 0 a staff fighter that crouches and swings
/// while running forward, which plays a spin attack.
const SPIN: Fight = Fight {
    kind: "staff",
    before: "tick 20 u0=-1\n",
    swing: "b0=attack f0=1 u0=-1",
    ..FIGHT
};

/// Seat 0's keys for a swing from the right to the left of the attacker,
/// from seat 1's left (`A_R2L`).
const SWING_FROM_LEFT: &str = "b0=attack r0=-1";
/// Seat 1's crosshair turned 15 degrees to the left of the chest, to the
/// right, or not at all.
const LEFT: &str = "t1=15";
const RIGHT: &str = "t1=-15";
const CENTRE: &str = "";

impl Fight {
    fn text(self) -> String {
        let flag = on_off(self.flag);
        format!(
            "scenario probe\nrules advanced\nseed 7\ntargeted {flag}\n\
             fighter 0 {} x=0 defence=2 tiers=2 fp=100 hp=40000\n\
             fighter 1 medium x=45 yaw=180 defence=2 tiers=2 fp=100 hp=40000\n\
             sheet 1 level=20 {}\nbegin\ntick 1 b0=attack b1=attack\ntick 60\n{}{}",
            self.kind,
            self.sheet,
            self.before,
            pressing(1, 0, self.swing, self.look)
        )
    }

    /// Every tick, and every blow that met seat 1.
    fn blows(self) -> Result<(Vec<Snapshot>, Vec<Blow>), String> {
        blows_at(&self.text(), Kind::Avatar(Seat(1)))
    }
}

/// A fight and how its first aimed block (a perfect or an ordinary aimed
/// guard) must go: the swing's side and the crosshair's side it met, its
/// guard and whether it was a perfect block.
struct Aimed {
    fight: Fight,
    swing: Option<Side>,
    crosshair: Option<Side>,
    guard: Guard,
    perfect: bool,
}

const AIMED: [(&str, Aimed); 5] = [
    (
        "right_side_is_perfect",
        Aimed {
            fight: Fight {
                look: LEFT,
                ..FIGHT
            },
            swing: Some(Side::Left),
            crosshair: Some(Side::Left),
            guard: Guard::Perfect,
            perfect: true,
        },
    ),
    (
        "wrong_side_is_aimed",
        Aimed {
            fight: Fight {
                look: RIGHT,
                ..FIGHT
            },
            swing: Some(Side::Left),
            crosshair: Some(Side::Right),
            guard: Guard::Active,
            perfect: false,
        },
    ),
    (
        "centre_is_aimed",
        Aimed {
            fight: FIGHT,
            swing: Some(Side::Left),
            crosshair: None,
            guard: Guard::Active,
            perfect: false,
        },
    ),
    (
        "no_side_is_perfect",
        Aimed {
            fight: Fight {
                look: RIGHT,
                ..SPIN
            },
            swing: None,
            crosshair: Some(Side::Right),
            guard: Guard::Perfect,
            perfect: true,
        },
    ),
    (
        "flag_off_ignores_aim",
        Aimed {
            fight: Fight {
                flag: false,
                look: RIGHT,
                ..FIGHT
            },
            swing: Some(Side::Left),
            crosshair: Some(Side::Right),
            guard: Guard::Perfect,
            perfect: true,
        },
    ),
];

fn aimed_block(case: &Aimed) -> Result<String, String> {
    let (_, blows) = case.fight.blows()?;
    let blow = blows
        .iter()
        .find(|blow| matches!(blow.guard, Guard::Perfect | Guard::Active))
        .ok_or_else(|| format!("no aimed block among {} blows", blows.len()))?;
    check(
        (blow.swing, blow.crosshair, blow.guard, blow.perfect)
            == (case.swing, case.crosshair, case.guard, case.perfect),
        format!(
            "the first aimed block, on tick {}: the swing from {:?}, the crosshair on {:?}, guard {:?}, perfect {}; expected {:?}, {:?}, {:?}, {}",
            blow.tick,
            blow.swing,
            blow.crosshair,
            blow.guard,
            blow.perfect,
            case.swing,
            case.crosshair,
            case.guard,
            case.perfect
        ),
    )
}

/// The perfect window seat 1 opened first (`perfect_until - aimed_at`, ms)
/// with the sheet keys `sheet` and targeted blocks `flag`. Seat 0 does not
/// swing.
fn first_window(flag: bool, sheet: &'static str) -> Result<u64, String> {
    let (ticks, _) = Fight {
        flag,
        sheet,
        swing: "",
        ..FIGHT
    }
    .blows()?;
    ticks
        .iter()
        .filter_map(|tick| tick.own(Seat(1)).and_then(|entity| entity.fighter))
        .map(|f| f.saber.duel)
        .find(|duel| duel.perfect_until > 0)
        .map(|duel| duel.perfect_until - duel.aimed_at)
        .ok_or_else(|| format!("no perfect window opened with {sheet}"))
}

/// With targeted blocks on, the perfect window is twice as long as with
/// them off, for Reflex saves 0 and 6.
fn window_doubled() -> Result<String, String> {
    let rows = [(0, "reflex=0"), (6, "reflex=6")]
        .into_iter()
        .map(|(reflex, sheet)| {
            let (on, off) = (first_window(true, sheet)?, first_window(false, sheet)?);
            Ok((
                format!("Reflex {reflex}: {on} ms on, {off} ms off"),
                on,
                off * 2,
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let said = rows
        .iter()
        .map(|(name, ..)| name.as_str())
        .collect::<Vec<_>>()
        .join("; ");
    table(rows).map(|right| format!("{right}: {said}"))
}

/// Seat 0 blocks two attackers that stand 30 degrees to each side of its
/// line and whose first blows meet it on the same tick. Seat 1's swing
/// comes from seat 0's left and seat 2's from its right. Seat 0's
/// crosshair is on seat 1's left, so it is on seat 2's left too. The first
/// blow of seat 1 meets a perfect block, and the first of seat 2 does not.
fn two_attackers() -> Result<String, String> {
    let fighter = |seat: u8, y: f32| {
        format!("fighter {seat} medium x=39 y={y} yaw=180 defence=2 tiers=2 fp=100 hp=40000\n")
    };
    let text = format!(
        "scenario probe\nrules advanced\nseed 7\ntargeted on\n\
         fighter 0 medium x=0 defence=2 tiers=2 fp=100 hp=40000\n{}{}\
         sheet 0 level=20\nbegin\ntick 1 b0=attack b1=attack b2=attack\ntick 60\n{}",
        fighter(1, 22.5),
        fighter(2, -22.5),
        pressing(0, 1, "b1=attack r1=-1 s1=0 b2=attack r2=1 s2=0", "t0=10")
    );
    let (ticks, blows) = blows_at(&text, Kind::Avatar(Seat(0)))?;
    let seat = |n: u8| {
        ticks
            .last()
            .and_then(|tick| tick.own(Seat(n)))
            .map(|e| e.id)
    };
    let first = |n: u8| {
        blows
            .iter()
            .find(|blow| Some(blow.attacker) == seat(n))
            .copied()
    };
    let (one, two) = (first(1), first(2));
    let right = match (one, two) {
        (Some(one), Some(two)) => {
            let left = Some(Side::Left);
            one.tick == two.tick
                && (one.swing, one.crosshair, one.guard, one.perfect)
                    == (left, left, Guard::Perfect, true)
                && (two.swing, two.crosshair, two.guard, two.perfect)
                    == (Some(Side::Right), left, Guard::Active, false)
        }
        _ => false,
    };
    check(
        right,
        format!("the first blow of seat 1: {one:?}; of seat 2: {two:?}"),
    )
}

/// The tick seat 0 starts its swing in `lagged`, after its saber is drawn.
const LAGGED_SWING: u64 = 182;

/// A duel under the Advanced rules with targeted blocks on, in which seat 1
/// plays with a view lag of 200 ms, so a blade's hit on it is held for its
/// grace. Seat 0 swings once from seat 1's left, and seat 1's crosshair is
/// on seat 0's left. Seat 1 holds block and presses attack from the tick
/// `block_from` on, and never with None.
fn lagged(block_from: Option<u64>) -> String {
    let look = format!("x1=1 s1=0 {LEFT}");
    let keys = |ticks: u64, keys: &str| format!("tick {ticks} {look} {keys}\n");
    let swing = keys(4, "b0=attack r0=-1");
    let after = match block_from {
        Some(from) => keys(from - LAGGED_SWING - 4, "") + &keys(80, "h1=block b1=attack"),
        None => keys(500, ""),
    };
    format!(
        "scenario probe\nrules advanced\nseed 7\ntargeted on\n\
         fighter 0 medium x=0 defence=2 tiers=2 fp=100 hp=40000\n\
         fighter 1 medium x=45 yaw=180 defence=2 tiers=2 fp=100 hp=40000\n\
         sheet 1 level=20\nbegin\nlag 1 200\ntick 1 b0=attack b1=attack\n\
         tick {}\n{swing}{after}",
        LAGGED_SWING - 2
    )
}

/// A blow held on a lagged seat is judged against the move its striker
/// played when the blade met the seat, not the move it plays when the hold
/// ends. Seat 0's blow is held on seat 1 while seat 0 goes on into its next
/// move. Seat 1 then blocks within its grace, with its crosshair on the side
/// of the held move, and the block is perfect. A snapshot taken during the
/// hold reads back with the held move.
fn held_blow_keeps_its_move() -> Result<String, String> {
    let fighter = |tick: &Snapshot, n: u8| tick.own(Seat(n)).and_then(|e| e.fighter);
    let ticks = play(&lagged(None))?;
    let switched = |held: &grace::Held| {
        ticks
            .iter()
            .filter(|tick| tick.tick > held.tick && tick.tick < held.until())
            .find(|tick| fighter(tick, 0).is_some_and(|f| f.saber.current_move != held.swung))
    };
    let (held, moved) = ticks
        .iter()
        .filter_map(|tick| fighter(tick, 1))
        .flat_map(|f| f.grace.into_iter().flatten())
        .find_map(|held| Some((held, switched(&held)?)))
        .ok_or("no blow was held on seat 1 across a move of seat 0")?;
    let body = |n: u8| moved.own(Seat(n)).and_then(|e| e.player);
    let (me, them) = body(1).zip(body(0)).ok_or("no seats")?;
    let later = fighter(moved, 0).ok_or("no seat 0")?.saber.current_move;
    let side = |mv: Move| aim::sides(&me, &them, mv);
    let kept = Snapshot::decode(&moved.encode()).is_ok_and(|again| again == *moved);
    let (_, blows) = blows_at(&lagged(Some(moved.tick + 1)), Kind::Avatar(Seat(1)))?;
    let blow = blows
        .iter()
        .find(|blow| blow.tick > held.tick)
        .ok_or("the held blow met no block")?;
    check(
        blow.guard == Guard::Perfect
            && blow.tick <= held.until()
            && side(later) != side(held.swung)
            && kept,
        format!(
            "held on tick {} until {} with {} {:?}, blocked on tick {} while seat 0 played {} {:?}: guard {:?}; the snapshot read back the same: {kept}",
            held.tick,
            held.until(),
            held.swung.name(),
            side(held.swung),
            blow.tick,
            later.name(),
            side(later),
            blow.guard
        ),
    )
}

/// The guards of the blows a creature met from seat 0's swings with
/// targeted blocks `flag`.
fn creature_guards(flag: bool) -> Result<Vec<Guard>, String> {
    let flag = on_off(flag);
    let text = format!(
        "scenario probe\nrules advanced\nseed 7\ntargeted {flag}\n\
         fighter 0 medium x=0 defence=2 tiers=2 fp=100 hp=40000\n\
         creature medium x=45 yaw=180 defence=2 tiers=2 hp=40000\n\
         begin\ntick 1 b0=attack\ntick 60\ntick 600 b0=attack c0=0\n"
    );
    let creature = Kind::Object(ObjectKind::Creature, 0);
    let (_, blows) = blows_at(&text, creature)?;
    Ok(blows.iter().map(|blow| blow.guard).collect())
}

/// A creature blocks a player's swings with a creature's guard with
/// targeted blocks on, exactly as with them off.
fn creature_guard_unchanged() -> Result<String, String> {
    let (on, off) = (creature_guards(true)?, creature_guards(false)?);
    check(
        on.contains(&Guard::Npc) && on == off,
        format!("the creature's guards with targeted blocks on: {on:?}; off: {off:?}"),
    )
}

/// The threat among hostile fighters (`aim::threat`). Of two fighters
/// swinging in front of the player within range, the one whose swing began
/// first is the threat. A fighter swinging out of range or behind the
/// player is never the threat. With nobody swinging, the nearest fighter in
/// front and within range is the threat. With nobody near there is none.
fn threat_order() -> Result<String, String> {
    let range = aim::threat_range();
    let near = |metres: f32, in_front: bool, swing_began: Option<u64>| aim::Near {
        distance: metres / UNIT,
        in_front,
        swing_began,
    };
    let (later, earlier) = (near(1.0, true, Some(900)), near(3.0, true, Some(400)));
    let (far, behind) = (near(5.0, true, Some(100)), near(1.0, false, Some(100)));
    let (close, closer) = (near(3.5, true, None), near(2.0, true, None));
    let rows: Vec<(&str, Vec<aim::Near>, Option<usize>)> = vec![
        ("the earlier swing", vec![later, earlier], Some(1)),
        ("a swing at 5 meters", vec![far, later], Some(1)),
        ("a swing behind", vec![behind, later], Some(1)),
        ("nobody swinging", vec![close, closer], Some(1)),
        (
            "a swing out of range over one at rest",
            vec![far, close],
            Some(1),
        ),
        ("only out of range and behind", vec![far, behind], None),
        ("nobody near", Vec::new(), None),
    ];
    let checked = table(
        rows.into_iter()
            .map(|(name, near, want)| (name.to_string(), aim::threat(&near), want))
            .collect(),
    )?;
    Ok(format!("{checked}; the range is {range} units"))
}

/// The ticks of a fight in which seat 1 holds block and turns its crosshair
/// to the left of seat 0 while seat 0 swings. Seat 0 swings from seat 1's
/// left, or plays a spin attack with `spin`, which has no side. The
/// reticule's probes read the view from these snapshots.
pub fn staged(spin: bool) -> Result<Vec<Snapshot>, String> {
    let fight = match spin {
        false => FIGHT,
        true => SPIN,
    };
    play(
        &Fight {
            look: LEFT,
            ..fight
        }
        .text(),
    )
}

/// Passes when every row's result equals its expected value, and lists
/// the rows that differ.
fn table<R: PartialEq + std::fmt::Debug>(rows: Vec<(String, R, R)>) -> Result<String, String> {
    let wrong: Vec<String> = rows
        .iter()
        .filter(|(_, got, want)| got != want)
        .map(|(name, got, want)| format!("{name}: {got:?}, expected {want:?}"))
        .collect();
    match wrong.is_empty() {
        true => Ok(format!("{} rows right", rows.len())),
        false => Err(wrong.join("; ")),
    }
}

/// A move that starts at `quad` and that an ordinary block can stop.
fn blockable_from(quad: Quad) -> Option<Move> {
    Move::ALL
        .iter()
        .copied()
        .find(|&mv| mv.data().start == quad && !duel::unblockable(mv))
}

/// The swing sides of the start quadrants with the two fighters facing each
/// other, with the attacker turned side-on, and with the attacker behind
/// the blocker, and of every move only a perfect block stops.
fn swing_side_table() -> Result<String, String> {
    use Quad::*;
    let (left, right, above) = SIDES;
    let facing = [
        (T, above),
        (TL, right),
        (L, right),
        (BL, right),
        (TR, left),
        (R, left),
        (BR, left),
        (B, None),
    ];
    let mut rows = Vec::new();
    let mut quad_row = |label: &str, quad: Quad, attacker: f32, blocker: f32, want| {
        let got = blockable_from(quad).map(|mv| aim::swing_side(mv, attacker, blocker));
        rows.push((format!("{label} {quad:?}"), got, Some(want)));
    };
    for (quad, want) in facing {
        quad_row("facing", quad, 0.0, 180.0, want);
        for turn in [90.0, -90.0] {
            let side_on = if quad == T { above } else { None };
            quad_row(&format!("turned {turn}"), quad, turn, 180.0, side_on);
        }
    }
    quad_row("behind", L, 0.0, 0.0, left);
    for &mv in Move::ALL.iter().filter(|&&mv| duel::unblockable(mv)) {
        rows.push((
            format!("unblockable {}", mv.name()),
            Some(aim::swing_side(mv, 0.0, 180.0)),
            Some(None),
        ));
    }
    table(rows)
}

/// The sides left, right and above, as the tables write them.
const SIDES: (Option<Side>, Option<Side>, Option<Side>) =
    (Some(Side::Left), Some(Side::Right), Some(Side::Above));

/// The crosshair sides of a line turned by (right, up) degrees off the way
/// to a chest 1.5 meters ahead. A positive right turn puts the crosshair to
/// the right of the chest on the screen. The edge of the dead zone, exactly
/// 3 degrees, is tested on the offsets themselves (`aim::side_of_offsets`),
/// and the lines turned to either side of it by 3.1 and 2.9 degrees.
fn crosshair_side_table() -> Result<String, String> {
    let (left, right, above) = SIDES;
    let edge = [
        (2.9, 0.0, None),
        (3.0, 0.0, right),
        (-3.0, 0.0, left),
        (0.0, 3.0, above),
        (0.0, 2.9, None),
    ];
    let mut rows: Vec<_> = edge
        .into_iter()
        .map(|(right_deg, up_deg, want)| {
            let got = aim::side_of_offsets(right_deg, up_deg);
            (format!("offsets ({right_deg}, {up_deg})"), got, want)
        })
        .collect();
    let lines = [
        (0.0, 0.0, None),
        (2.0, 2.0, None),
        (10.0, 0.0, right),
        (-10.0, 0.0, left),
        (0.0, 10.0, above),
        (6.0, 8.0, above),
        (8.0, 6.0, right),
        (-8.0, 6.0, left),
        (0.0, -10.0, None),
        (2.9, 0.0, None),
        (3.1, 0.0, right),
        (-3.1, 0.0, left),
    ];
    let chest = [1.5 / UNIT, 0.0, 0.0];
    rows.extend(lines.into_iter().map(|(right_deg, up_deg, want)| {
        let (way, ..) = math::angle_vectors([-up_deg, -right_deg, 0.0]);
        let got = aim::crosshair_side([0.0; 3], way, chest);
        (format!("line ({right_deg}, {up_deg})"), got, want)
    }));
    table(rows)
}

fn aimed_table() -> Result<String, String> {
    use Side::*;
    let mut cases: Vec<(Option<Side>, Option<Side>, bool)> =
        [None, Some(Left), Some(Right), Some(Above)]
            .into_iter()
            .map(|crosshair| (None, crosshair, true))
            .collect();
    cases.extend([
        (Some(Left), Some(Left), true),
        (Some(Left), Some(Right), false),
        (Some(Left), None, false),
        (Some(Above), Some(Above), true),
    ]);
    let rows = cases
        .into_iter()
        .map(|(swing, crosshair, want)| {
            (
                format!("{swing:?} {crosshair:?}"),
                aim::aimed(swing, crosshair),
                want,
            )
        })
        .collect();
    table(rows)
}

/// The side seen from a camera at `camera` looking along `view`: where the
/// crosshair, the middle of the screen, is against the chest's place on it.
fn screen_side(camera: Vec3, view: Vec3, chest: Vec3) -> Option<Side> {
    let (forward, right, up) = math::angle_vectors(view);
    let to = math::sub(chest, camera);
    let ahead = math::dot(to, forward);
    aim::side_of_offsets(
        -math::atan2_deg(math::dot(to, right), ahead),
        -math::atan2_deg(math::dot(to, up), ahead),
    )
}

/// For the third-person camera, the shoulder camera on each side and first
/// person, each looking some way off an attacker 1.5 meters ahead, the side
/// of the crosshair line (`aim::crosshair_line`) equals the side seen on
/// the camera's screen.
fn crosshair_line_matches_shots() -> Result<String, String> {
    let me = MotionState::standing([0.0, 0.0, 24.0], 0.0);
    let them = [1.5 / UNIT, 0.0, me.origin[2]];
    let chest = aim::chest(them, me.eye_height);
    let level = math::sub(chest, mind::eye(&me));
    let at_chest = -math::atan2_deg(level[2], level[0]);
    let looks = [
        (0.0, 0.0),
        (20.0, 0.0),
        (-20.0, 0.0),
        (0.0, 20.0),
        (0.0, -20.0),
    ];
    let mut rows = Vec::new();
    let mut seen = Vec::new();
    for (right_deg, up_deg) in looks {
        let looking = MotionState {
            view: [at_chest - up_deg, -right_deg, 0.0],
            ..me
        };
        let (forward, right, _) = math::angle_vectors(looking.view);
        let third = math::mad(mind::crosshair(&looking), -80.0, forward);
        let cameras = [
            ("third person", third),
            ("right shoulder", math::mad(third, 20.0, right)),
            ("left shoulder", math::mad(third, -20.0, right)),
            ("first person", mind::eye(&looking)),
        ];
        for (name, camera) in cameras {
            let aimed = MotionState {
                command_aim: mind::aim_for(&looking, camera),
                ..looking
            };
            let (from, way) = aim::crosshair_line(&aimed);
            let got = aim::crosshair_side(from, way, chest);
            seen.push(got);
            rows.push((
                format!("{name} ({right_deg}, {up_deg})"),
                got,
                screen_side(camera, looking.view, chest),
            ));
        }
    }
    let every = [Some(Side::Left), Some(Side::Right), Some(Side::Above), None];
    if let Some(missing) = every.iter().find(|side| !seen.contains(side)) {
        return Err(format!("no look gave the side {missing:?}"));
    }
    table(rows)
}

/// A snapshot of a new world in the placeholder level, under `rules`, with
/// the flag at `targeted`.
fn snapshot_with(rules: SaberRules, targeted: bool) -> Snapshot {
    let mut snapshot = Run::new(&Setup::pinned()).last;
    snapshot.saber_rules = rules;
    snapshot.targeted_blocks = targeted;
    snapshot
}

fn flag_round_trip() -> Result<String, String> {
    let held = snapshot_with(SaberRules::DuelAdvanced, true);
    let again = Snapshot::decode(&held.encode()).map_err(|e| e.to_string())?;
    check(
        again == held,
        format!("read back targeted_blocks = {}", again.targeted_blocks),
    )
}

fn old_snapshot_off() -> Result<String, String> {
    let before = crate::since::snapshot::FEAT_TARGETED_BLOCKS.1 - 1;
    let versions = Versions {
        snapshot: before,
        ..Versions::THIS_BUILD
    };
    let mut read = Reader::new("targeted", &[]).with_versions(versions);
    let older = decode_targeted(&mut read).map_err(|e| e.to_string())?;
    check(
        !older,
        format!("from snapshot version {before} targeted_blocks = {older}"),
    )
}

/// The world's answer for every set of rules and both values of the flag:
/// only the Advanced rules with the flag set turn targeted blocks on.
fn ja_ignores_flag() -> Result<String, String> {
    let cases = [
        (SaberRules::Ja, false, false),
        (SaberRules::Ja, true, false),
        (SaberRules::DuelAdvanced, false, false),
        (SaberRules::DuelAdvanced, true, true),
    ];
    let rows = cases
        .into_iter()
        .map(|(rules, flag, want)| {
            let world = SimWorld::from_snapshot(&snapshot_with(rules, flag), Level::arena())
                .map_err(|e| e.to_string())?;
            Ok((
                format!("{} with the flag {flag}", rules.name()),
                world.targeted(),
                want,
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    table(rows)
}
