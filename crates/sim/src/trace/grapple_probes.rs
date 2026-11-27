//! The grab with bare hands and its grapple finishers (`world::grapples`),
//! played as duels (`duels::play`) under the Advanced rules with seeded
//! rolls. Seat 0 grabs; seat 1 (40 units ahead, facing it) is grabbed. The
//! duels' animations are made up (`duels::duel_anims`), so the clips'
//! lengths are the table's, not JA's; the checks read them from it.

use super::duels::{Missing, duel_anims, play};
use crate::Seat;
use crate::combat::Fighter;
use crate::combat::damage::{self, Blow, FriendlyFire, Relation, Source};
use crate::combat::fighter::Location;
use crate::combat::passive::Game;
use crate::combat::sheet::{Delivery, Sheet, Weapon};
use crate::math;
use crate::motion::{Anim, MotionState};
use crate::probe::{Checks, check};
use crate::rng::SimRng;
use crate::snapshot::{Kind, Snapshot};
use crate::world::grapples::gap_units;
use rules::duel::grab;

/// The victim's maximum hit points, and a wound that leaves it at 30%.
const MAX_HP: i32 = 400;
const HURT: i32 = MAX_HP * 30;

/// Ticks that take at least `ms`.
fn steps(ms: i32) -> i32 {
    (ms * crate::rate::TICK_HZ as i32 + 999) / 1000
}

/// What seat `n` was at each tick.
fn states(ticks: &[Snapshot], n: u8) -> Vec<(MotionState, Fighter)> {
    ticks
        .iter()
        .filter_map(|tick| {
            let entity = tick.own(Seat(n))?;
            Some((entity.player?, entity.fighter?))
        })
        .collect()
}

/// Whether seat `n`'s legs ever played `anim`.
fn played(ticks: &[Snapshot], n: u8, anim: Anim) -> bool {
    states(ticks, n).iter().any(|(ps, _)| ps.legs.anim == anim)
}

/// The fighters' lines: seat 0 with bare hands and a strong attack (an
/// attack bonus far past any Defense here, so only a natural 1 misses), a
/// victim of `MAX_HP` 40 units ahead, and `more` after them.
fn fighters(grabber: &str, more: &str) -> String {
    format!(
        "fighter 0 {grabber} x=0 defence=2 tiers=2 fp=100 hp={MAX_HP}\n\
         fighter 1 fists x=40 yaw=180 defence=2 tiers=2 fp=100 hp={MAX_HP}\n{more}\
         sheet 0 bab=40\n"
    )
}

/// Seat 0 presses alt attack once, holding `keys` (tick keys for seat 0,
/// such as `f0=-1`) through the grab, with the victim at `hurt` hundredths
/// of an HP, under `rules`; `before` and `after` are more lines; then
/// `then` ms of nothing. The ticks.
fn grab_with(
    rules: &str,
    (grabber, more): (&str, &str),
    hurt: i32,
    keys: &str,
    (before, after): (&str, &str),
    then: i32,
) -> Result<Vec<Snapshot>, String> {
    let table = duel_anims(&Missing::None)?;
    let reach = table.get(Anim::BOTH_KYLE_GRAB).length_ms() + grab::REACH_LEFT_MS[0];
    play(&format!(
        "scenario probe\nrules {rules}\nseed 7\n{before}{}begin\nwound 1 {hurt}\n\
         duel 0 points={}\ntick 20\ntick 1 b0=alt {keys}\ntick {} {keys}\n{after}tick {}\n",
        fighters(grabber, more),
        rules::duel::meters::POINTS_MAX,
        steps(reach),
        steps(then),
    ))
}

/// `grab_with` under the Advanced rules, nothing more, then 12 s.
fn grab(keys: &str) -> Result<Vec<Snapshot>, String> {
    grab_with("advanced", ("fists", ""), HURT, keys, ("", ""), 12_000)
}

/// Seat 0 presses alt attack: its body plays the grab, and its Stamina
/// falls by `grab::USES`.
fn alt_attack_grabs() -> Result<String, String> {
    let ticks = grab("")?;
    let seen = states(&ticks, 0);
    let grabbed = played(&ticks, 0, Anim::BOTH_KYLE_GRAB);
    let points: Vec<i32> = seen
        .iter()
        .map(|(_, f)| i32::from(f.saber.duel.points))
        .collect();
    let lost = points.windows(2).map(|w| w[0] - w[1]).max().unwrap_or(0);
    check(
        grabbed && lost == grab::USES,
        format!(
            "the grab plays: {grabbed}; the most Stamina lost in a step {lost}, the rules ask {}",
            grab::USES
        ),
    )
}

/// With no one in reach the grab misses: the stumble plays, and an attack
/// held through it throws no punch while the stumble's timer runs.
fn grab_miss_stumbles() -> Result<String, String> {
    let table = duel_anims(&Missing::None)?;
    let reach = table.get(Anim::BOTH_KYLE_GRAB).length_ms() + grab::REACH_LEFT_MS[0];
    let miss = table.get(Anim::BOTH_KYLE_MISS).length_ms();
    let ticks = play(&format!(
        "scenario probe\nrules advanced\nseed 7\n{}begin\nduel 0 points={}\n\
         place 1 400 0 {} 180\ntick 20\ntick 1 b0=alt\ntick {} b0=attack\n",
        fighters("fists", ""),
        rules::duel::meters::POINTS_MAX,
        0.125 - crate::motion::SOLE_Z,
        steps(reach + miss),
    ))?;
    let seen = states(&ticks, 0);
    let stumbled = seen
        .iter()
        .any(|(ps, _)| ps.legs.anim == Anim::BOTH_KYLE_MISS);
    let punched = seen.iter().any(|(ps, _)| {
        ps.legs.anim == Anim::BOTH_KYLE_MISS
            && ps.legs.timer > 0
            && matches!(ps.torso.anim, Anim::BOTH_MELEE1 | Anim::BOTH_MELEE2)
    });
    let held = states(&ticks, 1).iter().any(|(_, f)| f.grapple.is_some());
    check(
        stumbled && !punched && !held,
        format!(
            "no one in reach: the stumble plays {stumbled}; a punch during it {punched}; anyone \
             held {held}"
        ),
    )
}

