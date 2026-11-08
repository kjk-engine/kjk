//! `kjk dialogue check k1|tsl [MODULE…]`: every conversation of a game's
//! modules played headless, each line checked against what a scene must do, so a
//! cutscene's faults are found by the machine rather than in a playtest.
//!
//! - **The runs.** Each module's sweep runs (`scene_sweep::hints`: from its
//!   entry, or `data/dialogue/<game>.sweep`'s lines), then one for each
//!   conversation an object of the module carries that those did not reach:
//!   the player uses the object (`--talk`) and takes the first reply each
//!   time, until the conversation ends (`TALK_MOST_S` at most). Each run is
//!   `dialogue_verbs::play_watched`, the sweep's own stepping.
//! - **The checks**, by number (`INVARIANTS`): 1 a body's scripted
//!   animations and the lines' `AnimList` play a clip its model chain has
//!   (`viewer::scripted_clips`); 2 a voiced line's voice is in the install,
//!   decodes and is heard (`dialogue_view::voice_unheard`); 3 each line's
//!   shot (`play_view::LineShots`) has its speaker or listener in its
//!   frustum, unoccluded, a staged line's bodies on their stunts' marks
//!   (`play_view::staged_places`); 4 a scene's blows (attack rows on `Acting`, the
//!   scripts' `Strike`s) play the attacker's clip, the target's reaction or
//!   death, a sound and, for a gunner, a bolt; 5 weapons in hand have a
//!   model (the armory's reading), and no slot holds a carried stack; 6 the dead
//!   stay down and lying poses hold; 7 a script's jump lands within `JUMP_M` of its mark
//!   (`sim::script::Jumped`); 8 every conversation ends, and its end script
//!   and its owner's end-of-conversation hook run; 9 at a line's start no
//!   participant stands off the floor, in a wall (KOTOR's walker, a point)
//!   or inside another body; 10 what a conversation reaches that the engine does not do: unbound
//!   routines (the report's gaps while it runs), DLG fields not read on the
//!   lines shown, a reply's `AnimList`; 11 module-only resources a view
//!   reads through the install's base (a weapon template only the module
//!   holds is noted: its sounds go by the sheet's base item); 12 JA's free
//!   play in a held scene (pain flinch, saber lock, a one-line conversation
//!   begun while it holds); 13 an entry is said by the body its `Speaker`
//!   names, never its owner standing in, and the view films that body
//!   speaking; beside the runs, every entry `Speaker` of the module's
//!   conversations is counted by what answers to it there
//!   (`speaker_census`); 14 a script's move ends where it was sent, not
//!   with no way there, given up short or jumped at a forced move's timeout
//!   (`sim::script::Moved`), judged for the moves of those a conversation's
//!   scripts direct (`Report::directed`; the rest, a creature's own
//!   routine, and a forced move timed out mid-walk are notes), a talk run
//!   stepping on while one still walks (`WALK_OUT_MS` at most); 15 a voiced line's speaker moves its lips: the
//!   line has a LIP, and the speaker's mouth (its hung head, else its body
//!   made with its face) has a `talk` clip that keys it
//!   (`dialogue_view::lips_unmoved`); 16 no body crosses the floor faster
//!   than `motion::MOVING_MPS` with no walk drawn on it, by what the view
//!   draws (`cast::drawn_acting`, `Assets::drawn`), whoever moves it; 17 a
//!   body settles: no hop, treading, stop-start or turning back, for
//!   `SETTLE_WATCH_MS` past the scene; 18 a body at a door neither stalls
//!   nor passes through it shut; 19 what a script orders is done: a held
//!   button stays held, a talent ordered is used, and a grenade thrown while
//!   a conversation holds the world is listed; 20 cause before effect: a
//!   grenade's burst before the deaths beside it, a shot before its hit,
//!   deaths and blasts in the shot (`scene_log`); 21 two seconds past a
//!   scene its bodies stand or lie on the floor, its `EffectDeath`s have
//!   killed, no grenade is left.
//! - **The report.** Each module is checked by a child `kjk` (`--jobs` at
//!   once): `report.json` and `SUMMARY.md` in the run folder hold every
//!   failure with its module, DLG and node, grouped by check and cause, the
//!   conversations reached, and the participants' hooks no run raised. What
//!   K1's executable does too is a note, not a failure: a scripted clip the
//!   body's chain lacks (no fallback), an `AnimList` value neither table
//!   names, a voice the install does not ship, a reply's `AnimList`, a talk
//!   run's stall or unframed shot (out of its story's order), a line's
//!   camera its area lacks, a bark asked for as one.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};
use std::sync::{Arc, Mutex};

use asset_odyssey::creature::{self, CreatureLook, Tables};
use asset_odyssey::model::ModelIr;
use bevy::math::Vec3;
use collision::{Contents, Extents};
use fmt_odyssey::ResType;
use kjk_game::play_view::{LineShots, ShotKind};
use kjk_game::viewer::{self, ResolverSource};
use serde_json::{Value, json};
use sim::combat::idle::Acting;
use sim::module::ObjectData;
use sim::script::Library;
use sim::script::dialogue::{Conversation, Line};
use sim::script::effects::blows;
use sim::{EntityState, Instances, Kind, SimId, Snapshot};
use vfs::resolver::Resolver;
use vfs::{Discovery, Game};

use crate::dialogue_verbs::{Run, Step, play_watched};
use crate::lifecycle::Installer;
use crate::motion::{self, Drawn, Motion};
use crate::scene_log::{self, Event, Scene};

/// How long a talk run steps at most (s).
const TALK_MOST_S: u64 = 240;
/// How many more talk runs each object's conversation gets, each taking
/// another reply at an offer the runs before took the first of
/// (`branches`).
const BRANCH_RUNS: usize = 4;
/// How long a run steps on after its conversations have all ended (ms).
const AFTER_MS: u64 = 3000;
/// How long more (ms) a run steps on while a body still walks a script's
/// move begun before its conversations ended (a walk out, which a forced
/// move gives 30 s by default before its jump).
const WALK_OUT_MS: u64 = 40_000;
/// Check 14's causes for a move a door shut in a talk run closes.
const NO_WAY_SHUT: &str = "no way leads there: a door its story opens is shut (a talk run, out of story order)";
const GAVE_UP_SHUT: &str =
    "the walk is given up short: a door its story opens is shut (a talk run, out of story order)";
const TIMED_OUT_SHUT: &str =
    "a forced move jumps there at its timeout: a door its story opens is shut (a talk run, out of story order)";
/// A conversation with no new line for this long (ms) has stalled.
const STALL_MS: u64 = 60_000;
/// When a line's shot is looked at (ms after it shows).
const SHOT_AT_MS: [u64; 2] = [300, 1500];
/// How far a jump may land from its mark (m).
const JUMP_M: f32 = 0.5;
/// How long after a blow begins it lands: half a round, as the sim's scene
/// blows do (`effects::Strike`).
const HALF_ROUND_MS: u64 = sim::combat::ROUND_MS / 2;
/// How long after a blow begins its target's reaction, sound and bolt are
/// looked for (ms).
const BLOW_WINDOW_MS: u64 = 3500;
/// Two bodies closer than this (m, flat) at a line's start stand inside
/// each other.
const INSIDE_M: f32 = 0.3;
/// The heights (m above the soles) a body is looked for in a shot: its
/// head (a little under `kjk_game::cast::HEAD_M`) and its chest.
const LOOKED_AT_M: [f32; 2] = [1.6, 1.0];
/// The screen's width to its height.
const ASPECT: f32 = 16.0 / 9.0;
/// `ANIMATION_LOOPING_*` poses a body lies in: dead, dead3, sleep, prone.
const LYING: [u16; 4] = [26, 30, 22, 23];
/// How far (m) from a line's speaker bodies' weapons are checked.
const WEAPONS_NEAR_M: f32 = 15.0;
/// How long past a scene its bodies are watched (ms): a body stopped by its
/// last line hops, if it will, within this.
const SETTLE_WATCH_MS: u64 = 2000;
/// How near a door (m) a body's stall is the door's (18).
const AT_DOOR_M: f32 = 3.0;
/// How near a door (m) a body's step is traced for passing through it.
const THROUGH_DOOR_M: f32 = 2.0;
/// A shot's eye jumping farther than this (m) in a step is a cut.
const CUT_M: f32 = 1.0;
/// How far before an effect (ms) its cause may come unremarked, and how far
/// from a burst (m) a death is laid to it (20).
const ORDER_SLACK_MS: u64 = 100;
const BURST_M: f32 = 6.0;
const BURST_BEFORE_MS: u64 = 4000;
/// How long (ms) a talent ordered has to be used (19): a throw's or
/// channel's start, or a walk within reach.
const TALENT_MS: u64 = 6000;
/// `visualeffects.2da` labels that are blasts (20).
const BLAST_LABELS: [&str; 5] = ["gren", "expl", "blast", "fireball", "deton"];

/// The checks' names, by number.
const INVARIANTS: [(u8, &str); 21] = [
    (1, "animations resolve"),
    (2, "voice is heard"),
    (3, "cameras frame someone"),
    (4, "scripted attacks give feedback"),
    (5, "weapons are shown"),
    (6, "the dead stay down"),
    (7, "jumps land on their marks"),
    (8, "conversations end"),
    (9, "bodies stay on the floor"),
    (10, "census gaps reached"),
    (11, "module-only resources read from the base"),
    (12, "free play in a held scene"),
    (13, "lines are said by whom they name"),
    (14, "scripted moves arrive"),
    (15, "lips move"),
    (16, "bodies move on moving legs"),
    (17, "bodies settle"),
    (18, "doors are no way through or stall"),
    (19, "what a script orders is done"),
    (20, "cause before effect, in the shot"),
    (21, "scenes end clean"),
];

/// One fault: which check, its cause (what groups it: no ids), where
/// (module, DLG, node `E12`/`R3`, `-` out of a line) and what exactly.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Failure {
    invariant: u8,
    cause: String,
    module: String,
    dialog: String,
    node: String,
    detail: String,
}

impl Failure {
    fn json(&self) -> Value {
        json!({ "invariant": self.invariant, "cause": self.cause, "module": self.module,
            "dialog": self.dialog, "node": self.node, "detail": self.detail })
    }
}

/// A line under way: its DLG and node.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Where {
    dialog: String,
    node: String,
}

impl Where {
    fn out_of_line() -> Where {
        Where {
            dialog: "-".into(),
            node: "-".into(),
        }
    }

    fn of(talk: &Conversation) -> Where {
        Where {
            dialog: talk.dialog.clone(),
            node: match (offered(talk), talk.replies.first()) {
                (true, Some(reply)) => format!("R{reply}"),
                _ => format!("E{}", talk.entry),
            },
        }
    }
}

/// Whether `talk` waits on a reply.
fn offered(talk: &Conversation) -> bool {
    talk.until_ms == u64::MAX && !talk.replies.is_empty()
}

/// What a module's runs read once: its resolver and the install's base,
/// the area's placed cameras, the dialog animations' pull-backs, the models
/// read, each body's scripted clips (`viewer::scripted_clips`: by constant,
/// how many events its clip has), the conversations' camera models.
struct Assets {
    game: Game,
    module: String,
    resolver: Resolver,
    base: Resolver,
    cameras: Vec<asset_odyssey::module::Camera>,
    pull_backs: Vec<f32>,
    models: BTreeMap<String, Option<ModelIr>>,
    bound: BTreeMap<(String, bool), Option<Bound>>,
    camera_models: BTreeMap<String, Arc<ModelIr>>,
    tables: Tables,
    /// `animations.2da`'s rows (`creature::animation_rows`), for naming
    /// the animations played by row.
    animation_rows: Vec<(String, bool)>,
    /// Each body's (`look.body`, its own model's) clips as the view times
    /// them, and whether it walks by its legs (a humanoid) and has a walk.
    gaits: BTreeMap<(String, bool), Gait>,
}

/// How a body walks and how long its clips show (`Assets::gait`).
#[derive(Clone, Default)]
struct Gait {
    humanoid: bool,
    walks: bool,
    clips: BTreeMap<u16, (f32, f32)>,
}

impl Assets {
    fn open(installer: &Installer, game: Game, module: &str) -> Result<Assets, String> {
        let resolver = installer.resolver(module)?;
        let base = installer.base_resolver()?.clone();
        let source = ResolverSource(&resolver);
        let ir = asset_odyssey::module::ModuleIr::build(module, &source)
            .map_err(|refusal| format!("{module}: {refusal:?}"))?;
        let cameras = ir
            .areas
            .iter()
            .find(|area| area.resref == ir.entry_area)
            .or(ir.areas.first())
            .map_or_else(Vec::new, |area| area.cameras.clone());
        let animation_rows = creature::animation_rows(&source);
        Ok(Assets {
            game,
            module: module.to_string(),
            pull_backs: viewer::pull_backs(&resolver),
            resolver,
            base,
            cameras,
            models: BTreeMap::new(),
            bound: BTreeMap::new(),
            camera_models: BTreeMap::new(),
            tables: Tables::default(),
            animation_rows,
            gaits: BTreeMap::new(),
        })
    }

    /// `entity`'s look: a seat's by its persona, a creature's by the
    /// appearance a script gave it, else by its template.
    fn look(
        &mut self,
        snapshot: &Snapshot,
        facts: Option<&ObjectData>,
        entity: &EntityState,
    ) -> Option<CreatureLook> {
        let source = ResolverSource(&self.resolver);
        let given = entity.fighter.as_ref().and_then(|f| f.look.appearance);
        let appearance = match entity.kind {
            Kind::Avatar(seat) => Some(snapshot.personas.iter().find(|p| p.seat == seat)?.appearance),
            _ => given,
        };
        match appearance {
            Some(row) => {
                creature::resolve_appearance(&source, u32::from(row), None, &mut self.tables).ok()
            }
            None => creature::resolve_with(&source, &facts?.template, &mut self.tables).ok(),
        }
    }

    /// The constants `look`'s body plays a clip for, and how many events
    /// (its sounds among them) each clip carries.
    fn bound(&mut self, look: &CreatureLook) -> Option<Bound> {
        let body = viewer::cached(&mut self.models, &self.resolver, &look.body).ok()??;
        let own = creature::creature_model(&look.model_type);
        let key = (look.body.clone(), own);
        if let Some(found) = self.bound.get(&key) {
            return found.clone();
        }
        let found =
            viewer::scripted_clips(&mut self.models, &self.resolver, (self.game, own), &body)
                .ok()
                .map(|clips| {
                    Arc::new(
                        clips
                            .iter()
                            .map(|(constant, clip)| (*constant, clip.events.len()))
                            .collect(),
                    )
                });
        self.bound.insert(key, found.clone());
        found
    }

    /// `look`'s gait, read once.
    fn gait(&mut self, look: &CreatureLook) -> Gait {
        let own = creature::creature_model(&look.model_type);
        let key = (look.body.clone(), own);
        if let Some(found) = self.gaits.get(&key) {
            return found.clone();
        }
        let mut gait = Gait::default();
        if let Ok(Some(body)) = viewer::cached(&mut self.models, &self.resolver, &look.body) {
            gait.humanoid = asset_ja::rig::is_humanoid(&body);
            gait.walks = ["cwalk", "walk", "crun", "run"].iter().any(|name| {
                viewer::clip(&mut self.models, &self.resolver, self.game, &body, name)
                    .is_ok_and(|clip| clip.is_some())
            });
            if let Ok(clips) =
                viewer::scripted_clips(&mut self.models, &self.resolver, (self.game, own), &body)
            {
                gait.clips = clips
                    .iter()
                    .map(|(constant, clip)| (*constant, (clip.length, clip.transition)))
                    .collect();
            }
        }
        self.gaits.insert(key, gait.clone());
        gait
    }

