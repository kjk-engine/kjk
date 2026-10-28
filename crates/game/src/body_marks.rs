//! The burns a body takes where a blade struck it, as JA's multiplayer client
//! lays ghoul2 "gore" on a model: a mark is the body's own triangles where
//! it was struck, drawn again with the mark's textures by coordinates
//! projected onto them, skinned as the body is, so it wraps round the body
//! and moves with it.
//!
//! - For 400 ms after a blade's hit on a body (a hit its snapshot tells:
//!   the client's window for such marks), each client frame the striker's
//!   blades as drawn (`blade_fx::Wielders`) meet the body as posed: where a
//!   blade passes within `TOUCH` of its skin, a saber mark
//!   (`gfx/damage/saberglowmark`: the burn band alpha-blended, the glow
//!   streak added and flickering, `rgbGen wave noise 1 0.5 0 2`), 3 to 4
//!   units either side, projected along the blade (hilt to tip), living 5
//!   to 10 s. A trail, one a frame, while the blade drags through.
//! - JA's multiplayer burns bolts' hits onto bodies too
//!   (`gfx/damage/bodyburnmark1`); here they leave none (the owner's call,
//!   2026-10-24: a black scorch on the face).
//! - A mark is laid across the blade's ray: its two axes are the ray's
//!   sides, turned by a random angle and scaled so the mark spans twice
//!   its half-width (`mark_axes`). Each vertex of the body as posed gets
//!   coordinates on the mark from its offset to the hit; the body's
//!   triangles are drawn again as the mark unless wholly off one edge of
//!   it, or wholly outside a depth along the ray from 10 units in front
//!   of the hit to 15 behind it (`DEPTH`); a triangle that crosses an
//!   edge is kept whole (`gore`).
//! - A body holds at most `MOST_RECORDS` marked meshes (the client's
//!   setting: a new mark waits till one goes), every body `ALL_RECORDS`
//!   (the oldest mark goes); a mark goes at once when its life is over
//!   (JA's fade is commented out).
//! - Marks lie `PUSH_M` out of the skin (for JA's shaders' `polygonOffset`).
//! - A cut that parts a body parts its marks with it (`severing::apply`):
//!   each mark's own triangles on either side of the cut, the limb's half
//!   going with the limb; every piece of a mark is its `Mark`, and goes
//!   with it.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use asset_odyssey::texture::TextureIr;
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use sim::SimId;
use sim::combat::{Delivery, Outcome};

use crate::blade_fx::Wielders;
use crate::cast::BodyRoots;
use crate::glow_shell::{GlowMaterial, SceneMeshes};
use crate::material::Surface;
use crate::scene::{DrawnBlades, game_to_bevy};
use crate::sim_host::{PresentedSnapshot, SceneSwap, view_ms};

const UNIT: f32 = asset_ja::UNIT;

/// The textures the mark's stages draw (files, not shaders: a shader name
/// gives its first stage only): the burn band, then the glow streak.
const BURN: &str = "gfx/damage/saberburnmark2.tga";
const GLOW: &str = "gfx/damage/saberglowmark.jpg";
/// The textures the combat effects load for the marks.
pub const TEXTURES: &[&str] = &[BURN, GLOW];

/// How long after a blade's hit its blades mark the body (ms).
const COMP_MS: f32 = 400.0;
/// How close a blade passes the skin to mark it (units): about a blade's
/// width [verify: JA's ghoul2 poly trace meets the triangles].
const TOUCH: f32 = 2.5;
/// The skin marked, along the ray from the hit (units).
const DEPTH: (f32, f32) = (-10.0, 15.0);
/// How far a mark lies out of the skin (metres): JA's `polygonOffset`
/// stands in; 3 mm sank under the skin's depth, KOTOR's shells stand
/// 2 cm out (`glow_shell::PUSH_M`).
const PUSH_M: f32 = 0.01;
const MOST_RECORDS: usize = 16;
const ALL_RECORDS: usize = 500;
/// A mark's size either side (units) and life (ms), each drawn between.
const SIZE: (f32, f32) = (3.0, 4.0);
const LIFE_MS: (f32, f32) = (5000.0, 10_000.0);

