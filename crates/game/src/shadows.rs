//! Dynamic shadows from capsules. Each character's body is a set of
//! capsules, the ones its ragdoll is fitted with (`ragdoll::Template`). The
//! play view places them on the character's bones every frame
//! (`play_view::cast_shadows`). This module sends them to the GPU each
//! frame, with one bounding sphere per character.
//!
//! `odyssey.wgsl` traces a cone from each fragment toward each light of the
//! light list, against the capsules of the characters near the fragment.
//! The cone is as wide as the light's source, so a shadow's edge widens
//! with its distance from the character. Every light of the list can cast,
//! fight lights included. A character also darkens the floor where its
//! feet meet it.
//!
//! A shadow takes from a surface the light it blocks. On a character, that
//! is the blocked light's own share of its lighting. On a room, whose light
//! is baked into its lightmap, it is an estimate of the light's share of
//! the lightmap. The estimate comes from the light's colour, falloff and
//! facing, as fixed-function lighting would light a character there. A
//! shadow never takes more than the lightmap holds, and never takes the
//! room below its fill light (`odyssey.wgsl`'s `room_shadow`).
//!
//! No shadow is cast through a wall. Each character casts only from the
//! lights that see it past the level's walls and floors (`Sight`). A fight's
//! light moves too fast to test every frame, so it casts from wherever it
//! is. The shader keeps any light out of the rooms not linked with the
//! light's own (`GpuLight::unseen`).

use std::collections::HashMap;

use bevy::asset::uuid_handle;
use bevy::prelude::*;
use bevy::render::render_resource::ShaderType;
use bevy::render::storage::ShaderBuffer;

use crate::material::{AreaLights, GpuLight, LIGHT_COMBAT};

/// The shadows' list, which every Odyssey material reads.
pub const AREA_SHADOWS: Handle<ShaderBuffer> = uuid_handle!("3c8f1d27-94b6-4e0a-b5d3-6a2e7f90c41b");

/// The most characters that cast at once (`odyssey.wgsl`'s `CASTERS`). The
/// ones nearest the camera cast.
pub const CASTERS: usize = 32;

/// The most capsules all the casting characters have together
/// (`odyssey.wgsl`'s `CAPSULES`). A humanoid has 15 and a beast at most 20.
pub const CAPSULES: usize = 512;

/// A light this wide (metres) is the area's sun. It is far off and over all
/// of the area (Taris's is 800 wide, some 290 m above the streets).
pub(crate) const SUN_RADIUS: f32 = 100.0;

/// How far a character or a light moves (metres) before the test of
/// whether the light sees the character is made again.
const SIGHT_MOVE: f32 = 0.5;

/// How far below the top of a character's bounding sphere its sight of a
/// light is tested from (metres). That is about the chest of a standing
/// humanoid.
const SIGHT_BELOW_TOP: f32 = 0.6;

/// The placeables' boxes, which every Odyssey material reads. They are
/// written when a placeable appears, moves or goes, not every frame.
pub const AREA_BOXES: Handle<ShaderBuffer> = uuid_handle!("8a41c6e2-5d73-4f19-a0b8-2e7c93d16f54");

/// The most placeables an area has boxes for (`odyssey.wgsl`'s `BoxList`).
pub const BOXES: usize = 256;

/// The most boxes traced in a frame, the ones nearest the camera
/// (`odyssey.wgsl`'s `NEAR_BOXES`).
pub const NEAR_BOXES: usize = 64;

/// A placeable whose box is no longer than this on its longest side
/// (metres) casts no shadow. A cup, a datapad or a small canister would not
/// show one.
const LEAST_BOX: f32 = 0.3;

/// A placeable whose box is longer than this on its longest side (metres)
/// casts no shadow. One box is a poor stand-in for a whole machine or a
/// ship's hull, and its shadow would fall where the model has none.
const MOST_BOX: f32 = 8.0;

/// A placeable lower than `FLAT_HEIGHT` metres, and lower than
/// `FLAT_SHARE` of its longest side, casts no shadow. Such a placeable is
/// a body lying on the floor, a mat or debris. It fills little of its box,
/// and the box's shadow would be a dark rectangle where the floor shows.
const FLAT_HEIGHT: f32 = 0.5;
const FLAT_SHARE: f32 = 0.35;

