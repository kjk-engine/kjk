//! What a model's clips key beyond places and turns: its meshes' alpha and
//! self-illumination (the swoop's start lights, its thrusters' glow cards)
//! and its particle emitters (the thrusters' flames, the rocks a buoy throws
//! up, Tatooine's blowing sand, Manaan's birds), read from the model's own
//! MDL; drawn for a minigame's models (`minigame_view`) and the rooms'
//! (`own_fx`).
//!
//! An emitter's values (reone's controller numbers) are its node's own,
//! as the clips playing on its model key them over that; the view keeps
//! them where the last clip left them (`Values`). A `Fountain` gives birth
//! at its birthrate (and its random one) a second, a `Single` keeps one
//! particle (again when it dies, if it loops), an `Explosion` bursts its
//! birthrate's worth when a clip playing on its model passes a `detonate`
//! event. A particle is born within the emitter's size, heads up its local
//! z within its spread at its velocity (plus up to its random velocity),
//! falls by its mass (metres a second each second [verify]: reone keeps
//! gravity for point-to-point emitters only), spins at its particle
//! rotation (radians a second [verify]) and lives its life expectancy, its
//! size, colour and alpha going from their start through their middle (at
//! its percent mid, else half way) to their end, its frame from the start
//! frame to the end one in the texture's grid: at its frames a second round
//! the run when it has them (the turret's tumbling asteroids), else once
//! over its life (reone), from a random frame when its header says so
//! (0x20). A life expectancy of −1 is for ever: its size, colour and alpha
//! go round with its frames' run at the emitter's frames a second, else
//! stay at their start (reone; Manaan's birds flapping). An emitter whose
//! header says its particles go with it (0x40)
//! keeps them in its own space, so a swoop's flames go with it; the rest
//! stay where they were born, with the emitter's velocity when the header
//! says so (0x80), bouncing off what their way meets (`Surfaces`) by
//! their bounce coefficient when it says so (0x10). Drawn as quads facing the camera (`Billboard_to_World_Z`
//! flat on the ground, `Billboard_to_Local_Z` on the emitter's x and y,
//! `Motion_Blur` the same drawn five times as long, as reone draws it),
//! added by their texture's alpha (`Lighten`) or blended by it; an emitter
//! with a chunk draws that model for each particle instead (reone draws
//! none). A view's
//! look, not the sim's: its randomness is the view's own.

use std::collections::BTreeMap;

use asset_odyssey::anim::AnimationIr;
use asset_odyssey::model::ModelIr;
use asset_odyssey::texture::{Blending, TextureIr};
use bevy::camera::visibility::NoFrustumCulling;
use bevy::math::{Mat4, Quat, Vec2, Vec3, Vec4};
use bevy::prelude::{
    Assets, Children, Commands, Entity, Handle, Image, Mesh, Mesh3d, MeshMaterial3d, Query,
    Transform, Visibility,
};
use fmt_odyssey::mdl::{
    self, CONTROLLER_ALPHA as ALPHA, CONTROLLER_SCALE as SCALE,
    CONTROLLER_SELF_ILLUM as SELF_ILLUM, Node, emitter_controller as emit,
};
use vfs::resolver::Resolver;

use crate::heat_haze::HeatHazeMaterial;
use crate::material::{self, AreaLight, Lighting, OdysseyMaterial};
#[cfg(feature = "probes")]
use crate::probe_kit::{Checks, check};
use crate::scene::Stores;

/// Emitter header flags (as reone reads them; KotorBlender `p2p`,
/// `p2p_sel`).
const P2P: u32 = 0x1;
const P2P_BEZIER: u32 = 0x2;
/// Moved by the area's wind (`fx_rain`'s and `fx_snow`'s emitters).
const WIND: u32 = 0x4;
const BOUNCE: u32 = 0x10;
const RANDOM: u32 = 0x20;
const INHERIT: u32 = 0x40;
const INHERIT_VELOCITY: u32 = 0x80;
/// The model classification bit the executable draws as a distortion (its
/// gob lists' test, `[model+0x6c] & 2`).
const DISTORTION_CLASS: u8 = 2;

/// The most particles one emitter keeps, whatever its stream holds: the
/// executable has no limit, this guards a frame (the densest stream, the
/// Dxun tomb's, holds about 1200).
const PARTICLES_MOST: usize = 4096;

/// How many particles an emitter can have alive at once: a stream of
/// `birthrate` (and up to `random_birthrate` more) a second, each living
/// `life` seconds, and the one being born; at most `PARTICLES_MOST`. TSL's
/// executable allocates particles as they are born and has no limit, so this
/// is the count a stream holds; a stream whose particles never die (a
/// negative life) holds as many as the guard lets it. (The executable's
/// frame-rate-dependent birth count, which drops the fraction of a step's
/// births, is not copied.)
pub(crate) fn particle_limit(birthrate: f32, random_birthrate: f32, life: f32) -> usize {
    let births = birthrate.max(0.0) + random_birthrate.max(0.0);
    if life < 0.0 && births > 0.0 {
        return PARTICLES_MOST;
    }
    ((births * life.max(0.0)).ceil() as usize + 1).min(PARTICLES_MOST)
}
/// A `Motion_Blur` particle riding with its emitter is drawn from where
/// the emitter was this long ago (seconds) when it moves faster than
/// `STREAK_SPEED_SQ` (metres a second, squared): the executable keeps a
/// tail [verify its rule for such a particle]; reone draws it five times
/// as long. Tuned on the bolts: a rifle's about 0.7 m.
const STREAK_S: f32 = 0.012;
const STREAK_SPEED_SQ: f32 = 1.0;
/// An emitter's size is in centimetres.
const SIZE_SCALE: f32 = 0.01;
/// A particle falls at its emitter's `mass` times this (metres a second
/// squared): the executable's step adds `mass × −9.81 × dt` to its fall
/// (K1 Mac build, the emitters' update functions).
const GRAVITY: f32 = 9.81;
/// A bouncing particle that meets a surface is put this far off it
/// (metres), and one left slower than `REST_SPEED_SQ` (metres a second,
/// squared) lies on it `REST_OFF` off, still, for the rest of its life (the
/// executable's emitter step).
const BOUNCE_OFF: f32 = 0.005;
const REST_OFF: f32 = 0.02;
const REST_SPEED_SQ: f32 = 0.5;

/// What a bouncing particle's way from one place to the next meets (game
/// space, metres): where, and the surface's normal there. The executable
/// traces it through the scene: the rooms first, then every model's
/// meshes but the emitter's own.
pub type Surfaces<'a> = &'a dyn Fn(Vec3, Vec3) -> Option<(Vec3, Vec3)>;

/// A level floor at `height`: what a way crossing it down meets.
pub fn floor(height: f32) -> impl Fn(Vec3, Vec3) -> Option<(Vec3, Vec3)> {
    move |from: Vec3, to: Vec3| {
        (from.z >= height && to.z < height).then(|| {
            let t = (from.z - height) / (from.z - to.z);
            (from.lerp(to, t), Vec3::Z)
        })
    }
}

/// Keys by time: one value per row (a colour's three).
type Keys = Vec<(f32, Vec<f32>)>;

/// What particles draw: places, texture coordinates and colours, with the
/// triangles' indices for chunks (quads take theirs in fours).
#[derive(Debug, Clone, Default)]
pub struct Shape {
    pub positions: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub colours: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
}

/// A chunk model's triangles in its own space, and its texture.
#[derive(Debug, Clone, Default)]
pub struct Chunk {
    pub positions: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    pub texture: String,
}

/// A node's alpha, self-illumination and scale keys in one clip.
#[derive(Debug, Clone, Default)]
pub struct Surface {
    pub alpha: Vec<(f32, f32)>,
    pub illum: Vec<(f32, [f32; 3])>,
    pub scale: Vec<(f32, f32)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Update {
    Fountain,
    Single,
    Explosion,
    /// Bolts from the emitter to its reference node (its child), made anew
    /// every `lightningDelay`.
    Lightning,
    /// The rest: not drawn.
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Facing {
    Camera,
    WorldZ,
    LocalZ,
    /// On the emitter's x and y, drawn long along y (reone's stretch).
    MotionBlur,
    /// Upright: turned about world z to the camera's side, its up world z
    /// (up, or down when the camera is upside down).
    AlignedWorldZ,
    /// Long along where it is going, its side square to that.
    AlignedDirection,
}

/// How a point-to-point emitter's particles reach its reference node: on a
/// curve from where each was born over its life, or pulled by `grav`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum P2p {
    Bezier,
    Gravity,
}

/// The header's blend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blend {
    Normal,
    Lighten,
    PunchThrough,
}

/// An emitter of the model: its node, how it gives birth and is drawn, its
/// own values and those each clip keys.
#[derive(Debug, Clone)]
pub struct Emitter {
    pub node: String,
    pub update: Update,
    pub facing: Facing,
    pub blend: Blend,
    pub texture: String,
    /// The model drawn for each particle, if not a quad.
    pub chunk: Option<String>,
    pub grid: [u32; 2],
    pub looping: bool,
    /// Its particles stay where they are born, not in its space.
    pub world: bool,
    /// Born with its velocity.
    pub inherit_velocity: bool,
    /// The area's wind moves its particles (`WeatherSpray`).
    pub wind: bool,
    /// Its particles bounce off what they meet, by its bounce coefficient.
    pub bounce: bool,
    /// Each particle starts at a random frame of its run.
    pub random_frame: bool,
    /// Its particles go to its reference node (its child), and how.
    pub p2p: Option<P2p>,
    /// Its model is of the distortion class (`distorts`).
    distort: bool,
    rest: BTreeMap<u32, Vec<f32>>,
    clips: BTreeMap<String, BTreeMap<u32, Keys>>,
}