/// A mark asked for: on whom, where and along which way (game space,
/// metres).
#[derive(Clone, Copy, Debug)]
struct Asked {
    who: SimId,
    at: Vec3,
    ray: Vec3,
}

/// The marks' images, from the combat effects' textures (`install`).
#[derive(Resource)]
struct Looks {
    images: BTreeMap<&'static str, Handle<Image>>,
    /// Each stage's material, by its texture, made once.
    materials: BTreeMap<&'static str, Handle<GlowMaterial>>,
}

/// A mark's entity, a piece of the mark laid as `Laid::id` (a cut may part
/// it in two, `severing`).
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Mark(pub u32);

/// A mark's glow stage (the streak added over its burn): a cut through it
/// puts it out (`severing::apply`), the cap's own seared glow standing for
/// it.
#[derive(Component)]
pub struct Glowing;

/// A mark laid: on whom, which (`Mark`), how many meshes it covers, when it
/// goes (the view's ms).
struct Laid {
    who: SimId,
    id: u32,
    records: usize,
    until: f32,
}

#[derive(Resource, Default)]
struct State {
    laid: VecDeque<Laid>,
    /// Blades' hits lately: striker, struck, when (the view's ms).
    hits: Vec<(SimId, SimId, f32)>,
    generation: u32,
    count: u32,
}

pub struct BodyMarksPlugin;

impl Plugin for BodyMarksPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<State>().add_systems(
            PostUpdate,
            mark.run_if(resource_exists::<Looks>)
                .run_if(resource_exists::<PresentedSnapshot>)
                .after(crate::combat_fx::FightDrawn)
                .after(crate::scene::BladesDrawn),
        );
    }
}

/// The marks' images, from the textures the combat effects loaded.
pub fn install(
    commands: &mut Commands,
    textures: &BTreeMap<String, TextureIr>,
    images: &mut Assets<Image>,
) {
    let images = TEXTURES
        .iter()
        .filter_map(|&name| {
            let mut texture = textures.get(name)?.clone();
            // JA's marks' maps clamp (`clampmap`): a triangle across a
            // mark's edge reaches past its texture.
            texture.facts.clamp = true;
            Some((name, images.add(crate::scene::material_image(&texture))))
        })
        .collect();
    commands.insert_resource(Looks {
        images,
        materials: BTreeMap::new(),
    });
}

impl Looks {
    /// A stage's material, made the first time: `texture` blended as
    /// `surface`.
    fn stage(
        &mut self,
        texture: &'static str,
        surface: Surface,
        materials: &mut Assets<GlowMaterial>,
    ) -> Option<Handle<GlowMaterial>> {
        if let Some(made) = self.materials.get(texture) {
            return Some(made.clone());
        }
        let image = self.images.get(texture)?.clone();
        let made = materials.add(GlowMaterial::mark(image, PUSH_M, surface));
        self.materials.insert(texture, made.clone());
        Some(made)
    }

    /// The mark's stages: its burn, alpha-blended, then its glow, added
    /// (`Glowing`).
    fn stages(
        &mut self,
        materials: &mut Assets<GlowMaterial>,
    ) -> Vec<(Handle<GlowMaterial>, bool)> {
        [(BURN, Surface::Blend), (GLOW, Surface::AddOne)]
            .into_iter()
            .filter_map(|(texture, surface)| {
                let made = self.stage(texture, surface, materials)?;
                Some((made, texture == GLOW))
            })
            .collect()
    }

