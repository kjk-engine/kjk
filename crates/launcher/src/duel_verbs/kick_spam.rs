//! Kick spam (`kjk combat duel --only kick_spam`): whether kicking as often
//! as the rules let a player is a way to win, under each rule set.
//!
//! - **In a row**: a player with full Stamina presses the kick key the
//!   moment each kick ends, at nobody, until a press starts no kick. Under
//!   JA's rules a kick costs nothing, so only the time cap stops it.
//! - **Kicked again**: a player kicks a standing foe and then kicks it again
//!   every time it can. The foe does its best: it gets up with the Force,
//!   lights its saber again and holds the block button whenever it is on
//!   its feet. The measure counts
//!   how often the foe goes down, how long it spends unable to block or
//!   strike, and how often it is knocked down again before it ever stood
//!   with its saber lit.
//! - **Mirror duels**: two equal players played by the bench's bot
//!   (`balance_verbs::bot`, average skill). In one set of duels neither
//!   kicks. In the other, seat 0 kicks whenever a kick can start and strikes
//!   its foe when it is down, and seat 1 never kicks. Each duel ends at a
//!   death or the time cap.

use serde_json::{Value, json};
use sim::combat::saber::duel::{meter, tap};
use sim::combat::saber::{SaberRules, Style};
use sim::combat::sheet::Sheet;
use sim::{Level, SimId, holds};

use super::{Bout, Keys};
use crate::balance_verbs::bot::{Bot, Kicking, SKILLS, Skill};
use crate::balance_verbs::tally::Tally;
use crate::saber_verbs::{ALT, ATTACK, Report, jedi, weapons_for};

/// The levels the kicks in a row are counted at: Stamina 62, 84 and 100.
const LEVELS: [u8; 3] = [1, 12, 20];
/// The level and vitality points of each duellist.
const LEVEL: u8 = 12;
const HP: u16 = 120;
/// How far apart (units) the kicker stands from a foe it kicks at once, and
/// from nobody.
const NEAR: f32 = 45.0;
const NOBODY: f32 = 400.0;
/// How long a press of the kick key may start no kick before the kicks in
/// a row end (ms), and the longest they are counted for.
const NO_KICK_MS: u64 = 400;
const IN_A_ROW_MS: u64 = 20_000;
/// How long the kicked-again scene lasts (ms).
const KICKED_MS: u64 = 10_000;
/// Mirror duels per side of the comparison, and the time cap (ms).
const DUELS: u64 = 40;
const DUEL_MS: u64 = 90_000;
/// How far apart (units) the mirror duellists start.
const DUEL_APART: f32 = 160.0;
/// The kicker walks at its foe when farther than this (units).
const CLOSE: f32 = 40.0;
/// The most a kick spammer may win of the mirror duels, and the most kicks
/// in a row a player may start from full Stamina under the Advanced rules.
const SPAM_WINS_MOST: f64 = 0.45;
const IN_A_ROW_MOST: u32 = 2;

/// A duellist of `level` with `weapons`, at full health.
fn duellist(weapons: [Option<sim::combat::sheet::Weapon>; 2], level: u8) -> Sheet {
    let mut sheet = jedi(weapons, 100);
    sheet.level = level;
    sheet.max_hp = HP;
    sheet
}

/// The keys that start a kick under `rules`: the kick key under the
/// Advanced rules, the alternate attack with the staff walking forward
/// under JA's.
fn kick_keys(rules: SaberRules) -> Keys {
    if rules.duel() {
        Keys {
            holds: holds::KICK,
            ..Keys::default()
        }
    } else {
        Keys::press(ALT).with(127, 0)
    }
}

/// A bout under `rules`: a staff kicker at the origin, `foe` `apart` units
/// ahead, both sabers lit.
fn bout(level: &Level, rules: SaberRules, sheets: [Sheet; 2], apart: f32, seed: u64) -> Bout {
    let mut bout = Bout::new(level, sheets, apart, seed);
    bout.under(rules);
    bout.light();
    bout
}

