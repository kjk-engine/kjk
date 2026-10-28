//! A model drawn by the clips playing on it, outside the scene's own
//! (`cast`, the rooms): the minigames' things (`minigame_view`) and the
//! scripts' visual effects (`script_fx`). Each is read off the frame
//! (`load`), spawned under the scene's root (`spawn`), posed by its clips
//! each frame (`animate`), and drawn with what its clips key beyond places
//! and turns: its meshes' alpha and self-illumination and its emitters'
//! particles (`effects`, `model_fx`).

use std::collections::BTreeMap;
use std::sync::Arc;

use asset_odyssey::anim::{AnimationIr, sample_orientation, sample_position};
use asset_odyssey::texture::TextureIr;
use bevy::prelude::*;
use sim::minigame::Anim;
use vfs::resolver::Resolver;

use crate::heat_haze::HeatHazeMaterial;
use crate::material::{AreaLight, OdysseyMaterial, Surface};
use crate::model_fx::{self, Chunk, Lit, ModelFx, Spray};
use crate::scene::{SceneModel, Spawner, Stores};
use crate::viewer::{self, Placed};

/// A model read: its placement, whether it turns with its thing's offset
/// (a minigame's turret), its clips by name (lowercase), and its emitters'
/// chunk models.
pub struct Model {
    pub placed: Placed,
    pub rotating: bool,
    pub clips: BTreeMap<String, Arc<AnimationIr>>,
    pub fx: Arc<ModelFx>,
    pub chunks: Arc<BTreeMap<String, Chunk>>,
    /// Its lowest point at rest (model space; the swoop's shadow lies on
    /// the track): where its thing's ground is.
    pub floor: f32,
}

impl Model {
    /// Another copy of it, to spawn again (a bolt's pool).
    pub fn copy(&self) -> Model {
        Model {
            placed: Placed::at(self.placed.ir.clone(), [0.0; 3]),
            rotating: self.rotating,
            clips: self.clips.clone(),
            fx: self.fx.clone(),
            chunks: self.chunks.clone(),
            floor: self.floor,
        }
    }
}

/// Model `name` read from `resolver`; `None` where the install lacks it.
pub fn load(resolver: &Resolver, name: &str, rotating: bool) -> Option<Model> {
    let ir = viewer::model(resolver, name).ok()??;
    let clips = ir
        .animations
        .iter()
        .map(|clip| (clip.name.clone(), Arc::new(clip.clone())))
        .collect();
    let fx = viewer::parsed(resolver, name)
        .ok()
        .flatten()
        .map_or_else(ModelFx::default, |model| ModelFx::read(&model));
    let chunks = fx.chunks(resolver);
    let floor = ir
        .meshes
        .iter()
        .flat_map(|mesh| {
            let world = Mat4::from_cols_array_2d(&ir.nodes[mesh.node].world).transpose();
            mesh.positions
                .iter()
                .map(move |p| world.transform_point3(Vec3::from_array(*p)).z)
        })
        .fold(f32::INFINITY, f32::min);
    Some(Model {
        placed: Placed::at(ir, [0.0; 3]),
        rotating,
        clips,
        fx: Arc::new(fx),
        chunks: Arc::new(chunks),
        floor: if floor.is_finite() { floor } else { 0.0 },
    })
}

/// The textures `models` draw with, their emitters' and chunks' too.
pub fn textures<'m>(
    resolver: &Resolver,
    models: impl Iterator<Item = &'m Model> + Clone,
) -> BTreeMap<String, TextureIr> {
    let (mut textures, _) = viewer::textures(resolver, models.clone().map(|model| &model.placed));
    for model in models {
        model_fx::add_textures(resolver, (&model.fx, &model.chunks), &mut textures);
    }
    textures
}

/// The names of the textures `model` is drawn with: its meshes' (diffuse
/// and lightmap), its emitters' and their chunks'.
pub fn texture_names(model: &Model) -> impl Iterator<Item = &str> {
    model
        .placed
        .ir
        .meshes
        .iter()
        .flat_map(|mesh| [mesh.texture.as_deref(), mesh.lightmap.as_deref()])
        .flatten()
        .chain(model_fx::texture_names((&model.fx, &model.chunks)).map(String::as_str))
}

