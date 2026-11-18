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
//! - A bolt hit that the owner's ruling lets burn (`bolt_hits::burns`)
//!   leaves a bolt burn where the bolt met the body. A bolt burn has three
//!   layers, all Jedi Academy's textures. The flash of the hit
//!   (`gfx/effects/whiteglow`) is added over the skin and turns from white
//!   through red to nothing in 0.6 s. The embers
//!   (`gfx/damage/bodyburnmark_glow`) are added over the skin and glow,
//!   flickering, for 3.5 s. The burn itself (`gfx/damage/bodyburnmark2`)
//!   is alpha-blended, lasts 8 s and fades out over its last 2 s. Each bolt
//!   burn has its own three materials, so each cools by itself. A burn is
//!   small for a pistol, larger for a rifle and largest for a Power Blast
//!   (`BOLT_SIZE`). It marks the skin from 3 units in front of the hit to 4
//!   units behind it, along the bolt's way (`BOLT_DEPTH`).
//! - A bolt burn goes where the bolt's line first meets the body as posed
//!   (`burn_place`). The line is traced from 1 m before the point where it
//!   passes the hit location's node (or the chest, for a body with no
//!   nodes) to 1 m past that point. When the first triangle it meets is the
//!   head's, the bolt leaves no burn. When it meets nothing, the burn goes
//!   on the skin nearest the node or the chest. A bolt burn never covers a
//!   mesh that hangs under the head node, nor a triangle with a corner
//!   skinned with a weight of 0.5 or more to the head node or a joint under
//!   it.
//! - A body keeps at most 6 bolt burns (`bolt_hits::MOST_BOLT_BURNS`). A
//!   seventh takes the place of the body's oldest bolt burn
//!   (`make_room`). Bolt burns are counted apart from lightsaber marks and
//!   never push one out.
//! - A mark is laid across the blade's ray: its two axes are the ray's
//!   sides, turned by a random angle and scaled so the mark spans twice
//!   its half-width (`mark_axes`). Each vertex of the body as posed gets
//!   coordinates on the mark from its offset to the hit; the body's
//!   triangles are drawn again as the mark unless wholly off one edge of
//!   it, or wholly on one side of a depth along the ray from 10 units in
//!   front of the hit to 15 behind it (`DEPTH`); a triangle that crosses an
//!   edge, or the whole depth window, is kept whole (`gore`).
//! - A body holds at most `MOST_RECORDS` meshes marked by lightsabers (the
//!   client's setting: a new mark waits till one goes), every body
//!   `ALL_RECORDS` (the oldest bolt burn goes first, then the oldest mark);
//!   a lightsaber mark goes at once when its life is over (JA's fade is
//!   commented out).
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
use sim::combat::{Delivery, Location, Outcome, mind};

use crate::blade_fx::Wielders;
use crate::bolt_hits::{self, BoltHits, BurnSize, MOST_BOLT_BURNS};
use crate::cast::{BodyRoots, Hitboxes};
use crate::glow_shell::{GlowMaterial, SceneMeshes};
use crate::material::Surface;
use crate::scene::{DrawnBlades, game_to_bevy};
use crate::sim_host::{PresentedSnapshot, SceneSwap, view_ms};

const UNIT: f32 = asset_ja::UNIT;

/// The textures the mark's stages draw (files, not shaders: a shader name
/// gives its first stage only): the burn band, then the glow streak.
const BURN: &str = "gfx/damage/saberburnmark2.tga";
const GLOW: &str = "gfx/damage/saberglowmark.jpg";
/// The textures of a bolt burn's three layers: the burn, its embers and the
/// flash of the hit.
const BOLT_BURN: &str = "gfx/damage/bodyburnmark2.tga";
const BOLT_EMBERS: &str = "gfx/damage/bodyburnmark_glow.jpg";
const BOLT_FLASH: &str = "gfx/effects/whiteglow.jpg";
/// The textures the combat effects load for the marks.
pub const TEXTURES: &[&str] = &[BURN, GLOW, BOLT_BURN, BOLT_EMBERS, BOLT_FLASH];