impl Emitter {
    /// Its birth rate and random birth rate at rest, and the most any of
    /// its clips keys its birth rate to (`None` where none keys it).
    pub fn birth_rates(&self) -> (f32, f32, Option<f32>) {
        let rest = |kind: u32| {
            (self.rest.get(&kind))
                .and_then(|row| row.first().copied())
                .unwrap_or(0.0)
        };
        let keyed = (self.clips.values())
            .filter_map(|clip| clip.get(&emit::BIRTHRATE))
            .flatten()
            .filter_map(|(_, row)| row.first().copied())
            .reduce(f32::max);
        (rest(emit::BIRTHRATE), rest(emit::RANDOM_BIRTHRATE), keyed)
    }

    /// The most particles it can have alive: what its stream holds at the
    /// most its rest values and clips give it (`particle_limit`); an
    /// explosion keeps at least one whole burst, its birthrate (a burst's
    /// count, 1 if none is set, as `Particles::step` has it).
    pub(crate) fn limit(&self) -> usize {
        let peak = |kind: u32, default: f32| {
            let keyed = self
                .clips
                .values()
                .filter_map(|clip| clip.get(&kind))
                .flatten()
                .filter_map(|(_, row)| row.first().copied());
            let rest = self.rest.get(&kind).and_then(|row| row.first().copied());
            keyed.chain(rest.or(Some(default))).fold(f32::MIN, f32::max)
        };
        let bursts = self.update == Update::Explosion;
        let births = peak(emit::BIRTHRATE, if bursts { 1.0 } else { 0.0 });
        let stream = particle_limit(
            births,
            peak(emit::RANDOM_BIRTHRATE, 0.0),
            peak(emit::LIFE_EXP, 1.0),
        );
        if bursts {
            stream.max((births.max(1.0).ceil() as usize).min(PARTICLES_MOST))
        } else {
            stream
        }
    }

    /// A heat haze: its model is the executable's distortion class
    /// (classification 2: Force Push's, Whirlwind's and Wave's ripples, the
    /// second ion ray's, the swoop's `lmg_distort`), whose nodes it draws
    /// into the stencil alone, its particles where their alpha passes the
    /// test, then the frame over them displaced (`heat_haze`). A punch-through
    /// emitter of any other model is drawn whole.
    pub fn distorts(&self) -> bool {
        self.distort
    }
}

/// What a model's clips key beyond places and turns.
#[derive(Debug, Clone, Default)]
pub struct ModelFx {
    /// By clip (lowercase), by node name (lowercase).
    pub surfaces: BTreeMap<String, BTreeMap<String, Surface>>,
    pub emitters: Vec<Emitter>,
    /// What its clips key on its light nodes (colour, radius,
    /// multiplier), by clip (lowercase), by node name (lowercase).
    pub lights: BTreeMap<String, BTreeMap<String, Vec<(u32, Keys)>>>,
    /// Its light nodes' names (lowercase).
    light_nodes: Vec<String>,
    /// The clips read so far (lowercase): a model's own come before its
    /// supermodels' of the same name (`add_clips`).
    clips_read: std::collections::BTreeSet<String>,
}

/// A controller's rows: (time, values), Bézier handles left out.
pub(crate) fn rows(controller: &mdl::Controller) -> Keys {
    let columns = usize::from(controller.columns);
    let stride = if controller.bezier {
        columns * 3
    } else {
        columns
    };
    controller
        .times
        .iter()
        .enumerate()
        .filter_map(|(row, &time)| {
            let at = row * stride;
            Some((time, controller.values.get(at..at + columns)?.to_vec()))
        })
        .collect()
}

fn walk<'a>(node: &'a Node, out: &mut Vec<&'a Node>) {
    out.push(node);
    for child in &node.children {
        walk(child, out);
    }
}

fn update(name: &str) -> Update {
    match name.trim().to_ascii_lowercase().as_str() {
        "fountain" => Update::Fountain,
        "single" => Update::Single,
        "explosion" => Update::Explosion,
        "lightning" => Update::Lightning,
        _ => Update::Other,
    }
}

fn facing(render: &str) -> Facing {
    match render.trim().to_ascii_lowercase().as_str() {
        "billboard_to_world_z" => Facing::WorldZ,
        "billboard_to_local_z" => Facing::LocalZ,
        "motion_blur" => Facing::MotionBlur,
        "aligned_to_world_z" => Facing::AlignedWorldZ,
        "aligned_to_particle_dir" => Facing::AlignedDirection,
        _ => Facing::Camera,
    }
}

fn blend(name: &str) -> Blend {
    match name.trim().to_ascii_lowercase().as_str() {
        "lighten" => Blend::Lighten,
        "punch-through" | "punchthrough" => Blend::PunchThrough,
        _ => Blend::Normal,
    }
}

impl ModelFx {
    /// What `model`'s clips key beyond places and turns.
    pub fn read(model: &mdl::Model) -> ModelFx {
        let mut fx = ModelFx::default();
        let mut nodes = Vec::new();
        walk(&model.root, &mut nodes);
        for node in &nodes {
            let Some(header) = &node.emitter else {
                continue;
            };
            let chunk = header.chunk.trim().to_ascii_lowercase();
            fx.emitters.push(Emitter {
                node: node.name.to_ascii_lowercase(),
                update: update(&header.update),
                facing: facing(&header.render),
                blend: blend(&header.blend),
                texture: header.texture.trim().to_ascii_lowercase(),
                chunk: (!chunk.is_empty() && chunk != "null").then_some(chunk),
                grid: header.grid.map(|g| g.max(1)),
                looping: header.looped,
                world: header.flags & INHERIT == 0,
                inherit_velocity: header.flags & INHERIT_VELOCITY != 0,
                wind: header.flags & WIND != 0,
                bounce: header.flags & BOUNCE != 0,
                random_frame: header.flags & RANDOM != 0,
                p2p: (header.flags & P2P != 0).then_some(if header.flags & P2P_BEZIER != 0 {
                    P2p::Bezier
                } else {
                    P2p::Gravity
                }),
                distort: model.classification & DISTORTION_CLASS != 0,
                rest: node
                    .controllers
                    .iter()
                    .filter_map(|c| Some((c.kind, rows(c).into_iter().next()?.1)))
                    .collect(),
                clips: BTreeMap::new(),
            });
        }
        fx.light_nodes = (nodes.iter())
            .filter(|node| node.light.is_some())
            .map(|node| node.name.to_ascii_lowercase())
            .collect();
        fx.add_clips(model);
        fx
    }

    /// What the clips of `model` (the model itself, or a supermodel of it)
    /// key on this model's nodes, for those not read yet: a model's own
    /// clip wins over its supermodel's of the same name, as the lookup by
    /// name finds it first.
    pub fn add_clips(&mut self, model: &mdl::Model) {
        let fx = self;
        for clip in &model.animations {
            let name = clip.name.to_ascii_lowercase();
            if !fx.clips_read.insert(name.clone()) {
                continue;
            }
            let mut keyed = Vec::new();
            walk(&clip.root, &mut keyed);
            for node in keyed {
                let node_name = node.name.to_ascii_lowercase();
                let keys = || -> Vec<(u32, Keys)> {
                    node.controllers
                        .iter()
                        .map(|c| (c.kind, rows(c)))
                        .filter(|(_, keys)| !keys.is_empty())
                        .collect()
                };
                if fx.light_nodes.contains(&node_name) {
                    let keys = keys();
                    if !keys.is_empty() {
                        fx.lights
                            .entry(name.clone())
                            .or_default()
                            .insert(node_name, keys);
                    }
                    continue;
                }
                if let Some(emitter) = fx.emitters.iter_mut().find(|e| e.node == node_name) {
                    emitter
                        .clips
                        .insert(name.clone(), keys().into_iter().collect());
                    continue;
                }
                let mut surface = Surface::default();
                for controller in &node.controllers {
                    match (controller.kind, controller.columns) {
                        (ALPHA, 1) => {
                            surface.alpha = rows(controller)
                                .into_iter()
                                .map(|(t, v)| (t, v[0]))
                                .collect()
                        }
                        (SELF_ILLUM, 3) => {
                            surface.illum = rows(controller)
                                .into_iter()
                                .map(|(t, v)| (t, [v[0], v[1], v[2]]))
                                .collect()
                        }
                        (SCALE, 1) => {
                            surface.scale = rows(controller)
                                .into_iter()
                                .map(|(t, v)| (t, v[0]))
                                .collect()
                        }
                        _ => {}
                    }
                }
                if !surface.alpha.is_empty()
                    || !surface.illum.is_empty()
                    || !surface.scale.is_empty()
                {
                    fx.surfaces
                        .entry(name.clone())
                        .or_default()
                        .insert(node_name, surface);
                }
            }
        }
    }

