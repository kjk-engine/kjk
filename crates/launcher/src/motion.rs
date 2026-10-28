//! What a scripted move looks like, measured from the snapshots: a body a
//! script moves (`Mind::moving`) or KOTOR's walker carries in a held scene
//! should always play a moving clip, and go its way without turning back
//! and forth or stopping and starting. `Motion` watches each step and
//! keeps the episodes that break either rule:
//!
//! - **Slide**: the body crosses the floor faster than `MOVING_MPS` while
//!   what the view draws on it is no move: its legs stand (no `WALK` or
//!   `RUN` in their animation), or, while a conversation holds the world, a
//!   script's animation plays on (`Acting`; the view breaks one off by
//!   motion only out of a held scene, `acting::Shows::follow`), or a hold
//!   (fear, a whirlwind) plays its clip.
//! - **Jitter**: within `WINDOW_MS` its steps turn back on themselves
//!   (more than 90° from the step before) `REVERSALS` times, or it stops and
//!   starts again `STOP_GOS` times.
//!
//! A jump or a push (over `SNAP_M` in a step) starts the body's track
//! afresh; a body off the floor or knocked back is not walking.

use std::collections::{BTreeMap, VecDeque};

use serde_json::json;
use sim::Snapshot;

/// Faster than this (m/s, over the floor) a body is moving.
const MOVING_MPS: f32 = 0.3;
/// Farther than this (m) in one step is a jump or a push, not a walk.
const SNAP_M: f32 = 1.0;
/// The window (ms) jitter is counted over, and what in it is jitter.
const WINDOW_MS: u64 = 1000;
const REVERSALS: usize = 3;
const STOP_GOS: usize = 3;
/// Slower than this (m/s) with its legs walking, a body treads in place;
/// for `LEAST_TREAD_MS` at least, it is kept.
const TREAD_MPS: f32 = 0.1;
const LEAST_TREAD_MS: u64 = 500;
/// Episodes of one kind on one body this near (ms) are one.
const JOIN_MS: u64 = 250;
/// A slide shorter than this (ms) is not kept: a walk's first and last
/// steps, as the legs' animation catches up.
const LEAST_SLIDE_MS: u64 = 200;

/// What broke the rule.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// Moving with this drawn: the legs' animation, or a script's
    /// animation number, or a hold.
    Slide(String),
    /// Turning back on itself or stopping and starting.
    Jitter(&'static str),
    /// Its legs walking or running where it stands (`TREAD_MPS`): a walk
    /// held by what it walks into.
    Treads,
}

/// One episode on one body.
#[derive(Debug, Clone)]
pub struct Episode {
    /// The level's name (its module) and the body's tag.
    pub module: String,
    pub who: String,
    pub kind: Kind,
    pub from_ms: u64,
    pub to_ms: u64,
    /// Whether a conversation held the world as it began.
    pub held: bool,
    /// Where it began (m).
    pub at: [f32; 3],
    /// Whether a script's move was under way (`Mind::moving`), else KOTOR's
    /// walker carried it in a held scene.
    pub scripted: bool,
}

/// A body's track: its last place, its recent steps (when, way, moving).
#[derive(Default)]
struct Track {
    last: Option<[f32; 3]>,
    steps: VecDeque<(u64, Option<[f32; 2]>)>,
    jitter_until: u64,
}

/// The watcher (see the module documentation).
#[derive(Default)]
pub struct Motion {
    tracks: BTreeMap<u32, Track>,
    pub episodes: Vec<Episode>,
    /// Steps a watched body moved, and of them those that slid.
    pub moving_steps: u64,
    pub sliding_steps: u64,
    /// The bodies watched moving.
    pub movers: std::collections::BTreeSet<String>,
    names: BTreeMap<u32, String>,
    /// The bodies whose every step is written down (`KJK_MOTION_TRACE`, tags
    /// comma-separated), and those lines.
    traced: Vec<String>,
    pub trace: Vec<String>,
    /// The level watched: ids are the level's, so another starts afresh.
    level: Option<sim::LevelId>,
}