/// A model spawned: its root and node entities, its animated meshes once
/// found, its emitters, and what its clips left: the clips played once
/// whose ends have landed (name, when they began), each node's place and
/// turn as last shown, and how far it banks (`minigame_view`'s swoop).
pub struct Spawned {
    pub model: Model,
    pub root: Entity,
    pub nodes: Vec<Entity>,
    pub lit: Option<Vec<Lit>>,
    pub sprays: Vec<Spray>,
    pub ended: Vec<(String, u64)>,
    pub shown: Vec<Option<(Vec3, Quat)>>,
    /// Each node's place and turn as the clips in place of the others last
    /// left them (the hull bar's lost segments stay down).
    pub kept: Vec<(Vec3, Quat)>,
    pub bank: f32,
    /// The clips playing on it this frame: its thing's, and those the
    /// engine plays itself (`engine`: a gun's `fire`, a hit's `damage`, a
    /// spent bolt's `explode`).
    pub anims: Vec<Anim>,
    pub engine: Vec<Anim>,
    /// The nodes whose mesh's texture its TXI makes additive (`blending
    /// additive`), alpha or not.
    pub glows: Vec<usize>,
}

impl Spawned {
    /// Made new for another use: no clip's ends, pose, surfaces or
    /// particles carried over from the last.
    pub fn reset(&mut self) {
        self.ended.clear();
        self.shown.fill(None);
        self.kept.clear();
        self.bank = 0.0;
        self.engine.clear();
        let ir = &self.model.placed.ir;
        for lit in self.lit.iter_mut().flatten() {
            lit.reset(ir);
        }
        for spray in &mut self.sprays {
            spray.reset(&self.model.fx);
        }
    }

    /// Hidden whole: its model and its sprays, which hang from the root it
    /// was spawned under. `Drawing::effects` shows the sprays again while
    /// the model shows.
    pub fn hide(&self, visibility: &mut Query<&mut Visibility>) {
        for entity in std::iter::once(self.root).chain(self.sprays.iter().map(|s| s.entity)) {
            if let Ok(mut visible) = visibility.get_mut(entity) {
                *visible = Visibility::Hidden;
            }
        }
    }

    /// Its root and every entity under it gone.
    pub fn despawn(&self, commands: &mut Commands) {
        for spray in &self.sprays {
            commands.entity(spray.entity).despawn();
        }
        commands.entity(self.root).despawn();
    }
}

/// What spawning needs: the spawner (its textures), the model stores, the
/// heat hazes' materials, the textures read, the area's light, whether the
/// scene is lit, the root to spawn under, and the emitters' seeds, drawn
/// on.
pub(crate) struct Spawning<'a, 'w, 's> {
    pub spawner: Spawner<'a>,
    pub stores: Stores<'a>,
    pub hazes: &'a mut Assets<HeatHazeMaterial>,
    pub textures: &'a BTreeMap<String, TextureIr>,
    pub area: AreaLight,
    pub lit: bool,
    pub root: Entity,
    pub seed: u32,
    pub commands: &'a mut Commands<'w, 's>,
}

impl Spawning<'_, '_, '_> {
    /// `model` spawned under the root, hidden if so.
    pub fn spawn(&mut self, model: Model, hidden: bool) -> Spawned {
        let (entity, nodes) = self.spawner.model(
            self.commands,
            &mut self.stores,
            (&model.placed, usize::MAX, !self.lit),
            (self.root, Transform::IDENTITY),
        );
        // Not one of the scene's own models (`cast`, `dialogue_view`).
        self.commands.entity(entity).remove::<SceneModel>();
        if hidden {
            self.commands.entity(entity).insert(Visibility::Hidden);
        }
        let sprays = model_fx::sprays(
            (&model.fx, &model.placed.ir, &model.chunks),
            (&self.spawner.textures, &self.area),
            &mut self.stores,
            self.hazes,
            self.commands,
            self.root,
            &mut self.seed,
        );
        // The sprays hang from the root spawned under, not the model's: a
        // model made hidden hides them too, else each of a pool's idle
        // copies' sprays is drawn every frame (`Spawned::hide`).
        if hidden {
            for spray in &sprays {
                self.commands
                    .entity(spray.entity)
                    .insert(Visibility::Hidden);
            }
        }
        let count = model.placed.ir.nodes.len();
        let glows = model_fx::glows(&model.placed.ir, self.textures);
        Spawned {
            model,
            root: entity,
            nodes,
            lit: None,
            sprays,
            ended: Vec::new(),
            shown: vec![None; count],
            kept: Vec::new(),
            bank: 0.0,
            anims: Vec::new(),
            engine: Vec::new(),
            glows,
        }
    }
}

/// A clip's moment `seconds` after it began: looping round, `None` once a
/// clip played once is over.
pub fn moment(clip: &AnimationIr, looping: bool, seconds: f32) -> Option<f32> {
    if clip.length <= 0.0 {
        Some(0.0)
    } else if looping {
        Some(seconds % clip.length)
    } else {
        (seconds <= clip.length).then_some(seconds)
    }
}

