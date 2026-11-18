//! Sniper Shot's balance, as the gunplay design asks for it
//! (`docs/superpowers/specs/2026-11-17-gunplay-design.md`, "Sniper Shot's
//! balance"). A soldier fires at an idle dummy for a minute with one pistol,
//! a pair of pistols or a rifle. The soldier fires plainly, charges Power
//! Blasts or steadies Sniper Shots, from the hip or with [Aim] held. The
//! dummy's Defense is set so that a plain shot hits 95%, 75% or 40% of the
//! time, and it stands 6 m or 30 m away. Each case is averaged over
//! `SEEDS` seeds.
//!
//! A feat attack is paid from the soldier's stamina. When the stamina runs
//! out, [Feat attack] fires plain shots until the stamina has come back, so
//! the damage a second of a feat attack is what a player who keeps the
//! button held deals over the whole minute. Each feat attack's case is
//! played again with the feat attack fired only while the stamina pays for
//! it, which gives what the feat alone deals for each point of stamina.
//!
//! The soldier aims at the middle of the dummy's body box, not at its
//! chest point (`mind::chest`). The hit-location rules count a hit at the
//! chest point's height on the front of the box as an arm hit, which deals
//! less damage. A shot with no spread lands there every time, while shots
//! that spread sometimes land on the head or the chest. Aiming at the
//! chest point would therefore make every accurate way of firing look
//! worse than it is.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use collision::UNIT;
use serde_json::{Value, json};
use sim::combat::sheet::{Delivery, Sheet, Weapon, feats};
use sim::combat::{damage, specials, weapons};
use sim::{Level, holds};

use super::{Bout, Keys, dummy};
use crate::saber_verbs::{ATTACK, Report};
use crate::specials_verbs::{pistol, rifle};

/// How long each case fires (seconds).
const SECONDS: u64 = 60;
const SEEDS: u64 = 8;
/// How long [Aim] is held before the minute starts, so the gun is fully
/// raised (ms). It is a little longer than the time the rule
/// `weapons.aim.in_ms` gives to raise the gun.
const RAISE_MS: u64 = 250;
/// How long the bench waits after the minute for the last bolts to land
/// (ms).
const TAIL_MS: u64 = 1500;
/// The dummy's Defense, named by a plain shot's chance to hit it
/// (thousandths).
const DEFENCES: [(&str, i64); 3] = [("low", 950), ("middle", 750), ("high", 400)];
const RANGES_M: [f32; 2] = [6.0, 30.0];
const LOW: usize = 0;
const HIGH: usize = 2;
const FAR: usize = 1;

/// What [Feat attack] does in a case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fire {
    Plain,
    PowerBlast(usize),
    SniperShot(usize),
}

/// A way of firing: plain, Power Blast or Sniper Shot, and whether [Aim]
/// is held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Way {
    fire: Fire,
    aimed: bool,
}

impl Way {
    /// Every way measured: plain fire, then each tier of Power Blast and of
    /// Sniper Shot, each from the hip and aimed.
    fn all() -> Vec<Way> {
        let fires = [Fire::Plain]
            .into_iter()
            .chain((1..=3).map(Fire::PowerBlast))
            .chain((1..=3).map(Fire::SniperShot));
        fires
            .flat_map(|fire| [false, true].map(|aimed| Way { fire, aimed }))
            .collect()
    }

    fn name(self) -> String {
        let fire = match self.fire {
            Fire::Plain => "plain".to_string(),
            Fire::PowerBlast(tier) => format!("Power Blast {tier}"),
            Fire::SniperShot(tier) => format!("Sniper Shot {tier}"),
        };
        let how = if self.aimed { "aimed" } else { "hip" };
        format!("{fire}, {how}")
    }

    /// The feats of this way's line up to its tier.
    fn feats(self) -> &'static [u16] {
        match self.fire {
            Fire::Plain => &[],
            Fire::PowerBlast(tier) => &feats::POWER_BLAST[..tier],
            Fire::SniperShot(tier) => &feats::SNIPER_SHOT[..tier],
        }
    }

    /// How long [Feat attack] is held before it is let go (ms): a Power
    /// Blast's full charge, or a Sniper Shot's steadying.
    fn hold_ms(self) -> u64 {
        use rules::feats::specials::{power_blast, sniper_shot};
        match self.fire {
            Fire::Plain => 0,
            Fire::PowerBlast(_) => power_blast::STEPS * power_blast::STEP_MS,
            Fire::SniperShot(_) => sniper_shot::STEADY_MS[usize::from(self.aimed)],
        }
    }
}

