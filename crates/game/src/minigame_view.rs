//! A minigame drawn (`sim::minigame`; `docs/MINIGAMES.md`): the
//! swoop and its track's pads and buoys, or the Ebon Hawk's gun and the Sith
//! fighters, from the presented snapshots' course, with the camera the
//! player's rig carries and the HUD its models' animations make.
//!
//! When the presented snapshot has a course, the minigame's models (each
//! follower's, each obstacle's, the bullets', the player's camera rig) are
//! read off the frame from the module's archives, then spawned under the
//! scene's root. Each frame after `play_view`, every thing stands where the
//! course has it between the last two ticks, turned as its track turns (and
//! the turret as the view turns it), its models posed by the animations the
//! scripts play on it (`SWMG_PlayAnimation`: the swoop's gear, timer and
//! meter, the turret's compass and hull bar), each from its rest pose, the
//! ones over the others in the order they began. The camera stands at the
//! rig's `camerahook` node, looking down its −Z with +Y up (Bevy's own
//! camera axes). The party, parked while the minigame plays, is hidden;
//! anything else the area places is drawn as play leaves it (the
//! executables keep only the party's creatures out of the scene).
//!
//! Beyond places and turns, what its models' clips key (`fx_model`,
//! `model_fx`):
//! each animated mesh gets a material of its own whose alpha and
//! self-illumination follow the clips (the start lights, the thrusters'
//! glow), and each emitter its particles, drawn after the camera is placed.
//! A model with no clip in place of the others plays its `ready…` clip, the
//! one it idles in (a buoy's sway) [verify].
//!
//! Its sounds, read with its models: each new bullet its bank's fire
//! sound, each hit its shooter's bank's collision sound, each death the
//! follower's death sound (`Sounds`: a fighter's explosion, a pad taken).
//! A target's hit sound is heard at most once per the hitting gun's rate
//! (`Rate_Of_Fire`): the turret's fighters land some 30 hits a second as
//! they arrive, which drowned the Hawk's "Incoming fighters!" (the owner's
//! call, 2026-10-03).
//! The player's own are heard in both ears; the others' from where they
//! are, full within `NEAR_M`, nothing past `FAR_M` [verify by ear].

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use asset_odyssey::anim::sample_orientation;
use asset_odyssey::hooks;
use asset_odyssey::texture::TextureIr;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use sim::minigame::{Course, Kind, Rules};

use crate::cast::BodyRoots;
use crate::fx_model::{self, Drawing, Model, Spawned, Spawning, animate, node_matrix};
use crate::heat_haze::HeatHazeMaterial;
use crate::lifecycle::Installer;
use crate::lifecycle_view::SceneCamera;
use crate::scene::{ModelAssets, SceneContent, SceneRoot, Spawner, game_to_bevy};
use crate::sim_host::{OwnSeat, PresentedSnapshot, SceneSwap};
use crate::sound::{Added, Cue, Cues, Forget, Hold, Loops};
use crate::viewer;

/// The rig node the camera stands at.
/// Bullets of one model drawn at once, at most.
const BULLETS_DRAWN: usize = 48;
/// The others' sounds: full within this, nothing past that (metres).
const NEAR_M: f32 = 40.0;
const FAR_M: f32 = 600.0;

pub struct MinigameViewPlugin;

impl Plugin for MinigameViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            (load, draw, effects, sounds)
                .chain()
                .run_if(resource_exists::<MinigameView>)
                .run_if(resource_exists::<PresentedSnapshot>)
                .after(crate::play_view::Drawn)
                .after(crate::own_fx::Posed)
                .before(crate::own_fx::Drawn)
                .before(bevy::transform::TransformSystems::Propagate),
        );
    }
}

/// The view's state, put in with a session (`install`).
#[derive(Resource)]
pub struct MinigameView {
    installer: Option<Arc<Installer>>,
    /// The scene swap the models belong to.
    generation: u32,
    state: State,
    /// The sounds' clips brought into the bank, by name.
    sounds: Vec<String>,
    /// What the sounds have been cued up to: the course's time and its
    /// last hit's number; none before the first frame drawn.
    heard: Option<(u64, i32)>,
    /// When each target's hit sound was last cued (course ms), by target
    /// and sound.
    hits: BTreeMap<(u16, String), u64>,
    /// The course times the models were last posed and their effects
    /// drawn at (ms).
    drawn_ms: Option<f64>,
    effects_ms: Option<f64>,
    /// What the engine's own clips have been played up to: the course's
    /// time and its last hit's number; the bolts in flight then.
    engine_seen: Option<(u64, i32)>,
    flying: Vec<sim::minigame::Bullet>,
}

/// A session's minigame view, its models read from `installer`.
pub fn install(world: &mut World, installer: Option<Arc<Installer>>) {
    world.insert_resource(MinigameView {
        installer,
        generation: 0,
        state: State::Idle,
        sounds: Vec::new(),
        heard: None,
        hits: BTreeMap::new(),
        drawn_ms: None,
        effects_ms: None,
        engine_seen: None,
        flying: Vec::new(),
    });
}

/// A minigame sound's name in the bank.
fn sound_name(resref: &str) -> String {
    format!("minigame/{resref}")
}

enum State {
    Idle,
    Loading(Task<Result<Ready, String>>),
    Drawn(Box<Drawn>),
    /// Its models did not load: tried again only in another scene.
    Failed,
}

/// Everything read off the frame.
struct Ready {
    /// By thing number (the player, the enemies, the obstacles).
    things: Vec<Vec<Model>>,
    /// The player's camera rig, when it is a model of its own.
    camera: Option<Model>,
    bullets: Vec<(String, Model)>,
    /// Each gun bank's gun model: (thing, bank, its id, model).
    guns: Vec<(usize, usize, u32, Model)>,
    textures: BTreeMap<String, TextureIr>,
    /// The sounds' clips, by resref.
    sounds: Vec<(String, audio::Clip)>,
}

/// A bolt as the course knows it: its shooter, bank, gunner and birth.
type Bolt = (u16, u8, u8, u64);