    /// What clip `clip` keys controller `kind` of light node `node` to at
    /// `at` seconds into it (linear between its keys), if it keys it.
    pub fn light_key(&self, (clip, node): (&str, &str), kind: u32, at: f32) -> Option<Vec<f32>> {
        let (_, keys) = (self.lights.get(clip)?.get(node)?.iter()).find(|(k, _)| *k == kind)?;
        // Linear between keys, held at the ends (`sample`).
        let next = keys.iter().position(|(time, _)| *time > at);
        let (a, b) = match next {
            None => (keys.last()?, keys.last()?),
            Some(0) => (keys.first()?, keys.first()?),
            Some(next) => (&keys[next - 1], &keys[next]),
        };
        let t = ((at - a.0) / (b.0 - a.0).max(f32::EPSILON)).clamp(0.0, 1.0);
        Some(a.1.iter().zip(&b.1).map(|(a, b)| a + (b - a) * t).collect())
    }
}

/// Linear between keys, held at the ends.
fn sample<T: Copy>(keys: &[(f32, T)], t: f32, mix: impl Fn(T, T, f32) -> T) -> Option<T> {
    let (first, last) = (keys.first()?, keys.last()?);
    if t <= first.0 {
        return Some(first.1);
    }
    if t >= last.0 {
        return Some(last.1);
    }
    let next = keys.iter().position(|(time, _)| *time > t)?;
    let (a, b) = (keys[next - 1], keys[next]);
    Some(mix(a.1, b.1, (t - a.0) / (b.0 - a.0).max(f32::EPSILON)))
}

pub fn sample_alpha(keys: &[(f32, f32)], t: f32) -> Option<f32> {
    sample(keys, t, |a, b, f| a + (b - a) * f)
}

pub fn sample_illum(keys: &[(f32, [f32; 3])], t: f32) -> Option<[f32; 3]> {
    sample(keys, t, |a, b, f| {
        [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * f)
    })
}

/// An emitter's values: its own, as the clips playing keyed them last.
#[derive(Debug, Clone, Default)]
pub struct Values(BTreeMap<u32, Vec<f32>>);

impl Values {
    pub fn of(emitter: &Emitter) -> Values {
        Values(emitter.rest.clone())
    }

    /// Clip `clip` at `at` seconds over these.
    pub fn key(&mut self, emitter: &Emitter, clip: &str, at: f32) {
        let Some(keys) = emitter.clips.get(clip) else {
            return;
        };
        for (kind, keys) in keys {
            let mix = |a: &Vec<f32>, b: &Vec<f32>, f: f32| -> Vec<f32> {
                a.iter().zip(b).map(|(a, b)| a + (b - a) * f).collect()
            };
            let (first, last) = (&keys[0], &keys[keys.len() - 1]);
            let value = if at <= first.0 {
                first.1.clone()
            } else if at >= last.0 {
                last.1.clone()
            } else {
                let next = keys
                    .iter()
                    .position(|(time, _)| *time > at)
                    .unwrap_or(keys.len() - 1);
                let (a, b) = (&keys[next - 1], &keys[next]);
                mix(&a.1, &b.1, (at - a.0) / (b.0 - a.0).max(f32::EPSILON))
            };
            self.0.insert(*kind, value);
        }
    }

    fn get(&self, kind: u32) -> Option<f32> {
        self.0.get(&kind).and_then(|v| v.first().copied())
    }

    fn or(&self, kind: u32, default: f32) -> f32 {
        self.get(kind).unwrap_or(default)
    }

    /// Its birthrate now (the dumps).
    pub fn birthrate(&self) -> f32 {
        self.or(emit::BIRTHRATE, 0.0)
    }

    /// These with no births: what a model gone from view leaves (its
    /// particles live out their lives, none born).
    pub fn quiet(&self) -> Values {
        let mut quiet = self.clone();
        for kind in [emit::BIRTHRATE, emit::RANDOM_BIRTHRATE] {
            quiet.0.insert(kind, vec![0.0]);
        }
        quiet
    }

    fn colour(&self, kind: u32) -> Option<Vec3> {
        let v = self.0.get(&kind)?;
        (v.len() >= 3).then(|| Vec3::new(v[0], v[1], v[2]))
    }

    /// From start through middle to end at `f` of a life; a missing start
    /// is 1, a missing end the start.
    fn three(&self, [start, middle, end]: [u32; 3], f: f32) -> f32 {
        let mid = self.or(emit::PERCENT_MID, 0.5).clamp(0.01, 0.99);
        let start = self.get(start).unwrap_or(1.0);
        let end = self.get(end).unwrap_or(start);
        match self.get(middle) {
            Some(middle) if f < mid => start + (middle - start) * f / mid,
            Some(middle) => middle + (end - middle) * (f - mid) / (1.0 - mid),
            None => start + (end - start) * f,
        }
    }

    /// How far through its life a particle `age` seconds old is: through
    /// its frames over and over for one that lives for ever.
    fn share(&self, age: f32) -> f32 {
        let life = self.or(emit::LIFE_EXP, 1.0);
        if life >= 0.0 {
            return (age / life.max(f32::EPSILON)).clamp(0.0, 1.0);
        }
        let start = self.or(emit::FRAME_START, 0.0);
        let frames = (self.or(emit::FRAME_END, start) - start).abs().floor() + 1.0;
        let length = frames / self.or(emit::FPS, 0.0);
        if length.is_finite() && length > 0.0 {
            (age % length) / length
        } else {
            0.0
        }
    }

    fn colour_at(&self, f: f32) -> Vec3 {
        let mid = self.or(emit::PERCENT_MID, 0.5).clamp(0.01, 0.99);
        let start = self.colour(emit::COLOR_START).unwrap_or(Vec3::ONE);
        let end = self.colour(emit::COLOR_END).unwrap_or(start);
        match self.colour(emit::COLOR_MID) {
            Some(middle) if f < mid => start.lerp(middle, f / mid),
            Some(middle) => middle.lerp(end, (f - mid) / (1.0 - mid)),
            None => start.lerp(end, f),
        }
    }
}

/// Where a point-to-point particle is `t` of its life on, from where it was
/// born to `to`: a cubic curve whose inner points stand off the ends along
/// the emitter's z by `p2p_bezier2` and `p2p_bezier3` of the way's length
/// [verify against the game's curve].
fn bezier(from: Vec3, to: Vec3, along: Vec3, (out, back): (f32, f32), t: f32) -> Vec3 {
    let reach = from.distance(to);
    let (a, b) = (from + along * reach * out, to + along * reach * back);
    let s = 1.0 - t;
    from * (s * s * s) + a * (3.0 * s * s * t) + b * (3.0 * s * t * t) + to * (t * t * t)
}

/// `particle`'s way on from `before`, if it met a surface: turned back off
/// it, slowed by `bounce`, put `BOUNCE_OFF` out; at rest there once slower
/// than `REST_SPEED_SQ` (the executable's emitter step).
fn bounce_off(particle: &mut Particle, before: Vec3, surfaces: Surfaces, bounce: f32) {
    let Some((met, normal)) = surfaces(before, particle.at) else {
        return;
    };
    let v = particle.velocity;
    particle.velocity = (v - 2.0 * v.dot(normal) * normal) * bounce;
    particle.at = met + normal * BOUNCE_OFF;
    if particle.velocity.length_squared() < REST_SPEED_SQ {
        particle.rest = true;
        particle.velocity = Vec3::ZERO;
        particle.at = met + normal * REST_OFF;
    }
}

/// A particle, in its emitter's space or the world's (metres, seconds).
#[derive(Debug, Clone, Copy)]
struct Particle {
    at: Vec3,
    velocity: Vec3,
    age: f32,
    /// Turned this far (radians), at this rate, about this axis (a chunk's;
    /// a quad turns in its plane).
    turned: f32,
    spin: f32,
    axis: Vec3,
    /// Its frame run's start (a random one's).
    first: f32,
    /// Where it was born.
    from: Vec3,
    /// A `Motion_Blur` particle's tail: a point that follows it, closing
    /// the share `min(dt, blurLength) / blurLength` of the gap each step,
    /// as the executable keeps one; the streak is drawn from it.
    trail: Vec3,
    /// It came to rest on a surface: it lies there, still.
    rest: bool,
}

/// An emitter's particles, the births its fountain owes (`fountain_births`), and where it was; a
/// lightning emitter's bolts (segments in its space) and how long until it
/// makes them anew.
#[derive(Debug, Clone)]
pub struct Particles {
    list: Vec<Particle>,
    owed: f32,
    born_single: bool,
    seed: u32,
    limit: usize,
    was: Option<Vec3>,
    /// How fast the emitter moved over its last step (its space's units a
    /// second): a streak riding with it is drawn along it.
    moving: Vec3,
    bolts: Vec<(Vec3, Vec3)>,
    next_bolt: f32,
}

impl Particles {
    /// How many are alive (the dumps).
    pub fn live(&self) -> usize {
        self.list.len() + self.bolts.len()
    }

    /// None alive, its rolls begun at `seed`, keeping `limit` at the most.
    pub fn new(seed: u32, limit: usize) -> Particles {
        Particles {
            list: Vec::new(),
            owed: 0.0,
            born_single: false,
            seed: seed.max(1),
            limit,
            was: None,
            moving: Vec3::ZERO,
            bolts: Vec::new(),
            next_bolt: 0.0,
        }
    }

    /// None alive and none owed, as new (its rolls go on).
    pub fn clear(&mut self) {
        *self = Particles::new(self.seed, self.limit);
    }

