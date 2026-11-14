//! The duel mode's probes (`kjk sim probes --only "duel rules"`): duels of
//! the `m6` trace's made-up fighters (`duels::play`) under the Advanced
//! rules, each check saying the numbers it saw. A perfect block refills its
//! blocker and staggers its attacker, and disarms it only when the cost
//! leaves it with no Stamina; a held block drains the Stamina; a blocked
//! swing costs its attacker the swing and the block; the attacker's own
//! Stamina picks the bounce it plays; a blocker worn down has its guard
//! broken and is set back up; the same fight decides the same way under
//! every seed; the Stamina refills, not while block is held; a creature
//! blocks of itself; a player turns bolts only holding block, a vibroblade
//! parries blows but turns no bolt; a kick floors one not absorbing it; a
//! creature's worn staff breaks, a player's never wears; a block held while
//! running holds at the run's own speed; two swings meeting lock; and under
//! JA the block and kick keys change nothing.

use super::duels::play;
use crate::Seat;
use crate::combat::Fighter;
use crate::combat::fighter::Outcome;
use crate::combat::saber::Move;
use crate::combat::saber::duel::Pace;
use crate::combat::saber::duel::exchange::{Guard, Took};
use crate::combat::saber::duel::shots;
use crate::combat::saber::duel::tap::{self, Tapped};
use crate::combat::sheet::feats;
use crate::probe::{Checks, check};
use crate::snapshot::{Kind, Snapshot};

/// Two medium-style players 45 units apart facing each other, lit, under
/// `rules`, with `more` directives before they begin and `then` after.
fn duel(rules: &str, more: &str, then: &str) -> String {
    duel_of(rules, ["medium", "medium"], more, then)
}

/// `duel` with each seat's fighter kind (`strong`, `staff`, ...).
fn duel_of(rules: &str, [one, two]: [&str; 2], more: &str, then: &str) -> String {
    format!(
        "scenario probe\nrules {rules}\nseed 7\n\
         fighter 0 {one} x=0 defence=2 tiers=2 fp=100 hp=400\n\
         fighter 1 {two} x=45 yaw=180 defence=2 tiers=2 fp=100 hp=400\n\
         {more}\nbegin\ntick 1 b0=attack b1=attack\ntick 60\n{then}\n"
    )
}

/// `line` repeated `times` times, a line each.
fn times(line: &str, times: usize) -> String {
    vec![line; times].join("\n")
}

/// A seat's fighter in `snapshot`.
fn seat(snapshot: &Snapshot, n: u8) -> Option<Fighter> {
    snapshot.own(Seat(n)).and_then(|entity| entity.fighter)
}

/// The first placed creature's fighter in `snapshot`.
fn creature(snapshot: &Snapshot) -> Option<Fighter> {
    snapshot.entities.iter().find_map(|entity| {
        (entity.kind == Kind::Object(crate::module::ObjectKind::Creature, 0))
            .then_some(entity.fighter)
            .flatten()
    })
}

/// Seat `n`'s fighter every tick of `ticks`.
fn fighters(ticks: &[Snapshot], n: u8) -> Vec<Fighter> {
    ticks.iter().filter_map(|tick| seat(tick, n)).collect()
}

/// A fighter's meter: its block points.
fn meter(f: &Fighter) -> i32 {
    i32::from(f.saber.duel.points)
}

/// What a blocker's bar fell by over a fight, in all: every fall from one
/// tick to the next added up. A player holding block never refills, so the
/// bar only falls, except where a broken guard sets it back up.
fn paid(ticks: &[Fighter]) -> i32 {
    ticks
        .windows(2)
        .map(|pair| (meter(&pair[0]) - meter(&pair[1])).max(0))
        .sum()
}

/// Every row of a rule's own table checked: what `played` gives for each
/// input against what the rules ask. Each row is said, whether it held or
/// not.
fn rule_rows<I: std::fmt::Debug + Copy, V: PartialEq + std::fmt::Debug>(
    rows: &[(I, V)],
    played: impl Fn(I) -> V,
) -> (bool, String) {
    let mut right = true;
    let mut said = Vec::new();
    for (input, want) in rows {
        let got = played(*input);
        right &= got == *want;
        said.push(format!("{input:?} gives {got:?}, the rules ask {want:?}"));
    }
    (right, said.join("; "))
}

/// How many of `f`'s events recorded `outcome`.
fn count_events(ticks: &[Fighter], outcome: Outcome) -> usize {
    let mut seen = std::collections::BTreeSet::new();
    for f in ticks {
        for event in f.events.iter().flatten() {
            if event.result == outcome {
                seen.insert(event.sequence);
            }
        }
    }
    seen.len()
}

/// The ticks the duel's lighting and settling take (`duel`): what is
/// kept before `then` begins.
const SETTLED: usize = 61;

/// What one exchange decided, as the blades' tap told it
/// (`duel::tap::Tapped::Exchange`).
#[derive(Debug, Clone, Copy)]
struct Met {
    /// The Stamina each side held as the decision read it, the attacker
    /// first.
    points: [i32; 2],
    /// What each side played, the attacker first.
    took: [Took; 2],
    stopped: bool,
    perfect: bool,
    /// How the blocker was guarding.
    guard: Guard,
}

/// `text` played, with every exchange its blades decided.
fn played(text: &str) -> Result<(Vec<Snapshot>, Vec<Met>), String> {
    tap::listen();
    let ticks = play(text);
    let met = tap::take()
        .into_iter()
        .filter_map(|tapped| match tapped {
            Tapped::Exchange {
                points,
                took,
                perfect,
                stopped,
                guard,
                ..
            } => Some(Met {
                points,
                took,
                stopped,
                perfect,
                guard,
            }),
            Tapped::Block { .. } | Tapped::Clash { .. } => None,
        })
        .collect();
    tap::stop();
    Ok((ticks?, met))
}

/// The first blow seat 1's held block stopped plainly (no perfect block).
fn first_blocked(met: &[Met]) -> Option<Met> {
    met.iter()
        .find(|met| met.stopped && !met.perfect && met.guard == Guard::Held)
        .copied()
}

/// Every row of `bands` checked: what the fight of a side set to `points`
/// Stamina played, against what the rules ask. Each row's numbers are
/// said, whether it held or not.
fn band_rows(
    bands: &[(i32, Took)],
    played: impl Fn(i32) -> Result<Took, String>,
) -> Result<(bool, String), String> {
    let mut right = true;
    let mut said = Vec::new();
    for &(points, want) in bands {
        let took = played(points)?;
        right &= took == want;
        said.push(format!(
            "from {points} Stamina it played {took:?}, the rules ask {want:?}"
        ));
    }
    Ok((right, said.join("; ")))
}