    /// What the view draws on a moving `entity` at `ms` (`cast::place`): a
    /// hold's clip or a script's while it shows; a creature model's own walk
    /// by its speed, a humanoid's by its legs. What breaks off as the body
    /// moves is the view's own rule (`acting::kept_on`).
    fn drawn(
        &mut self,
        snapshot: &Snapshot,
        objects: &Instances,
        (entity, fighter): (&EntityState, &sim::combat::Fighter),
        ms: u64,
    ) -> Drawn {
        let Some(look) = self.look(snapshot, objects.of_entity(entity), entity) else {
            return Drawn::Legs;
        };
        let gait = self.gait(&look);
        let [x, y, _] = entity.player.as_ref().map_or([0.0; 3], |player| player.velocity);
        let moving = x.hypot(y) >= kjk_game::cast::STILL_SPEED;
        let acting = kjk_game::cast::drawn_acting(fighter, moving, ms as f64)
            .filter(|acting| acting.animation != sim::combat::death::DIE)
            .filter(|acting| {
                let alive = fighter.alive();
                let shown = (!moving, moving, alive, snapshot.holds_world());
                kjk_game::acting::kept_on(acting, shown, ms as f64)
            });
        if let Some(acting) = acting
            && let Some((length, transition)) = kjk_game::acting::stance_of(acting.animation, 0)
                .into_iter()
                .flatten()
                .find_map(|row| gait.clips.get(&row).copied())
            && (ms as f64) < acting.since as f64 + kjk_game::acting::shows_ms(&acting, length, transition)
        {
            return Drawn::Over(format!("clip {}", self.animation_name(acting.animation)));
        }
        match (gait.humanoid, gait.walks) {
            (true, _) => Drawn::Legs,
            (false, true) => Drawn::Stride,
            (false, false) => Drawn::Over("its model has no walk".to_string()),
        }
    }

    /// `visualeffects.2da`'s label of row `row`.
    fn visual_label(&mut self, row: u16) -> String {
        let source = ResolverSource(&self.resolver);
        self.tables
            .get(&source, "visualeffects")
            .and_then(|table| table.resref(usize::from(row), "label"))
            .unwrap_or_else(String::new)
    }

    /// The camera model of `dlg` and its stunts' models, each read once.
    fn camera_model(&mut self, dlg: &fmt_odyssey::dlg::Dlg) {
        let stunts = dlg.stunts.iter().map(|(_, model)| model);
        for name in std::iter::once(&dlg.camera_model).chain(stunts) {
            let name = name.to_ascii_lowercase();
            if name.is_empty() || self.camera_models.contains_key(&name) {
                continue;
            }
            if let Ok(Some(model)) = viewer::cached(&mut self.models, &self.resolver, &name) {
                self.camera_models.insert(name, Arc::new(model));
            }
        }
    }

    /// `dialoganimations.2da`'s name of row `row`.
    fn dialog_row(&mut self, row: usize) -> String {
        let source = ResolverSource(&self.resolver);
        self.tables
            .get(&source, "dialoganimations")
            .and_then(|table| table.resref(row, "name"))
            .unwrap_or_else(|| "no name".into())
    }

    /// A constant's name for a report.
    fn animation_name(&self, animation: u16) -> String {
        if let Some((kind, row)) = blows::of(animation) {
            let what = match kind {
                blows::SWING => "swing",
                blows::HURT => "hit reaction",
                _ => "guard reaction",
            };
            return format!("{what} {}", self.row_name(row));
        }
        let named = |(constant, name): (u16, &str)| (constant == animation).then(|| name.to_string());
        kjk_game::acting::clips()
            .find_map(named)
            .or_else(|| creature::row_clips(&self.animation_rows).find_map(named))
            .map_or_else(
                || format!("animation {animation}"),
                |name| format!("{name} ({animation})"),
            )
    }

    /// An `animations.2da` row's name.
    fn row_name(&self, row: u16) -> String {
        self.animation_rows
            .get(usize::from(row))
            .map_or_else(|| format!("row {row}"), |(name, _)| name.clone())
    }
}

/// The events of the clip `bound` plays for blow `animation` (its row's
/// own, `acting::stance_of`).
fn blow_events(bound: &BTreeMap<u16, usize>, animation: u16) -> Option<usize> {
    kjk_game::acting::stance_of(animation, 0)
        .into_iter()
        .flatten()
        .find_map(|row| bound.get(&row).copied())
}

/// Whether `bound` plays `animation`: `LOOPING_READY` by any wield's ready
/// stance (`acting::stance_of`).
fn resolves(bound: &BTreeMap<u16, usize>, animation: u16) -> bool {
    use kjk_game::acting::{LOOPING_READY, READY};
    match animation {
        LOOPING_READY => (READY..=READY + 9).any(|c| bound.contains_key(&c)),
        _ if blows::of(animation).is_some() => blow_events(bound, animation).is_some(),
        _ => bound.contains_key(&animation),
    }
}

/// A body's soles (game space, metres).
fn soles(entity: &EntityState) -> Vec3 {
    entity.player.as_ref().map_or(Vec3::from_array(entity.position), |player| {
        kjk_game::cast::soles(player.origin)
    })
}

/// Whether `entity` shoots with what it holds.
fn gunner(entity: &EntityState, objects: &Instances) -> bool {
    let sheet = entity
        .sheet
        .or_else(|| objects.of_entity(entity).and_then(|f| f.sheet));
    let hand = entity.fighter.as_ref().map_or(0, |f| f.hand);
    sheet.is_some_and(|sheet| sheet.gunner(hand))
}

/// A body's scripted clips: by constant, how many events its clip has.
type Bound = Arc<BTreeMap<u16, usize>>;

/// A line's shot samples: where, and each whether it framed someone and
/// how it was filmed.
type Samples = (Where, Vec<(bool, String)>);

/// A blow seen begin: its attacker, its target, its row (none: an assured
/// blow, the attacker's own swing standing in), when, whether a gunner's,
/// and the line.
#[derive(Clone)]
struct Blow {
    attacker: SimId,
    target: Option<SimId>,
    row: Option<u16>,
    since: u64,
    gunner: bool,
    /// Whether the attacker's clip carries events (its sounds); none where
    /// its body is unknown.
    sounded: Option<bool>,
    at: Where,
}

/// What one run saw, step by step.
#[derive(Default)]
struct Watch {
    lines: BTreeSet<(u32, u64, bool)>,
    shots: BTreeMap<u32, LineShots>,
    /// Per line (conversation, shown ms): where, and the shot samples
    /// taken (whether each framed someone, how it was filmed).
    framed: BTreeMap<(u32, u64), Samples>,
    actings: BTreeSet<(u32, u16, u64)>,
    /// Reactions played: who, from when, the constant.
    reactions: Vec<(u32, u64, u16)>,
    blows: Vec<Blow>,
    strikes: BTreeSet<(u32, u32, u64)>,
    /// Shots seen fired: (shooter, when), by a bolt in flight or by its gun's
    /// timer starting over (a disruptor's beam has no bolt, `shot_fx`).
    missiles: BTreeSet<(u32, u64)>,
    /// Each fighter's weapon timer the step before; its keys are the
    /// fighters seen.
    timers: BTreeMap<u32, i32>,
    dead: BTreeMap<u32, u64>,
    lying: BTreeMap<u32, (u16, Vec3)>,
    gaps: BTreeMap<String, usize>,
    pains: BTreeMap<u32, u64>,
    /// Creatures seen swinging JA's own moves in a held scene (12).
    ja_swings: BTreeSet<u32>,
    /// The bodies whose weapons were checked, with what they wore then
    /// (a script's equip is checked again), and those found holding none
    /// out (5).
    weapons: BTreeSet<(u32, [Option<String>; 2])>,
    unshown: BTreeSet<u32>,
    timeline: Vec<(u64, Where)>,
    runs_at: BTreeMap<String, usize>,
    names: BTreeMap<u32, String>,
    voiced: BTreeSet<(String, u32)>,
    last_running: u64,
    /// The spans (ms, first and last step) a conversation ran through.
    running: Vec<(u64, u64)>,
    /// The step before's time (ms).
    stepped: u64,
    started: bool,
    /// The doors' and placeables' states the last step, by instance (14).
    doors: BTreeMap<u32, sim::ObjectState>,
    /// Every body's motion, standing or moved (16, 17, 18).
    motion: Motion,
    /// When a body last walked a script's move: the scene is watched
    /// `SETTLE_WATCH_MS` past it.
    walked_ms: u64,
    /// Each body's seconds spent near a door, by `Motion`'s name for it (18).
    at_doors: BTreeSet<(String, u64)>,
    /// Bodies' last places (m) while a conversation runs.
    places: BTreeMap<u32, [f32; 3]>,
    /// Lobs in flight: where each was last seen, its owner.
    lobs: BTreeMap<u32, (u32, [f32; 3], bool)>,
    /// Each mind's held button the step before, and until when (19).
    holds: BTreeMap<u32, (u16, u64)>,
    /// Those let go early: when, who, which, how long it had left, and
    /// whether a talent of its script's stood.
    let_go: Vec<(u64, u32, u16, u64, bool)>,
    /// The shot's eye last step (m).
    eye: Option<[f32; 3]>,
    /// How many of the report's effects were read.
    took_read: usize,
    /// Talents seen ordered: (who, its row, from when), and whether used.
    talents: BTreeMap<(u32, u16), (u64, bool, Where)>,
    /// Scenes' ends judged (21), by the conversation span's start.
    ended: BTreeSet<u64>,
    /// The bodies of the span under way: its participants (21).
    cast: BTreeSet<u32>,
    /// `EffectDeath`s taken: on whom, when.
    killed: BTreeMap<u32, (u64, Where)>,
    /// What each conversation saw (`scene_log`).
    scenes: BTreeMap<String, Scene>,
}

/// What a module's runs share: the conversations reached, the nodes shown,
/// the participants' objects, the failures, and the notes: what a check
/// finds that K1's executable does too (a clip the body lacks plays
/// nothing, with no fallback; a voice the install does not ship is not
/// heard; a reply's `AnimList` is not played), reported as information.
#[derive(Default)]
struct Found {
    failures: BTreeSet<Failure>,
    notes: BTreeSet<Failure>,
    reached: BTreeSet<String>,
    shown: BTreeSet<(String, bool, u32)>,
    participants: BTreeMap<String, Arc<ObjectData>>,
    runs: BTreeMap<String, usize>,
    /// What each conversation saw over all its runs (20).
    scenes: BTreeMap<String, Scene>,
}

/// One run's checker.
struct Checker<'a> {
    assets: &'a mut Assets,
    found: &'a mut Found,
    library: Arc<Library>,
    watch: Watch,
    /// A talk run: its player was put beside the object it uses
    /// (`by_hand::talk_to`), not on a floor of the scene's choosing, so its
    /// own footing is not judged.
    talked: bool,
}