    /// In [-1, 1).
    fn random(&mut self) -> f32 {
        let mut x = self.seed;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.seed = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }

    /// How many a fountain bears `dt` seconds on. A birth rate under one
    /// a second bears nothing, whatever its random birth rate; a higher
    /// one is moved, each step, by a whole number below the random birth
    /// rate, up or down (not below 0), and its births are owed by the
    /// second, the fraction kept for the next step (QUIRKS Q-0279: the
    /// executable's count follows the frame rate instead).
    fn fountain_births(&mut self, values: &Values, dt: f32) -> u32 {
        let rate = values.or(emit::BIRTHRATE, 0.0);
        if rate < 1.0 {
            return 0;
        }
        let spread = values.or(emit::RANDOM_BIRTHRATE, 0.0).trunc() as i32;
        let now = if spread > 0 {
            let by = ((self.random() * 0.5 + 0.5) * spread as f32) as i32 % spread;
            let by = if self.random() >= 0.0 { by } else { -by };
            (rate + by as f32).max(0.0)
        } else {
            rate
        };
        self.owed += now * dt;
        let count = self.owed.floor();
        self.owed -= count;
        count as u32
    }

    fn born(&mut self, emitter: &Emitter, values: &Values, space: Mat4, moving: Vec3) {
        if self.list.len() >= self.limit {
            return;
        }
        let (w, h) = (
            values.or(emit::X_SIZE, 0.0) * SIZE_SCALE / 2.0,
            values.or(emit::Y_SIZE, 0.0) * SIZE_SCALE / 2.0,
        );
        let at = Vec3::new(self.random() * w, self.random() * h, 0.0);
        // Turned off its z by up to half its spread, then anywhere round
        // it; as fast as its velocity, up or down by up to its random one
        // (the executable's birth).
        let tilt = (self.random() * 0.5 + 0.5) * values.or(emit::SPREAD, 0.0) / 2.0;
        let round = (self.random() * 0.5 + 0.5) * std::f32::consts::TAU;
        let way = Quat::from_rotation_z(round) * Quat::from_rotation_x(tilt) * Vec3::Z;
        let speed = values.or(emit::VELOCITY, 0.0) + self.random() * values.or(emit::RAND_VEL, 0.0);
        let (at, velocity) = if emitter.world {
            let way = space.transform_vector3(way).normalize_or_zero();
            let inherited = if emitter.inherit_velocity {
                moving
            } else {
                Vec3::ZERO
            };
            (space.transform_point3(at), way * speed + inherited)
        } else {
            (at, way * speed)
        };
        // A spinning one starts turned anywhere; a still one square on.
        let spin = values.or(emit::PARTICLE_ROT, 0.0);
        let turned = if spin == 0.0 {
            0.0
        } else {
            self.random() * std::f32::consts::PI
        };
        let axis = Vec3::new(self.random(), self.random(), self.random()).normalize_or(Vec3::Z);
        let first = if emitter.random_frame {
            (self.random() * 0.5 + 0.5) * 4096.0
        } else {
            0.0
        };
        self.list.push(Particle {
            at,
            velocity,
            age: 0.0,
            turned,
            spin,
            axis,
            first,
            from: at,
            trail: at,
            rest: false,
        });
    }

    /// `dt` seconds on, with `values`; `detonated` when a clip passed a
    /// `detonate` event; `space` the emitter's transform and `surfaces`
    /// what its particles' ways meet (game space); `target` a
    /// point-to-point emitter's reference node (in its space).
    pub fn step(
        &mut self,
        emitter: &Emitter,
        values: &Values,
        (dt, detonated): (f32, bool),
        (space, surfaces): (Mat4, Option<Surfaces>),
        target: Option<Vec3>,
    ) {
        let here = space.w_axis.truncate();
        let moving = match self.was.replace(here) {
            Some(was) if dt > 0.0 => (here - was) / dt,
            Some(_) => self.moving,
            None => Vec3::ZERO,
        };
        self.moving = moving;
        let life = values.or(emit::LIFE_EXP, 1.0);
        let fall = values.or(emit::MASS, 0.0) * GRAVITY;
        let down = if emitter.world {
            Vec3::NEG_Z
        } else {
            space
                .inverse()
                .transform_vector3(Vec3::NEG_Z)
                .normalize_or_zero()
        };
        // Bouncing off what they meet: those kept where they were born.
        let surfaces = surfaces.filter(|_| emitter.bounce && emitter.world);
        let bounce = values.or(emit::BOUNCE_CO, 0.0);
        // A point-to-point emitter's particles head for its reference node
        // (the beams' flame spray and rings), leaving along its z.
        let (goal, along) = match target {
            Some(target) if emitter.world => (
                Some(space.transform_point3(target)),
                space.transform_vector3(Vec3::Z).normalize_or_zero(),
            ),
            target => (target, Vec3::Z),
        };
        let pull = values.or(emit::GRAV, 0.0);
        let blur = values.or(emit::BLUR_LENGTH, 0.0);
        let follows = match blur > 0.0 {
            true => dt.min(blur) / blur,
            false => 1.0,
        };
        let bends = (
            values.or(emit::P2P_BEZIER2, 0.0),
            values.or(emit::P2P_BEZIER3, 0.0),
        );
        for particle in &mut self.list {
            particle.age += dt;
            let before = particle.at;
            match (emitter.p2p, goal) {
                (Some(P2p::Bezier), Some(goal)) => {
                    let t = if life > 0.0 {
                        (particle.age / life).min(1.0)
                    } else {
                        0.0
                    };
                    particle.at = bezier(particle.from, goal, along, bends, t);
                    particle.turned += particle.spin * dt;
                    continue;
                }
                (Some(P2p::Gravity), Some(goal)) => {
                    particle.velocity += (goal - particle.at).normalize_or_zero() * pull * dt;
                    particle.at += particle.velocity * dt;
                }
                _ if particle.rest => {}
                // Moved, then falling faster (the executable's order).
                _ => {
                    particle.at += particle.velocity * dt;
                    particle.velocity += down * fall * dt;
                }
            }
            particle.turned += particle.spin * dt;
            if let Some(surfaces) = surfaces
                && !particle.rest
            {
                bounce_off(particle, before, surfaces, bounce);
            }
            if emitter.facing == Facing::MotionBlur {
                particle.trail += (particle.at - particle.trail) * follows;
            }
        }
        if life >= 0.0 {
            self.list.retain(|particle| particle.age < life);
        }
        match emitter.update {
            Update::Fountain => {
                for _ in 0..self.fountain_births(values, dt) {
                    self.born(emitter, values, space, moving);
                }
            }
            Update::Single => {
                if !self.born_single || (self.list.is_empty() && emitter.looping) {
                    self.born_single = true;
                    self.born(emitter, values, space, moving);
                }
            }
            Update::Explosion if detonated => {
                let count = values.or(emit::BIRTHRATE, 1.0).max(1.0) as usize;
                for _ in 0..count {
                    self.born(emitter, values, space, moving);
                }
            }
            _ => {}
        }
    }

    /// A weather emitter's particles `dt` seconds on (EXE-NOTES
    /// `WEATHER_HOLD`, `WEATHER_WIND`): pushed by the wind when the area's
    /// wind moves them, then each folded into the square round the camera
    /// (`weather::wrapped`). Only those kept where they were born.
    fn hold(&mut self, emitter: &Emitter, weather: WeatherSpray, dt: f32) {
        if !emitter.world {
            return;
        }
        let push = if emitter.wind {
            weather.wind.extend(0.0) * dt
        } else {
            Vec3::ZERO
        };
        for particle in &mut self.list {
            particle.at = crate::weather::wrapped(particle.at + push, weather.centre);
        }
    }

    /// A lightning emitter `dt` seconds on: every `lightningDelay` its bolt
    /// made anew (reone's reading), from a random point of the emitter's
    /// size to `target` (its reference node, in its space) in
    /// `lightningSubDiv` + 1 segments, each strayed across by up to half the
    /// `lightningRadius`; none without a target or births.
    pub fn lightning(&mut self, values: &Values, dt: f32, target: Option<Vec3>, births: bool) {
        let Some(target) = target.filter(|_| births) else {
            self.bolts.clear();
            return;
        };
        self.next_bolt -= dt;
        if self.next_bolt > 0.0 && !self.bolts.is_empty() {
            return;
        }
        self.next_bolt = values.or(emit::LIGHTNING_DELAY, 0.0).max(1.0 / 60.0);
        let (w, h) = (
            values.or(emit::X_SIZE, 0.0) * SIZE_SCALE / 2.0,
            values.or(emit::Y_SIZE, 0.0) * SIZE_SCALE / 2.0,
        );
        let from = Vec3::new(self.random() * w, self.random() * h, 0.0);
        let parts = values.or(emit::LIGHTNING_SUBDIV, 0.0).clamp(0.0, 64.0) as usize + 1;
        let length = (target.z - from.z).abs() / parts as f32;
        // A beam's (no radius of its own) strays by its width [verify].
        let half = match values.or(emit::LIGHTNING_RADIUS, 0.0) {
            0.0 => bolt_width(values) / 2.0,
            radius => radius / 2.0,
        };
        self.bolts.clear();
        let mut at = from;
        for _ in 1..parts {
            let way = (target - at).normalize_or_zero();
            let stray = Vec3::new(self.random() * half, self.random() * half, 0.0);
            let next = at + way * length + stray;
            self.bolts.push((at, next));
            at = next;
        }
        self.bolts.push((at, target));
    }