/// A fight where seat 1 blocks perfectly: block held with attack pressed
/// afresh more often than the window lasts, so a window is open when seat
/// 0's swing lands. Seat 0 starts with `points` Stamina, and the blocker's
/// own is at half so its reward shows.
fn perfect_fight(points: i32) -> String {
    let set = if points > 0 {
        format!("duel 0 points={points}\n")
    } else {
        String::new()
    };
    let spells = format!(
        "{set}duel 1 points=50\nstat 1 fp 5000\n{}\n",
        times(
            "tick 15 b0=attack h1=block b1=attack\ntick 1 b0=attack h1=block",
            40
        )
    );
    // The blocker is level 20, so its Stamina holds 100 and its guard does
    // not break before the first perfect block. A broken guard would let
    // the attacker's blow land, and a landed blow gives the attacker
    // Stamina back (`gains::HURT`), which is what the attacker's own
    // Stamina is read for here.
    duel("advanced", "sheet 1 level=20", &spells)
}

/// A perfect block: the blocker's Stamina and the attacker's around it,
/// what the attacker plays, and the blocker's riposte window.
fn perfect() -> Result<String, String> {
    let ticks = play(&perfect_fight(0))?;
    let (blocker, attacker) = (fighters(&ticks, 1), fighters(&ticks, 0));
    let at = (1..blocker.len())
        .find(|&i| blocker[i].saber.duel.perfect_at != blocker[i - 1].saber.duel.perfect_at)
        .ok_or("no perfect block in 640 ticks")?;
    let (before, after) = (meter(&blocker[at - 1]), meter(&blocker[at]));
    let (paid_from, paid_to) = (meter(&attacker[at - 1]), meter(&attacker[at]));
    // The blocker's riposte window (`Duel::riposte_until`).
    let riposte = blocker[at].saber.duel.riposte_until > blocker[at - 1].saber.duel.riposte_until;
    let reeling = attacker[at..(at + 3).min(attacker.len())]
        .iter()
        .find(|f| f.saber.current_move.broken() || f.saber.current_move.bounce())
        .map(|f| f.saber.current_move);
    check(
        after > before && paid_to < paid_from && reeling.is_some() && riposte,
        format!(
            "advanced: perfect block at tick {at}: the blocker's Stamina {before} -> {after}, \
             the attacker's {paid_from} -> {paid_to}, it plays {}, riposte {riposte}",
            reeling.map_or("nothing", Move::name)
        ),
    )
}

/// What a perfectly blocked attacker who began with `points` Stamina
/// plays, and the Stamina it truly held when the perfect block met it.
fn perfect_took(points: i32) -> Result<(Took, i32), String> {
    let (_, met) = played(&perfect_fight(points))?;
    met.iter()
        .find(|met| met.perfect)
        .map(|met| (met.took[0], met.points[0]))
        .ok_or_else(|| format!("no perfect block in 640 ticks from {points} Stamina"))
}

/// A perfect block disarms its attacker only when the cost
/// (`perfect::ATTACKER_COST`) leaves the attacker with no Stamina.
/// Otherwise it heavily staggers them. The Stamina the attacker truly held
/// when the block met it decides which, and is said for each row.
fn perfect_disarms_only_at_zero() -> Result<String, String> {
    use rules::duel::perfect;
    let mut right = true;
    let mut played = Vec::new();
    let mut said = Vec::new();
    for points in [100, 1] {
        let (took, left) = perfect_took(points)?;
        let want = if left - perfect::ATTACKER_COST <= 0 {
            Took::Disarm
        } else {
            Took::HeavyStagger
        };
        right &= took == want;
        played.push(took);
        said.push(format!(
            "an attacker that began with {points} Stamina held {left} when the block met it and \
             played {took:?}, the rules ask {want:?}"
        ));
    }
    // One row must be disarmed and the other staggered, or the rule is
    // never tried.
    let both = played.contains(&Took::Disarm) && played.contains(&Took::HeavyStagger);
    check(
        right && both,
        format!("advanced: a perfect block, {}", said.join("; ")),
    )
}

/// One swing stopped by a held block: its attacker pays for starting the
/// swing (`costs::SWING`) and for the block (`block::BLOCKED`), and for
/// nothing else.
fn blocked_swing_costs_attacker() -> Result<String, String> {
    use crate::combat::saber::duel::standing;
    use rules::duel::{block, costs};
    let ticks = play(&duel("advanced", "", "tick 600 b0=attack h1=block"))?;
    let points: Vec<i32> = fighters(&ticks, 0).iter().map(meter).collect();
    // A seat's made-up fighter is level 1, so its Stamina is full at 62.
    let full = i32::from(standing::cap(1));
    let swung = full - costs::SWING;
    let at = points
        .iter()
        .position(|&left| left < swung)
        .ok_or_else(|| {
            format!("the attacker never paid for a blocked swing: it kept {swung} Stamina at worst")
        })?;
    let (lost, want) = (full - points[at], costs::SWING + block::BLOCKED);
    check(
        lost == want && points[..at].contains(&swung),
        format!(
            "advanced: the attacker's Stamina fell {full} -> {swung} on starting the swing and \
             -> {} once it was blocked, {lost} lost in all; the rules ask {want}",
            points[at]
        ),
    )
}

/// A creature swinging into a player's held block: the creature pays for
/// every blocked swing and is given nothing back, so it ends the fight with
/// less Stamina than it began with. No exchange rewards an attacker whose
/// blow was stopped.
fn npc_pays_for_a_blocked_swing() -> Result<String, String> {
    let text = "scenario probe\nrules advanced\nseed 7\n\
                fighter 0 medium x=0 defence=2 tiers=2 fp=100 hp=400\n\
                creature medium x=45 yaw=180 defence=2 tiers=2 fp=100 hp=400\n\
                begin\ntick 1 b0=attack\ntick 60\ntick 2000 h0=block c0=0\n";
    let (ticks, met) = played(text)?;
    let them: Vec<Fighter> = ticks.iter().filter_map(creature).collect();
    let (first, last) = (
        them.first().ok_or("no creature")?,
        them.last().ok_or("no creature")?,
    );
    let (before, after) = (meter(first), meter(last));
    let blocked = met.iter().filter(|met| met.stopped).count();
    check(
        blocked > 0 && after < before,
        format!(
            "advanced: the player's held block stopped {blocked} of the creature's swings; the \
             creature's Stamina went {before} -> {after}"
        ),
    )
}

/// What an attacker that held `points` Stamina before its swing plays when
/// a held block stops it.
fn blocked_bounce(points: i32) -> Result<Took, String> {
    let then = format!("duel 0 points={points}\ntick 600 b0=attack h1=block");
    let (_, met) = played(&duel("advanced", "", &then))?;
    first_blocked(&met)
        .map(|met| met.took[0])
        .ok_or_else(|| format!("no blow stopped by the held block from {points} Stamina"))
}