/// The finisher each key held through the grab picks.
const KEYS: [(&str, u8); 5] = [
    ("", 1),
    ("f0=1", 1),
    ("r0=1", 2),
    ("r0=-1", 2),
    ("f0=-1", 3),
];

/// The clips a finisher plays: the grabber's and the victim's.
fn clips(finisher: u8) -> (Anim, Anim) {
    match finisher {
        1 => (Anim::BOTH_KYLE_PA_1, Anim::BOTH_PLAYER_PA_1),
        2 => (Anim::BOTH_KYLE_PA_2, Anim::BOTH_PLAYER_PA_2),
        _ => (Anim::BOTH_KYLE_PA_3, Anim::BOTH_PLAYER_PA_3),
    }
}

/// No key or forward picks the first finisher, left or right the second,
/// back the third, each on both bodies. The keys are held from the grab's
/// start until it reaches.
fn finisher_by_key() -> Result<String, String> {
    let mut wrong = Vec::new();
    for (keys, finisher) in KEYS {
        let ticks = grab(keys)?;
        let (grabber, victim) = clips(finisher);
        if !played(&ticks, 0, grabber) || !played(&ticks, 1, victim) {
            wrong.push(format!("'{keys}' did not play finisher {finisher}"));
        }
    }
    check(
        wrong.is_empty(),
        format!("keys to finishers {KEYS:?}: {wrong:?}"),
    )
}

/// Keys held only as the grab begins (on alt attack's press), then keys held
/// as it reaches, and the finisher they pick.
const START_KEYS: [(&str, &str, u8); 4] = [
    ("r0=1", "", 2),
    ("f0=-1", "", 3),
    ("r0=1", "f0=-1", 3),
    ("f0=-1", "f0=1", 1),
];

/// A key held only as the grab begins picks the finisher when no key is
/// held as it reaches. A key held as it reaches wins over it.
fn finisher_by_start_key() -> Result<String, String> {
    let table = duel_anims(&Missing::None)?;
    let reach = table.get(Anim::BOTH_KYLE_GRAB).length_ms() + grab::REACH_LEFT_MS[0];
    let mut wrong = Vec::new();
    for (start, reaching, finisher) in START_KEYS {
        let ticks = play(&format!(
            "scenario probe\nrules advanced\nseed 7\n{}begin\nwound 1 {HURT}\n\
             duel 0 points={}\ntick 20\ntick 1 b0=alt {start}\ntick {} {reaching}\ntick {}\n",
            fighters("fists", ""),
            rules::duel::meters::POINTS_MAX,
            steps(reach),
            steps(12_000),
        ))?;
        let (grabber, victim) = clips(finisher);
        if !played(&ticks, 0, grabber) || !played(&ticks, 1, victim) {
            wrong.push(format!(
                "'{start}' at the start, '{reaching}' as it reaches did not play finisher {finisher}"
            ));
        }
    }
    check(
        wrong.is_empty(),
        format!("start keys, reach keys, finisher {START_KEYS:?}: {wrong:?}"),
    )
}

/// The first tick on which both are in the grab: where each stands and
/// looks.
fn placed(ticks: &[Snapshot]) -> Option<(MotionState, MotionState)> {
    let (a, b) = (states(ticks, 0), states(ticks, 1));
    a.iter()
        .zip(&b)
        .find(|((_, fa), (_, fb))| fa.grapple.is_some() && fb.grapple.is_some())
        .map(|((pa, _), (pb, _))| (*pa, *pb))
}

/// How far apart two stand (units, flat), and how far (degrees) each looks
/// from straight at the other.
fn stance(a: &MotionState, b: &MotionState) -> (f32, f32, f32) {
    let mut way = math::sub(b.origin, a.origin);
    way[2] = 0.0;
    let apart = math::normalize(&mut way);
    let yaw = way[1].atan2(way[0]).to_degrees();
    let off =
        |looks: f32, want: f32| crate::combat::saber::blades::normalize_180(looks - want).abs();
    (apart, off(a.view[1], yaw), off(b.view[1], yaw + 180.0))
}

/// After a grab holds, the victim stands the first finisher's gap
/// (`grab::GAP_M`) in front of the grabber, and the two face each other.
fn victim_placed() -> Result<String, String> {
    let ticks = grab("")?;
    let (a, b) = placed(&ticks).ok_or("no grab held")?;
    let (apart, off_a, off_b) = stance(&a, &b);
    let want = gap_units(1);
    check(
        (apart - want).abs() < 1.0 && off_a < 1.0 && off_b < 1.0,
        format!(
            "{apart:.1} units apart (want {want:.1}); the grabber looks {off_a:.1}° off the \
             victim, the victim {off_b:.1}° off the grabber"
        ),
    )
}

/// With the arena's block just behind the victim, the placement leaves the
/// victim's box clear of it: the grabber steps back instead.
fn grab_at_wall_keeps_victim_clear() -> Result<String, String> {
    // The block's face is at x = −6 m; the grabber faces it.
    let face = -6.0 / collision::UNIT;
    let half = crate::motion::HALF_WIDTH;
    let victim_x = face + half + 2.0;
    let z = 0.125 - crate::motion::SOLE_Z;
    let ticks = play(&format!(
        "scenario probe\nrules advanced\nseed 7\n{}begin\nwound 1 {HURT}\nduel 0 points={}\n\
         place 0 {} 0 {z} 180\nplace 1 {victim_x} 0 {z} 0\ntick 20\ntick 1 b0=alt\ntick {}\n",
        fighters("fists", ""),
        rules::duel::meters::POINTS_MAX,
        victim_x + 40.0,
        steps(4_000),
    ))?;
    let (a, b) = placed(&ticks).ok_or("no grab held")?;
    let (apart, ..) = stance(&a, &b);
    let clear = b.origin[0] - half >= face - 0.01;
    check(
        clear && (apart - gap_units(1)).abs() < 1.0,
        format!(
            "the victim's box reaches x {:.1} with the block at {face:.1}; {apart:.1} units apart \
             (want {:.1})",
            b.origin[0] - half,
            gap_units(1)
        ),
    )
}