/// How long a bolt burn lasts, and how long it fades out at the end of that
/// time (ms).
const BOLT_LIFE_MS: f32 = 8000.0;
const BOLT_FADE_MS: f32 = 2000.0;
/// How long a bolt burn's embers glow (ms).
const BOLT_EMBERS_MS: f32 = 3500.0;
/// How long the flash of a bolt's hit takes to turn from white through red
/// to nothing (ms).
const BOLT_FLASH_MS: f32 = 600.0;
/// The skin a bolt burn marks, along the bolt's way from the hit (units).
const BOLT_DEPTH: (f32, f32) = (-3.0, 4.0);
/// A bolt burn's half-width (units), drawn between the two numbers, for a
/// pistol, a rifle and a Power Blast (`BurnSize`, in its order).
const BOLT_SIZE: [(f32, f32); 3] = [(1.5, 2.0), (2.0, 2.5), (2.75, 3.25)];
/// How far a bolt's line is traced before and past the point where it
/// passes the body's hit location (metres).
const TRACE_M: f32 = 1.0;
/// A vertex skinned with at least this weight, in all, to the head node or
/// a joint under it is the head's.
const HEAD_WEIGHT: f32 = 0.5;
/// How bright a bolt burn's embers are when laid.
const EMBERS_GLOW: f32 = 1.6;
/// The embers flicker. The flicker moves their brightness up or down by at
/// most this fraction of it.
const EMBERS_FLICKER: f32 = 0.3;
/// The flash's colour as it cools, at each fraction of `BOLT_FLASH_MS`:
/// near white, orange, red, a dull red, then nothing.
const FLASH_KEYS: [(f32, Vec3); 5] = [
    (0.0, Vec3::new(1.0, 0.95, 0.8)),
    (0.15, Vec3::new(1.0, 0.6, 0.2)),
    (0.45, Vec3::new(0.8, 0.25, 0.05)),
    (0.8, Vec3::new(0.3, 0.04, 0.01)),
    (1.0, Vec3::ZERO),
];

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
    /// A bolt burn's own layers. `None` for a lightsaber's mark.
    bolt: Option<BoltStages>,
}

/// A bolt burn's own three materials (the burn, the embers and the flash)
/// and when it was laid (the view's ms). Each bolt burn cools by itself.
struct BoltStages {
    materials: [Handle<GlowMaterial>; 3],
    laid_ms: f32,
}

#[derive(Resource, Default)]
struct State {
    laid: VecDeque<Laid>,
    /// Blades' hits lately: striker, struck, when (the view's ms).
    hits: Vec<(SimId, SimId, f32)>,
    /// The bolt hit records already handled, by the body struck and the
    /// event's number.
    bolts_seen: BTreeSet<(SimId, u16)>,
    /// When each body's last bolt burn was laid (snapshot ms).
    last_burn: BTreeMap<SimId, u64>,
    generation: u32,
    count: u32,
    /// How many bolt burns were laid.
    bolt_count: u32,
}

/// The tints of a bolt burn's three layers (the burn, the embers and the
/// flash) `age` ms after the hit, `left` ms before the burn goes. `flicker`
/// scales the embers' brightness. The flash turns from near white through
/// red to nothing over `BOLT_FLASH_MS`. The embers fade out over
/// `BOLT_EMBERS_MS`. The burn is whole until its last `BOLT_FADE_MS`, and
/// fades out over them.
fn bolt_tints(age: f32, left: f32, flicker: f32) -> [Vec4; 3] {
    let cooled = (age / BOLT_FLASH_MS).clamp(0.0, 1.0);
    let flash = FLASH_KEYS
        .windows(2)
        .find(|pair| cooled <= pair[1].0)
        .map_or(Vec3::ZERO, |pair| {
            let ((from_at, from), (to_at, to)) = (pair[0], pair[1]);
            from.lerp(to, (cooled - from_at) / (to_at - from_at))
        });
    let embers_left = (1.0 - age / BOLT_EMBERS_MS).clamp(0.0, 1.0);
    let embers = Vec3::splat(EMBERS_GLOW * embers_left.sqrt() * flicker);
    let burn = (left / BOLT_FADE_MS).clamp(0.0, 1.0);
    [
        Vec4::new(1.0, 1.0, 1.0, burn),
        embers.extend(1.0),
        flash.extend(1.0),
    ]
}