/// The attacker's own Stamina picks the bounce after a blocked swing
/// (`block::BOUNCE_BANDS`), read after the swing's cost and the block's.
fn tired_attacker_staggers() -> Result<String, String> {
    let bands = [
        (15, Took::HeavyStagger),
        (40, Took::Bounce(Pace::Blocked)),
        (100, Took::Bounce(Pace::Parried)),
    ];
    let (right, said) = band_rows(&bands, blocked_bounce)?;
    check(right, format!("advanced: a blocked swing, {said}"))
}

/// A blocker with less Stamina than the block costs: the blow is not
/// stopped, the blocker is heavily staggered, and its Stamina is set to
/// `meters::BROKEN_TO`. A blocker above the tired mark never breaks, since
/// no block costs more than a tenth of the bar.
fn guard_breaks_at_zero() -> Result<String, String> {
    let broken_to = rules::duel::meters::BROKEN_TO;
    let then = "duel 1 points=3\ntick 600 b0=attack h1=block";
    let (ticks, met) = played(&duel("advanced", "", then))?;
    // Only a guard break leaves the blocker heavily staggered by a blow
    // that was not stopped: a lunge past a tired blocker staggers it
    // plainly, and an unblockable blow knocks it aside.
    let broke = met.iter().position(|met| {
        !met.stopped && met.guard != Guard::None && met.took[1] == Took::HeavyStagger
    });
    let points: Vec<i32> = fighters(&ticks, 1).iter().map(meter).collect();
    let lowest = points.iter().copied().min().unwrap_or(0);
    let back = points.iter().position(|&left| left == broken_to);
    check(
        broke.is_some() && back.is_some(),
        format!(
            "advanced: the blocker's guard broke on exchange {}; its Stamina fell to {lowest} \
             and stood at {broken_to} on tick {}",
            broke.map_or("never".into(), |at| at.to_string()),
            back.map_or("never".into(), |at| at.to_string())
        ),
    )
}

/// The same fight under several seeds: the exchange reads no random number,
/// so the first blow a held block stops is answered the same way every
/// time.
fn same_fight_same_result() -> Result<String, String> {
    let mut seen = Vec::new();
    for seed in 1..=SAME_SEEDS {
        let more = format!("seed {seed}");
        let (_, met) = played(&duel("advanced", &more, "tick 600 b0=attack h1=block"))?;
        let first = first_blocked(&met)
            .ok_or_else(|| format!("no blow stopped by the held block on seed {seed}"))?;
        seen.push(first.took);
    }
    let same = seen.windows(2).all(|pair| pair[0] == pair[1]);
    check(
        same,
        format!("advanced: the first blocked swing of {SAME_SEEDS} seeds was answered {seen:?}"),
    )
}

/// The seeds `same_fight_same_result` tries.
const SAME_SEEDS: u64 = 5;

/// How many times a duel's encoding held before the one bar: the thirteen
/// it holds now and the time the mishap count next fell, which stood fifth.
const OLD_TIMES: u64 = 14;

/// A saber saved before the one bar, decoded at the snapshot version just
/// before the Advanced rules' gate: its Stamina is kept and it is not
/// tired, while the mishap count, the fatigued and drained marks and the
/// decay time are dropped. The times after the dropped one still land in
/// the right fields, which the refill's and the disarm guard's show.
fn old_duel_state_loads() -> Result<String, String> {
    use crate::combat::saber::duel::Duel;
    use crate::wire::{Reader, Versions, Writer};
    let mut out = Writer(Vec::new());
    out.u8(42);
    // The mishap count, dropped.
    out.u8(9);
    // The old flags: fatigued stood at bit 0 and drained at bit 4.
    out.u16(1 | 1 << 4);
    // Each time its own number of seconds, in the old order.
    for nth in 1..=OLD_TIMES {
        out.u64(nth * 1000);
    }
    // The pace, the hilt's wear and the perfect parry playing.
    out.u8(0);
    out.u16(3);
    out.u8(0);
    let bytes = out.0;
    let before = crate::since::snapshot::FEAT_ADVANCED_COMBAT.1 - 1;
    let versions = Versions {
        snapshot: before,
        ..Versions::THIS_BUILD
    };
    let mut read = Reader::new("duel", &bytes).with_versions(versions);
    let duel = Duel::decode(&mut read)
        .map_err(|error| format!("a saber saved before the one bar: {error}"))?;
    check(
        duel.points == 42
            && !duel.tired()
            && duel.wear == 3
            && duel.refill_at == 4000
            && duel.guard_until == 6000,
        format!(
            "a saber from snapshot version {before}: {} Stamina, tired {}, wear {}, the refill \
             at {} ms and the disarm guard until {} ms",
            duel.points,
            duel.tired(),
            duel.wear,
            duel.refill_at,
            duel.guard_until
        ),
    )
}

/// Starting a swing takes Stamina from a creature as it does from a player
/// (`costs::SWING`): three swings begun take three times the cost.
fn swing_charges_creatures() -> Result<String, String> {
    use crate::combat::saber::duel::meter;
    use rules::duel::{costs, meters};
    let swings = [Move::A_TL2BR, Move::A_BR2TL, Move::A_T2B];
    let mut f = Fighter::fresh(10_000, 10);
    f.saber.duel.points = meters::POINTS_MAX;
    let mut current = Move::READY;
    for next in swings {
        meter::started(&mut f, current, next, meters::POINTS_MAX);
        current = next;
    }
    let (lost, want) = (
        i32::from(meters::POINTS_MAX) - meter(&f),
        swings.len() as i32 * costs::SWING,
    );
    check(
        lost == want,
        format!(
            "a creature's {} swings took {lost} Stamina, the rules ask {want}",
            swings.len()
        ),
    )
}

/// The Force pool no longer holds a block up: a player with no Force points
/// and 50 Stamina still blocks a swing and pays for it from the Stamina.
fn force_does_not_stop_a_block() -> Result<String, String> {
    let then = "duel 1 points=50\nstat 1 fp 0\ntick 600 b0=attack h1=block";
    let (ticks, met) = played(&duel("advanced", "", then))?;
    let blocker = fighters(&ticks, 1);
    let stopped = met.iter().filter(|met| met.stopped).count();
    let started = blocker
        .get(SETTLED)
        .map_or(0, |f| i32::from(f.saber.duel.points));
    let lowest = blocker.iter().map(meter).min().unwrap_or(0);
    check(
        stopped > 0 && lowest < started,
        format!(
            "advanced: a player with no Force points stopped {stopped} swings; its Stamina went \
             {started} -> {lowest}"
        ),
    )
}

