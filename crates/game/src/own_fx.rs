//! A module's rooms and placeables as their own data moves them
//! (`viewer::OwnFx`), and what their clips and MDLs key beyond places and
//! turns (`model_fx`): their meshes' alpha and self-illumination, and their
//! emitters' particles (Tatooine's blowing sand, Manaan's birds, the Star
//! Forge's captive Jedi held in their stasis fields, fire pits, steam
//! vents). The particles are drawn under the model's root, in its space,
//! so the room's culling (or a destroyed placeable's hiding) hides them
//! with it; a hidden model's go on being born and dying, but are not drawn.
//!
//! - A room's `animloop…` clips loop together from the scene's start, as
//!   reone's area plays every clip so named (looping, one over the other;
//!   Manaan's swoop track flies its birds by two of them). The
//!   `scriptloopNN` clip a script starts on it (`PlayRoomAnimation`, the
//!   snapshot's `AreaSound::room`) loops from when it began, over them, in
//!   every module: Peragus's sparks and ship smoke, the Ravager's flames,
//!   the turret's skybox.
//! - A placeable plays the clip its look picks (`sim::look`, its
//!   `ObjectState::state`), as KOTOR's executable plays them (its
//!   `animations.2da` placeable rows): its look's own loop (`default`,
//!   `open`, `close`, `dead`, `on`, `off`, `animloopNN`); a look changed in
//!   play (a script's `PlayAnimation`, the captives drained, a container
//!   opened and closed, a placeable broken by blows) its way there once
//!   first (`way`: `off2on`, `on2off`, `close2open`, `open2close`, `die`),
//!   from the snapshot it changed in; a blow while it is closed (its
//!   `ObjectState::damage` grown) its `damage` once. A look its model has no
//!   loop for is not taken up; where the scene is built, `default` stands
//!   in. Its clips are found through its model's supermodel chain, as the
//!   executable finds them: a "Sitting" placeable (a creature's model, in
//!   `AnimationState` 3) loops its chain's `dead`, the seated pose.
//! - A creature's body (`Plays::Body`) is posed by the cast; the clips the
//!   cast plays on it (`BodyClips`) key its emitters and pass their
//!   `detonate` events from where the frame before left them: the welding
//!   droid's sparks under its `weld`, a war droid's under its `cdie` and
//!   `cdead`.
//!
//! The view's look only: loops keep the animations' clock (`anim` holds
//! it, the particles with it), a placeable's changes the sim's, and the
//! particles' randomness is the view's own. A model's ground is not known
//! to its particles, so none bounce [verify].

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use asset_odyssey::anim::AnimationIr;
use bevy::prelude::*;
use console::{AnimationTime, DumpDue, DumpFacts};
use sim::module::ObjectKind;
use sim::{Kind, look};

use crate::heat_haze::HeatHazeMaterial;
use crate::lifecycle_view::SceneCamera;
use crate::material::OdysseyMaterial;
use crate::model_fx::{self, Lit, Spray};
#[cfg(feature = "probes")]
use crate::probe_kit::{Checks, check, not_installed};
use crate::scene::{
    Animator, ModelAssets, SceneContent, SceneModel, Spawner, animation_seconds, animator,
    game_to_bevy,
};
use crate::sim_host::PresentedSnapshot;
use crate::viewer::Plays;

/// The most a frame steps the particles: a long frame (a module loading)
/// does not throw them all at once.
const STEP_MAX: f32 = 0.25;

pub struct OwnFxPlugin;

/// Where the rooms and placeables are posed by their clips: after the play
/// view, before the minigame's view.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Posed;

/// Where their particles are drawn: after the minigame's camera is placed,
/// before the transforms are propagated.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Drawn;

/// The clips each creature body plays this frame and how far into each
/// (seconds), by its model's index: written by the cast for the bodies it
/// poses, read for their emitters (`Plays::Body`).
#[derive(Resource, Default)]
pub struct BodyClips(pub BTreeMap<usize, Vec<(Arc<AnimationIr>, f32)>>);