/// A clip playing on a model: its name and keys, its moment (s) and how
/// long ago it began (s; past the moment once a loop has come round), when
/// it began (ms) and whether it plays over the others.
pub struct Playing<'m> {
    pub name: &'m str,
    pub clip: &'m Arc<AnimationIr>,
    pub at: f32,
    pub elapsed: f32,
    pub since: f64,
    pub overlay: bool,
}

/// The clips playing on a model at `now_ms`: the last one not over the
/// others that the model has (a clip plays on the models that have it:
/// the turret's compass on its radar, its hull bar on its own), else the
/// model's `ready…` clip, looping; then the overlays in the order they
/// began.
pub fn active<'m>(model: &'m Model, anims: &[Anim], now_ms: f64) -> Vec<Playing<'m>> {
    let base = anims.iter().rposition(|anim| {
        !anim.overlay && model.clips.contains_key(&anim.name.to_ascii_lowercase())
    });
    let mut playing = Vec::new();
    // Idling when nothing plays in place of the others (or it is over).
    let base_plays = base.is_some_and(|k| {
        let anim = &anims[k];
        model
            .clips
            .get(&anim.name.to_ascii_lowercase())
            .is_some_and(|clip| {
                let seconds = ((now_ms - anim.since as f64) / 1000.0).max(0.0) as f32;
                moment(clip, anim.looping, seconds).is_some()
            })
    });
    if !base_plays
        && let Some((name, clip)) = model
            .clips
            .iter()
            .find(|(name, _)| name.starts_with("ready"))
        && let Some(at) = moment(clip, true, (now_ms / 1000.0) as f32)
    {
        playing.push(Playing {
            name,
            clip,
            at,
            elapsed: (now_ms / 1000.0) as f32,
            since: 0.0,
            overlay: false,
        });
    }
    for (k, anim) in anims.iter().enumerate() {
        if !anim.overlay && Some(k) != base {
            continue;
        }
        let Some((name, clip)) = model.clips.get_key_value(&anim.name.to_ascii_lowercase()) else {
            continue;
        };
        let seconds = ((now_ms - anim.since as f64) / 1000.0).max(0.0) as f32;
        if let Some(at) = moment(clip, anim.looping, seconds) {
            playing.push(Playing {
                name,
                clip,
                at,
                elapsed: seconds,
                since: anim.since as f64,
                overlay: anim.overlay,
            });
        }
    }
    playing
}

/// The clips played once that have ended since the last frame, at their
/// ends (once; `ended` remembers them): their last keys land on what a
/// model keeps (its alpha, self-illumination and emitters' values) before
/// those of the clips playing.
pub fn ended_since<'m>(
    model: &'m Model,
    anims: &[Anim],
    now_ms: f64,
    ended: &mut Vec<(String, u64)>,
) -> Vec<Playing<'m>> {
    ended.retain(|(name, since)| {
        anims
            .iter()
            .any(|anim| anim.since == *since && anim.name.eq_ignore_ascii_case(name))
    });
    let mut landed = Vec::new();
    for anim in anims.iter().filter(|anim| !anim.looping) {
        let Some((name, clip)) = model.clips.get_key_value(&anim.name.to_ascii_lowercase()) else {
            continue;
        };
        let seconds = ((now_ms - anim.since as f64) / 1000.0) as f32;
        let over = moment(clip, false, seconds.max(0.0)).is_none();
        if over && !ended.iter().any(|(n, s)| n == name && *s == anim.since) {
            ended.push((name.clone(), anim.since));
            landed.push(Playing {
                name,
                clip,
                at: clip.length,
                elapsed: clip.length,
                since: anim.since as f64,
                overlay: anim.overlay,
            });
        }
    }
    landed
}