    /// The glow's colour at `seconds` (its `rgbGen` wave; a wave's time is
    /// the renderer's, one for every mark).
    fn pulse(&self, seconds: f32, materials: &mut Assets<GlowMaterial>) {
        let value = (1.0 + 0.5 * noise(seconds * 2.0)).clamp(0.0, 1.0);
        if let Some(mut made) = self
            .materials
            .get(GLOW)
            .and_then(|handle| materials.get_mut(handle))
        {
            made.tint = Vec4::new(value, value, value, 1.0);
        }
    }
}

/// A smooth noise in −1..1 (the renderer's `R_NoiseGet4f` stand-in
/// [verify: its table's values]).
fn noise(x: f32) -> f32 {
    let hash = |i: i32| {
        let mut h = (i as u32).wrapping_mul(0x27d4_eb2d);
        h ^= h >> 15;
        h = h.wrapping_mul(0x85eb_ca6b);
        h ^= h >> 13;
        (h & 0xffff) as f32 / 32767.5 - 1.0
    };
    let floor = x.floor();
    let t = x - floor;
    let t = t * t * (3.0 - 2.0 * t);
    let i = floor as i32;
    hash(i) * (1.0 - t) + hash(i + 1) * t
}

/// Where things are.
type Places<'w, 's> = Query<'w, 's, &'static GlobalTransform>;

/// The meshes, skins and stages the marks read and make.
type MarkAssets<'w> = (
    ResMut<'w, Assets<Mesh>>,
    Res<'w, Assets<SkinnedMeshInverseBindposes>>,
    ResMut<'w, Assets<GlowMaterial>>,
);