/// How much of a ghost shows (M10): another seat's swoop in a co-op race.
const GHOST_ALPHA: f32 = 0.35;

/// Another run's swoop as a ghost: copies of the player's models, whether
/// their meshes have been faded yet, and its run's time as last drawn (ms).
struct Ghost {
    models: Vec<Spawned>,
    faded: bool,
    ms: f64,
}

/// A bullet model's copy and the bolt it drew last.
struct Pooled {
    spawned: Spawned,
    bolt: Option<Bolt>,
}

struct Drawn {
    things: Vec<Vec<Spawned>>,
    camera: Option<Spawned>,
    /// By bullet model: its spawned copies.
    bullets: BTreeMap<String, Vec<Pooled>>,
    guns: Vec<Gun>,
    bursts: Vec<Burst>,
    /// A co-op race's other runs' swoops, one set for each (`load` makes
    /// more as runs join), made of the player's models.
    ghosts: Vec<Ghost>,
    ghost_models: Vec<Model>,
    /// What the ghosts are drawn with: their models' textures alone, so a
    /// run joining makes no more images than those.
    ghost_textures: BTreeMap<String, TextureIr>,
}

/// A gun bank's gun model (the turret's `mgg_turret`), hung on its
/// follower's `gunbank<id>` node: which thing and bank, which of the
/// thing's models has the node and which node.
struct Gun {
    thing: usize,
    bank: usize,
    on: (usize, usize),
    spawned: Spawned,
}

/// A bolt spent where it hit, playing its `explode` there: its model,
/// where, which way it flew, when (course ms).
struct Burst {
    model: String,
    bolt: Bolt,
    at: Vec3,
    way: Vec3,
    since: u64,
}

/// How long the engine's own clips are kept on a model (ms; theirs are
/// shorter).
const ENGINE_KEPT_MS: u64 = 5000;
/// A spent bolt's `explode` without the clip (ms).
const BURST_MS: u64 = 3000;

fn read(installer: &Installer, module: &str, rules: &Rules) -> Result<Ready, String> {
    let resolver = installer.resolver(module)?;
    let load = |name: &str, rotating: bool| fx_model::load(&resolver, name, rotating);
    let followers = std::iter::once(&rules.player).chain(&rules.enemies);
    let mut things: Vec<Vec<Model>> = followers
        .map(|follower| {
            follower
                .models
                .iter()
                .filter_map(|(name, rotating)| load(name, *rotating))
                .collect()
        })
        .collect();
    things.extend(
        rules
            .obstacles
            .iter()
            .map(|obstacle| load(&obstacle.model, true).into_iter().collect()),
    );
    let camera = (!rules.player.camera.is_empty())
        .then(|| load(&rules.player.camera, rules.player.camera_rotate))
        .flatten();
    let mut names: Vec<&str> = std::iter::once(&rules.player)
        .chain(&rules.enemies)
        .flat_map(|follower| &follower.banks)
        .filter(|bank| bank.live())
        .map(|bank| bank.bullet_model.as_str())
        .collect();
    names.sort_unstable();
    names.dedup();
    let bullets = names
        .into_iter()
        .filter_map(|name| Some((name.to_string(), load(name, true)?)))
        .collect::<Vec<_>>();
    // Each bank's gun model; one not found or with nothing to draw (K1's
    // `mgg_null`) is none.
    let guns = std::iter::once(&rules.player)
        .chain(&rules.enemies)
        .enumerate()
        .flat_map(|(n, follower)| {
            follower
                .banks
                .iter()
                .enumerate()
                .map(move |(b, bank)| (n, b, bank.id, bank.gun_model.as_str()))
        })
        .filter(|(_, _, _, name)| !name.trim().is_empty())
        .filter_map(|(n, b, id, name)| Some((n, b, id, load(name, true)?)))
        .filter(|(.., model)| !(model.placed.ir.meshes.is_empty() && model.fx.emitters.is_empty()))
        .collect::<Vec<_>>();
    let models = things
        .iter()
        .flatten()
        .chain(&camera)
        .chain(bullets.iter().map(|(_, model)| model))
        .chain(guns.iter().map(|(_, _, _, model)| model));
    let textures = fx_model::textures(&resolver, models);
    let followers = || std::iter::once(&rules.player).chain(&rules.enemies);
    let mut names: Vec<&str> = followers()
        .flat_map(|follower| &follower.banks)
        .flat_map(|bank| [bank.fire_sound.as_str(), bank.hit_sound.as_str()])
        .chain(followers().flat_map(|follower| {
            [
                follower.death_sound.as_str(),
                follower.engine_sound.as_str(),
            ]
        }))
        .filter(|name| !name.trim().is_empty())
        .collect();
    names.sort_unstable();
    names.dedup();
    let sounds = names
        .into_iter()
        .filter_map(|name| {
            let clip = crate::sound::kotor_file(&resolver, name)
                .ok_or(())
                .and_then(|bytes| audio::Clip::kotor(&bytes).map_err(|_| ()));
            Some((name.to_string(), clip.ok()?))
        })
        .collect();
    Ok(Ready {
        things,
        camera,
        bullets,
        guns,
        textures,
        sounds,
    })
}

