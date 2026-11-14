//! The probes of how fighters pace and spend a fight under the Advanced
//! rules (`kjk sim probes --only duelling`). Special moves use up Stamina,
//! for every character, and a Force user's Force points are left for the
//! Force powers; under Jedi Academy's rules the older bar still pays.

mod stamina;

use super::duels::play;
use crate::Seat;
use crate::combat::Missile;
use crate::combat::pace::{
    Approach, Arms, BACKING_MS, Breaker, Charge, Dice, Ending, Keys, Pace as Pacing, Reaction,
    Sight, Special, TIP, Tier, WALK_WITHIN, Way, ai_may_special, aimed_at, approach_gun,
    bolt_headed_at, bolt_incoming, breaker, chain_limit, ending, found_tier, guard_delay_ms,
    guard_is_down, hit_lock_ms, holds_for_bolts, is_boss_tag, may_charge, moving_away, react,
    specials, tier, walks,
};
use crate::combat::saber::duel::exchange;
use crate::combat::saber::duel::standing::{cap, may_start_special, special_cost};
use crate::combat::saber::duel::{Duel, Pace, swing_speed};
use crate::combat::saber::{Move, Style};
use crate::combat::sheet::Sheet;
use crate::combat::{Fighter, Outcome};
use crate::math::Vec3;
use crate::module::ObjectKind;
use crate::motion::anim::is_kick;
use crate::motion::move_speed;
use crate::probe::{Checks, check};
use crate::rng::SimRng;
use crate::snapshot::SimId;
use crate::snapshot::{EntityState, Kind, Snapshot};
use crate::wire::{Reader, Versions, Writer};

/// Seat 0's level in the fights below, and so its Stamina maximum.
const LEVEL: u8 = 5;
/// How many ticks make a second.
const TICKS_PER_SECOND: usize = 125;
/// The ticks the fight's lighting and settling take.
const SETTLED: usize = 61;
/// The ticks after that a weapon takes to be drawn and ready.
const READY: usize = 150;
/// A tick safely inside that wait: the last one before a key is pressed.
const READY_AT: usize = SETTLED + READY - 5;
/// The `feat.2da` row of Rapid Shot, whose burst a gunner starts with the
/// alternate button.
const RAPID_SHOT: u16 = 30;

/// A fight of seat 0 (`kind`, `fp` its Force points) against a medium-style
/// foe standing by, under `rules`, with `then` after the settling.
fn fight(rules: &str, (kind, fp): (&str, u16), more: &str, then: &str) -> String {
    format!(
        "scenario probe\nrules {rules}\nseed 7\n\
         fighter 0 {kind} x=0 defence=2 tiers=2 fp={fp} hp=400\n\
         fighter 1 medium x=45 yaw=180 defence=2 tiers=2 fp=100 hp=400\n\
         sheet 0 level={LEVEL}\n{more}\nbegin\ntick 1 b0=attack b1=attack\ntick 60\n{then}\n"
    )
}

/// What a fight of seat 0 left of its meters: at the end of the settling,
/// and the lowest each fell to afterwards.
#[derive(Debug, Clone, Copy)]
struct Spent {
    stamina: i32,
    older: i32,
    /// The most Stamina paid in one tick: Stamina refilling meanwhile does
    /// not hide what a move used up.
    largest_fall: i32,
    lowest_older: i32,
    last_stamina: i32,
}

impl Spent {
    /// What seat 0 spent from the tick `from` on.
    fn of(ticks: &[Snapshot], from: usize) -> Result<Spent, String> {
        let fighters: Vec<Fighter> = ticks
            .iter()
            .filter_map(|tick| tick.own(Seat(0)).and_then(|entity| entity.fighter))
            .collect();
        let start = fighters.get(from).ok_or("no ticks")?;
        let after = &fighters[from..];
        let stamina = |f: &Fighter| i32::from(f.saber.duel.points);
        Ok(Spent {
            stamina: stamina(start),
            older: start.fp,
            largest_fall: after
                .windows(2)
                .map(|pair| stamina(&pair[0]) - stamina(&pair[1]))
                .max()
                .unwrap_or_default()
                .max(0),
            lowest_older: after.iter().map(|f| f.fp).min().unwrap_or_default(),
            last_stamina: after.last().map(stamina).unwrap_or_default(),
        })
    }

    /// Stamina used up.
    fn stamina_used(self) -> i32 {
        self.largest_fall
    }

    /// The older bar used up, in hundredths.
    fn older_used(self) -> i32 {
        self.older - self.lowest_older
    }
}

/// Whether the rules' own cost of a special move is the table asked for.
fn special_cost_table() -> Result<String, String> {
    let rows = [(25, 100, 25), (25, 62, 15), (1, 60, 1)];
    let got = rows.map(|(percent, cap, _)| special_cost(percent, cap));
    let want = rows.map(|(.., cost)| cost);
    check(
        got == want,
        format!("special_cost for {rows:?}: {got:?}, the rules ask {want:?}"),
    )
}

/// A tired fighter, or one without the points, starts no special move.
fn tired_starts_no_special() -> Result<String, String> {
    let rows = [(20, 5, false), (21, 21, true), (30, 31, false)];
    let got = rows.map(|(points, cost, _)| may_start_special(points, cost));
    let want = rows.map(|(.., may)| may);
    check(
        got == want,
        format!("may_start_special for {rows:?}: {got:?}, the rules ask {want:?}"),
    )
}

/// A creature keeps 40 Stamina back from a special move of its own choice.
fn ai_keeps_forty() -> Result<String, String> {
    let (enough, short) = (ai_may_special(65, 25), ai_may_special(64, 25));
    check(
        enough && !short,
        format!("with 65 Stamina and a cost of 25 {enough}; with 64 {short}"),
    )
}

/// Seat 0 holding `kind` with `fp` Force points starts a kata and then
/// stands still, under `rules`.
fn kata(rules: &str, kind: (&str, u16)) -> Result<Spent, String> {
    let then = format!("tick {READY}\ntick 3 b0=attack+alt\ntick 40");
    Spent::of(&play(&fight(rules, kind, "", &then))?, READY_AT)
}

/// Under the Advanced rules a kata uses up its percentage of the fighter's
/// maximum Stamina and no Force points, for a Jedi and for a soldier with a
/// vibroblade alike.
fn kata_uses_stamina() -> Result<String, String> {
    let want = special_cost(i32::from(rules::saber_moves::costs::KATA), cap(LEVEL));
    let mut right = true;
    let mut said = Vec::new();
    for (who, kind) in [
        ("a Jedi", ("medium", 100)),
        ("a soldier", ("vibroblade", 0)),
    ] {
        let spent = kata("advanced", kind)?;
        right &= spent.stamina_used() == want && spent.older_used() <= 0;
        said.push(format!(
            "{who} used up {} Stamina and {} hundredths of the older bar",
            spent.stamina_used(),
            spent.older_used()
        ));
    }
    check(
        right,
        format!(
            "advanced: {}; the rules ask {want} Stamina",
            said.join("; ")
        ),
    )
}

/// Under Jedi Academy's rules the same kata uses up the older bar, a
/// percentage of the pool, and no Stamina.
fn kata_uses_force_under_ja() -> Result<String, String> {
    let mut right = true;
    let mut said = Vec::new();
    for (who, kind) in [
        ("a Jedi", ("medium", 100)),
        ("a soldier", ("vibroblade", 0)),
    ] {
        let mut sheet = Sheet::commoner();
        sheet.max_fp = kind.1;
        let want = i32::from(sheet.pool()) * i32::from(rules::saber_moves::costs::KATA);
        let spent = kata("ja", kind)?;
        right &= spent.older_used() == want && spent.stamina_used() == 0;
        said.push(format!(
            "{who} used up {} hundredths of the older bar (the rules ask {want}) and {} Stamina",
            spent.older_used(),
            spent.stamina_used()
        ));
    }
    check(right, format!("ja: {}", said.join("; ")))
}

/// Seat 0 holding a pistol with `fp` Force points and Rapid Shot presses
/// the alternate button, after `more`, under `rules`.
fn burst(rules: &str, fp: u16, before: &str) -> Result<Spent, String> {
    let more = format!("sheet 0 feats={RAPID_SHOT}");
    let then = format!("tick {READY}\n{before}\ntick 3 b0=alt\ntick 80\ntick 3 b0=alt\ntick 20");
    Spent::of(
        &play(&fight(rules, ("pistol", fp), &more, &then))?,
        SETTLED + READY,
    )
}

/// Under the Advanced rules a feat special uses up its percentage of the
/// fighter's maximum Stamina and no Force points, for a gunner with and
/// without the Force; under Jedi Academy's rules it uses the older bar.
fn feat_special_uses_stamina() -> Result<String, String> {
    let want = special_cost(i32::from(rules::feats::specials::COST_PERCENT), cap(LEVEL));
    let mut right = true;
    let mut said = Vec::new();
    for (who, fp) in [("a Force user", 100), ("a gunner without the Force", 0)] {
        let advanced = burst("advanced", fp, "")?;
        let ja = burst("ja", fp, "")?;
        right &= advanced.stamina_used() == want
            && advanced.older_used() <= 0
            && ja.older_used() > 0
            && ja.stamina_used() == 0;
        said.push(format!(
            "{who}: advanced {} Stamina and {} older, ja {} Stamina and {} older",
            advanced.stamina_used(),
            advanced.older_used(),
            ja.stamina_used(),
            ja.older_used()
        ));
    }
    check(
        right,
        format!(
            "{}; the rules ask {want} Stamina when advanced",
            said.join("; ")
        ),
    )
}

/// A tired gunner's alternate button starts no special: its Stamina is not
/// used up, and it pays no Force points.
fn tired_gunner_starts_no_special() -> Result<String, String> {
    // A little under the tired mark, which Stamina refilling in the few ticks
    // before the key is read would otherwise cross.
    let tired = rules::duel::meters::TIRED - 5;
    let spent = burst("advanced", 100, &format!("duel 0 points={tired}"))?;
    check(
        // A plain shot may cost a point; a special would cost ten.
        spent.last_stamina + 2 >= tired && spent.older_used() <= 0,
        format!(
            "advanced: a gunner with {tired} Stamina ended with {} and used up {} older bar",
            spent.last_stamina,
            spent.older_used()
        ),
    )
}

/// Every character has Stamina under the Advanced rules, whatever it holds:
/// it starts at its maximum, never over it, and refills from a low bar.
fn every_holder_has_stamina() -> Result<String, String> {
    let mut right = true;
    let mut said = Vec::new();
    for kind in ["medium", "vibroblade", "pistol", "fists"] {
        let held = |points: Option<u8>| -> Result<(i32, i32), String> {
            let set = points.map_or(String::new(), |points| format!("duel 0 points={points}\n"));
            let then = format!("{set}tick 600");
            let spent = Spent::of(&play(&fight("advanced", (kind, 0), "", &then))?, SETTLED)?;
            Ok((spent.stamina, spent.last_stamina))
        };
        let (full, _) = held(None)?;
        let (_, refilled) = held(Some(10))?;
        let most = i32::from(cap(LEVEL));
        right &= full == most && refilled > 10;
        said.push(format!(
            "{kind}: starts at {full}, from 10 it rests to {refilled}"
        ));
    }
    check(
        right,
        format!(
            "advanced: {}; the maximum is {}",
            said.join("; "),
            cap(LEVEL)
        ),
    )
}

/// The moves that start or make up a swing (a kick is not one), in order,
/// each with the milliseconds its torso animation was held for on the tick it
/// began, for one fighter of a played scenario (`who` finds it in a snapshot).
fn swing_holds(
    text: &str,
    who: impl Fn(&Snapshot) -> Option<&EntityState>,
) -> Result<Vec<(Move, i32)>, String> {
    let mut holds = Vec::new();
    let mut last = None;
    for tick in &play(text)? {
        let Some(entity) = who(tick) else { continue };
        let (Some(fighter), Some(motion)) = (entity.fighter, entity.player) else {
            continue;
        };
        let current = fighter.saber.current_move;
        if last != Some(current) && (current.start() || current.attack()) && !current.kick() {
            holds.push((current, motion.torso.timer));
        }
        last = Some(current);
    }
    Ok(holds)
}

