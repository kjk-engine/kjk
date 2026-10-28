//! Where a launch's time goes: the loader's phases as they run (`timed`,
//! `record`) and moments since the process began (`mark`: loaded, the app
//! built, the world up, the scene spawned, the pipelines warmed), published for `dump` as
//! `launch`. `kjk launch-report` runs launches and tables them, so a phase
//! that grows shows without timing code of its own.

use std::sync::Mutex;
use std::time::Instant;

use bevy::prelude::*;
use console::{DumpDue, DumpFacts};
use serde_json::json;

struct Times {
    started: Instant,
    /// (phase, ms), in the order they ran; a phase inside another is named
    /// `outer/inner`.
    phases: Vec<(String, f64)>,
    /// (moment, ms since the process began), each kept the first time.
    marks: Vec<(String, f64)>,
}

static TIMES: Mutex<Option<Times>> = Mutex::new(None);

fn with<T>(f: impl FnOnce(&mut Times) -> T) -> T {
    let mut times = TIMES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    f(times.get_or_insert_with(|| Times {
        started: Instant::now(),
        phases: Vec::new(),
        marks: Vec::new(),
    }))
}

/// The process's start, as early as `run` can say it.
pub fn start() {
    with(|_| ());
}

/// `phase` took `ms`.
pub fn record(phase: &str, ms: f64) {
    with(|times| times.phases.push((phase.to_string(), ms)));
}

/// Runs `f` as `phase`.
pub fn timed<T>(phase: &str, f: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    let out = f();
    record(phase, started.elapsed().as_secs_f64() * 1000.0);
    out
}

/// Phases one after another under one name: each `lap` records the time
/// since the one before (or since `new`).
pub struct Laps {
    under: &'static str,
    last: Instant,
}

impl Laps {
    pub fn new(under: &'static str) -> Laps {
        Laps {
            under,
            last: Instant::now(),
        }
    }

    pub fn lap(&mut self, phase: &str) {
        record(
            &format!("{}/{phase}", self.under),
            self.last.elapsed().as_secs_f64() * 1000.0,
        );
        self.last = Instant::now();
    }
}

/// The process reached `moment` (the first time only).
pub fn mark(moment: &str) {
    with(|times| {
        if !times.marks.iter().any(|(name, _)| name == moment) {
            let ms = times.started.elapsed().as_secs_f64() * 1000.0;
            times.marks.push((moment.to_string(), ms));
        }
    });
}

/// The sim's world is up (before its scene is spawned).
fn world_up(states: Res<console::States>) {
    if states.0.get("world").copied().unwrap_or(false) {
        mark("world up");
    }
}

fn publish(due: Res<DumpDue>, mut facts: ResMut<DumpFacts>) {
    if !due.0 {
        return;
    }
    let (phases, marks) = with(|times| (times.phases.clone(), times.marks.clone()));
    let list = |items: Vec<(String, f64)>| -> Vec<serde_json::Value> {
        items
            .into_iter()
            .map(|(name, ms)| json!({ "name": name, "ms": ms }))
            .collect()
    };
    facts.0.insert(
        "launch".into(),
        json!({ "phases": list(phases), "marks": list(marks) }),
    );
}

pub struct LaunchTimesPlugin;

impl Plugin for LaunchTimesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (world_up, publish).before(console::ConsoleQueue));
    }
}