impl Checker<'_> {
    fn fail(&mut self, invariant: u8, cause: impl Into<String>, at: &Where, detail: impl Into<String>) {
        let failure = self.failure(invariant, cause, at, detail);
        self.found.failures.insert(failure);
    }

    /// What K1's executable does too: information, not a failure.
    fn note(&mut self, invariant: u8, cause: impl Into<String>, at: &Where, detail: impl Into<String>) {
        let note = self.failure(invariant, cause, at, detail);
        self.found.notes.insert(note);
    }

    fn failure(&self, invariant: u8, cause: impl Into<String>, at: &Where, detail: impl Into<String>) -> Failure {
        Failure {
            invariant,
            cause: cause.into(),
            module: self.assets.module.clone(),
            dialog: at.dialog.clone(),
            node: at.node.clone(),
            detail: detail.into(),
        }
    }

    /// The line under way at `ms` (the latest begun by then, while a
    /// conversation runs), else none.
    fn at(&self, ms: u64) -> Where {
        if !self.watch.running.iter().any(|(from, to)| (*from..=*to).contains(&ms)) {
            return Where::out_of_line();
        }
        self.watch
            .timeline
            .iter()
            .rev()
            .find(|(from, _)| *from <= ms)
            .map_or_else(Where::out_of_line, |(_, at)| at.clone())
    }

    /// The line of the conversation `moved` is a move of (14): one whose
    /// scripts gave its mover actions (`Report::directed`) before the move
    /// began, the move begun while it ran or within `AFTER_MS` of its end;
    /// the line under way then, or its last.
    fn directing(&self, report: &sim::script::Report, moved: &sim::script::Moved) -> Option<Where> {
        let from = moved.from_ms;
        let span = self.watch.running.iter().rev().find(|(began, to)| {
            let close = to + AFTER_MS;
            from <= close
                && (report.directed.iter())
                    .any(|(who, ms)| *who == moved.who && (*began..=from.min(close)).contains(ms))
        })?;
        Some(match self.at(from) {
            at if at.dialog != "-" => at,
            _ => self.at(span.1),
        })
    }

    fn name(&self, id: u32) -> String {
        self.watch
            .names
            .get(&id)
            .cloned()
            .unwrap_or_else(|| format!("#{id}"))
    }

    /// Whether the run goes on: until its conversations have ended and
    /// `AFTER_MS` more, once one began, and no body walks a script's move
    /// still (`WALK_OUT_MS` at most; talk runs only, `stop_quiet`).
    fn step(&mut self, step: &Step, stop_quiet: bool) -> bool {
        let (snapshot, world, now) = (step.snapshot, step.world, step.now);
        let level = world.level();
        let objects = Instances::in_snapshot(level, snapshot);
        for entity in &snapshot.entities {
            if let std::collections::btree_map::Entry::Vacant(slot) = self.watch.names.entry(entity.id.0) {
                match entity.kind {
                    Kind::Avatar(_) => {
                        slot.insert("PLAYER".into());
                    }
                    _ => {
                        if let Some(facts) = objects.of_entity(entity) {
                            slot.insert(facts.tag.clone());
                        }
                    }
                }
            }
        }
        self.watch_motion(step);
        let talks = &snapshot.talk.conversations;
        if let Some(first) = talks.first() {
            let at = Where::of(first);
            if self.watch.timeline.last().is_none_or(|(_, last)| *last != at) {
                self.watch.timeline.push((now, at));
            }
            self.watch.last_running = now;
            self.watch.started = true;
            match self.watch.running.last_mut() {
                Some(span) if span.1 == self.watch.stepped => span.1 = now,
                _ => self.watch.running.push((now, now)),
            }
        }
        let report = world.script_report();
        for talk in talks {
            self.found.reached.insert(talk.dialog.clone());
            for script in self
                .library
                .dialogue(&talk.dialog)
                .into_iter()
                .flat_map(|dlg| [dlg.on_end.clone(), dlg.on_abort.clone()])
            {
                if !script.is_empty() {
                    let key = sim::script::key(&script);
                    let ran = report.runs.get(&key).copied().unwrap_or(0);
                    self.watch.runs_at.entry(key).or_insert(ran);
                }
            }
            if self.watch.lines.insert((talk.id, talk.shown_ms, offered(talk))) {
                self.line_start(snapshot, world, &objects, talk);
            }
            self.shot(snapshot, world, talk, now);
        }
        // Routines unbound, called while a conversation runs (10).
        let mut called = Vec::new();
        for (routine, count) in &report.gaps {
            let before = self.watch.gaps.insert(routine.clone(), *count).unwrap_or(0);
            if *count > before {
                called.push(routine.clone());
            }
        }
        if let Some(talk) = talks.first() {
            for routine in called {
                self.fail(10, format!("unbound routine {routine}"), &Where::of(talk), "");
            }
        }
        let held: BTreeSet<u32> = if snapshot.holds_world() {
            talks
                .iter()
                .flat_map(|talk| [talk.speaker.0, talk.listener.0, talk.owner.0, talk.pc.0])
                .collect()
        } else {
            BTreeSet::new()
        };
        // Who stands in which line.
        let mut parts: BTreeMap<u32, Where> = BTreeMap::new();
        for talk in talks {
            let mut ids = vec![talk.speaker.0, talk.listener.0, talk.owner.0, talk.pc.0];
            ids.extend(talk.joined.iter().map(|id| id.0));
            for id in ids {
                parts.entry(id).or_insert_with(|| Where::of(talk));
            }
        }
        for entity in &snapshot.entities {
            if let Some(missile) = &entity.missile {
                self.watch.missiles.insert((missile.owner.0, now));
            }
            if let Some(fighter) = &entity.fighter {
                let id = entity.id.0;
                let before = self.watch.timers.insert(id, fighter.weapon_wait_ms).unwrap_or(0);
                if fighter.weapon_wait_ms > before && gunner(entity, &objects) {
                    self.watch.missiles.insert((id, now));
                }
                self.fighter(snapshot, &objects, (entity, fighter), parts.get(&id), (now, held.contains(&id)));
            }
        }
        // A scene's swing names its target by the script's blow (a
        // `CutsceneAttack` sets no fighter's target).
        let strikes = &world.script_state().strikes;
        for blow in self.watch.blows.iter_mut().filter(|blow| blow.target.is_none()) {
            if let Some(strike) = strikes.iter().find(|strike| strike.by.0 == blow.attacker.0) {
                blow.target = Some(SimId(strike.target.0));
            }
        }
        // Blows the scripts decided (4): a row's plays on its attacker's
        // `Acting`; an assured one's is its attacker's own swing.
        for strike in strikes {
            if self.watch.strikes.insert((strike.by.0, strike.target.0, strike.due_ms)) && strike.animation == 0 {
                let gunner = snapshot
                    .get(SimId(strike.by.0))
                    .is_some_and(|a| gunner(a, &objects));
                let at = self.at(now);
                self.watch.blows.push(Blow {
                    attacker: SimId(strike.by.0),
                    target: Some(SimId(strike.target.0)),
                    row: None,
                    since: strike.starts_ms(),
                    gunner,
                    sounded: None,
                    at,
                });
            }
        }
        self.watch.stepped = now;
        self.watch.doors = snapshot
            .entities
            .iter()
            .filter_map(|entity| match (entity.kind, entity.object) {
                (sim::Kind::Object(_, index), Some(state)) => Some((index, state)),
                _ => None,
            })
            .collect();
        let walking = snapshot
            .entities
            .iter()
            .any(|entity| entity.fighter.as_ref().is_some_and(|f| f.alive() && f.mind.moving));
        if walking {
            self.watch.walked_ms = now;
        }
        self.scene_events(step);
        let settled = now > self.watch.walked_ms + SETTLE_WATCH_MS;
        let over = now > self.watch.last_running + AFTER_MS
            && (settled || now > self.watch.last_running + WALK_OUT_MS);
        !(stop_quiet && self.watch.started && talks.is_empty() && over)
    }

    /// Every body's step for `Motion` (16, 17), what the view draws on it
    /// told; and whether it stands near a door, or steps through one shut
    /// (18), while a conversation runs or just after.
    fn watch_motion(&mut self, step: &Step) {
        let (snapshot, level, now) = (step.snapshot, step.level, step.now);
        let objects = Instances::in_snapshot(level, snapshot);
        let mut motion = std::mem::take(&mut self.watch.motion);
        let assets = &mut *self.assets;
        motion.watch_drawn(snapshot, level, step.msec, &mut |entity, fighter| {
            assets.drawn(snapshot, &objects, (entity, fighter), now)
        });
        let scene = !snapshot.talk.conversations.is_empty()
            || now <= self.watch.last_running + SETTLE_WATCH_MS && self.watch.started;
        if scene {
            let doors: Vec<(&EntityState, sim::ObjectState)> = snapshot
                .entities
                .iter()
                .filter_map(|entity| match (entity.kind, entity.object) {
                    (Kind::Object(sim::module::ObjectKind::Door, _), Some(state))
                        if state.state != collision::GONE =>
                    {
                        Some((entity, state))
                    }
                    _ => None,
                })
                .collect();
            let (mut shut_states, mut open_states) = (None, None);
            for entity in snapshot.entities.iter().filter(|entity| entity.is_actor()) {
                let (Some(fighter), Some(_)) = (entity.fighter.as_ref(), entity.player.as_ref()) else {
                    continue;
                };
                if !fighter.alive() {
                    continue;
                }
                let here = entity.position;
                let was = self.watch.places.insert(entity.id.0, here);
                let flat = |door: &&(&EntityState, sim::ObjectState)| {
                    (door.0.position[0] - here[0]).hypot(door.0.position[1] - here[1])
                };
                if doors.iter().any(|door| flat(&door) <= AT_DOOR_M)
                    && let Some(name) = motion.name_of(entity.id.0)
                {
                    self.watch.at_doors.insert((name.to_string(), now / 1000));
                }
                let Some(was) = was.filter(|was| (was[0] - here[0]).hypot(was[1] - here[1]) > 0.005) else {
                    continue;
                };
                // Straight through a door shut, as the step before and this
                // one have it: blocked with it, free with every door open.
                if doors.iter().any(|door| door.1.state == 0 && flat(&door) <= THROUGH_DOOR_M) {
                    let shut = shut_states.get_or_insert_with(|| level.states_in(snapshot));
                    let open = open_states.get_or_insert_with(|| level.states_open_in(snapshot));
                    let chest = |at: [f32; 3]| [at[0], at[1], at[2] + 1.0];
                    let blocked = |states: &Option<Vec<u8>>| {
                        level.with(states.as_deref(), |w| {
                            w.trace(chest(was), chest(here), &Extents::default(), Contents::SOLID).fraction < 1.0
                        })
                    };
                    if blocked(shut) && !blocked(open) {
                        let at = self.at(now);
                        let detail = format!(
                            "{} at {:.1},{:.1} {:.1} s{}",
                            self.name(entity.id.0),
                            here[0],
                            here[1],
                            now as f32 / 1000.0,
                            if fighter.mind.moving { ", a script's move" } else { "" }
                        );
                        match self.talked && !fighter.mind.moving {
                            true => self.note(18, "a body passes through a door shut (a talk run)", &at, detail),
                            false => self.fail(18, "a body passes through a door shut", &at, detail),
                        }
                    }
                }
            }
        }
        self.watch.motion = motion;
    }

    /// What this step adds to its scene's log (20) and to what scripts
    /// ordered (19): deaths, effects taken, lobs thrown and burst, movers,
    /// talents; and a scene's end judged once it is `SETTLE_WATCH_MS` past (21).
    fn scene_events(&mut self, step: &Step) {
        let (snapshot, world, now) = (step.snapshot, step.world, step.now);
        let talk = snapshot.talk.conversations.first().cloned();
        let at = talk.as_ref().map_or_else(Where::out_of_line, Where::of);
        let objects = Instances::in_snapshot(step.level, snapshot);
        let held = snapshot.holds_world();
        if let Some(talk) = &talk {
            let pair = kjk_game::cast::line_pair(snapshot, talk);
            self.watch.cast.extend([pair.speaker.0, pair.listener.0, talk.owner.0, talk.pc.0]);
            self.watch.cast.extend(talk.joined.iter().map(|id| id.0));
        }
        let shot = talk.as_ref().and_then(|talk| self.camera(snapshot, world, talk, now));
        let camera = shot.as_ref().map(|(frustum, _)| frustum);
        let seen = |point: [f32; 3]| camera.map(|frustum| frustum.holds(Vec3::from_array(point)));
        let mut events = Vec::new();
        // The shot cut: its eye jumped.
        let eye = shot.as_ref().map(|(_, eye)| eye.to_array());
        if let (Some(eye), Some(was)) = (eye, self.watch.eye)
            && Vec3::from_array(eye).distance(Vec3::from_array(was)) > CUT_M
        {
            let what = format!("eye {:.1},{:.1},{:.1}", eye[0], eye[1], eye[2]);
            events.push(Event::new(now, "cut", String::new(), what, None, eye));
        }
        self.watch.eye = eye;
        let report = world.script_report();
        // Who a script of this conversation directs.
        let began = self.watch.running.last().map_or(u64::MAX, |span| span.0);
        let directed = |who: u32| report.directed.iter().any(|(id, ms)| id.0 == who && *ms >= began);
        // Deaths and bodies moved while a conversation runs.
        for entity in snapshot.entities.iter().filter(|e| e.is_actor()) {
            let Some(fighter) = &entity.fighter else { continue };
            let id = entity.id.0;
            if talk.is_some() {
                self.watch.cast.extend(fighter.target.map(|t| t.0));
            }
            if talk.is_some() && fighter.dead() && self.watch.dead.get(&id) == Some(&now) {
                self.watch.cast.insert(id);
                let chest = [entity.position[0], entity.position[1], entity.position[2] + 1.0];
                events.push(Event::new(now, "death", self.name(id), String::new(), seen(chest), entity.position));
            }
            if let (true, Some(talk), true) = (held, &talk, fighter.alive()) {
                let why = match fighter.mind.moving {
                    true => "a script's move".to_string(),
                    false => format!("its AI ({:?})", fighter.mind.task),
                };
                let name = self.name(id);
                let scene = self.watch.scenes.entry(talk.dialog.clone()).or_default();
                scene.moved(id, name, why, entity.position);
            }
        }
        // A mind's held button let go before its time, with no fight on.
        let intents = &world.script_state().intents;
        for entity in &snapshot.entities {
            let Some(fighter) = &entity.fighter else { continue };
            let mind = &fighter.mind;
            let id = entity.id.0;
            if let Some((button, until)) = self.watch.holds.get(&id).copied()
                && until > now
                && mind.hold_until <= now
                && fighter.alive()
                && mind.task != sim::combat::ai::Task::Engage
            {
                let talent = intents.get(&id).is_some_and(|intent| intent.talent.is_some());
                self.watch.let_go.push((now, id, button, until - now, talent));
            }
            match mind.hold_until > now {
                true => self.watch.holds.insert(id, (mind.hold, mind.hold_until)),
                false => self.watch.holds.remove(&id),
            };
        }
        // Effects taken: blasts and deaths told.
        let took = &report.took[self.watch.took_read.min(report.took.len())..];
        self.watch.took_read = report.took.len();
        let mut killed = Vec::new();
        for effect in took {
            let target = snapshot.get(SimId(effect.target.0));
            let place = effect.at.or(target.map(|t| t.position));
            if effect.death && !effect.cast {
                killed.push(effect.target.0);
            }
            let labels: Vec<String> = effect.visuals.iter().map(|row| self.assets.visual_label(*row)).collect();
            let blast = labels
                .iter()
                .find(|label| BLAST_LABELS.iter().any(|b| label.to_ascii_lowercase().contains(b)));
            if talk.is_none() {
                continue;
            }
            let chest = place.map(|p| [p[0], p[1], p[2] + 1.0]);
            if let Some(label) = blast {
                let by = self.name(effect.by.0);
                events.push(Event::new(now, "blast", by, label.clone(), chest.and_then(&seen), place.unwrap_or([0.0; 3])));
            } else if effect.death {
                let on = self.name(effect.target.0);
                events.push(Event::new(now, "effect death", on, self.name(effect.by.0), chest.and_then(&seen), place.unwrap_or([0.0; 3])));
            }
        }
        for id in killed {
            self.watch.killed.entry(id).or_insert((now, at.clone()));
        }
        // Lobs: thrown, burst where last seen.
        let mut flying = BTreeSet::new();
        let lobs: Vec<(u32, u32, [f32; 3])> = snapshot
            .entities
            .iter()
            .filter_map(|e| Some((e.id.0, e.missile.as_ref().filter(|m| m.lob.is_some())?.owner.0, e.position)))
            .collect();
        for (id, owner, there) in lobs {
            flying.insert(id);
            if !self.watch.lobs.contains_key(&id) && talk.is_some() {
                let ordered = directed(owner);
                let who = self.name(owner);
                let what = if ordered { "ordered by the conversation's scripts" } else { "its AI's own" };
                events.push(Event::new(now, "throw", who.clone(), what.to_string(), seen(there), there));
                let cause = match (held, ordered) {
                    (true, true) => "a conversation's script throws a grenade while it holds the world",
                    (true, false) => "a creature's AI throws a grenade while a conversation holds the world",
                    (false, _) => "a grenade thrown during a conversation that holds nothing",
                };
                self.note(19, cause, &at, format!("{who} {:.1} s", now as f32 / 1000.0));
            }
            self.watch.lobs.insert(id, (owner, there, talk.is_some()));
        }
        let burst: Vec<u32> = self.watch.lobs.keys().filter(|id| !flying.contains(id)).copied().collect();
        for id in burst {
            let Some((owner, there, _)) = self.watch.lobs.remove(&id) else { continue };
            if talk.is_some() || self.watch.started && now <= self.watch.last_running + AFTER_MS {
                events.push(Event::new(now, "burst", self.name(owner), String::new(), seen(there), there));
            }
        }
        // Talents ordered and whether they came to anything.
        for (&who, intent) in &world.script_state().intents {
            let Some(sim::script::actions::Use::Power(row)) = intent.talent else { continue };
            if talk.is_some() {
                self.watch.talents.entry((who, row)).or_insert((now, false, at.clone()));
            }
        }
        for ((who, _), (_, used, _)) in &mut self.watch.talents {
            let acted = snapshot.get(SimId(*who)).and_then(|e| e.fighter.as_ref()).is_some_and(|f| {
                use sim::buttons;
                let held_down = buttons::FORCE_POWER
                    | buttons::FORCE_GRIP
                    | buttons::FORCE_LIGHTNING
                    | buttons::FORCE_DRAIN
                    | buttons::GRENADE
                    | buttons::ALT_ATTACK;
                f.last_command.buttons & held_down != 0 || f.pack.using.is_some() || f.pack.charging_from.is_some()
            });
            *used |= acted;
        }
        if let Some(talk) = &talk {
            let scene = self.watch.scenes.entry(talk.dialog.clone()).or_default();
            scene.events.extend(events);
        } else if let Some(dialog) = self.watch.timeline.last().map(|(_, at)| at.dialog.clone()) {
            self.watch.scenes.entry(dialog).or_default().events.extend(events);
        }
        // A scene's end, once it is past.
        if talk.is_none()
            && self.watch.started
            && now > self.watch.last_running + SETTLE_WATCH_MS
            && let Some(span) = self.watch.running.last().copied()
            && self.watch.ended.insert(span.0)
        {
            self.scene_end(snapshot, step.level, &objects);
        }
    }

    /// A scene `SETTLE_WATCH_MS` past (21): its bodies stand or lie on the
    /// floor, those the script killed dead, none stuck in a loop, no grenade
    /// left.
    fn scene_end(&mut self, snapshot: &Snapshot, level: &sim::Level, _objects: &Instances) {
        let last = self.at(self.watch.last_running);
        let states = level.states_in(snapshot);
        let cast = std::mem::take(&mut self.watch.cast);
        let now = snapshot.time_ms;
        for &id in &cast {
            let Some(entity) = snapshot.get(SimId(id)) else { continue };
            let (Some(fighter), Some(player)) = (entity.fighter.as_ref(), entity.player.as_ref()) else {
                continue;
            };
            if self.talked && matches!(entity.kind, Kind::Avatar(_)) {
                continue;
            }
            let who = self.name(id);
            let feet = soles(entity);
            let floor = !footing_faults(level, states.as_deref(), feet, None).is_empty();
            match (fighter.alive(), floor) {
                (true, true) => self.judged_end("a living body stands with no floor under it", &last, format!("{who} at {:.2?}", feet.to_array())),
                (false, true) => self.judged_end("a dead body lies with no floor under it", &last, format!("{who} at {:.2?}", feet.to_array())),
                _ => {}
            }
            if fighter.alive() && player.ground.is_none() {
                self.judged_end("a living body is off the floor", &last, who.clone());
            }
            if fighter.alive()
                && let Some(acting) = fighter.acting.filter(|a| a.ms == u32::MAX && !LYING.contains(&a.animation))
            {
                let name = self.assets.animation_name(acting.animation);
                self.note(21, format!("a looping clip goes on past the scene: {name}"), &last, who.clone());
            }
            if fighter.alive() && fighter.force.stunned_until > now + 10_000 {
                self.note(21, "a hold outlasts the scene by over 10 s", &last, who.clone());
            }
        }
        let killed = std::mem::take(&mut self.watch.killed);
        for (id, (ms, at)) in killed {
            let alive = snapshot.get(SimId(id)).and_then(|e| e.fighter.as_ref()).is_some_and(|f| f.alive());
            if alive {
                let detail = format!("{} at {:.1} s", self.name(id), ms as f32 / 1000.0);
                self.judged_end("an EffectDeath left its target alive", &at, detail);
            }
        }
        let lobs = snapshot.entities.iter().filter(|e| e.missile.as_ref().is_some_and(|m| m.lob.is_some())).count();
        if lobs > 0 {
            self.judged_end("a grenade is still live past the scene", &last, format!("{lobs}"));
        }
    }

    /// 21's fault: a failure, a talk run's (out of story order) a note.
    fn judged_end(&mut self, cause: &str, at: &Where, detail: String) {
        match self.talked {
            true => self.note(21, format!("{cause} (a talk run)"), at, detail),
            false => self.fail(21, cause, at, detail),
        }
    }

    /// `Motion`'s episodes into failures (16, 17, 18): those of a conversation
    /// or within `SETTLE_WATCH_MS` of its end, once a cause, body and line.
    fn motion_faults(&mut self) {
        let motion = std::mem::take(&mut self.watch.motion);
        let mut seen = BTreeSet::new();
        for episode in motion.kept() {
            let at = match self.at(episode.from_ms) {
                at if at.dialog != "-" => at,
                _ => {
                    let past = (self.watch.running.iter().rev())
                        .find(|(_, to)| *to <= episode.from_ms && episode.from_ms <= to + SETTLE_WATCH_MS);
                    match past {
                        Some(span) => self.at(span.1),
                        None => continue,
                    }
                }
            };
            let in_line = self.at(episode.from_ms).dialog != "-";
            let by = if episode.scripted { "a script's move" } else { "its AI" };
            let door = (self.watch.at_doors).contains(&(episode.who.clone(), episode.from_ms / 1000));
            let detail = format!(
                "{} {:.2}-{:.2} s at {:.1},{:.1}",
                episode.who,
                episode.from_ms as f32 / 1000.0,
                episode.to_ms as f32 / 1000.0,
                episode.at[0],
                episode.at[1]
            );
            let (invariant, cause) = match &episode.kind {
                motion::Kind::Slide(_) if !episode.held => continue,
                motion::Kind::Slide(how) => (16, format!("crosses the floor with {how} drawn, by {by}")),
                motion::Kind::Hop(how) => (17, format!("a body will not settle: {how}")),
                // After the line, a body's AI is free play's.
                motion::Kind::Jitter(_) | motion::Kind::Treads if !in_line && !episode.scripted => continue,
                motion::Kind::Jitter(how) if door => (18, format!("jitters at a door ({how}), by {by}")),
                motion::Kind::Treads if door => (18, format!("treads at a door, by {by}")),
                motion::Kind::Jitter(how) => (17, format!("jitters ({how}), by {by}")),
                motion::Kind::Treads => (17, format!("treads in place, by {by}")),
            };
            if !seen.insert((invariant, cause.clone(), episode.who.clone(), at.dialog.clone(), at.node.clone())) {
                continue;
            }
            // A talk run opens its scene out of story order: a body's own AI
            // there is its fight with the player, not the scene's.
            match self.talked && !episode.scripted {
                true => self.note(invariant, format!("{cause} (a talk run)"), &at, detail),
                false => self.fail(invariant, cause, &at, detail),
            }
        }
    }

    /// Scripts' orders and what came of them (19), and what the scenes' logs
    /// put out of order or off camera (20).
    fn order_faults(&mut self, report: &sim::script::Report) {
        for (ms, who, button, left, talent) in std::mem::take(&mut self.watch.let_go) {
            let at = self.at(ms);
            if at.dialog == "-" {
                continue;
            }
            let detail = format!("{} button {button} with {left} ms left at {:.1} s", self.name(who), ms as f32 / 1000.0);
            match talent {
                true => self.fail(19, "a held power its script chose is let go out of a fight", &at, detail),
                false => self.note(19, "a held button let go out of a fight", &at, detail),
            }
        }
        let end = self.watch.stepped;
        let talents = std::mem::take(&mut self.watch.talents);
        for ((who, row), (from, used, at)) in talents {
            if !used && end > from + TALENT_MS {
                let detail = format!("{} row {row} at {:.1} s", self.name(who), from as f32 / 1000.0);
                let directed = report.directed.iter().any(|(id, ms)| id.0 == who && *ms <= from);
                match directed && !self.talked {
                    true => self.fail(19, "a talent its script ordered is never used", &at, detail),
                    false => self.note(19, "a talent never used (its AI's, or a talk run's)", &at, detail),
                }
            }
        }
        // A gunner's hit before its bolt.
        let blows = self.watch.blows.clone();
        for blow in blows.iter().filter(|blow| blow.gunner && blow.at.dialog != "-") {
            let Some(target) = blow.target else { continue };
            let window = blow.since..blow.since + BLOW_WINDOW_MS;
            let shot = (self.watch.missiles.iter()).find(|(owner, ms)| *owner == blow.attacker.0 && window.contains(ms));
            let Some(&(_, shot)) = shot else { continue };
            let name = format!("{} at {}", self.name(blow.attacker.0), self.name(target.0));
            let timed = |what: u64| format!("{name}: {:.2} s, the shot {:.2} s", what as f32 / 1000.0, shot as f32 / 1000.0);
            // A round's two clips begin together, as KOTOR plays them: the
            // answer's first frames before the bolt are no fault; a death is.
            if let Some(died) = self.watch.dead.get(&target.0).copied().filter(|died| window.contains(died))
                && died + ORDER_SLACK_MS < shot
            {
                let detail = timed(died);
                self.fail(20, "a target dies before the shot that kills it", &blow.at, detail);
            }
            let hit = (self.watch.reactions.iter()).find(|(who, ms, _)| *who == target.0 && window.contains(ms));
            if let Some(&(_, hit, _)) = hit
                && hit + ORDER_SLACK_MS < shot
            {
                let detail = timed(hit);
                self.note(20, "a target's answer begins before the shot (the round's clips begin together)", &blow.at, detail);
            }
        }
        let scenes = std::mem::take(&mut self.watch.scenes);
        for (dialog, scene) in scenes {
            let at = Where { dialog: dialog.clone(), node: "-".into() };
            for (cause, faithful, detail) in scene_log::faults(&scene, BURST_M, BURST_BEFORE_MS, ORDER_SLACK_MS) {
                match faithful || self.talked {
                    true => self.note(20, cause, &at, detail),
                    false => self.fail(20, cause, &at, detail),
                }
            }
            self.found.scenes.entry(dialog).or_default().merge(scene);
        }
    }

    /// A fighter's step: the dead (6), its animation (1, 4), and JA's free
    /// play while it is in a held scene (12).
    fn fighter(
        &mut self,
        snapshot: &Snapshot,
        objects: &Instances,
        (entity, fighter): (&EntityState, &sim::combat::Fighter),
        part: Option<&Where>,
        (now, held): (u64, bool),
    ) {
        let id = entity.id.0;
        let at = self.at(now);
        let in_line = at.dialog != "-";
        if fighter.dead() {
            self.watch.dead.entry(id).or_insert(now);
        } else if fighter.alive() && self.watch.dead.remove(&id).is_some() && in_line {
            self.fail(6, "the dead rose", &at, self.name(id));
        }
        match fighter.acting.filter(|a| a.playing(now)) {
            Some(acting) if LYING.contains(&acting.animation) && acting.ms == u32::MAX => {
                self.watch.lying.insert(id, (acting.animation, soles(entity)));
            }
            Some(_) => {}
            None => {
                if let Some((pose, there)) = self.watch.lying.remove(&id)
                    && fighter.alive()
                    && soles(entity).distance(there) < 0.2
                    && in_line
                {
                    let detail = format!("{} ({})", self.name(id), self.assets.animation_name(pose));
                    self.fail(6, "a lying pose ended to idle where it lay", &at, detail);
                }
            }
        }
        if let Some(acting) = fighter.acting
            && self.watch.actings.insert((id, acting.animation, acting.since))
        {
            let own = part.cloned().unwrap_or_else(Where::out_of_line);
            self.acting(snapshot, objects, entity, acting, (&at, &own));
        }
        if held {
            let before = self
                .watch
                .pains
                .insert(id, fighter.pain_until)
                .unwrap_or(fighter.pain_until);
            if fighter.pain_until > before {
                self.fail(12, "JA pain flinch", &at, self.name(id));
            }
            if fighter.saber.lock.is_some() {
                self.fail(12, "saber lock", &at, self.name(id));
            }
            // A creature fights a held scene in KOTOR's rounds: a JA swing
            // of its own begun while it holds lands nothing and looks
            // unconnected (Taris's gang war before the rounds' clips); one
            // under way as the scene began plays out.
            let swing = fighter.saber.current_move;
            if !matches!(entity.kind, Kind::Avatar(_))
                && swing.start()
                && self.watch.ja_swings.insert(id)
            {
                let target = fighter.target.map_or_else(|| "nobody".into(), |t| self.name(t.0));
                let detail = format!("{} at {target}", self.name(id));
                // A talk run opens a conversation out of its story's order:
                // a speaker still hostile fights the player it talks to,
                // where the story makes it give up first
                // (`SurrenderToEnemies`, `CancelCombat`: the beaten
                // Vulkar's `k_ptar_vulcow_ud`), and reone, as the
                // executables, goes on fighting through a conversation.
                let at_seat = fighter
                    .target
                    .and_then(|t| snapshot.get(t))
                    .is_some_and(|t| matches!(t.kind, Kind::Avatar(_)));
                match self.talked && at_seat {
                    true => self.note(12, "a hostile speaker fights the player in a talk run (out of story order)", &at, detail),
                    false => self.fail(12, "JA swing in a scene round", &at, detail),
                }
            }
        }
    }

    /// An animation begun on a body: its clip (1), reported at the line
    /// the body stands in (`own`; out of a line, free play's), and a
    /// blow's part (4), at the line under way (`at`).
    fn acting(
        &mut self,
        snapshot: &Snapshot,
        objects: &Instances,
        entity: &EntityState,
        acting: Acting,
        (at, own): (&Where, &Where),
    ) {
        let id = entity.id.0;
        let bound = self
            .assets
            .look(snapshot, objects.of_entity(entity), entity)
            .and_then(|look| Some((self.assets.bound(&look)?, look.body)));
        match blows::of(acting.animation) {
            Some((blows::SWING, row)) => {
                // A swing the body has no clip for is silent, as it is
                // unseen: check 1's note, not a missing sound.
                let sounded = bound
                    .as_ref()
                    .and_then(|(bound, _)| blow_events(bound, acting.animation).map(|events| events > 0));
                self.watch.blows.push(Blow {
                    attacker: entity.id,
                    target: entity.fighter.as_ref().and_then(|f| f.target),
                    row: Some(row),
                    since: acting.since,
                    gunner: gunner(entity, objects),
                    sounded,
                    at: at.clone(),
                });
            }
            Some(_) => self.watch.reactions.push((id, acting.since, acting.animation)),
            None if acting.animation == sim::combat::death::DIE => {
                self.watch.reactions.push((id, acting.since, acting.animation));
            }
            None => {}
        }
        let Some((bound, body)) = bound else {
            return;
        };
        // K1's executable plays a scripted clip by its name on the body's
        // chain with no fallback: a body without it plays nothing.
        if !resolves(&bound, acting.animation) {
            let what = self.assets.animation_name(acting.animation);
            let cause = match blows::of(acting.animation) {
                Some(_) => format!("no clip for a blow's {}", what.split(' ').next().unwrap_or("")),
                None => format!("no clip for {what}"),
            };
            self.note(1, cause, own, format!("{} ({body}): {what}", self.name(id)));
        }
    }

    /// A line's start: its gestures (1), its voice (2), its participants'
    /// footing (9) and the weapons near its speaker (5, 11).
    fn line_start(&mut self, snapshot: &Snapshot, world: &sim::SimWorld, objects: &Instances, talk: &Conversation) {
        let library = self.library.clone();
        let Some(dlg) = library.dialogue(&talk.dialog) else {
            return;
        };
        let at = Where::of(talk);
        self.assets.camera_model(dlg);
        let pair = kjk_game::cast::line_pair(snapshot, talk);
        for id in [pair.speaker, pair.listener, SimId(talk.owner.0)] {
            if let Some(facts) = snapshot.get(id).and_then(|e| objects.of_entity(e)) {
                self.found
                    .participants
                    .entry(facts.tag.clone())
                    .or_insert_with(|| Arc::new(facts.clone()));
            }
        }
        let shown = offered(talk);
        let index = match shown {
            true => talk.replies.first().copied().unwrap_or(0),
            false => talk.entry,
        };
        self.found.shown.insert((talk.dialog.clone(), !shown, index));
        if !shown && let Some(node) = dlg.entries.get(talk.entry as usize) {
            self.speaker(snapshot, objects, node, talk, &at);
            self.gestures(node, &at);
            if self.watch.voiced.insert((talk.dialog.clone(), talk.entry)) {
                self.voice(snapshot, objects, node, talk, &at);
            }
        }
        self.footing(snapshot, world, talk, &at);
        let hidden = match (dlg.unequip_items, dlg.unequip_held) {
            (true, _) => kjk_game::weapons::Hide::All,
            (false, true) => kjk_game::weapons::Hide::Held,
            (false, false) => kjk_game::weapons::Hide::None,
        };
        self.weapons(snapshot, objects, (pair.speaker, hidden), &at);
    }

    /// An entry is said by whom its `Speaker` names (13): none named, the
    /// owner; else a body of that tag (any case), never the owner standing
    /// in for one not here (both executables pass such a line over); and
    /// the view's pair (camera, facing; the voice and lips read the same
    /// speaker) has that body speaking. A tag several objects share is a
    /// note: the nearest the owner speaks (`Step::speaker_for`).
    fn speaker(
        &mut self,
        snapshot: &Snapshot,
        objects: &Instances,
        node: &fmt_odyssey::dlg::Node,
        talk: &Conversation,
        at: &Where,
    ) {
        let said_by = SimId(talk.speaker.0);
        if kjk_game::cast::line_pair(snapshot, talk).speaker != said_by {
            self.fail(13, "the view's speaker is not the line's", at, self.name(said_by.0));
        }
        if node.speaker.is_empty() {
            if talk.speaker != talk.owner {
                self.fail(13, "a line naming no speaker said by another than its owner", at, self.name(said_by.0));
            }
            return;
        }
        let tag_of = |entity: &EntityState| objects.of_entity(entity).map_or("", |facts| facts.tag.as_str());
        let named = sim::script::key(&node.speaker);
        let speaker_tag = snapshot.get(said_by).map(tag_of);
        let detail = format!("Speaker {}, said by {}", node.speaker, self.name(said_by.0));
        match speaker_tag {
            None => self.fail(13, "the speaker is not in the world", at, detail.clone()),
            Some(tag) if tag == named => {}
            Some(_) if talk.speaker == talk.owner => {
                self.fail(13, "a line said by its owner for one not here", at, detail.clone());
            }
            Some(_) => self.fail(13, "a line said by a body of another tag", at, detail.clone()),
        }
        let sharing = snapshot.entities.iter().filter(|e| tag_of(e) == named).count();
        if sharing > 1 {
            self.note(13, "a speaker's tag several objects share", at, format!("{detail} ({sharing} answer)"));
        }
    }

    /// A line's `AnimList` (1): each value a gesture, a cutscene clip, or
    /// nothing.
    fn gestures(&mut self, node: &fmt_odyssey::dlg::Node, at: &Where) {
        for (tag, value) in &node.anims {
            if fmt_odyssey::dlg::cut_clip(*value).is_some() {
                continue;
            }
            // Values in neither table, and rows with no name, play nothing
            // in K1's executable too (docs/DIALOGUE.md, "Gestures").
            match value.checked_sub(10_000) {
                None => self.note(1, format!("AnimList value {value} in neither table"), at, tag.clone()),
                Some(row) if self.library.dialog_animation(*value).is_none() => {
                    let label = self.assets.dialog_row(usize::from(row));
                    self.note(1, format!("dialoganimations row {row} ({label}) plays nothing"), at, tag.clone());
                }
                Some(_) => {}
            }
        }
    }

    /// A voiced entry's voice (2): in the install, decoding, and heard from
    /// the line's camera.
    fn voice(&mut self, snapshot: &Snapshot, objects: &Instances, node: &fmt_odyssey::dlg::Node, talk: &Conversation, at: &Where) {
        let named: Vec<&str> = [node.sound.as_str(), node.vo.as_str()]
            .into_iter()
            .filter(|name| !name.is_empty())
            .collect();
        if named.is_empty() {
            return;
        }
        let Some((name, _)) = self.library.voice(node) else {
            let resolver = &self.assets.resolver;
            let detail = named.join(" ");
            if named.iter().any(|name| resolver.voice(name).is_some()) {
                self.fail(2, "voice in the install but not timed by the library", at, detail);
            } else if named.iter().any(|name| resolver.find(name, ResType::WAV).is_some()) {
                self.fail(2, "voice outside streamwaves/streamvoice", at, detail);
            } else {
                // The game does not ship it: the line is silent there too,
                // timed by its text.
                self.note(2, "voice missing from the install", at, detail);
            }
            return;
        };
        let speaker = snapshot
            .get(SimId(talk.speaker.0))
            .map_or(Vec3::ZERO, |e| soles(e) + Vec3::Z * LOOKED_AT_M[0]);
        if let Some(why) =
            kjk_game::dialogue_view::voice_unheard(&self.assets.resolver, name, (speaker, speaker))
        {
            self.fail(2, format!("voice {why}"), at, name.to_string());
        }
        self.lips(snapshot, objects, (node, name), talk, at);
    }

    /// A voiced entry's lips (15): its LIP (the voice played's, else the
    /// node's other sound's, as the view reads them), and a creature
    /// speaker's mouth with a `talk` clip that keys it.
    fn lips(
        &mut self,
        snapshot: &Snapshot,
        objects: &Instances,
        (node, voice): (&fmt_odyssey::dlg::Node, &str),
        talk: &Conversation,
        at: &Where,
    ) {
        let resolver = &self.assets.resolver;
        let lip = [voice, node.sound.as_str(), node.vo.as_str()]
            .into_iter()
            .any(|name| !name.is_empty() && resolver.find(name, ResType::LIP).is_some());
        if !lip {
            self.note(15, "voiced line with no LIP (no lips move)", at, voice.to_string());
            return;
        }
        let Some(entity) = snapshot.get(SimId(talk.speaker.0)) else {
            return;
        };
        if !matches!(entity.kind, Kind::Object(sim::module::ObjectKind::Creature, _)) {
            return;
        }
        let Some(look) = self.assets.look(snapshot, objects.of_entity(entity), entity) else {
            return;
        };
        let assets = &mut *self.assets;
        let Ok(Some(body)) = viewer::cached(&mut assets.models, &assets.resolver, &look.body) else {
            return;
        };
        let head = look
            .head
            .as_deref()
            .and_then(|head| viewer::cached(&mut assets.models, &assets.resolver, head).ok().flatten());
        let unmoved = kjk_game::dialogue_view::lips_unmoved(
            &mut assets.models,
            &assets.resolver,
            assets.game,
            (head.as_ref(), &body),
        );
        let who = self.name(entity.id.0);
        let detail = format!("{who} ({})", look.head.as_deref().unwrap_or(&look.body));
        match (unmoved, head) {
            (Some(_), _) => self.note(15, "no talk clip in its model chain (a mask, a droid: nothing moves in reone either)", at, detail),
            // Counted for the census: the lips moved on a body's own face.
            (None, None) => self.note(15, "lips on the body (its own face, no head)", at, detail),
            (None, Some(_)) => {}
        }
    }

    /// The participants' footing at a line's start (9).
    fn footing(&mut self, snapshot: &Snapshot, world: &sim::SimWorld, talk: &Conversation, at: &Where) {
        let level = world.level();
        let pair = kjk_game::cast::line_pair(snapshot, talk);
        let mut bodies = vec![pair.speaker.0, pair.listener.0, talk.owner.0, talk.pc.0];
        bodies.extend(talk.joined.iter().map(|id| id.0));
        bodies.sort_unstable();
        bodies.dedup();
        let standing = |entity: &&EntityState| {
            entity.is_actor()
                && entity.player.is_some()
                && entity.fighter.as_ref().is_none_or(|f| f.alive())
        };
        let states = level.states_in(snapshot);
        for &id in &bodies {
            let Some(entity) = snapshot.get(SimId(id)).filter(standing) else {
                continue;
            };
            if self.talked && matches!(entity.kind, Kind::Avatar(_)) {
                continue;
            }
            let feet = soles(entity);
            let origin = entity.player.as_ref().map(|player| player.origin);
            let place = format!("{} at {:.2?}", self.name(id), feet.to_array());
            for cause in footing_faults(level, states.as_deref(), feet, origin) {
                self.fail(9, cause, at, place.clone());
            }
            for other in snapshot.entities.iter().filter(standing) {
                let there = soles(other);
                let flat = (feet - there).truncate().length();
                if other.id.0 != id
                    && flat < INSIDE_M
                    && (feet.z - there.z).abs() < 1.0
                    && (!bodies.contains(&other.id.0) || other.id.0 > id)
                {
                    let detail = format!("{} and {}, {flat:.2} m apart", self.name(id), self.name(other.id.0));
                    self.fail(9, "a participant stands inside another body", at, detail);
                }
            }
        }
    }

    /// The weapons of the bodies near a line's speaker (5): each has a
    /// model, and one at least is out in a hand where the view draws them
    /// (`weapons::Hold::place`: a creature's, which carries none) unless
    /// the conversation hides them (`hidden`) or an animation does; a
    /// template only the module holds (11).
    fn weapons(&mut self, snapshot: &Snapshot, objects: &Instances, (speaker, hidden): (SimId, kjk_game::weapons::Hide), at: &Where) {
        let Some(near) = snapshot.get(speaker).map(soles) else {
            return;
        };
        let library = self.library.clone();
        let catalog = library.items();
        let ms = snapshot.time_ms as f64;
        let fought: BTreeSet<SimId> = snapshot
            .entities
            .iter()
            .filter_map(|e| e.fighter.filter(sim::combat::Fighter::alive)?.target)
            .collect();
        for entity in &snapshot.entities {
            if !entity.is_actor() || soles(entity).distance(near) > WEAPONS_NEAR_M {
                continue;
            }
            let Some(sheet) = entity
                .sheet
                .or_else(|| objects.of_entity(entity).and_then(|f| f.sheet))
            else {
                continue;
            };
            if sheet.weapons.iter().all(Option::is_none) {
                continue;
            }
            let who = self.name(entity.id.0);
            if let Some(fighter) = entity.fighter.filter(|f| f.alive() && matches!(entity.kind, Kind::Object(..)))
                && hidden == kjk_game::weapons::Hide::None
            {
                use kjk_game::weapons::{self, Hold, Place};
                let armed = weapons::armed(&fighter, ms, fought.contains(&entity.id));
                let hiding = fighter.acting.is_some_and(|a| kjk_game::acting::hides_items(&a, ms));
                let held = weapons::in_hand(armed, false, (false, hiding));
                let out = usize::from(fighter.hand.min(1));
                let hold = Hold::of(&sheet);
                let shown = (0..2).any(|slot| {
                    sheet.weapons[slot].is_some()
                        && hold.place((slot, out), (held, armed, false, true), hidden).0 == Place::Hand
                });
                if held && !shown && self.watch.unshown.insert(entity.id.0) {
                    let detail = format!("{who}: hand {out} out, armed {armed}");
                    self.fail(5, "equipped weapons drawn in no hand", at, detail);
                }
            }
            let worn = kjk_game::weapons::worn(snapshot, objects, Some(catalog), (entity, &sheet));
            if !self.watch.weapons.insert((entity.id.0, worn.clone())) {
                continue;
            }
            match kjk_game::weapons::sheet_models(&self.assets.resolver, &mut self.assets.models, &sheet, &worn) {
                Ok(models) => {
                    for (slot, model) in models.iter().enumerate() {
                        if let (Some(weapon), None) = (sheet.weapons[slot], model) {
                            let what = worn[slot].as_deref().unwrap_or("-");
                            let detail = format!("{who}: {what} (base item {})", weapon.base_item);
                            self.fail(5, "no model for an equipped weapon", at, detail);
                        }
                    }
                }
                Err(error) => self.fail(5, "a weapon model does not load", at, format!("{who}: {error}")),
            }
            for template in worn.iter().flatten() {
                let resref = fmt_odyssey::resref_of(template);
                if self.assets.base.find(resref, ResType::UTI).is_none()
                    && self.assets.resolver.find(resref, ResType::UTI).is_some()
                {
                    // The weapon sounds read the sheet's base item, which the
                    // sim reads through the module: nothing is missed.
                    let cause = "weapon template only in the module (its sounds by the sheet's base item)";
                    self.note(11, cause, at, format!("{who}: {template}"));
                }
            }
        }
    }

    /// The view from `talk`'s shot at `now`, as the view takes it.
    fn camera(&mut self, snapshot: &Snapshot, world: &sim::SimWorld, talk: &Conversation, now: u64) -> Option<(Frustum, Vec3)> {
        let level = world.level();
        let states = level.states_in(snapshot);
        let walls = |from: Vec3, to: Vec3| {
            level.with(states.as_deref(), |w| {
                w.trace(from.to_array(), to.to_array(), &Extents::default(), Contents::SOLID)
                    .fraction
                    < 1.0
            })
        };
        let staged: BTreeMap<SimId, Vec3> =
            kjk_game::play_view::staged_places(level, snapshot, talk, &self.assets.camera_models, now as f64)
                .into_iter()
                .collect();
        let head = |id: SimId| {
            snapshot.get(id).map(|e| {
                staged.get(&e.id).copied().unwrap_or_else(|| soles(e)) + Vec3::Z * kjk_game::cast::HEAD_M
            })
        };
        let shots = self.watch.shots.entry(talk.id).or_default();
        let (_, (eye, angles, fov)) = shots.shot(
            (level, snapshot, talk, now as f64),
            (&self.assets.cameras, &self.assets.pull_backs, &self.assets.camera_models),
            &head,
            &walls,
        )?;
        Some((Frustum::of(eye, angles, fov), Vec3::from(eye)))
    }

    /// The line's shot at `SHOT_AT_MS` after it showed (3): whether its
    /// speaker or listener is in the frustum, unoccluded.
    fn shot(&mut self, snapshot: &Snapshot, world: &sim::SimWorld, talk: &Conversation, now: u64) {
        let line = (talk.id, talk.shown_ms);
        let taken = self.watch.framed.get(&line).map_or(0, |(_, samples)| samples.len());
        let Some(due) = SHOT_AT_MS.get(taken) else {
            return;
        };
        if now < talk.shown_ms + due && (taken > 0 || talk.until_ms > talk.shown_ms + due) {
            return;
        }
        let level = world.level();
        let states = level.states_in(snapshot);
        let walls = |from: Vec3, to: Vec3| {
            level.with(states.as_deref(), |w| {
                w.trace(from.to_array(), to.to_array(), &Extents::default(), Contents::SOLID)
                    .fraction
                    < 1.0
            })
        };
        // A staged line's participants stand where the view draws them, on
        // their stunts' marks (`play_view::staged_places`), not where the
        // sim has them.
        let staged: BTreeMap<SimId, Vec3> =
            kjk_game::play_view::staged_places(level, snapshot, talk, &self.assets.camera_models, now as f64)
                .into_iter()
                .collect();
        let soles = |e: &EntityState| staged.get(&e.id).copied().unwrap_or_else(|| soles(e));
        let head = |id: SimId| {
            snapshot
                .get(id)
                .map(|e| soles(e) + Vec3::Z * kjk_game::cast::HEAD_M)
        };
        let shots = self.watch.shots.entry(talk.id).or_default();
        let Some((kind, (eye, angles, fov))) = shots.shot(
            (level, snapshot, talk, now as f64),
            (&self.assets.cameras, &self.assets.pull_backs, &self.assets.camera_models),
            &head,
            &walls,
        ) else {
            return;
        };
        let frustum = Frustum::of(eye, angles, fov);
        // A placed or animated camera stands where its scene's maker put
        // it, often past the walkmesh's walls the collision is made of: only
        // the dialog camera's shots are tested against them, as the view's
        // own set-up tests them.
        let walled = matches!(kind, ShotKind::Dialog(..));
        let seen = |point: Vec3| frustum.holds(point) && !(walled && walls(frustum.eye, point));
        let pair = kjk_game::cast::line_pair(snapshot, talk);
        // Anyone in frame will do (a cutscene's speaker is often an unseen
        // object while its camera films the fight it tells); a door or a
        // placeable speaking is its own wall, so in the frustum will do.
        let framed = snapshot.entities.iter().any(|e| {
            let feet = soles(e);
            match e.is_actor() {
                true => LOOKED_AT_M.iter().any(|h| seen(feet + Vec3::Z * *h)),
                false => {
                    [pair.speaker, pair.listener].contains(&e.id)
                        && [kjk_game::cast::HEAD_M, LOOKED_AT_M[0], LOOKED_AT_M[1]]
                            .iter()
                            .any(|h| frustum.holds(feet + Vec3::Z * *h))
                }
            }
        });
        let mut seen_as = Vec::new();
        for id in [pair.speaker, pair.listener] {
            let Some(feet) = snapshot.get(id).map(soles) else {
                continue;
            };
            let head = feet + Vec3::Z * LOOKED_AT_M[0];
            let off = frustum.off_axis(head);
            let wall = if walls(frustum.eye, head) { ", behind a wall" } else { "" };
            seen_as.push(format!("{} {off:.0} deg off axis{wall}", self.name(id.0)));
        }
        let staged = self.library.dialogue(&talk.dialog).is_some_and(|dlg| dlg.staged());
        let how = match kind {
            ShotKind::Animated => "animated camera".to_string(),
            ShotKind::Placed(id) => format!("placed camera {id}"),
            ShotKind::Dialog(angle, _) => format!("dialog angle {angle}"),
        };
        let how = format!(
            "{how}{}: fov {fov:.0}, {}",
            if staged { " (staged)" } else { "" },
            seen_as.join("; ")
        );
        self.watch
            .framed
            .entry(line)
            .or_insert_with(|| (Where::of(talk), Vec::new()))
            .1
            .push((framed, how));
    }

    /// What is judged once the run is over: the shots (3), the blows (4),
    /// the jumps (7), the conversations' ends (8), barks in held scenes (12)
    /// and replies' gestures (10).
    /// Whether only a door shut now closes `moved`'s way (14): from where it
    /// stood as it ended to where it was sent, no way leads through the
    /// doors as the last step had them (none opens for the held party,
    /// S4), and one does with every door open.
    fn shut_by_door(&self, level: &sim::Level, moved: &sim::script::Moved) -> bool {
        let state = |index: u32| {
            (self.watch.doors.get(&index).copied()).unwrap_or(sim::ObjectState::GONE)
        };
        let now = |index: u32| match moved.held_party {
            true => sim::walk::opening_none(state(index)),
            false => state(index),
        };
        let open = |index: u32| sim::ObjectState { state: 1, ..state(index) };
        !sim::walk::leads(level, moved.at, moved.to, &now) && sim::walk::leads(level, moved.at, moved.to, &open)
    }

    fn finish(&mut self, world: &sim::SimWorld, end_ms: u64) {
        let framed = std::mem::take(&mut self.watch.framed);
        for (at, samples) in framed.values() {
            if !samples.is_empty() && samples.iter().all(|(framed, _)| !framed) {
                let how = &samples[0].1;
                let kind = how.split([':', '(']).next().unwrap_or("").trim();
                let staged = if how.contains("(staged)") { " (staged)" } else { "" };
                // A line naming a camera its area lacks is filmed from the
                // area's origin (Q-0230); a talk run's scene starts out of
                // its story's order, its bodies off the marks its scripts
                // would have walked them to.
                let missing = how
                    .strip_prefix("placed camera ")
                    .and_then(|rest| rest.split(':').next()?.parse::<i32>().ok())
                    .is_some_and(|id| !self.assets.cameras.iter().any(|camera| camera.id == id));
                match (missing, self.talked) {
                    (true, _) => self.note(3, "the line's camera is not in its area (Q-0230)", at, how.clone()),
                    (false, true) => {
                        let cause = format!("{kind} frames nobody{staged} in a talk run (out of story order)");
                        self.note(3, cause, at, how.clone());
                    }
                    (false, false) => self.fail(3, format!("{kind} frames nobody{staged}"), at, how.clone()),
                }
            }
        }
        self.motion_faults();
        self.order_faults(world.script_report());
        self.blows();
        // One item a slot (5): a carried stack equipped whole, by a
        // script's equip, left the other hand bare (Calo Nord's two blasters,
        // one stack, in Taris's Lower City apartments; he ran in and
        // punched). A slot holding its template's own stack (a template
        // equips an item of `StackSize` 5) is the game's data, a note.
        let catalog = self.library.items().clone();
        for item in world.script_state().items.values() {
            if item.slot.is_none() || item.stack <= 1 {
                continue;
            }
            let who = self.name(item.holder);
            let detail = format!("{who}: {} x{}", item.template, item.stack);
            let own = catalog.get(&item.template).map_or(1, |facts| facts.stack);
            match item.stack > own {
                true => self.fail(5, "a carried stack is worn in one slot", &Where::out_of_line(), detail),
                false => self.note(5, "a template wears its item's own stack", &Where::out_of_line(), detail),
            }
        }
        let report = world.script_report();
        // A talk run's own walk-up (`by_hand::talk_to`), its player's first
        // jump, is the harness's.
        let walk_up = report
            .jumps
            .iter()
            .position(|jump| self.name(jump.who.0) == "PLAYER")
            .filter(|_| self.talked);
        for (index, jump) in report.jumps.iter().enumerate() {
            let at = self.at(jump.ms);
            if at.dialog == "-" || walk_up == Some(index) {
                continue;
            }
            if let Some((cause, off)) = jump_fault(jump) {
                let by = jump.displaced_by.map_or_else(|| "-".into(), |by| self.name(by));
                let detail = format!("{} {off:.2} m off {:.2?}, by {by}", self.name(jump.who.0), jump.mark);
                // A body on the mark, or a mark off the walkmesh, moves the
                // jump on to the rings, as the executables' safe-location
                // search does.
                match jump.displaced_by.is_some() || !jump.on_mesh {
                    true => self.note(7, cause, &at, detail),
                    false => self.fail(7, cause, &at, detail),
                }
            }
        }
        // A script's move ends where it was sent (14): one with no way there,
        // its walk given up short, or a forced one jumped there at its
        // timeout failed to walk (a cantina's walk out through its door to
        // the Lower City, Taris). A move is a conversation's when its mover
        // is one the conversation's scripts gave actions to
        // (`Report::directed`), the move begun after that while it ran or
        // within `AFTER_MS` of its end (its end script's walk out); any other
        // (a walkway's, an AI's wander, both the creature's own routine) is
        // free play's, a note. A forced move whose timeout comes while its
        // walk still carries it on is the script's own plan (K1's walkways
        // time a leg at 1.25 m/s, 30 s at least, and a longer way round
        // outlasts it), a note too; and so, in a talk run, is a move whose
        // way only a door shut now closes (the story opens it first: the
        // Leviathan's cells before the escape).
        let level = world.level();
        for moved in &report.moves {
            let shut = || self.talked && self.shut_by_door(level, moved);
            let (cause, faithful) = match moved.ended {
                sim::script::Ended::Arrived => continue,
                sim::script::Ended::NoWay if shut() => (NO_WAY_SHUT, true),
                sim::script::Ended::NoWay => ("no way leads there", false),
                sim::script::Ended::GaveUp if shut() => (GAVE_UP_SHUT, true),
                sim::script::Ended::GaveUp => ("the walk is given up short", false),
                sim::script::Ended::TimedOut if shut() => (TIMED_OUT_SHUT, true),
                sim::script::Ended::TimedOut => ("a forced move jumps there at its timeout", false),
                sim::script::Ended::TimedOutWalking => (
                    "a forced move's timeout comes before its walk ends (the script's jump)",
                    true,
                ),
            };
            let at = self.directing(report, moved);
            let detail = format!(
                "{} {:.1}-{:.1} s",
                self.name(moved.who.0),
                moved.from_ms as f32 / 1000.0,
                moved.to_ms as f32 / 1000.0
            );
            match at {
                Some(at) if !faithful => self.fail(14, cause, &at, detail),
                Some(at) => self.note(14, cause, &at, detail),
                None => self.note(14, cause, &Where::out_of_line(), detail),
            }
        }
        let mut lines: BTreeMap<u32, (String, String, u64, Option<bool>)> = BTreeMap::new();
        for line in &report.talk {
            match line {
                Line::Entry { ms, id, dialog, entry, .. } => {
                    lines.insert(*id, (dialog.clone(), format!("E{entry}"), *ms, None));
                }
                Line::Picked { ms, id, dialog, reply } => {
                    lines.insert(*id, (dialog.clone(), format!("R{reply}"), *ms, None));
                    let anims = self
                        .library
                        .dialogue(dialog)
                        .and_then(|dlg| dlg.replies.get(*reply as usize))
                        .is_some_and(|node| !node.anims.is_empty());
                    // K1's executable gestures entries alone.
                    if anims {
                        let at = Where { dialog: dialog.clone(), node: format!("R{reply}") };
                        self.note(10, "a reply's AnimList is not played", &at, "");
                    }
                }
                Line::Ended { id, aborted, .. } => {
                    if let Some(line) = lines.get_mut(id) {
                        line.3 = Some(*aborted);
                    }
                }
                Line::Bark { ms, who, conversation, .. } => {
                    let at = self.at(*ms);
                    // One the step a conversation begins in is before it:
                    // a step's scripts bark before its conversations start.
                    let begun = self.watch.running.iter().any(|(from, to)| from < ms && ms <= to);
                    let held = begun && at.dialog != "-";
                    // Both executables refuse a conversation begun while
                    // one holds the world, and show a bark asked for as one
                    // (`SpeakOneLinerConversation`, `ActionBarkString`).
                    match (held, conversation) {
                        (false, _) => {}
                        (true, true) => self.fail(12, "a one-line conversation in a scene", &at, self.name(who.0)),
                        (true, false) => self.note(12, "a bark in a scene (shown by the executables too)", &at, self.name(who.0)),
                    }
                }
                _ => {}
            }
        }
        for (dialog, node, last, ended) in lines.into_values() {
            let at = Where { dialog: dialog.clone(), node };
            match ended {
                None if end_ms.saturating_sub(last) > STALL_MS => {
                    let still = (end_ms - last) / 1000;
                    let detail = format!("no new line for {still} s");
                    // A talk run starts a conversation out of its story's
                    // order: its scene's scripts may meet the story's own
                    // triggers and globals (dan13_zhar's line moves the
                    // player into the council's trigger, which begins it
                    // anew). Its stall is the harness's until a story run
                    // stalls too.
                    match self.talked {
                        true => self.note(8, "stalled on a line in a talk run (out of story order)", &at, detail),
                        false => self.fail(8, "stalled on a line", &at, detail),
                    }
                }
                None => {}
                Some(aborted) => {
                    let Some(dlg) = self.library.dialogue(&dialog) else {
                        continue;
                    };
                    let script = if aborted { &dlg.on_abort } else { &dlg.on_end };
                    if script.is_empty() || self.library.program(script).is_none() {
                        continue;
                    }
                    let key = sim::script::key(script);
                    let before = self.watch.runs_at.get(&key).copied().unwrap_or(0);
                    if report.runs.get(&key).copied().unwrap_or(0) <= before {
                        self.fail(8, "the end script did not run", &at, key);
                    }
                }
            }
        }
        for (script, count) in &report.runs {
            *self.found.runs.entry(script.clone()).or_insert(0) += count;
        }
    }

    /// Each blow's feedback (4): the attacker's clip, its target's
    /// reaction or death, a sound, and a gunner's bolt.
    fn blows(&mut self) {
        let blows = std::mem::take(&mut self.watch.blows);
        for blow in &blows {
            if blow.at.dialog == "-" {
                continue;
            }
            let attacker = self.name(blow.attacker.0);
            let what = blow.row.map_or("an assured blow".to_string(), |row| self.assets.row_name(row));
            let target = blow.target.map_or_else(|| "-".into(), |t| self.name(t.0));
            let detail = format!("{attacker} {what} at {target}");
            // Only a fighter alive as the blow begins answers it (a shot at a
            // placeable, at a body, has nothing to play), and not one in its
            // own swing as the blow lands, which plays on (`blows::struck`).
            let hit = blow.since + HALF_ROUND_MS;
            // Its own swing overlapping the blow's half round takes its
            // body (a scene's two fighters trading blows on one step: the
            // later animation set wins, as KOTOR's do).
            let swinging = |target: SimId| {
                blows.iter().any(|own| {
                    own.attacker == target && own.since <= hit && blow.since <= own.since + HALF_ROUND_MS
                })
            };
            let answers = blow.target.is_some_and(|target| {
                self.watch.timers.contains_key(&target.0)
                    && self.watch.dead.get(&target.0).is_none_or(|died| *died >= blow.since)
                    && !swinging(target)
            });
            let blow = Blow {
                target: blow.target.filter(|_| answers),
                ..blow.clone()
            };
            for cause in blow_faults(&blow, &self.watch.reactions, &self.watch.missiles, &self.watch.dead) {
                self.fail(4, cause, &blow.at, detail.clone());
            }
        }
    }
}