#[allow(clippy::too_many_arguments)]
fn mark(
    presented: Res<PresentedSnapshot>,
    swap: Res<SceneSwap>,
    (mut state, mut looks): (ResMut<State>, ResMut<Looks>),
    (roots, wielders, drawn): (Res<BodyRoots>, Res<Wielders>, Res<DrawnBlades>),
    (children, scene_meshes, places): (Query<&'static Children>, SceneMeshes, Places),
    (mut meshes, bindposes, mut materials): MarkAssets,
    marks: Query<(Entity, &Mark, &Mesh3d, Option<&SkinnedMesh>)>,
    (mut facts, dump_due): (ResMut<console::DumpFacts>, Res<console::DumpDue>),
    mut commands: Commands,
) {
    let state = &mut *state;
    // The marks gone this frame, every piece of each (by `Mark`).
    let mut gone = BTreeSet::new();
    if swap.generation != state.generation {
        state.generation = swap.generation;
        gone.extend(state.laid.drain(..).map(|laid| laid.id));
        state.hits.clear();
    }
    let now = view_ms(&presented.0) as f32;
    looks.pulse(now / 1000.0, &mut materials);
    state.laid.retain(|laid| {
        let over = laid.until <= now;
        if over {
            gone.insert(laid.id);
        }
        !over
    });
    // A blade's hits on bodies, new this frame.
    for snapshot in &presented.1 {
        for entity in &snapshot.entities {
            let Some(f) = entity.fighter else { continue };
            for event in f.events.iter().flatten() {
                let blade = matches!(event.weapon, Some(Delivery::Saber(_) | Delivery::Melee));
                let struck = matches!(
                    event.result,
                    Outcome::Hit | Outcome::Critical | Outcome::Automatic
                );
                if blade
                    && struck
                    && event.tick == snapshot.tick
                    && event.damage > 0
                    && let Some(by) = event.by
                {
                    state.hits.retain(|hit| (hit.0, hit.1) != (by, entity.id));
                    state.hits.push((by, entity.id, now));
                }
            }
        }
    }
    state.hits.retain(|&(.., at)| now - at <= COMP_MS);
    let mut wanted = Vec::new();
    let world = Bodies {
        roots: &roots,
        children: &children,
        scene_meshes: &scene_meshes,
        places: &places,
        bindposes: &bindposes,
    };
    for &(by, victim, _) in &state.hits {
        let posed = world.posed_of(victim, &meshes);
        for &(_, _, index) in wielders.0.iter().filter(|(who, ..)| *who == by) {
            let Some(Some([hilt, tip])) = drawn.0.get(index) else {
                continue;
            };
            if let Some(at) = touched(&posed, (*hilt, *tip)) {
                wanted.push(Asked {
                    who: victim,
                    at,
                    ray: *tip - *hilt,
                });
            }
        }
    }
    for asked in wanted {
        lay(
            state,
            asked,
            &world,
            now,
            &mut looks,
            (&mut meshes, &mut materials),
            &mut commands,
        );
    }
    // Past `ALL_RECORDS`, the oldest go.
    let mut all: usize = state.laid.iter().map(|laid| laid.records).sum();
    while all > ALL_RECORDS
        && let Some(oldest) = state.laid.pop_front()
    {
        all -= oldest.records;
        gone.insert(oldest.id);
    }
    for (entity, mark, ..) in &marks {
        if gone.contains(&mark.0) {
            commands.entity(entity).despawn();
        }
    }
    // A dump's check: how far the middle of a mark's triangle is drawn off
    // the bodies' skin (their nearest vertex), the most of any: a mark
    // across a cut, run from the body to the limb, spans open air.
    let off_skin_cm = dump_due.0.then(|| {
        let skin: Vec<Vec3> = roots
            .0
            .values()
            .flat_map(|&root| std::iter::once(root).chain(children.iter_descendants(root)))
            .filter_map(|entity| {
                let (mesh3d, _, skin, _) = scene_meshes.get(entity).ok()?;
                Some(as_drawn(&world, &meshes, (entity, mesh3d, skin))?.0)
            })
            .flatten()
            .collect();
        marks
            .iter()
            .filter_map(|(entity, _, mesh3d, skin)| {
                as_drawn(&world, &meshes, (entity, mesh3d, skin))
            })
            .flat_map(|(at, corners)| {
                corners
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .filter_map(|&[a, b, c]| Some((*at.get(a)? + *at.get(b)? + *at.get(c)?) / 3.0))
                    .collect::<Vec<_>>()
            })
            .map(|middle| {
                skin.iter()
                    .map(|vertex| vertex.distance(middle))
                    .fold(f32::MAX, f32::min)
            })
            .fold(0.0, f32::max)
            * 100.0
    });
    facts.0.insert(
        "body_marks".into(),
        serde_json::json!({
            "laid": state.count,
            "now": state.laid.len(),
            "records": state.laid.iter().map(|laid| laid.records).sum::<usize>(),
            "pieces": marks.iter().count(),
            "off_skin_cm": off_skin_cm,
        }),
    );
}

/// A mesh (`entity`'s) as drawn now: its vertices (Bevy's world, metres)
/// and its triangles' corners.
fn as_drawn(
    world: &Bodies,
    meshes: &Assets<Mesh>,
    (entity, mesh3d, skin): (Entity, &Mesh3d, Option<&SkinnedMesh>),
) -> Option<(Vec<Vec3>, Vec<usize>)> {
    let mesh = meshes.get(&mesh3d.0)?;
    let read = |id| mesh.try_attribute_option(id).ok().flatten();
    let Some(VertexAttributeValues::Float32x3(positions)) = read(Mesh::ATTRIBUTE_POSITION) else {
        return None;
    };
    let at = match skin {
        Some(skin) => world.skinned(
            skin,
            positions,
            (
                read(Mesh::ATTRIBUTE_JOINT_INDEX),
                read(Mesh::ATTRIBUTE_JOINT_WEIGHT),
            ),
        )?,
        None => {
            let place = world.places.get(entity).ok()?;
            positions
                .iter()
                .map(|p| place.transform_point(Vec3::from_array(*p)))
                .collect()
        }
    };
    let corners = match mesh.try_indices_option().ok().flatten()? {
        Indices::U16(list) => list.iter().map(|&v| usize::from(v)).collect(),
        Indices::U32(list) => list.iter().map(|&v| v as usize).collect(),
    };
    Some((at, corners))
}

/// What the marks read the bodies from.
struct Bodies<'a, 'w, 's> {
    roots: &'a BodyRoots,
    children: &'a Query<'w, 's, &'static Children>,
    scene_meshes: &'a SceneMeshes<'w, 's>,
    places: &'a Places<'w, 's>,
    bindposes: &'a Assets<SkinnedMeshInverseBindposes>,
}