impl Motion {
    /// A watcher tracing the bodies `KJK_MOTION_TRACE` names.
    pub fn new() -> Motion {
        Motion {
            traced: std::env::var("KJK_MOTION_TRACE")
                .map(|tags| tags.split(',').map(str::to_string).collect())
                .unwrap_or_else(|_| Vec::new()),
            ..Motion::default()
        }
    }

    /// Watches `snapshot`, `msec` after the one before, of `level`.
    pub fn watch(&mut self, snapshot: &Snapshot, level: &sim::Level, msec: u32) {
        if self.level != Some(level.id()) {
            self.level = Some(level.id());
            self.tracks.clear();
            self.names.clear();
        }
        let module = level.id().name;
        let now = snapshot.time_ms;
        let held = snapshot.holds_world();
        let seconds = msec.max(1) as f32 / 1000.0;
        let mut instances = None;
        for entity in &snapshot.entities {
            let (Some(player), Some(fighter)) = (&entity.player, &entity.fighter) else {
                continue;
            };
            let mind = &fighter.mind;
            let scripted = mind.moving;
            let carried = held && mind.walker.is_some();
            let track = self.tracks.entry(entity.id.0).or_default();
            let at = entity.position;
            let last = track.last.replace(at);
            if !fighter.alive() || !(scripted || carried) {
                track.steps.clear();
                continue;
            }
            let Some(last) = last else { continue };
            let step = [at[0] - last[0], at[1] - last[1]];
            let far = (step[0] * step[0] + step[1] * step[1]).sqrt();
            if far > SNAP_M {
                track.steps.clear();
                continue;
            }
            let moving = far / seconds >= MOVING_MPS;
            let name = self.names.entry(entity.id.0).or_insert_with(|| {
                let objects =
                    instances.get_or_insert_with(|| sim::Instances::in_snapshot(level, snapshot));
                match entity.kind {
                    sim::Kind::Avatar(seat) => format!("player{}", seat.0),
                    _ => objects
                        .of_entity(entity)
                        .map_or_else(|| format!("#{}", entity.id.0), |o| o.tag.clone()),
                }
            });
            let name = name.clone();
            if self.traced.contains(&name) {
                let walker = mind.walker.map(|w| {
                    format!(
                        "goal {:.2},{:.2} corner {:?} stalls {} aside {} rounding {} bumps {} by {:?}",
                        w.goal[0] * collision::UNIT,
                        w.goal[1] * collision::UNIT,
                        w.waypoint.map(|c| [c[0] * collision::UNIT, c[1] * collision::UNIT]),
                        w.stalls,
                        w.aside_until_ms > now,
                        w.rounding,
                        w.bumps,
                        w.bumped.map(|(who, at)| (who.0, at[0] * collision::UNIT, at[1] * collision::UNIT))
                    )
                });
                self.trace.push(format!(
                    "{module} {now} {name} at {:.3},{:.3} step {:.3} yaw {:.0} legs {} acting {:?} carried {} task {:?} cmd {},{} | {}",
                    at[0],
                    at[1],
                    far,
                    player.view[1],
                    player.legs.anim.name(),
                    fighter.acting.filter(|a| a.playing(now)).map(|a| a.animation),
                    player.flags & sim::motion::motion_flags::CARRIED != 0,
                    mind.task,
                    fighter.last_command.forward,
                    fighter.last_command.right,
                    walker.unwrap_or_else(String::new)
                ));
            }
            let mut found = Vec::new();
            // Thrown or off the floor, a body is moved by its blow or its
            // fall, not its walk.
            let thrown = player.ground.is_none()
                || player.flags & sim::motion::motion_flags::KNOCKBACK_HOLD != 0;
            let legs = player.legs.anim.name();
            let walking = legs.contains("WALK") || legs.contains("RUN");
            if moving && !thrown {
                self.moving_steps += 1;
                self.movers.insert(name.clone());
                let acting = fighter
                    .acting
                    .filter(|acting| acting.playing(now))
                    .filter(|acting| acting.animation != sim::combat::death::DIE)
                    .filter(|_| held);
                let hold = matches!(
                    fighter.force.hold,
                    sim::script::effects::Hold::Fear | sim::script::effects::Hold::Whirlwind
                ) && fighter.force.stunned_until > now;
                let slide = if let Some(acting) = acting {
                    Some(format!("animation {}", acting.animation))
                } else if hold {
                    Some(format!("hold {:?}", fighter.force.hold))
                } else if !walking {
                    Some(format!("legs {legs}"))
                } else {
                    None
                };
                if let Some(slide) = slide {
                    self.sliding_steps += 1;
                    found.push(Kind::Slide(slide));
                }
            }
            if walking && !thrown && far / seconds < TREAD_MPS {
                found.push(Kind::Treads);
            }
            let way = moving.then(|| [step[0] / far, step[1] / far]);
            track.steps.push_back((now, way));
            while track
                .steps
                .front()
                .is_some_and(|(when, _)| *when + WINDOW_MS < now)
            {
                track.steps.pop_front();
            }
            let ways: Vec<Option<[f32; 2]>> = track.steps.iter().map(|(_, way)| *way).collect();
            let moved: Vec<[f32; 2]> = ways.iter().flatten().copied().collect();
            let reversals = moved
                .windows(2)
                .filter(|pair| pair[0][0] * pair[1][0] + pair[0][1] * pair[1][1] < 0.0)
                .count();
            let stop_gos = ways
                .windows(2)
                .filter(|pair| pair[0].is_none() && pair[1].is_some())
                .count();
            if now >= track.jitter_until {
                let jitter = if reversals >= REVERSALS {
                    Some("turns back")
                } else if stop_gos >= STOP_GOS {
                    Some("stops and starts")
                } else {
                    None
                };
                if let Some(jitter) = jitter {
                    track.jitter_until = now + WINDOW_MS;
                    found.push(Kind::Jitter(jitter));
                }
            }
            for kind in found {
                self.note(Episode {
                    module: module.clone(),
                    who: name.clone(),
                    kind,
                    from_ms: now,
                    to_ms: now,
                    held,
                    at,
                    scripted,
                });
            }
        }
    }