/// Starts reading the minigame's models when a course comes into view,
/// and spawns them once read.
#[allow(clippy::too_many_arguments)]
fn load(
    mut view: ResMut<MinigameView>,
    presented: Res<PresentedSnapshot>,
    swap: Res<SceneSwap>,
    scene: Res<SceneContent>,
    roots: Query<Entity, With<SceneRoot>>,
    mut assets: ModelAssets,
    (mut added, mut forget, mut hazes): (
        ResMut<Added>,
        ResMut<Forget>,
        ResMut<Assets<HeatHazeMaterial>>,
    ),
    mut commands: Commands,
) {
    let view = &mut *view;
    if swap.generation != view.generation {
        // Their models went with the old scene; their sounds leave the bank.
        view.generation = swap.generation;
        view.state = State::Idle;
        view.heard = None;
        view.hits.clear();
        view.drawn_ms = None;
        view.effects_ms = None;
        view.engine_seen = None;
        view.flying.clear();
        forget.0.append(&mut view.sounds);
    }
    let current = presented.0.current();
    let (Some(_), Some(rules)) = (current.minigame.as_ref(), swap.level.minigame()) else {
        return;
    };
    match &mut view.state {
        State::Idle => {
            let Some(installer) = view.installer.clone() else {
                view.state = State::Failed;
                return;
            };
            let (module, rules) = (swap.level.resources().to_string(), rules.clone());
            view.state = State::Loading(
                AsyncComputeTaskPool::get().spawn(async move { read(&installer, &module, &rules) }),
            );
        }
        State::Loading(task) => {
            let Some(done) = block_on(poll_once(task)) else {
                return;
            };
            let Some(root) = roots.iter().next() else {
                view.state = State::Failed;
                return;
            };
            view.state = match done {
                Ok(mut ready) => {
                    for (resref, clip) in std::mem::take(&mut ready.sounds) {
                        view.sounds.push(sound_name(&resref));
                        added.0.push((sound_name(&resref), clip));
                    }
                    State::Drawn(Box::new(spawn(
                        ready,
                        &scene.0,
                        root,
                        (&mut assets, &mut hazes),
                        &mut commands,
                    )))
                }
                Err(error) => {
                    warn!("the minigame's models: {error}");
                    State::Failed
                }
            };
        }
        // A race's ghosts: one for each other run, more as runs join.
        State::Drawn(drawn) => {
            let Some(course) = current
                .minigame
                .as_ref()
                .filter(|_| rules.kind == Kind::Swoop)
            else {
                return;
            };
            let Some(root) = roots.iter().next() else {
                return;
            };
            let wanted = course.runs.len();
            if drawn.ghosts.len() >= wanted {
                return;
            }
            let (images, stores) = assets.split();
            let mut spawning = Spawning {
                spawner: Spawner::new(&drawn.ghost_textures, scene.0.area_light(), images),
                stores,
                hazes: &mut hazes,
                textures: &drawn.ghost_textures,
                area: scene.0.area_light(),
                lit: scene.0.lit,
                root,
                seed: drawn.ghosts.len() as u32 + 1,
                commands: &mut commands,
            };
            let more: Vec<Ghost> = (drawn.ghosts.len()..wanted)
                .map(|_| Ghost {
                    models: drawn
                        .ghost_models
                        .iter()
                        .map(|model| spawning.spawn(model.copy(), true))
                        .collect(),
                    faded: false,
                    ms: 0.0,
                })
                .collect();
            drawn.ghosts.extend(more);
        }
        State::Failed => {}
    }
}

fn spawn(
    ready: Ready,
    scene: &viewer::LoadedScene,
    root: Entity,
    (assets, hazes): (&mut ModelAssets, &mut Assets<HeatHazeMaterial>),
    commands: &mut Commands,
) -> Drawn {
    let Ready {
        things,
        camera,
        bullets,
        guns,
        textures,
        ..
    } = ready;
    let (images, stores) = assets.split();
    let mut spawning = Spawning {
        spawner: Spawner::new(&textures, scene.area_light(), images),
        stores,
        hazes,
        textures: &textures,
        area: scene.area_light(),
        lit: scene.lit,
        root,
        seed: 1,
        commands,
    };
    let mut one = |model: Model, hidden: bool| spawning.spawn(model, hidden);
    let things: Vec<Vec<Spawned>> = things
        .into_iter()
        .map(|models| models.into_iter().map(|model| one(model, false)).collect())
        .collect();
    let camera = camera.map(|model| one(model, false));
    let bullets = bullets
        .into_iter()
        .map(|(name, model)| {
            let copies = (0..BULLETS_DRAWN)
                .map(|_| Pooled {
                    spawned: one(model.copy(), true),
                    bolt: None,
                })
                .collect();
            (name, copies)
        })
        .collect();
    // Each gun on the first of its thing's models with its bank's node.
    let guns = guns
        .into_iter()
        .filter_map(|(thing, bank, id, model)| {
            let node = format!("gunbank{id}");
            let on = things
                .get(thing)?
                .iter()
                .enumerate()
                .find_map(|(k, spawned)| {
                    let found = spawned.model.placed.ir.node_index(&node)?;
                    Some((k, found))
                })?;
            Some(Gun {
                thing,
                bank,
                on,
                spawned: one(model, false),
            })
        })
        .collect();
    // The player's models again, for the ghosts (`load`), with what they
    // are drawn with.
    let ghost_models: Vec<Model> = things
        .first()
        .into_iter()
        .flatten()
        .map(|spawned| spawned.model.copy())
        .collect();
    drop(spawning);
    let ghost_textures = textures
        .into_iter()
        .filter(|(name, _)| {
            ghost_models
                .iter()
                .any(|model| fx_model::texture_names(model).any(|used| used == name))
        })
        .collect();
    Drawn {
        things,
        camera,
        bullets,
        guns,
        bursts: Vec::new(),
        ghosts: Vec::new(),
        ghost_models,
        ghost_textures,
    }
}

/// The turn of a follower `n` at the course's moment: its track's, and the
/// turret's offset (pitch about x, yaw about z, degrees) for one that
/// turns with it: `aim`, a gunner's own (M11), else the player's.
fn turn(rules: &Rules, course: &Course, n: usize, rotating: bool, aim: Option<Vec3>) -> Quat {
    let (Some(follower), Some(mover)) = (rules.follower(n), course.movers.get(n)) else {
        return Quat::IDENTITY;
    };
    let [x, y, z, w] = follower.path.turn(mover.track);
    let track = Quat::from_xyzw(x, y, z, w).normalize();
    if rules.kind == Kind::Turret && n == 0 && rotating {
        let [pitch, _, yaw] = aim.map_or(mover.offset, |aim| aim.to_array());
        track * Quat::from_rotation_z(yaw.to_radians()) * Quat::from_rotation_x(pitch.to_radians())
    } else {
        track
    }
}