impl Plugin for OwnFxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Movers>()
            .init_resource::<BodyClips>()
            .configure_sets(
                PostUpdate,
                (Posed, Drawn)
                    .chain()
                    .run_if(resource_exists::<SceneContent>)
                    .after(crate::play_view::Drawn)
                    .before(bevy::transform::TransformSystems::Propagate),
            )
            .add_systems(
                PostUpdate,
                ((bind, look, pose).chain().in_set(Posed), draw.in_set(Drawn)),
            );
    }
}

/// The scene's models that move of themselves, which of its `own_fx` were
/// bound, the next emitter's seed, the clock they were last shown at
/// and how far it went since the frame before.
#[derive(Resource)]
struct Movers {
    movers: Vec<Mover>,
    bound: BTreeSet<usize>,
    seed: u32,
    seconds: Option<f32>,
    dt: f32,
}

impl Default for Movers {
    fn default() -> Movers {
        Movers {
            movers: Vec::new(),
            bound: BTreeSet::new(),
            seed: 1,
            seconds: None,
            dt: 0.0,
        }
    }
}

/// A room, placeable or creature's body spawned: which of the scene's
/// `own_fx`, its root and node entities, its clips bound to its nodes (by
/// lowercase name, those that move any), the clips playing this frame with
/// their moments and how long ago each began (past its moment once a loop
/// has come round), a body's clips' moments the frame before, its animated
/// meshes once found, its emitters, and a placeable's look.
struct Mover {
    fx: usize,
    root: Entity,
    nodes: Vec<Entity>,
    animators: BTreeMap<String, Animator>,
    moments: Vec<(Arc<AnimationIr>, f32, f32)>,
    was: BTreeMap<String, f32>,
    /// Its model's light nodes' names, lowercase (a body's lights).
    light_nodes: Vec<String>,
    lit: Option<Vec<Lit>>,
    sprays: Vec<Spray>,
    look: Option<Look>,
}

/// What a placeable shows as the sim has it: whether a snapshot was seen
/// yet, its look, the loop it holds and the clip on its way there, since
/// when (sim ms; `None` as the scene was built, so no way there plays), and
/// the hit points it had lost and when it last lost some.
#[derive(Debug, Default)]
struct Look {
    seen: bool,
    state: u8,
    held: Option<Arc<AnimationIr>>,
    way: Option<Arc<AnimationIr>>,
    since: Option<f64>,
    damage: u16,
    hit: Option<f64>,
}

/// The clip a placeable plays from look `from` on its way to `to`, as the
/// executable picks it: to open always `close2open`, to closed only from
/// open, back to its default the way it came, none to a loop.
fn way(from: u8, to: u8) -> Option<&'static str> {
    match (from, to) {
        (_, look::ON) => Some("off2on"),
        (_, look::OFF) => Some("on2off"),
        (_, look::OPEN) => Some("close2open"),
        (look::OPEN, look::CLOSED) => Some("open2close"),
        (_, look::DEAD) => Some("die"),
        (look::OPEN, look::DEFAULT) => Some("open2close"),
        (look::ON, look::DEFAULT) => Some("on2off"),
        (look::OFF, look::DEFAULT) => Some("off2on"),
        _ => None,
    }
}

/// Every clip a placeable's looks play: each look's loop, the ways between
/// them, and a blow's `damage`.
pub(crate) fn look_clips() -> impl Iterator<Item = String> {
    let looks: Vec<u8> = [
        look::DEFAULT,
        look::OPEN,
        look::CLOSED,
        look::DEAD,
        look::ON,
        look::OFF,
    ]
    .into_iter()
    .chain((0..10).map(|n| look::ANIMLOOP + n))
    .collect();
    let ways: BTreeSet<&str> = (looks.iter())
        .flat_map(|&from| looks.iter().filter_map(move |&to| way(from, to)))
        .chain(["damage"])
        .collect();
    (looks.into_iter().map(look::clip_name)).chain(ways.into_iter().map(String::from))
}