/// A tired fighter's swings play slowly (`speed::TIRED` on top of the style's
/// speed), one above the tired mark at the style's own speed.
fn tired_swings_are_slow() -> Result<String, String> {
    use crate::combat::saber::duel::{Duel, play_speed};
    use rules::duel::{meters, speed};
    let at = |points: u8| {
        let duel = Duel {
            points,
            ..Duel::FRESH
        };
        play_speed(&duel, None, Move::A_TL2BR, 0)
    };
    let tired = meters::TIRED as u8;
    let (slow, normal) = (at(tired), at(tired + 1));
    let want = [speed::TIRED * speed::STYLES[1], speed::STYLES[1]];
    let marks = [
        Duel {
            points: tired,
            ..Duel::FRESH
        }
        .tired(),
        Duel {
            points: tired + 1,
            ..Duel::FRESH
        }
        .tired(),
    ];
    check(
        [slow, normal] == want && marks == [true, false],
        format!(
            "advanced: a swing from {tired} Stamina plays at {slow} normal speed and one from {} \
             at {normal}, the rules ask {want:?}; tired {marks:?}",
            tired + 1
        ),
    )
}

/// Block held alone against seat 0's swings: it pays from its block
/// points, the Force left alone.
fn held() -> Result<String, String> {
    let ticks = play(&duel("advanced", "", "tick 2000 b0=attack h1=block"))?;
    let blocker = fighters(&ticks, 1);
    let (first, last) = (
        blocker.first().ok_or("no ticks")?,
        blocker.last().ok_or("no ticks")?,
    );
    let blocks = count_events(&blocker, Outcome::Parried);
    let (points, force) = (
        (first.saber.duel.points, last.saber.duel.points),
        (first.fp, last.fp),
    );
    check(
        points.1 < points.0 && blocks > 0,
        format!(
            "advanced: {blocks} blows blocked; block points {} -> {}, Force (hundredths) {} -> {}",
            points.0, points.1, force.0, force.1
        ),
    )
}

/// The Stamina left standing: from 40 over ten seconds it refills; while
/// block is held it stays where it is.
fn refill() -> Result<String, String> {
    // The blocker settled, and at the end.
    let rest = |hold: &str| -> Result<[Fighter; 2], String> {
        let then = format!("duel 1 points=40\ntick 1250 {hold}");
        let blocker = fighters(&play(&duel("advanced", "", &then))?, 1);
        let first = *blocker.get(SETTLED).ok_or("no ticks")?;
        Ok([first, *blocker.last().ok_or("no ticks")?])
    };
    let (rested, holding) = (rest("")?, rest("h1=block")?);
    let points = rested.map(|f| f.saber.duel.points);
    let held = holding.map(|f| f.saber.duel.points);
    check(
        points[1] > points[0] && held[1] <= held[0],
        format!(
            "advanced: Stamina {} -> {} in 10 s; holding block, {} -> {}",
            points[0], points[1], held[0], held[1]
        ),
    )
}

/// A hostile creature with a saber, attacked by seat 0: it blocks of
/// itself, paying from its block points.
fn creature_blocks() -> Result<String, String> {
    let text = "scenario probe\nrules advanced\nseed 7\n\
                fighter 0 medium x=0 defence=2 tiers=2 fp=100 hp=400\n\
                creature medium x=45 yaw=180 defence=2 tiers=2 fp=100\n\
                begin\ntick 1 b0=attack\ntick 60\ntick 800 b0=attack c0=0\n";
    let ticks = play(text)?;
    let them: Vec<Fighter> = ticks.iter().filter_map(creature).collect();
    let (first, last) = (
        them.first().ok_or("no creature")?,
        them.last().ok_or("no creature")?,
    );
    let blocks = count_events(&them, Outcome::Parried);
    check(
        blocks > 0,
        format!(
            "advanced: the creature blocked {blocks} times; its Stamina {} -> {}",
            first.saber.duel.points, last.saber.duel.points
        ),
    )
}

/// Where a creature put at 200 on the floor's axis stands: the world keeps a
/// creature's place in its own frame, so the shooter is placed round what
/// the snapshot says.
fn creature_origin() -> Result<[f32; 2], String> {
    let z = 0.125 - crate::motion::SOLE_Z;
    let text = format!(
        "scenario probe\nrules advanced\nseed 7\nfighter 0 pistol x=0 hp=400\n\
         creature medium x=200 yaw=180 hp=400\nbegin\nput c0 200 0 {z} 180\ntick 1\n"
    );
    let ticks = play(&text)?;
    let origin = ticks
        .last()
        .and_then(|snapshot| {
            snapshot.entities.iter().find_map(|entity| {
                (entity.kind == Kind::Object(crate::module::ObjectKind::Creature, 0))
                    .then_some(entity.player)
                    .flatten()
            })
        })
        .map(|ps| ps.origin)
        .ok_or("no creature")?;
    Ok([origin[0], origin[1]])
}

/// What a pistol's bolts did to the target of `shot_from`: how many were
/// fired, how many the target deflected, how many hit it, the health it lost
/// and the Stamina it paid.
#[derive(Debug, Clone, Copy)]
struct Shots {
    fired: usize,
    deflected: usize,
    hit: usize,
    lost: i32,
    paid: i32,
}

/// A pistol shooting the fighter `target` (a `fighter 1 KIND` or `creature
/// KIND` line), standing at 200 and facing 180 with `tier` tiers of Jedi
/// Defense, from `degrees` off the way it faces, the target holding `hold`,
/// under the Advanced rules: the bolts it deflected, the health it lost and
/// the Stamina it paid. The target has health enough to outlast every bolt,
/// and a second of quiet at the end lets the last bolt land.
fn shot_from(target: &str, tier: u8, hold: &str, degrees: f32) -> Result<Shots, String> {
    let creature_target = target.starts_with("creature");
    // A creature turns to face its enemy unless it is stunned, so it is
    // stunned and the shooter moves round it. A player always looks at the
    // shooter, so it is turned away from it by `degrees` instead.
    let (sin, cos) = (180.0 + degrees).to_radians().sin_cos();
    let (x, y) = if creature_target {
        let [x, y] = creature_origin()?;
        (x + 200.0 * cos, y + 200.0 * sin)
    } else {
        (0.0, 0.0)
    };
    let z = 0.125 - crate::motion::SOLE_Z;
    let (lights, sheet, still, keys) = if creature_target {
        let feats: Vec<String> = feats::JEDI_DEFENSE
            .iter()
            .take(usize::from(tier))
            .map(u16::to_string)
            .collect();
        (
            String::new(),
            if feats.is_empty() {
                String::new()
            } else {
                format!("sheet c0 feats={}", feats.join(","))
            },
            format!("put c0 200 0 {z} 180\nforce c0 stunned=100000"),
            "c0=0".to_string(),
        )
    } else {
        (
            "b1=attack".to_string(),
            String::new(),
            String::new(),
            format!("t1={degrees}"),
        )
    };
    let text = format!(
        "scenario probe\nrules advanced\nseed 7\n\
         fighter 0 pistol x={x:.3} y={y:.3} yaw={degrees} hp=400\n\
         {target} x=200 yaw=180 defence={tier} tiers=2 fp=100 hp=30000\n\
         {sheet}\nbegin\n{still}\ntick 1 {lights}\ntick 60\ntick 600 b0=attack {hold} {keys}\ntick 60 {hold} {keys}\n"
    );
    let ticks = play(&text)?;
    let them: Vec<Fighter> = if creature_target {
        ticks.iter().filter_map(creature).collect()
    } else {
        fighters(&ticks, 1)
    };
    let (first, last) = (them.first().ok_or("none")?, them.last().ok_or("none")?);
    // What deflecting paid (a bolt sent back that hurts its shooter
    // gives a little back).
    let points = |f: &Fighter| i32::from(f.saber.duel.points);
    let paid: i32 = them
        .windows(2)
        .map(|w| (points(&w[0]) - points(&w[1])).max(0))
        .sum();
    let fired: std::collections::BTreeSet<_> = ticks
        .iter()
        .flat_map(|snapshot| &snapshot.entities)
        .filter(|entity| entity.kind == Kind::Missile)
        .map(|entity| entity.id)
        .collect();
    let hit = [Outcome::Hit, Outcome::Critical, Outcome::Automatic]
        .into_iter()
        .map(|outcome| count_events(&them, outcome))
        .sum();
    Ok(Shots {
        fired: fired.len(),
        deflected: count_events(&them, Outcome::Deflected),
        hit,
        lost: first.hp - last.hp,
        paid,
    })
}