    /// A lightning emitter's bolts as quads (game space) into `out`: each
    /// segment `lightningScale` wide, long along it, turned to the camera
    /// (`right`, `up`).
    fn bolt_quads(&self, values: &Values, space: Mat4, (right, up): (Vec3, Vec3), out: &mut Shape) {
        let colour = values.colour_at(0.0);
        let alpha = values.or(emit::ALPHA_START, 1.0).clamp(0.0, 1.0);
        let rgba = [colour.x, colour.y, colour.z, alpha];
        let half = bolt_width(values) / 2.0;
        let toward = right.cross(up).normalize_or(Vec3::Z);
        for (from, to) in &self.bolts {
            let (a, b) = (space.transform_point3(*from), space.transform_point3(*to));
            let along = (b - a).normalize_or_zero();
            let across = along.cross(toward).normalize_or(right) * half;
            for (corner, uv) in [
                (a - across, [0.0, 1.0]),
                (a + across, [1.0, 1.0]),
                (b + across, [1.0, 0.0]),
                (b - across, [0.0, 0.0]),
            ] {
                out.positions.push(corner.to_array());
                out.uvs.push(uv);
                out.colours.push(rgba);
            }
        }
    }

    /// Each particle's place (game space), share of its life and colour.
    fn shown<'a>(
        &'a self,
        emitter: &'a Emitter,
        values: &'a Values,
        space: Mat4,
    ) -> impl Iterator<Item = (&'a Particle, Vec3, f32, [f32; 4])> + 'a {
        self.list.iter().map(move |particle| {
            let f = values.share(particle.age);
            let alpha = values.three([emit::ALPHA_START, emit::ALPHA_MID, emit::ALPHA_END], f);
            let colour = values.colour_at(f);
            let centre = if emitter.world {
                particle.at
            } else {
                space.transform_point3(particle.at)
            };
            let rgba = [colour.x, colour.y, colour.z, alpha.clamp(0.0, 1.0)];
            (particle, centre, f, rgba)
        })
    }

    /// The quads to draw (game space) into `out`: `space` the emitter's
    /// transform, `right` and `up` the camera's (game space).
    pub fn quads(
        &self,
        emitter: &Emitter,
        values: &Values,
        space: Mat4,
        (right, up): (Vec3, Vec3),
        out: &mut Shape,
    ) {
        if emitter.update == Update::Lightning {
            self.bolt_quads(values, space, (right, up), out);
            return;
        }
        let (frames_from, frames_to) = (
            values.or(emit::FRAME_START, 0.0),
            values.or(emit::FRAME_END, values.or(emit::FRAME_START, 0.0)),
        );
        let fps = values.or(emit::FPS, 0.0);
        let span = (frames_to - frames_from).abs().floor() + 1.0;
        let [gx, gy] = emitter.grid.map(|g| g.max(1));
        let (axis_x, axis_y) = match emitter.facing {
            Facing::Camera | Facing::AlignedWorldZ | Facing::MotionBlur => (right, up),
            Facing::WorldZ => (Vec3::X, Vec3::Y),
            Facing::LocalZ | Facing::AlignedDirection => (
                space.transform_vector3(Vec3::X).normalize_or(Vec3::X),
                space.transform_vector3(Vec3::Y).normalize_or(Vec3::Y),
            ),
        };
        let upright = if axis_y.z < 0.0 { Vec3::NEG_Z } else { Vec3::Z };
        for (particle, centre, f, rgba) in self.shown(emitter, values, space) {
            let size = values.three([emit::SIZE_START, emit::SIZE_MID, emit::SIZE_END], f) / 2.0;
            // At its frames a second, round its run (the asteroids
            // tumbling); else once over its life (reone).
            // Frames on from its first, the run's way (a run may go down).
            let step = if fps > 0.0 {
                (particle.age * fps + particle.first).floor() % span
            } else {
                ((span - 1.0) * f + particle.first).round() % span
            };
            let way = if frames_to < frames_from { -1.0 } else { 1.0 };
            let frame = (frames_from + way * step).max(0.0) as u32 % (gx * gy);
            let (column, row) = ((frame % gx) as f32, (frame / gx) as f32);
            let (u0, v0) = (column / gx as f32, row / gy as f32);
            let (u1, v1) = (u0 + 1.0 / gx as f32, v0 + 1.0 / gy as f32);
            let (sin, cos) = particle.turned.sin_cos();
            let spun = (axis_x * cos + axis_y * sin) * size;
            let (x, y) = match emitter.facing {
                Facing::AlignedWorldZ => (spun, upright * size),
                Facing::AlignedDirection => {
                    let heading = if emitter.world {
                        particle.velocity
                    } else {
                        space.transform_vector3(particle.velocity)
                    };
                    let along = heading.normalize_or(axis_y);
                    (square_to(along, axis_x, axis_y) * size, along * size)
                }
                _ => (spun, (axis_y * cos - axis_x * sin) * size),
            };
            // A streak faces the camera, long from its tail to it.
            let (tail, centre, x, y) = match emitter.facing {
                Facing::MotionBlur => {
                    // One riding with its emitter streaks along the
                    // emitter's own way (a bolt's `zap`).
                    let tail = match emitter.world {
                        true => particle.trail,
                        false if self.moving.length_squared() > STREAK_SPEED_SQ => {
                            centre - self.moving * STREAK_S
                        }
                        false => space.transform_point3(particle.trail),
                    };
                    let gap = centre - tail;
                    let seen = right * gap.dot(right) + up * gap.dot(up);
                    match seen.length() > f32::EPSILON {
                        true => {
                            let along = seen.normalize();
                            let across = right * along.dot(up) - up * along.dot(right);
                            (tail, centre, across * size, along * size)
                        }
                        false => (centre, centre, x, y),
                    }
                }
                _ => (centre, centre, x, y),
            };
            for (corner, uv) in [
                (tail - x - y, [u0, v1]),
                (tail + x - y, [u1, v1]),
                (centre + x + y, [u1, v0]),
                (centre - x + y, [u0, v0]),
            ] {
                out.positions.push(corner.to_array());
                out.uvs.push(uv);
                out.colours.push(rgba);
            }
        }
    }

    /// Each particle as `chunk`'s triangles (game space) into `out`, turned
    /// about its own axis, at the model's own size [verify].
    pub fn chunks(
        &self,
        emitter: &Emitter,
        values: &Values,
        space: Mat4,
        chunk: &Chunk,
        out: &mut Shape,
    ) {
        for (particle, centre, _, rgba) in self.shown(emitter, values, space) {
            let turn = Quat::from_axis_angle(particle.axis, particle.turned);
            let base = out.positions.len() as u32;
            for (p, uv) in chunk.positions.iter().zip(&chunk.uvs) {
                out.positions
                    .push((centre + turn * Vec3::from_array(*p)).to_array());
                out.uvs.push(*uv);
                out.colours.push(rgba);
            }
            out.indices.extend(chunk.indices.iter().map(|i| base + i));
        }
    }
}

/// The side of a quad long along `along` (a unit vector): `x` turned square
/// to it, else `y`.
fn square_to(along: Vec3, x: Vec3, y: Vec3) -> Vec3 {
    let square = |v: Vec3| (v - along * along.dot(v)).normalize_or_zero();
    let side = square(x);
    if side == Vec3::ZERO { square(y) } else { side }
}

/// How wide a lightning emitter's bolts are drawn (metres): its
/// `lightningScale`, else (the beams', which set none) its particles'
/// starting size [verify].
fn bolt_width(values: &Values) -> f32 {
    match values.or(emit::LIGHTNING_SCALE, 0.0) {
        0.0 => values.or(emit::SIZE_START, 0.0),
        scale => scale,
    }
}

/// The events `clip` passes between `from` and `to` seconds of it, by
/// name: a `to` before `from` is a loop's wrap, the clip's end (its last
/// frame too) to the end and then its start to `to`.
pub fn passed(clip: &AnimationIr, from: f32, to: f32) -> impl Iterator<Item = &str> {
    clip.events
        .iter()
        .filter(move |(at, _)| {
            if to < from {
                (*at >= from && *at <= clip.length) || *at < to
            } else {
                *at >= from && *at < to
            }
        })
        .map(|(_, name)| name.as_str())
}

/// Whether `clip` passes a `detonate` event between `from` and `to`
/// seconds of it (`passed`).
pub fn detonates(clip: &AnimationIr, from: f32, to: f32) -> bool {
    passed(clip, from, to).any(|name| name.eq_ignore_ascii_case("detonate"))
}

/// Whether `clip` passed a `detonate` event in the `dt` seconds up to its
/// moment `at`, `elapsed` seconds after it began: a looping clip whose
/// moment has just come round (`elapsed` past `at`) passes its end first.
pub fn detonates_over(clip: &AnimationIr, (at, elapsed): (f32, f32), dt: f32) -> bool {
    let from = if elapsed > at && at < dt {
        at - dt + clip.length
    } else {
        at - dt.min(at)
    };
    detonates(clip, from, at + f32::EPSILON)
}

/// A mesh whose alpha or self-illumination a clip keys, with a material of
/// its own: its node, and its alpha and self-illumination now (the rest
/// pose's until a clip keys them; then where the last clip left them).
pub struct Lit {
    node: usize,
    material: Handle<OdysseyMaterial>,
    alpha: f32,
    illum: [f32; 3],
}