/// A mesh of a body as posed now: the mesh entity, its asset, its skin,
/// and its vertices where they are drawn (game space, metres).
struct Posed {
    entity: Entity,
    mesh: Handle<Mesh>,
    skin: Option<SkinnedMesh>,
    at: Vec<Vec3>,
}

impl Bodies<'_, '_, '_> {
    /// Every mesh of `who`'s body as posed now.
    fn posed_of(&self, who: SimId, meshes: &Assets<Mesh>) -> Vec<Posed> {
        self.roots
            .0
            .get(&who)
            .map_or(Vec::new(), |root| self.posed(*root, meshes))
    }

    /// Its meshes that keep a copy to read (a body's, skinned; a head's): each
    /// vertex where it is drawn. A weapon's keep none, and are not marked.
    fn posed(&self, root: Entity, meshes: &Assets<Mesh>) -> Vec<Posed> {
        let to_game = game_to_bevy().inverse();
        std::iter::once(root)
            .chain(self.children.iter_descendants(root))
            .filter_map(|entity| {
                let (mesh3d, _, skin, _) = self.scene_meshes.get(entity).ok()?;
                let mesh = meshes.get(&mesh3d.0)?;
                let read = |id| mesh.try_attribute_option(id).ok().flatten();
                let Some(VertexAttributeValues::Float32x3(positions)) =
                    read(Mesh::ATTRIBUTE_POSITION)
                else {
                    return None;
                };
                let at = match skin {
                    Some(skin) => self.skinned(
                        skin,
                        positions,
                        (
                            read(Mesh::ATTRIBUTE_JOINT_INDEX),
                            read(Mesh::ATTRIBUTE_JOINT_WEIGHT),
                        ),
                    )?,
                    None => {
                        let place = self.places.get(entity).ok()?;
                        positions
                            .iter()
                            .map(|p| place.transform_point(Vec3::from_array(*p)))
                            .collect()
                    }
                };
                Some(Posed {
                    entity,
                    mesh: mesh3d.0.clone(),
                    skin: skin.cloned(),
                    at: at.into_iter().map(|point| to_game * point).collect(),
                })
            })
            .collect()
    }

    /// `positions` skinned by `skin` as posed now (Bevy's world).
    fn skinned(
        &self,
        skin: &SkinnedMesh,
        positions: &[[f32; 3]],
        attributes: (
            Option<&VertexAttributeValues>,
            Option<&VertexAttributeValues>,
        ),
    ) -> Option<Vec<Vec3>> {
        let (
            Some(VertexAttributeValues::Uint16x4(indices)),
            Some(VertexAttributeValues::Float32x4(weights)),
        ) = attributes
        else {
            return None;
        };
        let binds = self.bindposes.get(&skin.inverse_bindposes)?;
        let joints: Vec<Mat4> = skin
            .joints
            .iter()
            .zip(binds.iter())
            .map(|(joint, bind)| {
                self.places
                    .get(*joint)
                    .map_or(Mat4::IDENTITY, |place| place.to_matrix() * *bind)
            })
            .collect();
        Some(
            positions
                .iter()
                .zip(indices.iter().zip(weights))
                .map(|(p, (joint, weight))| {
                    let p = Vec3::from_array(*p);
                    (0..4).fold(Vec3::ZERO, |sum, k| {
                        let matrix = joints
                            .get(usize::from(joint[k]))
                            .copied()
                            .unwrap_or(Mat4::IDENTITY);
                        sum + matrix.transform_point3(p) * weight[k]
                    })
                })
                .collect(),
        )
    }
}