impl Look {
    /// What the snapshot of `at` (sim ms) says of it: its look and the hit
    /// points it lost. A look its model has no loop for is not taken up
    /// (the executable drops it), but where the scene is built, which holds
    /// `default` instead; a change plays its way there first.
    fn see(
        &mut self,
        (state, damage): (u8, u16),
        clips: &BTreeMap<String, Arc<AnimationIr>>,
        at: f64,
    ) {
        let first = !std::mem::replace(&mut self.seen, true);
        if first {
            self.state = state;
            self.held = clips
                .get(&look::clip_name(state))
                .or_else(|| clips.get("default"))
                .cloned();
        } else if state != self.state
            && let Some(held) = clips.get(&look::clip_name(state))
        {
            self.way = way(self.state, state)
                .and_then(|name| clips.get(name))
                .cloned();
            self.since = Some(at);
            self.state = state;
            self.held = Some(held.clone());
        }
        if damage > self.damage && !first {
            self.hit = Some(at);
        }
        self.damage = damage;
    }

    /// The clip it plays at `now` (sim ms; `seconds` the animations' clock)
    /// and how far in: its way to its look once, a blow's `damage` once
    /// while it is closed (as the executable plays it), then its look's
    /// loop.
    fn playing(
        &self,
        clips: &BTreeMap<String, Arc<AnimationIr>>,
        now: f64,
        seconds: f32,
    ) -> Option<(Arc<AnimationIr>, f32, f32)> {
        let once = |clip: Option<&Arc<AnimationIr>>, since: Option<f64>| {
            let clip = clip?;
            let at = (((now - since?) / 1000.0) as f32).max(0.0);
            (at < clip.length).then(|| (clip.clone(), at, at))
        };
        let damage = clips.get("damage").filter(|_| self.state == look::CLOSED);
        once(self.way.as_ref(), self.since)
            .or_else(|| once(damage, self.hit))
            .or_else(|| {
                let clip = self.held.as_ref()?;
                Some((
                    clip.clone(),
                    seconds % clip.length.max(f32::EPSILON),
                    seconds,
                ))
            })
    }
}

/// Binds each model's clips and spawns its emitters' sprays once its
/// model is spawned: a new scene's from its start, a placeable made in
/// play (`made`, which adds to the scene's `own_fx` unnoticed) as it
/// comes; one whose model left the scene is dropped.
fn bind(
    scene: Res<SceneContent>,
    models: Query<(Entity, &SceneModel)>,
    mut movers: ResMut<Movers>,
    mut assets: ModelAssets,
    mut hazes: ResMut<Assets<HeatHazeMaterial>>,
    mut commands: Commands,
) {
    let loaded = &scene.0;
    if scene.is_changed() {
        *movers = Movers::default();
    }
    movers.movers.retain(|mover| models.contains(mover.root));
    let ready: Vec<(usize, (Entity, &SceneModel))> = (0..loaded.own_fx.len())
        .filter(|fx| !movers.bound.contains(fx))
        .filter_map(|fx| {
            let model = loaded.own_fx[fx].model;
            Some((
                fx,
                models.iter().find(|(_, spawned)| spawned.index == model)?,
            ))
        })
        .collect();
    if ready.is_empty() {
        return;
    }
    // The emitters' textures alone get images of their own.
    let textures: BTreeMap<String, _> = ready
        .iter()
        .flat_map(|(fx, _)| {
            let own = &loaded.own_fx[*fx];
            model_fx::texture_names((&own.fx, &own.chunks))
        })
        .filter_map(|name| Some((name.clone(), loaded.textures.get(name)?.clone())))
        .collect();
    let (images, mut stores) = assets.split();
    let spawner = Spawner::new(&textures, loaded.area_light(), images);
    let area = loaded.area_light();
    let mut seed = movers.seed;
    for (fx, (root, spawned)) in ready {
        movers.bound.insert(fx);
        let own = &loaded.own_fx[fx];
        let placed = &loaded.models[own.model];
        let sprays = model_fx::sprays(
            (&own.fx, &placed.ir, &own.chunks),
            (&spawner.textures, &area),
            &mut stores,
            &mut hazes,
            &mut commands,
            root,
            &mut seed,
        );
        let animators = own
            .plays
            .clips()
            .filter_map(|clip| {
                let bound = animator(placed, clip, &spawned.nodes)?;
                Some((clip.name.to_ascii_lowercase(), bound))
            })
            .collect();
        movers.movers.push(Mover {
            fx,
            root,
            nodes: spawned.nodes.clone(),
            animators,
            moments: Vec::new(),
            was: BTreeMap::new(),
            light_nodes: (placed.ir.lights.iter())
                .map(|light| placed.ir.nodes[light.node].name.to_ascii_lowercase())
                .collect(),
            lit: None,
            sprays,
            look: matches!(own.plays, Plays::Look { .. }).then(Look::default),
        });
    }
    movers.seed = seed;
}