/// `shot_from` in front of a `kind` player holding `hold`, who has the first
/// two tiers of Jedi Defense.
fn shot_at(kind: &str, hold: &str) -> Result<(usize, i32, i32), String> {
    let shots = shot_from(&format!("fighter 1 {kind}"), 2, hold, 0.0)?;
    Ok((shots.deflected, shots.lost, shots.paid))
}

/// Who a bolt is shot at in `bolt_rows`.
#[derive(Debug, Clone, Copy)]
enum Target {
    /// A player with a lightsaber, holding block.
    Player,
    /// A player with a vibroblade, holding block.
    Vibroblade,
    /// A creature with a lightsaber.
    Creature,
}

/// What a bolt does in a row of `bolt_rows`.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Fate {
    /// Every bolt is deflected.
    Deflected,
    /// Every bolt lands on the target.
    Hit,
}

/// Every row of `(who, Jedi Defense tier, degrees off where they face, what
/// the rules ask)` played. A row holds only when every bolt fired is
/// accounted for as a deflection or a hit, so a bolt that vanishes fails it,
/// and every bolt does what the rules ask.
fn bolt_rows(rows: &[(Target, u8, i32, Fate)]) -> Result<(bool, String), String> {
    let mut right = true;
    let mut said = Vec::new();
    for &(who, tier, degrees, want) in rows {
        let (line, hold) = match who {
            Target::Player => ("fighter 1 medium", "h1=block"),
            Target::Vibroblade => ("fighter 1 vibroblade", "h1=block"),
            Target::Creature => ("creature medium", ""),
        };
        let shots = shot_from(line, tier, hold, degrees as f32)?;
        let Shots {
            fired,
            deflected,
            hit,
            ..
        } = shots;
        let did = match want {
            Fate::Deflected => deflected > 0,
            Fate::Hit => deflected == 0 && hit == fired,
        };
        right &= fired > 0 && fired == deflected + hit && did;
        said.push(format!(
            "{who:?} at tier {tier}, {degrees} degrees: of {fired} bolts {deflected} deflected \
             for {} Stamina and {hit} hit, {} health lost, the rules ask {want:?}",
            shots.paid, shots.lost
        ));
    }
    Ok((right, said.join("; ")))
}

/// Whether a shooter `degrees` off where a fighter faces lies in the arc of
/// Jedi Defense `tier`.
fn arc_at((tier, degrees): (u8, i32)) -> bool {
    let (sin, cos) = (degrees as f32).to_radians().sin_cos();
    shots::in_arc(tier, [1.0, 0.0, 0.0], [cos, sin, 0.0])
}

/// Each tier's arc, to either side: just inside, exactly on the edge and just
/// outside it; straight behind is outside every tier, and no feat covers
/// nothing.
fn arc_by_tier() -> Result<String, String> {
    let mut rows = vec![((0, 0), false)];
    for (tier, edge) in [(1u8, 60), (2, 90), (3, 120)] {
        for side in [1, -1] {
            rows.extend([
                ((tier, side * (edge - 1)), true),
                ((tier, side * edge), true),
                ((tier, side * (edge + 1)), false),
            ]);
        }
        rows.push(((tier, 180), false));
    }
    let (right, said) = rule_rows(&rows, arc_at);
    check(right, format!("advanced: {said}"))
}

/// What a deflected bolt of a normal cost costs a blocker, by feat tier and
/// weapon deflection bonus.
fn bolt_price_by_tier() -> Result<String, String> {
    let rows = [
        ((20, 1, 0), 17),
        ((20, 3, 0), 11),
        ((20, 3, 3), 8),
        ((20, 3, 20), 5),
        ((1, 3, 0), 1),
    ];
    let (right, said) = rule_rows(&rows, |(cost, tier, deflect)| {
        shots::bolt_price(cost, tier, deflect)
    });
    check(right, format!("advanced: {said}"))
}

/// A lightsaber in hand without the Jedi Defense feat deflects nothing, even
/// with block held.
fn no_feat_no_deflection() -> Result<String, String> {
    let (right, said) = bolt_rows(&[(Target::Player, 0, 0, Fate::Hit)])?;
    check(right, format!("advanced: {said}"))
}

/// A vibroblade parries blows but never deflects a bolt, whatever the feat.
fn vibroblade_never_deflects() -> Result<String, String> {
    let (right, said) = bolt_rows(&[(Target::Vibroblade, 3, 0, Fate::Hit)])?;
    check(right, format!("advanced: {said}"))
}

/// A bolt from straight behind hits every tier, for a player and a creature.
/// A player with the third tier still deflects one from 100 degrees.
fn bolt_from_behind_hits() -> Result<String, String> {
    let mut rows = vec![(Target::Player, 3, 100, Fate::Deflected)];
    for who in [Target::Player, Target::Creature] {
        rows.extend((1..=3).map(|tier| (who, tier, 180, Fate::Hit)));
    }
    let (right, said) = bolt_rows(&rows)?;
    check(right, format!("advanced: {said}"))
}

/// A creature with the first tier is hit by a bolt from 80 degrees and
/// deflects one from 50, and the second tier deflects the one from 80.
fn creature_side_shot() -> Result<String, String> {
    let rows = [
        (Target::Creature, 1, -80, Fate::Hit),
        (Target::Creature, 1, 80, Fate::Hit),
        (Target::Creature, 1, 50, Fate::Deflected),
        (Target::Creature, 2, 80, Fate::Deflected),
    ];
    let (right, said) = bolt_rows(&rows)?;
    check(right, format!("advanced: {said}"))
}

