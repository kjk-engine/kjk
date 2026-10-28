//! The probes of a pistol in each hand (`kjk sim probes --only "dual
//! pistols"`), KOTOR's two pistols (as reone has them), on the proving
//! ground, no install:
//! - the hands take turns: right, left, right, half a refire apart, each
//!   bolt from its own hand's muzzle (`hand_muzzle`);
//! - each hand rolls with its own two-weapon penalty
//!   (`passive::hand_attack`);
//! - the hand keys take neither gun down (both are out);
//! - a trigger let go banks no shot;
//! - a pair broken mid-fight fires its one gun from the right, at its
//!   refire;
//! - the same every run;
//! - the gun feats: Rapid Shot's extra attacks are the main hand's (main,
//!   off, main; Multi Shot main, off, main, main), Power Blast fires both
//!   guns at once for one cost, Sniper Shot the main gun alone;
//! - a hostile creature with a pistol in each hand fires both, oftener
//!   than with one (`ai.gunners.pair_spacing`);
//! - a pair's trigger state, mid-burst, comes back whole from a snapshot.

use super::{Fighting, Id, PlayerSheet, SimWorld};
use crate::combat::sheet::feats;
use crate::combat::{Fighter, Sheet, passive};
use crate::input::{Command, Seat, TickInput, buttons};
use crate::math;
use crate::probe::{Checks, check};
use crate::snapshot::Snapshot;
use crate::{StepReason, WorldConfig};

const MSEC: u32 = 8;

/// A bolt as it left: when (sim ms), its side of the shooter (+ right),
/// its weapon's to hit and the attack's time its damage counts (ms).
#[derive(Debug, Clone, Copy)]
struct Fired {
    ms: u64,
    side: f32,
    attack: i8,
    cadence: u32,
}

impl Fired {
    fn left(&self) -> bool {
        self.side < 0.0
    }
}

/// The dev soldier with its blaster pistol in each hand.
fn pair() -> Sheet {
    let mut sheet = Sheet::dev_soldier();
    sheet.weapons[1] = sheet.weapons[0];
    sheet
}

/// `pair()` knowing the feat rows `known`.
fn trained(known: &[u16]) -> Sheet {
    let mut sheet = pair();
    sheet.feats = known.iter().fold(sheet.feats, |rows, row| rows.with(*row));
    sheet
}

struct Range {
    world: SimWorld,
    tick: u64,
    fired: Vec<Fired>,
    last: Option<Snapshot>,
}

impl Range {
    fn new(sheet: Sheet) -> Result<Range, String> {
        let level = crate::determinism::proving_ground();
        let config = WorldConfig {
            sheet,
            ..WorldConfig::one_seat()
        };
        let world = SimWorld::new(config, level).map_err(|error| error.to_string())?;
        Ok(Range {
            world,
            tick: 0,
            fired: Vec::new(),
            last: None,
        })
    }

    /// `ms` of steps with `command`, the bolts the player fires kept.
    fn run(&mut self, ms: u32, command: Command) {
        for _ in 0..ms / MSEC {
            self.tick += 1;
            let mut input = TickInput::default();
            input.insert(Seat(0), command);
            let snapshot = crate::step(
                &mut self.world,
                self.tick,
                &input,
                MSEC,
                StepReason::Authority,
            );
            let Some(me) = snapshot.own(Seat(0)) else {
                continue;
            };
            let Some(ps) = me.player else { continue };
            let (_, right, _) = math::angle_vectors(ps.view);
            let eye = crate::combat::mind::eye(&ps);
            for missile in snapshot.entities.iter().filter_map(|e| e.missile) {
                if missile.owner == me.id && missile.from_ms == snapshot.time_ms {
                    self.fired.push(Fired {
                        ms: snapshot.time_ms,
                        side: math::dot(math::sub(missile.base, eye), right),
                        attack: missile.weapon.attack_bonus,
                        cadence: missile.cadence_ms,
                    });
                }
            }
            self.last = Some(snapshot);
        }
    }

    /// The player's second bar now (hundredths).
    fn pool(&self) -> i32 {
        self.last
            .as_ref()
            .and_then(|snapshot| snapshot.own(Seat(0)))
            .and_then(|me| me.fighter)
            .map_or(i32::MIN, |fighter| fighter.fp)
    }

    /// The sides of the bolts fired since the `from`th, left true.
    fn sides(&self, from: usize) -> Vec<bool> {
        self.fired[from..].iter().map(Fired::left).collect()
    }