/// Each placeable's look as the presented snapshot has it; as placed
/// where no sim runs (`kjk module`).
fn look(
    scene: Res<SceneContent>,
    presented: Option<Res<PresentedSnapshot>>,
    mut movers: ResMut<Movers>,
) {
    let loaded = &scene.0;
    let Some(presented) = presented else {
        for mover in &mut movers.movers {
            if let (Some(look), Plays::Look { placed, clips, .. }) =
                (&mut mover.look, &loaded.own_fx[mover.fx].plays)
                && !look.seen
            {
                look.see((*placed, 0), clips, 0.0);
            }
        }
        return;
    };
    let current = presented.0.current();
    for mover in &mut movers.movers {
        let (Some(look), Plays::Look { object, clips, .. }) =
            (&mut mover.look, &loaded.own_fx[mover.fx].plays)
        else {
            continue;
        };
        let Some(entity) = current
            .entities
            .iter()
            .find(|entity| entity.kind == Kind::Object(ObjectKind::Placeable, *object))
        else {
            continue;
        };
        let state = entity.object.unwrap_or(sim::ObjectState::default());
        look.see((state.state, state.damage), clips, current.time_ms as f64);
    }
}

/// Poses each model by the clips it plays this frame and keys its meshes'
/// alpha and self-illumination.
#[allow(clippy::too_many_arguments)]
fn pose(
    (time, held): (Res<Time>, Res<AnimationTime>),
    scene: Res<SceneContent>,
    (presented, bodies): (Option<Res<PresentedSnapshot>>, Res<BodyClips>),
    mut movers: ResMut<Movers>,
    mut transforms: Query<&mut Transform>,
    (children, mut mesh_materials): (
        Query<&Children>,
        Query<&mut MeshMaterial3d<OdysseyMaterial>>,
    ),
    mut materials: ResMut<Assets<OdysseyMaterial>>,
) {
    let movers = &mut *movers;
    if movers.movers.is_empty() {
        return;
    }
    let loaded = &scene.0;
    let seconds = animation_seconds(&time, &held);
    movers.dt = movers
        .seconds
        .replace(seconds)
        .map_or(0.0, |was| (seconds - was).clamp(0.0, STEP_MAX));
    // The sim's time between the presented snapshots (ms), and the room
    // clip a script plays.
    let (now, asked) = presented.map_or((0.0, None), |presented| {
        (
            crate::sim_host::view_ms(&presented.0),
            presented.0.current().area_sound.room,
        )
    });
    let looped = |clip: &Arc<AnimationIr>, elapsed: f32| {
        (
            clip.clone(),
            elapsed % clip.length.max(f32::EPSILON),
            elapsed,
        )
    };
    for mover in &mut movers.movers {
        let own = &loaded.own_fx[mover.fx];
        let ir = &loaded.models[own.model].ir;
        // Each clip's moment; the later ones' keys over the earlier ones'.
        mover.moments = match (&own.plays, &mover.look) {
            (
                Plays::Room {
                    loops,
                    scripted,
                    room,
                },
                _,
            ) => {
                // A script's clip loops from when it began, over the loops.
                let script = asked.filter(|asked| asked.room == *room).and_then(|asked| {
                    let since = ((now - asked.since_ms as f64).max(0.0) / 1000.0) as f32;
                    Some(looped(scripted.get(&asked.anim)?, since))
                });
                loops
                    .iter()
                    .map(|clip| looped(clip, seconds))
                    .chain(script)
                    .collect()
            }
            (Plays::Look { clips, .. }, Some(look)) => {
                look.playing(clips, now, seconds).into_iter().collect()
            }
            (Plays::Look { .. }, None) => Vec::new(),
            // Posed by the cast: only its emitters are keyed here.
            (Plays::Body, _) => {
                let playing = bodies.0.get(&own.model).into_iter().flatten();
                playing.map(|(clip, at)| (clip.clone(), *at, *at)).collect()
            }
        };
        for (clip, at, _) in &mover.moments {
            if let Some(animator) = mover.animators.get(&clip.name.to_ascii_lowercase()) {
                animator.pose(*at, &mut transforms);
            }
        }
        if mover.lit.is_none() && !matches!(own.plays, Plays::Body) {
            mover.lit = model_fx::own_materials(
                (&own.fx, ir, &mover.nodes),
                &model_fx::glows(ir, &loaded.textures),
                &children,
                &mut mesh_materials,
                &mut materials,
            );
        }
        for lit in mover.lit.iter_mut().flatten() {
            for (clip, at, _) in &mover.moments {
                lit.key((&own.fx, ir), &clip.name.to_ascii_lowercase(), *at);
            }
            lit.show(&mut materials);
        }
    }
}