/// Where thing `n` stands between the last two ticks' courses.
fn place(rules: &Rules, (before, now): (&Course, &Course), alpha: f32, n: usize) -> Option<Vec3> {
    let at = Vec3::from_array(now.place(rules, n)?);
    let was = before.place(rules, n).map_or(at, Vec3::from_array);
    // A jump (a loop's wrap) is not blended across.
    Some(if was.distance(at) > 50.0 {
        at
    } else {
        was.lerp(at, alpha)
    })
}

/// The swoop's bank (`bankl_01`, `bankl_02` … and `bankr_…`): a chain of
/// clips each turning the bike one step further about its forward axis, in
/// its clip's length; no script plays them, so the engine must, by how the
/// bike drifts. Here the bank goes toward `share` (of the most the bike
/// drifts) of the steps the model has clips for (ten on K1's swoops), one
/// step a clip's length, and the bike is turned as the clip for the step it
/// is in has it part way [verify by eye against the game]. `bankl` turns
/// its top toward +x, the way a positive drift goes.
///
/// Moves `spawned`'s bank toward `share` over `dt` seconds; its turns, by
/// node, relative to rest (none for a model without the clips).
fn bank(spawned: &mut Spawned, share: f32, dt: f32) -> Vec<(usize, Quat)> {
    let clips = &spawned.model.clips;
    let Some(step) = clips.get("bankl_01").map(|clip| clip.length.max(0.01)) else {
        return Vec::new();
    };
    let steps = (1..)
        .take_while(|n| clips.contains_key(&format!("bankl_{n:02}")))
        .count() as f32;
    let target = (share * steps).clamp(-steps, steps);
    let reach = dt / step;
    spawned.bank += (target - spawned.bank).clamp(-reach, reach);
    let level = spawned.bank.abs();
    if level < 0.01 {
        return Vec::new();
    }
    let side = if spawned.bank > 0.0 { "bankl" } else { "bankr" };
    let n = level.ceil().clamp(1.0, steps);
    let Some(clip) = clips.get(&format!("{side}_{:02}", n as u32)) else {
        return Vec::new();
    };
    let at = (level - (n - 1.0)).clamp(0.0, 1.0) * clip.length;
    let ir = &spawned.model.placed.ir;
    clip.bind(ir)
        .into_iter()
        .enumerate()
        .filter_map(|(k, track)| {
            let [x, y, z, w] = sample_orientation(&clip.tracks[track?].orientations, at)?;
            let [rx, ry, rz, rw] = ir.nodes[k].rotation;
            let rest = Quat::from_xyzw(rx, ry, rz, rw);
            Some((k, rest.inverse() * Quat::from_xyzw(x, y, z, w)))
        })
        .collect()
}

/// The animations playing on thing `n` (a dead follower its `die`, once).
fn anims_of(rules: &Rules, now: &Course, n: usize) -> Vec<sim::minigame::Anim> {
    let followers = rules.followers();
    match now.movers.get(n) {
        Some(mover) if !mover.alive => vec![sim::minigame::Anim {
            name: "die".into(),
            looping: false,
            overlay: false,
            since: mover.died,
        }],
        Some(mover) => mover.anims.clone(),
        None => now
            .stills
            .get(n.saturating_sub(followers))
            .map_or_else(Vec::new, |still| still.anims.clone()),
    }
}

/// What the viewer at `seat` sees of the course between the last two
/// ticks: the whole course, its own run then and now (a co-op race's,
/// M10; the course itself in any other), the blend between them and the
/// course time it stands for.
struct Seen<'s> {
    whole: &'s Course,
    whole_before: Option<&'s Course>,
    before: &'s Course,
    now: &'s Course,
    alpha: f32,
    now_ms: f64,
}

fn seen<'s>(
    (previous, current): (&'s sim::Snapshot, &'s sim::Snapshot),
    alpha: f32,
    rules: &Rules,
    seat: u8,
) -> Option<Seen<'s>> {
    let whole = current.minigame.as_ref().filter(|now| now.fits(rules))?;
    let whole_before = previous
        .minigame
        .as_ref()
        .filter(|before| before.fits(rules));
    let now = whole.run_of(seat);
    let before = whole_before
        .map(|before| before.run_of(seat))
        .filter(|before| before.seat == now.seat)
        .unwrap_or(now);
    let now_ms =
        before.time_ms as f64 + (now.time_ms as f64 - before.time_ms as f64) * f64::from(alpha);
    Some(Seen {
        whole,
        whole_before,
        before,
        now,
        alpha,
        now_ms,
    })
}

/// The seat this view is for: its own, else the first.
fn own_seat(own: Option<Res<OwnSeat>>) -> u8 {
    own.map_or(0, |own| own.0.0)
}