/// Poses a model by the clips playing on it at `now_ms`, `dt` seconds after
/// the last frame, with `turns` (by node, relative to rest: the swoop's
/// bank) on top: the clip in place of the others over where such clips
/// last left each node (rest at first: a clip keys only what it moves, as
/// the turret's hull bar drops a segment a clip), then on each node what
/// the last overlay to key it keys, added (the swoop's camera pulled back by
/// `cDistL` while `camshake` shakes it), and the scale the last of them
/// keys. A node a clip began keying less than its transition ago eases
/// there from where it was last shown.
pub fn animate(
    spawned: &mut Spawned,
    anims: &[Anim],
    (now_ms, dt): (f64, f32),
    turns: Vec<(usize, Quat)>,
    q: &mut Query<&mut Transform>,
) {
    let ir = &spawned.model.placed.ir;
    let rest: Vec<(Vec3, Quat)> = ir
        .nodes
        .iter()
        .map(|node| {
            let [x, y, z, w] = node.rotation;
            (
                Vec3::from_array(node.translation),
                Quat::from_xyzw(x, y, z, w),
            )
        })
        .collect();
    // Where the clips in place of the others left each node.
    let mut pose = if spawned.kept.len() == rest.len() {
        spawned.kept.clone()
    } else {
        rest.clone()
    };
    let mut over: Vec<(Option<Vec3>, Option<Quat>)> = vec![(None, None); rest.len()];
    // The youngest clip keying each node: how long it has played, its
    // transition.
    let mut young: Vec<Option<(f32, f32)>> = vec![None; rest.len()];
    // The scale the clips key on a node (a shield's sphere drawn in), the
    // last to key it.
    let mut scales: Vec<Option<f32>> = vec![None; rest.len()];
    for playing in active(&spawned.model, anims, now_ms) {
        for (node, surface) in spawned
            .model
            .fx
            .surfaces
            .get(playing.name)
            .into_iter()
            .flatten()
        {
            if let (Some(k), Some(scale)) = (
                ir.node_index(node),
                model_fx::sample_alpha(&surface.scale, playing.at),
            ) {
                scales[k] = Some(scale);
            }
        }
        let age = ((now_ms - playing.since) / 1000.0) as f32;
        // The nodes the clip poses (`AnimationIr::bind`), each by its keys.
        let bound = playing.clip.bind(ir);
        for (k, track) in bound
            .iter()
            .enumerate()
            .filter_map(|(k, track)| Some((k, &playing.clip.tracks[(*track)?])))
        {
            let place = sample_position(&track.positions, playing.at)
                .map(|key| ir.animation_scale * Vec3::from_array(key));
            let turn = sample_orientation(&track.orientations, playing.at)
                .map(|[x, y, z, w]| Quat::from_xyzw(x, y, z, w));
            if place.is_none() && turn.is_none() {
                continue;
            }
            if playing.overlay {
                over[k].0 = place.or(over[k].0);
                over[k].1 = turn.or(over[k].1);
            } else {
                if let Some(place) = place {
                    pose[k].0 = rest[k].0 + place;
                }
                if let Some(turn) = turn {
                    pose[k].1 = turn;
                }
            }
            if young[k].is_none_or(|(was, _)| age < was) {
                young[k] = Some((age, playing.clip.transition));
            }
        }
    }
    spawned.kept = pose.clone();
    for (k, (place, turn)) in over.into_iter().enumerate() {
        if let Some(place) = place {
            pose[k].0 += place;
        }
        if let Some(turn) = turn {
            pose[k].1 = pose[k].1 * rest[k].1.inverse() * turn;
        }
    }
    for (k, turn) in turns {
        pose[k].1 *= turn;
    }
    for (k, (mut place, mut turn)) in pose.into_iter().enumerate() {
        if let (Some((age, transition)), Some((was_place, was_turn))) = (young[k], spawned.shown[k])
            && age < transition
        {
            let f = (dt / (transition - age + dt).max(f32::EPSILON)).clamp(0.0, 1.0);
            place = was_place.lerp(place, f);
            turn = was_turn.slerp(turn, f);
        }
        spawned.shown[k] = Some((place, turn));
        if let Ok(mut transform) = q.get_mut(spawned.nodes[k]) {
            transform.translation = place;
            transform.rotation = turn;
            if let Some(scale) = scales[k] {
                transform.scale = Vec3::splat(scale);
            }
        }
    }
}

/// A node's transform in the model's space, from the posed transforms
/// (`local`: an entity's).
pub fn node_matrix(
    spawned: &Spawned,
    index: usize,
    local: impl Fn(Entity) -> Option<Mat4>,
) -> Mat4 {
    model_fx::node_matrix((&spawned.model.placed.ir, &spawned.nodes), index, local)
}

/// What drawing a model's effects reads and writes.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Drawing<'w, 's> {
    pub transforms: Query<'w, 's, &'static Transform>,
    children: Query<'w, 's, &'static Children>,
    mesh_materials: Query<'w, 's, &'static mut MeshMaterial3d<OdysseyMaterial>>,
    materials: ResMut<'w, Assets<OdysseyMaterial>>,
    meshes: ResMut<'w, Assets<Mesh>>,
    pub visibility: Query<'w, 's, &'static mut Visibility>,
}