    /// `episode` kept, or joined to the last of its kind on its body.
    fn note(&mut self, episode: Episode) {
        if let Some(was) = self.episodes.iter_mut().rev().find(|was| {
            was.who == episode.who
                && was.kind == episode.kind
                && was.to_ms + JOIN_MS >= episode.from_ms
        }) {
            was.to_ms = episode.to_ms;
            return;
        }
        self.episodes.push(episode);
    }

    /// The episodes worth reporting: every jitter, slides of
    /// `LEAST_SLIDE_MS` and more.
    pub fn kept(&self) -> impl Iterator<Item = &Episode> {
        self.episodes.iter().filter(|episode| match episode.kind {
            Kind::Slide(_) => episode.to_ms - episode.from_ms >= LEAST_SLIDE_MS,
            Kind::Treads => episode.to_ms - episode.from_ms >= LEAST_TREAD_MS,
            Kind::Jitter(_) => true,
        })
    }

    /// The facts: counts and each kept episode.
    pub fn facts(&self) -> serde_json::Value {
        let episodes: Vec<serde_json::Value> = self.kept().map(Episode::facts).collect();
        let (slides, jitters, treads) = self
            .kept()
            .fold((0, 0, 0), |(s, j, t), episode| match episode.kind {
                Kind::Slide(_) => (s + 1, j, t),
                Kind::Jitter(_) => (s, j + 1, t),
                Kind::Treads => (s, j, t + 1),
            });
        json!({
            "movers": self.movers.len(),
            "moving_steps": self.moving_steps,
            "sliding_steps": self.sliding_steps,
            "slides": slides,
            "jitters": jitters,
            "treads": treads,
            "episodes": episodes,
        })
    }
}

impl Episode {
    /// One line: when, who, what.
    pub fn line(&self) -> String {
        let what = match &self.kind {
            Kind::Slide(how) => format!("slides ({how})"),
            Kind::Jitter(how) => format!("jitters ({how})"),
            Kind::Treads => "treads in place".to_string(),
        };
        format!(
            "{} {:>8.2}-{:<8.2} {} {what}{}{} at {:.1},{:.1},{:.1}",
            self.module,
            self.from_ms as f64 / 1000.0,
            self.to_ms as f64 / 1000.0,
            self.who,
            if self.held { ", held" } else { "" },
            if self.scripted { ", scripted move" } else { ", walker" },
            self.at[0],
            self.at[1],
            self.at[2],
        )
    }