/// A second grabber's grab on a victim already held always misses.
fn grabbed_victim_cannot_be_grabbed() -> Result<String, String> {
    let table = duel_anims(&Missing::None)?;
    let reach = table.get(Anim::BOTH_KYLE_GRAB).length_ms() + grab::REACH_LEFT_MS[0];
    let ticks = play(&format!(
        "scenario probe\nrules advanced\nseed 7\n{}begin\nwound 1 {HURT}\nduel 0 points={max}\n\
         duel 2 points={max}\ntick 20\ntick 1 b0=alt\ntick {}\ntick 1 b2=alt t2=0\ntick {}\n",
        fighters(
            "fists",
            "fighter 2 fists x=40 y=40 yaw=-90 defence=2 tiers=2 fp=100 hp=400\nsheet 2 bab=40\n"
        ),
        steps(reach),
        steps(reach + 2_000),
        max = rules::duel::meters::POINTS_MAX,
    ))?;
    let first = states(&ticks, 1).iter().any(|(_, f)| f.grapple.is_some());
    let second = states(&ticks, 2).iter().any(|(_, f)| f.grapple.is_some());
    let stumbled = played(&ticks, 2, Anim::BOTH_KYLE_MISS);
    check(
        first && !second && stumbled,
        format!(
            "the victim held: {first}; the second grabber holds: {second}; it stumbles: {stumbled}"
        ),
    )
}

/// A beast placed in front (a creature with a strike of its own) is never
/// grabbed: the grab misses.
fn no_beasts_or_droids() -> Result<String, String> {
    let ticks = play(&format!(
        "scenario probe\nrules advanced\nseed 7\n\
         fighter 0 fists x=0 defence=2 tiers=2 fp=100 hp=400\nsheet 0 bab=40\n\
         creature fists x=40 yaw=180 hp=400 beast=1\nbegin\nduel 0 points={}\n\
         tick 20 c0=0\ntick 1 b0=alt c0=0\ntick {} c0=0\n",
        rules::duel::meters::POINTS_MAX,
        steps(4_000),
    ))?;
    let held = states(&ticks, 0).iter().any(|(_, f)| f.grapple.is_some());
    let stumbled = played(&ticks, 0, Anim::BOTH_KYLE_MISS);
    check(
        !held && stumbled,
        format!("a beast in front: held {held}; the grab stumbles {stumbled}"),
    )
}

/// With friendly fire off the players are one party, and none grabs
/// another.
fn no_party_grab_without_friendly_fire() -> Result<String, String> {
    let ticks = grab_with(
        "advanced",
        ("fists", ""),
        HURT,
        "",
        ("friendly off\n", ""),
        2_000,
    )?;
    let held = states(&ticks, 1).iter().any(|(_, f)| f.grapple.is_some());
    check(
        !held,
        format!("friendly fire off: the party member held {held}"),
    )
}

/// Under JA's rules alt attack with bare hands does not grab.
fn no_grab_under_ja_rules() -> Result<String, String> {
    let ticks = grab_with("ja", ("fists", ""), HURT, "", ("", ""), 2_000)?;
    let grabbed = played(&ticks, 0, Anim::BOTH_KYLE_GRAB);
    check(!grabbed, format!("JA's rules: the grab plays {grabbed}"))
}

/// A grab takes a victim on a hillside. The report's save from Dantooine's
/// plains had the player 1.15 m above the Mandalorian, 4.73 m away (a
/// 14° slope): at a punch's reach on that slope, the victim stands lower
/// than JA's flat 8 units allow, and is taken. A victim 1 m up a ledge at
/// that reach is not.
fn grab_on_a_slope() -> Result<String, String> {
    use crate::world::grapples::level_within;
    let reach = crate::world::strikes::player_punch_reach_m() / collision::UNIT;
    let slope = 1.15 / 4.73;
    let below = reach * slope;
    let ledge = 1.0 / collision::UNIT;
    check(
        below > grab::HEIGHT_UNITS && below <= level_within(reach) && ledge > level_within(reach),
        format!(
            "at a punch's reach of {reach:.1} units: the slope's drop {below:.1}, the ledge {ledge:.1}, allowed {:.1} (flat {:.1})",
            level_within(reach),
            grab::HEIGHT_UNITS
        ),
    )
}

/// With a blade in hand alt attack does not grab.
fn no_grab_with_weapon() -> Result<String, String> {
    let ticks = grab_with("advanced", ("vibroblade", ""), HURT, "", ("", ""), 2_000)?;
    let grabbed = played(&ticks, 0, Anim::BOTH_KYLE_GRAB);
    check(
        !grabbed,
        format!("a vibroblade in hand: the grab plays {grabbed}"),
    )
}

/// A finisher's blow is one unarmed hit sized at a chance of 100%: the
/// same blow without `sure` is that much smaller, by KOTOR's chance to hit.
fn finisher_blows_full_damage() -> Result<String, String> {
    let mut grabber = Sheet::commoner();
    grabber.bab = 2;
    let target = Sheet::commoner();
    let blow = |sure: bool| Blow {
        attacker: None,
        relation: Relation::Hostile,
        source: Source::Weapon {
            weapon: Weapon::UNARMED,
            cadence_ms: damage::ROUND_MS as u32,
            share: 1000,
            area: false,
            second: None,
            sure,
        },
        location: Location::None,
        direction: None,
        sneak: 0,
        aim: 0,
    };
    let p = damage::chance(&grabber, &Weapon::UNARMED, &target);
    let mut bad = Vec::new();
    for seed in 0..20 {
        let rng = SimRng::new(seed, 1);
        let size = |sure: bool| {
            let mut rng = rng;
            damage::assess(
                &blow(sure),
                Some(&grabber),
                &target,
                FriendlyFire::Full,
                Game::K1,
                &mut rng,
            )
            .damage
        };
        let (whole, scaled) = (size(true), size(false));
        let want = i64::from(whole) * p / 1000;
        if (i64::from(scaled) - want).abs() > 1 || whole <= scaled {
            bad.push(format!(
                "seed {seed}: sure {whole}, plain {scaled}, want plain {want}"
            ));
        }
    }
    let ticks = grab("")?;
    let blows = blows_on(&ticks);
    check(
        bad.is_empty() && !blows.is_empty() && blows.iter().all(|d| *d > 0),
        format!(
            "chance to hit {p}‰; {bad:?}; the first finisher's blows dealt {blows:?} hundredths"
        ),
    )
}