/// A player with the first tier is hit by a bolt from 80 degrees and
/// deflects one from 50. A player with the second tier deflects one from 80
/// and is hit by one from 100.
fn player_side_shot() -> Result<String, String> {
    let rows = [
        (Target::Player, 1, 80, Fate::Hit),
        (Target::Player, 1, -50, Fate::Deflected),
        (Target::Player, 2, -80, Fate::Deflected),
        (Target::Player, 2, 100, Fate::Hit),
    ];
    let (right, said) = bolt_rows(&rows)?;
    check(right, format!("advanced: {said}"))
}

/// The way to a shooter that is not known is the way the bolt came from: a
/// bolt flying toward -x came from +x.
fn unknown_shooter_way() -> Result<String, String> {
    let rows = [
        (
            (Some([300.0, 0.0, 0.0]), [-900.0, 0.0, 0.0]),
            [1.0, 0.0, 0.0],
        ),
        ((None, [-900.0, 0.0, 0.0]), [1.0, 0.0, 0.0]),
        ((None, [0.0, 450.0, 0.0]), [0.0, -1.0, 0.0]),
    ];
    let (right, said) = rule_rows(&rows, |(shooter, velocity)| {
        let mut way = shots::to_shooter([0.0; 3], shooter, velocity);
        crate::math::normalize(&mut way);
        way
    });
    check(right, format!("advanced: {said}"))
}

fn bolts() -> Result<String, String> {
    let (open, guarded) = (shot_at("medium", "")?, shot_at("medium", "h1=block")?);
    check(
        guarded.0 > open.0 && guarded.1 < open.1 && guarded.2 > 0,
        format!(
            "advanced: without block {} deflected, {} health lost; holding block {} deflected, {} \
             health lost, {} block points paid",
            open.0, open.1, guarded.0, guarded.1, guarded.2
        ),
    )
}

/// A player's vibroblade under MDA: held in block it parries a saber's
/// blows from its block points, as a lightsaber does; held against a
/// pistol it turns no bolt (only lightsabers do).
fn vibroblade_blocks() -> Result<String, String> {
    let then = "tick 2000 b0=attack h1=block";
    let blocker = fighters(
        &play(&duel_of("advanced", ["medium", "vibroblade"], "", then))?,
        1,
    );
    let parried = count_events(&blocker, Outcome::Parried);
    let points = (
        blocker.first().ok_or("no ticks")?.saber.duel.points,
        blocker.last().ok_or("no ticks")?.saber.duel.points,
    );
    let (deflected, lost, _) = shot_at("vibroblade", "h1=block")?;
    check(
        parried > 0 && points.1 < points.0 && deflected == 0 && lost > 0,
        format!(
            "advanced: the vibroblade parries {parried} blows, block points {} -> {}; \
             against bolts {deflected} deflected, {lost} health lost",
            points.0, points.1
        ),
    )
}

/// Seat 0's kick key against seat 1 under MDA: floored with its saber out.
fn kicked() -> Result<String, String> {
    let ticks = play(&duel("advanced", "", "tick 150\ntick 1 h0=kick\ntick 200"))?;
    let victim = fighters(&ticks, 1);
    let floored = victim.iter().position(|f| f.saber.floored());
    let unlit = victim.iter().any(|f| !f.saber.on);
    check(
        floored.is_some() && unlit,
        format!(
            "advanced: the kick floors seat 1 at tick {} and its saber goes out ({unlit})",
            floored.map_or("never".into(), |at| at.to_string())
        ),
    )
}

/// The most a blocker's hilt wore over `ticks`, how often it parried, and
/// the tick it broke.
fn wore(ticks: &[Fighter]) -> (u16, usize, Option<usize>) {
    let worn = ticks.iter().map(|f| f.saber.duel.wear).max().unwrap_or(0);
    let broke = ticks.iter().position(|f| f.saber.duel.broken);
    (worn, count_events(ticks, Outcome::Parried), broke)
}

/// A strong-style attacker against a creature's staff already worn to 90
/// (MDA): the hilt wears through and breaks, and fights on with one blade.
fn staff_breaks() -> Result<String, String> {
    let text = "scenario probe\nrules advanced\nseed 7\n\
                fighter 0 strong x=0 defence=2 tiers=2 fp=100 hp=400\n\
                creature staff x=45 yaw=180 defence=2 tiers=2 fp=100 hp=400\n\
                begin\nduel c0 wear=90\ntick 9000 b0=attack c0=0\n";
    let them: Vec<Fighter> = play(text)?.iter().filter_map(creature).collect();
    let (worn, parried, broke) = wore(&them);
    check(
        broke.is_some(),
        format!(
            "advanced: the creature's staff wears from 90 to {worn} over {parried} parries, breaking at tick {}",
            broke.map_or("never".into(), |at| at.to_string())
        ),
    )
}

/// The same against a player's staff held in block: a player's hilt never
/// wears (`duel.wear.who`, creatures' only).
fn player_staff_holds() -> Result<String, String> {
    let then = "duel 1 wear=90\ntick 3000 b0=attack h1=block";
    let blocker = fighters(
        &play(&duel_of("advanced", ["strong", "staff"], "", then))?,
        1,
    );
    let (worn, parried, broke) = wore(&blocker);
    check(
        parried > 0 && worn == 90 && broke.is_none(),
        format!(
            "advanced: the player's staff parries {parried} times and stays at {worn}, broken {}",
            broke.is_some()
        ),
    )
}

/// Both seats swinging at each other with equal power under `rules`, over
/// several seeds: a saber lock starts (the setting's chance on each clash
/// that tries one).
fn locks(rules: &str) -> Result<String, String> {
    let (mut clashes, mut tried) = (0, 0);
    for seed in 1..=LOCK_SEEDS {
        tried = seed;
        let more = format!("seed {seed}");
        let ticks = play(&duel(rules, &more, "tick 3000 b0=attack b1=attack"))?;
        let (one, two) = (fighters(&ticks, 0), fighters(&ticks, 1));
        clashes += count_events(&one, Outcome::Parried);
        if let Some(at) = one
            .iter()
            .zip(&two)
            .position(|(a, b)| a.saber.lock.is_some() && b.saber.lock.is_some())
        {
            return check(
                true,
                format!("{rules}: locked at tick {at} of seed {seed}, after {clashes} blows met"),
            );
        }
    }
    check(
        false,
        format!("{rules}: {clashes} blows met over {tried} seeds, never a lock"),
    )
}