/// A lamp's sight of a character is tested from no higher than this above
/// the character's chest (metres). Many rooms' lamps hang above the
/// ceiling their light panels are set in. Leviathan's hall lamps hang 20 to
/// 33 m over its floor. A line to the lamp itself would meet the ceiling,
/// so the line is tested to a point under the lamp. That line still meets
/// any wall between the lamp and the character.
const SIGHT_ABOVE: f32 = 2.0;

/// A character as the shader reads it.
#[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType)]
pub struct GpuCaster {
    /// The sphere holding all its capsules: xyz its centre (Bevy world
    /// space), w its radius.
    pub sphere: Vec4,
    /// x has bit i set when light i of the light list casts its shadow. y
    /// is its first capsule, and z how many capsules it has.
    pub lights: UVec4,
}

/// A capsule as the shader reads it. It is the segment from `a` to `b`
/// (Bevy world space), and `a.w` is its radius.
#[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType)]
pub struct GpuCapsule {
    pub a: Vec4,
    pub b: Vec4,
}

/// A placeable's box as the shader reads it. `centre`'s w is the radius of
/// the sphere holding the box. Each of `x`, `y` and `z` is one of the box's
/// axes (Bevy world space), and its w is half the box's size along it.
#[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType)]
pub struct GpuBox {
    pub centre: Vec4,
    pub x: Vec4,
    pub y: Vec4,
    pub z: Vec4,
}

/// The placeables' boxes, as the shader reads them.
#[derive(Clone, Copy, Debug, PartialEq, ShaderType)]
pub struct BoxList {
    pub count: u32,
    pub boxes: [GpuBox; BOXES],
}

impl Default for BoxList {
    fn default() -> BoxList {
        BoxList {
            count: 0,
            boxes: [GpuBox::default(); BOXES],
        }
    }
}

/// On a placeable's root: its box in its model's own frame, the lowest and
/// the highest corner. The scene puts it on the placeables that would show
/// a shadow (`BoxTally::of`).
#[derive(Component, Clone, Copy, Debug)]
pub struct ShadowBox {
    pub low: Vec3,
    pub high: Vec3,
}

/// How many placeables of a scene got a box, and how many were left out as
/// too small or too large, for the dump.
#[derive(Default)]
pub struct BoxTally {
    boxed: usize,
    small: usize,
    large: usize,
    flat: usize,
}

impl BoxTally {
    /// The box for a placeable whose model spans `bounds` in its own
    /// frame, unless it is too small, too large or too flat to cast as
    /// one.
    pub fn of(&mut self, bounds: Option<(Vec3, Vec3)>) -> Option<ShadowBox> {
        let (low, high) = bounds?;
        let longest = (high - low).max_element();
        if longest <= LEAST_BOX {
            self.small += 1;
            return None;
        }
        if longest > MOST_BOX {
            self.large += 1;
            return None;
        }
        // The model's own frame is the game's, z up.
        let height = high.z - low.z;
        if height < FLAT_HEIGHT && height < FLAT_SHARE * longest {
            self.flat += 1;
            return None;
        }
        self.boxed += 1;
        Some(ShadowBox { low, high })
    }

    /// The counts, for the dump's scene facts.
    pub fn facts(&self) -> serde_json::Value {
        serde_json::json!({
            "boxed": self.boxed,
            "too_small": self.small,
            "too_large": self.large,
            "flat": self.flat,
        })
    }
}

/// The boxes of the shown placeables, as last written to `AREA_BOXES`, by
/// root, for picking the ones near the camera each frame.
#[derive(Resource, Default)]
struct Boxes(Vec<(Entity, GpuBox)>);

/// The casters and their capsules, as the shader reads them.
#[derive(Clone, Copy, Debug, PartialEq, ShaderType)]
pub struct ShadowList {
    /// 1 shows the rooms' shadows alone (`KJK_SHADOW_DEBUG`), for checking
    /// them. 2 shows how they are made (`odyssey.wgsl`'s `room_shadow`).
    pub debug: u32,
    /// How many of `casters` cast.
    pub count: u32,
    /// How many of `near_boxes` are traced.
    pub box_count: u32,
    pub casters: [GpuCaster; CASTERS],
    pub capsules: [GpuCapsule; CAPSULES],
    /// The boxes nearest the camera. Each one's x is its place in
    /// `AREA_BOXES`, and bit i of its y is set when light i of the light
    /// list casts its shadow.
    pub near_boxes: [UVec4; NEAR_BOXES],
}

