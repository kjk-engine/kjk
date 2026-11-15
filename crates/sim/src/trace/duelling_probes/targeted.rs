//! The probes of targeted blocks (`kjk sim probes --only targeted`). The
//! session's flag survives a snapshot, an older snapshot reads it as off, and
//! the world turns targeted blocks on only under the Advanced rules with the
//! flag set. The sides (`duel::aim`) follow the spec's tables: the side of
//! a swing by its start quadrant and the two fighters' facings, the
//! direction of the last camera turn by the turns of the last 200 ms, and
//! the side of that direction by its angle. The turn is cleared when block
//! is released, and it survives a snapshot. In a fight, a player's perfect
//! block needs the last camera turn toward the swing's side, whatever the
//! crosshair points at, a swing with no side needs no aim, the flag off asks
//! for no aim, the window lasts `targeted::WINDOW_PERCENT` of its length with the flag
//! off, each of two attackers is judged
//! against its own side, a blow held for a lagged player's grace is judged
//! against the move it was struck with, and a creature's guard does not
//! change. The reticule's threat is the fighter in front and within range
//! whose swing began first, else the nearest.

use super::super::duels::play;
use crate::Seat;
use crate::combat::saber::duel::exchange::Guard;
use crate::combat::saber::duel::tap::{self, Tapped};
use crate::combat::saber::duel::{self, aim, aim::Side, aim::Turn};
use crate::combat::saber::{Move, Quad, SaberRules, grace};
use crate::determinism::{Run, Setup};
use crate::math::{self, Vec3};
use crate::module::ObjectKind;
use crate::probe::{Checks, check};
use crate::snapshot::{Kind, SimId, Snapshot, decode_targeted};
use crate::wire::{Reader, Versions};
use crate::{Level, SimWorld};
use collision::UNIT;

pub fn probes() -> Checks {
    let released = play(RELEASED);
    vec![
        ("flag_round_trip", flag_round_trip()),
        ("old_snapshot_off", old_snapshot_off()),
        ("ja_ignores_flag", ja_ignores_flag()),
        ("swing_side_table", swing_side_table()),
        ("turn_table", turn_table()),
        ("aim_side_table", aim_side_table()),
        ("aimed_table", aimed_table()),
        ("turn_cleared_on_release", turn_cleared(&released)),
        ("turn_round_trip", turn_round_trip(&released)),
        ("lead_clears_turn", lead_clears_turn()),
        ("aim_kept_through_parry", aim_kept_through_parry()),
        ("blade_with_the_button", blade_with_the_button()),
    ]
    .into_iter()
    .chain(AIMED.iter().map(|(name, case)| (*name, aimed_block(case))))
    .chain([
        ("last_turn_decides", last_turn_decides()),
        ("window_lengthened", window_lengthened()),
        ("two_attackers", two_attackers()),
        ("held_blow_keeps_its_move", held_blow_keeps_its_move()),
        ("creature_guard_unchanged", creature_guard_unchanged()),
        ("threat_order", threat_order()),
    ])
    .collect()
}

/// One blow that met a blocker, as the exchange tap told it, with the side
/// its swing came from and the side the blocker's last camera turn pointed
/// toward, both on the tick before the blow. The attacker's place and the
/// blocker's view on that tick are kept too.
#[derive(Debug, Clone, Copy)]
struct Blow {
    tick: u64,
    attacker: SimId,
    guard: Guard,
    perfect: bool,
    swing: Option<Side>,
    turned: Option<Side>,
    attacker_at: Option<Vec3>,
    view: Option<Vec3>,
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
            let of = |id: SimId| {
                let entity = before?.entities.iter().find(|e| e.id == id)?;
                Some((entity.fighter?, entity.player?))
            };
            let (them, me) = (of(attacker), of(body));
            let (swing, turned) = match (them, me) {
                (Some((fighter, ps)), Some((mine, me))) => {
                    aim::sides(&me, &mine.saber.turn, &ps, fighter.saber.current_move)
                }
                _ => (None, None),
            };
            Blow {
                tick,
                attacker,
                guard,
                perfect,
                swing,
                turned,
                attacker_at: them.map(|(_, ps)| ps.origin),
                view: me.map(|(_, ps)| ps.view),
            }
        })
        .collect();
    Ok((ticks, blows))
}