/// Seat 0 running forward for `RUN_TICKS` with block held, and again
/// without it, its foe far away: the held block counts as held on every
/// tick its legs run, and the run covers the same ground (no sprint).
fn runs_blocking() -> Result<String, String> {
    let run = |hold: &str| -> Result<(f32, usize, usize), String> {
        let script = format!(
            "scenario probe\nrules advanced\nseed 7\n\
             fighter 0 medium x=0 defence=2 tiers=2 fp=100 hp=400\n\
             fighter 1 medium x=4000 yaw=180 defence=2 tiers=2 fp=100 hp=400\n\
             begin\ntick 1 b0=attack b1=attack\ntick 60\ntick {RUN_TICKS} f0=127 {hold}\n"
        );
        let ticks = play(&script)?;
        let runner: Vec<_> = ticks[SETTLED..]
            .iter()
            .filter_map(|tick| tick.own(Seat(0)))
            .collect();
        let at = |entity: Option<&&crate::snapshot::EntityState>| {
            entity
                .and_then(|entity| entity.player.as_ref())
                .map(|ps| ps.origin)
                .ok_or("no ticks")
        };
        let (first, last) = (at(runner.first())?, at(runner.last())?);
        let running = runner.iter().filter(|entity| {
            entity
                .player
                .as_ref()
                .is_some_and(|ps| crate::motion::anim::is_run(ps.legs.anim))
        });
        let held = running
            .clone()
            .filter(|entity| entity.fighter.is_some_and(|f| f.saber.duel.block_held))
            .count();
        let covered = crate::math::flat_length(crate::math::sub(last, first));
        Ok((covered, running.count(), held))
    };
    let (plain, _, _) = run("")?;
    let (blocking, ran, held) = run("h0=block")?;
    check(
        ran > 0 && held == ran && (blocking - plain).abs() <= plain * 0.01,
        format!(
            "advanced: running {ran} ticks with block held, held on {held}; {blocking:.0} units \
             covered, {plain:.0} without block"
        ),
    )
}

/// The ticks `runs_blocking` runs for.
const RUN_TICKS: usize = 120;

/// The seeds `locks` tries.
const LOCK_SEEDS: u64 = 120;

/// The most Stamina a character can hold, by their level
/// (`standing::cap`).
fn cap_by_level() -> Result<String, String> {
    use crate::combat::saber::duel::standing::cap;
    let rows = [(0u8, 60u8), (1, 62), (20, 100), (50, 100)];
    let (right, said) = rule_rows(&rows, cap);
    check(
        right,
        format!("advanced: the maximum Stamina by level, {said}"),
    )
}

/// How long a character waits for Stamina to come back, by their
/// Constitution modifier (`standing::refill_wait`, thousandths of the
/// usual wait).
fn refill_by_constitution() -> Result<String, String> {
    use crate::combat::saber::duel::standing::refill_wait;
    let rows = [
        (0i32, 1000i64),
        (4, 800),
        (-5, 1250),
        (30, 500),
        (-30, 1500),
    ];
    let (right, said) = rule_rows(&rows, refill_wait);
    check(
        right,
        format!("advanced: the refill wait in thousandths by Constitution modifier, {said}"),
    )
}

/// How long the perfect block window stays open, by the difficulty's own
/// window and the character's Reflex save (`standing::window_ms`).
fn window_by_reflex() -> Result<String, String> {
    use crate::combat::saber::duel::standing::window_ms;
    let rows = [
        ((150u64, 0i32), 150u64),
        ((150, 10), 200),
        ((50, -20), 50),
        ((250, 100), 400),
    ];
    let (right, said) = rule_rows(&rows, |(base, reflex)| window_ms(base, reflex));
    check(
        right,
        format!("advanced: the perfect block window in ms by base and Reflex save, {said}"),
    )
}

/// A critical hit against a block takes twice the Stamina
/// (`block::CRITICAL_TIMES`, applied after the rounding).
fn critical_doubles_the_cost() -> Result<String, String> {
    use crate::combat::saber::duel::exchange::blocked_cost;
    let plain = blocked_cost(10, 500, false);
    let critical = blocked_cost(10, 500, true);
    check(
        critical == 2 * plain,
        format!(
            "advanced: a block of 10 Stamina at an even chance costs {plain}, and {critical} when \
             the swing is a critical hit"
        ),
    )
}

/// A fighter whose Stamina stands above the maximum their level allows is
/// held down to it on the next step, and never shows a bar over the full
/// one. A seat's made-up fighter is level 1, so its maximum is 62.
fn points_clamped_to_cap() -> Result<String, String> {
    use crate::combat::saber::duel::standing;
    let then = "tick 200\nduel 1 points=100\ntick 1";
    let ticks = play(&duel("advanced", "", then))?;
    let blocker = fighters(&ticks, 1);
    let last = meter(blocker.last().ok_or("no ticks")?);
    let want = i32::from(standing::cap(1));
    check(
        last == want,
        format!(
            "advanced: a level 1 fighter set to 100 Stamina stood at {last} after one step; its \
             maximum is {want}"
        ),
    )
}

/// What the Stamina of a player holding block fell by over one fight, and
/// how many blows it parried, with the attacker at `level` levels.
fn drain_from_level(level: u8) -> Result<(i32, usize), String> {
    let more = format!("sheet 0 level={level}");
    let then = "duel 1 points=100\ntick 600 b0=attack h1=block";
    let blocker = fighters(&play(&duel("advanced", &more, then))?, 1);
    Ok((paid(&blocker), count_events(&blocker, Outcome::Parried)))
}

/// The attacker's attack bonus against the blocker's Defense decides what
/// a blocked swing costs: the same blocker loses more Stamina to a level 20
/// attacker than to a level 1 attacker.
fn strong_attacker_drains_more() -> Result<String, String> {
    let (weak, weak_blocks) = drain_from_level(1)?;
    let (strong, strong_blocks) = drain_from_level(20)?;
    check(
        strong > weak,
        format!(
            "advanced: a level 1 attacker took {weak} Stamina over {weak_blocks} parries, a level \
             20 attacker took {strong} over {strong_blocks}"
        ),
    )
}

/// What a vibroblade blocker's Stamina fell by over one fight, and how many
/// blows it parried, holding the melee proficiency feat or not.
fn drain_without_proficiency(proficient: bool) -> Result<(i32, usize), String> {
    let text = format!(
        "scenario probe\nrules advanced\nseed 7\n\
         fighter 0 medium x=0 defence=2 tiers=2 fp=100 hp=400\n\
         fighter 1 vibroblade x=45 yaw=180 defence=2 tiers=2 fp=100 hp=400 proficient={}\n\
         begin\ntick 1 b0=attack b1=attack\ntick 60\n\
         duel 1 points=100\ntick 600 b0=attack h1=block\n",
        u8::from(proficient)
    );
    let blocker = fighters(&play(&text)?, 1);
    Ok((paid(&blocker), count_events(&blocker, Outcome::Parried)))
}