/// Seat 0, a medium-style fighter, swings once at a foe standing by.
fn player_swing_holds(rules: &str) -> Result<Vec<(Move, i32)>, String> {
    let then = format!("tick {READY}\ntick 3 b0=attack\ntick 100");
    swing_holds(&fight(rules, ("medium", 100), "", &then), |tick| {
        tick.own(Seat(0))
    })
}

/// A creature with a vibroblade fights seat 0, who holds block for a second
/// and a half and then lets it go.
fn creature_swing_holds(rules: &str) -> Result<Vec<(Move, i32)>, String> {
    let text = format!(
        "scenario probe\nrules {rules}\nseed 7\n\
         fighter 0 medium x=0 defence=2 tiers=2 fp=100 hp=400\n\
         creature vibroblade x=45 yaw=180 defence=2 tiers=2 hp=400\n\
         begin\ntick 1\ntick 60\ntick 150 h0=block c0=0\ntick 650 c0=0\n"
    );
    swing_holds(&text, |tick| {
        tick.entities
            .iter()
            .find(|e| e.kind == Kind::Object(ObjectKind::Creature, 0))
    })
}

/// The first move that both lists hold, with each list's hold of it.
fn first_shared(advanced: &[(Move, i32)], ja: &[(Move, i32)]) -> Option<(Move, i32, i32)> {
    advanced.iter().find_map(|&(swing, held)| {
        ja.iter()
            .find(|&&(other, _)| other == swing)
            .map(|&(_, base)| (swing, held, base))
    })
}

/// The hold of a medium-style player's wind-up under Jedi Academy's rules,
/// measured on c169ad51, before the Advanced rules slowed their swings, in
/// milliseconds.
const JA_WIND_UP_HELD: i32 = 449;

/// Each style swings at its own speed, and a weapon that is not a
/// lightsaber at the medium one.
fn swing_speeds() -> Result<String, String> {
    let rows = [
        (Some(Style::Fast), 0.95),
        (Some(Style::Medium), 0.85),
        (Some(Style::Strong), 0.80),
        (Some(Style::Dual), 0.85),
        (Some(Style::Staff), 0.85),
        (None, 0.85),
    ];
    let got = rows.map(|(style, _)| swing_speed(style));
    let want = rows.map(|(_, speed)| speed);
    check(
        got == want,
        format!("swing_speed for {rows:?}: {got:?}, the rules ask {want:?}"),
    )
}

/// A real fighter's swing holds each of its moves longer under the Advanced
/// rules than under Jedi Academy's, for a player with a lightsaber and for
/// a creature with a melee weapon.
fn swings_last_longer() -> Result<String, String> {
    let mut right = true;
    let mut said = Vec::new();
    for (who, advanced, ja) in [
        (
            "a player",
            player_swing_holds("advanced")?,
            player_swing_holds("ja")?,
        ),
        (
            "a creature",
            creature_swing_holds("advanced")?,
            creature_swing_holds("ja")?,
        ),
    ] {
        match first_shared(&advanced, &ja) {
            Some((swing, held, base)) => {
                right &= held > base;
                said.push(format!(
                    "{who}: {} held {held} ms advanced, {base} ms ja",
                    swing.name()
                ));
            }
            None => {
                right = false;
                said.push(format!(
                    "{who}: no move in common, advanced {advanced:?}, ja {ja:?}"
                ));
            }
        }
    }
    check(right, said.join("; "))
}

/// Under Jedi Academy's rules a player's wind-up holds its move for the
/// milliseconds measured before the Advanced rules changed swings.
fn ja_swing_unchanged() -> Result<String, String> {
    let held = player_swing_holds("ja")?
        .first()
        .map_or(0, |&(_, held)| held);
    check(
        held == JA_WIND_UP_HELD,
        format!("ja: the first move of a swing held {held} ms; c169ad51 held {JA_WIND_UP_HELD} ms"),
    )
}

/// The pace a move plays at through the real `move_speed`, for each cause
/// of it: a bounce at its pace's speed, a swing's start and attack at the
/// style's speed, a tired swing slower again, and a riposte quicker than
/// the swing it replaces.
fn speed_by_cause() -> Result<String, String> {
    use rules::duel::{meters, perfect, speed};
    let tired = meters::TIRED as u8;
    let rows: [(&str, Duel, Move, f32); 8] = [
        (
            "blocked",
            Duel {
                pace: Pace::Blocked,
                ..Duel::FRESH
            },
            Move::B1_BR,
            speed::BOUNCES[0],
        ),
        (
            "parried",
            Duel {
                pace: Pace::Parried,
                ..Duel::FRESH
            },
            Move::B1_BR,
            speed::BOUNCES[1],
        ),
        (
            "staggered",
            Duel {
                pace: Pace::Staggered,
                ..Duel::FRESH
            },
            Move::B1_BR,
            speed::BOUNCES[2],
        ),
        (
            "perfect",
            Duel {
                pace: Pace::Perfect,
                ..Duel::FRESH
            },
            Move::B1_BR,
            speed::BOUNCES[3],
        ),
        ("start", Duel::FRESH, Move::S_TL2BR, speed::STYLES[1]),
        ("attack", Duel::FRESH, Move::A_TL2BR, speed::STYLES[1]),
        (
            "tired",
            Duel {
                points: tired,
                ..Duel::FRESH
            },
            Move::A_TL2BR,
            speed::TIRED * speed::STYLES[1],
        ),
        (
            "riposte",
            Duel {
                riposte_until: 10,
                ..Duel::FRESH
            },
            Move::A_TL2BR,
            speed::STYLES[1] / perfect::RIPOSTE_TIME,
        ),
    ];
    let mut wrong = Vec::new();
    for (cause, duel, next, want) in rows {
        let got = move_speed(Some(&duel), Some(Style::Medium), next.data().anim, next, 0);
        if (got - want).abs() > 1e-6 {
            wrong.push(format!("{cause}: {got}, the rules ask {want}"));
        }
    }
    check(
        wrong.is_empty(),
        format!(
            "advanced: {} causes play at their speeds; wrong: {wrong:?}",
            rows.len()
        ),
    )
}

/// Whether an AI fighter walks near a blade and not near a blaster.
fn walks_near_a_blade() -> Result<String, String> {
    let rows = [
        (100.0, false, true),
        (130.0, false, false),
        (100.0, true, false),
    ];
    let got = rows.map(|(gap, ranged, _)| walks(gap, ranged, 0, false));
    let want = rows.map(|(.., walk)| walk);
    check(
        got == want,
        format!("walks within {WALK_WITHIN} for {rows:?}: {got:?}, the rules ask {want:?}"),
    )
}

/// A target that has backed away for longer than the limit is run after,
/// until it is within reach.
fn runs_after_a_backing_target() -> Result<String, String> {
    let rows = [
        (BACKING_MS + 100, false, false),
        (BACKING_MS + 100, true, true),
        (BACKING_MS, false, true),
    ];
    let got = rows.map(|(ms, reach, _)| walks(100.0, false, ms, reach));
    let want = rows.map(|(.., walk)| walk);
    check(
        got == want,
        format!("walks after (ms, in reach) {rows:?}: {got:?}, the rules ask {want:?}"),
    )
}

/// A melee target `apart` units away (centre to centre) that does not
/// move, seen by a creature that holds its place.
fn duel_sight(apart: f32) -> Sight {
    Sight {
        foe: SimId(2),
        gap: apart - TIP,
        in_view: true,
        closing: None,
        arms: Arms::Saber { lit: true },
        swinging: false,
        grounded: true,
        moving: false,
        coming: 0.0,
        facing: 1.0,
        busy: false,
        ally: false,
        duel: true,
        ranged: false,
        receding: false,
        blocking: false,
        tired: false,
        reeling: false,
        facing_away: false,
        special: false,
        foe_bounced: false,
        own_points: 100,
        tier: Tier::Ordinary,
        own_lock: false,
        own_ducked: false,
        own_ready: true,
        own_free: true,
        own_kicking: false,
        style: None,
        kata_cost: Some(25),
        lunge_cost: 25,
        knows_push: false,
        own_swinging: false,
        other_charging: false,
        deflects: false,
        shot_at: 0,
        bolt_incoming: false,
        health: 100.0,
        saber_move: 0,
        bounced: false,
        broken: false,
        parried: false,
        pain_until: 0,
    }
}

/// The way and the keys of `count` thinks, 100 ms apart, of a fresh pace
/// that sees `sight(look)` on each.
fn looks(sight: impl Fn(u64) -> Sight, count: u64) -> Vec<(Way, Keys)> {
    (looked(sight, count).into_iter())
        .map(|(way, _, keys)| (way, keys))
        .collect()
}

/// The same with the reaction of each think.
fn looked(sight: impl Fn(u64) -> Sight, count: u64) -> Vec<(Way, Reaction, Keys)> {
    let mut pace = Pacing::default();
    let id = SimId(1);
    let mut kicked = false;
    (0..count)
        .map(|look| {
            let now = 1000 + look * 100;
            let mut seen = sight(look);
            seen.own_kicking |= kicked;
            let sight = &seen;
            let mut dice = Dice::new(now, id);
            let clear = |_: i8, _: i8| true;
            let (way, reaction) = pace.decide(sight, now, &mut dice, &clear);
            let keys = Keys {
                forward: match way {
                    Way::Advance => 127,
                    Way::Retreat => -127,
                    Way::Hold => 0,
                },
                ..Keys::default()
            };
            let keys = pace.finish(keys, sight, reaction, now, &mut dice, &clear, 0.0, 1.0);
            kicked = keys.kick;
            (way, reaction, keys)
        })
        .collect()
}

/// A creature keeps a sidestep for half a second or longer: over 20 looks
/// its side key changes at most twice.
fn sidestep_is_kept() -> Result<String, String> {
    let looked = looks(|_| duel_sight(80.0), 20);
    let sides: Vec<i8> = looked.iter().map(|(_, keys)| keys.right.signum()).collect();
    let runs: Vec<usize> = sides
        .chunk_by(|a, b| a == b)
        .filter(|run| run[0] != 0)
        .map(<[i8]>::len)
        .collect();
    let kept = runs
        .split_last()
        .is_some_and(|(_, earlier)| earlier.iter().all(|&len| len >= 5));
    check(
        !runs.is_empty() && kept,
        format!("runs of a side key in 20 looks: {runs:?}, sides {sides:?}"),
    )
}

/// Whether a creature that sees a target 36 units beyond its reach, moving
/// with `velocity(look)` away from it along x, still walks on look 20 (2.1 s
/// in), and whether it walked on look 5.
fn walks_after_backing(velocity: impl Fn(u64) -> Vec3) -> (bool, bool) {
    let looked = looks(
        |look| {
            let mut sight = duel_sight(0.0);
            sight.gap = 36.0;
            sight.receding = moving_away(velocity(look), [1.0, 0.0, 0.0]);
            sight
        },
        21,
    );
    (looked[5].1.walk, looked[20].1.walk)
}