/// The keys of 30 rounds in which seat `blocker` holds block and presses
/// attack afresh every 16 ticks, more often than its perfect window lasts,
/// while the other seats press `others`. The blocker looks at seat `at`,
/// turned by `look`.
fn pressing(blocker: u8, at: u8, others: &str, look: &str) -> String {
    let held = format!("{others} h{blocker}=block s{blocker}={at} {look}");
    format!("tick 15 {held} b{blocker}=attack\ntick 1 {held}\n").repeat(30)
}

/// The scenario's word for targeted blocks `flag` (`targeted on|off`).
fn on_off(flag: bool) -> &'static str {
    if flag { "on" } else { "off" }
}

/// A duel under the Advanced rules with targeted blocks `flag`. Seat 0 is
/// a `kind` fighter at x=0. Seat 1 is a level 20 medium fighter at x=45
/// facing it, with the sheet keys `sheet` (`sheet 1 KEYS`). Seat 0 plays
/// `before` once. When `from` is set, seat 1 then holds block for 32 ticks
/// (256 ms, longer than a camera turn is kept, and two rounds of
/// `pressing`, so the blows meet seat 1 at the same point of its presses)
/// looking at seat 0 turned by `from`. Then seat 0 holds `swing`, while seat 1 blocks (`pressing`)
/// looking at seat 0 turned by `look`. Seat 1's camera turns on the first
/// tick of `pressing` from the look before it to `look`.
#[derive(Clone, Copy)]
struct Fight {
    flag: bool,
    kind: &'static str,
    sheet: &'static str,
    before: &'static str,
    from: Option<&'static str>,
    swing: &'static str,
    look: &'static str,
}