const GUNS: [&str; 3] = ["one pistol", "a pair", "a rifle"];

fn guns(at: usize) -> [Option<Weapon>; 2] {
    match at {
        0 => [Some(pistol()), None],
        1 => [Some(pistol()), Some(pistol())],
        _ => [Some(rifle()), None],
    }
}

/// The time between two shots of the gun in the main hand (ms).
fn refire_ms(at: usize) -> u64 {
    match guns(at)[0].map(|gun| gun.delivery) {
        Some(Delivery::Blaster(blaster)) => u64::from(weapons::primary(blaster).refire_ms),
        _ => 0,
    }
}

/// One case: a gun, a way of firing, a Defense, a range.
#[derive(Debug, Clone, Copy)]
struct Case {
    gun: usize,
    way: Way,
    defence: usize,
    range: usize,
}

/// What one case dealt and spent over the minute: damage (HP), stamina
/// spent (points, the sum of every drop of the soldier's stamina), feat
/// attacks fired, and shots fired. A shot is a step on which the soldier's
/// gun fired, so a pair's Sniper Shot or Power Blast from both guns is one
/// shot.
#[derive(Debug, Clone, Copy, Default)]
struct Measured {
    damage: f64,
    spent: f64,
    feat_shots: f64,
    shots: f64,
}

impl Measured {
    fn mean(all: &[Measured]) -> Measured {
        let count = all.len().max(1) as f64;
        let mean = |of: fn(&Measured) -> f64| all.iter().map(of).sum::<f64>() / count;
        Measured {
            damage: mean(|m| m.damage),
            spent: mean(|m| m.spent),
            feat_shots: mean(|m| m.feat_shots),
            shots: mean(|m| m.shots),
        }
    }

    fn dps(&self) -> f64 {
        self.damage / SECONDS as f64
    }

    /// The share of the shots that were plain shots.
    fn plain_share(&self) -> f64 {
        1.0 - self.feat_shots / self.shots.max(1.0)
    }

    /// The damage dealt for each point of stamina spent.
    fn per_point(&self) -> Option<f64> {
        (self.spent > 0.0).then(|| self.damage / self.spent)
    }
}

/// The soldier and the dummy of `case`, `case.range` apart on the ring.
fn bout(level: &Level, case: Case, seed: u64) -> (Bout, Sheet) {
    let mut sheet = Sheet::dev_soldier();
    sheet.weapons = guns(case.gun);
    let mut target = dummy([None, None]);
    target.max_hp = u16::MAX;
    let weapon = sheet.main_weapon();
    let wanted = DEFENCES[case.defence].1;
    sheet.bab = (0..=60)
        .find(|bab| {
            sheet.bab = *bab;
            damage::chance(&sheet, &weapon, &target) >= wanted
        })
        .expect("a base attack bonus for the chance to hit");
    for row in case.way.feats() {
        sheet.feats.insert(*row);
    }
    let half = RANGES_M[case.range] / UNIT / 2.0;
    (Bout::placed(level, [sheet, target], [-half, half], seed), sheet)
}

/// One case played for the minute. With `sparing`, [Feat attack] is
/// pressed only while the stamina can pay for it and [Attack] is never
/// held, so every shot is a feat attack.
fn play(level: &Level, case: Case, sparing: bool, seed: u64) -> Measured {
    let (mut bout, sheet) = bout(level, case, seed);
    let aim = if case.way.aimed { holds::AIM } else { 0 };
    let keys = Keys::default().at_middle();
    bout.hold(RAISE_MS, [keys.holding(aim), Keys::default()]);
    let cost = specials::cost(&sheet);
    let from = bout.fighter(1).hp;
    let end = bout.now() + SECONDS * 1000;
    let mut last_fp = bout.fighter(0).fp;
    let mut fired_at = bout.fighter(0).shooting.fired_at;
    let mut measured = Measured::default();
    while bout.now() < end {
        let me = bout.fighter(0);
        let feat = match case.way.fire {
            Fire::Plain => false,
            _ => match me.shooting.charging_from {
                // A charge or a steadying is let go once it has been held
                // long enough. The press goes down again on the next step.
                Some(from) => bout.now() < from + case.way.hold_ms(),
                None => !sparing || me.fp >= cost,
            },
        };
        let attack = if sparing { 0 } else { ATTACK };
        let held = aim | if feat { holds::FEAT } else { 0 };
        bout.step([Keys::press(attack).holding(held).at_middle(), Keys::default()]);
        let me = bout.fighter(0);
        if me.fp < last_fp {
            measured.spent += f64::from(last_fp - me.fp) / 100.0;
            measured.feat_shots += 1.0;
        }
        if me.shooting.fired_at != fired_at {
            measured.shots += 1.0;
        }
        (last_fp, fired_at) = (me.fp, me.shooting.fired_at);
    }
    bout.hold(TAIL_MS, [Keys::default(); 2]);
    measured.damage = f64::from(from - bout.fighter(1).hp) / 100.0;
    measured
}