/// Kicks a player of `at` starts in a row from full Stamina under `rules`,
/// and its Stamina before and after.
fn in_a_row(level: &Level, rules: SaberRules, at: u8) -> (u32, i32, i32) {
    let kicker = duellist(weapons_for(Style::Staff), at);
    let foe = duellist(weapons_for(Style::Medium), at);
    let mut bout = bout(level, rules, [kicker, foe], NOBODY, 3);
    let points = |bout: &Bout| i32::from(bout.fighter(0).saber.duel.points);
    let full = points(&bout);
    let (mut kicks, mut was_kicking, mut idle_since) = (0, false, bout.now());
    let end = bout.now() + IN_A_ROW_MS;
    while bout.now() < end {
        let kicking = bout.fighter(0).saber.current_move.kick();
        if kicking && !was_kicking {
            kicks += 1;
        }
        if kicking {
            idle_since = bout.now();
        } else if bout.now() > idle_since + NO_KICK_MS {
            break;
        }
        was_kicking = kicking;
        // A kick starts on a press: the key goes down every other tick.
        let tap = !kicking && bout.last.tick.is_multiple_of(2);
        let press = if tap { kick_keys(rules) } else { Keys::default() };
        bout.step([press, Keys::default()]);
    }
    (kicks, full, points(&bout))
}

/// What the kicked-again scene showed.
#[derive(Debug, Default, serde::Serialize)]
struct KickedAgain {
    kicks: u32,
    floored: u32,
    /// Times the foe went down again before it ever stood with its saber
    /// lit since it last went down.
    floored_before_acting: u32,
    helpless_share: f64,
    longest_helpless_ms: u64,
    foe_hp_lost: f64,
}

/// The kicked-again scene under `rules` (see the module's notes).
fn kicked_again(level: &Level, rules: SaberRules, seed: u64) -> KickedAgain {
    let kicker = duellist(weapons_for(Style::Staff), LEVEL);
    let mut foe = duellist(weapons_for(Style::Medium), LEVEL);
    foe.max_hp = 1000;
    let mut bout = bout(level, rules, [kicker, foe], NEAR, seed);
    let mut seen = KickedAgain::default();
    let hp = bout.fighter(1).hp;
    let (mut was_down, mut was_kicking, mut stood_since_down) = (false, false, true);
    let (mut helpless, mut stretch, mut ticks) = (0u64, 0u64, 0u64);
    let tick_ms = 1000 / u64::from(bout.rate.hz());
    let end = bout.now() + KICKED_MS;
    while bout.now() < end {
        let (me, them) = (bout.fighter(0), bout.fighter(1));
        let kicking = me.saber.current_move.kick();
        seen.kicks += u32::from(kicking && !was_kicking);
        was_kicking = kicking;
        let down = them.saber.floored();
        if down && !was_down {
            seen.floored += 1;
            seen.floored_before_acting += u32::from(!stood_since_down);
            stood_since_down = false;
        }
        was_down = down;
        let able = !down && them.saber.on;
        stood_since_down |= able;
        ticks += 1;
        if able {
            stretch = 0;
        } else {
            helpless += 1;
            stretch += tick_ms;
            seen.longest_helpless_ms = seen.longest_helpless_ms.max(stretch);
        }
        let apart = sim::math::length(sim::math::sub(bout.ps(1).origin, bout.ps(0).origin));
        let affords = !rules.duel() || meter::can_kick(i32::from(me.saber.duel.points));
        let tap = bout.last.tick.is_multiple_of(2);
        let kick = if tap && !kicking && !down && affords && apart < NEAR + 15.0 {
            kick_keys(rules)
        } else if apart > CLOSE && !kicking {
            Keys::default().with(127, 0).walking()
        } else {
            Keys::default()
        };
        // On the ground the foe gets up with the Force, which kicks nobody.
        let defend = if down {
            Keys::default().up(if tap { 127 } else { 0 })
        } else if !them.saber.on {
            let press = if tap { ATTACK } else { 0 };
            Keys::press(press)
        } else {
            Keys::default().holding(holds::BLOCK)
        };
        bout.step([kick, defend]);
    }
    seen.helpless_share = helpless as f64 / ticks.max(1) as f64;
    seen.foe_hp_lost = f64::from(hp - bout.fighter(1).hp) / 100.0;
    seen
}