/// A fight with targeted blocks on, a medium seat 0 swinging from the
/// blocker's left, and the blocker looking straight at the attacker, never
/// turning while it holds block.
const FIGHT: Fight = Fight {
    flag: true,
    kind: "medium",
    sheet: "",
    before: "",
    from: None,
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
/// Seat 1's look turned 15 degrees to the left of seat 0, to the right, or
/// not at all.
const LEFT: &str = "t1=15";
const RIGHT: &str = "t1=-15";
const CENTRE: &str = "";

impl Fight {
    fn text(self) -> String {
        let flag = on_off(self.flag);
        let from = (self.from).map_or(String::new(), |from| {
            format!("tick 32 h1=block s1=0 {from}\n")
        });
        format!(
            "scenario probe\nrules advanced\nseed 7\ntargeted {flag}\n\
             fighter 0 {} x=0 defence=2 tiers=2 fp=100 hp=40000\n\
             fighter 1 medium x=45 yaw=180 defence=2 tiers=2 fp=100 hp=40000\n\
             sheet 1 level=20 {}\nbegin\ntick 1 b0=attack b1=attack\ntick 60\n{}{from}{}",
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
/// guard) must go: the swing's side and the side of the last camera turn it
/// met, its guard and whether it was a perfect block.
struct Aimed {
    fight: Fight,
    swing: Option<Side>,
    turned: Option<Side>,
    guard: Guard,
    perfect: bool,
}

const AIMED: [(&str, Aimed); 5] = [
    (
        "turn_toward_the_side_is_perfect",
        Aimed {
            fight: Fight {
                look: LEFT,
                ..FIGHT
            },
            swing: Some(Side::Left),
            turned: Some(Side::Left),
            guard: Guard::Perfect,
            perfect: true,
        },
    ),
    (
        "turn_to_the_other_side_is_aimed",
        Aimed {
            fight: Fight {
                look: RIGHT,
                ..FIGHT
            },
            swing: Some(Side::Left),
            turned: Some(Side::Right),
            guard: Guard::Active,
            perfect: false,
        },
    ),
    (
        "no_turn_is_aimed",
        Aimed {
            fight: FIGHT,
            swing: Some(Side::Left),
            turned: None,
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
            turned: Some(Side::Right),
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
            turned: None,
            guard: Guard::Perfect,
            perfect: true,
        },
    ),
];

/// The first aimed block (a perfect or an ordinary aimed guard) of `fight`.
fn first_aimed(fight: Fight) -> Result<Blow, String> {
    let (_, blows) = fight.blows()?;
    blows
        .iter()
        .find(|blow| matches!(blow.guard, Guard::Perfect | Guard::Active))
        .copied()
        .ok_or_else(|| format!("no aimed block among {} blows", blows.len()))
}

/// A blow as the probes write it: its tick, its two sides and its guard.
fn said(blow: &Blow) -> String {
    format!(
        "on tick {}: the swing from {:?}, the last turn toward {:?}, guard {:?}, perfect {}",
        blow.tick, blow.swing, blow.turned, blow.guard, blow.perfect
    )
}

fn aimed_block(case: &Aimed) -> Result<String, String> {
    let blow = first_aimed(case.fight)?;
    check(
        (blow.swing, blow.turned, blow.guard, blow.perfect)
            == (case.swing, case.turned, case.guard, case.perfect),
        format!(
            "the first aimed block {}; expected {:?}, {:?}, {:?}, {}",
            said(&blow),
            case.swing,
            case.turned,
            case.guard,
            case.perfect
        ),
    )
}

/// Only the last camera turn decides, not where the crosshair points. In
/// both fights seat 1 ends up looking straight at seat 0, which swings from
/// seat 1's left. In the first, seat 1 held block looking to the right of
/// seat 0 and then turned left onto it, and the block is perfect. In the
/// second, seat 1 held block looking to the left and then turned right onto
/// it, and the block is an ordinary aimed block. On the tick before each
/// block, seat 0 stands in the same place and seat 1 looks the same way.
fn last_turn_decides() -> Result<String, String> {
    let turned = |from| {
        first_aimed(Fight {
            from: Some(from),
            ..FIGHT
        })
    };
    // The fight that starts looking right ends with a turn to the left, and
    // the other way round.
    let (left, right) = (turned(RIGHT)?, turned(LEFT)?);
    let near = |a: Option<Vec3>, b: Option<Vec3>| {
        a.zip(b)
            .is_some_and(|(a, b)| math::length(math::sub(a, b)) < 0.01)
    };
    let same_place = near(left.attacker_at, right.attacker_at) && near(left.view, right.view);
    let (left_side, right_side) = (Some(Side::Left), Some(Side::Right));
    check(
        same_place
            && (left.swing, left.turned, left.guard) == (left_side, left_side, Guard::Perfect)
            && (right.swing, right.turned, right.guard) == (left_side, right_side, Guard::Active),
        format!(
            "turned left: {}; turned right: {}; seat 0 at {:?} and {:?}, seat 1 looking {:?} and {:?}",
            said(&left),
            said(&right),
            left.attacker_at,
            right.attacker_at,
            left.view,
            right.view
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

/// With targeted blocks on, the perfect window lasts
/// `targeted::WINDOW_PERCENT` of its length with them off, for Reflex saves
/// 0 and 6.
fn window_lengthened() -> Result<String, String> {
    let rows = [(0, "reflex=0"), (6, "reflex=6")]
        .into_iter()
        .map(|(reflex, sheet)| {
            let (on, off) = (first_window(true, sheet)?, first_window(false, sheet)?);
            Ok((
                format!("Reflex {reflex}: {on} ms on, {off} ms off"),
                on,
                off * rules::duel::targeted::WINDOW_PERCENT / 100,
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
/// comes from seat 0's left and seat 2's from its right. Seat 0's last
/// camera turn is to the left. The first blow of seat 1 meets a perfect
/// block, and the first of seat 2 does not.
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
                && (one.swing, one.turned, one.guard, one.perfect)
                    == (left, left, Guard::Perfect, true)
                && (two.swing, two.turned, two.guard, two.perfect)
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
/// grace. Seat 0 swings once from seat 1's left, and seat 1 looks at seat
/// 0. Seat 1 holds block and presses attack from the tick `block_from` on,
/// and on that tick it turns its camera to the left. When `block_from` is
/// None, seat 1 never blocks.
fn lagged(block_from: Option<u64>) -> String {
    let keys = |ticks: u64, keys: &str| format!("tick {ticks} s1=0 {keys}\n");
    let swing = keys(4, "b0=attack r0=-1");
    let after = match block_from {
        Some(from) => {
            let block = format!("{LEFT} h1=block b1=attack");
            keys(from - LAGGED_SWING - 4, "") + &keys(80, &block)
        }
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
/// move. Seat 1 then blocks within its grace, with its last camera turn
/// toward the side of the held move, and the block is perfect. A snapshot
/// taken during the hold reads back with the held move.
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
    let side = |mv: Move| aim::swing_side(mv, them.view[1], me.view[1]);
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

/// A fight staged for the reticule's probes (`staged`).
#[derive(Clone, Copy, Debug)]
pub enum Stage {
    /// Seat 0 swings from seat 1's left, and seat 1 turned its camera to
    /// the left as it raised its guard.
    TurnedLeft,
    /// Seat 0 swings from seat 1's left, and seat 1 has not turned its
    /// camera since it raised its guard.
    Unturned,
    /// Seat 0 plays a spin attack, which has no side, and seat 1 turned its
    /// camera to the left as it raised its guard.
    Spin,
    /// The fight `TurnedLeft` in the player restrict mode, in which seat 0
    /// cannot attack.
    Restricted,
}

/// The ticks of the fight `stage`, in which seat 1 holds block while seat 0
/// swings. The reticule's probes read the view from these snapshots.
pub fn staged(stage: Stage) -> Result<Vec<Snapshot>, String> {
    let fight = match stage {
        Stage::TurnedLeft => Fight {
            look: LEFT,
            ..FIGHT
        },
        Stage::Unturned => FIGHT,
        Stage::Spin => Fight { look: LEFT, ..SPIN },
        Stage::Restricted => Fight {
            look: LEFT,
            before: "restrict on\n",
            ..FIGHT
        },
    };
    play(&fight.text())
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

/// How long one step of the turn table lasts (ms): one step at the sim's
/// own rate.
const STEP_MS: u64 = 1000 / crate::rate::TICK_HZ as u64;

/// Camera turns, one after another, as `aim_after` takes them: how long
/// each lasts (ms), and how far it turns to the right and up (degrees).
type Turns = [(u64, f32, f32)];

/// The aim after a body's camera turns `turns`, taken one after another.
/// Each turn moves the camera `right` degrees to the right and `up` degrees
/// up, spread evenly over `ms` milliseconds of steps (at least one step).
/// A turn of no degrees holds the camera still. The camera starts at a yaw
/// of 175 degrees, so that a turn of 10 degrees to the left crosses the wrap
/// of the command's angles at 180 degrees.
fn aim_after(turns: &Turns) -> Option<f32> {
    let (mut turn, mut now) = (Turn::KEYED, 1000);
    let (mut right, mut up) = (0.0, 0.0);
    let short = |right: f32, up: f32| {
        [
            math::degrees_to_short(-up),
            math::degrees_to_short(175.0 - right),
            0,
        ]
    };
    for &(ms, by_right, by_up) in turns {
        let steps = (ms / STEP_MS).max(1);
        let (from_right, from_up) = (right, up);
        for step in 1..=steps {
            let done = step as f32 / steps as f32;
            let before = short(right, up);
            (right, up) = (from_right + by_right * done, from_up + by_up * done);
            now += STEP_MS;
            turn.step(now, before, short(right, up));
        }
    }
    turn.aim
}

/// The aim of a table of camera turns (`aim::Turn`). A turn up is 0
/// degrees, and the angle grows clockwise. A turn of 1.4 degrees, under the
/// least turn of 1.5, changes nothing, and a turn of 1.6 degrees sets the
/// aim. A drift of 1 degree or of 2.5 degrees spread over 400 ms changes
/// nothing, because no 200 ms of it turns more than 1.5 degrees. A turn of
/// 2 degrees over 120 ms or over 240 ms sets the aim, because 200 ms of
/// either turns more than 1.5 degrees. A still camera keeps the last
/// direction, and a later turn replaces it.
fn turn_table() -> Result<String, String> {
    let rows: [(&str, &Turns, Option<f32>); 14] = [
        ("right", &[(0, 10.0, 0.0)], Some(90.0)),
        ("left, across the wrap", &[(0, -10.0, 0.0)], Some(270.0)),
        ("up", &[(0, 0.0, 10.0)], Some(0.0)),
        ("down", &[(0, 0.0, -10.0)], Some(180.0)),
        ("down and to the left", &[(0, -10.0, -10.0)], Some(225.0)),
        (
            "up and to the right over 100 ms",
            &[(100, 6.0, 8.0)],
            Some(36.869_896),
        ),
        ("1.4 degrees to the right", &[(0, 1.4, 0.0)], None),
        ("1.6 degrees to the right", &[(0, 1.6, 0.0)], Some(90.0)),
        ("1 degree over 400 ms", &[(400, 1.0, 0.0)], None),
        ("2.5 degrees over 400 ms", &[(400, 2.5, 0.0)], None),
        ("2 degrees over 240 ms", &[(240, 2.0, 0.0)], Some(90.0)),
        ("2 degrees over 120 ms", &[(120, 2.0, 0.0)], Some(90.0)),
        (
            "right, still for a second",
            &[(0, 10.0, 0.0), (1000, 0.0, 0.0)],
            Some(90.0),
        ),
        (
            "right, still, then down",
            &[(0, 10.0, 0.0), (300, 0.0, 0.0), (0, 0.0, -10.0)],
            Some(180.0),
        ),
    ];
    // An angle within a twentieth of a degree of the expected one counts as
    // equal, and is shown as the expected one. The command's angles are
    // whole steps of 360 / 65 536 degrees, so a turn of 10 degrees is not
    // exactly 10 degrees.
    let close = |got: Option<f32>, want: Option<f32>| match (got, want) {
        (Some(got), Some(want)) if (got - want).abs() < 0.05 => Some(want),
        _ => got,
    };
    table(
        rows.into_iter()
            .map(|(name, turns, want)| (name.to_string(), close(aim_after(turns), want), want))
            .collect(),
    )
}

/// The side of a table of aim angles (`aim::aim_side`), the four edges
/// included. Right runs from 45 up to 135 degrees and left from 225 up to
/// 315. At 45 and 315 the turn points right and left, and at 135 and 225 it
/// points down, with no side.
fn aim_side_table() -> Result<String, String> {
    let (left, right, above) = SIDES;
    let rows = [
        (0.0, above),
        (30.0, above),
        (44.9, above),
        (45.0, right),
        (90.0, right),
        (134.9, right),
        (135.0, None),
        (180.0, None),
        (225.0, None),
        (225.1, left),
        (270.0, left),
        (315.0, left),
        (315.1, above),
        (359.9, above),
    ];
    table(
        rows.into_iter()
            .map(|(deg, want)| (format!("{deg} degrees"), aim::aim_side(deg), want))
            .collect(),
    )
}

fn aimed_table() -> Result<String, String> {
    use Side::*;
    let mut cases: Vec<(Option<Side>, Option<Side>, bool)> =
        [None, Some(Left), Some(Right), Some(Above)]
            .into_iter()
            .map(|turned| (None, turned, true))
            .collect();
    cases.extend([
        (Some(Left), Some(Left), true),
        (Some(Left), Some(Right), false),
        (Some(Left), None, false),
        (Some(Above), Some(Above), true),
    ]);
    let rows = cases
        .into_iter()
        .map(|(swing, turned, want)| {
            (
                format!("{swing:?} {turned:?}"),
                aim::aimed(swing, turned),
                want,
            )
        })
        .collect();
    table(rows)
}

/// A duel under the Advanced rules with targeted blocks on. Seat 1 looks at
/// seat 0, which does not swing, in four stretches of 10 ticks. In the
/// first it raises its guard. In the second it holds block and turns its
/// camera 10 degrees to the right. In the third it releases block, and in
/// the fourth it raises its guard again without turning.
const RELEASED: &str = "scenario probe\nrules advanced\nseed 7\ntargeted on\n\
     fighter 0 medium x=0 defence=2 tiers=2 fp=100 hp=40000\n\
     fighter 1 medium x=45 yaw=180 defence=2 tiers=2 fp=100 hp=40000\n\
     begin\ntick 1 b0=attack b1=attack\ntick 60\n\
     tick 10 h1=block s1=0\ntick 10 h1=block s1=0 t1=-10\n\
     tick 10 s1=0 t1=-10\ntick 10 h1=block s1=0 t1=-10\n";

/// The names of the four stretches of `RELEASED`, and seat 1's aim expected
/// at the end of each.
const STRETCHES: [(&str, Option<f32>); 4] = [
    ("block raised", None),
    ("turned right", Some(90.0)),
    ("block released", None),
    ("block raised again", None),
];

/// Seat 1's camera turn at the end of each stretch of `RELEASED`, with the
/// snapshot it was read from.
fn stretch_ends(ticks: &[Snapshot]) -> Result<Vec<(Snapshot, Turn)>, String> {
    let ends =
        (1..=STRETCHES.len()).map(|n| ticks.len().checked_sub(1 + 10 * (STRETCHES.len() - n)));
    ends.map(|at| {
        let snapshot = at.and_then(|at| ticks.get(at)).ok_or("too few ticks")?;
        let fighter = snapshot.own(Seat(1)).and_then(|e| e.fighter);
        let turn = fighter.ok_or("no seat 1")?.saber.turn;
        Ok((snapshot.clone(), turn))
    })
    .collect()
}

/// A body that raises its guard has no aim until the camera turns. A turn
/// to the right while block is held points the aim right. Releasing block
/// clears the aim, and it stays cleared when block is raised again without
/// a turn.
fn turn_cleared(ticks: &Result<Vec<Snapshot>, String>) -> Result<String, String> {
    let ends = stretch_ends(ticks.as_ref().map_err(Clone::clone)?)?;
    let rows = (STRETCHES.iter().zip(&ends))
        .map(|((name, want), (_, turn))| {
            let got = turn.aim.map(|aim| aim.round());
            (
                name.to_string(),
                (got, turn.side()),
                (*want, want.and_then(aim::aim_side)),
            )
        })
        .collect();
    table(rows)
}

/// Seat 0 holds block and turns its camera 10 degrees to the right. Then it
/// takes over a companion (`lend c0`) and goes on holding block without a
/// turn. Before the takeover, seat 0's own character aims right. On the
/// first step of the takeover, the own character's turn is `Turn::NONE` and
/// the companion has no aim. Ten steps later the companion holds block and
/// still has no aim.
fn lead_clears_turn() -> Result<String, String> {
    let text = "scenario probe\nrules advanced\nseed 7\ntargeted on\n\
         fighter 0 medium x=0 defence=2 tiers=2 fp=100 hp=40000\n\
         fighter 1 medium x=45 yaw=180 defence=2 tiers=2 fp=100 hp=40000\n\
         creature staff faction=friendly x=-60 yaw=0 defence=2 tiers=2 hp=40000\n\
         begin\ntick 1 b0=attack b1=attack\ntick 60\n\
         tick 10 h0=block s0=1\ntick 10 h0=block s0=1 t0=-10\nlend c0\n\
         tick 1 h0=block s0=1 t0=-10\ntick 10 h0=block s0=1 t0=-10\n";
    let ticks = play(text)?;
    let at = |from_end: usize| {
        let at = ticks.len().checked_sub(1 + from_end);
        at.and_then(|at| ticks.get(at)).ok_or("too few ticks")
    };
    let (before, first, later) = (at(11)?, at(10)?, at(0)?);
    let companion = Kind::Object(ObjectKind::Creature, 0);
    let saber = |snapshot: &Snapshot, kind: Kind| {
        let entity = snapshot.entities.iter().find(|e| e.kind == kind);
        entity.and_then(|e| e.fighter).map(|f| f.saber)
    };
    let own = Kind::Avatar(Seat(0));
    let led = first
        .entities
        .iter()
        .find(|e| e.kind == companion)
        .map(|e| e.id);
    let got = (
        saber(before, own).and_then(|s| s.turn.side()),
        first.leading.is_some() && first.leading == led,
        saber(first, own).map(|s| s.turn == Turn::NONE),
        saber(first, companion).map(|s| s.turn.aim),
        saber(later, companion).map(|s| (s.duel.block_held, s.turn.aim)),
    );
    let want = (
        Some(Side::Right),
        true,
        Some(true),
        Some(None),
        Some((true, None)),
    );
    check(
        got == want,
        format!(
            "own aim side before the takeover, the companion leads, own turn cleared on the first step, the companion's aim on the first step, the companion's block and aim ten steps later: {got:?}; expected {want:?}"
        ),
    )
}

/// The aim stays while the block button stays down, even while the body
/// cannot guard. Seat 1 turns its camera to the left as it raises its guard
/// and keeps the block button down for the whole fight. It makes a perfect
/// block, and its perfect parry leaves it unable to guard for some steps
/// (`Duel::block_held` false). On every step of the 600 ms after the
/// perfect block, seat 1 keeps the aim it had when it blocked.
fn aim_kept_through_parry() -> Result<String, String> {
    let ticks = play(
        &Fight {
            look: LEFT,
            ..FIGHT
        }
        .text(),
    )?;
    let fighter = |tick: &Snapshot| tick.own(Seat(1)).and_then(|e| e.fighter);
    let blocked = (ticks.iter())
        .find_map(|tick| Some(fighter(tick)?.saber.duel.perfect_at).filter(|&at| at != 0))
        .ok_or("seat 1 makes no perfect block")?;
    let after: Vec<(u64, bool, Option<f32>, Move)> = (ticks.iter())
        .filter(|tick| (blocked..blocked + 600).contains(&tick.time_ms))
        .filter_map(|tick| {
            let saber = fighter(tick)?.saber;
            Some((
                tick.time_ms,
                saber.duel.block_held,
                saber.turn.aim,
                saber.current_move,
            ))
        })
        .collect();
    let unguarded = after.iter().filter(|(_, held, ..)| !held).count();
    let mut moves: Vec<&str> = (after.iter())
        .filter(|(_, held, ..)| !held)
        .map(|(.., mv)| mv.name())
        .collect();
    moves.dedup();
    let lost: Vec<u64> = (after.iter())
        .filter(|(_, _, aim, _)| !aim.is_some_and(|aim| aim::aim_side(aim) == Some(Side::Left)))
        .map(|(at, ..)| *at)
        .collect();
    check(
        unguarded > 0 && lost.is_empty() && !after.is_empty(),
        format!(
            "the perfect block at {blocked} ms; of the {} steps in the 600 ms after it, seat 1 could not guard on {unguarded}, playing {moves:?}; the steps without the left aim: {lost:?}",
            after.len()
        ),
    )
}

/// The blade and the block button of seat 0, which holds block with the
/// button down and turns its camera to the right, and then plays on with
/// the button down after `change`. Each row gives, on the last step,
/// whether the blade is lit, whether it is out of the hand, whether seat 0
/// can guard, and whether the turn keeps the button down with its aim.
///
/// In the player restrict mode (`restrict on`), the held block keeps the
/// lightsaber lit and the guard up, as it did before targeted blocks. After
/// a disarm (`away 0 lying 0`), the blade is out of the hand and seat 0
/// cannot guard, while the turn keeps the button and the aim.
fn blade_with_the_button() -> Result<String, String> {
    let last = |change: &str| {
        let text = format!(
            "scenario probe\nrules advanced\nseed 7\ntargeted on\n\
             fighter 0 medium x=0 defence=2 tiers=2 fp=100 hp=40000\n\
             fighter 1 medium x=45 yaw=180 defence=2 tiers=2 fp=100 hp=40000\n\
             begin\ntick 1 b0=attack b1=attack\ntick 60\n\
             tick 10 h0=block s0=1\ntick 10 h0=block s0=1 t0=-10\n{change}\n\
             tick 40 h0=block s0=1 t0=-10\n"
        );
        let ticks = play(&text)?;
        let fighter = (ticks.last())
            .and_then(|tick| tick.own(Seat(0)))
            .and_then(|e| e.fighter)
            .ok_or("no seat 0")?;
        let saber = fighter.saber;
        Ok::<_, String>((
            saber.on,
            saber.thrown.is_some(),
            saber.duel.block_held,
            saber.turn.held,
            saber.turn.side(),
        ))
    };
    let right = Some(Side::Right);
    let rows = vec![
        (
            "restrict mode".to_string(),
            last("restrict on")?,
            (true, false, true, true, right),
        ),
        (
            "disarmed".to_string(),
            last("away 0 lying 0")?,
            (true, true, false, true, right),
        ),
    ];
    table(rows)
}

/// A snapshot taken while seat 1 holds block after a turn to the right reads
/// back equal through its bytes, its camera turn included.
fn turn_round_trip(ticks: &Result<Vec<Snapshot>, String>) -> Result<String, String> {
    let ends = stretch_ends(ticks.as_ref().map_err(Clone::clone)?)?;
    let (snapshot, turn) = &ends[1];
    let again = Snapshot::decode(&snapshot.encode()).map_err(|e| e.to_string())?;
    let read = again
        .own(Seat(1))
        .and_then(|e| e.fighter)
        .map(|f| f.saber.turn);
    check(
        again == *snapshot && read == Some(*turn) && turn.aim.is_some(),
        format!(
            "the aim {:?} read back as {:?}",
            turn.aim,
            read.map(|turn| turn.aim)
        ),
    )
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