/// A camera's view: its eye (game space, metres), its axes (forward,
/// right, up) and the tangent of half its vertical field of view, on a
/// screen `ASPECT` wide.
struct Frustum {
    eye: Vec3,
    axes: [Vec3; 3],
    tan_v: f32,
}

impl Frustum {
    /// The view from `eye` at view angles `angles`, `fov` degrees high.
    fn of(eye: [f32; 3], angles: [f32; 3], fov: f32) -> Frustum {
        let (forward, right, up) = sim::math::angle_vectors(angles);
        Frustum {
            eye: Vec3::from_array(eye),
            axes: [forward, right, up].map(Vec3::from_array),
            tan_v: (fov.to_radians() / 2.0).tan(),
        }
    }

    /// Whether `point` is on screen.
    fn holds(&self, point: Vec3) -> bool {
        let [forward, right, up] = self.axes;
        let d = point - self.eye;
        let depth = d.dot(forward);
        depth > 0.05
            && d.dot(up).abs() <= self.tan_v * depth
            && d.dot(right).abs() <= self.tan_v * ASPECT * depth
    }

    /// How far (degrees) `point` lies off the view's axis.
    fn off_axis(&self, point: Vec3) -> f32 {
        let along = (point - self.eye).normalize_or_zero().dot(self.axes[0]);
        along.clamp(-1.0, 1.0).acos().to_degrees()
    }
}

