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
use crate::combat::saber::duel::tap::{self, Tapped};
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
                took,
                perfect,
                stopped,
                guard,
                ..
            } => Some(Met {
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
    duel("advanced", "", &spells)
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

/// What a perfectly blocked attacker who held `points` Stamina plays.
fn perfect_took(points: i32) -> Result<Took, String> {
    let (_, met) = played(&perfect_fight(points))?;
    met.iter()
        .find(|met| met.perfect)
        .map(|met| met.took[0])
        .ok_or_else(|| format!("no perfect block in 640 ticks from {points} Stamina"))
}

/// A perfect block disarms its attacker only when the cost leaves the
/// attacker with no Stamina. Otherwise it heavily staggers them.
fn perfect_disarms_only_at_zero() -> Result<String, String> {
    let bands = [(100, Took::HeavyStagger), (15, Took::Disarm)];
    let (right, said) = band_rows(&bands, perfect_took)?;
    check(right, format!("advanced: a perfect block, {said}"))
}

/// One swing stopped by a held block: its attacker pays for starting the
/// swing (`costs::SWING`) and for the block (`block::BLOCKED`), and for
/// nothing else.
fn blocked_swing_costs_attacker() -> Result<String, String> {
    use rules::duel::{block, costs, meters};
    let ticks = play(&duel("advanced", "", "tick 600 b0=attack h1=block"))?;
    let points: Vec<i32> = fighters(&ticks, 0).iter().map(meter).collect();
    let full = i32::from(meters::POINTS_MAX);
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
        (25, Took::HeavyStagger),
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
            "mda: {blocks} blows blocked; block points {} -> {}, Force (hundredths) {} -> {}",
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
    let text = "scenario probe\nrules mda\nseed 7\n\
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

/// A pistol shooting seat 1, a `kind` fighter holding `hold`, under MDA:
/// the bolts it deflected, the health it lost and the block points it
/// paid.
fn shot_at(kind: &str, hold: &str) -> Result<(usize, i32, i32), String> {
    let text = format!(
        "scenario probe\nrules mda\nseed 7\n\
         fighter 0 pistol x=0 hp=400\n\
         fighter 1 {kind} x=200 yaw=180 defence=2 tiers=2 fp=100 hp=400\n\
         begin\ntick 1 b1=attack\ntick 60\ntick 600 b0=attack {hold}\n"
    );
    let ticks = play(&text)?;
    let them = fighters(&ticks, 1);
    let (first, last) = (them.first().ok_or("none")?, them.last().ok_or("none")?);
    // What deflecting paid (a bolt sent back that hurts its shooter
    // gives a little back).
    let points = |f: &Fighter| i32::from(f.saber.duel.points);
    let paid: i32 = them
        .windows(2)
        .map(|w| (points(&w[0]) - points(&w[1])).max(0))
        .sum();
    Ok((
        count_events(&them, Outcome::Deflected),
        first.hp - last.hp,
        paid,
    ))
}

fn bolts() -> Result<String, String> {
    let (open, guarded) = (shot_at("medium", "")?, shot_at("medium", "h1=block")?);
    check(
        guarded.0 > open.0 && guarded.1 < open.1 && guarded.2 > 0,
        format!(
            "mda: without block {} deflected, {} health lost; holding block {} deflected, {} \
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
            "mda: the vibroblade parries {parried} blows, block points {} -> {}; \
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
            "mda: the kick floors seat 1 at tick {} and its saber goes out ({unlit})",
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
    let text = "scenario probe\nrules mda\nseed 7\n\
                fighter 0 strong x=0 defence=2 tiers=2 fp=100 hp=400\n\
                creature staff x=45 yaw=180 defence=2 tiers=2 fp=100 hp=400\n\
                begin\nduel c0 wear=90\ntick 9000 b0=attack c0=0\n";
    let them: Vec<Fighter> = play(text)?.iter().filter_map(creature).collect();
    let (worn, parried, broke) = wore(&them);
    check(
        broke.is_some(),
        format!(
            "mda: the creature's staff wears from 90 to {worn} over {parried} parries, breaking at tick {}",
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
            "mda: the player's staff parries {parried} times and stays at {worn}, broken {}",
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
            "scenario probe\nrules mda\nseed 7\n\
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
            "mda: running {ran} ticks with block held, held on {held}; {blocking:.0} units \
             covered, {plain:.0} without block"
        ),
    )
}

/// The ticks `runs_blocking` runs for.
const RUN_TICKS: usize = 120;

/// The seeds `locks` tries.
const LOCK_SEEDS: u64 = 30;

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
        ("meters_refill", refill()),
        ("creature_blocks", creature_blocks()),
        ("bolts_need_block", bolts()),
        ("vibroblade_blocks", vibroblade_blocks()),
        ("kick_floors", kicked()),
        ("staff_breaks", staff_breaks()),
        ("player_staff_holds", player_staff_holds()),
        ("running_block", runs_blocking()),
        ("locks_mda", locks("advanced")),
        ("ja_unchanged", ja_still()),
    ]
}