impl Lit {
    /// Back to its mesh's own alpha and self-illumination (`ir` its model).
    pub fn reset(&mut self, ir: &ModelIr) {
        if let Some(mesh) = ir.meshes.iter().find(|mesh| mesh.node == self.node) {
            (self.alpha, self.illum) = (mesh.alpha, mesh.self_illum);
        }
    }

    /// What `clip` keys on its node `at` seconds in, over what it shows.
    pub fn key(&mut self, (fx, ir): (&ModelFx, &ModelIr), clip: &str, at: f32) {
        let node = ir.nodes[self.node].name.to_ascii_lowercase();
        if let Some(keys) = fx.surfaces.get(clip).and_then(|s| s.get(&node)) {
            self.alpha = sample_alpha(&keys.alpha, at).unwrap_or(self.alpha);
            self.illum = sample_illum(&keys.illum, at).unwrap_or(self.illum);
        }
    }

    /// Its material made to show its alpha and self-illumination.
    pub fn show(&self, materials: &mut Assets<OdysseyMaterial>) {
        let glow = Vec4::new(self.illum[0], self.illum[1], self.illum[2], 1.0);
        if materials
            .get(&self.material)
            .is_some_and(|m| m.params.tint.w != self.alpha || m.params.self_illum != glow)
            && let Some(mut material) = materials.get_mut(&self.material)
        {
            material.params.tint.w = self.alpha;
            material.params.self_illum = glow;
        }
    }
}

/// The nodes of `ir` whose mesh's texture its TXI makes additive
/// (`blending additive`), alpha or not.
pub fn glows(ir: &ModelIr, textures: &BTreeMap<String, TextureIr>) -> Vec<usize> {
    ir.meshes
        .iter()
        .filter(|mesh| {
            mesh.texture
                .as_ref()
                .and_then(|name| textures.get(name))
                .is_some_and(|texture| texture.facts.blending == Blending::Additive)
        })
        .map(|mesh| mesh.node)
        .collect()
}

/// The meshes of a model's nodes (`ir`, spawned as `nodes`) a clip keys
/// alpha or self-illumination on, each given a copy of its material: one
/// keyed alpha drawn blended, or added by its alpha when it is additive or
/// its node one of `glows` (reone draws a texture its TXI makes additive
/// additive with alpha or without; xoreos only without: the swoop's
/// thruster cards, `lmg_jet01` and `02`, are DXT5 with alpha); `None`
/// until the model's meshes are spawned.
pub fn own_materials(
    (fx, ir, nodes): (&ModelFx, &ModelIr, &[Entity]),
    glows: &[usize],
    children: &Query<&Children>,
    mesh_materials: &mut Query<&mut MeshMaterial3d<OdysseyMaterial>>,
    materials: &mut Assets<OdysseyMaterial>,
) -> Option<Vec<Lit>> {
    let mut keyed: BTreeMap<usize, bool> = BTreeMap::new();
    for surfaces in fx.surfaces.values() {
        // A node a clip only scales keeps the materials it has.
        let drawn = surfaces
            .iter()
            .filter(|(_, surface)| !surface.alpha.is_empty() || !surface.illum.is_empty());
        for (node, surface) in drawn {
            if let Some(index) = ir.node_index(node) {
                *keyed.entry(index).or_insert(false) |= !surface.alpha.is_empty();
            }
        }
    }
    let mut lit = Vec::new();
    for (&node, &fades) in &keyed {
        let Some(mesh) = ir.meshes.iter().find(|mesh| mesh.node == node) else {
            continue;
        };
        let meshes: Vec<Entity> = children
            .get(nodes[node])
            .map(|kids| {
                kids.iter()
                    .copied()
                    .filter(|kid| mesh_materials.contains(*kid))
                    .collect()
            })
            .unwrap_or(Vec::new());
        if meshes.is_empty() {
            // Not spawned yet (its texture missing: never drawn).
            continue;
        }
        for entity in meshes {
            let Ok(mut handle) = mesh_materials.get_mut(entity) else {
                continue;
            };
            let mut copy = materials.get(&handle.0).cloned()?;
            if fades && (glows.contains(&node) || copy.surface == material::Surface::Additive) {
                copy = copy.faded_additive();
            } else if fades
                && matches!(
                    copy.surface,
                    material::Surface::Opaque
                        | material::Surface::OpaqueAlpha
                        | material::Surface::OpaqueEnvMasked
                )
            {
                copy.surface = material::Surface::Blend;
            }
            let material = materials.add(copy);
            handle.0 = material.clone();
            lit.push(Lit {
                node,
                material,
                alpha: mesh.alpha,
                illum: mesh.self_illum,
            });
        }
    }
    (!keyed.is_empty() || lit.is_empty()).then_some(lit)
}

/// What a weather emitter's particles keep to (`weather`): the camera's
/// point (game space, x and y) they are folded round, and the area's wind
/// (m/s, game space) on those the wind moves.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeatherSpray {
    pub centre: Vec2,
    pub wind: Vec2,
}

/// An emitter drawn: which of its model's, its node, its values as the
/// clips last keyed them, its particles and the mesh they are drawn in;
/// a weather emitter's, what they keep to.
pub struct Spray {
    pub emitter: usize,
    pub node: Option<usize>,
    /// A lightning or point-to-point emitter's reference node (its child),
    /// where its bolts or particles go, and where that is in its space, as
    /// its drawer last put it.
    pub reference: Option<usize>,
    pub target: Option<Vec3>,
    values: Values,
    particles: Particles,
    mesh: Handle<Mesh>,
    /// Its mesh holds nothing (`fill`): an emitter with nothing to draw
    /// leaves it as it is, rather than sending an empty mesh anew each
    /// frame.
    empty: bool,
    pub entity: Entity,
    pub weather: Option<WeatherSpray>,
}

/// A spray under `parent` for each emitter of a model (`fx`, `ir`, its
/// emitters' `chunks`) that is drawn, their seeds drawn on from `seed`: a
/// heat haze's with its own material, a chunk's with its model's texture
/// blended, the rest with the emitter's texture added or blended.
#[allow(clippy::too_many_arguments)]
pub(crate) fn sprays(
    (fx, ir, chunks): (&ModelFx, &ModelIr, &BTreeMap<String, Chunk>),
    (textures, area): (&BTreeMap<&str, Handle<Image>>, &AreaLight),
    stores: &mut Stores,
    hazes: &mut Assets<HeatHazeMaterial>,
    commands: &mut Commands,
    parent: Entity,
    seed: &mut u32,
) -> Vec<Spray> {
    fx.emitters
        .iter()
        .enumerate()
        .filter(|(_, e)| e.update != Update::Other)
        .map(|(k, e)| {
            let mesh = stores.meshes.add(crate::scene::coloured_quads(
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ));
            let spray = commands
                .spawn((
                    Mesh3d(mesh.clone()),
                    Transform::IDENTITY,
                    Visibility::default(),
                    NoFrustumCulling,
                ))
                .id();
            if e.distorts() {
                let art = textures.get(e.texture.as_str()).cloned();
                commands
                    .entity(spray)
                    .insert(MeshMaterial3d(hazes.add(HeatHazeMaterial::new(art))));
            } else {
                let chunk = e.chunk.as_ref().and_then(|name| chunks.get(name));
                let (texture, surface) = match chunk {
                    Some(chunk) => (chunk.texture.as_str(), material::Surface::BlendVertex),
                    // Added by the texture's alpha: KOTOR's flares and
                    // sparks are white, their shape in their alpha; one
                    // without alpha adds whole.
                    None if e.blend == Blend::Lighten => {
                        (e.texture.as_str(), material::Surface::Lighten)
                    }
                    None => (e.texture.as_str(), material::Surface::BlendVertex),
                };
                let lighting = Lighting {
                    self_illum: [0.0; 3],
                    ambient: [1.0; 3],
                    diffuse: [1.0; 3],
                    alpha: 1.0,
                    is_static: false,
                    is_room: false,
                    fogged: false,
                };
                let made = OdysseyMaterial::new(
                    Vec4::ONE,
                    textures.get(texture).cloned(),
                    None,
                    None,
                    surface,
                    &lighting,
                    area,
                );
                // Punch-through: whole where its alpha is above nothing
                // (the second ion ray's rings), as the executable draws it.
                let material = stores.materials.add(match (chunk, e.blend) {
                    (None, Blend::PunchThrough) => made.punch_through(),
                    _ => made,
                });
                commands.entity(spray).insert(MeshMaterial3d(material));
            }
            commands.entity(parent).add_child(spray);
            *seed = seed.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
            let node = emitter_node(ir, &e.node);
            let reference = node
                .filter(|_| e.update == Update::Lightning || e.p2p.is_some())
                .and_then(|node| ir.nodes.iter().position(|child| child.parent == Some(node)));
            Spray {
                emitter: k,
                node,
                reference,
                target: None,
                values: Values::of(e),
                particles: Particles::new(*seed, e.limit()),
                mesh,
                empty: true,
                entity: spray,
                weather: None,
            }
        })
        .collect()
}

/// The node of `ir`'s emitter `name` (lowercase): the emitter's own, not
/// another node of its name (`fx_rain`'s emitter shares the model's name
/// with the root above it).
pub(crate) fn emitter_node(ir: &ModelIr, name: &str) -> Option<usize> {
    ir.emitters
        .iter()
        .find(|emitter| emitter.name == name)
        .map(|emitter| emitter.node)
        .or_else(|| ir.node_index(name))
}