/// Where a blade from `hilt` to `tip` (game metres) passes closest to the
/// skin, if within `TOUCH`: the skin's point there.
fn touched(posed: &[Posed], (hilt, tip): (Vec3, Vec3)) -> Option<Vec3> {
    let along = tip - hilt;
    let length = along.length_squared().max(1e-8);
    posed
        .iter()
        .flat_map(|mesh| mesh.at.iter().copied())
        .map(|point| {
            let t = ((point - hilt).dot(along) / length).clamp(0.0, 1.0);
            (point, (hilt + along * t).distance(point))
        })
        .filter(|&(_, distance)| distance <= TOUCH * UNIT)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(point, _)| point)
}

/// Lays `asked` on its body, if the body has room for it.
fn lay(
    state: &mut State,
    asked: Asked,
    world: &Bodies,
    now: f32,
    looks: &mut Looks,
    (meshes, materials): (&mut Assets<Mesh>, &mut Assets<GlowMaterial>),
    commands: &mut Commands,
) {
    let Some(ray) = asked.ray.try_normalize() else {
        return;
    };
    let held: usize = state
        .laid
        .iter()
        .filter(|laid| laid.who == asked.who)
        .map(|laid| laid.records)
        .sum();
    if held >= MOST_RECORDS {
        return;
    }
    let mut rng = crate::view_rng::ja_seed(asked.who.0 ^ state.count, now.to_bits());
    let theta = rng.range(0.0, std::f32::consts::TAU);
    let size = rng.range(SIZE.0, SIZE.1) * UNIT;
    let (s_size, t_size) = (size, size);
    let life = rng.range(LIFE_MS.0, LIFE_MS.1);
    let (s_axis, t_axis) = mark_axes(ray, theta, (s_size, t_size));
    let stages = looks.stages(materials);
    let id = state.count;
    let mut records = 0;
    let covered: Vec<(Posed, Mesh)> = world
        .posed_of(asked.who, meshes)
        .into_iter()
        .filter_map(|posed| {
            let mesh = meshes.get(&posed.mesh)?;
            let made = gore(mesh, &posed.at, (asked.at, ray), (s_axis, t_axis))?;
            Some((posed, made))
        })
        .collect();
    for (posed, gore) in covered {
        let gore = meshes.add(gore);
        records += 1;
        for (stage, glows) in &stages {
            let mut spawned = commands.spawn((
                Mesh3d(gore.clone()),
                MeshMaterial3d(stage.clone()),
                Mark(id),
                Transform::IDENTITY,
                NoFrustumCulling,
            ));
            if let Some(skin) = &posed.skin {
                spawned.insert(skin.clone());
            }
            if *glows {
                spawned.insert(Glowing);
            }
            let spawned = spawned.id();
            commands.entity(posed.entity).add_child(spawned);
        }
    }
    if records == 0 {
        return;
    }
    state.count += 1;
    state.laid.push_back(Laid {
        who: asked.who,
        id,
        records,
        until: now + life,
    });
}

/// The mark's two axes across `ray` (a unit vector), turned by `theta`
/// (radians); `s_size` and `t_size` (metres) are half-widths: the mark
/// spans twice each along its axis. A point offset along an axis by its
/// half-width moves its coordinate by 0.5.
fn mark_axes(ray: Vec3, theta: f32, (s_size, t_size): (f32, f32)) -> (Vec3, Vec3) {
    let mut across = ray.cross(Vec3::Z);
    // A ray nearly straight up or down takes its side from +Y instead.
    if across.length_squared() < 0.1 {
        across = ray.cross(Vec3::Y);
    }
    let across = across.normalize();
    let up = ray.cross(across).normalize();
    let (sin, cos) = theta.sin_cos();
    let t_axis = (cos * across + sin * up) * (0.5 / t_size);
    let s_axis = (-sin * across + cos * up) * (0.5 / s_size);
    (s_axis, t_axis)
}