/// What each blow of seat 0's on seat 1 dealt (hundredths), in order.
fn blows_on(ticks: &[Snapshot]) -> Vec<i32> {
    let mut seen = std::collections::BTreeMap::new();
    for (_, f) in states(ticks, 1) {
        for event in f.newest() {
            if event.by == Some(crate::SimId(0)) && event.weapon == Some(Delivery::Unarmed) {
                seen.insert(event.sequence, event.damage);
            }
        }
    }
    seen.into_values().collect()
}

/// Each finisher lands as many blows as JA's: the rules' list, and one more
/// as the third's flight ends.
fn blow_count_matches_ja() -> Result<String, String> {
    let mut wrong = Vec::new();
    for (keys, finisher) in [("", 1u8), ("r0=1", 2), ("f0=-1", 3)] {
        let ticks = grab_with(
            "advanced",
            ("fists", ""),
            MAX_HP * 100,
            keys,
            ("", ""),
            12_000,
        )?;
        let want = match finisher {
            1 => grab::BLOWS_1.len(),
            2 => grab::BLOWS_2.len(),
            _ => grab::BLOWS_3.len() + 1,
        };
        let got = blows_on(&ticks).len();
        if got != want {
            wrong.push(format!("finisher {finisher}: {got} blows, want {want}"));
        }
    }
    check(wrong.is_empty(), format!("{wrong:?}"))
}

/// When any finisher ends without killing its victim, the victim is let go
/// alive and knocked down, and the second finisher does not keep playing its
/// stand-up animation.
fn victim_down_at_end() -> Result<String, String> {
    let mut wrong = Vec::new();
    for (keys, finisher) in [("", 1u8), ("r0=1", 2), ("f0=-1", 3)] {
        let ticks = grab_with(
            "advanced",
            ("fists", ""),
            MAX_HP * 100,
            keys,
            ("", ""),
            12_000,
        )?;
        let seen = states(&ticks, 1);
        let Some(end) = seen.iter().rposition(|(_, f)| f.grapple.is_some()) else {
            wrong.push(format!("finisher {finisher}: never held"));
            continue;
        };
        let released = seen.get(end + 1);
        let down = released.is_some_and(|(_, f)| {
            f.alive() && f.saber.floored() && f.grapple.is_none()
        });
        if !down {
            wrong.push(format!(
                "finisher {finisher}: not knocked down alive as it ended"
            ));
        }
        if finisher == 2 {
            let knocked = released
                .is_some_and(|(ps, _)| ps.legs.anim == Anim::BOTH_KNOCKDOWN2);
            let continued = seen[end + 1..]
                .iter()
                .any(|(ps, _)| ps.legs.anim == Anim::BOTH_PLAYER_PA_2);
            if !knocked || continued {
                wrong.push(format!(
                    "finisher 2: knockdown {knocked}, continued PA_2 {continued}"
                ));
            }
        }
    }
    check(wrong.is_empty(), format!("{wrong:?}"))
}

/// The third finisher throws its victim into the flight, away from the
/// grabber.
fn finisher_three_throws() -> Result<String, String> {
    let ticks = grab_with(
        "advanced",
        ("fists", ""),
        MAX_HP * 100,
        "f0=-1",
        ("", ""),
        12_000,
    )?;
    let (a, b) = (states(&ticks, 0), states(&ticks, 1));
    let apart: Vec<f32> = a
        .iter()
        .zip(&b)
        .map(|((pa, _), (pb, _))| math::flat_length(math::sub(pb.origin, pa.origin)))
        .collect();
    let thrown = b
        .iter()
        .position(|(ps, _)| ps.legs.anim == Anim::BOTH_PLAYER_PA_3_FLY);
    let later = thrown.and_then(|at| apart.get(at + steps(300) as usize));
    let at = thrown.and_then(|at| apart.get(at));
    let away = at.zip(later).is_some_and(|(at, later)| later > at);
    check(
        thrown.is_some() && away,
        format!(
            "the flight plays from tick {thrown:?}; apart {at:?} units then, {later:?} 300 ms later"
        ),
    )
}

/// A victim the first blow would kill stays standing at 1 HP, marked, while
/// the finisher plays; the blows after deal nothing; it dies on release.
fn death_waits_for_release() -> Result<String, String> {
    let ticks = grab_with("advanced", ("fists", ""), 150, "", ("", ""), 12_000)?;
    let seen = states(&ticks, 1);
    let during: Vec<&Fighter> = seen
        .iter()
        .filter(|(_, f)| f.grapple.is_some())
        .map(|(_, f)| f)
        .collect();
    let marked = during
        .iter()
        .any(|f| f.grapple.is_some_and(|held| held.dead));
    let standing = during.iter().all(|f| f.alive() && f.hp >= 100);
    let blows = blows_on(&ticks);
    let after_mark = blows.iter().skip(1).all(|d| *d == 0);
    let died = seen.iter().any(|(_, f)| !f.alive());
    check(
        marked && standing && after_mark && died && !during.is_empty(),
        format!(
            "marked {marked}; alive at 1 HP or more while held {standing}; blows {blows:?}; dead \
             after {died}"
        ),
    )
}

