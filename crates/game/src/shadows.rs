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

/// The casters and their capsules, as the shader reads them.
#[derive(Clone, Copy, Debug, PartialEq, ShaderType)]
pub struct ShadowList {
    /// 1 shows the rooms' shadows alone (`KJK_SHADOW_DEBUG`), for checking
    /// them. 2 shows how they are made (`odyssey.wgsl`'s `room_shadow`).
    pub debug: u32,
    /// How many of `casters` cast.
    pub count: u32,
    pub casters: [GpuCaster; CASTERS],
    pub capsules: [GpuCapsule; CAPSULES],
}

impl Default for ShadowList {
    fn default() -> ShadowList {
        ShadowList {
            debug: 0,
            count: 0,
            casters: [GpuCaster::default(); CASTERS],
            capsules: [GpuCapsule::default(); CAPSULES],
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
        app.init_resource::<RoomSurfaces>()
            .init_resource::<ShadowSettings>()
            .init_resource::<Casters>()
            .add_systems(
                PostUpdate,
                upload
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

/// Writes the casters nearest the camera and their capsules for the
/// shader.
#[allow(clippy::too_many_arguments)]
fn upload(
    casters: Res<Casters>,
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
