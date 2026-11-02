//! What each conversation showed, for `kjk dialogue check` (20): its deaths,
//! blasts, throws and bursts in the order they came, each with whether the
//! shot under way held it, and every body a conversation moved, with what
//! moved it. `SCENES.md` in the run folder holds them scene by scene, and
//! `faults` reads them for what came before its cause or off camera.

use std::collections::BTreeMap;

use serde_json::{Value, json};

/// How far (m) a body has to go in a scene to be listed as moved.
const MOVED_M: f32 = 0.5;

/// One thing a scene showed: when (sim ms), its kind (`death`, `effect
/// death`, `blast`, `throw`, `burst`), who, what, whether the shot held it
/// (none: no shot), and where (m).
#[derive(Debug, Clone)]
pub struct Event {
    pub ms: u64,
    pub kind: &'static str,
    pub who: String,
    pub what: String,
    pub seen: Option<bool>,
    pub at: [f32; 3],
}

impl Event {
    pub fn new(ms: u64, kind: &'static str, who: String, what: String, seen: Option<bool>, at: [f32; 3]) -> Event {
        Event { ms, kind, who, what, seen, at }
    }

    fn line(&self) -> String {
        let shot = match self.seen {
            Some(true) => "in shot",
            Some(false) => "off camera",
            None => "no shot",
        };
        format!("{:.2} s {} {} {} ({shot})", self.ms as f32 / 1000.0, self.kind, self.who, self.what)
    }

    fn flat(&self, other: &Event) -> f32 {
        (self.at[0] - other.at[0]).hypot(self.at[1] - other.at[1])
    }
}

/// A body moving in a scene: its name, what moved it, where it began and how
/// far it has come.
#[derive(Debug, Clone)]
struct Mover {
    name: String,
    why: String,
    last: [f32; 3],
    metres: f32,
}

/// What one conversation saw.
#[derive(Debug, Clone, Default)]
pub struct Scene {
    pub events: Vec<Event>,
    /// By body id, and what moved it (a body moved by a script, then its AI,
    /// is listed under each).
    movers: BTreeMap<(u32, String), Mover>,
    /// How many runs reached it.
    runs: usize,
}

impl Scene {
    /// `id` stands at `at` this step, moved by `why`.
    pub fn moved(&mut self, id: u32, name: String, why: String, at: [f32; 3]) {
        // Its last place, whatever moved it there.
        let last = self
            .movers
            .iter()
            .filter(|((body, _), _)| *body == id)
            .map(|(_, mover)| mover.last)
            .next();
        let mover = self
            .movers
            .entry((id, why.clone()))
            .or_insert_with(|| Mover { name, why, last: last.unwrap_or(at), metres: 0.0 });
        let step = (at[0] - mover.last[0]).hypot(at[1] - mover.last[1]);
        // A jump is no walk.
        if step < 1.0 {
            mover.metres += step;
        }
        for ((body, _), mover) in &mut self.movers {
            if *body == id {
                mover.last = at;
            }
        }
    }

    /// `other`'s run added.
    pub fn merge(&mut self, other: Scene) {
        self.runs += 1;
        // The first run that showed something is the one listed: a later
        // branch plays the same lines again.
        if self.events.is_empty() {
            self.events = other.events;
        }
        for (key, mover) in other.movers {
            let was = self.movers.entry(key).or_insert_with(|| Mover { metres: 0.0, ..mover.clone() });
            was.metres = was.metres.max(mover.metres);
        }
    }
}

/// What `scene`'s log puts out of order or off camera: (cause, whether a
/// script's own order or a note's matter, what). A body dead before the lob
/// beside it bursts within `before_ms` is the engine's; before a script's own
/// blast is that script's order (a note); so is a death or blast off camera.
pub fn faults(scene: &Scene, burst_m: f32, before_ms: u64, slack_ms: u64) -> Vec<(String, bool, String)> {
    let mut out = Vec::new();
    let of = |kind: &'static str| scene.events.iter().filter(move |event| event.kind == kind);
    for (blast, thrown) in [("burst", true), ("blast", false)] {
        for blast in of(blast) {
            for death in of("death") {
                let early = death.ms + slack_ms < blast.ms && death.ms + before_ms >= blast.ms;
                if early && death.flat(blast) <= burst_m {
                    let cause = match thrown {
                        true => "a body dies before the grenade beside it bursts",
                        false => "a body dies before a script's blast beside it (the script's order)",
                    };
                    let detail = format!("{}: dies {:.2} s, {} {:.2} s", death.who, death.ms as f32 / 1000.0, blast.kind, blast.ms as f32 / 1000.0);
                    out.push((cause.to_string(), !thrown, detail));
                }
            }
        }
    }
    for event in &scene.events {
        if matches!(event.kind, "death" | "blast" | "burst") && event.seen == Some(false) {
            out.push((format!("a {} off camera", event.kind), true, event.line()));
        }
    }
    out
}

/// The scenes as `report.json` holds them: those with anything to tell.
pub fn json(scenes: &BTreeMap<String, Scene>) -> Value {
    let told: BTreeMap<&String, Value> = scenes
        .iter()
        .filter_map(|(dialog, scene)| {
            let movers: Vec<Value> = scene
                .movers
                .values()
                .filter(|mover| mover.metres >= MOVED_M)
                .map(|mover| json!({ "who": mover.name, "why": mover.why, "m": (mover.metres * 10.0).round() / 10.0 }))
                .collect();
            if scene.events.is_empty() && movers.is_empty() {
                return None;
            }
            let events: Vec<String> = scene.events.iter().map(Event::line).collect();
            Some((dialog, json!({ "events": events, "movers": movers })))
        })
        .collect();
    json!(told)
}

/// `SCENES.md`: for every scene with a death, blast or throw, what it showed;
/// for all, who moved and by what.
pub fn summary(reports: &[Value]) -> String {
    let mut out = String::from("# Scenes: deaths, blasts, throws and movers\n");
    for report in reports {
        let Some(scenes) = report["scenes"].as_object() else {
            continue;
        };
        for (dialog, scene) in scenes {
            out.push_str(&format!("\n## {}/{dialog}\n", report["module"].as_str().unwrap_or("")));
            for line in scene["events"].as_array().into_iter().flatten() {
                out.push_str(&format!("- {}\n", line.as_str().unwrap_or("")));
            }
            let movers = scene["movers"].as_array().into_iter().flatten();
            let moved: Vec<String> = movers
                .map(|mover| format!("{} {} m ({})", mover["who"].as_str().unwrap_or(""), mover["m"], mover["why"].as_str().unwrap_or("")))
                .collect();
            if !moved.is_empty() {
                out.push_str(&format!("- moved: {}\n", moved.join("; ")));
            }
        }
    }
    out
}