/// A third fighter punches the victim (at 15 HP) to death during the
/// finisher, holding attack for 6 s (its chained punches deal a round's
/// damage over a round):
/// the finisher plays on to its end, the victim standing until it is let go.
fn third_party_kill_waits_for_release() -> Result<String, String> {
    let table = duel_anims(&Missing::None)?;
    let reach = table.get(Anim::BOTH_KYLE_GRAB).length_ms() + grab::REACH_LEFT_MS[0];
    let ticks = play(&format!(
        "scenario probe\nrules advanced\nseed 7\n{}begin\nwound 1 1500\nduel 0 points={}\n\
         tick 20\ntick 1 b0=alt\ntick {}\ntick {} b2=attack s2=1\ntick {}\n",
        fighters(
            "fists",
            "fighter 2 fists x=40 y=30 yaw=-90 defence=2 tiers=2 fp=100 hp=400\nsheet 2 bab=40\n"
        ),
        rules::duel::meters::POINTS_MAX,
        steps(reach),
        steps(6_000),
        steps(10_000),
    ))?;
    let (a, b) = (states(&ticks, 0), states(&ticks, 1));
    let punched = b.iter().any(|(_, f)| {
        f.newest()
            .any(|event| event.by == Some(crate::SimId(2)) && event.damage > 0)
    }) || b
        .iter()
        .any(|(_, f)| f.grapple.is_some_and(|held| held.dead));
    let standing = b
        .iter()
        .filter(|(_, f)| f.grapple.is_some())
        .all(|(_, f)| f.alive());
    let (grabber, _) = clips(1);
    let length = table.get(grabber).length_ms();
    let run = a.iter().filter(|(ps, _)| ps.legs.anim == grabber).count() as i32;
    let whole = run >= steps(length) - 1;
    let died = b.iter().any(|(_, f)| !f.alive());
    let hp: Vec<i32> = b.iter().map(|(_, f)| f.hp).collect();
    let mut changes = hp.clone();
    changes.dedup();
    check(
        punched && standing && whole && died,
        format!(
            "the third fighter's blows landed {punched}; the victim stood while held {standing}; \
             the finisher played {run} ticks of {} {whole}; dead after {died}; its hit points {changes:?}",
            steps(length)
        ),
    )
}

/// A third fighter kicks the victim again and again during the finisher. The
/// kicks hurt it, but none knocks it down or moves it while it is held, and
/// the finisher plays on to its end.
fn kicked_victim_stays_held() -> Result<String, String> {
    let table = duel_anims(&Missing::None)?;
    let reach = table.get(Anim::BOTH_KYLE_GRAB).length_ms() + grab::REACH_LEFT_MS[0];
    let run = |kicks: &str| {
        play(&format!(
            "scenario probe\nrules advanced\nseed 7\n{}begin\nwound 1 {HURT}\nduel 0 points={}\n\
             tick 20\ntick 1 b0=alt\ntick {}\n{kicks}tick {}\n",
            fighters(
                "fists",
                "fighter 2 fists x=47 y=24 yaw=-90 defence=2 tiers=2 fp=100 hp=400\n\
                 sheet 2 bab=40\n"
            ),
            rules::duel::meters::POINTS_MAX,
            steps(reach),
            steps(6_000),
        ))
    };
    let held = |ticks: &[Snapshot]| {
        states(ticks, 1)
            .iter()
            .filter(|(_, f)| f.grapple.is_some())
            .count()
    };
    let calm = held(&run("")?);
    let ticks = run(&"tick 1 h2=kick\ntick 8\n".repeat(8))?;
    let b = states(&ticks, 1);
    let kicked = b.iter().any(|(_, f)| {
        f.newest()
            .any(|event| event.by == Some(crate::SimId(2)) && event.damage > 0)
    });
    let floored_held = b
        .iter()
        .filter(|(_, f)| f.grapple.is_some() && f.saber.floored())
        .count();
    let kicked_held = held(&ticks);
    check(
        kicked && floored_held == 0 && kicked_held == calm,
        format!(
            "the third fighter's kicks landed {kicked}; steps the victim lay down while held \
             {floored_held}; held for {kicked_held} steps, {calm} without the kicks"
        ),
    )
}

/// The grabber knocked down during a finisher lets go: neither is held, and
/// the victim falls.
fn grabber_down_breaks() -> Result<String, String> {
    let ticks = mid_finisher("force 0 knockdown=2000\n", HURT)?;
    freed(&ticks, "the grabber knocked down")
}

/// The two pushed apart past the break distance are both let go.
fn break_past_distance() -> Result<String, String> {
    let ticks = mid_finisher("shift 1 60 0 0\n", HURT)?;
    freed(&ticks, "the victim pushed 60 units away")
}

/// The second finisher (the duels' table gives it the longest grabber's
/// clip), with `change` (duel lines) made a third of the way into the
/// shorter of its two clips.
fn mid_finisher(change: &str, hurt: i32) -> Result<Vec<Snapshot>, String> {
    let table = duel_anims(&Missing::None)?;
    let reach = table.get(Anim::BOTH_KYLE_GRAB).length_ms() + grab::REACH_LEFT_MS[0];
    let (grabber, victim) = clips(2);
    let shorter = table
        .get(grabber)
        .length_ms()
        .min(table.get(victim).length_ms());
    play(&format!(
        "scenario probe\nrules advanced\nseed 7\n{}begin\nwound 1 {hurt}\nduel 0 points={}\n\
         tick 20\ntick 1 b0=alt r0=1\ntick {} r0=1\ntick {}\n{change}tick 1\ntick {}\n",
        fighters("fists", ""),
        rules::duel::meters::POINTS_MAX,
        steps(reach),
        steps(shorter / 3).max(1),
        steps(1_000),
    ))
}