/// Keys each model's emitters by the clips it plays, steps their particles
/// and draws a shown model's, facing the camera as placed.
fn draw(
    scene: Res<SceneContent>,
    mut movers: ResMut<Movers>,
    transforms: Query<&Transform>,
    cameras: Query<Entity, SceneCamera>,
    visibility: Query<&Visibility>,
    (mut meshes, mut body_lights): (ResMut<Assets<Mesh>>, ResMut<crate::scene::BodyLights>),
    (mut facts, dump_due): (ResMut<DumpFacts>, Res<DumpDue>),
) {
    let movers = &mut *movers;
    if movers.movers.is_empty() {
        body_lights.set_if_neq(crate::scene::BodyLights(Vec::new()));
        return;
    }
    let mut carried = Vec::new();
    let (loaded, dt) = (&scene.0, movers.dt);
    // The camera's right and up, in the game's space, turned into each
    // model's (a room is only moved; a placeable turned by its bearing).
    let turn = cameras
        .iter()
        .next()
        .and_then(|camera| transforms.get(camera).ok())
        .map_or(Quat::IDENTITY, |t| t.rotation);
    let local = |e: Entity| transforms.get(e).ok().map(|t| t.to_matrix());
    for mover in &mut movers.movers {
        let own = &loaded.own_fx[mover.fx];
        let placed = &loaded.models[own.model];
        let ir = &placed.ir;
        let body = matches!(own.plays, Plays::Body);
        // A body turns as the cast turns its root.
        let bearing = match body {
            true => transforms
                .get(mover.root)
                .map_or(Quat::IDENTITY, |t| t.rotation),
            false => Quat::from_rotation_z(placed.facing),
        };
        let game = bearing.inverse() * game_to_bevy().inverse() * turn;
        let facing = (game * Vec3::X, game * Vec3::Y);
        // The clips' names as the model's effects key them, once a frame.
        let names: Vec<String> = (mover.moments.iter())
            .map(|(clip, _, _)| clip.name.to_ascii_lowercase())
            .collect();
        // A body's clips pass their events from where the frame before
        // left them (the cast says nothing of how long ago one began).
        let was = std::mem::take(&mut mover.was);
        let body_detonated = body
            && mover.moments.iter().any(|(clip, at, _)| {
                was.get(&clip.name)
                    .is_some_and(|&from| model_fx::detonates(clip, from, *at))
            });
        let shown = visibility
            .get(mover.root)
            .is_ok_and(|v| *v != Visibility::Hidden);
        if body {
            mover.was = (mover.moments.iter())
                .map(|(clip, at, _)| (clip.name.clone(), *at))
                .collect();
            // A hidden body (gone, stealthed) lights nothing; nor does a
            // light its clips leave dark.
            let root = transforms
                .get(mover.root)
                .map_or(Mat4::IDENTITY, |t| t.to_matrix());
            let playing: Vec<(&str, f32)> = (names.iter().map(String::as_str))
                .zip(mover.moments.iter().map(|(_, at, _)| *at))
                .collect();
            let lit = (ir.lights.iter().zip(&mover.light_nodes)).filter(|_| shown);
            carried.extend(
                lit.map(|(light, node)| {
                    let at = root * model_fx::node_matrix((ir, &mover.nodes), light.node, local);
                    body_light((&own.fx, node), light, &playing, at)
                })
                .filter(|light| light.color_multiplier.truncate() != Vec3::ZERO),
            );
        }
        let spaces: Vec<Mat4> = mover
            .sprays
            .iter()
            .map(|spray| {
                spray.node.map_or(Mat4::IDENTITY, |node| {
                    model_fx::node_matrix((ir, &mover.nodes), node, local)
                })
            })
            .collect();
        model_fx::aim(
            &mut mover.sprays,
            &spaces,
            (ir, &mover.nodes, Mat4::IDENTITY),
            local,
        );
        for (spray, space) in mover.sprays.iter_mut().zip(spaces) {
            let mut detonated = body_detonated;
            for ((clip, at, elapsed), name) in mover.moments.iter().zip(&names) {
                spray.key(&own.fx, name, *at);
                detonated |= !body && model_fx::detonates_over(clip, (*at, *elapsed), dt);
            }
            spray.step(&own.fx, (dt, detonated), (space, None), true);
            if shown {
                spray.fill((&own.fx, &own.chunks), space, facing, true, &mut meshes);
            }
        }
    }
    body_lights.set_if_neq(crate::scene::BodyLights(carried));
    if dump_due.0 {
        let lights: Vec<[f32; 4]> = (body_lights.0.iter())
            .map(|light| light.color_multiplier.to_array())
            .collect();
        facts
            .0
            .insert("body_lights".into(), serde_json::json!(lights));
        // Rooms, placeables and bodies, each under its own key.
        let kind = |plays: &Plays| match plays {
            Plays::Room { .. } => "room_fx",
            Plays::Look { .. } => "placeable_fx",
            Plays::Body => "body_fx",
        };
        for key in ["room_fx", "placeable_fx", "body_fx"] {
            let dumped: Vec<serde_json::Value> = movers
                .movers
                .iter()
                .filter(|mover| kind(&loaded.own_fx[mover.fx].plays) == key)
                .map(|mover| {
                    let own = &loaded.own_fx[mover.fx];
                    serde_json::json!({
                        "model": loaded.models[own.model].ir.name,
                        "object": match &own.plays {
                            Plays::Look { object, .. } => Some(object),
                            Plays::Room { .. } | Plays::Body => None,
                        },
                        "clips": own.plays.clips().map(|clip| (&clip.name, clip.length)).collect::<Vec<_>>(),
                        "look": mover.look.as_ref().map(|look| look.state),
                        "playing": mover.moments.iter().map(|(clip, at, _)| (&clip.name, at)).collect::<Vec<_>>(),
                        "posed": mover.animators.len(),
                        "lit": mover.lit.as_ref().map_or(0, Vec::len),
                        "emitters": mover.sprays.len(),
                        "particles": mover.sprays.iter().map(Spray::particles).sum::<usize>(),
                        "shown": visibility.get(mover.root).is_ok_and(|v| *v != Visibility::Hidden),
                    })
                })
                .collect();
            facts.0.insert(key.into(), serde_json::json!(dumped));
        }
    }
}