/// What a body standing with its soles at `feet` (game space, metres) and
/// its box at `origin` (units) lacks in `level` with its doors at `states`
/// (9): a floor within 0.6 m under it, and room for what KOTOR's walker
/// is in a scene, a point a body high (`sim::WALKER_HALF_WIDTH_M`, as a
/// jump's place is tested): JA's box is free play's.
fn footing_faults(level: &sim::Level, states: Option<&[u8]>, feet: Vec3, origin: Option<[f32; 3]>) -> Vec<&'static str> {
    use sim::motion::{SOLE_Z, STAND_HEIGHT};
    let mut faults = Vec::new();
    let floor = level.with(states, |w| {
        w.trace(
            (feet + Vec3::Z * 0.3).to_array(),
            (feet - Vec3::Z * 0.6).to_array(),
            &Extents::default(),
            Contents::SOLID,
        )
    });
    if floor.fraction >= 1.0 {
        faults.push("no floor under a participant");
    }
    let unit = collision::UNIT;
    let half = sim::WALKER_HALF_WIDTH_M;
    let extents = Extents {
        mins: [-half, -half, SOLE_Z * unit],
        maxs: [half, half, STAND_HEIGHT * unit],
    };
    let stuck = origin.is_some_and(|origin| {
        let at = origin.map(|v| v * unit);
        level.with(states, |w| w.trace(at, at, &extents, Contents::PLAYER_SOLID).start_solid)
    });
    if stuck {
        faults.push("a participant stands in solid geometry");
    }
    faults
}