/// Each lightning or point-to-point spray's target aimed at its reference
/// node: where that node is in its emitter's space (`spaces`, each spray's
/// node's transform), the nodes posed as `local` has them and the model
/// placed by `root`.
pub fn aim(
    sprays: &mut [Spray],
    spaces: &[Mat4],
    (ir, nodes, root): (&ModelIr, &[Entity], Mat4),
    local: impl Fn(Entity) -> Option<Mat4>,
) {
    for (spray, space) in sprays.iter_mut().zip(spaces) {
        spray.target = spray.reference.map(|reference| {
            let at = root * node_matrix((ir, nodes), reference, &local);
            space.inverse().transform_point3(at.w_axis.truncate())
        });
    }
}

impl Spray {
    /// Its emitter's birthrate now (the dumps).
    pub fn birthrate(&self) -> f32 {
        self.values.birthrate()
    }

    /// How many particles it has.
    pub fn particles(&self) -> usize {
        self.particles.live()
    }

    /// What `clip` keys on its emitter `at` seconds in, over its values.
    pub fn key(&mut self, fx: &ModelFx, clip: &str, at: f32) {
        self.values.key(&fx.emitters[self.emitter], clip, at);
    }

    /// Its emitter's own values again and no particles, as new (its rolls
    /// go on).
    pub fn reset(&mut self, fx: &ModelFx) {
        self.values = Values::of(&fx.emitters[self.emitter]);
        self.particles.clear();
    }

    /// Its particles `dt` seconds on (`detonated` when a clip passed a
    /// `detonate` event), `space` its node's transform, meeting
    /// `surfaces`; none born unless `births`.
    pub fn step(
        &mut self,
        fx: &ModelFx,
        (dt, detonated): (f32, bool),
        (space, surfaces): (Mat4, Option<Surfaces>),
        births: bool,
    ) {
        let emitter = &fx.emitters[self.emitter];
        let quiet;
        let values = if births {
            &self.values
        } else {
            quiet = self.values.quiet();
            &quiet
        };
        if emitter.update == Update::Lightning {
            self.particles.lightning(values, dt, self.target, births);
        } else {
            self.particles.step(
                emitter,
                values,
                (dt, detonated),
                (space, surfaces),
                self.target,
            );
            if let Some(weather) = self.weather {
                self.particles.hold(emitter, weather, dt);
            }
        }
    }

    /// Its mesh made of its particles as they are (none unless `shown`),
    /// facing the camera's `right` and `up`, all in its parent's space.
    pub fn fill(
        &mut self,
        (fx, chunks): (&ModelFx, &BTreeMap<String, Chunk>),
        space: Mat4,
        facing: (Vec3, Vec3),
        shown: bool,
        meshes: &mut Assets<Mesh>,
    ) {
        let emitter = &fx.emitters[self.emitter];
        let mut shape = Shape::default();
        if shown {
            match emitter.chunk.as_ref().and_then(|name| chunks.get(name)) {
                Some(chunk) => {
                    self.particles
                        .chunks(emitter, &self.values, space, chunk, &mut shape)
                }
                None => self
                    .particles
                    .quads(emitter, &self.values, space, facing, &mut shape),
            }
        }
        let empty = shape.positions.is_empty();
        if empty && self.empty {
            return;
        }
        self.empty = empty;
        if let Some(mut mesh) = meshes.get_mut(&self.mesh) {
            *mesh = if shape.indices.is_empty() {
                crate::scene::coloured_quads(shape.positions, shape.uvs, shape.colours)
            } else {
                crate::scene::coloured_triangles(
                    shape.positions,
                    shape.uvs,
                    shape.colours,
                    shape.indices,
                )
            };
        }
    }
}

/// A chunk model's textured triangles, in its own space.
fn chunk(resolver: &Resolver, name: &str) -> Option<Chunk> {
    let ir = crate::viewer::model(resolver, name).ok()??;
    let mut chunk = Chunk::default();
    for mesh in &ir.meshes {
        let Some(texture) = &mesh.texture else {
            continue;
        };
        let world = Mat4::from_cols_array_2d(&ir.nodes[mesh.node].world).transpose();
        let base = chunk.positions.len() as u32;
        chunk.positions.extend(
            mesh.positions
                .iter()
                .map(|p| world.transform_point3(Vec3::from_array(*p)).to_array()),
        );
        chunk.uvs.extend(
            (0..mesh.positions.len()).map(|k| mesh.uv1.get(k).copied().unwrap_or([0.0; 2])),
        );
        chunk.indices.extend(mesh.indices.iter().map(|i| base + i));
        if chunk.texture.is_empty() {
            chunk.texture = texture.clone();
        }
    }
    (!chunk.indices.is_empty()).then_some(chunk)
}

impl ModelFx {
    /// Its emitters' chunk models, by name.
    pub fn chunks(&self, resolver: &Resolver) -> BTreeMap<String, Chunk> {
        self.emitters
            .iter()
            .filter_map(|emitter| emitter.chunk.clone())
            .filter_map(|name| Some((name.clone(), chunk(resolver, &name)?)))
            .collect()
    }
}

/// What `fx`'s emitters and their `chunks` draw with.
pub fn texture_names<'a>(
    (fx, chunks): (&'a ModelFx, &'a BTreeMap<String, Chunk>),
) -> impl Iterator<Item = &'a String> {
    fx.emitters
        .iter()
        .map(|e| &e.texture)
        .chain(chunks.values().map(|chunk| &chunk.texture))
}

/// Adds to `textures` what `fx`'s emitters and their `chunks` draw with,
/// where the install has it.
pub fn add_textures(
    resolver: &Resolver,
    (fx, chunks): (&ModelFx, &BTreeMap<String, Chunk>),
    textures: &mut BTreeMap<String, TextureIr>,
) {
    for name in texture_names((fx, chunks)) {
        if !name.is_empty()
            && !textures.contains_key(name)
            && let Some(texture) = crate::viewer::texture(resolver, name)
        {
            textures.insert(name.clone(), texture);
        }
    }
}

/// Node `index`'s transform in its model's space (`ir`, spawned as
/// `nodes`), from the posed transforms (`local`: an entity's).
pub fn node_matrix(
    (ir, nodes): (&ModelIr, &[Entity]),
    index: usize,
    local: impl Fn(Entity) -> Option<Mat4>,
) -> Mat4 {
    let mut matrix = Mat4::IDENTITY;
    let mut at = Some(index);
    while let Some(i) = at {
        matrix = local(nodes[i]).unwrap_or(Mat4::IDENTITY) * matrix;
        at = ir.nodes[i].parent;
    }
    matrix
}

/// The count a stream holds: a downpour's, a small stream's, one with a
/// random birth rate, one with no births (the one being born), one whose
/// particles never die (the guard's count, none if none are born), and the
/// guard on a count past it.
#[cfg(feature = "probes")]
pub fn limit_probes() -> Checks {
    [
        ("limit_rain", 150.0, 0.0, 7.0, 1051),
        ("limit_small", 36.0, 0.0, 2.0, 73),
        ("limit_random_birthrate", 10.0, 5.0, 2.0, 31),
        ("limit_none_born", 0.0, 0.0, 3.0, 1),
        ("limit_endless_life", 4.0, 0.0, -1.0, PARTICLES_MOST),
        ("limit_endless_none_born", 0.0, 0.0, -1.0, 1),
    ]
    .into_iter()
    .map(|(name, rate, random, life, expected)| {
        let got = particle_limit(rate, random, life);
        (name, check(got == expected, format!("{got} of {expected}")))
    })
    .chain([(
        "limit_most",
        check(
            particle_limit(1e6, 0.0, 1e3) == PARTICLES_MOST,
            "capped".into(),
        ),
    )])
    .chain(emitter_limit_probes())
    .chain(facing_probes())
    .chain(detonation_probes())
    .chain(bounce_probes())
    .chain(birth_probes())
    .chain(crate::particle_surfaces::probes())
    .collect()
}

/// A particle at `at` heading `velocity`, stepped `dt` seconds over a floor
/// at 0 by a bouncing world emitter of `bounce` (no fall).
#[cfg(feature = "probes")]
fn bounced(at: Vec3, velocity: Vec3, bounce: f32, dt: f32) -> Particle {
    let mut particle = Particle {
        at,
        velocity,
        age: 0.0,
        turned: 0.0,
        spin: 0.0,
        axis: Vec3::Z,
        first: 0.0,
        from: at,
        trail: at,
        rest: false,
    };
    let before = particle.at;
    particle.at += particle.velocity * dt;
    bounce_off(&mut particle, before, &floor(0.0), bounce);
    particle
}