/// A blow marks the victim to die, then the grabber is knocked down: the
/// grab breaks, and the victim dies then, not living on at 1 HP.
fn broken_grab_still_kills() -> Result<String, String> {
    let ticks = mid_finisher("force 0 knockdown=2000\n", 150)?;
    let seen = states(&ticks, 1);
    let marked = seen
        .iter()
        .any(|(_, f)| f.grapple.is_some_and(|held| held.dead));
    let died = seen.iter().any(|(_, f)| !f.alive());
    check(
        marked && died,
        format!("the victim marked to die {marked}; dead after the break {died}"),
    )
}

/// After the change, both are free and the victim is down.
fn freed(ticks: &[Snapshot], what: &str) -> Result<String, String> {
    let (a, b) = (states(ticks, 0), states(ticks, 1));
    let held_before = b.iter().any(|(_, f)| f.grapple.is_some());
    let last = a.last().zip(b.last());
    let free = last.is_some_and(|((_, fa), (_, fb))| fa.grapple.is_none() && fb.grapple.is_none());
    let fell = b.iter().any(|(_, f)| f.saber.floored());
    check(
        held_before && free && fell,
        format!("{what}: held first {held_before}; both free after {free}; the victim fell {fell}"),
    )
}

/// Nothing loops: through each finisher the victim's clip plays once, from
/// its start, never started over.
fn victim_clip_never_loops() -> Result<String, String> {
    let table = duel_anims(&Missing::None)?;
    let mut wrong = Vec::new();
    for (keys, finisher) in [("", 1u8), ("r0=1", 2), ("f0=-1", 3)] {
        let ticks = grab_with(
            "advanced",
            ("fists", ""),
            MAX_HP * 100,
            keys,
            ("", ""),
            12_000,
        )?;
        let (_, victim) = clips(finisher);
        let seen = states(&ticks, 1);
        let runs: Vec<(bool, i32)> = seen
            .iter()
            .filter(|(ps, _)| ps.legs.anim == victim)
            .map(|(ps, _)| (ps.legs.flip, ps.legs.timer))
            .collect();
        let once = runs
            .windows(2)
            .all(|w| w[0].0 == w[1].0 && w[1].1 <= w[0].1);
        if runs.is_empty() || !once {
            wrong.push(format!(
                "finisher {finisher}: {} ticks, played once {once}",
                runs.len()
            ));
        }
    }
    let lengths: Vec<String> = [1u8, 2, 3]
        .iter()
        .map(|&n| {
            let (g, v) = clips(n);
            format!(
                "{} {} ms, {} {} ms",
                g.name(),
                table.get(g).length_ms(),
                v.name(),
                table.get(v).length_ms()
            )
        })
        .collect();
    check(
        wrong.is_empty(),
        format!("{wrong:?}; the table's clips: {lengths:?}"),
    )
}

/// The first placed creature's legs' animations every tick.
fn creature_legs(ticks: &[Snapshot]) -> Vec<Anim> {
    ticks
        .iter()
        .filter_map(|tick| {
            tick.entities.iter().find_map(|entity| {
                (entity.kind == Kind::Object(crate::module::ObjectKind::Creature, 0))
                    .then_some(entity.player)
                    .flatten()
            })
        })
        .map(|ps| ps.legs.anim)
        .collect()
}

/// A hostile creature with bare hands and a strong attack fights seat 0
/// (bare hands too, standing still) at `hp` hundredths of its 400 HP,
/// under `rules`, for 8 s: whether it grabbed, and whether it punched.
fn enemy_against(rules: &str, hp: i32) -> Result<(bool, bool), String> {
    let ticks = play(&format!(
        "scenario probe\nrules {rules}\nseed 7\n\
         fighter 0 fists x=0 defence=2 tiers=2 fp=100 hp={MAX_HP}\n\
         creature fists x=45 yaw=180 defence=2 tiers=2 fp=100 hp={MAX_HP}\n\
         sheet c0 bab=40\nbegin\nwound 0 {hp}\ntick {}\n",
        steps(8_000),
    ))?;
    let legs = creature_legs(&ticks);
    let grabbed = legs.contains(&Anim::BOTH_KYLE_GRAB);
    let punched = ticks.iter().any(|tick| {
        tick.entities.iter().any(|entity| {
            entity.kind == Kind::Object(crate::module::ObjectKind::Creature, 0)
                && entity.player.is_some_and(|ps| {
                    matches!(ps.torso.anim, Anim::BOTH_MELEE1 | Anim::BOTH_MELEE2)
                })
        })
    });
    Ok((grabbed, punched))
}

/// An enemy with bare hands grabs a target below `grab::HURT_BELOW_PCT` of
/// its health within its reach.
fn enemy_grabs_hurt_target() -> Result<String, String> {
    let (grabbed, _) = enemy_against("advanced", HURT)?;
    check(
        grabbed,
        format!("a target at 30% health: the enemy grabs {grabbed}"),
    )
}

/// It never grabs a target at or above that share: it punches. (The
/// target starts whole, so the punches do not bring it below the share.)
fn enemy_never_grabs_healthy_target() -> Result<String, String> {
    let (grabbed, punched) = enemy_against("advanced", MAX_HP * 100)?;
    check(
        !grabbed && punched,
        format!("a target at full health: the enemy grabs {grabbed}, punches {punched}"),
    )
}

/// Under JA's rules the enemy only punches, a hurt target too.
fn no_new_moves_under_ja_rules() -> Result<String, String> {
    let (grabbed, punched) = enemy_against("ja", HURT)?;
    check(
        !grabbed && punched,
        format!("JA's rules, a target at 30% health: the enemy grabs {grabbed}, punches {punched}"),
    )
}

/// How far (metres) a thrown victim lands from where it was thrown.
const LANDS_M: (f32, f32) = (1.5, 2.0);