    fn fighter(&mut self, change: impl Fn(&mut Fighter, &mut Sheet)) {
        let mut query = self
            .world
            .world
            .query::<(&Id, &mut Fighting, &mut PlayerSheet)>();
        for (_, mut fighting, mut sheet) in query.iter_mut(&mut self.world.world) {
            change(&mut fighting.0, &mut sheet.0);
        }
    }
}

fn firing() -> Command {
    Command {
        angles: crate::moves::view_angles(2.0, 0.0),
        buttons: buttons::ATTACK,
        ..Command::default()
    }
}

fn alt() -> Command {
    Command {
        buttons: buttons::ALT_ATTACK,
        ..firing()
    }
}

fn idle() -> Command {
    Command {
        buttons: 0,
        ..firing()
    }
}

fn refire() -> u64 {
    u64::from(rules::weapons::pistol::REFIRE_MS)
}

/// The gaps between consecutive bolts (ms).
fn gaps(fired: &[Fired]) -> Vec<u64> {
    fired.windows(2).map(|w| w[1].ms - w[0].ms).collect()
}

pub fn probes() -> Checks {
    type Case = fn() -> Result<String, String>;
    let cases: [(&'static str, Case); 11] = [
        ("pair_hands_alternate", alternate),
        ("pair_hands_to_hit", to_hit),
        ("pair_keys_do_nothing", keys),
        ("pair_release_banks_nothing", release),
        ("pair_broken_fires_right", broken),
        ("pair_same_every_run", same),
        ("pair_rapid_shot_order", rapid),
        ("pair_power_blast_both", power),
        ("pair_sniper_main_only", sniper),
        ("pair_creature_fires_both", creature),
        ("pair_state_round_trip", round_trip),
    ];
    cases.into_iter().map(|(name, run)| (name, run())).collect()
}

/// Held 3 s: right, left, right…, half a refire apart (a tick either way).
fn alternate() -> Result<String, String> {
    let mut range = Range::new(pair())?;
    range.run(3000, firing());
    let fired = &range.fired;
    let turns = fired
        .iter()
        .enumerate()
        .all(|(at, shot)| shot.left() == (at % 2 == 1));
    let half = refire() / 2;
    let even = gaps(fired)
        .iter()
        .all(|gap| gap.abs_diff(half) <= u64::from(MSEC));
    let sides: Vec<bool> = fired.iter().map(Fired::left).collect();
    check(
        fired.len() >= 6 && turns && even,
        format!(
            "{} bolts, left: {sides:?}, gaps {:?}",
            fired.len(),
            gaps(fired)
        ),
    )
}

/// Each hand's bolts carry its own two-weapon penalty: the main hand's
/// to hit is the off hand's plus the difference `hand_attack` gives.
fn to_hit() -> Result<String, String> {
    let mut range = Range::new(pair())?;
    range.run(2000, firing());
    let of = |left: bool| {
        range
            .fired
            .iter()
            .filter(|shot| shot.left() == left)
            .map(|shot| shot.attack)
            .collect::<Vec<_>>()
    };
    let (right, left) = (of(false), of(true));
    let sheet = pair();
    let wanted = passive::hand_attack(&sheet, 0) - passive::hand_attack(&sheet, 1);
    let same = |hits: &[i8]| hits.windows(2).all(|w| w[0] == w[1]);
    let apart = match (right.first(), left.first()) {
        (Some(r), Some(l)) => i32::from(*r) - i32::from(*l),
        _ => i32::MIN,
    };
    check(
        same(&right) && same(&left) && apart == wanted && wanted > 0,
        format!("right {right:?}, left {left:?}: apart {apart}, wanted {wanted}"),
    )
}

/// The left hand's key with a pair out: the hand stays the right, no
/// weapon goes down, and the guns go on taking turns.
fn keys() -> Result<String, String> {
    let mut range = Range::new(pair())?;
    range.run(500, firing());
    range.run(
        1500,
        Command {
            weapon: 1,
            ..firing()
        },
    );
    let fighter = range
        .last
        .as_ref()
        .and_then(|snapshot| snapshot.own(Seat(0)))
        .and_then(|me| me.fighter)
        .ok_or("no fighter")?;
    let half = refire() / 2;
    let even = gaps(&range.fired)
        .iter()
        .all(|gap| gap.abs_diff(half) <= u64::from(MSEC));
    check(
        fighter.hand == 0 && even && range.fired.len() >= 4,
        format!(
            "hand {}, {} bolts, gaps {:?}",
            fighter.hand,
            range.fired.len(),
            gaps(&range.fired)
        ),
    )
}

/// Held 2 s, let go 3 s, pressed again: the first two bolts after are half
/// a refire apart, not one on the other.
fn release() -> Result<String, String> {
    let mut range = Range::new(pair())?;
    range.run(2000, firing());
    range.run(3000, idle());
    let before = range.fired.len();
    let pressed = range.fired.last().map_or(0, |shot| shot.ms) + 3000;
    range.run(1000, firing());
    let after = &range.fired[before..];
    let first = after.first().map(|shot| shot.ms.saturating_sub(pressed));
    let gap = gaps(after).first().copied();
    check(
        first.is_some_and(|ms| ms <= 3000)
            && gap.is_some_and(|gap| gap + u64::from(MSEC) >= refire() / 2),
        format!("first bolt {first:?} ms after the let-go began, then {gap:?} ms"),
    )
}

/// The left pistol taken from the hand mid-fight: every bolt after leaves
/// the right, a whole refire apart.
fn broken() -> Result<String, String> {
    let mut range = Range::new(pair())?;
    range.run(1500, firing());
    range.fighter(|_, sheet| sheet.weapons[1] = None);
    let before = range.fired.len();
    range.run(3000, firing());
    let after = &range.fired[before..];
    let right = after.iter().all(|shot| !shot.left());
    let whole = gaps(after)
        .iter()
        .all(|gap| gap.abs_diff(refire()) <= u64::from(MSEC));
    check(
        after.len() >= 3 && right && whole,
        format!(
            "{} bolts after, left: {:?}, gaps {:?}",
            after.len(),
            after.iter().map(Fired::left).collect::<Vec<_>>(),
            gaps(after)
        ),
    )
}

/// Two runs of the same fire end on the same snapshot.
fn same() -> Result<String, String> {
    let run = || -> Result<_, String> {
        let mut range = Range::new(pair())?;
        range.run(3000, firing());
        Ok(range.last.map(|snapshot| snapshot.hash()))
    };
    let (one, two) = (run()?, run()?);
    check(one.is_some() && one == two, format!("{one:?} and {two:?}"))
}

/// One press of the alternate with Rapid Shot: main, off, main; with Multi
/// Shot (its third tier) main, off, main, main.
fn rapid() -> Result<String, String> {
    let burst = |known: &[u16]| -> Result<Vec<bool>, String> {
        let mut range = Range::new(trained(known))?;
        range.run(200, idle());
        range.run(1000, alt());
        Ok(range.sides(0))
    };
    let rapid = burst(&feats::RAPID_SHOT[..1])?;
    let multi = burst(&feats::RAPID_SHOT)?;
    check(
        rapid == [false, true, false] && multi == [false, true, false, false],
        format!("Rapid Shot, left: {rapid:?}; Multi Shot: {multi:?}"),
    )
}

/// A Power Blast charged and let go: a bolt from each gun on the same
/// step, each counting the charge, the bar down by what one gun's blast
/// takes (one special's cost, the bar refilling meanwhile alike).
fn power() -> Result<String, String> {
    let blast = |sheet: Sheet| -> Result<(Vec<Fired>, i32), String> {
        let mut range = Range::new(sheet)?;
        range.run(200, idle());
        let before = range.pool();
        range.run(600, alt());
        range.run(200, idle());
        Ok((range.fired.clone(), before - range.pool()))
    };
    let (fired, spent) = blast(trained(&feats::POWER_BLAST[..1]))?;
    let mut one = trained(&feats::POWER_BLAST[..1]);
    one.weapons[1] = None;
    let (_, alone) = blast(one)?;
    let together = matches!(&fired[..], [a, b] if a.ms == b.ms && a.left() != b.left());
    let charged = fired.iter().all(|shot| u64::from(shot.cadence) > refire());
    check(
        together && charged && spent == alone && spent > 0,
        format!(
            "left: {:?}, cadences {:?}; spent {spent}, one gun's {alone}",
            fired.iter().map(Fired::left).collect::<Vec<_>>(),
            fired.iter().map(|shot| shot.cadence).collect::<Vec<_>>()
        ),
    )
}

/// Aiming a Sniper Shot and firing: every bolt leaves the main gun.
fn sniper() -> Result<String, String> {
    let mut range = Range::new(trained(&feats::SNIPER_SHOT[..1]))?;
    range.run(200, idle());
    range.run(
        2000,
        Command {
            buttons: buttons::ATTACK | buttons::ALT_ATTACK,
            ..firing()
        },
    );
    let sides = range.sides(0);
    check(
        sides.len() >= 2 && sides.iter().all(|left| !left),
        format!("left: {sides:?}"),
    )
}

/// The bolts (sim ms, left) a hostile with `sheet` standing 5 m from the
/// player fires at it over 6 s.
fn creature_fire(sheet: Sheet) -> Result<Vec<(u64, bool)>, String> {
    use crate::combat::sheet::factions::{HOSTILE_1, PLAYER};
    use crate::module::{ObjectData, ObjectKind, Placement, Repute};
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    data.placements.push(Placement::Own(ObjectData {
        kind: ObjectKind::Creature,
        tag: "pair_gunner".into(),
        sheet: Some(Sheet {
            faction: HOSTILE_1,
            max_hp: 500,
            ..sheet
        }),
        ..ObjectData::unknown(
            "pair_gunner",
            crate::determinism::ground_point(-4.0, 3.0, 0.0),
            0.0,
        )
    }));
    data.repute = Repute::at_war([PLAYER, HOSTILE_1]);
    let config = WorldConfig {
        sheet: Sheet {
            max_hp: 5000,
            ..Sheet::dev_soldier()
        },
        ..WorldConfig::one_seat()
    };
    let mut world = SimWorld::new(config, ground.with_module(data)).map_err(|e| e.to_string())?;
    let mut fired = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for tick in 1..=6000 / u64::from(MSEC) {
        let mut input = TickInput::default();
        input.insert(Seat(0), Command::default());
        let snapshot = crate::step(&mut world, tick, &input, MSEC, StepReason::Authority);
        let me = snapshot.own(Seat(0)).map(|me| me.id);
        for entity in &snapshot.entities {
            let Some(missile) = entity.missile else {
                continue;
            };
            let Some(shooter) = snapshot.get(missile.owner).and_then(|e| e.player) else {
                continue;
            };
            if Some(missile.owner) == me || !seen.insert(entity.id) {
                continue;
            }
            let (_, right, _) = math::angle_vectors(shooter.view);
            let off = math::sub(missile.base, crate::combat::mind::eye(&shooter));
            fired.push((missile.from_ms, math::dot(off, right) < 0.0));
        }
    }
    Ok(fired)
}

/// A hostile with a pistol in each hand fires from both, its shots closer
/// together than the same gunner's with one pistol.
fn creature() -> Result<String, String> {
    let both = creature_fire(pair())?;
    let one = creature_fire(Sheet {
        weapons: [Sheet::dev_soldier().weapons[0], None],
        ..Sheet::dev_soldier()
    })?;
    let mean = |fired: &[(u64, bool)]| {
        let gaps: Vec<u64> = fired.windows(2).map(|w| w[1].0 - w[0].0).collect();
        gaps.iter().sum::<u64>() as f32 / gaps.len().max(1) as f32
    };
    let hands = |left: bool| both.iter().filter(|(_, l)| *l == left).count();
    check(
        hands(false) >= 2 && hands(true) >= 2 && mean(&both) < mean(&one) * 0.75,
        format!(
            "pair: {} right, {} left, every {:.0} ms; one pistol: {} bolts, every {:.0} ms",
            hands(false),
            hands(true),
            mean(&both),
            one.len(),
            mean(&one)
        ),
    )
}

/// `Shooting` with every pair field set (the left's timer, its turn, the
/// hands of the last shot, a Rapid Shot's left shot still to fire), written
/// and read back: the same.
fn round_trip() -> Result<String, String> {
    use crate::combat::specials::Shooting;
    use crate::wire::{Reader, Writer};
    let sent = Shooting {
        burst: 1,
        off_ms: 75,
        next_off: true,
        hands: 1,
        off_burst: true,
        ..Shooting::default()
    };
    let mut out = Writer::default();
    sent.encode(&mut out);
    let got =
        Shooting::decode(&mut Reader::new("shooting", &out.0)).map_err(|e| format!("{e:?}"))?;
    check(got == sent, format!("sent {sent:?}, read {got:?}"))
}