/// One mirror duel: `skills` playing seat 0 and seat 1. The fight's tally
/// and who won ("a", "b", "both" or "none").
fn duel(level: &Level, rules: SaberRules, skills: [Skill; 2], seed: u64) -> (Tally, &'static str) {
    let sheet = duellist(weapons_for(Style::Medium), LEVEL);
    let mut bout = bout(level, rules, [sheet, sheet], DUEL_APART, seed);
    let ids: [SimId; 2] = [bout.seat(0).id, bout.seat(1).id];
    let mut bots = [0, 1].map(|seat| Bot::new(skills[seat], rules, seed * 2 + seat as u64));
    let pool = sheet.pool();
    let tick_ms = 1000.0 / f64::from(bout.rate.hz());
    let fighters = |bout: &Bout| [bout.fighter(0), bout.fighter(1)];
    let mut tally = Tally::new(ids, fighters(&bout).each_ref());
    let start = bout.now();
    tap::listen();
    let mut winner = "none";
    while bout.now() < start + DUEL_MS {
        let now = bout.now();
        let both = fighters(&bout);
        let apart = sim::math::length(sim::math::sub(bout.ps(1).origin, bout.ps(0).origin));
        let commands = [0u8, 1].map(|seat| {
            let (me, foe) = (&both[usize::from(seat)], &both[usize::from(1 - seat)]);
            let pressed = bots[usize::from(seat)].keys(now, tick_ms, me, (ids[usize::from(1 - seat)], foe), apart);
            let keys = Keys {
                buttons: pressed.buttons,
                holds: pressed.holds,
                ..Keys::default()
            }
            .with(pressed.forward, pressed.right);
            bout.command(seat, keys, &bout.ps(1 - seat))
        });
        bout.advance(commands);
        let both = fighters(&bout);
        let motions = [bout.ps(0), bout.ps(1)];
        tally.tick(
            bout.now() - start,
            [(&both[0], pool, &motions[0]), (&both[1], pool, &motions[1])],
            &tap::take(),
        );
        winner = match (both[0].alive(), both[1].alive()) {
            (true, true) => continue,
            (true, false) => "a",
            (false, true) => "b",
            (false, false) => "both",
        };
        break;
    }
    tap::stop();
    tally.finish();
    (tally, winner)
}

/// Mirror duels of `skills` under `rules`: seat 0's wins, seat 1's, and the
/// averages that matter for kicks.
fn mirror(level: &Level, rules: SaberRules, skills: [Skill; 2]) -> Value {
    let (mut won, mut lost) = (0u32, 0u32);
    let mut sums = [[0.0f64; 7]; 2];
    for seed in 0..DUELS {
        let (tally, winner) = duel(level, rules, skills, seed);
        won += u32::from(winner == "a");
        lost += u32::from(winner == "b");
        let ticks = tally.ticks.max(1) as f64;
        for (sum, side) in sums.iter_mut().zip(&tally.sides) {
            let values = [
                f64::from(side.kicks_thrown),
                f64::from(side.kicks_landed),
                f64::from(side.kicked_floored),
                side.helpless_ticks as f64 / ticks,
                side.kick_damage as f64 / 100.0,
                (side.damage - side.kick_weapon_damage) as f64 / 100.0,
                f64::from(side.swings),
            ];
            for (total, value) in sum.iter_mut().zip(values) {
                *total += value;
            }
        }
    }
    let n = DUELS as f64;
    let side = |sum: &[f64; 7]| {
        let per_point = |hp: f64, points: f64| if points > 0.0 { hp / points } else { 0.0 };
        json!({
            "kicks": sum[0] / n,
            "kicks_landed": sum[1] / n,
            "knocked_down_by_kicks": sum[2] / n,
            "helpless_share": sum[3] / n,
            "hp_per_kick_point": per_point(sum[4], sum[0] * f64::from(rules::duel::kicks::USES)),
            "hp_per_swing_point": per_point(sum[5], sum[6] * f64::from(rules::duel::costs::SWING)),
        })
    };
    json!({
        "duels": DUELS,
        "seat_0_wins": won,
        "seat_1_wins": lost,
        "seat_0_win_share": f64::from(won) / n,
        "seats": [side(&sums[0]), side(&sums[1])],
    })
}