/// The executable's bounce: turned back off the surface's normal, slowed by
/// the bounce coefficient, `BOUNCE_OFF` out; at rest `REST_OFF` out once
/// slower than `REST_SPEED_SQ`; none for a way that meets nothing.
#[cfg(feature = "probes")]
fn bounce_probes() -> Checks {
    let near = |a: Vec3, b: Vec3| (a - b).length() < 1e-4;
    let fast = bounced(
        Vec3::new(0.0, 0.0, 0.05),
        Vec3::new(1.0, 0.0, -4.0),
        0.3,
        0.1,
    );
    let slow = bounced(
        Vec3::new(0.0, 0.0, 0.05),
        Vec3::new(0.0, 0.0, -1.0),
        0.3,
        0.1,
    );
    let clear = bounced(
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(0.0, 0.0, -1.0),
        0.3,
        0.1,
    );
    vec![
        (
            "bounce_turned_back",
            check(
                !fast.rest
                    && near(fast.velocity, Vec3::new(0.3, 0.0, 1.2))
                    && (fast.at.z - BOUNCE_OFF).abs() < 1e-4,
                format!("{} at {}", fast.velocity, fast.at),
            ),
        ),
        (
            "bounce_comes_to_rest",
            check(
                slow.rest && slow.velocity == Vec3::ZERO && (slow.at.z - REST_OFF).abs() < 1e-4,
                format!("{} at {}", slow.velocity, slow.at),
            ),
        ),
        (
            "bounce_none_in_the_clear",
            check(
                !clear.rest && near(clear.velocity, Vec3::NEG_Z),
                format!("{}", clear.velocity),
            ),
        ),
    ]
}

/// The executable's birth: turned off the emitter's z by up to half its
/// spread (a full turn's reach every way, below too), as fast as its
/// velocity up or down by up to its random one.
#[cfg(feature = "probes")]
fn birth_probes() -> Checks {
    let born = |spread: f32| {
        let rest = [
            (emit::LIFE_EXP, 5.0),
            (emit::VELOCITY, 3.0),
            (emit::RAND_VEL, 2.0),
            (emit::SPREAD, spread),
            (emit::BIRTHRATE, 200.0),
        ];
        let emitter = Emitter {
            world: true,
            ..limit_emitter(Update::Explosion, &rest, &[])
        };
        let mut particles = Particles::new(7, 256);
        let values = Values::of(&emitter);
        particles.step(&emitter, &values, (0.0, true), (Mat4::IDENTITY, None), None);
        particles
            .list
            .iter()
            .map(|p| p.velocity)
            .collect::<Vec<_>>()
    };
    let narrow = born(0.5);
    let tilted = |v: &Vec3| v.normalize().angle_between(Vec3::Z);
    let widest = narrow.iter().map(tilted).fold(0.0, f32::max);
    let full = born(std::f32::consts::TAU);
    let below = full.iter().filter(|v| v.z < 0.0).count();
    let speeds = full.iter().map(|v| v.length());
    let (slowest, fastest) = speeds.fold((f32::MAX, 0.0_f32), |(lo, hi), s| (lo.min(s), hi.max(s)));
    vec![
        (
            "born_within_half_the_spread",
            check(widest <= 0.25 + 1e-4, format!("widest {widest}")),
        ),
        (
            "born_every_way_at_a_full_turn",
            check(below > 0, format!("{below} of {} below", full.len())),
        ),
        (
            "born_speed_up_and_down",
            check(
                slowest < 3.0 && fastest > 3.0 && slowest >= 1.0 && fastest <= 5.0,
                format!("{slowest}..{fastest}"),
            ),
        ),
    ]
}

/// An emitter of `update` with `rest` values and a clip keying `keyed`.
#[cfg(feature = "probes")]
fn limit_emitter(update: Update, rest: &[(u32, f32)], keyed: &[(u32, f32)]) -> Emitter {
    let values = |values: &[(u32, f32)]| values.iter().map(|&(kind, v)| (kind, vec![v])).collect();
    Emitter {
        node: String::new(),
        update,
        facing: Facing::Camera,
        blend: Blend::Normal,
        texture: String::new(),
        chunk: None,
        grid: [1, 1],
        looping: false,
        world: false,
        inherit_velocity: false,
        wind: false,
        bounce: false,
        random_frame: false,
        p2p: None,
        distort: false,
        rest: values(rest),
        clips: BTreeMap::from([(
            "on".to_string(),
            keyed
                .iter()
                .map(|&(kind, v)| (kind, vec![(0.0, vec![v])]))
                .collect(),
        )]),
    }
}

/// What `Emitter::limit` gives: a fountain keyed on by a clip from a rest of
/// none, a fountain with no birthrate at all, and an explosion's whole burst
/// (past a short life's stream, the birthrate's default 1, the guard on it).
#[cfg(feature = "probes")]
fn emitter_limit_probes() -> Checks {
    let (birth, life) = (emit::BIRTHRATE, emit::LIFE_EXP);
    [
        (
            "limit_clip_keyed",
            Update::Fountain,
            vec![(birth, 0.0), (life, 2.0)],
            vec![(birth, 10.0)],
            21,
        ),
        (
            "limit_fountain_no_birthrate",
            Update::Fountain,
            vec![(life, 5.0)],
            vec![],
            1,
        ),
        (
            "limit_burst_whole",
            Update::Explosion,
            vec![(birth, 300.0), (life, 0.1)],
            vec![],
            300,
        ),
        (
            "limit_burst_default_one",
            Update::Explosion,
            vec![(life, 5.0)],
            vec![],
            6,
        ),
        (
            "limit_burst_most",
            Update::Explosion,
            vec![(birth, 10_000.0), (life, 0.1)],
            vec![],
            PARTICLES_MOST,
        ),
    ]
    .into_iter()
    .map(|(name, update, rest, keyed, expected)| {
        let got = limit_emitter(update, &rest, &keyed).limit();
        (name, check(got == expected, format!("{got} of {expected}")))
    })
    .collect()
}

/// One particle of a `facing` emitter (heading up its own z, placed by
/// `space`) drawn to a camera of `right` and `up`: its quad's side and
/// height as unit vectors.
#[cfg(feature = "probes")]
fn facing_axes(facing: Facing, (right, up): (Vec3, Vec3), space: Mat4) -> (Vec3, Vec3) {
    let rest = [
        (emit::LIFE_EXP, 5.0),
        (emit::SIZE_START, 2.0),
        (emit::VELOCITY, 3.0),
    ];
    let emitter = Emitter {
        facing,
        ..limit_emitter(Update::Single, &rest, &[])
    };
    let values = Values::of(&emitter);
    let mut particles = Particles::new(1, 8);
    particles.step(&emitter, &values, (0.1, false), (space, None), None);
    let mut shape = Shape::default();
    particles.quads(&emitter, &values, space, (right, up), &mut shape);
    let corner = |k: usize| Vec3::from_array(shape.positions[k]);
    (
        (corner(1) - corner(0)).normalize_or_zero(),
        (corner(3) - corner(0)).normalize_or_zero(),
    )
}

/// The orientations drawn about an axis: world z's quad stands on it,
/// turned to the camera's side (its up down for an upside-down camera), and
/// the particle direction's is long along where its particle heads, its
/// side the emitter's x turned square to that.
#[cfg(feature = "probes")]
fn facing_probes() -> Checks {
    let near = |a: Vec3, b: Vec3| (a - b).length() < 1e-4;
    let (side, height) = facing_axes(Facing::AlignedWorldZ, (Vec3::X, Vec3::Y), Mat4::IDENTITY);
    let hanging = Vec3::new(0.0, 0.6, -0.8);
    let (_, upside_down) = facing_axes(Facing::AlignedWorldZ, (Vec3::X, hanging), Mat4::IDENTITY);
    // The emitter's z turned onto world x, its x onto -z.
    let turned = Mat4::from_rotation_y(std::f32::consts::FRAC_PI_2);
    let (across, along) = facing_axes(Facing::AlignedDirection, (Vec3::Y, Vec3::Z), turned);
    vec![
        (
            "facing_world_z",
            check(
                near(side, Vec3::X) && near(height, Vec3::Z),
                format!("{side} {height}"),
            ),
        ),
        (
            "facing_world_z_upside_down",
            check(near(upside_down, Vec3::NEG_Z), format!("{upside_down}")),
        ),
        (
            "facing_particle_dir",
            check(
                near(along, Vec3::X) && near(across, Vec3::NEG_Z),
                format!("{along} {across}"),
            ),
        ),
    ]
}

/// A one-second clip with a `detonate` event at its last frame.
#[cfg(feature = "probes")]
fn detonating_clip() -> AnimationIr {
    AnimationIr {
        name: "loop".into(),
        length: 1.0,
        transition: 0.0,
        root: None,
        tracks: Vec::new(),
        settings: Vec::new(),
        events: vec![(1.0, "detonate".into())],
    }
}

/// A `detonate` at a loop's last frame: passed once as the clip wraps, not
/// the frame before or after it, nor as a looping clip first begins.
#[cfg(feature = "probes")]
fn detonation_probes() -> Checks {
    let clip = detonating_clip();
    let passed = |frames: &[(f32, f32)]| {
        frames
            .iter()
            .filter(|&&(at, elapsed)| detonates_over(&clip, (at, elapsed), 0.1))
            .count()
    };
    vec![
        (
            "detonate_on_wrap",
            check(detonates(&clip, 0.95, 0.05), "from 0.95 to 0.05".into()),
        ),
        (
            "detonate_once_over_a_loop",
            check(
                passed(&[(0.9, 0.9), (0.05, 1.05), (0.15, 1.15)]) == 1,
                "frames up to 0.9, over the wrap and past it".into(),
            ),
        ),
        (
            "detonate_not_as_a_loop_begins",
            check(passed(&[(0.05, 0.05)]) == 0, "its first frame".into()),
        ),
    ]
}