/// The room that plays a script's clip, and its number: Peragus's
/// `103pert`, whose `scriptloop01` keys its three puffs' birth rate (none
/// at rest).
#[cfg(feature = "probes")]
const SCRIPTED_ROOM: (&str, &str, u8) = ("103per", "103pert", 1);

/// A script's room clip played in a module's scene.
#[cfg(feature = "probes")]
pub fn probes() -> Checks {
    vec![("script_room_clip_emits", script_room_clip_probe())]
}

/// TSL's `103per` with its rooms and placeables spawned and the sim saying
/// a script plays `scriptloop01` on `103pert` from 0 ms: stepped 2 s at
/// 0.1 s (the sim's clock with it), the room plays the clip and its puffs
/// have particles.
#[cfg(feature = "probes")]
fn script_room_clip_probe() -> Result<String, String> {
    use crate::scene::{Culling, SceneContent, spawn_scene};
    let (module, room, anim) = SCRIPTED_ROOM;
    let discovery = crate::discover();
    if discovery.get(vfs::Game::Kotor2).is_none() {
        return Ok(not_installed(vfs::Game::Kotor2));
    }
    let scene = crate::viewer::load_module(&discovery, "tsl", module)?;
    let mut start = sim::SimWorld::new(sim::WorldConfig::one_seat(), sim::Level::arena())
        .map_err(|why| format!("{why:?}"))?
        .snapshot();
    start.area_sound.room = Some(sim::RoomAnim {
        room: sim::room_key(room),
        anim,
        since_ms: 0,
    });
    let presented = |ms: u64| {
        let mut snapshot = start.clone();
        snapshot.time_ms = ms;
        PresentedSnapshot(sim::Presented::new(snapshot), Vec::new())
    };
    let mut app = crate::probe_kit::scene_app();
    app.add_plugins(OwnFxPlugin)
        .init_resource::<Assets<crate::scene_fx::RefractionMaterial>>()
        .init_resource::<Culling>()
        .insert_resource(presented(0))
        .insert_resource(SceneContent(Box::new(scene)))
        .add_systems(
            Update,
            spawn_scene.run_if(resource_exists_and_changed::<SceneContent>),
        );
    for step in 0..=20u64 {
        let world = app.world_mut();
        world.resource_mut::<AnimationTime>().0 = Some(step as f32 / 10.0);
        world.insert_resource(presented(step * 100));
        world.resource_mut::<DumpDue>().0 = step == 20;
        app.update();
    }
    let facts = app.world().resource::<DumpFacts>();
    let rooms = facts
        .0
        .get("room_fx")
        .and_then(|rooms| rooms.as_array())
        .ok_or("no room_fx")?;
    let entry = rooms
        .iter()
        .find(|fx| fx["model"] == room)
        .ok_or(format!("no room_fx entry for {room}"))?;
    let scripted = |fx: &serde_json::Value, wanted: &str| {
        fx["playing"].as_array().is_some_and(|clips| {
            clips.iter().any(|played| {
                played[0]
                    .as_str()
                    .is_some_and(|name| name.starts_with(wanted))
            })
        })
    };
    let playing = scripted(entry, &format!("scriptloop{anim:02}"));
    // No other room plays a script's clip.
    let others = rooms
        .iter()
        .filter(|fx| fx["model"] != room && scripted(fx, "scriptloop"))
        .count();
    let particles = entry["particles"].as_u64().unwrap_or(0);
    check(
        playing && particles > 0 && others == 0,
        format!(
            "{room}: playing {}, {} emitters, {particles} particles; {others} other rooms playing a script's clip",
            entry["playing"], entry["emitters"]
        ),
    )
}