/// Which laid mark to take away so `who` can take one more bolt burn: its
/// oldest bolt burn, once it has `MOST_BOLT_BURNS`. None while it has fewer.
/// A lightsaber's mark is never chosen.
fn make_room(laid: &VecDeque<Laid>, who: SimId) -> Option<u32> {
    let mut burns = laid
        .iter()
        .filter(|laid| laid.who == who && laid.bolt.is_some());
    let oldest = burns.next()?;
    (1 + burns.count() >= MOST_BOLT_BURNS).then_some(oldest.id)
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

    /// A new bolt burn's own three materials, laid at `now` (the view's ms):
    /// its burn, alpha-blended, then its embers and its flash, added. None
    /// when a texture is missing.
    fn bolt_stages(&self, materials: &mut Assets<GlowMaterial>, now: f32) -> Option<BoltStages> {
        let layers = [
            (BOLT_BURN, Surface::Blend),
            (BOLT_EMBERS, Surface::AddOne),
            (BOLT_FLASH, Surface::AddOne),
        ];
        let tints = bolt_tints(0.0, BOLT_LIFE_MS, 1.0);
        let mut made = Vec::with_capacity(layers.len());
        for ((texture, surface), tint) in layers.into_iter().zip(tints) {
            let image = self.images.get(texture)?.clone();
            let mut material = GlowMaterial::mark(image, PUSH_M, surface);
            material.tint = tint;
            made.push(materials.add(material));
        }
        Some(BoltStages {
            materials: made.try_into().ok()?,
            laid_ms: now,
        })
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

/// What the marks read of the fight: the bodies, the blades as drawn, the
/// bolt hits and the hit locations.
type FightReads<'w> = (
    Res<'w, BodyRoots>,
    Res<'w, Wielders>,
    Res<'w, DrawnBlades>,
    Res<'w, BoltHits>,
    Res<'w, Hitboxes>,
);

/// The meshes, skins and stages the marks read and make.
type MarkAssets<'w> = (
    ResMut<'w, Assets<Mesh>>,
    Res<'w, Assets<SkinnedMeshInverseBindposes>>,
    ResMut<'w, Assets<GlowMaterial>>,
);

/// A mark's piece: its entity, which mark it is, its material, its mesh and
/// its skin.
type MarkPieces = (
    Entity,
    &'static Mark,
    &'static MeshMaterial3d<GlowMaterial>,
    &'static Mesh3d,
    Option<&'static SkinnedMesh>,
);