impl Default for ShadowList {
    fn default() -> ShadowList {
        ShadowList {
            debug: 0,
            count: 0,
            box_count: 0,
            casters: [GpuCaster::default(); CASTERS],
            capsules: [GpuCapsule::default(); CAPSULES],
            near_boxes: [UVec4::ZERO; NEAR_BOXES],
        }
    }
}

/// The options' Shadows and Soft Shadows (`settings`). With Shadows off,
/// nothing casts. With Shadows on, the area's lamps and its sun cast. Soft
/// Shadows on lets a fight's lights cast too. The option keeps the name the
/// games' options screen gives it.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct ShadowSettings {
    pub on: bool,
    pub soft: bool,
}

impl Default for ShadowSettings {
    fn default() -> ShadowSettings {
        ShadowSettings {
            on: true,
            soft: true,
        }
    }
}

/// A character that may cast this frame. Its capsules are in Bevy world
/// space. Each is the segment between its two ends and its radius.
pub struct Caster {
    /// Its body's root, which tells one character from another between
    /// frames.
    pub root: Entity,
    pub capsules: Vec<(Vec3, Vec3, f32)>,
}

/// The characters that may cast this frame, and what tells whether a light
/// sees them. The play view fills it each frame.
#[derive(Resource, Default)]
pub struct Casters {
    pub bodies: Vec<Caster>,
    pub sight: Option<Sight>,
}

/// The level's walls and floors, and its doors as they stand, for the
/// lines from the lights to the characters.
pub struct Sight {
    pub level: sim::Level,
    pub doors: Option<Vec<u8>>,
}

impl Sight {
    /// Whether a line from a light at `from` reaches within `near` of a
    /// character's chest at `to` (Bevy world space) before it meets
    /// anything solid. The level's collision holds the characters' bodies
    /// too, so a line from inside the character would start blocked.
    fn clear(&self, from: Vec3, to: Vec3, near: f32) -> bool {
        let to_game = crate::scene::game_to_bevy().inverse();
        let trace = self.level.line(
            self.doors.as_deref(),
            (to_game * from).to_array(),
            (to_game * to).to_array(),
        );
        let length = from.distance(to);
        !trace.start_solid && trace.fraction * length >= length - near
    }
}

/// A room surface, for the rays that find where blaster bolts and blades
/// meet the rooms.
#[derive(Clone, Copy, Debug)]
pub struct Surface {
    pub corners: [Vec3; 3],
}

/// Each room's surfaces in Bevy world space, by room (`LoadedScene::rooms`).
#[derive(Resource, Default)]
pub struct RoomSurfaces(pub Vec<Vec<Surface>>);

/// `KJK_SHADOW_DEBUG` set. The rooms then show their shadows alone.
struct ShadowDebug(u32);

impl Default for ShadowDebug {
    fn default() -> ShadowDebug {
        ShadowDebug(match std::env::var("KJK_SHADOW_DEBUG").as_deref() {
            Ok("2") => 2,
            Ok(_) => 1,
            Err(_) => 0,
        })
    }
}

/// Whether each light saw each character when last tested. The key is the
/// character's root and the light's place. The value is where the
/// character's chest was, whether the light saw it, and the frame it was
/// last asked for.
#[derive(Default)]
struct Seen(HashMap<(Entity, [u32; 3]), (Vec3, bool, u32)>);

pub struct ShadowsPlugin;

impl Plugin for ShadowsPlugin {
    fn build(&self, app: &mut App) {
        let _ = app
            .world_mut()
            .resource_mut::<Assets<ShaderBuffer>>()
            .insert(&AREA_SHADOWS, ShaderBuffer::from(ShadowList::default()));
        let _ = app
            .world_mut()
            .resource_mut::<Assets<ShaderBuffer>>()
            .insert(&AREA_BOXES, ShaderBuffer::from(BoxList::default()));
        app.init_resource::<RoomSurfaces>()
            .init_resource::<ShadowSettings>()
            .init_resource::<Casters>()
            .init_resource::<Boxes>()
            .add_systems(
                PostUpdate,
                (place_boxes, upload)
                    .chain()
                    .after(bevy::transform::TransformSystems::Propagate)
                    .after(crate::play_view::ShadowsCast),
            );
    }
}

fn is_sun(light: &GpuLight) -> bool {
    light.position_radius.w >= SUN_RADIUS
}

