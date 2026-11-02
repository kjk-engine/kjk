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
//!   (more than 90° from the way before, each way kept for `TURN_M`)
//!   `REVERSALS` times, or it stops for `LEAST_STOP_MS` and starts again
//!   `STOP_GOS` times.
//! - **Hop**: a body standing where it is (`everyone`) whose legs flip into
//!   JA's jump or fall, or that leaves the floor `HOPS` times in
//!   `HOP_WINDOW_MS` (for `HOP_AIR_MS`, or by `HOP_RISE_M`: a step's gravity
//!   as a walk stops shows nothing), neither thrown nor jumped by a command:
//!   it will not settle (a straight-down velocity normalized up,
//!   `motion::ground_move`).
//!
//! A playthrough watches the bodies a script moves or the walker carries;
//! `kjk dialogue check` watches `everyone`, standing or moved by their AI,
//! and says what the view draws over their legs (`watch_drawn`).
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
/// A way kept for less than this (m) is no turn: a shudder, seen as one.
const TURN_M: f32 = 0.05;
/// A stop shorter than this (ms) is a frame or two's: no stop to the eye.
const LEAST_STOP_MS: u64 = 64;
/// Slower than this (m/s) with its legs walking, a body treads in place;
/// for `LEAST_TREAD_MS` at least, it is kept.
const TREAD_MPS: f32 = 0.1;
const LEAST_TREAD_MS: u64 = 500;
/// Episodes of one kind on one body this near (ms) are one.
const JOIN_MS: u64 = 250;
/// A slide shorter than this (ms) is not kept: a walk's first and last
/// steps, as the legs' animation catches up.
const LEAST_SLIDE_MS: u64 = 200;
/// Leaving the floor this often within this long (ms) is a hop.
const HOPS: usize = 2;
const HOP_WINDOW_MS: u64 = 2000;
/// Off the floor this long (ms) or this far (m) up or down is off it to see.
const HOP_AIR_MS: u64 = 50;
const HOP_RISE_M: f32 = 0.02;

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
    /// Standing, it will not stay on the floor: its legs' animation, or
    /// how often it left.
    Hop(String),
}

/// What a moving body shows, as the view draws it (`Motion::watch_drawn`).
pub enum Drawn {
    /// Its legs' animation walks it or does not.
    Legs,
    /// Its own walk and run, by its speed whatever its legs (a beast's, a
    /// droid's, `cast::Clips`).
    Stride,
    /// This over its legs.
    Over(String),
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
    steps: VecDeque<(u64, Option<[f32; 2]>, f32)>,
    jitter_until: u64,
    /// When it last left the floor to see, whether it stood on it last step,
    /// and when and how high it left it (none once told).
    left: VecDeque<u64>,
    grounded: bool,
    leaving: Option<(u64, f32)>,
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
    /// Every living body is watched, not only those a script moves.
    everyone: bool,
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

    /// A watcher of every body (see the module documentation).
    pub fn everyone() -> Motion {
        Motion {
            everyone: true,
            ..Motion::new()
        }
    }

    /// The name it reports the body `id` by, once seen moving.
    pub fn name_of(&self, id: u32) -> Option<&str> {
        self.names.get(&id).map(String::as_str)
    }

    /// Watches `snapshot`, `msec` after the one before, of `level`, judging
    /// a held scene's script animation or hold from the sim's own timers.
    pub fn watch(&mut self, snapshot: &Snapshot, level: &sim::Level, msec: u32) {
        let now = snapshot.time_ms;
        let held = snapshot.holds_world();
        self.watch_drawn(snapshot, level, msec, &mut |_, fighter| {
            let acting = fighter
                .acting
                .filter(|acting| acting.playing(now))
                .filter(|acting| acting.animation != sim::combat::death::DIE)
                .filter(|_| held);
            let hold = matches!(
                fighter.force.hold,
                sim::script::effects::Hold::Fear | sim::script::effects::Hold::Whirlwind
            ) && fighter.force.stunned_until > now;
            match (acting, hold) {
                (Some(acting), _) => Drawn::Over(format!("animation {}", acting.animation)),
                (None, true) => Drawn::Over(format!("hold {:?}", fighter.force.hold)),
                _ => Drawn::Legs,
            }
        });
    }

