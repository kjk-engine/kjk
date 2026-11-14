//! The probes of how fighters pace and spend a fight under the Advanced
//! rules (`kjk sim probes --only duelling`). Special moves use up Stamina,
//! for every character, and a Force user's Force points are left for the
//! Force powers; under Jedi Academy's rules the older bar still pays.

use super::duels::play;
use crate::Seat;
use crate::combat::Fighter;
use crate::combat::pace::{
    Arms, BACKING_MS, Dice, Keys, Pace as Pacing, Reaction, Sight, TIP, Tier, WALK_WITHIN, Way,
    ai_may_special, moving_away, react, tier, walks,
};
use crate::combat::saber::duel::exchange;
use crate::combat::saber::duel::standing::{cap, may_start_special, special_cost};
use crate::combat::saber::duel::{Duel, Pace, swing_speed};
use crate::combat::saber::{Move, Style};
use crate::combat::sheet::Sheet;
use crate::math::Vec3;
use crate::module::ObjectKind;
use crate::motion::anim::is_kick;
use crate::motion::move_speed;
use crate::probe::{Checks, check};
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
    lowest_stamina: i32,
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
            lowest_stamina: after.iter().map(stamina).min().unwrap_or_default(),
            lowest_older: after.iter().map(|f| f.fp).min().unwrap_or_default(),
            last_stamina: after.last().map(stamina).unwrap_or_default(),
        })
    }

    /// Stamina used up.
    fn stamina_used(self) -> i32 {
        self.stamina - self.lowest_stamina
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
    let tired = rules::duel::meters::TIRED;
    let spent = burst("advanced", 100, &format!("duel 0 points={tired}"))?;
    check(
        spent.lowest_stamina >= tired && spent.older_used() <= 0,
        format!(
            "advanced: a gunner with {tired} Stamina used up {} Stamina and {} older bar",
            spent.stamina_used(),
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
        own_free: true,
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
    (0..count)
        .map(|look| {
            let now = 1000 + look * 100;
            let sight = &sight(look);
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
            (
                way,
                reaction,
                pace.finish(keys, sight, reaction, now, &mut dice, &clear, 0.0, 1.0),
            )
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
        ..Pacing::default()
    };
    let mut out = Writer::default();
    held.encode(&mut out);
    let bytes = out.0;
    let again = Pacing::decode(&mut Reader::new("pace", &bytes)).map_err(|e| e.to_string())?;
    let old = bytes.len() - 70;
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

/// The moves a stagger, a knocked-aside blow and a broken guard play count as
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

/// Seat 0 holds block for 2.5 seconds in front of a hostile creature with a
/// vibroblade, under `rules`. The tick the creature first kicked, and seat 0's
/// Stamina at the end.
fn block_held_before_a_creature(rules: &str) -> Result<(Option<usize>, i32), String> {
    let text = format!(
        "scenario probe\nrules {rules}\nseed 7\n\
         fighter 0 medium x=0 defence=2 tiers=2 fp=100 hp=400\n\
         creature vibroblade x=45 yaw=180 defence=2 tiers=2 hp=400\n\
         begin\ntick 1\ntick 60\ntick 400 h0=block c0=0\n"
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

/// A creature kicks a target that has held block for 2.5 seconds, and the
/// kick leaves the target with more than 20 Stamina.
fn kick_breaks_a_long_block() -> Result<String, String> {
    let (kicked, stamina) = block_held_before_a_creature("advanced")?;
    let (earliest, latest) = (
        SETTLED + TICKS_PER_SECOND * 2,
        SETTLED + TICKS_PER_SECOND * 4,
    );
    check(
        kicked.is_some_and(|tick| (earliest..=latest).contains(&tick)) && !exchange::tired(stamina),
        format!(
            "the first kick on tick {kicked:?}, asked for between {earliest} and {latest}; the blocker ends with {stamina} Stamina"
        ),
    )
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
/// block throughout (but for the looks `released` holds), as `free(look)`
/// says whether the creature may start a kick.
fn kick_looks(count: u64, released: u64, free: impl Fn(u64) -> bool) -> Vec<Keys> {
    let sight = |look| {
        let mut sight = duel_sight(45.0);
        sight.blocking = look != released;
        sight.own_free = free(look);
        sight
    };
    looked(sight, count)
        .into_iter()
        .map(|(_, _, keys)| keys)
        .collect()
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

pub fn probes() -> Checks {
    vec![
        ("refused_kick_does_not_stall", refused_kick_does_not_stall()),
        ("kicks_are_bounded", kicks_are_bounded()),
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
    ]
}