/// The first and third finishers throw the victim back, and it comes to
/// rest `LANDS_M` from where it was thrown.
fn throw_lands_near() -> Result<String, String> {
    let mut found = Vec::new();
    let mut ok = true;
    for (keys, finisher, anim) in [
        ("", 1u8, Anim::BOTH_KNOCKDOWN2),
        ("f0=-1", 3, Anim::BOTH_PLAYER_PA_3_FLY),
    ] {
        let ticks = grab_with(
            "advanced",
            ("fists", ""),
            MAX_HP * 100,
            keys,
            ("", ""),
            12_000,
        )?;
        let seen = states(&ticks, 1);
        let thrown = seen
            .iter()
            .position(|(ps, _)| ps.legs.anim == anim)
            .ok_or(format!("finisher {finisher}: no throw"))?;
        let at = |i: usize| seen.get(i).map(|(ps, _)| ps.origin);
        let (from, to) = (
            at(thrown),
            at(thrown + steps(3_000) as usize).or(at(seen.len() - 1)),
        );
        let (from, to) = from.zip(to).ok_or("no ticks")?;
        let metres = math::flat_length(math::sub(to, from)) * collision::UNIT;
        ok &= (LANDS_M.0..=LANDS_M.1).contains(&metres);
        found.push(format!("finisher {finisher}: {metres:.2} m"));
    }
    check(
        ok,
        format!(
            "{} (want {} to {} m)",
            found.join(", "),
            LANDS_M.0,
            LANDS_M.1
        ),
    )
}

/// When a blow in the second finisher marks the victim to die, the finisher
/// lets go `grab::LET_GO_MS` in and the victim dies then, falling as a ragdoll
/// from the pose it is in (`Fighter::blasted_ms`) without playing the rest
/// of its clip.
fn finisher_two_kills_as_ragdoll() -> Result<String, String> {
    let ticks = grab_with("advanced", ("fists", ""), 150, "r0=1", ("", ""), 12_000)?;
    let seen: Vec<(u64, MotionState, Fighter)> = ticks
        .iter()
        .filter_map(|tick| {
            let entity = tick.own(Seat(1))?;
            Some((tick.time_ms, entity.player?, entity.fighter?))
        })
        .collect();
    let marked = seen
        .iter()
        .any(|(.., f)| f.grapple.is_some_and(|held| held.dead));
    let held = seen
        .iter()
        .rposition(|(.., f)| f.grapple.is_some())
        .ok_or("no grab held")?;
    let (released, ps, f) = seen.get(held + 1).ok_or("never let go")?;
    let dead = !f.alive();
    let limp = f.blasted_ms > 0 && f.blasted_ms <= *released;
    let stood = seen[held + 1..]
        .iter()
        .any(|(_, ps, _)| ps.legs.anim == Anim::BOTH_PLAYER_PA_2);
    check(
        marked && dead && limp && !stood && ps.legs.anim != Anim::BOTH_PLAYER_PA_2,
        format!(
            "marked {marked}; let go at {released} ms: dead {dead}; falls limp {limp} (from {} ms); its clip plays on after {stood}",
            f.blasted_ms
        ),
    )
}

/// Neutral outsiders, faction friends with friendly fire off, and plot NPCs
/// are never grabbed, while ordinary hostile NPCs are grabbed.
fn no_neutral_friendly_or_plot_grab() -> Result<String, String> {
    let table = duel_anims(&Missing::None)?;
    let reach = table.get(Anim::BOTH_KYLE_GRAB).length_ms() + grab::REACH_LEFT_MS[0];
    let cases = [
        ("neutral", "friendly off\n", "faction=friendly", false),
        ("neutral_ff", "", "faction=friendly", false),
        ("ally", "friendly off\n", "faction=ally", false),
        ("plot", "friendly off\n", "plot=1", false),
        ("hostile", "friendly off\n", "", true),
    ];
    let mut wrong = Vec::new();
    for (label, before, creature_opts, want) in cases {
        let ticks = play(&format!(
            "scenario probe\nrules advanced\nseed 7\n{before}\
             fighter 0 fists x=0 defence=2 tiers=2 fp=100 hp={MAX_HP}\n\
             creature fists x=40 yaw=180 defence=2 tiers=2 fp=100 hp={MAX_HP} {creature_opts}\n\
             sheet 0 bab=40\nbegin\nwound c0 {HURT}\nduel 0 points={}\n\
             tick 20\ntick 1 b0=alt\ntick {}\ntick {}\n",
            rules::duel::meters::POINTS_MAX,
            steps(reach),
            steps(2_000),
        ))?;
        let held = creature_states(&ticks)
            .iter()
            .any(|(_, f)| f.grapple.is_some());
        if held != want {
            wrong.push(format!("{label}: held {held}, want {want}"));
        }
    }
    check(wrong.is_empty(), format!("{wrong:?}"))
}

/// The first placed creature's body and fight every tick.
fn creature_states(ticks: &[Snapshot]) -> Vec<(MotionState, Fighter)> {
    ticks
        .iter()
        .filter_map(|tick| {
            tick.entities.iter().find_map(|entity| {
                (entity.kind == Kind::Object(crate::module::ObjectKind::Creature, 0))
                    .then(|| entity.player.zip(entity.fighter))
                    .flatten()
            })
        })
        .collect()
}

/// Seat 0 grabs a hostile creature with a blaster rifle in hand, at 30% of
/// its health, holding `keys` through the grab. Its own mind keeps fighting
/// meanwhile. The creature's body and fight every tick.
fn grab_gunner(keys: &str) -> Result<Vec<(MotionState, Fighter)>, String> {
    let table = duel_anims(&Missing::None)?;
    let reach = table.get(Anim::BOTH_KYLE_GRAB).length_ms() + grab::REACH_LEFT_MS[0];
    let ticks = play(&format!(
        "scenario probe\nrules advanced\nseed 7\n\
         fighter 0 fists x=0 defence=2 tiers=2 fp=100 hp={MAX_HP}\nsheet 0 bab=40\n\
         creature rifle x=40 yaw=180 defence=2 tiers=2 fp=100 hp={MAX_HP}\n\
         begin\nwound c0 {HURT}\nduel 0 points={}\n\
         tick 1 c0=0\ntick 1 b0=alt c0=0 {keys}\ntick {} c0=0 {keys}\ntick {} c0=0\n",
        rules::duel::meters::POINTS_MAX,
        steps(reach),
        steps(12_000),
    ))?;
    Ok(creature_states(&ticks))
}