/// The sphere holding `capsules`: its centre and radius.
fn bounds(capsules: &[(Vec3, Vec3, f32)]) -> (Vec3, f32) {
    let (low, high) = capsules.iter().fold(
        (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
        |(low, high), &(a, b, r)| (low.min(a.min(b) - r), high.max(a.max(b) + r)),
    );
    let centre = (low + high) * 0.5;
    let radius = capsules
        .iter()
        .map(|&(a, b, r)| centre.distance(a).max(centre.distance(b)) + r)
        .fold(0.0, f32::max);
    (centre, radius)
}

/// The capsule a ragdoll's solid stands for, in its node's frame. A
/// capsule is itself. A droid's box is a capsule along its longest side,
/// as wide as the mean of its other two sides.
pub(crate) fn capsule_of(solid: &ragdoll::Solid) -> (Vec3, Vec3, f32) {
    match *solid {
        ragdoll::Solid::Capsule {
            ends: [a, b],
            radius,
        } => (a, b, radius),
        ragdoll::Solid::Cuboid { centre, turn, half } => {
            let mut sides = [(half.x, Vec3::X), (half.y, Vec3::Y), (half.z, Vec3::Z)];
            sides.sort_by(|a, b| b.0.total_cmp(&a.0));
            let radius = (sides[1].0 + sides[2].0) * 0.5;
            let along = turn * sides[0].1 * (sides[0].0 - radius).max(0.0);
            (centre - along, centre + along, radius)
        }
    }
}

/// Which lights of `lights` cast a character's shadow, as bits. A light
/// casts when it reaches the character's sphere and, unless it is a
/// fight's light, when it sees the character's chest (`seen`). A light
/// that only brightens the ambient casts none. A fight's light casts only
/// with `fights` on.
fn casting_lights(
    lights: &[GpuLight],
    (root, centre, radius): (Entity, Vec3, f32),
    fights: bool,
    (sight, seen, frame): (Option<&Sight>, &mut Seen, u32),
    (tested, blocked): (&mut usize, &mut usize),
) -> u32 {
    let chest = centre + Vec3::Y * (radius - SIGHT_BELOW_TOP).max(0.0);
    let mut bits = 0u32;
    for (index, light) in lights.iter().enumerate() {
        if light.kind.x != 0 {
            continue;
        }
        let fight = light.kind.z == LIGHT_COMBAT;
        if fight && !fights {
            continue;
        }
        let at = light.position_radius.truncate();
        let reach = light.position_radius.w + light.way.w + radius;
        if at.distance_squared(centre) >= reach * reach {
            continue;
        }
        let clear = fight
            || sight.is_none_or(|sight| {
                let key = (root, at.to_array().map(f32::to_bits));
                match seen.0.get_mut(&key) {
                    Some((was, clear, asked)) if was.distance(chest) < SIGHT_MOVE => {
                        *asked = frame;
                        *clear
                    }
                    _ => {
                        *tested += 1;
                        // The sun is tested along its direction, not to
                        // where it hangs far over the area. A lamp is
                        // tested to a point under it (`SIGHT_ABOVE`).
                        let to = if is_sun(light) {
                            chest + (at - chest).normalize_or_zero() * 60.0
                        } else {
                            at.with_y(at.y.min(chest.y + SIGHT_ABOVE))
                        };
                        let clear = sight.clear(to, chest, radius);
                        seen.0.insert(key, (chest, clear, frame));
                        clear
                    }
                }
            });
        if clear {
            bits |= 1 << index;
        } else {
            *blocked += 1;
        }
    }
    bits
}

/// A placeable's box in Bevy world space, placed by its root.
fn world_box(shadow: &ShadowBox, placed: &GlobalTransform) -> GpuBox {
    let (scale, turn, _) = placed.to_scale_rotation_translation();
    let half = (shadow.high - shadow.low) * 0.5 * scale;
    let centre = placed.transform_point((shadow.low + shadow.high) * 0.5);
    GpuBox {
        centre: centre.extend(half.length()),
        x: (turn * Vec3::X).extend(half.x),
        y: (turn * Vec3::Y).extend(half.y),
        z: (turn * Vec3::Z).extend(half.z),
    }
}

/// Writes the shown placeables' boxes to `AREA_BOXES` when one appears,
/// moves, or is hidden or goes. Most placeables never move, so the boxes
/// are written once when an area is shown.
fn place_boxes(
    placed: Query<(Entity, &ShadowBox, &GlobalTransform, &InheritedVisibility)>,
    settings: Res<ShadowSettings>,
    mut boxes: ResMut<Boxes>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
) {
    let now: Vec<(Entity, GpuBox)> = placed
        .iter()
        .filter(|(_, _, _, shown)| settings.on && shown.get())
        .map(|(entity, shadow, at, _)| (entity, world_box(shadow, at)))
        .take(BOXES)
        .collect();
    if now == boxes.0 {
        return;
    }
    let mut list = Box::new(BoxList {
        count: now.len() as u32,
        ..default()
    });
    for (slot, (_, gpu)) in list.boxes.iter_mut().zip(&now) {
        *slot = *gpu;
    }
    if let Some(mut buffer) = buffers.get_mut(&AREA_BOXES) {
        buffer.set_data(*list);
    }
    boxes.0 = now;
}

/// Writes the casters nearest the camera and their capsules for the
/// shader.
#[allow(clippy::too_many_arguments)]
fn upload(
    (casters, boxes): (Res<Casters>, Res<Boxes>),
    area: Res<AreaLights>,
    settings: Res<ShadowSettings>,
    cameras: Query<&GlobalTransform, With<crate::scene::FlyCamera>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut facts: ResMut<console::DumpFacts>,
    (mut last, mut seen, mut frame): (Local<Box<ShadowList>>, Local<Seen>, Local<u32>),
    debug: Local<ShadowDebug>,
) {
    *frame = frame.wrapping_add(1);
    let eye = cameras
        .single()
        .map_or(Vec3::ZERO, GlobalTransform::translation);
    let lights = &area.0.lights[..(area.0.count as usize).min(area.0.lights.len())];
    let mut list = Box::new(ShadowList {
        debug: debug.0,
        ..default()
    });
    // The lights about the casters nearest the camera, for the dump.
    let mut about: Vec<serde_json::Value> = Vec::new();
    let mut near: Vec<(&Caster, Vec3, f32)> = if settings.on {
        casters
            .bodies
            .iter()
            .filter(|caster| !caster.capsules.is_empty())
            .map(|caster| {
                let (centre, radius) = bounds(&caster.capsules);
                (caster, centre, radius)
            })
            .collect()
    } else {
        Vec::new()
    };
    near.sort_by(|a, b| {
        a.1.distance_squared(eye)
            .total_cmp(&b.1.distance_squared(eye))
    });
    let (mut placed, mut tested, mut blocked) = (0usize, 0usize, 0usize);
    for (caster, centre, radius) in near {
        let count = caster.capsules.len();
        if list.count as usize >= CASTERS || placed + count > CAPSULES {
            break;
        }
        let bits = casting_lights(
            lights,
            (caster.root, centre, radius),
            settings.soft,
            (casters.sight.as_ref(), &mut seen, *frame),
            (&mut tested, &mut blocked),
        );
        if facts.wanted() && about.len() < 4 {
            let mut row = lights_about(lights, centre, radius, bits);
            let round = |v: Vec3| v.to_array().map(|x| (x * 100.0).round() / 100.0);
            row["capsules"] = caster
                .capsules
                .iter()
                .map(|&(a, b, r)| {
                    serde_json::json!([round(a), round(b), (r * 100.0).round() / 100.0])
                })
                .collect();
            about.push(row);
        }
        list.casters[list.count as usize] = GpuCaster {
            sphere: centre.extend(radius),
            lights: UVec4::new(bits, placed as u32, count as u32, 0),
        };
        for (slot, &(a, b, r)) in list.capsules[placed..].iter_mut().zip(&caster.capsules) {
            *slot = GpuCapsule {
                a: a.extend(r),
                b: b.extend(0.0),
            };
        }
        placed += count;
        list.count += 1;
    }
    // The boxes nearest the camera, each with the lights that cast its
    // shadow.
    let mut near_boxes: Vec<(usize, Vec3, f32, Entity)> = boxes
        .0
        .iter()
        .enumerate()
        .map(|(index, (root, gpu))| (index, gpu.centre.truncate(), gpu.centre.w, *root))
        .collect();
    near_boxes.sort_by(|a, b| {
        a.1.distance_squared(eye)
            .total_cmp(&b.1.distance_squared(eye))
    });
    for (slot, &(index, centre, radius, root)) in list.near_boxes.iter_mut().zip(&near_boxes) {
        let bits = casting_lights(
            lights,
            (root, centre, radius),
            settings.soft,
            (casters.sight.as_ref(), &mut seen, *frame),
            (&mut tested, &mut blocked),
        );
        *slot = UVec4::new(index as u32, bits, 0, 0);
        list.box_count += 1;
    }
    // A test not asked for again this frame is of a character or a light
    // gone.
    let now = *frame;
    seen.0.retain(|_, (_, _, asked)| *asked == now);
    if **last != *list
        && let Some(mut buffer) = buffers.get_mut(&AREA_SHADOWS)
    {
        buffer.set_data(*list);
        *last = list;
    }
    if facts.wanted() {
        let shown = &last;
        // The boxes nearest the camera. Each gives its centre and its half
        // sizes (Bevy world space, metres).
        let round = |x: f32| (x * 100.0).round() / 100.0;
        let box_list: Vec<serde_json::Value> = near_boxes
            .iter()
            .take(8)
            .map(|near| {
                let gpu = boxes.0[near.0].1;
                let centre = gpu.centre.truncate().to_array().map(round);
                let half = [gpu.x.w, gpu.y.w, gpu.z.w].map(round);
                serde_json::json!([centre, half])
            })
            .collect();
        let pairs: u32 = shown.casters[..shown.count as usize]
            .iter()
            .map(|caster| caster.lights.x.count_ones())
            .sum();
        facts.0.insert(
            "shadows".into(),
            serde_json::json!({
                "candidates": casters.bodies.len(),
                "casters": shown.count,
                "capsules": placed,
                "boxes": boxes.0.len(),
                "near_boxes": shown.box_count,
                "box_list": box_list,
                "light_pairs": pairs,
                "sight_tests": tested,
                "blocked_pairs": blocked,
                "nearest": about,
            }),
        );
    }
}

/// The lights within reach of a caster, for the dump. Each gives its
/// place in the light list, its distance from the caster's centre and its
/// height over it (metres), its radius, whether it is a fight's light, and
/// whether it casts the caster's shadow.
fn lights_about(lights: &[GpuLight], centre: Vec3, radius: f32, bits: u32) -> serde_json::Value {
    let round = |x: f32| (x * 100.0).round() / 100.0;
    let rows: Vec<serde_json::Value> = lights
        .iter()
        .enumerate()
        .filter(|(_, light)| {
            let reach = light.position_radius.w + light.way.w + radius;
            light.kind.x == 0 && light.position_radius.truncate().distance(centre) < reach
        })
        .map(|(index, light)| {
            let at = light.position_radius.truncate();
            serde_json::json!([
                index,
                round(at.distance(centre)),
                round(at.y - centre.y),
                round(light.position_radius.w),
                light.kind.z == LIGHT_COMBAT,
                bits & (1 << index) != 0,
            ])
        })
        .collect();
    serde_json::json!({ "centre": centre.to_array().map(round), "radius": round(radius), "lights": rows })
}

#[cfg(feature = "probes")]
pub mod probes;

/// A room model's surfaces in Bevy world space, at rest. `frame` places it
/// in the game's space, and `to_bevy` turns that into Bevy's.
pub fn surfaces(placed: &crate::viewer::Placed, to_bevy: Quat, frame: Mat4) -> Vec<Surface> {
    let world = Mat4::from_quat(to_bevy) * frame;
    placed
        .ir
        .meshes
        .iter()
        .filter(|mesh| mesh.skin.is_none())
        .flat_map(|mesh| {
            let node = world * crate::scene::to_mat4(&placed.ir.nodes[mesh.node].world);
            let at = move |index: u32| {
                mesh.positions
                    .get(index as usize)
                    .map(|p| node.transform_point3(Vec3::from_array(*p)))
            };
            mesh.indices
                .as_chunks::<3>()
                .0
                .iter()
                .filter_map(move |&[a, b, c]| Some([at(a)?, at(b)?, at(c)?]))
        })
        .map(|corners| Surface { corners })
        .collect()
}

/// Where along `direction` from `origin` the ray meets `triangle`
/// (Möller–Trumbore), on either face.
pub(crate) fn ray_hit(origin: Vec3, direction: Vec3, [a, b, c]: &[Vec3; 3]) -> Option<f32> {
    let (ab, ac) = (*b - *a, *c - *a);
    let p = direction.cross(ac);
    let det = ab.dot(p);
    if det.abs() < 1e-8 {
        return None;
    }
    let to_origin = origin - *a;
    let u = to_origin.dot(p) / det;
    let q = to_origin.cross(ab);
    let v = direction.dot(q) / det;
    let t = ac.dot(q) / det;
    (u >= 0.0 && v >= 0.0 && u + v <= 1.0 && t > 0.0).then_some(t)
}