/// The triangles of `mesh` a mark struck at `hit` along `ray` covers, as
/// a mesh of their own with the mark's coordinates (`s_axis`, `t_axis`
/// from `mark_axes`) as its texture coordinates. `at` is the mesh's
/// vertices as posed (game metres), one per vertex in its order. A
/// triangle is kept unless all three of its corners lie off the same side
/// of the mark, or all three outside its depth along the ray; one that
/// crosses an edge is kept whole. None when nothing is covered (or the
/// mesh has no index list).
fn gore(
    mesh: &Mesh,
    at: &[Vec3],
    (hit, ray): (Vec3, Vec3),
    (s_axis, t_axis): (Vec3, Vec3),
) -> Option<Mesh> {
    let indices: Vec<usize> = match mesh.indices()? {
        Indices::U16(list) => list.iter().map(|&v| usize::from(v)).collect(),
        Indices::U32(list) => list.iter().map(|&v| v as usize).collect(),
    };
    // Each point's coordinates on the mark, and which edges it lies past:
    // one bit each for s below 0 and above 1, t below 0 and above 1, and
    // the depth window.
    let marked: Vec<([f32; 2], u8)> = at
        .iter()
        .map(|&point| {
            let delta = point - hit;
            let s = delta.dot(s_axis) + 0.5;
            let t = delta.dot(t_axis) + 0.5;
            let depth = delta.dot(ray) / UNIT;
            let off = u8::from(s < 0.0)
                | u8::from(s > 1.0) << 1
                | u8::from(t < 0.0) << 2
                | u8::from(t > 1.0) << 3
                | u8::from(!(DEPTH.0 <= depth && depth <= DEPTH.1)) << 4;
            ([s, t], off)
        })
        .collect();

    // The kept triangles' vertices, numbered as first met.
    let mut order: Vec<usize> = Vec::new();
    let mut renumbered: Vec<Option<u32>> = vec![None; at.len()];
    let mut kept: Vec<u32> = Vec::new();
    for triangle in indices.as_chunks::<3>().0 {
        if triangle.iter().any(|&v| v >= at.len()) {
            continue;
        }
        let shared = triangle.iter().fold(u8::MAX, |all, &v| all & marked[v].1);
        if shared != 0 {
            continue;
        }
        for &v in triangle {
            let number = *renumbered[v].get_or_insert_with(|| {
                order.push(v);
                (order.len() - 1) as u32
            });
            kept.push(number);
        }
    }
    if kept.is_empty() {
        return None;
    }

    let mut made = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    for attribute in [
        Mesh::ATTRIBUTE_POSITION,
        Mesh::ATTRIBUTE_NORMAL,
        Mesh::ATTRIBUTE_JOINT_INDEX,
        Mesh::ATTRIBUTE_JOINT_WEIGHT,
    ] {
        if let Some(values) = mesh
            .attribute(attribute)
            .and_then(|values| picked(values, &order))
        {
            made.insert_attribute(attribute, values);
        }
    }
    let uvs: Vec<[f32; 2]> = order.iter().map(|&v| marked[v].0).collect();
    made.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    made.insert_indices(Indices::U32(kept));
    Some(made)
}

/// `values` at `order`'s vertices (of the kinds a body's meshes hold).
fn picked(values: &VertexAttributeValues, order: &[usize]) -> Option<VertexAttributeValues> {
    fn pick<T: Copy>(all: &[T], order: &[usize]) -> Vec<T> {
        order.iter().map(|&v| all[v]).collect()
    }
    Some(match values {
        VertexAttributeValues::Float32x3(all) => VertexAttributeValues::Float32x3(pick(all, order)),
        VertexAttributeValues::Float32x4(all) => VertexAttributeValues::Float32x4(pick(all, order)),
        VertexAttributeValues::Uint16x4(all) => VertexAttributeValues::Uint16x4(pick(all, order)),
        _ => return None,
    })
}