/// `jobs` run on every core, each over `SEEDS` seeds and averaged, in
/// order. Each seed's run keeps its own slot, so the averages add up in
/// the same order whichever run ends first.
fn measure(level: &Level, jobs: &[(Case, bool)]) -> Vec<Measured> {
    let runs: Vec<(usize, u64)> = (0..jobs.len())
        .flat_map(|at| (1..=SEEDS).map(move |seed| (at, seed)))
        .collect();
    let next = AtomicUsize::new(0);
    let done = Mutex::new(vec![[Measured::default(); SEEDS as usize]; jobs.len()]);
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get());
    std::thread::scope(|scope| {
        for _ in 0..workers.min(runs.len()) {
            scope.spawn(|| {
                while let Some(&(at, seed)) = runs.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let (case, sparing) = jobs[at];
                    let measured = play(level, case, sparing, seed);
                    if let Ok(mut done) = done.lock() {
                        done[at][(seed - 1) as usize] = measured;
                    }
                }
            });
        }
    });
    let done = done.into_inner().unwrap_or_else(|poisoned| poisoned.into_inner());
    done.iter().map(|runs| Measured::mean(runs)).collect()

}

/// The table and the three checks (see the module documentation).
pub(super) fn sniper_balance(level: &Level, report: &mut Report) -> Value {
    outln!(
        "sniper balance: {SECONDS} s at an idle dummy, {SEEDS} seeds; damage a second, stamina spent a second, damage per point of stamina, gain over plain aimed fire"
    );
    let ways = Way::all();
    let mut cases = Vec::new();
    for gun in 0..GUNS.len() {
        for &way in &ways {
            for defence in 0..DEFENCES.len() {
                for range in 0..RANGES_M.len() {
                    cases.push(Case { gun, way, defence, range });
                }
            }
        }
    }
    // Every feat attack's case is played a second time with the feat
    // attack fired only while the stamina pays for it, so every shot is a
    // feat attack: what the feat alone deals for its stamina.
    let feat_cases: Vec<Case> = cases.iter().copied().filter(|c| c.way.fire != Fire::Plain).collect();
    let jobs: Vec<(Case, bool)> = cases
        .iter()
        .map(|case| (*case, false))
        .chain(feat_cases.iter().map(|case| (*case, true)))
        .collect();
    let measured = measure(level, &jobs);
    let (held, sparing) = measured.split_at(cases.len());
    let at = |all: &[Case], gun: usize, way: Way, defence: usize, range: usize| {
        all.iter()
            .position(|c| c.gun == gun && c.way == way && c.defence == defence && c.range == range)
    };
    let find = |gun, way, defence, range| held[at(&cases, gun, way, defence, range).expect("every case is measured")];
    let alone = |case: &Case| at(&feat_cases, case.gun, case.way, case.defence, case.range).map(|i| sparing[i]);
    let plain = |aimed: bool| Way { fire: Fire::Plain, aimed };
    outln!(
        "  | Gun | Way of firing | Defense | Range | Damage a second | Stamina a second | Plain shots | Damage per point, feat alone | Gain over plain aimed |"
    );
    outln!("  |---|---|---|---|---|---|---|---|---|");
    let mut rows = Vec::new();
    for (case, m) in cases.iter().zip(held) {
        let base = find(case.gun, plain(true), case.defence, case.range).dps();
        let gain = m.dps() / base - 1.0;
        let per_point = alone(case).and_then(|feat| feat.per_point());
        let feat = case.way.fire != Fire::Plain;
        outln!(
            "  | {} | {} | {} | {} m | {:.2} | {:.2} | {} | {} | {:+.0}% |",
            GUNS[case.gun],
            case.way.name(),
            DEFENCES[case.defence].0,
            RANGES_M[case.range],
            m.dps(),
            m.spent / SECONDS as f64,
            if feat { format!("{:.0}%", m.plain_share() * 100.0) } else { String::new() },
            per_point.map_or(String::new(), |p| format!("{p:.2}")),
            gain * 100.0,
        );
        rows.push(json!({
            "gun": GUNS[case.gun], "way": case.way.name(), "defence": DEFENCES[case.defence].0,
            "range_m": RANGES_M[case.range], "dps": m.dps(), "stamina_per_second": m.spent / SECONDS as f64,
            "plain_share": feat.then(|| m.plain_share()), "damage_per_point_feat_alone": per_point,
            "feat_shots": m.feat_shots, "shots": m.shots, "gain": gain,
        }));
    }
    let snipers: Vec<Way> = ways
        .iter()
        .copied()
        .filter(|way| matches!(way.fire, Fire::SniperShot(_)))
        .collect();
    let blast = |way: Way| match way.fire {
        Fire::SniperShot(tier) => Way { fire: Fire::PowerBlast(tier), ..way },
        _ => way,
    };
    let mut beats_hip = Vec::new();
    let mut under_blast = Vec::new();
    for (gun, gun_name) in GUNS.iter().enumerate() {
        let hip = find(gun, plain(false), HIGH, FAR).dps();
        for &way in &snipers {
            let dps = find(gun, way, HIGH, FAR).dps();
            beats_hip.push((gun_name, way.name(), dps, hip));
            for (range, metres) in RANGES_M.iter().enumerate() {
                let sniper = find(gun, way, LOW, range).dps();
                let power = find(gun, blast(way), LOW, range).dps();
                under_blast.push((gun_name, way.name(), metres, sniper, power));
            }
        }
    }
    let failing = |all: Vec<String>| if all.is_empty() { "all".to_string() } else { all.join("; ") };
    report.check(
        "against high Defense at 30 m, Sniper Shot deals more damage a second than plain fire from the hip, with every gun and tier",
        beats_hip.iter().all(|(.., dps, hip)| dps > hip),
        failing(
            beats_hip
                .iter()
                .filter(|(.., dps, hip)| dps <= hip)
                .map(|(gun, way, dps, hip)| format!("{gun} {way} {dps:.2} against {hip:.2} HP/s"))
                .collect(),
        ),
    );
    report.check(
        "against low Defense, Sniper Shot deals no more damage a second than Power Blast of the same tier, fired the same way",
        under_blast.iter().all(|(.., sniper, power)| sniper <= power),
        failing(
            under_blast
                .iter()
                .filter(|(.., sniper, power)| sniper > power)
                .map(|(gun, way, range, sniper, power)| {
                    format!("{gun} {way} at {range} m {sniper:.2} against {power:.2} HP/s")
                })
                .collect(),
        ),
    );
    outln!("  one Sniper Shot against high Defense at 30 m, and plain aimed fire over the time it takes:");
    let mut bursts = Vec::new();
    let singles = feat_cases
        .iter()
        .zip(sparing)
        .filter(|(c, _)| matches!(c.way.fire, Fire::SniperShot(_)) && c.defence == HIGH && c.range == FAR);
    for (case, m) in singles {
        let shot = m.damage / m.feat_shots.max(1.0);
        // The steadying overlaps the gun's refire, so a chained Sniper
        // Shot takes the longer of the two.
        let took_ms = case.way.hold_ms().max(refire_ms(case.gun));
        let burst = find(case.gun, plain(true), HIGH, FAR).dps() * took_ms as f64 / 1000.0;
        outln!(
            "  {:<11} {:<20} {shot:>6.2} HP a shot ({:.1} shots)   plain aimed fire over {took_ms} ms {burst:>6.2} HP",
            GUNS[case.gun],
            case.way.name(),
            m.feat_shots,
        );
        rows.push(json!({
            "gun": GUNS[case.gun], "way": case.way.name(), "one_shot": shot, "took_ms": took_ms, "plain_aimed_burst": burst,
        }));
        bursts.push((GUNS[case.gun], case.way.name(), shot, burst));
    }
    report.check(
        "against high Defense at 30 m, one Sniper Shot deals more than plain aimed fire deals in the time the Sniper Shot takes",
        bursts.iter().all(|(.., shot, burst)| shot > burst),
        failing(
            bursts
                .iter()
                .filter(|(.., shot, burst)| shot <= burst)
                .map(|(gun, way, shot, burst)| format!("{gun} {way} {shot:.2} against {burst:.2} HP"))
                .collect(),
        ),
    );
    Value::Array(rows)
}
