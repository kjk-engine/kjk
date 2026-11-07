//! The duel modes' probes (`kjk sim probes --only "duel rules"`): duels of
//! the `m6` trace's made-up fighters (`duels::play`) under MD and MDA, each
//! check saying the numbers it saw. A perfect block refills its blocker and
//! staggers its attacker; a held block drains the block points (MDA) or the
//! Force (MD); a high mishap count ends in a stagger and in a disarm (past
//! the normal difficulty's guard); the meters refill and the mishaps fall,
//! the Force slower with block held; a creature blocks of itself; a player
//! turns bolts only holding block; a kick floors one not absorbing it; a
//! worn staff breaks; two swings meeting lock; and under JA the block and
//! kick keys change nothing.

use super::duels::play;
use crate::Seat;
use crate::combat::Fighter;
use crate::combat::fighter::Outcome;
use crate::combat::saber::Move;
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

/// A fighter's meter in points: block points under MDA, else its Force
/// in points of its 100.
fn meter(f: &Fighter, mda: bool) -> i32 {
    if mda {
        i32::from(f.saber.duel.points)
    } else {
        f.fp / 100
    }
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

/// A perfect block under `rules` (block held, attack pressed afresh more
/// often than the window lasts, so one is open when seat 0's swing
/// lands): the blocker's meter and the attacker's mishaps around it, and
/// what the attacker plays.
fn perfect(rules: &str) -> Result<String, String> {
    let mda = rules == "mda";
    // The blocker's meters at half, so its reward shows.
    let spells = format!(
        "duel 1 points=50\nstat 1 fp 5000\n{}\n",
        times(
            "tick 15 b0=attack h1=block b1=attack\ntick 1 b0=attack h1=block",
            40
        )
    );
    let ticks = play(&duel(rules, "", &spells))?;
    let (blocker, attacker) = (fighters(&ticks, 1), fighters(&ticks, 0));
    let at = (1..blocker.len())
        .find(|&i| blocker[i].saber.duel.perfect_at != blocker[i - 1].saber.duel.perfect_at)
        .ok_or("no perfect block in 640 ticks")?;
    let (before, after) = (meter(&blocker[at - 1], mda), meter(&blocker[at], mda));
    let (calm, upset) = (
        attacker[at - 1].saber.duel.mishap,
        attacker[at].saber.duel.mishap,
    );
    let (paid_from, paid_to) = (meter(&attacker[at - 1], mda), meter(&attacker[at], mda));
    // MDA alone opens the blocker's riposte window (`Duel::riposte_until`).
    let riposte = blocker[at].saber.duel.riposte_until > blocker[at - 1].saber.duel.riposte_until;
    let reeling = attacker[at..(at + 3).min(attacker.len())]
        .iter()
        .find(|f| f.saber.current_move.broken() || f.saber.current_move.bounce())
        .map(|f| f.saber.current_move);
    check(
        after > before
            && upset > calm
            && paid_to < paid_from
            && reeling.is_some()
            && riposte == mda,
        format!(
            "{rules}: perfect block at tick {at}: blocker's meter {before} -> {after}, \
             attacker's {paid_from} -> {paid_to}, its mishaps {calm} -> {upset}, it plays {}, \
             riposte {riposte}",
            reeling.map_or("nothing", Move::name)
        ),
    )
}

/// Block held alone against seat 0's swings: what it pays, from which
/// pool.
fn held(rules: &str) -> Result<String, String> {
    let mda = rules == "mda";
    let ticks = play(&duel(rules, "", "tick 2000 b0=attack h1=block"))?;
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
    let drained = if mda {
        points.1 < points.0
    } else {
        force.1 < force.0 && points.1 == points.0
    };
    check(
        drained && blocks > 0,
        format!(
            "{rules}: {blocks} blows blocked; block points {} -> {}, Force (hundredths) {} -> {}",
            points.0, points.1, force.0, force.1
        ),
    )
}

/// Seat 0 swinging at seat 1's held block with `mishaps` already: the
/// first tick it is `wanted` (a stagger, a disarm), and when.
fn mishap(mishaps: u8, ticks: usize, wanted: fn(&Fighter) -> bool) -> Result<(usize, u8), String> {
    let then = format!("duel 0 mishap={mishaps}\ntick {ticks} b0=attack h1=block");
    let played = play(&duel("mda", "", &then))?;
    let attacker = fighters(&played, 0);
    attacker
        .iter()
        .position(wanted)
        .map(|at| (at, attacker[at].saber.duel.mishap))
        .ok_or_else(|| format!("not in {ticks} ticks"))
}

fn staggered() -> Result<String, String> {
    let (at, count) = mishap(10, 600, |f| {
        matches!(
            f.saber.current_move,
            Move::H1_T_ | Move::H1_TR | Move::H1_TL | Move::H1_BL | Move::H1_B_ | Move::H1_BR
        )
    })?;
    check(
        count >= rules::duel::ladder::RUNGS[1],
        format!(
            "mda: from 10 mishaps the attacker is heavily staggered at tick {at}, with {count}"
        ),
    )
}

fn disarmed() -> Result<String, String> {
    let (at, count) = mishap(15, 4000, |f| f.saber.thrown.is_some())?;
    check(
        count >= rules::duel::ladder::RUNGS[2] - 3,
        format!(
            "mda: at the top rung the attacker loses its saber at tick {at} (the normal \
             difficulty's guard refusing the first), with {count} mishaps"
        ),
    )
}

/// The meters left standing: MDA's block points from 40 and mishaps from
/// 8 over ten seconds; MD's Force from 30 points over five, with block
/// held and without.
fn refill() -> Result<String, String> {
    let rested = play(&duel("mda", "", "duel 1 points=40 mishap=8\ntick 1250"))?;
    let blocker = fighters(&rested, 1);
    let (first, last) = (
        blocker.get(SETTLED).ok_or("no ticks")?,
        blocker.last().ok_or("no ticks")?,
    );
    let points = (first.saber.duel.points, last.saber.duel.points);
    let mishaps = (first.saber.duel.mishap, last.saber.duel.mishap);
    let force = |hold: &str| -> Result<(i32, i32), String> {
        let then = format!("stat 1 fp 3000\ntick 625 {hold}");
        let ticks = play(&duel("md", "", &then))?;
        let f = fighters(&ticks, 1);
        Ok((f.get(SETTLED).ok_or("none")?.fp, f.last().ok_or("none")?.fp))
    };
    let (free, holding) = (force("")?, force("h1=block")?);
    let gained = |(from, to): (i32, i32)| to - from;
    check(
        points.1 > points.0 && mishaps.1 < mishaps.0 && gained(free) > gained(holding),
        format!(
            "mda: block points {} -> {}, mishaps {} -> {} in 10 s; md: Force (hundredths) \
             {} -> {} in 5 s, {} -> {} holding block",
            points.0, points.1, mishaps.0, mishaps.1, free.0, free.1, holding.0, holding.1
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
            "mda: the creature blocked {blocks} times; its block points {} -> {}, mishaps {} -> {}",
            first.saber.duel.points,
            last.saber.duel.points,
            first.saber.duel.mishap,
            last.saber.duel.mishap
        ),
    )
}

/// A pistol firing at a lit saber under MD: what lands with block held and
/// without.
fn bolts() -> Result<String, String> {
    let shoot = |hold: &str| -> Result<(usize, i32, i32), String> {
        let text = format!(
            "scenario probe\nrules md\nseed 7\n\
             fighter 0 pistol x=0 hp=400\n\
             fighter 1 medium x=200 yaw=180 defence=2 tiers=2 fp=100 hp=400\n\
             begin\ntick 1 b1=attack\ntick 60\ntick 600 b0=attack {hold}\n"
        );
        let ticks = play(&text)?;
        let them = fighters(&ticks, 1);
        let (first, last) = (them.first().ok_or("none")?, them.last().ok_or("none")?);
        // What deflecting paid (a bolt sent back that hurts its shooter
        // gives a little back).
        let paid: i32 = them.windows(2).map(|w| (w[0].fp - w[1].fp).max(0)).sum();
        Ok((
            count_events(&them, Outcome::Deflected),
            first.hp - last.hp,
            paid,
        ))
    };
    let (open, guarded) = (shoot("")?, shoot("h1=block")?);
    check(
        guarded.0 > open.0 && guarded.1 < open.1 && guarded.2 > 0,
        format!(
            "md: without block {} deflected, {} health lost; holding block {} deflected, {} \
             health lost, {} Force paid (hundredths)",
            open.0, open.1, guarded.0, guarded.1, guarded.2
        ),
    )
}

/// Seat 0's kick key against seat 1 under MD: floored with its saber out.
fn kicked() -> Result<String, String> {
    let ticks = play(&duel("md", "", "tick 150\ntick 1 h0=kick\ntick 200"))?;
    let victim = fighters(&ticks, 1);
    let floored = victim.iter().position(|f| f.saber.floored());
    let unlit = victim.iter().any(|f| !f.saber.on);
    check(
        floored.is_some() && unlit,
        format!(
            "md: the kick floors seat 1 at tick {} and its saber goes out ({unlit})",
            floored.map_or("never".into(), |at| at.to_string())
        ),
    )
}

/// A strong-style attacker against a staff held in block, already worn
/// to 90 (MDA): the hilt wears through and breaks, and fights on with one
/// blade.
fn staff_breaks() -> Result<String, String> {
    let then = "duel 1 wear=90\ntick 3000 b0=attack h1=block";
    let blocker = fighters(&play(&duel_of("mda", ["strong", "staff"], "", then))?, 1);
    let worn = blocker.iter().map(|f| f.saber.duel.wear).max().unwrap_or(0);
    let broke = blocker.iter().position(|f| f.saber.duel.broken);
    check(
        broke.is_some(),
        format!(
            "mda: the staff wears from 90 to {worn}, breaking at tick {}",
            broke.map_or("never".into(), |at| at.to_string())
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

/// The seeds `locks` tries.
const LOCK_SEEDS: u64 = 10;

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
        ("perfect_block_md", perfect("md")),
        ("perfect_block_mda", perfect("mda")),
        ("held_block_mda", held("mda")),
        ("held_block_md", held("md")),
        ("mishap_stagger", staggered()),
        ("mishap_disarm", disarmed()),
        ("meters_refill", refill()),
        ("creature_blocks", creature_blocks()),
        ("bolts_need_block", bolts()),
        ("kick_floors", kicked()),
        ("staff_breaks", staff_breaks()),
        ("locks_md", locks("md")),
        ("locks_mda", locks("mda")),
        ("ja_unchanged", ja_still()),
    ]
}