impl Drawing<'_, '_> {
    /// `spawned`'s meshes given materials of their own drawn at `alpha` of
    /// what they were, blended (a co-op race's ghosts, M10); its keyed
    /// meshes made after take theirs from these. Whether its meshes stood
    /// yet: none the frame it is spawned.
    pub fn fade(&mut self, spawned: &Spawned, alpha: f32) -> bool {
        let meshes: Vec<Entity> = spawned
            .nodes
            .iter()
            .filter_map(|node| self.children.get(*node).ok())
            .flat_map(|kids| kids.iter())
            .filter(|kid| self.mesh_materials.contains(*kid))
            .collect();
        for entity in &meshes {
            let Ok(mut handle) = self.mesh_materials.get_mut(*entity) else {
                continue;
            };
            let Some(mut faded) = self.materials.get(&handle.0).cloned() else {
                continue;
            };
            faded.params.tint.w *= alpha;
            if matches!(
                faded.surface,
                Surface::Opaque | Surface::OpaqueAlpha | Surface::OpaqueEnvMasked
            ) {
                faded.surface = Surface::Blend;
            }
            handle.0 = self.materials.add(faded);
        }
        !meshes.is_empty()
    }

    /// What `spawned`'s clips key beyond places and turns at `now_ms`, `dt`
    /// seconds on, its particles meeting `surfaces`: its animated meshes' alpha and
    /// self-illumination (each given a material of its own once spawned,
    /// kept where the last clip to key them left them: the start lights
    /// stay lit as the count goes on, a thruster's glow from gear to gear),
    /// and its emitters' particles from their nodes. A model gone from view
    /// (a fighter after its `die`) leaves its particles to live out their
    /// lives, none born. `facing`: the camera's right and up, in the game's
    /// space (`facing`).
    pub fn effects(
        &mut self,
        spawned: &mut Spawned,
        surfaces: Option<model_fx::Surfaces>,
        (now_ms, dt): (f64, f32),
        facing: (Vec3, Vec3),
    ) {
        let anims = spawned.anims.clone();
        let ended = ended_since(&spawned.model, &anims, now_ms, &mut spawned.ended);
        let playing = active(&spawned.model, &anims, now_ms);
        let shown = self
            .visibility
            .get(spawned.root)
            .is_ok_and(|v| *v != Visibility::Hidden);
        if spawned.lit.is_none() {
            let model = &spawned.model;
            spawned.lit = model_fx::own_materials(
                (&model.fx, &model.placed.ir, &spawned.nodes),
                &spawned.glows,
                &self.children,
                &mut self.mesh_materials,
                &mut self.materials,
            );
        }
        let model = (&*spawned.model.fx, &spawned.model.placed.ir);
        for lit in spawned.lit.iter_mut().flatten() {
            for playing in ended.iter().chain(&playing) {
                lit.key(model, playing.name, playing.at);
            }
            lit.show(&mut self.materials);
        }
        let transforms = &self.transforms;
        let local = |e: Entity| transforms.get(e).ok().map(|t| t.to_matrix());
        let root = local(spawned.root).unwrap_or(Mat4::IDENTITY);
        let spaces: Vec<Mat4> = spawned
            .sprays
            .iter()
            .map(|spray| {
                root * spray
                    .node
                    .map_or(Mat4::IDENTITY, |node| node_matrix(spawned, node, local))
            })
            .collect();
        // A lightning emitter's bolts end at its reference node.
        model_fx::aim(
            &mut spawned.sprays,
            &spaces,
            (&spawned.model.placed.ir, &spawned.nodes, root),
            local,
        );
        let (fx, chunks) = (&*spawned.model.fx, &*spawned.model.chunks);
        for (spray, space) in spawned.sprays.iter_mut().zip(spaces) {
            let mut detonated = false;
            for playing in &ended {
                spray.key(fx, playing.name, playing.at);
            }
            for playing in &playing {
                spray.key(fx, playing.name, playing.at);
                detonated |=
                    model_fx::detonates_over(playing.clip, (playing.at, playing.elapsed), dt);
            }
            let lingering = !shown && spray.particles() > 0;
            if shown || lingering {
                spray.step(fx, (dt, detonated && shown), (space, surfaces), shown);
            }
            spray.fill(
                (fx, chunks),
                space,
                facing,
                shown || lingering,
                &mut self.meshes,
            );
            if let Ok(mut visible) = self.visibility.get_mut(spray.entity) {
                *visible = if shown || lingering {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                };
            }
        }
    }
}

/// The camera's right and up in the game's space, from its turn (`cameras`'
/// first; Bevy's axes).
pub fn facing(turn: Quat) -> (Vec3, Vec3) {
    let game = crate::scene::game_to_bevy().inverse() * turn;
    (game * Vec3::X, game * Vec3::Y)
}