/// What `jump` came to, if it missed its mark (7): the cause, and how far
/// off it landed (m; 0 where it did not).
fn jump_fault(jump: &sim::script::Jumped) -> Option<(&'static str, f32)> {
    let flat = |at: [f32; 3]| (at[0] - jump.mark[0]).hypot(at[1] - jump.mark[1]);
    // One that finds no place stays: on its mark already, it is there.
    let Some(landed) = jump.landed else {
        let off = flat(jump.from);
        return (off > JUMP_M).then_some(("no free place within 20 m", off));
    };
    let off = flat(landed);
    let cause = match (jump.displaced_by, jump.on_mesh) {
        (Some(_), _) => "a body stood on the mark",
        (None, false) => "the mark lies off the walkmesh",
        (None, true) => "the mark is no free floor",
    };
    (off > JUMP_M).then_some((cause, off))
}

/// What `blow` lacked (4) given the reactions played (who, from when,
/// what), the bolts seen (owner, when) and the deaths (who, when): its
/// target's reaction or death, a gunner's bolt, a sound on its swing.
fn blow_faults(
    blow: &Blow,
    reactions: &[(u32, u64, u16)],
    missiles: &BTreeSet<(u32, u64)>,
    dead: &BTreeMap<u32, u64>,
) -> Vec<&'static str> {
    let window = blow.since..blow.since + BLOW_WINDOW_MS;
    let mut faults = Vec::new();
    if let Some(target) = blow.target {
        let reacted = reactions.iter().any(|(who, ms, _)| *who == target.0 && window.contains(ms))
            || dead.get(&target.0).is_some_and(|ms| window.contains(ms));
        if !reacted {
            faults.push("the target shows no reaction");
        }
    }
    let bolt = missiles.iter().any(|(owner, ms)| *owner == blow.attacker.0 && window.contains(ms));
    if blow.gunner && !bolt {
        faults.push("a gunner's blow fires no shot");
    }
    if !blow.gunner && blow.sounded == Some(false) {
        faults.push("the swing's clip has no sound event");
    }
    faults
}