/// `kjk combat duel --only kick_spam`.
pub(super) fn kick_spam(level: &Level, report: &mut Report) -> Value {
    outln!(
        "kick_spam: a kick costs {} Stamina under the Advanced rules",
        rules::duel::kicks::USES
    );
    let average = SKILLS.iter().find(|skill| skill.name == "average").copied().expect("the average skill");
    let calm = Skill { kick: 0.0, ..average };
    let spammer = Skill {
        kicking: Kicking::Spam,
        ..average
    };
    let mut facts = serde_json::Map::new();
    for rules in SaberRules::ALL {
        let name = rules.name();
        let rows: Vec<Value> = LEVELS
            .iter()
            .map(|&at| {
                let (kicks, full, left) = in_a_row(level, rules, at);
                outln!("  {name}: level {at}, {kicks} kicks in a row from {full} Stamina, {left} left");
                json!({ "level": at, "kicks": kicks, "stamina": full, "left": left })
            })
            .collect();
        if rules.duel() {
            let most = rows.iter().filter_map(|row| row["kicks"].as_u64()).max().unwrap_or(0);
            report.check(
                format!("{name}: at most {IN_A_ROW_MOST} kicks in a row from full Stamina"),
                most <= u64::from(IN_A_ROW_MOST),
                format!("{most} kicks in a row"),
            );
        }
        let again: Vec<KickedAgain> = (0..4).map(|seed| kicked_again(level, rules, seed)).collect();
        for (seed, seen) in again.iter().enumerate() {
            outln!(
                "  {name}: kicked again (seed {seed}): {} kicks, {} knocked down, {} of them before the foe stood with its saber lit, helpless {:.0}% of the time, longest {} ms, {:.1} HP lost",
                seen.kicks,
                seen.floored,
                seen.floored_before_acting,
                seen.helpless_share * 100.0,
                seen.longest_helpless_ms,
                seen.foe_hp_lost
            );
        }
        let calm_duels = mirror(level, rules, [calm, calm]);
        let spam_duels = mirror(level, rules, [spammer, calm]);
        for (what, duels) in [("neither kicks", &calm_duels), ("seat 0 kicks whenever it can", &spam_duels)] {
            let seats = &duels["seats"];
            outln!(
                "  {name}: mirror duels, {what}: seat 0 won {} and seat 1 won {} of {DUELS}; seat 0 kicked {:.1} times a duel and landed {:.1}; seat 1 was knocked down by a kick {:.1} times and helpless {:.0}% of the time; seat 0 dealt {:.2} HP for each Stamina point spent on kicks and {:.2} for each spent on swings",
                duels["seat_0_wins"],
                duels["seat_1_wins"],
                seats[0]["kicks"].as_f64().unwrap_or(0.0),
                seats[0]["kicks_landed"].as_f64().unwrap_or(0.0),
                seats[1]["knocked_down_by_kicks"].as_f64().unwrap_or(0.0),
                seats[1]["helpless_share"].as_f64().unwrap_or(0.0) * 100.0,
                seats[0]["hp_per_kick_point"].as_f64().unwrap_or(0.0),
                seats[0]["hp_per_swing_point"].as_f64().unwrap_or(0.0),
            );
        }
        if rules.duel() {
            let share = spam_duels["seat_0_win_share"].as_f64().unwrap_or(1.0);
            report.check(
                format!("{name}: a kick spammer wins at most {:.0}% of mirror duels", SPAM_WINS_MOST * 100.0),
                share <= SPAM_WINS_MOST,
                format!("won {:.0}%", share * 100.0),
            );
        }
        facts.insert(
            name.into(),
            json!({
                "in_a_row": rows,
                "kicked_again": again,
                "mirror_no_kicks": calm_duels,
                "mirror_kick_spam": spam_duels,
            }),
        );
    }
    Value::Object(facts)
}

