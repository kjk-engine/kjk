//! The probes of how fighters pace and spend a fight under the Advanced
//! rules (`kjk sim probes --only duelling`). Special moves use up Stamina,
//! for every character, and a Force user's Force points are left for the
//! Force powers; under Jedi Academy's rules the older bar still pays.

use super::duels::play;
use crate::Seat;
use crate::combat::Fighter;
use crate::combat::pace::ai_may_special;
use crate::combat::saber::duel::standing::{cap, may_start_special, special_cost};
use crate::combat::sheet::Sheet;
use crate::probe::{Checks, check};
use crate::snapshot::Snapshot;

/// Seat 0's level in the fights below, and so its Stamina maximum.
const LEVEL: u8 = 5;
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

pub fn probes() -> Checks {
    vec![
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
    ]
}