#[allow(clippy::too_many_arguments)]
fn mark(
    presented: Res<PresentedSnapshot>,
    swap: Res<SceneSwap>,
    (mut state, mut looks): (ResMut<State>, ResMut<Looks>),
    (roots, wielders, drawn, records, hitboxes): FightReads,
    (children, scene_meshes, places): (Query<&'static Children>, SceneMeshes, Places),
    (mut meshes, bindposes, mut materials): MarkAssets,
    marks: Query<MarkPieces>,
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
        state.bolts_seen.clear();
        state.last_burn.clear();
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
    // Each bolt burn cools and fades by itself. Its embers and its flash go
    // once they are dark, and they never light again.
    let mut dark = BTreeSet::new();
    for laid in &state.laid {
        let Some(stages) = &laid.bolt else { continue };
        let flicker = 1.0 + EMBERS_FLICKER * noise((now + stages.laid_ms) / 90.0);
        let tints = bolt_tints(now - stages.laid_ms, laid.until - now, flicker);
        for (layer, (material, tint)) in stages.materials.iter().zip(tints).enumerate() {
            if layer > 0 && tint.truncate().max_element() <= 0.0 {
                dark.insert(material.id());
            } else if let Some(mut made) = materials.get_mut(material) {
                made.tint = tint;
            }
        }
    }
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
        hitboxes: &hitboxes,
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
        let posed = world.posed_of(asked.who, &meshes);
        lay(
            (state, &mut gone),
            (asked, None),
            posed,
            now,
            &mut looks,
            (&mut meshes, &mut materials),
            &mut commands,
        );
    }
    // The bolt hits new this frame, oldest first, each burning as the
    // owner's ruling says.
    for hit in &records.0 {
        if !state.bolts_seen.insert((hit.victim, hit.sequence))
            || !bolt_hits::burns(hit, state.last_burn.get(&hit.victim).copied())
        {
            continue;
        }
        let body = crate::cast::entity(presented.0.current(), hit.victim).and_then(|e| e.player);
        let aim = bolt_aim(hit.node, body.as_ref(), hit.from);
        let posed = world.posed_of(hit.victim, &meshes);
        let Some(at) = burn_place(&posed, &meshes, (hit.from, hit.way), aim) else {
            continue;
        };
        let asked = Asked {
            who: hit.victim,
            at,
            ray: hit.way,
        };
        let size = bolt_hits::burn_size(hit.one_handed, hit.power_blast);
        let laid = lay(
            (state, &mut gone),
            (asked, Some(size)),
            posed,
            now,
            &mut looks,
            (&mut meshes, &mut materials),
            &mut commands,
        );
        if laid {
            state.last_burn.insert(hit.victim, hit.ms);
        }
    }
    // A record is handled once. It is forgotten once it leaves the records.
    state.bolts_seen.retain(|&seen| {
        records
            .0
            .iter()
            .any(|hit| (hit.victim, hit.sequence) == seen)
    });
    // Past `ALL_RECORDS`, the oldest bolt burn goes first, then the oldest
    // mark.
    let mut all: usize = state.laid.iter().map(|laid| laid.records).sum();
    while all > ALL_RECORDS {
        let oldest = state
            .laid
            .iter()
            .position(|laid| laid.bolt.is_some())
            .unwrap_or(0);
        let Some(oldest) = state.laid.remove(oldest) else {
            break;
        };
        all -= oldest.records;
        gone.insert(oldest.id);
    }
    for (entity, mark, material, ..) in &marks {
        if gone.contains(&mark.0) || dark.contains(&material.id()) {
            commands.entity(entity).despawn();
        }
    }
    // A dump's check: how far the middle of a mark's triangle is drawn off
    // the bodies' skin (the nearest point of their nearest triangle), the
    // most of any. A mark across a cut, run from the body to the limb, spans
    // open air.
    let off_skin_cm = dump_due.0.then(|| {
        let skin: Vec<[Vec3; 3]> = roots
            .0
            .values()
            .flat_map(|&root| std::iter::once(root).chain(children.iter_descendants(root)))
            .filter_map(|entity| {
                let (mesh3d, _, skin, _) = scene_meshes.get(entity).ok()?;
                as_drawn(&world, &meshes, (entity, mesh3d, skin))
            })
            .flat_map(|drawn| triangles_of(&drawn))
            .collect();
        marks
            .iter()
            .filter_map(|(entity, _, _, mesh3d, skin)| {
                as_drawn(&world, &meshes, (entity, mesh3d, skin))
            })
            .flat_map(|drawn| triangles_of(&drawn))
            .map(|[a, b, c]| {
                let middle = (a + b + c) / 3.0;
                skin.iter()
                    .map(|&triangle| nearest_on_triangle(middle, triangle).distance(middle))
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
            "bolt_now": state.laid.iter().filter(|laid| laid.bolt.is_some()).count(),
            "bolt_laid": state.bolt_count,
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

/// The triangles of a mesh as drawn (`as_drawn`), each as its three corners.
fn triangles_of((at, corners): &(Vec<Vec3>, Vec<usize>)) -> Vec<[Vec3; 3]> {
    corners
        .as_chunks::<3>()
        .0
        .iter()
        .filter_map(|&[a, b, c]| Some([*at.get(a)?, *at.get(b)?, *at.get(c)?]))
        .collect()
}

/// The point of the triangle `[a, b, c]` nearest `point`. It is found by the
/// region of the triangle's plane the point lies over: a corner, an edge or
/// the inside (Ericson, "Real-Time Collision Detection", 5.1.5).
fn nearest_on_triangle(point: Vec3, [a, b, c]: [Vec3; 3]) -> Vec3 {
    let (ab, ac, ap) = (b - a, c - a, point - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = point - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = point - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && d4 - d3 >= 0.0 && d5 - d6 >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let sum = va + vb + vc;
    if sum.abs() < 1e-12 {
        return a;
    }
    a + ab * (vb / sum) + ac * (vc / sum)
}

/// What the marks read the bodies from.
struct Bodies<'a, 'w, 's> {
    roots: &'a BodyRoots,
    children: &'a Query<'w, 's, &'static Children>,
    scene_meshes: &'a SceneMeshes<'w, 's>,
    places: &'a Places<'w, 's>,
    bindposes: &'a Assets<SkinnedMeshInverseBindposes>,
    hitboxes: &'a Hitboxes,
}

/// A mesh of a body as posed now: the mesh entity, its asset, its skin,
/// and its vertices where they are drawn (game space, metres).
struct Posed {
    entity: Entity,
    mesh: Handle<Mesh>,
    skin: Option<SkinnedMesh>,
    at: Vec<Vec3>,
    /// Whether the mesh hangs under the body's head node (a head model's).
    head_mesh: bool,
    /// Whether each vertex is the head's: skinned with a weight of 0.5 or
    /// more to the head node or a joint under it.
    head_vertex: Vec<bool>,
}

impl Bodies<'_, '_, '_> {
    /// Every mesh of `who`'s body as posed now.
    fn posed_of(&self, who: SimId, meshes: &Assets<Mesh>) -> Vec<Posed> {
        let Some(&root) = self.roots.0.get(&who) else {
            return Vec::new();
        };
        // The head node (`Location::Head`'s) and every node under it.
        let head: BTreeSet<Entity> = self
            .hitboxes
            .0
            .get(&who)
            .and_then(|spots| spots[Location::Head as usize])
            .map_or_else(BTreeSet::new, |node| {
                std::iter::once(node)
                    .chain(self.children.iter_descendants(node))
                    .collect()
            });
        self.posed(root, &head, meshes)
    }

    /// Its meshes that keep a copy to read (a body's, skinned; a head's): each
    /// vertex where it is drawn. A weapon's keep none, and are not marked.
    /// `head` is the head node and every node under it.
    fn posed(&self, root: Entity, head: &BTreeSet<Entity>, meshes: &Assets<Mesh>) -> Vec<Posed> {
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
                let joints = (
                    read(Mesh::ATTRIBUTE_JOINT_INDEX),
                    read(Mesh::ATTRIBUTE_JOINT_WEIGHT),
                );
                let at = match skin {
                    Some(skin) => self.skinned(skin, positions, joints)?,
                    None => {
                        let place = self.places.get(entity).ok()?;
                        positions
                            .iter()
                            .map(|p| place.transform_point(Vec3::from_array(*p)))
                            .collect()
                    }
                };
                let head_vertex = match (skin, joints) {
                    (
                        Some(skin),
                        (
                            Some(VertexAttributeValues::Uint16x4(indices)),
                            Some(VertexAttributeValues::Float32x4(weights)),
                        ),
                    ) => {
                        let head_joints: Vec<bool> = skin
                            .joints
                            .iter()
                            .map(|joint| head.contains(joint))
                            .collect();
                        head_weighted(indices, weights, &head_joints)
                    }
                    _ => vec![false; at.len()],
                };
                Some(Posed {
                    entity,
                    mesh: mesh3d.0.clone(),
                    skin: skin.cloned(),
                    at: at.into_iter().map(|point| to_game * point).collect(),
                    head_mesh: head.contains(&entity),
                    head_vertex,
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

/// Lays `asked` on its body (`posed`, its meshes as posed now), if the body
/// has room for it: a lightsaber's mark, or a bolt burn of the size `bolt`
/// names. A bolt burn that covers some skin takes away the body's oldest
/// bolt burn when the body already has `MOST_BOLT_BURNS`, and adds its mark
/// to `gone`. Whether it was laid.
fn lay(
    (state, gone): (&mut State, &mut BTreeSet<u32>),
    (asked, bolt): (Asked, Option<BurnSize>),
    posed: Vec<Posed>,
    now: f32,
    looks: &mut Looks,
    (meshes, materials): (&mut Assets<Mesh>, &mut Assets<GlowMaterial>),
    commands: &mut Commands,
) -> bool {
    let Some(ray) = asked.ray.try_normalize() else {
        return false;
    };
    // Only lightsaber marks count toward `MOST_RECORDS`. Bolt burns make
    // their own room (`make_room`).
    let held: usize = state
        .laid
        .iter()
        .filter(|laid| laid.who == asked.who && laid.bolt.is_none())
        .map(|laid| laid.records)
        .sum();
    if bolt.is_none() && held >= MOST_RECORDS {
        return false;
    }
    let mut rng = crate::view_rng::ja_seed(asked.who.0 ^ state.count, now.to_bits());
    let theta = rng.range(0.0, std::f32::consts::TAU);
    let sizes = bolt.map_or(SIZE, |size| BOLT_SIZE[size as usize]);
    let size = rng.range(sizes.0, sizes.1) * UNIT;
    let (s_size, t_size) = (size, size);
    let (life, depth) = match bolt {
        Some(_) => (BOLT_LIFE_MS, BOLT_DEPTH),
        None => (rng.range(LIFE_MS.0, LIFE_MS.1), DEPTH),
    };
    let (s_axis, t_axis) = mark_axes(ray, theta, (s_size, t_size));
    let axes = (s_axis, t_axis);
    let covered = cover(posed, meshes, (asked.at, ray), axes, depth, bolt.is_some());
    if covered.is_empty() {
        return false;
    }
    let own = match bolt {
        Some(_) => match looks.bolt_stages(materials, now) {
            Some(own) => Some(own),
            None => return false,
        },
        None => None,
    };
    if own.is_some()
        && let Some(oldest) = make_room(&state.laid, asked.who)
    {
        state.laid.retain(|laid| laid.id != oldest);
        gone.insert(oldest);
    }
    // A bolt burn's layers: its burn, then its embers and its flash, which
    // glow.
    let stages = match &own {
        Some(own) => own
            .materials
            .iter()
            .cloned()
            .zip([false, true, true])
            .collect(),
        None => looks.stages(materials),
    };
    let id = state.count;
    let records = covered.len();
    for (posed, gore) in covered {
        let gore = meshes.add(gore);
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
    state.count += 1;
    if own.is_some() {
        state.bolt_count += 1;
    }
    state.laid.push_back(Laid {
        who: asked.who,
        id,
        records,
        until: now + life,
        bolt: own,
    });
    true
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
/// vertices as posed (game metres), one per vertex in its order, and `head`
/// says which of them are the head's (`Posed::head_vertex`; an empty list
/// names none). `depth` is the window of skin marked along the ray from the
/// hit (units). A triangle is kept unless all three of its corners lie off
/// the same side of the mark, or all three in front of the depth window, or
/// all three behind it. A triangle that crosses an edge, or the whole
/// window, is kept whole. A triangle with a corner that is the head's is
/// never kept. None when nothing is covered (or the mesh has no index list).
fn gore(
    mesh: &Mesh,
    (at, head): (&[Vec3], &[bool]),
    (hit, ray): (Vec3, Vec3),
    (s_axis, t_axis): (Vec3, Vec3),
    depth: (f32, f32),
) -> Option<Mesh> {
    let indices = index_list(mesh)?;
    // Each point's coordinates on the mark, and which edges it lies past:
    // one bit each for s below 0 and above 1, t below 0 and above 1, and
    // in front of and behind the depth window.
    let marked: Vec<([f32; 2], u8)> = at
        .iter()
        .map(|&point| {
            let delta = point - hit;
            let s = delta.dot(s_axis) + 0.5;
            let t = delta.dot(t_axis) + 0.5;
            let along = delta.dot(ray) / UNIT;
            let off = u8::from(s < 0.0)
                | u8::from(s > 1.0) << 1
                | u8::from(t < 0.0) << 2
                | u8::from(t > 1.0) << 3
                | u8::from(along < depth.0) << 4
                | u8::from(along > depth.1) << 5;
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
        if shared != 0 || is_head(head, triangle) {
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

/// Whether a triangle (its corners' numbers) has a corner that `head` (one
/// flag per vertex) says is the head's.
fn is_head(head: &[bool], triangle: &[usize]) -> bool {
    triangle
        .iter()
        .any(|&v| head.get(v).copied().unwrap_or(false))
}

/// The mesh's index list, as vertex numbers. None when it has none.
fn index_list(mesh: &Mesh) -> Option<Vec<usize>> {
    Some(match mesh.indices()? {
        Indices::U16(list) => list.iter().map(|&v| usize::from(v)).collect(),
        Indices::U32(list) => list.iter().map(|&v| v as usize).collect(),
    })
}

/// Which vertices are the head's: skinned with a weight of 0.5 or more, in
/// all, to joints that `head_joints` (one flag per joint of the skin) says
/// are the head node or under it. `indices` and `weights` are the mesh's
/// joint numbers and weights, four for each vertex.
fn head_weighted(indices: &[[u16; 4]], weights: &[[f32; 4]], head_joints: &[bool]) -> Vec<bool> {
    indices
        .iter()
        .zip(weights)
        .map(|(joints, weights)| {
            let on_head: f32 = joints
                .iter()
                .zip(weights)
                .filter(|(joint, _)| {
                    head_joints
                        .get(usize::from(**joint))
                        .copied()
                        .unwrap_or(false)
                })
                .map(|(_, weight)| weight)
                .sum();
            on_head >= HEAD_WEIGHT
        })
        .collect()
}

/// The marks of the meshes in `posed` that a mark struck at `hit` along
/// `ray` covers (`gore`), each with its mesh. With `spare_head` set (a bolt
/// burn), a mesh that hangs under the head node is never marked, nor is a
/// triangle with a corner that is the head's. A lightsaber's mark may reach
/// the head.
fn cover(
    posed: Vec<Posed>,
    meshes: &Assets<Mesh>,
    hit: (Vec3, Vec3),
    axes: (Vec3, Vec3),
    depth: (f32, f32),
    spare_head: bool,
) -> Vec<(Posed, Mesh)> {
    posed
        .into_iter()
        .filter(|posed| !(spare_head && posed.head_mesh))
        .filter_map(|posed| {
            let mesh = meshes.get(&posed.mesh)?;
            let head: &[bool] = if spare_head { &posed.head_vertex } else { &[] };
            let made = gore(mesh, (&posed.at, head), hit, axes, depth)?;
            Some((posed, made))
        })
        .collect()
}

/// How far along a line from `from` along `way` (a unit vector) it meets
/// the triangle `[a, b, c]` (game metres), from either side. None when it
/// misses it or meets it behind `from`. This is Möller and Trumbore's test.
fn meet_triangle((from, way): (Vec3, Vec3), [a, b, c]: [Vec3; 3]) -> Option<f32> {
    let (ab, ac) = (b - a, c - a);
    let across = way.cross(ac);
    let det = ab.dot(across);
    if det.abs() < 1e-12 {
        return None;
    }
    let to_from = from - a;
    let u = to_from.dot(across) / det;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let up = to_from.cross(ab);
    let v = way.dot(up) / det;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let along = ac.dot(up) / det;
    (along >= 0.0).then_some(along)
}

/// Where a line from `from` along `way` (a unit vector, game metres) first
/// meets a triangle of `posed`, within `reach` metres: the point, and
/// whether that triangle is the head's (its mesh hangs under the head node,
/// or a corner is the head's).
fn first_meeting(
    posed: &[Posed],
    meshes: &Assets<Mesh>,
    (from, way): (Vec3, Vec3),
    reach: f32,
) -> Option<(Vec3, bool)> {
    let mut first: Option<(f32, bool)> = None;
    for mesh in posed {
        let Some(corners) = meshes.get(&mesh.mesh).and_then(index_list) else {
            continue;
        };
        for triangle in corners.as_chunks::<3>().0 {
            let [Some(&a), Some(&b), Some(&c)] = triangle.map(|v| mesh.at.get(v)) else {
                continue;
            };
            let Some(along) = meet_triangle((from, way), [a, b, c]) else {
                continue;
            };
            if along > reach || first.is_some_and(|(nearest, _)| nearest <= along) {
                continue;
            }
            first = Some((
                along,
                mesh.head_mesh || is_head(&mesh.head_vertex, triangle),
            ));
        }
    }
    first.map(|(along, head)| (from + way * along, head))
}

/// The vertex of `posed` nearest `point` (game metres) that is not the
/// head's, and not on a mesh under the head node.
fn nearest_skin(posed: &[Posed], point: Vec3) -> Option<Vec3> {
    posed
        .iter()
        .filter(|mesh| !mesh.head_mesh)
        .flat_map(|mesh| {
            mesh.at
                .iter()
                .enumerate()
                .filter(|&(v, _)| !mesh.head_vertex.get(v).copied().unwrap_or(false))
                .map(|(_, &vertex)| vertex)
        })
        .min_by(|a, b| a.distance(point).total_cmp(&b.distance(point)))
}

/// Where a bolt along the line through `from` along `way` (a unit vector,
/// game metres) that aimed at `aim` leaves its burn. The line is traced
/// from `TRACE_M` before the point where it passes `aim` to `TRACE_M` past
/// it, so the trace starts in front of the body even when `from` is the
/// bolt as drawn past the body. The burn goes where the line first meets the
/// body. When that is the head, there is no burn. When the line meets
/// nothing, the burn goes on the skin nearest `aim` (`nearest_skin`).
fn burn_place(
    posed: &[Posed],
    meshes: &Assets<Mesh>,
    line: (Vec3, Vec3),
    aim: Vec3,
) -> Option<Vec3> {
    let (_, met) = traced(posed, meshes, line, aim);
    place_of(posed, met, aim)
}

/// The trace of `burn_place`: the point where the line through `from` along
/// `way` passes `aim`, and where the line first meets the body, with
/// whether that is the head.
fn traced(
    posed: &[Posed],
    meshes: &Assets<Mesh>,
    (from, way): (Vec3, Vec3),
    aim: Vec3,
) -> (Vec3, Option<(Vec3, bool)>) {
    let passing = bolt_hits::nearest_on_line((from, way), aim);
    let start = passing - way * TRACE_M;
    let met = first_meeting(posed, meshes, (start, way), 2.0 * TRACE_M);
    (passing, met)
}

/// Where a burn goes, from where its bolt's line first met the body (`met`,
/// from `traced`), as `burn_place` says.
fn place_of(posed: &[Posed], met: Option<(Vec3, bool)>, aim: Vec3) -> Option<Vec3> {
    match met {
        Some((_, true)) => None,
        Some((point, false)) => Some(point),
        None => nearest_skin(posed, aim),
    }
}

/// What a bolt aims at (game metres): the hit location's `node`. A body with
/// no hit location nodes (a droid's) is aimed at its chest, read from its
/// movement state `body`. With neither, the bolt aims at `from`.
fn bolt_aim(node: Option<Vec3>, body: Option<&sim::motion::MotionState>, from: Vec3) -> Vec3 {
    node.or_else(|| body.map(|body| Vec3::from_array(mind::chest(body)) * UNIT))
        .unwrap_or(from)
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

#[cfg(feature = "probes")]
pub mod armour_probes;
#[cfg(feature = "probes")]
mod probes;
#[cfg(feature = "probes")]
pub use probes::probes;