    fn facts(&self) -> serde_json::Value {
        json!({
            "module": self.module,
            "who": self.who,
            "kind": match &self.kind {
                Kind::Slide(how) => format!("slide: {how}"),
                Kind::Jitter(how) => format!("jitter: {how}"),
                Kind::Treads => "treads".to_string(),
            },
            "from_ms": self.from_ms,
            "to_ms": self.to_ms,
            "held": self.held,
            "scripted": self.scripted,
            "at": self.at,
        })
    }
}

/// `kjk dialogue motion GAME [MODULE…]`: each module (every one of the
/// game's when none is named) stepped headless as `kjk dialogue sweep`
/// steps it (its runs from `data/dialogue/<game>.sweep`, else one from its
/// entry), its scripted moves watched (`Motion`). The episodes go to
/// `motion.txt`, the counts to `facts.json`, in the run folder; it fails
/// when any is found.
pub fn sweep(discovery: &vfs::Discovery, game: &str, modules: &[String]) -> std::process::ExitCode {
    let go = || -> Result<(std::path::PathBuf, u64, u64, usize), String> {
        let kind = vfs::Game::from_key(game)
            .filter(|game| *game != vfs::Game::JediAcademy)
            .ok_or(format!("{game}: k1 or tsl"))?;
        let (anims, _) = crate::stage::anims(discovery);
        let installer =
            crate::lifecycle::Installer::new(discovery, kind, std::sync::Arc::new(anims))?;
        let modules = if modules.is_empty() {
            installer.modules()
        } else {
            modules.to_vec()
        };
        let folder = crate::runtime::create_run_folder()
            .map_err(|error| format!("the run folder: {error}"))?;
        eprintln!("run folder: {}", folder.display());
        let (mut lines, mut per_module) = (Vec::new(), Vec::new());
        let (mut moving, mut sliding, mut episodes) = (0, 0, 0);
        for module in &modules {
            for hint in crate::scene_sweep::hints(game, module)? {
                let flags = hint.play_flags().join(" ");
                let mut motion = Motion::new();
                let played = crate::dialogue_verbs::play_watched(
                    &installer,
                    module,
                    &hint.run()?,
                    &mut |step| {
                        motion.watch(step.snapshot, step.level, step.msec);
                        true
                    },
                );
                if let Err(error) = played {
                    lines.push(format!("{module} {flags}: not run: {error}"));
                    continue;
                }
                let kept: Vec<&Episode> = motion.kept().collect();
                moving += motion.moving_steps;
                sliding += motion.sliding_steps;
                episodes += kept.len();
                eprintln!(
                    "{module} {flags}: {} movers, {} moving steps, {} sliding, {} episodes",
                    motion.movers.len(),
                    motion.moving_steps,
                    motion.sliding_steps,
                    kept.len()
                );
                lines.extend(kept.iter().map(|episode| episode.line()));
                lines.extend(motion.trace.iter().cloned());
                per_module.push(json!({ "module": module, "run": flags, "motion": motion.facts() }));
            }
        }
        let mut body = lines.join("\n");
        body.push('\n');
        std::fs::write(folder.join("motion.txt"), body)
            .map_err(|error| format!("motion.txt: {error}"))?;
        crate::runtime::write_facts_in(
            &folder,
            json!({
                "game": game,
                "moving_steps": moving,
                "sliding_steps": sliding,
                "episodes": episodes,
                "runs": per_module,
            }),
        );
        Ok((folder, moving, sliding, episodes))
    };
    match go() {
        Ok((folder, moving, sliding, episodes)) => {
            outln!(
                "{moving} moving steps, {sliding} sliding, {episodes} episodes; {}",
                folder.join("motion.txt").display()
            );
            if episodes == 0 {
                std::process::ExitCode::SUCCESS
            } else {
                std::process::ExitCode::FAILURE
            }
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
}