/// A held victim plays its finisher's clip, legs and torso, every step it is
/// held: no shot, no change of weapons, no raised gun, no flinch and no
/// jump's or landing's legs take the clip's place. The third finisher's
/// victim flies in its flight's clip once thrown. Its mind starts nothing
/// while it is held, so its command holds no button. A gunner is the
/// victim, for each finisher.
fn held_victim_keeps_its_clip() -> Result<String, String> {
    let mut wrong = Vec::new();
    for (keys, finisher) in [("", 1u8), ("r0=1", 2), ("f0=-1", 3)] {
        let seen = grab_gunner(keys)?;
        let (_, victim) = clips(finisher);
        let held: Vec<&(MotionState, Fighter)> =
            seen.iter().filter(|(_, f)| f.grapple.is_some()).collect();
        // The third finisher's victim flies in its own clip once thrown.
        let clip = |f: &Fighter| match f.grapple {
            Some(held) if held.flying => Anim::BOTH_PLAYER_PA_3_FLY,
            _ => victim,
        };
        let off: Vec<String> = held
            .iter()
            .filter(|(ps, f)| {
                ps.legs.anim != clip(f)
                    || ps.torso.anim != clip(f)
                    || f.saber.change != crate::combat::saber::Change::None
                    || f.last_command.buttons != 0
            })
            .map(|(ps, f)| {
                format!(
                    "{}/{} {:?}, buttons {}",
                    ps.legs.anim.name(),
                    ps.torso.anim.name(),
                    f.saber.change,
                    f.last_command.buttons
                )
            })
            .collect();
        if held.is_empty() || !off.is_empty() {
            wrong.push(format!(
                "finisher {finisher}: {} steps held, {} off the clip, first {:?}",
                held.len(),
                off.len(),
                off.first()
            ));
        }
    }
    check(wrong.is_empty(), format!("{wrong:?}"))
}

/// The third finisher throws a victim that a blow marked to die: it is dead
/// from the step it is thrown and falls limp as a ragdoll
/// (`Fighter::blasted_ms`). It never flies alive in the flight's clip.
fn finisher_three_throws_marked_victim_limp() -> Result<String, String> {
    let ticks = grab_with("advanced", ("fists", ""), 150, "f0=-1", ("", ""), 12_000)?;
    let seen: Vec<(u64, MotionState, Fighter)> = ticks
        .iter()
        .filter_map(|tick| {
            let entity = tick.own(Seat(1))?;
            Some((tick.time_ms, entity.player?, entity.fighter?))
        })
        .collect();
    let marked = seen
        .iter()
        .any(|(.., f)| f.grapple.is_some_and(|held| held.dead));
    let held = seen
        .iter()
        .rposition(|(.., f)| f.grapple.is_some())
        .ok_or("no grab held")?;
    let (at, _, f) = seen.get(held + 1).ok_or("never let go")?;
    let limp = !f.alive() && f.blasted_ms > 0 && f.blasted_ms <= *at;
    let flew_alive = seen
        .iter()
        .any(|(.., f)| f.grapple.is_some_and(|held| held.flying));
    check(
        marked && limp && !flew_alive,
        format!(
            "marked to die {marked}; let go at {at} ms: dead and limp {limp} (from {} ms); flew alive in the flight's clip {flew_alive}",
            f.blasted_ms
        ),
    )
}

/// A probe: its name and what runs it.
type Probe = (&'static str, fn() -> Result<String, String>);

pub fn probes() -> Checks {
    let all: [Probe; 31] = [
        ("kicked_victim_stays_held", kicked_victim_stays_held),
        (
            "finisher_three_throws_marked_victim_limp",
            finisher_three_throws_marked_victim_limp,
        ),
        ("finisher_by_start_key", finisher_by_start_key),
        ("held_victim_keeps_its_clip", held_victim_keeps_its_clip),
        ("alt_attack_grabs", alt_attack_grabs),
        ("grab_miss_stumbles", grab_miss_stumbles),
        ("finisher_by_key", finisher_by_key),
        ("victim_placed", victim_placed),
        (
            "grab_at_wall_keeps_victim_clear",
            grab_at_wall_keeps_victim_clear,
        ),
        (
            "grabbed_victim_cannot_be_grabbed",
            grabbed_victim_cannot_be_grabbed,
        ),
        ("no_beasts_or_droids", no_beasts_or_droids),
        (
            "no_party_grab_without_friendly_fire",
            no_party_grab_without_friendly_fire,
        ),
        (
            "no_neutral_friendly_or_plot_grab",
            no_neutral_friendly_or_plot_grab,
        ),
        ("no_grab_under_ja_rules", no_grab_under_ja_rules),
        ("no_grab_with_weapon", no_grab_with_weapon),
        ("grab_on_a_slope", grab_on_a_slope),
        ("finisher_blows_full_damage", finisher_blows_full_damage),
        ("blow_count_matches_ja", blow_count_matches_ja),
        ("victim_down_at_end", victim_down_at_end),
        ("finisher_three_throws", finisher_three_throws),
        ("death_waits_for_release", death_waits_for_release),
        (
            "third_party_kill_waits_for_release",
            third_party_kill_waits_for_release,
        ),
        ("grabber_down_breaks", grabber_down_breaks),
        ("break_past_distance", break_past_distance),
        ("broken_grab_still_kills", broken_grab_still_kills),
        ("victim_clip_never_loops", victim_clip_never_loops),
        ("throw_lands_near", throw_lands_near),
        (
            "finisher_two_kills_as_ragdoll",
            finisher_two_kills_as_ragdoll,
        ),
        ("enemy_grabs_hurt_target", enemy_grabs_hurt_target),
        (
            "enemy_never_grabs_healthy_target",
            enemy_never_grabs_healthy_target,
        ),
        ("no_new_moves_under_ja_rules", no_new_moves_under_ja_rules),
    ];
    all.into_iter().map(|(name, run)| (name, run())).collect()
}