/// Light `light` of a body (its node `node`, lowercase, in `fx`'s model)
/// where `at` (game space, the node's transform) puts it, its colour,
/// radius and multiplier as the last of the clips playing (`playing`, by
/// lowercase name and moment) to key each leaves it, else at rest: the
/// welding droid's `auroralight01` flickering under `weld`, dark at rest.
fn body_light(
    (fx, node): (&model_fx::ModelFx, &str),
    light: &asset_odyssey::model::LightIr,
    playing: &[(&str, f32)],
    at: Mat4,
) -> crate::material::GpuLight {
    use fmt_odyssey::mdl::{
        CONTROLLER_LIGHT_COLOR, CONTROLLER_LIGHT_MULTIPLIER, CONTROLLER_LIGHT_RADIUS,
    };
    let keyed = |kind: u32| {
        (playing.iter().rev()).find_map(|&(clip, at)| fx.light_key((clip, node), kind, at))
    };
    let colour = match keyed(CONTROLLER_LIGHT_COLOR).as_deref() {
        Some(&[r, g, b, ..]) => Vec3::new(r, g, b),
        _ => Vec3::from_array(light.color),
    };
    let first = |kind: u32, rest: f32| keyed(kind).and_then(|v| v.first().copied()).unwrap_or(rest);
    let position = game_to_bevy() * at.transform_point3(Vec3::ZERO);
    crate::material::GpuLight {
        position_radius: position.extend(first(CONTROLLER_LIGHT_RADIUS, light.radius)),
        color_multiplier: colour.extend(first(CONTROLLER_LIGHT_MULTIPLIER, light.multiplier)),
        kind: UVec4::new(u32::from(light.ambient_only), light.dynamic_type, 0, 0),
    }
}