#[allow(clippy::too_many_arguments)]
fn draw(
    mut view: ResMut<MinigameView>,
    presented: Res<PresentedSnapshot>,
    swap: Res<SceneSwap>,
    roots: Option<Res<BodyRoots>>,
    mut transforms: Query<&mut Transform>,
    mut visibility: Query<&mut Visibility>,
    mut cameras: Query<(Entity, &mut Projection), SceneCamera>,
    (mut facts, dump_due, own): (
        ResMut<console::DumpFacts>,
        Res<console::DumpDue>,
        Option<Res<OwnSeat>>,
    ),
) {
    let view = &mut *view;
    let State::Drawn(drawn) = &mut view.state else {
        return;
    };
    let (previous, current) = (presented.0.previous(), presented.0.current());
    let seat = own_seat(own);
    let Some(rules) = swap.level.minigame() else {
        return;
    };
    let Some(Seen {
        whole,
        whole_before,
        before,
        now,
        alpha,
        now_ms,
    }) = seen((previous, current), presented.0.alpha(), rules, seat)
    else {
        return;
    };
    // A gunner turns the hull's gun its own way and sees its own compass
    // (M11).
    let gunner = whole
        .gunners
        .iter()
        .position(|gunner| gunner.seat == seat)
        .map_or(0, |k| k + 1);
    let aim = whole
        .gunners
        .get(gunner.wrapping_sub(1))
        .map(|gunner| Vec3::from_array(gunner.offset));
    let player_anims = now.lens_anims(rules, gunner);
    let dt = view
        .drawn_ms
        .replace(now_ms)
        .filter(|was| *was <= now_ms)
        .map_or(0.0, |was| ((now_ms - was) / 1000.0) as f32);
    // The party, parked, hidden; what else the area places drawn as play
    // leaves it (both executables keep a creature in the party out of the
    // scene while a minigame plays, and nothing else: Peragus' hangar's
    // troopers and barrels under its turret).
    if let Some(roots) = roots {
        let party = current
            .entities
            .iter()
            .filter(|entity| {
                current.in_party(entity) || matches!(entity.kind, sim::Kind::Avatar(_))
            })
            .filter_map(|entity| roots.0.get(&entity.id));
        for &root in party {
            if let Ok(mut shown) = visibility.get_mut(root) {
                *shown = Visibility::Hidden;
            }
        }
    }
    let followers = rules.followers();
    let bursts = swap
        .level
        .module()
        .is_none_or(|module| sim::minigame::bolts_burst(&module.name));
    engine_clips(
        drawn,
        (rules, now, bursts),
        (&mut view.engine_seen, &mut view.flying),
    );
    let share = (rules.kind == Kind::Swoop).then(|| now.drift_share());
    for (n, spawned) in drawn.things.iter_mut().enumerate() {
        let alive = match n.checked_sub(followers) {
            // An obstacle a script took off the course is gone.
            Some(k) => now.stills.get(k).is_some_and(|still| !still.gone),
            None => now.movers.get(n).is_some_and(|mover| mover.alive),
        };
        // A follower gone shows its death clip once, if its models have one.
        let died = now.movers.get(n).map_or(0, |mover| mover.died);
        let dying = |model: &Model| {
            model
                .clips
                .get("die")
                .is_some_and(|clip| now_ms < died as f64 + f64::from(clip.length) * 1000.0)
        };
        let at = place(rules, (before, now), alpha, n);
        let anims = match n {
            0 if now.movers[0].alive => player_anims.clone(),
            _ => anims_of(rules, now, n),
        };
        for model in spawned {
            model.anims = anims.iter().chain(&model.engine).cloned().collect();
            let shown = at.is_some() && (alive || dying(&model.model));
            if let Ok(mut visible) = visibility.get_mut(model.root) {
                *visible = if shown {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
            }
            let Some(at) = at.filter(|_| shown) else {
                continue;
            };
            if let Ok(mut transform) = transforms.get_mut(model.root) {
                *transform = Transform::from_translation(at).with_rotation(turn(
                    rules,
                    now,
                    n,
                    model.model.rotating,
                    aim,
                ));
            }
            let turns = share
                .filter(|_| n == 0)
                .map_or_else(Vec::new, |share| bank(model, share, dt));
            let playing = model.anims.clone();
            animate(model, &playing, (now_ms, dt), turns, &mut transforms);
        }
    }
    // The other runs' swoops, each a ghost where its run has it (M10).
    let others: Vec<&Course> = std::iter::once(whole)
        .chain(&whole.runs)
        .filter(|run| run.seat != now.seat)
        .collect();
    for (k, ghost) in drawn.ghosts.iter_mut().enumerate() {
        let run = others.get(k).copied();
        // Where it is, and its own clock (a run joined late started late).
        let at = run
            .filter(|run| run.movers.first().is_some_and(|mover| mover.alive))
            .and_then(|run| {
                let was = whole_before
                    .map(|before| before.run_of(run.seat))
                    .filter(|before| before.seat == run.seat)
                    .unwrap_or(run);
                let ms = was.time_ms as f64
                    + (run.time_ms as f64 - was.time_ms as f64) * f64::from(alpha);
                Some((place(rules, (was, run), alpha, 0)?, ms))
            });
        for model in &mut ghost.models {
            if let Ok(mut visible) = visibility.get_mut(model.root) {
                *visible = if at.is_some() {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
            }
            let (Some((at, run_ms)), Some(run)) = (at, run) else {
                continue;
            };
            ghost.ms = run_ms;
            if let Ok(mut transform) = transforms.get_mut(model.root) {
                *transform = Transform::from_translation(at).with_rotation(turn(
                    rules,
                    run,
                    0,
                    model.model.rotating,
                    None,
                ));
            }
            model.anims = run.movers[0].anims.clone();
            let turns = bank(model, run.drift_share(), dt);
            let playing = model.anims.clone();
            animate(model, &playing, (run_ms, dt), turns, &mut transforms);
        }
    }
    // The player's camera rig: its own model, else one of its models.
    let player_at = place(rules, (before, now), alpha, 0).unwrap_or(Vec3::ZERO);
    if let Some(rig) = &drawn.camera
        && let Ok(mut transform) = transforms.get_mut(rig.root)
    {
        *transform = Transform::from_translation(player_at).with_rotation(turn(
            rules,
            now,
            0,
            rig.model.rotating,
            aim,
        ));
    }
    if let Some(rig) = &mut drawn.camera {
        rig.anims = player_anims.clone();
        animate(
            rig,
            &player_anims,
            (now_ms, dt),
            Vec::new(),
            &mut transforms,
        );
    }
    // The guns, where their nodes are on their followers' models.
    for gun in &mut drawn.guns {
        let Some(owner) = drawn
            .things
            .get(gun.thing)
            .and_then(|models| models.get(gun.on.0))
        else {
            continue;
        };
        let shown = visibility
            .get(owner.root)
            .is_ok_and(|v| *v != Visibility::Hidden);
        let root = transforms
            .get(owner.root)
            .map_or(Mat4::IDENTITY, |t| t.to_matrix());
        let hung = root
            * node_matrix(owner, gun.on.1, |e| {
                transforms.get(e).ok().map(|t| t.to_matrix())
            });
        if let Ok(mut transform) = transforms.get_mut(gun.spawned.root) {
            *transform = Transform::from_matrix(hung);
        }
        if let Ok(mut visible) = visibility.get_mut(gun.spawned.root) {
            *visible = if shown {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
        gun.spawned.anims = gun.spawned.engine.clone();
        let playing = gun.spawned.anims.clone();
        animate(
            &mut gun.spawned,
            &playing,
            (now_ms, dt),
            Vec::new(),
            &mut transforms,
        );
    }
    let rigs = drawn
        .camera
        .iter()
        .chain(drawn.things.first().into_iter().flatten());
    let hook = rigs.into_iter().find_map(|spawned| {
        let index = spawned.model.placed.ir.node_index(hooks::CAMERA_HOOK)?;
        let root = transforms.get(spawned.root).ok()?.to_matrix();
        Some(
            root * node_matrix(spawned, index, |e| {
                transforms.get(e).ok().map(|t| t.to_matrix())
            }),
        )
    });
    if let Some(hook) = hook {
        let world = Mat4::from_quat(game_to_bevy()) * hook;
        for (entity, mut projection) in cameras.iter_mut() {
            if let Ok(mut transform) = transforms.get_mut(entity) {
                *transform = Transform::from_matrix(world);
            }
            if let Projection::Perspective(perspective) = &mut *projection {
                if rules.view_angle > 0.0 {
                    perspective.fov = rules.view_angle.to_radians();
                }
                perspective.near = now.clip[0].max(0.05);
                perspective.far = now.clip[1].max(perspective.near + 1.0);
            }
        }
    }
    if dump_due.0 {
        let near: Vec<serde_json::Value> = (1..drawn.things.len())
            .filter_map(|n| {
                let at = now.place(rules, n)?;
                let models = drawn.things[n].len();
                let shown = drawn.things[n].iter().any(|model| {
                    visibility
                        .get(model.root)
                        .is_ok_and(|v| *v != Visibility::Hidden)
                });
                let first = drawn.things[n].first();
                let meshes = first.map_or(0, |m| m.model.placed.ir.meshes.len());
                let name = first.map_or(String::new(), |m| m.model.placed.ir.name.clone());
                let root = first
                    .and_then(|m| transforms.get(m.root).ok())
                    .map(|t| t.translation.to_array());
                (at[1] > player_at.y - 20.0 && at[1] < player_at.y + 300.0).then(|| {
                    serde_json::json!({ "thing": n, "at": at, "models": models, "shown": shown,
                        "meshes": meshes, "name": name, "root": root })
                })
            })
            .collect();
        facts.0.insert(
            "minigame".into(),
            serde_json::json!({
                "time_ms": now.time_ms,
                // Whose run this view draws, and the co-op it has (M10, M11).
                "seat": now.seat,
                "runs": whole.runs.len(),
                "gunners": whole.gunners.len(),
                "ghosts_shown": drawn.ghosts.iter().filter(|ghost| ghost.models.iter().any(|model| {
                    visibility.get(model.root).is_ok_and(|v| *v != Visibility::Hidden)
                })).count(),
                "player": player_at.to_array(),
                "speed": now.movers[0].speed,
                "blur": now.blur,
                "offset": now.movers[0].offset[0],
                "drift_share": now.drift_share(),
                "room_clip": current
                    .area_sound
                    .room
                    .map(|room| (room.room, room.anim, room.since_ms)),
                "bank": drawn.things.first().and_then(|models| models.first()).map(|m| m.bank),
                // Each follower: hit points, alive, the clips on it.
                "movers": now.movers.iter().map(|mover| serde_json::json!([
                    mover.hp,
                    mover.max_hp,
                    mover.alive,
                    mover.anims.iter().map(|anim| anim.name.as_str()).collect::<Vec<_>>(),
                ])).collect::<Vec<_>>(),
                // The player's emitters: node, particles alive, birthrate.
                "emitters": drawn.things.first().into_iter().flatten().flat_map(|model| {
                    model.sprays.iter().map(|spray| {
                        let node = &model.model.fx.emitters[spray.emitter].node;
                        serde_json::json!([node, spray.particles(), spray.birthrate()])
                    })
                }).collect::<Vec<_>>(),
                "camera_hook": hook.map(|m| m.w_axis.truncate().to_array()),
                "things": drawn.things.iter().map(Vec::len).collect::<Vec<_>>(),
                "ahead": near,
                "bullets": now.bullets.len(),
            }),
        );
    }
    // Bullets in flight, drawn along their way; then those spent, each
    // playing its `explode` where it hit.
    let mut placed: Vec<(String, Bolt, Vec3, Vec3, Vec<sim::minigame::Anim>)> = now
        .bullets
        .iter()
        .filter_map(|bullet| {
            let bank = rules
                .follower(usize::from(bullet.shooter))
                .and_then(|follower| follower.banks.get(usize::from(bullet.bank)))?;
            let at = Vec3::from_array(bullet.at(now_ms as u64));
            let way = Vec3::from_array(bullet.velocity).normalize_or(Vec3::Y);
            let bolt = (bullet.shooter, bullet.bank, bullet.gunner, bullet.born);
            Some((bank.bullet_model.clone(), bolt, at, way, Vec::new()))
        })
        .collect();
    let lasts = |model: &str| {
        drawn
            .bullets
            .get(model)
            .and_then(|copies| copies.first())
            .and_then(|copy| copy.spawned.model.clips.get("explode"))
            .map_or(BURST_MS, |clip| (clip.length * 1000.0) as u64)
    };
    let kept: Vec<u64> = drawn
        .bursts
        .iter()
        .map(|burst| lasts(&burst.model))
        .collect();
    let mut keep = kept.into_iter();
    drawn
        .bursts
        .retain(|burst| now.time_ms < burst.since + keep.next().unwrap_or(BURST_MS));
    placed.extend(drawn.bursts.iter().map(|burst| {
        let explode = sim::minigame::Anim {
            name: "explode".into(),
            looping: false,
            overlay: false,
            since: burst.since,
        };
        (
            burst.model.clone(),
            burst.bolt,
            burst.at,
            burst.way,
            vec![explode],
        )
    }));
    // Each bolt in the copy that drew it last, else in one no bolt still
    // in play holds, made new (a copy's clips and particles stay its bolt's).
    let live: BTreeSet<Bolt> = placed.iter().map(|(_, bolt, ..)| *bolt).collect();
    let mut used: BTreeMap<String, Vec<bool>> = BTreeMap::new();
    for (model, bolt, at, way, anims) in placed {
        let Some(copies) = drawn.bullets.get_mut(&model) else {
            continue;
        };
        let taken = used
            .entry(model)
            .or_insert_with(|| vec![false; copies.len()]);
        let free = |k: usize| !taken[k] && copies[k].bolt.is_none_or(|held| !live.contains(&held));
        let Some(k) = (0..copies.len())
            .find(|&k| !taken[k] && copies[k].bolt == Some(bolt))
            .or_else(|| (0..copies.len()).find(|&k| free(k)))
        else {
            continue;
        };
        taken[k] = true;
        let pooled = &mut copies[k];
        if pooled.bolt != Some(bolt) {
            pooled.spawned.reset();
            pooled.bolt = Some(bolt);
        }
        let copy = &mut pooled.spawned;
        if let Ok(mut transform) = transforms.get_mut(copy.root) {
            *transform = Transform::from_translation(at).with_rotation(heading(way));
        }
        if let Ok(mut visible) = visibility.get_mut(copy.root) {
            *visible = Visibility::Inherited;
        }
        copy.anims = anims;
        let playing = copy.anims.clone();
        animate(copy, &playing, (now_ms, dt), Vec::new(), &mut transforms);
    }
    for (name, copies) in &mut drawn.bullets {
        let taken = used.get(name.as_str());
        for (k, Pooled { spawned: copy, .. }) in copies.iter_mut().enumerate() {
            if taken.is_some_and(|taken| taken[k]) {
                continue;
            }
            if let Ok(mut visible) = visibility.get_mut(copy.root) {
                *visible = Visibility::Hidden;
            }
            copy.anims.clear();
        }
    }
}

/// A bolt's turn: its +y along `way`, its +z up as near as may be (the
/// Sith twin bolt's pair side by side, not rolled).
pub(crate) fn heading(way: Vec3) -> Quat {
    let right = way.cross(Vec3::Z);
    let right = if right.length_squared() > 1e-6 {
        right.normalize()
    } else {
        Vec3::X
    };
    let up = right.cross(way).normalize_or(Vec3::Z);
    Quat::from_mat3(&Mat3::from_cols(right, way, up))
}

/// The clips the engine plays itself, as the course shows what happened
/// since the last frame (`seen`: the course time and last hit number
/// then; `flying`: the bolts in flight then): a gun bank's `fire` on its
/// follower's models and its gun as a bolt leaves it, `damage` on a
/// follower hit, and each bolt spent before its time a burst where it hit
/// (its `explode`) where they burst (`bursts`: `sim::minigame::bolts_burst`).
/// No script plays these [verify against the game].
fn engine_clips(
    drawn: &mut Drawn,
    (rules, now, bursts): (&Rules, &Course, bool),
    (seen, flying): (&mut Option<(u64, i32)>, &mut Vec<sim::minigame::Bullet>),
) {
    let last_fact = now.facts.last().map_or(0, |(number, _)| *number);
    let Some((seen_ms, seen_fact)) = seen.replace((now.time_ms, last_fact)) else {
        *flying = now.bullets.clone();
        return;
    };
    let clip = |name: &str, since: u64| sim::minigame::Anim {
        name: name.into(),
        looping: false,
        overlay: true,
        since,
    };
    let push = |spawned: &mut Spawned, anim: sim::minigame::Anim| {
        spawned
            .engine
            .retain(|kept| kept.since + ENGINE_KEPT_MS > now.time_ms && kept.name != anim.name);
        spawned.engine.push(anim);
    };
    for bullet in now.bullets.iter().filter(|bullet| bullet.born > seen_ms) {
        let shooter = usize::from(bullet.shooter);
        for model in drawn.things.get_mut(shooter).into_iter().flatten() {
            push(model, clip("fire", bullet.born));
        }
        for gun in drawn
            .guns
            .iter_mut()
            .filter(|gun| gun.thing == shooter && gun.bank == usize::from(bullet.bank))
        {
            push(&mut gun.spawned, clip("fire", bullet.born));
        }
    }
    for (_, fact) in now.facts.iter().filter(|(number, _)| *number > seen_fact) {
        if let (Some(target), Some(_)) = (fact.target, fact.shooter) {
            for model in drawn
                .things
                .get_mut(usize::from(target))
                .into_iter()
                .flatten()
            {
                push(model, clip("damage", now.time_ms));
            }
        }
    }
    let same = |a: &sim::minigame::Bullet, b: &sim::minigame::Bullet| {
        a.shooter == b.shooter && a.bank == b.bank && a.gunner == b.gunner && a.born == b.born
    };
    for spent in flying.iter().filter(|bullet| {
        bursts && now.time_ms < bullet.until && !now.bullets.iter().any(|b| same(b, bullet))
    }) {
        let Some(bank) = rules
            .follower(usize::from(spent.shooter))
            .and_then(|follower| follower.banks.get(usize::from(spent.bank)))
        else {
            continue;
        };
        drawn.bursts.push(Burst {
            model: bank.bullet_model.clone(),
            bolt: (spent.shooter, spent.bank, spent.gunner, spent.born),
            at: Vec3::from_array(spent.at(now.time_ms)),
            way: Vec3::from_array(spent.velocity).normalize_or(Vec3::Y),
            since: now.time_ms,
        });
    }
    *flying = now.bullets.clone();
}

/// The sounds of what happened in the course since the last frame (see the
/// module documentation).
fn sounds(
    mut view: ResMut<MinigameView>,
    presented: Res<PresentedSnapshot>,
    swap: Res<SceneSwap>,
    (mut cues, mut loops): (ResMut<Cues>, ResMut<Loops>),
    own: Option<Res<OwnSeat>>,
) {
    let view = &mut *view;
    if !matches!(view.state, State::Drawn(_)) {
        return;
    }
    let current = presented.0.current();
    let (Some(whole), Some(rules)) = (current.minigame.as_ref(), swap.level.minigame()) else {
        return;
    };
    if !whole.fits(rules) {
        return;
    }
    // The viewer's own run's (M10): the others' ghosts are silent.
    let now = whole.run_of(own_seat(own));
    let last_fact = now.facts.last().map_or(0, |(number, _)| *number);
    // From the first frame on: nothing before it is heard again.
    let Some((heard_ms, heard_fact)) = view.heard.replace((now.time_ms, last_fact)) else {
        return;
    };
    // The course's moments on the world's clock.
    let start = current.time_ms as f32 - now.time_ms as f32;
    let loaded = |resref: &str| view.sounds.contains(&sound_name(resref));
    let mut cue = |resref: &str, n: usize, at: Option<[f32; 3]>, ms: f32| {
        if resref.trim().is_empty() || !loaded(resref) {
            return;
        }
        let at = at.or_else(|| now.place(rules, n)).unwrap_or([0.0; 3]);
        cues.0.push(Cue {
            falloff: falloff(n),
            ..Cue::at(
                sound_name(resref),
                Vec3::from_array(at) / collision::UNIT,
                start + ms,
            )
        });
    };
    let bank = |shooter: u16, bank: usize| rules.follower(usize::from(shooter))?.banks.get(bank);
    for bullet in now.bullets.iter().filter(|bullet| bullet.born > heard_ms) {
        if let Some(bank) = bank(bullet.shooter, usize::from(bullet.bank)) {
            let at = Some(bullet.from);
            cue(
                &bank.fire_sound,
                usize::from(bullet.shooter),
                at,
                bullet.born as f32,
            );
        }
    }
    for (_, fact) in now.facts.iter().filter(|(number, _)| *number > heard_fact) {
        let (Some(target), Some(shooter)) = (fact.target, fact.shooter) else {
            continue;
        };
        let hit = rules.follower(usize::from(shooter)).and_then(|follower| {
            follower
                .banks
                .iter()
                .find(|bank| !bank.hit_sound.is_empty())
        });
        if let Some(bank) = hit {
            let key = (target, bank.hit_sound.clone());
            let last = view.hits.get(&key).copied();
            if last.is_some_and(|last| now.time_ms < last + u64::from(bank.rate_ms)) {
                continue;
            }
            view.hits.insert(key, now.time_ms);
            cue(
                &bank.hit_sound,
                usize::from(target),
                None,
                now.time_ms as f32,
            );
        }
    }
    for (n, mover) in now.movers.iter().enumerate() {
        if !mover.alive
            && mover.died > heard_ms
            && let Some(follower) = rules.follower(n)
        {
            cue(&follower.death_sound, n, None, mover.died as f32);
        }
    }
    // Each living follower's engine (`Sounds` `Engine`: TSL's fighters'),
    // a loop where it is from its start (both executables play it looping
    // as it sets off), the player's in both ears.
    for n in (0..now.movers.len()).filter(|&n| now.movers[n].alive) {
        let (Some(follower), Some(at)) = (rules.follower(n), now.place(rules, n)) else {
            continue;
        };
        if follower.engine_sound.trim().is_empty() || !loaded(&follower.engine_sound) {
            continue;
        }
        loops.0.push(Hold {
            falloff: falloff(n),
            ..Hold::at(
                sound_name(&follower.engine_sound),
                Vec3::from_array(at) / collision::UNIT,
            )
        });
    }
}

/// How follower `n`'s sounds fall off: the player's heard in both ears,
/// the others' from where they are.
fn falloff(n: usize) -> audio::Falloff {
    if n == 0 {
        audio::Falloff::Everywhere
    } else {
        audio::Falloff::Range {
            near: NEAR_M / collision::UNIT,
            far: FAR_M / collision::UNIT,
            placed: true,
        }
    }
}

/// What the clips key beyond places and turns, after the camera is placed
/// (see the module documentation).
fn effects(
    mut view: ResMut<MinigameView>,
    presented: Res<PresentedSnapshot>,
    swap: Res<SceneSwap>,
    cameras: Query<&Transform, SceneCamera>,
    mut drawing: Drawing,
    own: Option<Res<OwnSeat>>,
) {
    let view = &mut *view;
    let State::Drawn(drawn) = &mut view.state else {
        return;
    };
    let (previous, current) = (presented.0.previous(), presented.0.current());
    let Some(rules) = swap.level.minigame() else {
        return;
    };
    let Some(Seen { now_ms, .. }) = seen(
        (previous, current),
        presented.0.alpha(),
        rules,
        own_seat(own),
    ) else {
        return;
    };
    let was_ms = view
        .effects_ms
        .replace(now_ms)
        .filter(|was| *was <= now_ms)
        .unwrap_or(now_ms);
    let dt = ((now_ms - was_ms) / 1000.0) as f32;
    let facing = fx_model::facing(cameras.iter().next().map_or(Quat::IDENTITY, |t| t.rotation));
    // Each thing's ground: under its first model's lowest point.
    let grounds: Vec<Option<f32>> = drawn
        .things
        .iter()
        .map(|models| {
            let first = models.first()?;
            Some(drawing.transforms.get(first.root).ok()?.translation.z + first.model.floor)
        })
        .collect();
    let ground = |n: usize| grounds.get(n).copied().flatten();
    let rig = drawn.camera.iter_mut().map(|rig| (ground(0), rig));
    let things = drawn
        .things
        .iter_mut()
        .enumerate()
        .flat_map(|(n, models)| models.iter_mut().map(move |model| (ground(n), model)));
    let guns = drawn
        .guns
        .iter_mut()
        .map(|gun| (ground(gun.thing), &mut gun.spawned));
    let bullets = drawn
        .bullets
        .values_mut()
        .flatten()
        .map(|copy| (None, &mut copy.spawned));
    for (ground, spawned) in things.chain(rig).chain(guns).chain(bullets) {
        let floor = ground.map(crate::model_fx::floor);
        let surfaces = floor
            .as_ref()
            .map(|floor| floor as crate::model_fx::Surfaces);
        drawing.effects(spawned, surfaces, (now_ms, dt), facing);
    }
    for ghost in &mut drawn.ghosts {
        for model in &ghost.models {
            ghost.faded |= drawing.fade(model, GHOST_ALPHA);
        }
        for model in &mut ghost.models {
            drawing.effects(model, None, (ghost.ms, dt), facing);
        }
    }
}