/// The runs of `module`: the sweep's (`scene_sweep::hints`), then one for
/// each conversation of its objects those did not reach.
fn check_module(discovery: &Discovery, game: Game, module: &str) -> Result<Value, String> {
    let started = std::time::Instant::now();
    let (anims, _) = crate::stage::anims(discovery);
    let installer = Installer::new(discovery, game, Arc::new(anims))?;
    let mut assets = Assets::open(&installer, game, module)?;
    let prepared = installer.prepare_one(module, None)?;
    let library = prepared
        .plan
        .level
        .scripts()
        .cloned()
        .ok_or("the module has no scripts")?;
    let objects = conversations(prepared.plan.level.placed());
    let speakers = crate::speaker_census::census(&library, prepared.plan.level.placed());
    drop(prepared);
    let mut found = Found::default();
    let mut queue: Vec<(String, Run, bool)> = Vec::new();
    for hint in crate::scene_sweep::hints(game.key(), module)? {
        let flags = hint.play_flags().join(" ");
        let label = if flags.is_empty() { "entry".into() } else { flags };
        queue.push((label, hint.run()?, false));
    }
    // Each talk run's tag, the picks it was given and how many runs its
    // conversation has had.
    let mut tried: BTreeSet<(String, Vec<u8>)> = BTreeSet::new();
    let mut branched: BTreeMap<String, usize> = BTreeMap::new();
    let sweeps = queue.len();
    let mut runs = Vec::new();
    let mut at = 0;
    loop {
        if at == sweeps {
            // The objects' conversations those runs did not reach.
            for (tag, dialog) in objects.iter().filter(|(_, d)| !found.reached.contains(d)) {
                tried.insert((tag.clone(), Vec::new()));
                queue.push((format!("--talk {tag} ({dialog})"), talk_run(tag, Vec::new()), true));
            }
        }
        if at == queue.len() {
            break;
        }
        let (label, run, quiet) = &queue[at];
        let (label, quiet, talk, picks) = (label.clone(), *quiet, run.talk.clone(), run.picks.clone());
        at += 1;
        let mut checker = Checker {
            assets: &mut assets,
            found: &mut found,
            library: library.clone(),
            watch: Watch { motion: Motion::everyone(), ..Watch::default() },
            talked: quiet,
        };
        let mut offers = Vec::new();
        let lines = match play_watched(&installer, module, run, &mut |step| checker.step(step, quiet)) {
            Ok(mut played) => {
                let end = played.world.snapshot().time_ms;
                checker.finish(&played.world, end);
                let talk = &played.world.script_report().talk;
                offers = talk
                    .iter()
                    .filter_map(|line| match line {
                        Line::Offered { replies, .. } => Some(replies.len()),
                        _ => None,
                    })
                    .collect();
                Ok(talk.iter().filter(|line| matches!(line, Line::Entry { .. })).count())
            }
            Err(error) => Err(error),
        };
        // A talk run's other replies, a few runs more (`BRANCH_RUNS`).
        if let (true, Some(tag)) = (quiet, talk) {
            let done = branched.entry(tag.clone()).or_insert(0);
            for picks in branches(&picks, &offers) {
                if *done >= BRANCH_RUNS {
                    break;
                }
                if tried.insert((tag.clone(), picks.clone())) {
                    *done += 1;
                    let label = format!("--talk {tag} --picks {picks:?}");
                    queue.push((label, talk_run(&tag, picks), true));
                }
            }
        }
        drop(checker);
        if let Err(error) = &lines {
            found.failures.insert(Failure {
                invariant: 8,
                cause: "the run did not start".into(),
                module: module.to_string(),
                dialog: "-".into(),
                node: "-".into(),
                detail: format!("{label}: {error}"),
            });
        }
        runs.push(json!({ "run": label, "lines": lines.unwrap_or(0) }));
    }
    Ok(module_report(module, started, &runs, (&library, &assets), &mut found, speakers))
}

/// A module's part of `report.json`: `found` over `runs`.
fn module_report(
    module: &str,
    started: std::time::Instant,
    runs: &[Value],
    (library, assets): (&Library, &Assets),
    found: &mut Found,
    speakers: Value,
) -> Value {
    for (dialog, node, field) in unread_fields(&assets.resolver, &found.shown) {
        found.failures.insert(Failure {
            invariant: 10,
            cause: format!("DLG field {field} not read"),
            module: module.to_string(),
            dialog,
            node,
            detail: String::new(),
        });
    }
    json!({
        "module": module,
        "ms": started.elapsed().as_millis() as u64,
        "runs": runs,
        "dialogs": library.dialogues().count(),
        "reached": found.reached,
        "nodes_shown": found.shown.len(),
        "nodes": found.reached.iter().filter_map(|dialog| library.dialogue(dialog))
            .map(|dlg| dlg.entries.len() + dlg.replies.len()).sum::<usize>(),
        "talk_runs": runs.len(),
        "failures": found.failures.iter().map(Failure::json).collect::<Vec<_>>(),
        "notes": found.notes.iter().map(Failure::json).collect::<Vec<_>>(),
        "unraised_hooks": unraised_hooks(found),
        "speakers": speakers,
        "scenes": scene_log::json(&found.scenes),
    })
}

/// The checks over a story's playthrough (`KJK_SCENE_CHECK=1`): every module
/// it passes through, its bodies, party and state the story's own, where a
/// `--talk` run's are the module's as placed. What it finds is the evidence;
/// a talk run's only a lead.
pub(crate) struct StoryCheck<'a> {
    installer: &'a Installer,
    game: Game,
    route: String,
    open: Option<StoryModule>,
    reports: Vec<Value>,
}

/// The module a story's playthrough is in.
struct StoryModule {
    module: String,
    started: std::time::Instant,
    now: u64,
    assets: Assets,
    library: Arc<Library>,
    found: Found,
    watch: Watch,
}

impl<'a> StoryCheck<'a> {
    /// One when `KJK_SCENE_CHECK` asks.
    pub(crate) fn asked(installer: &'a Installer, game: Game, route: String) -> Option<StoryCheck<'a>> {
        std::env::var_os("KJK_SCENE_CHECK").map(|_| StoryCheck { installer, game, route, open: None, reports: Vec::new() })
    }

    /// A step `msec` long, `world` as it left it.
    pub(crate) fn step(&mut self, world: &sim::SimWorld, snapshot: &Snapshot, msec: u32) {
        let module = snapshot.place.as_ref().map_or_else(String::new, |place| place.module.clone());
        if self.open.as_ref().is_some_and(|open| open.module != module) {
            self.leave(world);
        }
        if self.open.is_none() {
            let library = world.level().scripts().cloned();
            let assets = Assets::open(self.installer, self.game, &module);
            if let (Some(library), Ok(assets)) = (library, assets) {
                self.open = Some(StoryModule {
                    module,
                    started: std::time::Instant::now(),
                    now: snapshot.time_ms,
                    assets,
                    library,
                    found: Found::default(),
                    watch: Watch { motion: Motion::everyone(), ..Watch::default() },
                });
            }
        }
        let Some(open) = self.open.as_mut() else {
            return;
        };
        open.now = snapshot.time_ms;
        let mut checker = Checker {
            assets: &mut open.assets,
            found: &mut open.found,
            library: open.library.clone(),
            watch: std::mem::take(&mut open.watch),
            talked: false,
        };
        let step = crate::dialogue_verbs::Step { now: snapshot.time_ms, msec, snapshot, world, level: world.level() };
        checker.step(&step, false);
        open.watch = checker.watch;
    }

    /// The module ends, `world` still its own.
    pub(crate) fn leave(&mut self, world: &sim::SimWorld) {
        let Some(mut open) = self.open.take() else {
            return;
        };
        let mut checker = Checker {
            assets: &mut open.assets,
            found: &mut open.found,
            library: open.library.clone(),
            watch: open.watch,
            talked: false,
        };
        checker.finish(world, open.now);
        drop(checker);
        let speakers = crate::speaker_census::census(&open.library, world.level().placed());
        let runs = [json!({ "run": self.route, "lines": 0 })];
        let library = open.library.clone();
        self.reports.push(module_report(&open.module, open.started, &runs, (&library, &open.assets), &mut open.found, speakers));
    }

    /// The run ends: `scene_check.json`'s text.
    pub(crate) fn finish(mut self, world: &sim::SimWorld) -> String {
        self.leave(world);
        serde_json::to_string(&self.reports).expect("json values serialize")
    }
}

/// `SUMMARY.md`, `SCENES.md` and `report.json` in a chain's run `folder`, of
/// the routes' `scene_check.json` at `routes`: a module its routes share is
/// one, what any of them found kept.
pub(crate) fn story_report(game: &str, folder: &Path, routes: &[std::path::PathBuf], seconds: u64) -> Result<(), String> {
    let game = Game::from_key(game).ok_or_else(|| format!("{game}: k1 or tsl"))?;
    let mut modules: BTreeMap<String, Value> = BTreeMap::new();
    for path in routes {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let reports: Vec<Value> = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        for report in reports {
            let module = report["module"].as_str().unwrap_or("").to_string();
            match modules.get_mut(&module) {
                None => {
                    modules.insert(module, report);
                }
                Some(was) => merge_module(was, &report),
            }
        }
    }
    let reports: Vec<Value> = modules.into_values().collect();
    let summary = summary(game, &reports, seconds);
    std::fs::write(folder.join("SUMMARY.md"), &summary).map_err(|e| e.to_string())?;
    std::fs::write(folder.join("SCENES.md"), scene_log::summary(&reports)).map_err(|e| e.to_string())?;
    let merged = json!({ "seconds": seconds, "story": true, "modules": reports });
    std::fs::write(folder.join("report.json"), serde_json::to_string(&merged).expect("json")).map_err(|e| e.to_string())?;
    println!("story check: {}", folder.join("SUMMARY.md").display());
    Ok(())
}

/// `other`'s findings added to `was`, the same module's in another route.
fn merge_module(was: &mut Value, other: &Value) {
    for field in ["failures", "notes", "reached", "runs"] {
        let (Some(into), Some(from)) = (was[field].as_array_mut(), other[field].as_array()) else {
            continue;
        };
        for item in from {
            if !into.contains(item) {
                into.push(item.clone());
            }
        }
    }
    for field in ["nodes_shown", "nodes"] {
        was[field] = json!(was[field].as_u64().unwrap_or(0).max(other[field].as_u64().unwrap_or(0)));
    }
    was["ms"] = json!(was["ms"].as_u64().unwrap_or(0) + other["ms"].as_u64().unwrap_or(0));
    if let (Some(into), Some(from)) = (was["scenes"].as_object_mut(), other["scenes"].as_object()) {
        for (dialog, scene) in from {
            into.entry(dialog.clone()).or_insert_with(|| scene.clone());
        }
    }
}

/// A talk run: the player uses the object tagged `tag` and answers its
/// offers by `picks` (from 1; the first reply past them).
fn talk_run(tag: &str, picks: Vec<u8>) -> Run {
    Run {
        seconds: TALK_MOST_S,
        talk: Some(tag.to_string()),
        read_ms: 500,
        picks,
        ..Run::default()
    }
}

/// The picks of the runs that take each other reply once where a run
/// answered with `picks` took the first: at each offer past its picks (`offers`,
/// how many replies each offered, in order), the picks so far, the first
/// reply up to it, then each other reply there.
fn branches(picks: &[u8], offers: &[usize]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for (at, &count) in offers.iter().enumerate().skip(picks.len()) {
        for other in 2..=count.min(usize::from(u8::MAX)) {
            let mut taken = picks.to_vec();
            taken.resize(at, 1);
            taken.push(other as u8);
            out.push(taken);
        }
    }
    out
}

/// The objects of `placed` that carry a conversation and a tag: (tag,
/// DLG), one object a conversation.
fn conversations(placed: &[Arc<ObjectData>]) -> Vec<(String, String)> {
    let mut seen = BTreeSet::new();
    placed
        .iter()
        .filter(|facts| !facts.conversation.is_empty() && !facts.tag.is_empty())
        .filter(|facts| seen.insert(facts.conversation.clone()))
        .map(|facts| (facts.tag.clone(), facts.conversation.clone()))
        .collect()
}

/// The fields `fmt_odyssey::dlg` does not read, set on the lines `shown`
/// (DLG, an entry, its index): (DLG, node, label).
fn unread_fields(resolver: &Resolver, shown: &BTreeSet<(String, bool, u32)>) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    let dialogs: BTreeSet<&String> = shown.iter().map(|(dialog, ..)| dialog).collect();
    for dialog in dialogs {
        let Some(gff) = resolver
            .find(dialog, ResType::DLG)
            .and_then(|found| found.location.read().ok())
            .and_then(|bytes| fmt_odyssey::gff::Gff::parse(&bytes).ok())
        else {
            continue;
        };
        for (entry, index, label) in fmt_odyssey::dlg::unread_fields(&gff.root) {
            if shown.contains(&(dialog.clone(), entry, index)) {
                let node = format!("{}{index}", if entry { "E" } else { "R" });
                out.push((dialog.clone(), node, label));
            }
        }
    }
    out
}

