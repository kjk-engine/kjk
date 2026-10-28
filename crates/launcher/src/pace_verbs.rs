//! `kjk move pace <game>`: the whole game's census of how fast its
//! creatures walk and run out of a fight. Every module (or those starting
//! with `--only`) is installed for one standing player and stepped headless,
//! its scripts running, and each creature's ground speed is taken over every
//! 200 ms it walks or runs (its legs' animation) while nobody fights it: the
//! speed KOTOR would move it at (`sim::walk::kotor_speed`, its
//! `creaturespeed.2da` rates) beside the speed it moved. A window more than
//! `FAST` over KOTOR's is counted fast, one more than `FAST` under it slow.

use std::collections::BTreeMap;
use std::process::ExitCode;

use serde_json::json;
use sim::motion::{Anim, BASE_SPEED};
use vfs::{Discovery, Game};

use crate::script_verbs::step_module_watched;

/// Ticks a window spans (200 ms).
const WINDOW_TICKS: u64 = 25;
/// How far off KOTOR's speed a window may be (a share of it) and still
/// count as at it.
const FAST: f32 = 0.15;
/// Below this (m/s) a window is not a walk.
const MOVING: f32 = 0.2;

/// A creature's gait, by its legs' animation.
fn gait(legs: Anim) -> Option<bool> {
    match legs {
        Anim::BOTH_WALK1 | Anim::BOTH_WALKBACK1 => Some(false),
        Anim::BOTH_RUN1 | Anim::BOTH_RUNBACK1 => Some(true),
        _ => None,
    }
}

/// One module's windows, walks and runs apart: (measured, KOTOR's) m/s,
/// and the tags that went fast or slow.
#[derive(Default)]
struct Paced {
    walks: Vec<(f32, f32)>,
    runs: Vec<(f32, f32)>,
    off: BTreeMap<String, (usize, f32, f32)>,
    /// Each creature's windows by (tag and id, run): measured, KOTOR's.
    creatures: BTreeMap<(String, bool), Vec<(f32, f32)>>,
}

impl Paced {
    fn fast(windows: &[(f32, f32)]) -> usize {
        windows.iter().filter(|(m, k)| *m > k * (1.0 + FAST)).count()
    }

    fn slow(windows: &[(f32, f32)]) -> usize {
        windows.iter().filter(|(m, k)| *m < k * (1.0 - FAST)).count()
    }
}

fn median(mut values: Vec<f32>) -> f32 {
    values.sort_by(f32::total_cmp);
    values.get(values.len() / 2).copied().unwrap_or(0.0)
}

/// One module stepped `seconds`, its creatures' windows taken.
fn pace_module(
    installer: &crate::lifecycle::Installer,
    module: &str,
    seconds: u64,
    follow: &[String],
) -> Result<Paced, String> {
    let options = crate::sweep_verbs::coverage(seconds);
    let mut paced = Paced::default();
    let mut starts: BTreeMap<u32, ([f32; 3], Anim)> = BTreeMap::new();
    let mut tick = 0u64;
    let mut watch = |snapshot: &sim::Snapshot, world: &sim::SimWorld| {
        tick += 1;
        if !tick.is_multiple_of(WINDOW_TICKS) {
            return;
        }
        let level = world.level();
        let table = level
            .scripts()
            .map_or_else(Default::default, |library| library.move_rates());
        let objects = sim::Instances::in_snapshot(level, snapshot);
        let now = snapshot.time_ms;
        let mut next = BTreeMap::new();
        for entity in &snapshot.entities {
            let (Some(ps), Some(fighter)) = (entity.player, entity.fighter) else {
                continue;
            };
            if matches!(entity.kind, sim::Kind::Avatar(_)) || !fighter.alive() {
                continue;
            }
            let at = entity.position;
            next.insert(entity.id.0, (at, ps.legs.anim));
            let Some((from, legs)) = starts.get(&entity.id.0).copied() else {
                continue;
            };
            let fighting = fighter.target.is_some()
                || fighter.in_combat(now)
                || (fighter.mind.leader.is_some() && snapshot.in_combat);
            let Some(run) = gait(ps.legs.anim).filter(|run| gait(legs) == Some(*run)) else {
                continue;
            };
            let speed = sim::math::flat_distance(at, from) / (WINDOW_TICKS as f32 / sim::TICK_HZ as f32);
            if fighting || speed < MOVING {
                continue;
            }
            let data = objects.of_entity(entity);
            let Some(rates) = sim::walk::rates(data.and_then(|data| data.body.as_ref()), false, table)
            else {
                continue;
            };
            let kotor = sim::walk::kotor_speed(rates, ps.run_speed / BASE_SPEED, run);
            let window = (speed, kotor);
            let tag = data.map_or_else(String::new, |data| data.tag.clone());
            if Paced::fast(&[window]) + Paced::slow(&[window]) > 0 {
                let entry = paced.off.entry(tag.clone()).or_insert((0, speed, kotor));
                entry.0 += 1;
            }
            let who = format!("{tag}#{}", entity.id.0);
            paced.creatures.entry((who, run)).or_default().push(window);
            if run {
                paced.runs.push(window);
            } else {
                paced.walks.push(window);
            }
        }
        starts = next;
    };
    let stepped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        step_module_watched(installer, module, &options, follow, &mut watch).map(|_| ())
    }));
    match stepped {
        Ok(Ok(())) => Ok(paced),
        Ok(Err(error)) => Err(format!("not installed: {error}")),
        Err(_) => Err("panicked".into()),
    }
}