    /// `watch`, `over` saying what is drawn over a moving body's legs.
    pub fn watch_drawn(
        &mut self,
        snapshot: &Snapshot,
        level: &sim::Level,
        msec: u32,
        over: &mut dyn FnMut(&sim::EntityState, &sim::combat::Fighter) -> Drawn,
    ) {
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
            if !self.traced.is_empty() && !(scripted || carried) && fighter.alive() {
                let objects =
                    instances.get_or_insert_with(|| sim::Instances::in_snapshot(level, snapshot));
                let tag = objects
                    .of_entity(entity)
                    .map_or_else(String::new, |o| o.tag.clone());
                if self.traced.contains(&tag) {
                    self.trace.push(format!(
                        "{module} {now} {tag} rest at {:.3},{:.3} z {:.3} vel {:?} ground {:?} flags {} yaw {:.0} legs {} torso {} acting {:?} task {:?} cmd {},{} buttons {}",
                        at[0],
                        at[1],
                        at[2],
                        player.velocity,
                        player.ground,
                        player.flags,
                        player.view[1],
                        player.legs.anim.name(),
                        player.torso.anim.name(),
                        fighter.acting.filter(|a| a.playing(now)).map(|a| a.animation),
                        mind.task,
                        fighter.last_command.forward,
                        fighter.last_command.right,
                        fighter.last_command.buttons,
                    ));
                }
            }
            let thrown = player.ground.is_none()
                || player.flags & sim::motion::motion_flags::KNOCKBACK_HOLD != 0;
            // A body dead where it lies stays there: one crossing the floor
            // glides on its dying clip.
            let glides = self.everyone
                && fighter.dead()
                && !thrown
                && last.is_some_and(|last| {
                    let far = (at[0] - last[0]).hypot(at[1] - last[1]);
                    far <= SNAP_M && far / seconds >= MOVING_MPS
                });
            if !fighter.alive() || !(scripted || carried || self.everyone) {
                track.steps.clear();
                if glides {
                    let who = Self::named(&mut self.names, entity, level, snapshot, &mut instances);
                    self.note(Episode {
                        module: module.clone(),
                        who,
                        kind: Kind::Slide("dead".to_string()),
                        from_ms: now,
                        to_ms: now,
                        held,
                        at,
                        scripted,
                    });
                }
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
            let name = Self::named(&mut self.names, entity, level, snapshot, &mut instances);
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
            let legs = player.legs.anim.name();
            let on_floor = player.ground.is_some();
            let track = self.tracks.entry(entity.id.0).or_default();
            if self.everyone {
                if track.grounded && !on_floor {
                    track.leaving = Some((now, at[2]));
                }
                track.grounded = on_floor;
                if on_floor {
                    track.leaving = None;
                }
                if let Some((from, z)) = track.leaving
                    && (now >= from + HOP_AIR_MS || (at[2] - z).abs() >= HOP_RISE_M)
                {
                    track.left.push_back(from);
                    track.leaving = None;
                }
                while track.left.front().is_some_and(|when| *when + HOP_WINDOW_MS < now) {
                    track.left.pop_front();
                }
                let knocked = player.flags & sim::motion::motion_flags::KNOCKBACK_HOLD != 0;
                let jumped = fighter.last_command.up > 0;
                if !knocked && !jumped && !moving {
                    if matches!(legs, "BOTH_JUMP1" | "BOTH_INAIR1") {
                        found.push(Kind::Hop(format!("legs {legs}")));
                    } else if track.left.len() >= HOPS {
                        found.push(Kind::Hop("leaves the floor again and again".to_string()));
                    }
                }
            }
            let walking = legs.contains("WALK") || legs.contains("RUN");
            if moving && !thrown {
                self.moving_steps += 1;
                self.movers.insert(name.clone());
                let slide = match over(entity, fighter) {
                    Drawn::Over(how) => Some(how),
                    Drawn::Legs if !walking => Some(format!("legs {legs}")),
                    Drawn::Legs | Drawn::Stride => None,
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
            let track = self.tracks.entry(entity.id.0).or_default();
            track.steps.push_back((now, way, far));
            while track
                .steps
                .front()
                .is_some_and(|(when, ..)| *when + WINDOW_MS < now)
            {
                track.steps.pop_front();
            }
            let reversals = turns_back(&track.steps);
            let stop_gos = stops_and_goes(&track.steps);
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

    /// `entity`'s name for a report, kept.
    fn named(
        names: &mut BTreeMap<u32, String>,
        entity: &sim::EntityState,
        level: &sim::Level,
        snapshot: &Snapshot,
        instances: &mut Option<sim::Instances>,
    ) -> String {
        names
            .entry(entity.id.0)
            .or_insert_with(|| {
                let objects =
                    instances.get_or_insert_with(|| sim::Instances::in_snapshot(level, snapshot));
                match entity.kind {
                    sim::Kind::Avatar(seat) => format!("player{}", seat.0),
                    _ => objects
                        .of_entity(entity)
                        .map_or_else(|| format!("#{}", entity.id.0), |o| o.tag.clone()),
                }
            })
            .clone()
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
            Kind::Jitter(_) | Kind::Hop(_) => true,
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
                Kind::Hop(_) => (s, j + 1, t),
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
            Kind::Hop(how) => format!("hops ({how})"),
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
                Kind::Hop(how) => format!("hop: {how}"),
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

/// How often `steps` turn back on a way kept for `TURN_M`, each way after one
/// as long: a way shorter than that is passed over.
fn turns_back(steps: &VecDeque<(u64, Option<[f32; 2]>, f32)>) -> usize {
    let mut ways: Vec<([f32; 2], f32)> = Vec::new();
    for (_, way, far) in steps {
        let Some(way) = way else { continue };
        match ways.last_mut() {
            Some((last, kept)) if last[0] * way[0] + last[1] * way[1] >= 0.0 => {
                *last = *way;
                *kept += far;
            }
            _ => ways.push((*way, *far)),
        }
    }
    ways.retain(|(_, kept)| *kept >= TURN_M);
    ways.windows(2)
        .filter(|pair| pair[0].0[0] * pair[1].0[0] + pair[0].0[1] * pair[1].0[1] < 0.0)
        .count()
}

/// How often `steps` start again after a stop of `LEAST_STOP_MS`.
fn stops_and_goes(steps: &VecDeque<(u64, Option<[f32; 2]>, f32)>) -> usize {
    let mut goes = 0;
    let mut stopped = None;
    for (when, way, _) in steps {
        match (way, stopped) {
            (None, None) => stopped = Some(*when),
            (Some(_), Some(from)) => {
                goes += usize::from(*when >= from + LEAST_STOP_MS);
                stopped = None;
            }
            _ => {}
        }
    }
    goes
}