/// A target that backs away for longer than 1.5 seconds is run after, at any
/// speed and with a short pause; one that only crosses in front is not.
fn backing_is_run_after() -> Result<String, String> {
    type Case = (&'static str, fn(u64) -> Vec3, bool);
    let cases: [Case; 4] = [
        ("straight backer", |_| [60.0, 0.0, 0.0], false),
        ("strafer", |_| [0.0, 60.0, 0.0], true),
        (
            "back, pause 100 ms, back",
            |look| {
                if look == 8 {
                    [0.0; 3]
                } else {
                    [60.0, 0.0, 0.0]
                }
            },
            false,
        ),
        ("slow backer", |_| [6.0, 0.0, 0.0], false),
    ];
    let wrong: Vec<String> = cases
        .iter()
        .filter_map(|&(name, velocity, walks_at_end)| {
            let got = walks_after_backing(velocity);
            (got != (true, walks_at_end)).then(|| format!("{name}: walks at 5 and 20 {got:?}"))
        })
        .collect();
    check(
        wrong.is_empty(),
        format!("{} cases; wrong: {wrong:?}", cases.len()),
    )
}

/// A choice to advance, retreat or stand is kept for half a second: a
/// target that jumps from far away to inside the creature's reach changes
/// the way only on the sixth look.
fn choice_is_kept() -> Result<String, String> {
    let looked = looks(|look| duel_sight(if look < 5 { 400.0 } else { 0.0 }), 7);
    let ways: Vec<Way> = looked.iter().map(|(way, _)| *way).collect();
    check(
        ways[..5].iter().all(|way| *way == ways[0]) && ways[5] != ways[0],
        format!("the ways of 7 looks: {ways:?}"),
    )
}

/// A creature within a blade's reach of a melee target walks while it
/// swings.
fn swings_at_a_walk() -> Result<String, String> {
    let looked = looks(|_| duel_sight(80.0), 20);
    let swings: Vec<&Keys> = (looked.iter().map(|(_, keys)| keys))
        .filter(|keys| keys.attack)
        .collect();
    check(
        !swings.is_empty() && swings.iter().all(|keys| keys.walk),
        format!(
            "{} of {} swinging looks walk",
            swings.iter().filter(|keys| keys.walk).count(),
            swings.len()
        ),
    )
}

/// The pacing fields are saved and read back, and a pace saved before them
/// reads as zeros.
fn pacing_is_saved() -> Result<String, String> {
    let held = Pacing {
        receding_since: 4_100,
        receding_last: 4_400,
        last_look: 4_500,
        sidestep_until: 5_200,
        sidestep_side: -1,
        decided_until: 6_300,
        decided_way: 1,
        block_since: 7_000,
        lock_won_at: 7_500,
        locked: true,
        backing_off: true,
        rested: true,
        keys: Keys {
            up: -1,
            alt: true,
            kick: true,
            ..Keys::default()
        },
        kick_tries: 2,
        kick_last: 7_700,
        kick_wait: 7_800,
        charge: Charge::Run,
        charge_until: 8_000,
        charge_foe: 3,
        charge_lunge: true,
        again_at: 9_000,
        forced: Some(Tier::Skilled),
        tier: Tier::Boss,
        chain_limit: 3,
        rest_until: 9_500,
        swung: true,
        special: 2,
        special_since: 9_600,
        last_special: 5,
        special_after: 9_700,
        ..Pacing::default()
    };
    let mut out = Writer::default();
    held.encode(&mut out);
    let bytes = out.0;
    let again = Pacing::decode(&mut Reader::new("pace", &bytes)).map_err(|e| e.to_string())?;
    let old = bytes.len() - 121;
    let before = crate::since::snapshot::FEAT_ADVANCED_PACING.1 - 1;
    let versions = Versions {
        snapshot: before,
        ..Versions::THIS_BUILD
    };
    let mut read = Reader::new("pace", &bytes[..old]).with_versions(versions);
    let older = Pacing::decode(&mut read).map_err(|e| e.to_string())?;
    check(
        again == held && older == Pacing::default(),
        format!("read back {again:?}; from snapshot version {before} {older:?}"),
    )
}

/// A duelling sight of a target 80 units away (within reach), changed by `edit`.
fn seen(edit: impl Fn(&mut Sight)) -> Sight {
    let mut sight = duel_sight(80.0);
    edit(&mut sight);
    sight
}

/// What `react` makes of `sight` for a creature of `tier` with `points`
/// Stamina, resting or not, whose target has blocked for `block_ms` and which
/// won a lock `lock_ms` ago.
fn reaction(
    sight: &Sight,
    tier: Tier,
    points: i32,
    (rested, backing_off): (bool, bool),
    (block_ms, lock_ms): (u64, Option<u64>),
) -> Reaction {
    let tired = points <= 20;
    react(
        sight,
        tired,
        points,
        tier,
        rested,
        backing_off,
        block_ms,
        lock_ms,
    )
}

/// A creature with plenty of Stamina and nothing special to read.
fn calm(sight: &Sight, tier: Tier) -> Reaction {
    reaction(sight, tier, 100, (false, false), (0, None))
}

/// A tired target is pressed.
fn presses_a_tired_target() -> Result<String, String> {
    let tired = calm(&seen(|s| s.tired = true), Tier::Skilled);
    let fresh = calm(&seen(|_| {}), Tier::Skilled);
    check(
        tired == Reaction::Press && fresh == Reaction::None,
        format!("tired target {tired:?}, fresh target {fresh:?}"),
    )
}

/// A target that is staggered, knocked aside or has its guard broken is
/// swung at at once, by every tier.
fn swings_at_a_reeling_target() -> Result<String, String> {
    let got = [Tier::Ordinary, Tier::Skilled, Tier::Boss]
        .map(|tier| calm(&seen(|s| s.reeling = true), tier));
    check(
        got == [Reaction::SwingNow; 3],
        format!("a reeling target, by tier: {got:?}"),
    )
}

/// A swing that bounced off the block is answered at once by a Skilled
/// fighter or a boss, and not by an Ordinary one.
fn skilled_swings_out_of_a_parry() -> Result<String, String> {
    let got = [Tier::Ordinary, Tier::Skilled, Tier::Boss]
        .map(|tier| calm(&seen(|s| s.foe_bounced = true), tier) == Reaction::SwingNow);
    check(
        got == [false, true, true],
        format!("swings at once after a bounce, Ordinary, Skilled, Boss: {got:?}"),
    )
}

/// A won lock is pressed for 2 seconds, and in that time there is no
/// sidestep: a creature that was sidestepping holds its side key at zero for
/// the whole window.
fn presses_a_won_lock() -> Result<String, String> {
    let at = |ms| {
        reaction(
            &seen(|_| {}),
            Tier::Skilled,
            100,
            (false, false),
            (0, Some(ms)),
        )
    };
    let edge = (at(1999), at(2000));
    let looked = looked(
        |look| {
            seen(|s| {
                s.own_lock = look == 10;
                s.reeling = look == 11;
            })
        },
        31,
    );
    let sides: Vec<i8> = looked[12..].iter().map(|(.., keys)| keys.right).collect();
    let pressed = looked[12..]
        .iter()
        .all(|(_, reaction, _)| *reaction == Reaction::SwingNow);
    check(
        edge == (Reaction::SwingNow, Reaction::None)
            && pressed
            && sides.iter().all(|&side| side == 0),
        format!("at 1999 and 2000 ms {edge:?}; pressed {pressed}; side keys {sides:?}"),
    )
}

/// A target that swings during the won-lock window is guarded against: the
/// creature does not drop its guard for the whole window.
fn guards_during_a_won_lock() -> Result<String, String> {
    let looked = looked(
        |look| {
            seen(|s| {
                s.own_lock = look == 0;
                s.reeling = look == 1;
                s.swinging = look >= 2;
                s.gap = -20.0;
            })
        },
        6,
    );
    let swings: Vec<bool> = looked[2..].iter().map(|(.., keys)| keys.attack).collect();
    check(
        swings.iter().any(|attack| !attack),
        format!("attack key while the target swings in the window: {swings:?}"),
    )
}

/// The looks of a creature that has sidestepped for 1 second (a melee target
/// 80 units off), then sees `late(look)` for `count` looks: the way, reaction
/// and keys of the looks after the first 10.
fn after_sidestep(
    late: impl Fn(Tier) -> Sight,
    tier: Tier,
    count: u64,
) -> Vec<(Way, Reaction, Keys)> {
    let early = |look| {
        let mut sight = if look < 10 {
            duel_sight(80.0)
        } else {
            late(tier)
        };
        sight.tier = tier;
        sight
    };
    looked(early, 10 + count).split_off(10)
}

/// An Ordinary or Skilled fighter steps away from a kata 120 units off at a
/// run, and not from one 130 units off, and holds no side key while it does,
/// even after a sidestep. A boss does not step away.
fn steps_away_from_a_kata() -> Result<String, String> {
    let kata = |apart: f32| Sight {
        special: true,
        ..duel_sight(apart)
    };
    let got = [Tier::Ordinary, Tier::Skilled, Tier::Boss]
        .map(|tier| (calm(&kata(120.0), tier), calm(&kata(130.0), tier)));
    let want = [
        (Reaction::StepAway, Reaction::None),
        (Reaction::StepAway, Reaction::None),
        (Reaction::Guard, Reaction::None),
    ];
    let run = after_sidestep(|_| kata(120.0), Tier::Skilled, 5);
    let backs = run.iter().all(|(way, _, keys)| {
        *way == Way::Retreat && keys.forward < 0 && !keys.walk && keys.right == 0
    });
    check(
        got == want && backs,
        format!(
            "120 and 130 units by tier {got:?}; backs away at a run, no side key {backs}: {run:?}"
        ),
    )
}

/// A boss near a kata stands still with its guard up, and its other reads
/// still work: a kata that leaves its target reeling is swung at.
fn boss_guards_against_a_kata() -> Result<String, String> {
    let kata = |reeling| Sight {
        special: true,
        reeling,
        ..duel_sight(120.0)
    };
    let held = after_sidestep(|_| kata(false), Tier::Boss, 5);
    let stands = held.iter().all(|(way, reaction, keys)| {
        *way == Way::Hold
            && *reaction == Reaction::Guard
            && keys.forward == 0
            && keys.right == 0
            && !keys.attack
    });
    let swings = calm(&kata(true), Tier::Boss);
    check(
        stands && swings == Reaction::SwingNow,
        format!("a boss near a kata: {held:?}; reeling too: {swings:?}"),
    )
}

/// The moves a stagger, a knocked-aside blow and an empty block play count as
/// reeling, and a ready stance or a bounce does not.
fn reeling_moves() -> Result<String, String> {
    use Move::*;
    let reeling = [
        H1_T_, H1_TR, H1_TL, H1_BL, H1_B_, H1_BR, K1_T_, K1_BL, V1_BR, V1_T_,
    ];
    let steady = [READY, B1_BL, B1_T_, PARRY_UP];
    let wrong: Vec<Move> = (reeling.iter().filter(|m| !m.reeling()))
        .chain(steady.iter().filter(|m| m.reeling()))
        .copied()
        .collect();
    check(wrong.is_empty(), format!("moves read wrongly: {wrong:?}"))
}

/// A target with its back turned is swung at at once.
fn swings_at_a_turned_back() -> Result<String, String> {
    let got = calm(&seen(|s| s.facing_away = true), Tier::Ordinary);
    check(got == Reaction::SwingNow, format!("a turned back: {got:?}"))
}

/// A block held for 2 seconds is broken, and one held for less is not: seen
/// by a pace, the first think after the 2 seconds breaks it.
fn breaks_a_long_block() -> Result<String, String> {
    let at = |ms| {
        reaction(
            &seen(|s| s.blocking = true),
            Tier::Skilled,
            100,
            (false, false),
            (ms, None),
        )
    };
    let edge = (at(1999), at(2000));
    let looked = looked(|_| seen(|s| s.blocking = true), 22);
    let first = looked
        .iter()
        .position(|(_, reaction, _)| *reaction == Reaction::BreakBlock);
    check(
        edge == (Reaction::None, Reaction::BreakBlock) && first == Some(20),
        format!("at 1999 and 2000 ms {edge:?}; a pace breaks it on look {first:?} (of 22)"),
    )
}

/// The reactions of a creature of `tier` that sees `points(look)` Stamina in
/// itself on each of `count` looks.
fn tiring(tier: Tier, points: impl Fn(u64) -> i32, count: u64) -> Vec<Reaction> {
    let looks = looked(
        |look| {
            seen(|s| {
                s.tier = tier;
                s.own_points = points(look);
            })
        },
        count,
    );
    looks.into_iter().map(|(_, reaction, _)| reaction).collect()
}

/// A tired fighter backs off, and keeps backing off until it has 40 Stamina.
fn backs_off_when_tired() -> Result<String, String> {
    let stamina = [15, 25, 39, 40, 41];
    let got = tiring(Tier::Skilled, |look| stamina[look as usize], 5);
    let want = [
        Reaction::BackOff,
        Reaction::BackOff,
        Reaction::BackOff,
        Reaction::None,
        Reaction::None,
    ];
    check(got == want, format!("with {stamina:?} Stamina: {got:?}"))
}

/// A boss backs off once in a fight; a Skilled fighter every time.
fn boss_rests_once() -> Result<String, String> {
    let stamina = [15, 40, 15];
    let reactions = |tier| tiring(tier, |look| stamina[look as usize], 3);
    let (boss, skilled) = (reactions(Tier::Boss), reactions(Tier::Skilled));
    check(
        boss == [Reaction::BackOff, Reaction::None, Reaction::None]
            && skilled == [Reaction::BackOff, Reaction::None, Reaction::BackOff],
        format!("boss {boss:?}, skilled {skilled:?}"),
    )
}

/// A tired fighter against a target with a gun does not back off, and does not
/// start to.
fn tired_against_a_gun_does_not_back_off() -> Result<String, String> {
    let sight = seen(|s| {
        s.ranged = true;
        s.own_points = 10;
        s.tier = Tier::Skilled;
    });
    let mut pace = Pacing::default();
    let mut dice = Dice::new(1000, SimId(1));
    let (way, reaction) = pace.decide(&sight, 1000, &mut dice, &|_, _| true);
    check(
        reaction != Reaction::BackOff && !pace.backing_off,
        format!(
            "tired against a gun: {way:?} {reaction:?}, backing off {}",
            pace.backing_off
        ),
    )
}

/// A creature's tier comes from its sheet, a boss from its tag, and a forced
/// tier overrides both.
fn tier_is_found() -> Result<String, String> {
    let rows = [
        (tier(false, 0, false, None), Tier::Ordinary),
        (tier(true, 0, false, None), Tier::Skilled),
        (tier(false, 1, false, None), Tier::Skilled),
        (tier(true, 3, true, None), Tier::Boss),
        (tier(true, 3, true, Some(Tier::Ordinary)), Tier::Ordinary),
    ];
    check(
        rows.iter().all(|(got, want)| got == want),
        format!("tiers: {rows:?}"),
    )
}

/// Seat 0 holds block for `ticks` ticks in front of a hostile creature of
/// `kind` (a vibroblade or a lit lightsaber style), under `rules`. The tick
/// the creature first kicked, and seat 0's Stamina at the end.
fn block_held_before(rules: &str, kind: &str, ticks: u32) -> Result<(Option<usize>, i32), String> {
    let text = format!(
        "scenario probe\nrules {rules}\nseed 7\n\
         fighter 0 medium x=0 defence=2 tiers=2 fp=100 hp=400\n\
         creature {kind} x=45 yaw=180 defence=2 tiers=2 hp=400\n\
         begin\ntick 1\ntick 60\ntick {ticks} h0=block c0=0\n"
    );
    let ticks = play(&text)?;
    let kicked = ticks.iter().position(|tick| {
        tick.entities.iter().any(|entity| {
            entity.kind == Kind::Object(ObjectKind::Creature, 0)
                && entity
                    .player
                    .is_some_and(|ps| is_kick(ps.legs.anim) || is_kick(ps.torso.anim))
        })
    });
    let stamina = ticks
        .last()
        .and_then(|tick| tick.own(Seat(0)))
        .and_then(|entity| entity.fighter)
        .map(|f| i32::from(f.saber.duel.points))
        .ok_or("no fighter")?;
    Ok((kicked, stamina))
}

fn block_held_before_a_creature(rules: &str) -> Result<(Option<usize>, i32), String> {
    block_held_before(rules, "vibroblade", 400)
}

/// A creature with a vibroblade, or with a lit lightsaber that first has to
/// finish the swing it began, kicks a target that has held block for 2
/// seconds, and the kick leaves the target with more than 20 Stamina.
fn kick_breaks_a_long_block() -> Result<String, String> {
    let rows = [("vibroblade", 400, 4), ("medium", 800, 8)];
    let mut bad = Vec::new();
    for (kind, ticks, latest_second) in rows {
        let (kicked, stamina) = block_held_before("advanced", kind, ticks)?;
        let window = SETTLED + TICKS_PER_SECOND * 2..=SETTLED + TICKS_PER_SECOND * latest_second;
        if !(kicked.is_some_and(|tick| window.contains(&tick)) && !exchange::tired(stamina)) {
            bad.push(format!(
                "{kind}: first kick on tick {kicked:?} (asked for {window:?}), {stamina} Stamina"
            ));
        }
    }
    check(bad.is_empty(), format!("kicks that were not made: {bad:?}"))
}

/// Under Jedi Academy's rules the same creature starts no kick.
fn no_kick_under_ja() -> Result<String, String> {
    let (kicked, _) = block_held_before_a_creature("ja")?;
    check(
        kicked.is_none(),
        format!("under JA the creature kicked on tick {kicked:?}"),
    )
}

/// The keys of `count` thinks of a creature 45 units from a target that holds
/// block throughout, as `tweak(look, sight)` sets what the creature sees.
fn kick_looks_by(count: u64, tweak: impl Fn(u64, &mut Sight)) -> Vec<Keys> {
    let sight = |look| {
        let mut sight = duel_sight(45.0);
        sight.blocking = true;
        tweak(look, &mut sight);
        sight
    };
    looked(sight, count)
        .into_iter()
        .map(|(_, _, keys)| keys)
        .collect()
}

/// The same, but for the looks `released` holds, as `free(look)` says whether
/// the creature may start a kick.
fn kick_looks(count: u64, released: u64, free: impl Fn(u64) -> bool) -> Vec<Keys> {
    kick_looks_by(count, |look, sight| {
        sight.blocking = look != released;
        sight.own_free = free(look);
    })
}

/// A creature that cannot start a kick (floored, locked or off the ground)
/// when it sets out to break a block does not stand still: it swings within
/// 1 second, and it presses no kick while it cannot.
fn refused_kick_does_not_stall() -> Result<String, String> {
    let (from, until) = (20, 40);
    let keys = kick_looks(50, u64::MAX, |look| !(from..until).contains(&look));
    let kicked = keys[from as usize..until as usize].iter().any(|k| k.kick);
    let swung = keys[from as usize..until as usize]
        .iter()
        .position(|k| k.attack);
    check(
        !kicked && swung.is_some_and(|look| look <= 11),
        format!(
            "kicked while unable: {kicked}; first swing {swung:?} looks after the 2 seconds (asked for 11 or fewer)"
        ),
    )
}

/// A creature kicks twice at a block that is held, never on two thinks in a
/// row, and then swings; when the block is let go and held again it kicks
/// twice more.
fn kicks_are_bounded() -> Result<String, String> {
    let keys = kick_looks(100, 60, |_| true);
    let presses: Vec<usize> = (0..keys.len()).filter(|&i| keys[i].kick).collect();
    let apart = presses.windows(2).all(|w| w[1] > w[0] + 1);
    let swings = keys[30..60].iter().any(|k| k.attack);
    check(
        presses.len() == 4 && presses[..2].iter().all(|&i| i < 30) && apart && swings,
        format!(
            "kicks on looks {presses:?} (two before look 30, two after look 60, none on neighbours); swung before the release: {swings}"
        ),
    )
}

/// A creature whose swing is still under way when its target's block turns
/// long waits for the swing to end, and then kicks before it swings again.
fn swing_is_waited_out_before_a_kick() -> Result<String, String> {
    let busy_until: usize = 35;
    let keys = kick_looks_by(60, |look, sight| sight.busy = look < busy_until as u64);
    let first = |pick: fn(&Keys) -> bool| keys.iter().position(pick);
    let kick = first(|k| k.kick);
    let swing = keys[20..]
        .iter()
        .position(|k| k.attack)
        .map(|look| look + 20);
    check(
        kick.is_some_and(|look| look >= busy_until) && swing.is_none_or(|look| Some(look) > kick),
        format!(
            "busy until look {busy_until}: first kick on look {kick:?}, first swing on look {swing:?}"
        ),
    )
}

/// A kick key press that starts no kick (the creature is never seen
/// kicking) is not counted: it presses more than twice before it swings.
fn swallowed_kick_is_not_counted() -> Result<String, String> {
    let mut pace = Pacing::default();
    let id = SimId(1);
    let presses = (0..60u64)
        .filter(|look| {
            let now = 1000 + look * 100;
            let mut sight = duel_sight(45.0);
            sight.blocking = true;
            let sight = &sight;
            let mut dice = Dice::new(now, id);
            let clear = |_: i8, _: i8| true;
            let (_, reaction) = pace.decide(sight, now, &mut dice, &clear);
            let keys = pace.finish(
                Keys::default(),
                sight,
                reaction,
                now,
                &mut dice,
                &clear,
                0.0,
                1.0,
            );
            keys.kick
        })
        .count();
    check(
        presses > 2,
        format!("kick presses that started no kick: {presses} (more than 2 asked for)"),
    )
}

/// The charge's distances: from 4 to 8 meters against a melee target, from 4
/// to 12 meters against a blaster.
fn charge_distances() -> Result<String, String> {
    let rows = [
        (150.0, false, false),
        (157.0, false, true),
        (315.0, false, true),
        (320.0, false, false),
        (470.0, true, true),
        (475.0, true, false),
    ];
    let got = rows.map(|(gap, ranged, _)| may_charge(true, gap, ranged, 0, 0, false));
    let want = rows.map(|(.., may)| may);
    check(
        got == want,
        format!("may_charge for (gap, ranged) {rows:?}: {got:?}, the rules ask {want:?}"),
    )
}

/// A creature that is not Skilled, that waits for its next charge or whose
/// target is charged at by another does not charge.
fn charge_waits() -> Result<String, String> {
    let rows = [
        ("ready", true, 5_000, 5_000, false, true),
        ("waiting", true, 4_999, 5_000, false, false),
        ("another charges", true, 5_000, 5_000, true, false),
        ("not skilled", false, 5_000, 5_000, false, false),
    ];
    let got = rows.map(|(_, skilled, now, again, other, _)| {
        may_charge(skilled, 200.0, false, now, again, other)
    });
    let want = rows.map(|(.., may)| may);
    check(
        got == want,
        format!("may_charge for {rows:?}: {got:?}, the rules ask {want:?}"),
    )
}

/// The fast style with the Stamina ends a charge with the lunge, everyone
/// else with a swing at a run.
fn charge_endings() -> Result<String, String> {
    let rows = [
        (Some(Style::Fast), true, Ending::Lunge),
        (Some(Style::Fast), false, Ending::RunningSwing),
        (Some(Style::Medium), true, Ending::RunningSwing),
        (None, true, Ending::RunningSwing),
    ];
    let got = rows.map(|(style, enough, _)| ending(style, enough));
    let want = rows.map(|(.., end)| end);
    check(
        got == want,
        format!("ending for (style, enough Stamina) {rows:?}: {got:?}, the rules ask {want:?}"),
    )
}

/// The pace of a Skilled creature that sees `sight(look)` on `count` thinks,
/// 100 ms apart from 1000 ms on: its pace and its keys after each.
fn charging_looks(
    sight: impl Fn(u64) -> Sight,
    count: u64,
    way_clear: bool,
) -> Vec<(Pacing, Keys)> {
    let mut pace = Pacing::default();
    (0..count)
        .map(|look| {
            let now = 1000 + look * 100;
            let mut sight = sight(look);
            sight.tier = Tier::Skilled;
            let mut dice = Dice::new(now, SimId(1));
            let clear = |_: i8, _: i8| way_clear;
            let (way, reaction) = pace.decide(&sight, now, &mut dice, &clear);
            let forward = if way == Way::Advance { 127 } else { 0 };
            let keys = Keys {
                forward,
                ..Keys::default()
            };
            let keys = pace.finish(keys, &sight, reaction, now, &mut dice, &clear, 0.0, 1.0);
            (pace, keys)
        })
        .collect()
}

/// A charge stands still for 600 ms and then runs: seat 0 stands 250 units
/// from a Skilled creature, which first holds its ready pose (the marks on its
/// duel), and then moves.
fn windup_before_the_run() -> Result<String, String> {
    let ticks = charge_fight("medium", "tick 700")?;
    let at = first_windup(&ticks).ok_or("the creature never wound up a charge")?;
    let until = creature_duel(&ticks[at])?.windup_until;
    let started = ticks[at].time_ms;
    let held: Vec<f32> = ticks[at..]
        .iter()
        .filter(|tick| tick.time_ms >= started + 200 && tick.time_ms < until)
        .filter_map(creature_origin)
        .map(|origin| origin[1])
        .collect();
    let (low, high) = held.iter().fold((f32::MAX, f32::MIN), |(low, high), &x| {
        (low.min(x), high.max(x))
    });
    let later = ticks
        .iter()
        .find(|tick| tick.time_ms >= until + 400)
        .and_then(creature_origin)
        .map(|origin| origin[1]);
    check(
        until - started >= 580
            && until - started <= 700
            && !held.is_empty()
            && high - low < 2.0
            && later.is_some_and(|x| x < low - 20.0),
        format!(
            "wound up from {started} ms to {until} ms; y while it held the pose {low:.1} to {high:.1}; y 400 ms after it {later:?}"
        ),
    )
}

/// A tired blocker holding block cannot block the running swing that ends a
/// charge: its blade is struck, it is staggered and the swing bounces off. A
/// blocker with Stamina left is not staggered.
fn tired_cannot_block_a_charge() -> Result<String, String> {
    let outcome = |points: i32| -> Result<(bool, bool), String> {
        let ticks = charge_fight(
            "medium",
            &format!("duel 0 points={points}\ntick 700 h0=block c0=0"),
        )?;
        let moves = |tick: &Snapshot| {
            let move_of = |entity: Option<&crate::EntityState>| {
                entity.and_then(|e| e.fighter).map(|f| f.saber.current_move)
            };
            let creature = tick
                .entities
                .iter()
                .find(|e| e.kind == Kind::Object(ObjectKind::Creature, 0));
            (move_of(tick.own(Seat(0))), move_of(creature))
        };
        let charging = ticks
            .iter()
            .filter(|tick| creature_duel(tick).is_ok_and(|duel| duel.charging));
        let (staggered, bounced) = charging.fold((false, false), |(staggered, bounced), tick| {
            let (blocker, creature) = moves(tick);
            (
                staggered || blocker.is_some_and(Move::reeling),
                bounced || creature.is_some_and(Move::bounce),
            )
        });
        Ok((staggered, bounced))
    };
    let (tired, fresh) = (outcome(15)?, outcome(100)?);
    check(
        tired == (true, true) && !fresh.0,
        format!(
            "with 15 Stamina the blocker was staggered and the swing bounced during the charge: {tired:?}; with 100 Stamina {fresh:?}"
        ),
    )
}

/// The charge's marks clear when the ending attack is over, when its target
/// is taken away during the wind-up, and when its target is replaced.
fn charge_mark_clears() -> Result<String, String> {
    let ticks = charge_fight("medium", "tick 700")?;
    let marked = |tick: &Snapshot| {
        creature_duel(tick).is_ok_and(|duel| duel.charging || duel.windup_until > 0)
    };
    let charged = ticks
        .iter()
        .any(|tick| creature_duel(tick).is_ok_and(|d| d.charging));
    let after_ending = ticks.last().is_some_and(|tick| !marked(tick));
    let wound = first_windup(&ticks).ok_or("the creature never wound up a charge")?;
    let removed = charge_fight(
        "medium",
        &format!(
            "tick {}\nstat 0 dead 1\ntick 30",
            wound.saturating_sub(SETTLED) + 20
        ),
    )?;
    let after_removal = removed.last().is_some_and(|tick| !marked(tick));
    let replaced = charging_looks(
        |look| Sight {
            foe: SimId(if look < 3 { 2 } else { 3 }),
            ..duel_sight(250.0)
        },
        5,
        true,
    );
    let (before, now) = (replaced[2].0, replaced[4].0);
    check(
        charged
            && after_ending
            && after_removal
            && before.charge == Charge::Windup
            && now.charge == Charge::None
            && now.again_at > 1400,
        format!(
            "charged {charged}; marks clear after the ending {after_ending}, after its target was killed {after_removal}; replaced target: {:?} then {:?}",
            before.charge, now.charge
        ),
    )
}

/// A charge that does not reach its target (here `way_clear` says no, as it
/// does at a wall or a ledge: it stands still against it) ends 3 seconds
/// after its run began,
/// and its creature then waits 8 to 15 seconds before the next.
fn blocked_charge_gives_up() -> Result<String, String> {
    let looked = charging_looks(|_| duel_sight(250.0), 150, false);
    let run = looked
        .iter()
        .position(|(pace, _)| pace.charge == Charge::Run);
    let over = looked.iter().position(|(pace, _)| pace.again_at > 0);
    let again = looked
        .last()
        .map(|(pace, _)| pace.again_at)
        .unwrap_or_default();
    let restarted = looked
        .iter()
        .enumerate()
        .skip(over.unwrap_or(0))
        .any(|(look, (pace, _))| 1000 + 100 * (look as u64) < again && pace.charge != Charge::None);
    check(
        matches!((run, over), (Some(run), Some(over)) if over - run == 30)
            && (1000 + 100 * over.unwrap_or(0) as u64 + 8_000
                ..=1000 + 100 * over.unwrap_or(0) as u64 + 15_000)
                .contains(&again)
            && !restarted,
        format!(
            "the run began on look {run:?}, the charge ended on look {over:?}; it may charge again at {again} ms; restarted too early {restarted}"
        ),
    )
}

/// A charger that dies in its wind-up lets go of its mark: its marks clear
/// and another creature may charge at the same target.
fn dead_charger_does_not_block_others() -> Result<String, String> {
    let creatures = ["medium x=0 y=200", "medium x=100 y=200"];
    let ticks = charge_scene(&creatures, "tick 700")?;
    let first = (0..2u32)
        .filter_map(|n| {
            let at = ticks.iter().position(|tick| {
                creature_fighter(tick, n).is_ok_and(|f| f.saber.duel.windup_until > 0)
            })?;
            Some((at, n))
        })
        .min()
        .ok_or("no creature wound up a charge")?;
    let (at, charger) = first;
    let other = 1 - charger;
    let killed = charge_scene(
        &creatures,
        &format!(
            "tick {}\nstat c{charger} dead 1\ntick 700",
            at.saturating_sub(SETTLED) + 20
        ),
    )?;
    let marked = |tick: &Snapshot, n: u32| {
        creature_fighter(tick, n)
            .is_ok_and(|f| f.saber.duel.charging || f.saber.duel.windup_until > 0)
    };
    let corpse_clear = killed.last().is_some_and(|tick| !marked(tick, charger));
    let other_charged = killed[at + 20..].iter().any(|tick| marked(tick, other));
    check(
        corpse_clear && other_charged,
        format!(
            "creature {charger} wound up first and was killed 160 ms in: its marks clear {corpse_clear}; creature {other} charged afterwards {other_charged}"
        ),
    )
}

/// A charger that is stunned (as pain and confusion do) ends its charge at
/// once, plays no strike and waits before charging again.
fn stunned_charger_ends_its_charge() -> Result<String, String> {
    let ticks = charge_fight("medium", "tick 700")?;
    let run = ticks
        .iter()
        .position(|tick| creature_duel(tick).is_ok_and(|duel| duel.charging))
        .ok_or("the creature never charged")?;
    let stunned = charge_fight(
        "medium",
        &format!(
            "tick {}\nforce c0 stunned=1500\ntick 500",
            run.saturating_sub(SETTLED) + 5
        ),
    )?;
    let after = &stunned[run + 8..];
    let marked = after
        .iter()
        .any(|tick| creature_duel(tick).is_ok_and(|duel| duel.charging || duel.windup_until > 0));
    let struck = after.iter().any(|tick| {
        creature_fighter(tick, 0)
            .is_ok_and(|f| f.saber.current_move.attack() || f.saber.current_move.start())
    });
    let waits = stunned
        .last()
        .and_then(|tick| creature_fighter(tick, 0).ok())
        .is_some_and(|f| f.mind.pace.again_at > stunned[run + 5].time_ms);
    check(
        !marked && !struck && waits,
        format!("after the stun: marks on {marked}, struck {struck}, wait set {waits}"),
    )
}

/// Of two creatures in charge distance of one target, only one winds up or
/// charges at a time.
fn one_charge_at_a_time() -> Result<String, String> {
    let ticks = charge_scene(&["medium x=0 y=200", "medium x=100 y=200"], "tick 900")?;
    let marks = |tick: &Snapshot| {
        (0..2)
            .filter(|&n| {
                tick.entities
                    .iter()
                    .find(|e| e.kind == Kind::Object(ObjectKind::Creature, n))
                    .and_then(|e| e.fighter)
                    .is_some_and(|f| f.saber.duel.charging || f.saber.duel.windup_until > 0)
            })
            .count()
    };
    let counts: Vec<usize> = ticks.iter().map(marks).collect();
    let most = counts.iter().copied().max().unwrap_or_default();
    check(
        most == 1,
        format!("the most creatures winding up or charging at once: {most}"),
    )
}

/// A charge of a creature in the fast style ends with the lunge, which
/// hits the target; the marks are clear once it is over.
fn fast_charge_ends_with_a_lunge() -> Result<String, String> {
    let ticks = charge_fight("fast", "saber c0 style=fast\ntick 700")?;
    let lunged = ticks.iter().any(|tick| {
        tick.entities
            .iter()
            .find(|entity| entity.kind == Kind::Object(ObjectKind::Creature, 0))
            .and_then(|entity| entity.fighter)
            .is_some_and(|fighter| fighter.saber.current_move == Move::A_LUNGE)
    });
    let hp = |tick: &Snapshot| tick.own(Seat(0)).and_then(|e| e.fighter).map(|f| f.hp);
    let hit = ticks.last().and_then(hp) < ticks.first().and_then(hp);
    let clear = ticks
        .last()
        .is_some_and(|tick| creature_duel(tick).is_ok_and(|d| !d.charging && d.windup_until == 0));
    check(
        lunged && hit && clear,
        format!(
            "the fast creature lunged: {lunged}; hit its target: {hit}; marks clear at the end: {clear}"
        ),
    )
}

/// The ticks of seat 0 standing at the origin with a creature at each of
/// `creatures` (a kind and a place, facing seat 0) under the Advanced rules:
/// the settling, then `then`.
fn charge_scene(creatures: &[&str], then: &str) -> Result<Vec<Snapshot>, String> {
    let creatures: String = creatures
        .iter()
        .map(|creature| format!("creature {creature} yaw=270 defence=2 tiers=2 hp=400\n"))
        .collect();
    play(&format!(
        "scenario probe\nrules advanced\nseed 7\n\
         fighter 0 medium x=0 defence=2 tiers=2 fp=100 hp=400\n\
         {creatures}begin\ntick 1\ntick 60\n{then}\n"
    ))
}

/// The ticks of a creature holding `kind` 200 units from seat 0: the
/// settling, then `then`.
fn charge_fight(kind: &str, then: &str) -> Result<Vec<Snapshot>, String> {
    charge_scene(&[&format!("{kind} x=0 y=200")], then)
}

/// The fighter of creature `n` on a tick.
fn creature_fighter(tick: &Snapshot, n: u32) -> Result<Fighter, String> {
    tick.entities
        .iter()
        .find(|entity| entity.kind == Kind::Object(ObjectKind::Creature, n))
        .and_then(|entity| entity.fighter)
        .ok_or_else(|| format!("no creature {n}"))
}

/// The first creature's duel on a tick.
fn creature_duel(tick: &Snapshot) -> Result<Duel, String> {
    creature_fighter(tick, 0).map(|fighter| fighter.saber.duel)
}

/// Where the creature stands on a tick.
fn creature_origin(tick: &Snapshot) -> Option<Vec3> {
    tick.entities
        .iter()
        .find(|entity| entity.kind == Kind::Object(ObjectKind::Creature, 0))
        .and_then(|entity| entity.player)
        .map(|ps| ps.origin)
}

/// The first tick the creature winds up a charge.
fn first_windup(ticks: &[Snapshot]) -> Option<usize> {
    ticks
        .iter()
        .position(|tick| creature_duel(tick).is_ok_and(|duel| duel.windup_until > 0))
}

/// What `approach_gun` makes of a fighter (`deflects`, `tired`) that may or may
/// not charge (`may`), `gap` units from a target with a blaster.
fn approach_is(
    (deflects, tired, gap, may): (bool, bool, f32, bool),
    want: Approach,
) -> Result<String, String> {
    let got = approach_gun(deflects, tired, gap, may);
    check(
        got == want,
        format!(
            "deflects {deflects}, tired {tired}, {gap} units, may charge {may}: {got:?}, the rules ask {want:?}"
        ),
    )
}

/// A creature stands while a bolt is on its way and for 400 ms after the last
/// shot, and walks on at 400 ms.
fn holds_while_bolts_come() -> Result<String, String> {
    let rows = [(true, 5_000, true), (false, 399, true), (false, 400, false)];
    let got = rows.map(|(incoming, since, _)| holds_for_bolts(incoming, since));
    let want = rows.map(|(.., hold)| hold);
    check(
        got == want,
        format!(
            "holds_for_bolts for (bolt in flight, ms since the shot) {rows:?}: {got:?}, the rules ask {want:?}"
        ),
    )
}

/// A bolt is headed at a body when it flies toward it along a line that passes
/// within the lane, seen from above.
fn bolt_is_headed_at() -> Result<String, String> {
    let rows = [
        (
            "straight at it",
            [0.0, 0.0, 40.0],
            [0.0, 1000.0, 0.0],
            500,
            [0.0, 800.0, 0.0],
            true,
        ),
        (
            "passed it",
            [0.0, 0.0, 40.0],
            [0.0, 1000.0, 0.0],
            1_000,
            [0.0, 800.0, 0.0],
            false,
        ),
        (
            "wide of it",
            [0.0, 0.0, 40.0],
            [0.0, 1000.0, 0.0],
            100,
            [100.0, 800.0, 0.0],
            false,
        ),
        (
            "fired away",
            [0.0, 0.0, 40.0],
            [0.0, -1000.0, 0.0],
            100,
            [0.0, 800.0, 0.0],
            false,
        ),
    ];
    let got = rows.map(|(_, base, velocity, ms, at, _)| bolt_incoming(base, velocity, 0, ms, at));
    let want = rows.map(|(.., headed)| headed);
    check(
        got == want,
        format!("bolt_incoming {rows:?}: {got:?}, the rules ask {want:?}"),
    )
}

/// A creature that deflects and walks at a shooter stands from each shot
/// until 400 ms after it and while its bolt is on the way, walks forward in the
/// pauses, and begins its charge from 8 meters: a shooter fires every 2
/// seconds from 900 units away, and each bolt is on its way for 300 ms.
fn advances_between_shots() -> Result<String, String> {
    let mut pace = Pacing::default();
    let mut gap = 900.0;
    let mut moved_in_a_hold = false;
    let mut walked = 0;
    let mut charged_from = None;
    for look in 0..200u64 {
        let now = 1000 + look * 100;
        let since_shot = (now + 500) % 2000;
        let in_flight = since_shot < 300;
        let held = holds_for_bolts(in_flight, since_shot);
        let mut sight = duel_sight(gap);
        sight.ranged = true;
        sight.deflects = true;
        sight.tier = Tier::Skilled;
        sight.bolt_incoming = in_flight;
        sight.shot_at = now - since_shot;
        let mut dice = Dice::new(now, SimId(1));
        let clear = |_: i8, _: i8| true;
        let (way, reaction) = pace.decide(&sight, now, &mut dice, &clear);
        if pace.charge != Charge::None {
            charged_from = Some(gap);
            break;
        }
        let forward = if way == Way::Advance { 127 } else { 0 };
        let keys = Keys {
            forward,
            ..Keys::default()
        };
        let keys = pace.finish(keys, &sight, reaction, now, &mut dice, &clear, 0.0, 1.0);
        moved_in_a_hold |= held && keys.forward != 0;
        if keys.forward != 0 && keys.walk {
            walked += 1;
            gap -= 10.0;
        }
    }
    check(
        !moved_in_a_hold && walked > 20 && charged_from.is_some_and(|gap| gap <= 320.0),
        format!(
            "moved while holding: {moved_in_a_hold}; looks walked {walked}; began its charge at {charged_from:?} units"
        ),
    )
}

/// A creature that deflects and walks at a shooter, with no shot or bolt about,
/// walks forward straight (no side key), with its guard up.
fn guarded_walk_keys() -> Result<String, String> {
    let sight = seen(|s| {
        *s = duel_sight(600.0);
        s.ranged = true;
        s.deflects = true;
        s.tier = Tier::Skilled;
    });
    let mut pace = Pacing::default();
    let mut dice = Dice::new(1000, SimId(1));
    let clear = |_: i8, _: i8| true;
    let (way, reaction) = pace.decide(&sight, 1000, &mut dice, &clear);
    let keys = Keys {
        forward: 127,
        ..Keys::default()
    };
    let keys = pace.finish(keys, &sight, reaction, 1000, &mut dice, &clear, 0.0, 1.0);
    check(
        way == Way::Advance && keys.forward > 0 && keys.walk && pace.guard_until > 1000,
        format!("{way:?} {keys:?}, guard up until {}", pace.guard_until),
    )
}

/// Only a shot aimed at a creature makes it stand: a shooter aiming along a line
/// that passes within the lane of it counts, one aiming elsewhere does not.
fn aim_is_at_it() -> Result<String, String> {
    let rows = [
        ("straight at it", [0.0, 1.0, 0.0], true),
        ("at an ally 400 units aside", [0.7, 0.7, 0.0], false),
        ("away from it", [0.0, -1.0, 0.0], false),
    ];
    let got = rows.map(|(_, forward, _)| aimed_at([0.0, 0.0, 0.0], forward, [0.0, 800.0, 0.0]));
    let want = rows.map(|(.., aimed)| aimed);
    check(
        got == want,
        format!("aimed_at {rows:?}: {got:?}, the rules ask {want:?}"),
    )
}

/// The ticks of a rifle creature at (0, 600) firing at seat 0, a medium-style
/// fighter at the origin, under `rules`, after the settling.
fn gun_scene(rules: &str, ticks: usize) -> Result<Vec<Snapshot>, String> {
    play(&format!(
        "scenario probe\nrules {rules}\nseed 7\n\
         fighter 0 medium x=0 defence=2 tiers=2 fp=100 hp=4000\n\
         creature rifle x=0 y=600 yaw=270 hp=400\n\
         begin\ntick 1\ntick 60\ntick {ticks}\n"
    ))
}

/// A creature that shoots stamps the time of its shot under the Advanced rules
/// and not under JA's, and a bolt of its is headed at its target while it
/// flies and no longer after it has hit; the same bolt, if nothing could turn it,
/// is not.
fn creature_shots_are_seen() -> Result<String, String> {
    let fired_at = |rules: &str| -> Result<u64, String> {
        let ticks = gun_scene(rules, 400)?;
        let last = ticks.last().ok_or("no ticks")?;
        creature_fighter(last, 0).map(|fighter| fighter.shooting.fired_at)
    };
    let (advanced, ja) = (fired_at("advanced")?, fired_at("ja")?);
    let ticks = gun_scene("advanced", 400)?;
    let creature = ticks
        .iter()
        .flat_map(|tick| tick.entities.iter())
        .find(|entity| entity.kind == Kind::Object(ObjectKind::Creature, 0))
        .map(|entity| entity.id)
        .ok_or("no creature")?;
    let headed: Vec<bool> = ticks
        .iter()
        .filter_map(|tick| {
            let at = tick.own(Seat(0)).and_then(|entity| entity.player)?.origin;
            let missiles = tick
                .entities
                .iter()
                .filter_map(|entity| entity.missile.as_ref());
            Some(bolt_headed_at(missiles, creature, tick.time_ms, at))
        })
        .collect();
    let first = headed.iter().position(|&headed| headed);
    let undeflectable = ticks.iter().any(|tick| {
        let at = tick
            .own(Seat(0))
            .and_then(|entity| entity.player)
            .map(|ps| ps.origin);
        let missiles: Vec<Missile> = tick
            .entities
            .iter()
            .filter_map(|entity| entity.missile)
            .map(|missile| Missile {
                blockable: false,
                ..missile
            })
            .collect();
        at.is_some_and(|at| bolt_headed_at(&missiles, creature, tick.time_ms, at))
    });
    let after = first.is_some_and(|first| headed[first..].iter().any(|&headed| !headed));
    check(
        advanced > 0 && ja == 0 && first.is_some() && after && !undeflectable,
        format!(
            "fired_at: Advanced {advanced}, JA {ja}; a bolt headed at seat 0 from tick {first:?}, and later none: {after}; headed if undeflectable: {undeflectable}"
        ),
    )
}

/// The tiers, in order.
const TIERS: [Tier; 3] = [Tier::Ordinary, Tier::Skilled, Tier::Boss];

/// The pace and the keys after each of `count` thinks, 100 ms apart, of a
/// fresh pace that sees `sight(look)`. When `swings` its own swing is under way
/// for 4 thinks after it presses attack; else no swing ever begins.
fn drive(count: u64, swings: bool, sight: impl Fn(u64) -> Sight) -> Vec<(Pacing, Keys)> {
    let mut pace = Pacing::default();
    let mut swinging_until = 0;
    (0..count)
        .map(|look| {
            let now = 1000 + look * 100;
            let mut seen = sight(look);
            seen.own_swinging = swings && look < swinging_until;
            seen.own_ducked = pace.keys.up < 0;
            let mut dice = Dice::new(now, SimId(1));
            let clear = |_: i8, _: i8| true;
            let (way, reaction) = pace.decide(&seen, now, &mut dice, &clear);
            let keys = Keys {
                forward: match way {
                    Way::Advance => 127,
                    Way::Retreat => -127,
                    Way::Hold => 0,
                },
                ..Keys::default()
            };
            let keys = pace.finish(keys, &seen, reaction, now, &mut dice, &clear, 0.0, 1.0);
            if swings && !seen.own_swinging && keys.attack {
                swinging_until = look + 4;
            }
            (pace, keys)
        })
        .collect()
}

/// A creature of `tier` and `style` with `points` Stamina, with a target 60
/// units away.
fn tiered(tier: Tier, style: Option<Style>, points: i32) -> Sight {
    let mut sight = duel_sight(60.0);
    sight.tier = tier;
    sight.style = style;
    sight.own_points = points;
    sight
}

/// The special moves a drive pressed the keys of, in the order it began them.
fn begun(run: &[(Pacing, Keys)]) -> Vec<Special> {
    let before = std::iter::once(0).chain(run.iter().map(|(pace, _)| pace.special));
    before
        .zip(run.iter().map(|(pace, _)| pace.special))
        .filter(|&(was, now)| was == 0 && now != 0)
        .filter_map(|(_, now)| Special::of_mark(now))
        .collect()
}

/// A tag is a boss's by the list, in any case, and a party member is never
/// one.
fn companion_is_never_a_boss() -> Result<String, String> {
    let rows = [
        (found_tier(true, 0, ("darthsion", false), None), Tier::Boss),
        (found_tier(true, 0, ("DarthSion", false), None), Tier::Boss),
        (
            found_tier(true, 0, ("darthsion", true), None),
            Tier::Skilled,
        ),
        (found_tier(false, 0, ("kreia", true), None), Tier::Ordinary),
        (
            found_tier(true, 0, ("g_darkjedi01", false), None),
            Tier::Skilled,
        ),
    ];
    check(
        rows.iter().all(|(got, want)| got == want) && !is_boss_tag("g_darkjedi01"),
        format!("tiers (tag, in the party): {rows:?}"),
    )
}

/// An Ordinary creature starts no charge and no special move over a minute
/// of fighting, of whatever style, with the target near and far.
fn ordinary_stays_plain() -> Result<String, String> {
    let run = drive(600, true, |look| {
        let apart = if look % 40 < 20 { 60.0 } else { 200.0 };
        let mut sight = tiered(Tier::Ordinary, Some(Style::Fast), 100);
        sight.gap = apart - TIP;
        sight
    });
    let charged = run.iter().any(|(pace, _)| pace.charge != Charge::None);
    let special = run
        .iter()
        .any(|(pace, keys)| pace.special != 0 || keys.alt || keys.up != 0);
    let swings = run.iter().filter(|(_, keys)| keys.attack).count();
    check(
        !charged && !special && swings > 5,
        format!(
            "charged {charged}, pressed a special move {special}, looks pressing attack {swings}"
        ),
    )
}

/// A boss starts the special moves of its style, and a style names them.
fn boss_uses_its_style() -> Result<String, String> {
    let has = |style: Style, wanted: &[Special]| {
        wanted
            .iter()
            .all(|special| specials(Some(style)).contains(special))
    };
    let table = has(Style::Fast, &[Special::Lunge])
        && has(Style::Medium, &[Special::FlipSlash, Special::Leap])
        && has(Style::Staff, &[Special::JumpAttack, Special::Spin])
        && specials(None).is_empty();
    let styles = [
        Style::Fast,
        Style::Medium,
        Style::Strong,
        Style::Dual,
        Style::Staff,
    ];
    let seen: Vec<(Style, Vec<Special>)> = styles
        .into_iter()
        .map(|style| {
            let run = drive(600, true, |_| tiered(Tier::Boss, Some(style), 100));
            (style, begun(&run))
        })
        .collect();
    let own = seen
        .iter()
        .all(|(style, moves)| moves.iter().all(|m| specials(Some(*style)).contains(m)));
    let many = seen.iter().filter(|(_, moves)| moves.len() >= 4).count() >= 3;
    check(
        table && own && many,
        format!(
            "style table {table}; every move its style's {own}; counts {:?}",
            seen.iter().map(|(s, m)| (*s, m.len())).collect::<Vec<_>>()
        ),
    )
}

/// A boss that cannot start a special move (too little Stamina, no style, a
/// kata its style lacks) swings plainly on the same look and presses no special
/// keys.
fn special_falls_back() -> Result<String, String> {
    let cases: [(&str, Sight); 3] = [
        ("50 Stamina", tiered(Tier::Boss, Some(Style::Medium), 50)),
        ("no style", tiered(Tier::Boss, None, 100)),
        ("a style with no kata and no Stamina for the rest", {
            let mut sight = tiered(Tier::Boss, Some(Style::Dual), 55);
            sight.kata_cost = None;
            sight
        }),
    ];
    let got: Vec<(&str, bool, bool)> = cases
        .iter()
        .map(|(name, sight)| {
            let run = drive(300, true, |_| *sight);
            let plain = run
                .iter()
                .any(|(_, keys)| keys.attack && keys.up == 0 && !keys.alt);
            let special = run
                .iter()
                .any(|(pace, keys)| pace.special != 0 || keys.alt || keys.up != 0);
            (*name, plain, special)
        })
        .collect();
    check(
        got.iter().all(|&(_, plain, special)| plain && !special),
        format!("(case, swung plainly, pressed special keys): {got:?}"),
    )
}

/// A special move the sim never starts (its swing does not begin) is given up
/// within 8 thinks, and the creature then swings plainly: it never stands
/// pressing keys.
fn refused_special_gives_up() -> Result<String, String> {
    let run = drive(300, false, |_| tiered(Tier::Boss, Some(Style::Medium), 100));
    let pressed = |keys: &Keys| keys.alt || keys.up != 0;
    let longest = run
        .chunk_by(|a, b| pressed(&a.1) == pressed(&b.1))
        .filter(|chunk| pressed(&chunk[0].1))
        .map(<[_]>::len)
        .max()
        .unwrap_or(0);
    let tried = !begun(&run).is_empty();
    let plain = run
        .iter()
        .filter(|(_, keys)| keys.attack && !pressed(keys))
        .count();
    check(
        tried && longest <= 8 && plain > 20,
        format!(
            "tried a special move {tried}; longest run of its keys {longest} thinks (8 at most); plain swings pressed {plain}"
        ),
    )
}

/// A boss never starts the same special move twice in a row when its style has
/// more than one.
fn no_special_twice() -> Result<String, String> {
    let run = drive(1200, true, |_| tiered(Tier::Boss, Some(Style::Staff), 100));
    let moves = begun(&run);
    let twice = moves.windows(2).any(|pair| pair[0] == pair[1]);
    check(
        moves.len() >= 6 && !twice,
        format!(
            "{} special moves begun, the same one twice in a row: {twice}",
            moves.len()
        ),
    )
}

/// What `chain_limit` asks of each tier, and the longest chain of swings a
/// creature of each tier makes against a tired target.
fn chain_limits() -> Result<String, String> {
    let pressed = TIERS.map(|tier| chain_limit(tier, true));
    let plain = TIERS.map(|tier| chain_limit(tier, false));
    let longest = TIERS.map(longest_chain);
    let longest: Vec<u8> = longest.into_iter().collect::<Result<_, _>>()?;
    check(
        pressed == [2, 3, 5] && plain == [2, 2, 2] && longest[..] == pressed[..],
        format!(
            "limits against a tired target {pressed:?}, otherwise {plain:?}; longest chains a staff fighter of each tier made against a tired target {longest:?}"
        ),
    )
}

/// The longest chain of swings that a creature of `tier` with a staff makes
/// against a tired target. A chain ends where a swing ends at the bottom, so
/// a chain seldom runs long by itself: the scenario puts the creature
/// 2 swings short of its tier's limit, in a swing that ends at its side,
/// 12 times in 40 seconds, and the chain it then makes runs to the limit and
/// no further.
fn longest_chain(tier: Tier) -> Result<u8, String> {
    let name = format!("{tier:?}").to_lowercase();
    let start = chain_limit(tier, true) - 2;
    let tired = "duel 0 points=5\ntick 100\n".repeat(3);
    let attempt = format!("duel 0 points=5\nsaber c0 move=A_L2R chain={start}\ntick 100\n{tired}");
    let ticks = play(&format!(
        "scenario probe\nrules advanced\nseed 7\n\
         fighter 0 medium x=0 defence=2 tiers=2 fp=100 hp=40000\n\
         creature medium x=45 yaw=180 defence=2 tiers=2 hp=400\n\
         begin\ntick 1 b0=attack\ntick 60\nsaber c0 style=staff\nduel c0 tier={name}\n{}",
        attempt.repeat(12)
    ))?;
    Ok(ticks
        .iter()
        .filter_map(|tick| creature_fighter(tick, 0).ok())
        .map(|fighter| fighter.saber.chain)
        .max()
        .unwrap_or(0))
}

/// A creature in its rest between attacks swings at once at a reeling target
/// and not at a target that stands.
fn reads_beat_the_rest() -> Result<String, String> {
    let resting = |reeling: bool| {
        let mut pace = Pacing {
            rest_until: 10_000,
            ..Pacing::default()
        };
        let mut sight = tiered(Tier::Skilled, None, 100);
        sight.reeling = reeling;
        let mut dice = Dice::new(1000, SimId(1));
        let clear = |_: i8, _: i8| true;
        let (_, reaction) = pace.decide(&sight, 1000, &mut dice, &clear);
        pace.finish(
            Keys::default(),
            &sight,
            reaction,
            1000,
            &mut dice,
            &clear,
            0.0,
            1.0,
        )
        .attack
    };
    let (reeling, standing) = (resting(true), resting(false));
    check(
        reeling && !standing,
        format!(
            "swung in its rest at a reeling target {reeling}, at a target that stands {standing}"
        ),
    )
}

/// A creature rests between attacks for its tier's time: the thinks between
/// the end of one swing and the start of the next.
fn rests_between_attacks() -> Result<String, String> {
    let bounds = [(1500, 3000), (1000, 2000), (500, 1500)];
    let got: Vec<(Tier, u64, u64)> = TIERS
        .into_iter()
        .map(|tier| {
            let run = drive(600, true, |_| tiered(tier, None, 100));
            let presses: Vec<u64> = (0..run.len() as u64)
                .filter(|&look| {
                    run[look as usize].1.attack
                        && look
                            .checked_sub(1)
                            .is_none_or(|before| !run[before as usize].1.attack)
                })
                .collect();
            let gaps: Vec<u64> = presses
                .windows(2)
                .map(|pair| (pair[1] - pair[0]) * 100 - 400)
                .collect();
            (
                tier,
                gaps.iter().copied().min().unwrap_or(0),
                gaps.iter().copied().max().unwrap_or(0),
            )
        })
        .collect();
    check(
        got.iter()
            .zip(bounds)
            .all(|(&(_, least, most), (low, high))| least + 100 >= low && most <= high + 200),
        format!("(tier, least rest, most rest) in ms: {got:?}; asked {bounds:?} (100 ms of slack)"),
    )
}

/// The guard delay of each tier over 200 draws.
fn guard_delay_by_tier() -> Result<String, String> {
    let mut rng = SimRng::new(7, 1);
    let draws: Vec<Vec<u64>> = TIERS
        .map(|tier| (0..200).map(|_| guard_delay_ms(tier, &mut rng)).collect())
        .into();
    let within =
        |at: usize, low: u64, high: u64| draws[at].iter().all(|ms| (low..=high).contains(ms));
    let both = [100, 200].iter().all(|ms| draws[2].contains(ms));
    check(
        within(0, 200, 600)
            && within(1, 200, 400)
            && draws[2].iter().all(|&ms| ms == 200 || ms == 100)
            && both,
        format!(
            "least and most by tier: {:?}; a boss drew both 100 and 200: {both}",
            draws
                .iter()
                .map(|d| (d.iter().min(), d.iter().max()))
                .collect::<Vec<_>>()
        ),
    )
}

/// The lockout after a hit by tier, and a swing that reaches a creature 300 ms
/// after it was hit.
fn hit_lock_by_tier() -> Result<String, String> {
    let locks = TIERS.map(hit_lock_ms);
    let down = TIERS.map(|tier| guard_is_down(tier, Some(300), Some((500, 200))));
    check(
        locks == [400, 200, 100] && down == [true, false, false],
        format!("lockouts {locks:?}; guard down 300 ms after a hit: {down:?}"),
    )
}

/// How many of 20 swings that reach a creature's guard 350 ms after they
/// began find it down, for a creature of `tier` (one guard delay each).
fn swings_getting_past(tier: Tier) -> usize {
    let mut rng = SimRng::new(11, 1);
    (0..20)
        .filter(|_| guard_is_down(tier, None, Some((350, guard_delay_ms(tier, &mut rng)))))
        .count()
}

/// A swing that reaches the guard 350 ms after it began gets past an Ordinary
/// creature's guard more often than past a Skilled one's, and never past a
/// boss's.
fn second_swing_beats_ordinary() -> Result<String, String> {
    let past = TIERS.map(swings_getting_past);
    check(
        past[0] > past[1] && past[1] > past[2] && past[2] == 0,
        format!("of 20 swings, past an Ordinary, a Skilled and a Boss guard: {past:?}"),
    )
}

/// The ticks a creature takes to draw its saber.
const DRAW_TICKS: u32 = 200;

/// The moves a creature of `tier` and `style` plays in `seconds` against a
/// target with plenty of health, with the lines `then` run after it has drawn
/// its saber.
fn creature_moves(
    tier: Tier,
    style: &str,
    seconds: usize,
    then: &str,
) -> Result<Vec<Move>, String> {
    let name = format!("{tier:?}").to_lowercase();
    let ticks = play(&format!(
        "scenario probe\nrules advanced\nseed 7\n\
         fighter 0 medium x=0 defence=2 tiers=2 fp=100 hp=40000\n\
         creature {style} x=45 yaw=180 defence=2 tiers=2 hp=400\n\
         sheet c0 level=12\n\
         begin\ntick 1 b0=attack\ntick 60\nsaber c0 style={style}\n\
         duel c0 tier={name}\ntick {DRAW_TICKS}\n{then}tick {}\n",
        seconds * 125
    ))?;
    let mut moves: Vec<Move> = ticks
        .iter()
        .filter_map(|tick| creature_fighter(tick, 0).ok())
        .map(|fighter| fighter.saber.current_move)
        .collect();
    moves.dedup();
    Ok(moves)
}

/// A special move a boss of a saber style is made to try, by the keys the
/// mind presses for it, and what plays: a style, the move's name in a
/// scenario, and the move that must play.
const STYLE_SPECIALS: [(&str, &str, Move); 13] = [
    ("fast", "lunge", Move::A_LUNGE),
    ("fast", "kata", Move::A1_SPECIAL),
    ("medium", "kata", Move::A2_SPECIAL),
    ("medium", "flip_slash", Move::A_FLIP_SLASH),
    ("medium", "leap", Move::A_FLIP_SLASH),
    ("strong", "kata", Move::A3_SPECIAL),
    ("strong", "leap", Move::A_JUMP_T__B_),
    ("dual", "kata", Move::DUAL_SPIN_PROTECT),
    ("dual", "jump_attack", Move::JUMPATTACK_DUAL),
    ("dual", "spin", Move::SPINATTACK_DUAL),
    ("staff", "jump_attack", Move::JUMPATTACK_STAFF_RIGHT),
    ("staff", "spin", Move::SPINATTACK),
    ("staff", "kata", Move::STAFF_SOULCAL),
];

/// A creature that is mid-swing gives up the keys of a special move at once,
/// so the scenario asks for it again every `NUDGE_TICKS` ticks, `NUDGES`
/// times, and the creature starts it in a pause between its chains.
const NUDGE_TICKS: u32 = 20;
const NUDGES: usize = 40;

/// A boss of each saber style that presses the keys of each special move of
/// its style plays that move; an Ordinary creature plays none, and a boss
/// left to itself starts a special move.
fn boss_starts_specials_in_the_world() -> Result<String, String> {
    let mut missing = Vec::new();
    for (style, special, wanted) in STYLE_SPECIALS {
        let nudges = format!("duel c0 points=70 rest=1500 special={special}\ntick {NUDGE_TICKS}\n")
            .repeat(NUDGES);
        let played = creature_moves(Tier::Boss, style, 1, &nudges)?;
        if !played.contains(&wanted) {
            missing.push(format!("{style} {special}: {wanted:?}"));
        }
    }
    let alone = creature_moves(Tier::Boss, "fast", 60, "")?;
    let ordinary = creature_moves(Tier::Ordinary, "fast", 60, "")?;
    let started = |moves: &[Move]| moves.iter().filter(|mv| mv.special()).count();
    check(
        missing.is_empty() && started(&alone) > 0 && started(&ordinary) == 0,
        format!(
            "moves that did not play when their keys were pressed: {missing:?}; a boss left alone started {} special moves, an Ordinary creature {}",
            started(&alone),
            started(&ordinary)
        ),
    )
}

/// The blows `fighter`'s events recorded as `outcome`, over `ticks`, counted
/// once each and kept by `keep` (a blow's attacker).
fn counted_events(
    ticks: &[Snapshot],
    fighter_of: impl Fn(&Snapshot) -> Option<Fighter>,
    outcome: Outcome,
    keep: impl Fn(Option<SimId>) -> bool,
) -> usize {
    let mut seen = std::collections::BTreeSet::new();
    for fighter in ticks.iter().filter_map(fighter_of) {
        for event in fighter.events.iter().flatten() {
            if event.result == outcome && keep(event.by) {
                seen.insert(event.sequence);
            }
        }
    }
    seen.len()
}

/// How many blows a swing of seat 0 landed on the creature and how many the
/// creature turned: `keys` is what seat 0 presses to swing, `before` the
/// ticks from then to the moment the creature is hit by something else (the
/// world, `duel c0 hit=1`). The creature does not attack. A hit that
/// happened before the swing is counted as one of the blows landed.
fn swing_at_hit_creature(
    tier: Tier,
    (kind, keys): (&str, &str),
    before: usize,
) -> Result<(usize, usize), String> {
    let name = format!("{tier:?}").to_lowercase();
    let ticks = play(&format!(
        "scenario probe\nrules advanced\nseed 7\n\
         fighter 0 {kind} x=0 defence=2 tiers=2 fp=100 hp=40000\n\
         creature medium x=45 yaw=180 defence=2 tiers=2 hp=40000\n\
         begin\ntick 1 b0=attack\ntick 60\nduel c0 tier={name} rest=100000\ntick 250\n\
         {keys}tick {before}\nduel c0 hit=1\ntick 125\n"
    ))?;
    let of_creature = |tick: &Snapshot| creature_fighter(tick, 0).ok();
    Ok((
        counted_events(&ticks, of_creature, Outcome::Hit, |_| true),
        counted_events(&ticks, of_creature, Outcome::Parried, |_| true),
    ))
}

/// A swing that reaches a creature 300 ms after it was hit is blocked by a
/// Boss and not by an Ordinary creature: the lockout after a hit is 100 ms
/// for the Boss and 400 ms for the Ordinary one.
fn hit_lock_in_the_world() -> Result<String, String> {
    let swing = ("medium", "tick 1 b0=attack\n");
    let [ordinary, boss] =
        [Tier::Ordinary, Tier::Boss].map(|tier| swing_at_hit_creature(tier, swing, HIT_AT_TICKS));
    let (ordinary, boss) = (ordinary?, boss?);
    check(
        ordinary == (2, 0) && boss == (1, 1),
        format!(
            "(blows landed counting the one before, blows turned) with the creature hit {} ms after the swing's key was pressed, about 300 ms before the swing reached it: Ordinary {ordinary:?}, Boss {boss:?}",
            HIT_AT_TICKS * 8
        ),
    )
}

/// Ticks after the swing's key is pressed that the world hits the creature
/// in `hit_lock_in_the_world`: the swing reaches it about 300 ms later.
const HIT_AT_TICKS: usize = 90;

/// A swing that hurt a creature while its guard was down is a hit: it is not
/// turned later when the guard is up. A spin of the staff reaches the
/// creature several times.
fn hurt_swing_is_not_turned_after() -> Result<String, String> {
    let spin = ("staff", "tick 12 u0=-1\ntick 1 u0=-1 f0=127 b0=attack\n");
    let (hits, turned) = swing_at_hit_creature(Tier::Skilled, spin, 40)?;
    check(
        hits >= 2 && turned == 0,
        format!(
            "a spin hit a Skilled creature {hits} times counting the one before it, and was turned {turned} times"
        ),
    )
}

/// A player's block holds as before: a creature's swing at seat 0, who holds
/// block while the world hits it every 80 ms, is turned and never hits, for
/// any tier of its own; without block it hits.
fn player_block_is_unchanged() -> Result<String, String> {
    let rows: Vec<(&str, usize, usize)> =
        [("Ordinary", "h0=block"), ("Boss", "h0=block"), ("Boss", "")]
            .iter()
            .map(|&(tier, hold)| {
                let blows = format!("duel 0 tier={tier} hit=1\ntick 10 {hold}\n").repeat(75);
                let ticks = play(&format!(
                    "scenario probe\nrules advanced\nseed 7\n\
                 fighter 0 medium x=0 defence=2 tiers=2 fp=100 hp=40000\n\
                 creature medium x=45 yaw=180 defence=2 tiers=2 hp=40000\n\
                 begin\ntick 1 b0=attack\ntick 60\nduel c0 tier=boss\ntick 250\n{blows}"
                ))?;
                let of_player =
                    |tick: &Snapshot| tick.own(Seat(0)).and_then(|entity| entity.fighter);
                let by_creature = |by: Option<SimId>| by.is_some();
                Ok((
                    tier,
                    counted_events(&ticks, of_player, Outcome::Hit, by_creature),
                    counted_events(&ticks, of_player, Outcome::Parried, by_creature),
                ))
            })
            .collect::<Result<_, String>>()?;
    check(
        rows[0].1 == 0 && rows[0].2 > 0 && rows[1].1 == 0 && rows[1].2 > 0 && rows[2].1 > 0,
        format!(
            "(own tier, blows by the creature that hit, that were turned): with block held {:?}, {:?}; without block {:?}",
            rows[0], rows[1], rows[2]
        ),
    )
}

/// A breaker by tier.
fn breakers_by_tier() -> Result<String, String> {
    let got = [
        breaker(Tier::Ordinary, true, true),
        breaker(Tier::Skilled, true, true),
        breaker(Tier::Skilled, false, true),
        breaker(Tier::Boss, false, true),
        breaker(Tier::Boss, true, false),
        breaker(Tier::Boss, false, false),
    ];
    let want = [
        Breaker::Kick,
        Breaker::Push,
        Breaker::Kick,
        Breaker::Special,
        Breaker::Push,
        Breaker::Kick,
    ];
    check(got == want, format!("breakers {got:?}, asked {want:?}"))
}

pub fn probes() -> Checks {
    let mut all = vec![
        ("companion_is_never_a_boss", companion_is_never_a_boss()),
        ("ordinary_stays_plain", ordinary_stays_plain()),
        ("boss_uses_its_style", boss_uses_its_style()),
        ("special_falls_back", special_falls_back()),
        ("refused_special_gives_up", refused_special_gives_up()),
        ("no_special_twice", no_special_twice()),
        ("chain_limits", chain_limits()),
        ("reads_beat_the_rest", reads_beat_the_rest()),
        ("rests_between_attacks", rests_between_attacks()),
        ("guard_delay_by_tier", guard_delay_by_tier()),
        ("hit_lock_by_tier", hit_lock_by_tier()),
        ("second_swing_beats_ordinary", second_swing_beats_ordinary()),
        (
            "boss_starts_specials_in_the_world",
            boss_starts_specials_in_the_world(),
        ),
        ("breakers_by_tier", breakers_by_tier()),
        ("hit_lock_in_the_world", hit_lock_in_the_world()),
        (
            "hurt_swing_is_not_turned_after",
            hurt_swing_is_not_turned_after(),
        ),
        ("player_block_is_unchanged", player_block_is_unchanged()),
        ("advances_between_shots", advances_between_shots()),
        ("guarded_walk_keys", guarded_walk_keys()),
        ("aim_is_at_it", aim_is_at_it()),
        ("creature_shots_are_seen", creature_shots_are_seen()),
        ("holds_while_bolts_come", holds_while_bolts_come()),
        ("bolt_is_headed_at", bolt_is_headed_at()),
        (
            "deflector_walks_guarded",
            approach_is((true, false, 400.0, true), Approach::WalkGuarded),
        ),
        (
            "deflector_charges_from_eight_meters",
            approach_is((true, false, 300.0, true), Approach::Charge),
        ),
        (
            "tired_deflector_charges",
            approach_is((true, true, 450.0, true), Approach::Charge),
        ),
        (
            "tired_deflector_runs_when_far",
            approach_is((true, true, 500.0, false), Approach::Run),
        ),
        (
            "blade_runs_at_a_gun",
            approach_is((false, false, 400.0, false), Approach::Run),
        ),
        ("charge_distances", charge_distances()),
        ("charge_waits", charge_waits()),
        ("charge_endings", charge_endings()),
        ("windup_before_the_run", windup_before_the_run()),
        ("tired_cannot_block_a_charge", tired_cannot_block_a_charge()),
        ("charge_mark_clears", charge_mark_clears()),
        (
            "fast_charge_ends_with_a_lunge",
            fast_charge_ends_with_a_lunge(),
        ),
        ("blocked_charge_gives_up", blocked_charge_gives_up()),
        ("one_charge_at_a_time", one_charge_at_a_time()),
        (
            "dead_charger_does_not_block_others",
            dead_charger_does_not_block_others(),
        ),
        (
            "stunned_charger_ends_its_charge",
            stunned_charger_ends_its_charge(),
        ),
        ("refused_kick_does_not_stall", refused_kick_does_not_stall()),
        ("kicks_are_bounded", kicks_are_bounded()),
        (
            "swing_is_waited_out_before_a_kick",
            swing_is_waited_out_before_a_kick(),
        ),
        (
            "swallowed_kick_is_not_counted",
            swallowed_kick_is_not_counted(),
        ),
        ("kick_breaks_a_long_block", kick_breaks_a_long_block()),
        ("no_kick_under_ja", no_kick_under_ja()),
        ("pacing_is_saved", pacing_is_saved()),
        ("walks_near_a_blade", walks_near_a_blade()),
        ("runs_after_a_backing_target", runs_after_a_backing_target()),
        ("sidestep_is_kept", sidestep_is_kept()),
        ("choice_is_kept", choice_is_kept()),
        ("backing_is_run_after", backing_is_run_after()),
        ("swings_at_a_walk", swings_at_a_walk()),
        ("tier_is_found", tier_is_found()),
        ("presses_a_tired_target", presses_a_tired_target()),
        ("swings_at_a_reeling_target", swings_at_a_reeling_target()),
        (
            "skilled_swings_out_of_a_parry",
            skilled_swings_out_of_a_parry(),
        ),
        ("presses_a_won_lock", presses_a_won_lock()),
        ("steps_away_from_a_kata", steps_away_from_a_kata()),
        ("boss_guards_against_a_kata", boss_guards_against_a_kata()),
        ("reeling_moves", reeling_moves()),
        ("guards_during_a_won_lock", guards_during_a_won_lock()),
        ("swings_at_a_turned_back", swings_at_a_turned_back()),
        ("breaks_a_long_block", breaks_a_long_block()),
        ("backs_off_when_tired", backs_off_when_tired()),
        ("boss_rests_once", boss_rests_once()),
        (
            "tired_against_a_gun_does_not_back_off",
            tired_against_a_gun_does_not_back_off(),
        ),
        ("special_cost_table", special_cost_table()),
        ("tired_starts_no_special", tired_starts_no_special()),
        ("ai_keeps_forty", ai_keeps_forty()),
        ("kata_uses_stamina", kata_uses_stamina()),
        ("kata_uses_force_under_ja", kata_uses_force_under_ja()),
        ("feat_special_uses_stamina", feat_special_uses_stamina()),
        (
            "tired_gunner_starts_no_special",
            tired_gunner_starts_no_special(),
        ),
        ("every_holder_has_stamina", every_holder_has_stamina()),
        ("swing_speeds", swing_speeds()),
        ("swings_last_longer", swings_last_longer()),
        ("ja_swing_unchanged", ja_swing_unchanged()),
        ("speed_by_cause", speed_by_cause()),
    ];
    all.extend(stamina::probes());
    all
}