/// A character with no proficiency feat for the weapon in their hand still
/// blocks with it, and counts their Defense `sheet::NOT_PROFICIENT` points
/// lower, so every block costs them more Stamina.
fn unproficient_blocks_at_a_penalty() -> Result<String, String> {
    let (trained, trained_blocks) = drain_without_proficiency(true)?;
    let (untrained, untrained_blocks) = drain_without_proficiency(false)?;
    check(
        untrained_blocks > 0 && untrained > trained,
        format!(
            "advanced: a vibroblade blocker with the melee proficiency feat parried \
             {trained_blocks} blows for {trained} Stamina; one without it parried \
             {untrained_blocks} for {untrained}"
        ),
    )
}

/// A lightsaber user with no Jedi Defense feat blocks a swing: the feat no
/// longer decides whether the block button works.
fn saber_blocks_without_jedi_defense() -> Result<String, String> {
    let text = "scenario probe\nrules advanced\nseed 7\n\
                fighter 0 medium x=0 defence=2 tiers=2 fp=100 hp=400\n\
                fighter 1 medium x=45 yaw=180 defence=0 tiers=2 fp=100 hp=400\n\
                begin\ntick 1 b0=attack b1=attack\ntick 60\n\
                tick 600 b0=attack h1=block\n";
    let (ticks, met) = played(text)?;
    let blocker = fighters(&ticks, 1);
    let parried = count_events(&blocker, Outcome::Parried);
    let stopped = met.iter().filter(|met| met.stopped).count();
    check(
        parried > 0 && stopped > 0,
        format!(
            "advanced: a lightsaber user with no Jedi Defense feat parried {parried} blows and \
             stopped {stopped} of them"
        ),
    )
}

/// A creature parries every swing its block can pay for: the rule that it
/// parried only the swings KOTOR's attack roll would have missed is gone,
/// so an attacker certain to hit (a chance of `margin::MOST`) is still
/// stopped. The creature is level 20, so its maximum Stamina is 100.
fn creature_parry_needs_no_miss_roll() -> Result<String, String> {
    use crate::Seat;
    use crate::combat::Sheet;
    use crate::combat::saber::duel::standing;
    let text = "scenario probe\nrules advanced\nseed 7\n\
                fighter 0 medium x=0 defence=2 tiers=2 fp=100 hp=400\n\
                creature medium x=45 yaw=180 defence=2 tiers=2 fp=100 hp=400\n\
                sheet 0 level=20\nsheet c0 level=20\n\
                begin\ntick 1 b0=attack\ntick 60\ntick 600 b0=attack c0=0\n";
    let (ticks, met) = played(text)?;
    let first = ticks.first().ok_or("no ticks")?;
    let attacker = first
        .own(Seat(0))
        .and_then(|entity| entity.sheet)
        .ok_or("no attacker sheet")?;
    let weapon = attacker.weapons[0].unwrap_or(attacker.main_weapon());
    // A made-up creature's sheet is a commoner's with its feats, so a
    // commoner stands for it here.
    let blocker = Sheet::commoner();
    let chance = standing::block_chance(&attacker, &weapon, &blocker, &weapon);
    let stopped = met
        .iter()
        .filter(|met| met.stopped && met.guard == Guard::Npc)
        .count();
    let points = ticks
        .iter()
        .filter_map(creature)
        .map(|f| i32::from(f.saber.duel.points))
        .max()
        .unwrap_or(0);
    check(
        stopped > 0 && chance == rules::combat::margin::MOST,
        format!(
            "advanced: the level 20 attacker's chance to hit is {chance} in a thousand; the \
             creature held {points} Stamina and stopped {stopped} of its swings"
        ),
    )
}

/// Under JA the block and kick keys change nothing: the meters stay fresh
/// and seat 1 is harmed as without them.
fn ja_still() -> Result<String, String> {
    let fight = |hold: &str| -> Result<Vec<Fighter>, String> {
        let then = format!("tick 600 b0=attack {hold}");
        Ok(fighters(&play(&duel("ja", "", &then))?, 1))
    };
    let (plain, held) = (fight("")?, fight("h1=block+kick")?);
    let fresh = held
        .iter()
        .all(|f| f.saber.duel == crate::combat::saber::duel::Duel::FRESH);
    let same = plain
        .iter()
        .zip(&held)
        .all(|(a, b)| a.hp == b.hp && a.saber.current_move == b.saber.current_move);
    check(
        fresh && same,
        format!(
            "ja: meters fresh {fresh}; seat 1 the same every tick of {} ({} health at the end)",
            plain.len(),
            plain.last().map_or(0, |f| f.hp)
        ),
    )
}

pub fn probes() -> Checks {
    vec![
        ("perfect_block", perfect()),
        (
            "perfect_disarms_only_at_zero",
            perfect_disarms_only_at_zero(),
        ),
        ("held_block", held()),
        (
            "blocked_swing_costs_attacker",
            blocked_swing_costs_attacker(),
        ),
        (
            "npc_pays_for_a_blocked_swing",
            npc_pays_for_a_blocked_swing(),
        ),
        ("tired_attacker_staggers", tired_attacker_staggers()),
        ("guard_breaks_at_zero", guard_breaks_at_zero()),
        ("same_fight_same_result", same_fight_same_result()),
        ("old_duel_state_loads", old_duel_state_loads()),
        ("swing_charges_creatures", swing_charges_creatures()),
        ("force_does_not_stop_a_block", force_does_not_stop_a_block()),
        ("tired_swings_are_slow", tired_swings_are_slow()),
        ("meters_refill", refill()),
        ("creature_blocks", creature_blocks()),
        ("bolts_need_block", bolts()),
        ("vibroblade_blocks", vibroblade_blocks()),
        ("kick_floors", kicked()),
        ("staff_breaks", staff_breaks()),
        ("player_staff_holds", player_staff_holds()),
        ("running_block", runs_blocking()),
        ("locks_advanced", locks("advanced")),
        ("cap_by_level", cap_by_level()),
        ("refill_by_constitution", refill_by_constitution()),
        ("window_by_reflex", window_by_reflex()),
        ("critical_doubles_the_cost", critical_doubles_the_cost()),
        ("points_clamped_to_cap", points_clamped_to_cap()),
        ("strong_attacker_drains_more", strong_attacker_drains_more()),
        (
            "unproficient_blocks_at_a_penalty",
            unproficient_blocks_at_a_penalty(),
        ),
        (
            "saber_blocks_without_jedi_defense",
            saber_blocks_without_jedi_defense(),
        ),
        (
            "creature_parry_needs_no_miss_roll",
            creature_parry_needs_no_miss_roll(),
        ),
        ("arc_by_tier", arc_by_tier()),
        ("bolt_price_by_tier", bolt_price_by_tier()),
        ("no_feat_no_deflection", no_feat_no_deflection()),
        ("vibroblade_never_deflects", vibroblade_never_deflects()),
        ("bolt_from_behind_hits", bolt_from_behind_hits()),
        ("creature_side_shot", creature_side_shot()),
        ("player_side_shot", player_side_shot()),
        ("unknown_shooter_way", unknown_shooter_way()),
        ("ja_unchanged", ja_still()),
    ]
}