/// The participants' hooks with a script the library has that no run
/// raised: (tag, hook field, script).
fn unraised_hooks(found: &Found) -> Vec<Value> {
    let mut out = Vec::new();
    for (tag, facts) in &found.participants {
        for (hook, script) in &facts.scripts {
            if script.is_empty() || found.runs.contains_key(&sim::script::key(script)) {
                continue;
            }
            out.push(json!({ "tag": tag, "hook": hook, "script": script }));
        }
    }
    out
}

/// Checks `modules` of `game` (every module of the install when none),
/// each in a child `kjk` (`jobs` at once); with `report`, checks the one
/// module here and writes its JSON there (the child's part).
pub fn check(
    discovery: &Discovery,
    game: &str,
    modules: &[String],
    jobs: usize,
    report: Option<&Path>,
) -> ExitCode {
    let Some(game) = Game::from_key(game).filter(|game| *game != Game::JediAcademy) else {
        eprintln!("{game}: k1 or tsl");
        return ExitCode::FAILURE;
    };
    if let Some(path) = report {
        let [module] = modules else {
            eprintln!("--report checks one module");
            return ExitCode::FAILURE;
        };
        let facts = check_module(discovery, game, module).unwrap_or_else(|error| {
            json!({ "module": module, "error": error, "failures": [] })
        });
        let text = serde_json::to_string(&facts).expect("json values serialize");
        return match std::fs::write(path, text) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{}: {error}", path.display());
                ExitCode::FAILURE
            }
        };
    }
    match parent(discovery, game, modules, jobs) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

/// The modules checked in children, their reports merged.
fn parent(discovery: &Discovery, game: Game, modules: &[String], jobs: usize) -> Result<ExitCode, String> {
    let started = std::time::Instant::now();
    let modules = if modules.is_empty() {
        let anims = Arc::new(sim::motion::AnimTable::stand_in());
        Installer::new(discovery, game, anims)?.modules()
    } else {
        modules.to_vec()
    };
    let folder = crate::runtime::create_run_folder().map_err(|e| format!("the run folder: {e}"))?;
    eprintln!("run folder: {}", folder.display());
    let parts = folder.join("modules");
    std::fs::create_dir_all(&parts).map_err(|e| e.to_string())?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let queue = Mutex::new(modules.iter());
    let done = Mutex::new(0usize);
    std::thread::scope(|scope| {
        for _ in 0..jobs.max(1) {
            scope.spawn(|| {
                loop {
                    let Some(module) = queue.lock().ok().and_then(|mut q| q.next()) else {
                        return;
                    };
                    let out = parts.join(format!("{module}.json"));
                    let status = Command::new(&exe)
                        .args(["dialogue", "check", game.key(), module, "--report"])
                        .arg(&out)
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .status();
                    let mut done = done.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                    *done += 1;
                    eprintln!("{}/{} {module}: {}", *done, modules.len(), match status {
                        Ok(status) if status.success() => "checked".to_string(),
                        Ok(status) => format!("child {status}"),
                        Err(error) => error.to_string(),
                    });
                }
            });
        }
    });
    let mut reports = Vec::new();
    for module in &modules {
        let path = parts.join(format!("{module}.json"));
        let report = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .unwrap_or_else(|| json!({ "module": module, "error": "the child wrote no report", "failures": [] }));
        reports.push(report);
    }
    let seconds = started.elapsed().as_secs();
    let summary = summary(game, &reports, seconds);
    std::fs::write(folder.join("SUMMARY.md"), &summary).map_err(|e| e.to_string())?;
    std::fs::write(folder.join("SCENES.md"), scene_log::summary(&reports)).map_err(|e| e.to_string())?;
    let merged = json!({ "seconds": seconds, "modules": reports });
    std::fs::write(folder.join("report.json"), serde_json::to_string(&merged).expect("json"))
        .map_err(|e| e.to_string())?;
    println!("{summary}");
    println!("report: {}", folder.join("report.json").display());
    let _ = std::fs::remove_dir_all(&parts);
    let failures: usize = reports.iter().map(|r| r["failures"].as_array().map_or(0, Vec::len)).sum();
    Ok(if failures == 0 { ExitCode::SUCCESS } else { ExitCode::FAILURE })
}

/// The run's summary: coverage, then failures by check and cause, each
/// cause with how many, in how many DLGs, and an example.
fn summary<'a>(game: Game, reports: &'a [Value], seconds: u64) -> String {
    let mut out = String::new();
    let count = |key: &str| reports.iter().map(|r| r[key].as_u64().unwrap_or(0)).sum::<u64>();
    let reached: usize = reports.iter().map(|r| r["reached"].as_array().map_or(0, Vec::len)).sum();
    let errors: Vec<String> = reports
        .iter()
        .filter_map(|r| Some(format!("{}: {}", r["module"].as_str()?, r["error"].as_str()?)))
        .collect();
    out.push_str(&format!(
        "# {} conversation check\n\n{} modules in {seconds} s, {} runs; {reached} of {} conversations reached, \
         {} of their {} lines (entries and replies) shown.\n",
        game.key().to_uppercase(),
        reports.len(),
        count("talk_runs"),
        count("dialogs"),
        count("nodes_shown"),
        count("nodes"),
    ));
    if !errors.is_empty() {
        out.push_str(&format!("\nModules not checked: {}\n", errors.join("; ")));
    }
    let listed = |key: &str| -> Vec<&Value> {
        reports
            .iter()
            .flat_map(|r| r[key].as_array().into_iter().flatten())
            .collect()
    };
    let (failures, notes) = (listed("failures"), listed("notes"));
    let of = |list: &[&'a Value], number: u8| -> Vec<&'a Value> {
        list.iter().copied().filter(|f| f["invariant"] == number).collect()
    };
    let dialogs = |list: &[&Value]| -> usize {
        list.iter()
            .map(|f| format!("{}/{}", f["module"], f["dialog"]))
            .collect::<BTreeSet<_>>()
            .len()
    };
    out.push_str("\n| # | check | failures | DLGs | information |\n|---|---|---|---|---|\n");
    for (number, name) in INVARIANTS {
        let (failed, noted) = (of(&failures, number), of(&notes, number));
        out.push_str(&format!("| {number} | {name} | {} | {} | {} |\n", failed.len(), dialogs(&failed), noted.len()));
    }
    for (number, name) in INVARIANTS {
        for (list, title) in [(&failures, ""), (&notes, ", information")] {
            let listed = of(list, number);
            if !listed.is_empty() {
                out.push_str(&format!("\n## {number}. {name}{title}\n\n"));
                causes(&mut out, &listed, dialogs);
            }
        }
    }
    let mut hooks: BTreeMap<String, usize> = BTreeMap::new();
    for hook in reports.iter().flat_map(|r| r["unraised_hooks"].as_array().into_iter().flatten()) {
        *hooks.entry(hook["hook"].as_str().unwrap_or("").to_string()).or_insert(0) += 1;
    }
    if !hooks.is_empty() {
        out.push_str("\n## Participants' hooks no run raised (information)\n\n| hook | objects |\n|---|---|\n");
        for (hook, n) in hooks {
            out.push_str(&format!("| {hook} | {n} |\n"));
        }
    }
    out.push_str(&crate::speaker_census::summary(reports));
    out
}

/// One check's `listed` failures or notes by cause, each with how many, in
/// how many DLGs, and an example.
fn causes(out: &mut String, listed: &[&Value], dialogs: impl Fn(&[&Value]) -> usize) {
    let mut causes: BTreeMap<String, Vec<&Value>> = BTreeMap::new();
    for failure in listed {
        causes.entry(failure["cause"].as_str().unwrap_or("").to_string()).or_default().push(failure);
    }
    let mut causes: Vec<_> = causes.into_iter().collect();
    causes.sort_by_key(|(_, of)| std::cmp::Reverse(of.len()));
    out.push_str("| cause | count | DLGs | example |\n|---|---|---|---|\n");
    for (cause, of) in causes {
        let first = of[0];
        let text = |key: &str| first[key].as_str().unwrap_or("").replace('|', "/");
        out.push_str(&format!(
            "| {} | {} | {} | {}/{} {}: {} |\n",
            cause.replace('|', "/"),
            of.len(),
            dialogs(&of),
            text("module"),
            text("dialog"),
            text("node"),
            text("detail"),
        ));
    }
}

/// The checks' own probes (`kjk sim probes --only "dialogue check"`), on
/// data made here:
/// - 1: a body with a wield's ready stance plays `LOOPING_READY`; one
///   without a `bow` clip does not play `ANIMATION_FIREFORGET_BOW`;
/// - 3: a camera at the origin looking down +X at 55° holds a head 3 m
///   ahead, not one behind it nor one 3 m to its side;
/// - 4: a gunner's shot with neither its target's reaction nor a shot is
///   both faults; with them, none; a swing whose clip has no event lacks a
///   sound;
/// - 7: a jump 0.8 m off its mark with a body on it is displaced by it; one
///   0.3 m off is on its mark; one with no place, its jumper 3 m off, is
///   reported;
/// - 9: in the arena (a floor at z 0) a body standing on it has its floor
///   and room; one 3 m above it has no floor;
/// - 20: a death 2 s before a burst beside it is a fault, a death after it,
///   or far from it, none; before a script's blast, a note;
/// - 10: a DLG entry's field no code reads (`Unknown` 3) is a field not
///   read; its `CamVidEffect` 3, `Comment` and `SoundExists` 1, and an
///   unknown field of −1, are not.
pub fn probes() -> sim::probe::Checks {
    use fmt_odyssey::gff::{GffField, GffStruct, GffValue};
    use sim::probe::check;
    let mut checks = Vec::new();
    let ready: BTreeMap<u16, usize> = [(kjk_game::acting::READY + 2, 0), (5, 1)].into();
    checks.push((
        "check 1: ready stance by wield, no bow without its clip",
        check(
            resolves(&ready, kjk_game::acting::LOOPING_READY) && resolves(&ready, 5) && !resolves(&ready, 105),
            format!("{ready:?}"),
        ),
    ));
    let frustum = Frustum::of([0.0; 3], [0.0; 3], 55.0);
    let (ahead, behind, aside) = (Vec3::new(3.0, 0.0, 0.0), Vec3::new(-3.0, 0.0, 0.0), Vec3::new(0.5, 3.0, 0.0));
    checks.push((
        "check 3: a camera holds what is before it",
        check(
            frustum.holds(ahead) && !frustum.holds(behind) && !frustum.holds(aside),
            format!("ahead {}, behind {}, aside {}", frustum.holds(ahead), frustum.holds(behind), frustum.holds(aside)),
        ),
    ));
    // K1's rows: the rifle's shot clip and a blade holder's answer to it.
    const B7A1: u16 = 239;
    const G2D1: u16 = 124;
    let shot = Blow {
        attacker: SimId(1),
        target: Some(SimId(2)),
        row: Some(B7A1),
        since: 1000,
        gunner: true,
        sounded: Some(true),
        at: Where::out_of_line(),
    };
    let bare = blow_faults(&shot, &[], &BTreeSet::new(), &BTreeMap::new());
    let answered = blow_faults(
        &shot,
        &[(2, 1500, blows::HURT + G2D1)],
        &[(1, 1400)].into(),
        &BTreeMap::new(),
    );
    let silent = blow_faults(
        &Blow { gunner: false, sounded: Some(false), ..shot },
        &[],
        &BTreeSet::new(),
        &[(2, 2000)].into(),
    );
    checks.push((
        "check 4: a shot's reaction and bolt, a swing's sound",
        check(
            bare.len() == 2 && answered.is_empty() && silent == ["the swing's clip has no sound event"],
            format!("bare {bare:?}, answered {answered:?}, silent {silent:?}"),
        ),
    ));
    let jump = |landed: Option<[f32; 3]>, by: Option<u32>| sim::script::Jumped {
        ms: 0,
        who: script_ncs::vm::ObjectId(1),
        mark: [0.0; 3],
        from: [3.0, 0.0, 0.0],
        on_mesh: true,
        landed,
        displaced_by: by,
    };
    let pushed = jump_fault(&jump(Some([0.8, 0.0, 0.0]), Some(7)));
    let close = jump_fault(&jump(Some([0.3, 0.0, 0.0]), None));
    let nowhere = jump_fault(&jump(None, None));
    checks.push((
        "check 7: a jump off its mark",
        check(
            pushed.is_some_and(|(cause, _)| cause == "a body stood on the mark")
                && close.is_none()
                && nowhere.is_some(),
            format!("pushed {pushed:?}, close {close:?}, nowhere {nowhere:?}"),
        ),
    ));
    let event = |ms, kind, at| Event::new(ms, kind, "x".into(), String::new(), Some(true), at);
    let scene = |events| {
        let mut scene = Scene::default();
        scene.events = events;
        scene
    };
    let order = |events| scene_log::faults(&scene(events), BURST_M, BURST_BEFORE_MS, ORDER_SLACK_MS);
    let early = order(vec![event(1000, "death", [0.0; 3]), event(3000, "burst", [1.0, 0.0, 0.0])]);
    let after = order(vec![event(3000, "burst", [0.0; 3]), event(3100, "death", [1.0, 0.0, 0.0])]);
    let far = order(vec![event(1000, "death", [0.0; 3]), event(3000, "burst", [20.0, 0.0, 0.0])]);
    let scripted = order(vec![event(1000, "death", [0.0; 3]), event(3000, "blast", [1.0, 0.0, 0.0])]);
    checks.push((
        "check 20: a death before the burst beside it",
        check(
            early.len() == 1
                && !early[0].1
                && after.is_empty()
                && far.is_empty()
                && scripted.len() == 1
                && scripted[0].1,
            format!("early {early:?}, after {after:?}, far {far:?}, scripted {scripted:?}"),
        ),
    ));
    let arena = sim::Level::arena();
    let origin = |z: f32| Some([0.0, 0.0, z / collision::UNIT - sim::motion::SOLE_Z]);
    let standing = footing_faults(&arena, None, Vec3::new(0.0, 0.0, 0.01), origin(0.01));
    let floating = footing_faults(&arena, None, Vec3::new(0.0, 0.0, 3.0), origin(3.0));
    checks.push((
        "check 9: a body's floor and room",
        check(
            standing.is_empty() && floating == ["no floor under a participant"],
            format!("standing {standing:?}, floating {floating:?}"),
        ),
    ));
    let field = |label: &str, value: GffValue| GffField {
        label: label.as_bytes().to_vec(),
        value,
    };
    let entry = GffStruct {
        id: 0,
        fields: vec![
            field("Unknown", GffValue::Int(3)),
            field("CamVidEffect", GffValue::Int(3)),
            field("Comment", GffValue::String(b"a note".to_vec())),
            field("SoundExists", GffValue::Byte(1)),
        ],
    };
    let none = GffStruct {
        id: 1,
        fields: vec![field("Unknown", GffValue::Int(-1))],
    };
    let root = GffStruct {
        id: u32::MAX,
        fields: vec![field("EntryList", GffValue::List(vec![entry, none]))],
    };
    let unread = fmt_odyssey::dlg::unread_fields(&root);
    checks.push((
        "check 10: DLG fields not read",
        check(
            unread == [(true, 0, "Unknown".to_string())],
            format!("{unread:?}"),
        ),
    ));
    checks
}