/// `kjk move pace`: `seconds` of each module of `game` (those starting
/// with `only`, if given), the creatures tagged `follow` following the
/// player.
pub fn pace(
    discovery: &Discovery,
    game: &str,
    seconds: u64,
    only: Option<&str>,
    follow: &[String],
) -> ExitCode {
    let run = || -> Result<serde_json::Value, String> {
        let game = Game::from_key(game).ok_or(format!("{game:?}: use k1 or tsl"))?;
        let anims = std::sync::Arc::new(sim::motion::AnimTable::stand_in());
        let installer = crate::lifecycle::Installer::new(discovery, game, anims)?;
        let modules: Vec<String> = installer
            .modules()
            .into_iter()
            .filter(|module| only.is_none_or(|only| module.starts_with(only)))
            .collect();
        let mut facts = BTreeMap::new();
        let (mut walks, mut runs) = (Vec::new(), Vec::new());
        let mut off: BTreeMap<String, (usize, f32, f32)> = BTreeMap::new();
        for module in &modules {
            let paced = match pace_module(&installer, module, seconds, follow) {
                Ok(paced) => paced,
                Err(error) => {
                    outln!("  {module:<12} {error}");
                    continue;
                }
            };
            let measured = |windows: &[(f32, f32)]| median(windows.iter().map(|w| w.0).collect());
            let kotor = |windows: &[(f32, f32)]| median(windows.iter().map(|w| w.1).collect());
            if !paced.walks.is_empty() || !paced.runs.is_empty() {
                outln!(
                    "  {module:<12} walks {:>4} ({:.2} m/s, KOTOR {:.2}; fast {:>4} slow {:>4})  runs {:>4} ({:.2} m/s, KOTOR {:.2}; fast {:>4} slow {:>4})",
                    paced.walks.len(),
                    measured(&paced.walks),
                    kotor(&paced.walks),
                    Paced::fast(&paced.walks),
                    Paced::slow(&paced.walks),
                    paced.runs.len(),
                    measured(&paced.runs),
                    kotor(&paced.runs),
                    Paced::fast(&paced.runs),
                    Paced::slow(&paced.runs),
                );
            }
            facts.insert(
                module.clone(),
                json!({
                    "walks": paced.walks.len(),
                    "walks_fast": Paced::fast(&paced.walks),
                    "walks_slow": Paced::slow(&paced.walks),
                    "walk_mps": measured(&paced.walks),
                    "runs": paced.runs.len(),
                    "runs_fast": Paced::fast(&paced.runs),
                    "runs_slow": Paced::slow(&paced.runs),
                    "run_mps": measured(&paced.runs),
                    "creatures": paced.creatures.iter().map(|((who, run), windows)| json!({
                        "who": who,
                        "run": run,
                        "windows": windows.len(),
                        "mps": measured(windows),
                        "kotor_mps": kotor(windows),
                    })).collect::<Vec<_>>(),
                }),
            );
            for (tag, (count, speed, kotor)) in paced.off {
                let entry = off
                    .entry(format!("{module} {tag}"))
                    .or_insert((0, speed, kotor));
                entry.0 += count;
            }
            walks.extend(paced.walks);
            runs.extend(paced.runs);
        }
        outln!(
            "{}: {} modules, {} s each; walks {} windows, {} fast, {} slow; runs {} windows, {} fast, {} slow",
            game.key(),
            modules.len(),
            seconds,
            walks.len(),
            Paced::fast(&walks),
            Paced::slow(&walks),
            runs.len(),
            Paced::fast(&runs),
            Paced::slow(&runs),
        );
        let mut ranked: Vec<_> = off.iter().collect();
        ranked.sort_by(|a, b| b.1.0.cmp(&a.1.0).then(a.0.cmp(b.0)));
        outln!("  most windows off KOTOR's speed (windows, a measured m/s, KOTOR's):");
        for (who, (count, speed, kotor)) in ranked.iter().take(25) {
            outln!("    {who:<40} {count:>4} {speed:>5.2} {kotor:>5.2}");
        }
        Ok(json!({
            "game": game.key(),
            "seconds": seconds,
            "walks": walks.len(),
            "walks_fast": Paced::fast(&walks),
            "walks_slow": Paced::slow(&walks),
            "runs": runs.len(),
            "runs_fast": Paced::fast(&runs),
            "runs_slow": Paced::slow(&runs),
            "modules": facts,
            "off": ranked.iter().map(|(who, (count, speed, kotor))| json!({
                "who": who, "windows": count, "mps": speed, "kotor_mps": kotor,
            })).collect::<Vec<_>>(),
        }))
    };
    match run() {
        Ok(facts) => {
            crate::collision_verbs::write_facts(facts);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
