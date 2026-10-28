//! Dynamic shadows: characters, doors and placeables cast them from the
//! area's lights onto the rooms and onto one another.
//!
//! The games draw a character's shadow from one light, the nearest whose
//! light header carries the shadow flag, fading it in and out as that light
//! changes (as reone does, at 2 a second). Few
//! lights carry the flag (Taris's upper city has one of its 70: its sun),
//! so here any light that lights more than ambience may cast, a flagged one
//! first, each fading the same way: up to `POINT_SLOTS` lamps about the
//! player at once, and the area's sun. A lamp casts only from above the
//! player's head, so no shadow is thrown sideways across a room from a lamp
//! on the wall at knee height.
//!
//! Bevy renders the shadow maps, from lights that light nothing (no
//! material here reads Bevy's lights): a map per lamp slot, from a spot
//! light at the lamp facing straight down, and the sun's cascades, from a
//! directional light turned along it. A lamp casts only from overhead, so
//! its downward cone holds what a cube map's six faces would (a cube map is
//! six views to render, each costing about as much as a spot light's one). `odyssey.wgsl` reads the maps through
//! the slots' list (`AREA_SHADOWS`): a soft, percentage-closer filter whose
//! penumbra widens with the gap between caster and receiver, and whose
//! shadow fades out as that gap grows, as a shadow in a lightmapped room's
//! soft light would.
//!
//! Each light renders two maps: one of the casters, and one of the rooms
//! alone, which says where the light truly falls. A caster's shadow lies
//! only where the rooms let the light through: not through a wall from a
//! lamp in the next room, nor from the sun under a roof.
//!
//! What casts is marked when its mesh is spawned (`ShadowCaster`, and
//! `ShadowRoom` for a room's solid surfaces); anything else drawn with the
//! Odyssey material is kept out of the maps. A caster goes into them only
//! while a casting light reaches it: skinned meshes are never
//! frustum-culled, so every character in the area would otherwise be drawn
//! into every face of every map.

use bevy::asset::uuid_handle;
use bevy::camera::visibility::RenderLayers;
use bevy::light::{CascadeShadowConfigBuilder, DirectionalLightShadowMap, NotShadowCaster};
use bevy::prelude::*;
use bevy::render::render_resource::ShaderType;
use bevy::render::storage::ShaderBuffer;

use crate::material::{GpuLight, OdysseyMaterial};

/// The lamps that cast at once (`odyssey.wgsl`'s `POINT_SLOTS`): each
/// renders two maps a frame.
const POINT_SLOTS: usize = 2;

/// The slots: the lamps', then the sun's (`odyssey.wgsl`'s `SHADOW_SLOTS`).
pub const SLOTS: usize = POINT_SLOTS + 1;
const SUN: usize = POINT_SLOTS;

/// The slots' list, which every Odyssey material reads.
pub const AREA_SHADOWS: Handle<ShaderBuffer> = uuid_handle!("3c8f1d27-94b6-4e0a-b5d3-6a2e7f90c41b");

/// How fast a shadow fades in or out as its light changes: a slot's strength
/// moves this much a second (half a second from none to full). Our chosen
/// speed.
const FADE_SPEED: f32 = 2.0;

/// A lamp casts only from this high above the player's feet (metres),
/// about the head: lower, the shadow of the body stretches across the
/// floor.
const ABOVE_FEET: f32 = 1.6;

/// A light this wide (metres) is the area's sun: far off and over all of it
/// (Taris's, 800, some 290 above the streets).
const SUN_RADIUS: f32 = 100.0;

/// A light taking a slot from one that holds it must score this much
/// better, so two lights alike do not trade the slot back and forth.
const HYSTERESIS: f32 = 1.25;

/// A light whose header asks it to cast counts this much stronger.
const FLAGGED: f32 = 2.0;

/// The size of a lamp's source (metres): the penumbra's width follows it.
/// Room lights are lamps and light panels, not points.
const LAMP_RADIUS: f32 = 0.25;

/// The sun's size as an angle (radians): wider than the sun's own, for a
/// shadow as soft as the games' lightmapped light.
const SUN_ANGLE: f32 = 0.06;

/// Where a free lamp slot's light waits, lighting and shadowing nothing.
const PARKED: Vec3 = Vec3::new(0.0, -100_000.0, 0.0);

/// Each of the sun's cascades and each lamp's map, texels
/// (`odyssey.wgsl`'s `MAP_SIZE`).
const MAP_SIZE: usize = 2048;

/// A lamp's cone, from straight down (radians): to the lowest a lamp
/// casts from over the chest (`LOWEST_SINE`, some 73° from straight down).
const LAMP_CONE: f32 = 1.28;

/// The sun's casters' cascades: each is a view to render, as costly as a
/// lamp's map.
const SUN_CASCADES: usize = 2;

/// The render layers the maps are drawn from: the casters', and the rooms'
/// (both are also on the cameras' layer 0).
const CASTER_LAYER: usize = 3;
const ROOM_LAYER: usize = 4;

/// A lamp's rooms-only map is told from its casters' map by the light's
/// size, which nothing here otherwise reads (`odyssey.wgsl`'s
/// `lamp_light`).
const ROOM_MAP_TAG: f32 = 1.0;

/// A sun's rooms-only cascades are told from its casters' by its colour
/// (`odyssey.wgsl`'s `sun_light`); it lights nothing at this illuminance.
const SUN_TAG_ILLUMINANCE: f32 = 1e-4;

/// A slot as the shader reads it.
#[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType)]
pub struct GpuShadow {
    /// Where its maps are cast from (Bevy world space), and the light's
    /// radius.
    pub position_radius: Vec4,
    /// Where the light itself is, as the material's light list has it.
    pub light_at: Vec4,
    /// x: strength (0 to 1, fading), y: the source's size (a lamp's radius
    /// in metres, the sun's angle).
    pub params: Vec4,
}

/// The characters whose feet are grounded at once (`odyssey.wgsl`'s
/// `FEET`), the nearest the player.
const FEET: usize = 16;

/// The slots and the feet, as the shader reads them.
#[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType)]
pub struct ShadowList {
    /// 1 shows the shadows alone (`KJK_SHADOW_DEBUG`), for checking them.
    pub debug: u32,
    /// 1: soft edges, 0: hard ones.
    pub soft: u32,
    pub feet_count: u32,
    pub slots: [GpuShadow; SLOTS],
    /// Where each character stands (Bevy world space).
    pub feet: [Vec4; FEET],
}

/// The options' Shadows and Soft Shadows (`settings`): shadows cast at
/// all, and with soft edges or hard ones.
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

/// A mesh that casts a shadow: a character's, a door's or a placeable's.
#[derive(Component)]
pub struct ShadowCaster;

/// The layers a camera of the world sees: its own, and the maps' (on which
/// nothing else is drawn).
pub fn view_layers() -> RenderLayers {
    RenderLayers::from_layers(&[0, CASTER_LAYER, ROOM_LAYER])
}

/// A room's solid surface: it blocks the lights in the rooms-only maps.
#[derive(Component)]
pub struct ShadowRoom;

/// A light of the scene that may cast (one that lights more than
/// ambience), in Bevy world space; or a ceiling light standing in for a
/// room's light panels (`ceiling_lights`).
#[derive(Clone, Copy, Debug)]
pub struct CastingLight {
    /// Which light: its place in `ShadowLights`, or a ceiling light's
    /// (`CEILING_ID` on).
    pub id: u32,
    pub light: GpuLight,
    /// Its header carries the shadow flag.
    pub flagged: bool,
    /// The room whose model holds it (`LoadedScene::rooms`); none for a
    /// placeable's.
    pub room: Option<usize>,
}

impl CastingLight {
    /// Where its shadows are cast from: itself, or, when it hangs above
    /// the ceiling over the player, the same place just under that
    /// ceiling. Rooms' lamps often hang above the ceiling their light
    /// panels are set in (the Endar Spire's quarters', some 8 m over a 3 m
    /// room), where the rooms' maps would keep all their light off the
    /// room.
    /// A ceiling light always hangs just under the ceiling.
    fn casts_from(&self, focus: &Focus) -> Vec3 {
        let at = self.light.position_radius.truncate();
        match focus.ceiling {
            Some(ceiling)
                if !is_sun(&self.light)
                    && (self.is_ceiling_light() || at.y > ceiling - UNDER_CEILING) =>
            {
                Vec3::new(at.x, ceiling - UNDER_CEILING, at.z)
            }
            _ => at,
        }
    }

    fn is_ceiling_light(&self) -> bool {
        self.id >= CEILING_ID
    }
}

/// A room surface, for the rays that find ceilings and walls.
#[derive(Clone, Copy, Debug)]
pub struct Surface {
    pub corners: [Vec3; 3],
    /// Its front faces down: a ceiling's.
    pub down: bool,
}

/// Each room's surfaces in Bevy world space, by room (`LoadedScene::rooms`).
#[derive(Resource, Default)]
pub struct RoomSurfaces(pub Vec<Vec<Surface>>);

/// Ceiling lights' ids (`CastingLight::id`) from here on.
const CEILING_ID: u32 = 1 << 30;

/// The ceiling lights stand on a grid this wide (metres), as a ship's or a
/// building's light panels do; four about the player at a time.
const CEILING_GRID: f32 = 4.0;

/// A ceiling light's reach (metres) and multiplier: dimmer than a lamp of
/// the room, which casts first when one reaches the player.
const CEILING_RADIUS: f32 = 7.0;
const CEILING_MULTIPLIER: f32 = 0.5;

/// How much of a lamp's shadow a ceiling light's is (`odyssey.wgsl`'s
/// `lamp_presence`): the panels' light is spread wide.
const CEILING_WEIGHT: f32 = 1.0;

/// The ceiling lights about the player: the corners of the grid's cell
/// they stand in, just under the ceiling over them (none under the sky).
fn ceiling_lights(focus: &Focus) -> Vec<CastingLight> {
    let Some(ceiling) = focus.ceiling else {
        return Vec::new();
    };
    // It reaches the floor under a high ceiling too (the Sith Academy's
    // halls are 10 m high).
    let radius = CEILING_RADIUS.max(ceiling - focus.feet.y + CEILING_RADIUS / 2.0);
    let cell = (focus.feet.xz() / CEILING_GRID).floor();
    let (x, z) = (cell.x as i32, cell.y as i32);
    [(0, 0), (1, 0), (0, 1), (1, 1)]
        .into_iter()
        .map(|(dx, dz)| {
            let (gx, gz) = (x + dx, z + dz);
            let corner = Vec2::new(gx as f32, gz as f32) * CEILING_GRID;
            // An id per grid point, stable while the player stays near.
            let id = CEILING_ID | ((gx as u32 & 0x7fff) << 15) | (gz as u32 & 0x7fff);
            CastingLight {
                id,
                light: GpuLight {
                    position_radius: Vec4::new(corner.x, focus.feet.y, corner.y, radius),
                    color_multiplier: Vec4::new(1.0, 1.0, 1.0, CEILING_MULTIPLIER),
                    kind: UVec4::ZERO,
                },
                flagged: false,
                room: focus.room,
            }
        })
        .collect()
}

/// How far under a ceiling (metres) a lamp above it casts from.
const UNDER_CEILING: f32 = 0.25;

/// A ceiling counts from this high over the feet (metres): lower, it is a
/// door frame or a shelf passed under.
const LOWEST_CEILING: f32 = 2.2;

/// Higher than this over the feet (metres), a surface is a sky dome, not a
/// ceiling (Kashyyyk's, some 150 m up).
const HIGHEST_CEILING: f32 = 15.0;

/// The ceiling's height over the feet (metres) where none is found and the
/// player stands in no room.
const ASSUMED_CEILING: f32 = 3.5;

/// How fast (a second) the ceiling followed eases to a new one.
const CEILING_EASE: f32 = 4.0;

/// The scene's lights that may cast.
#[derive(Resource, Default)]
pub struct ShadowLights(pub Vec<CastingLight>);

/// A slot's light, its strength, and whether it is still wanted (else it
/// fades out and frees the slot).
#[derive(Clone, Copy, Debug)]
struct Held {
    light: CastingLight,
    strength: f32,
    wanted: bool,
}

#[derive(Resource, Default)]
struct Slots([Option<Held>; SLOTS]);

/// A spot light that renders a lamp slot's map: its casters', or its
/// rooms' alone.
#[derive(Component)]
struct LampLight(usize);

/// A directional light that renders the sun's cascades.
#[derive(Component)]
struct SunLight;

/// `KJK_SHADOW_DEBUG` set: the rooms show their shadows alone.
struct ShadowDebug(u32);

impl Default for ShadowDebug {
    fn default() -> ShadowDebug {
        ShadowDebug(u32::from(std::env::var_os("KJK_SHADOW_DEBUG").is_some()))
    }
}

/// Where shadows are picked for: the player's feet, else under the camera;
/// the room they stand in, the rooms it sees (its VIS entry), and the
/// height of the ceiling over them (eased), none under the sky.
#[derive(Resource, Default)]
struct Focus {
    feet: Vec3,
    room: Option<usize>,
    seen: Vec<usize>,
    ceiling: Option<f32>,
    /// The rooms keep the sky off the player (found again as they move
    /// `CLEARANCE_MOVE`, from where it was last found).
    sheltered: bool,
    sheltered_at: Option<Vec3>,
}

/// Where the characters nearest the player stand, nearest first.
#[derive(Resource, Default)]
struct Feet(Vec<Vec3>);

pub struct ShadowsPlugin;

impl Plugin for ShadowsPlugin {
    fn build(&self, app: &mut App) {
        let _ = app
            .world_mut()
            .resource_mut::<Assets<ShaderBuffer>>()
            .insert(&AREA_SHADOWS, ShaderBuffer::from(ShadowList::default()));
        app.insert_resource(DirectionalLightShadowMap { size: MAP_SIZE })
            .init_resource::<ShadowLights>()
            .init_resource::<RoomSurfaces>()
            .init_resource::<Slots>()
            .init_resource::<Focus>()
            .init_resource::<Feet>()
            .init_resource::<ShadowSettings>()
            .add_systems(Startup, spawn_shadow_lights)
            .add_systems(
                Update,
                (
                    focus,
                    gather_feet,
                    pick,
                    upload,
                    report,
                    place_shadow_lights,
                    gate_casters,
                )
                    .chain()
                    .after(crate::scene::LightsPicked),
            )
            .add_systems(PostUpdate, (keep_out_of_maps, layer_casters_and_rooms));
    }
}

fn spawn_shadow_lights(mut commands: Commands) {
    for (layer, tag) in [(CASTER_LAYER, 0.0), (ROOM_LAYER, ROOM_MAP_TAG)] {
        for slot in 0..POINT_SLOTS {
            commands.spawn((
                LampLight(slot),
                SpotLight {
                    intensity: 0.0,
                    range: 0.01,
                    radius: tag,
                    shadow_maps_enabled: true,
                    // Texel-scaled along the normal; the depth bias is
                    // metres.
                    shadow_depth_bias: 0.02,
                    shadow_normal_bias: 0.9,
                    shadow_map_near_z: 0.05,
                    outer_angle: LAMP_CONE,
                    inner_angle: LAMP_CONE,
                    ..default()
                },
                RenderLayers::layer(layer),
                Transform::from_translation(PARKED).looking_to(Vec3::NEG_Y, Vec3::X),
            ));
        }
    }
    // The rooms' map only says whether the sun reaches a point at all (a
    // roof over it), which one cascade over the whole reach answers.
    for (layer, colour, cascades) in [
        (CASTER_LAYER, Color::linear_rgb(1.0, 0.0, 0.0), SUN_CASCADES),
        (ROOM_LAYER, Color::linear_rgb(0.0, 1.0, 0.0), 1),
    ] {
        commands.spawn((
            SunLight,
            DirectionalLight {
                color: colour,
                illuminance: SUN_TAG_ILLUMINANCE,
                shadow_maps_enabled: false,
                shadow_depth_bias: 0.03,
                shadow_normal_bias: 1.2,
                ..default()
            },
            CascadeShadowConfigBuilder {
                num_cascades: cascades,
                minimum_distance: 0.1,
                first_cascade_far_bound: 8.0,
                maximum_distance: 60.0,
                overlap_proportion: 0.25,
            }
            .build(),
            RenderLayers::layer(layer),
            Transform::default(),
        ));
    }
}

/// The player's feet and room (`scene::PlayerAt`), the rooms that room sees,
/// and the ceiling over the player.
fn focus(
    (player, player_room): (Res<crate::scene::PlayerAt>, Res<crate::scene::PlayerRoom>),
    scene: Option<Res<crate::scene::SceneContent>>,
    (surfaces, time, lights): (Res<RoomSurfaces>, Res<Time>, Res<ShadowLights>),
    mut focus: ResMut<Focus>,
) {
    let Some(at) = player.0 else { return };
    let Some(scene) = scene else { return };
    let at = crate::scene::game_to_bevy() * at;
    focus.feet = at;
    let rooms = &scene.0.rooms;
    let room = player_room.0;
    if room != focus.room {
        focus.seen = room.map_or(Vec::new(), |here| {
            (0..rooms.len())
                .filter(|&other| {
                    rooms[here]
                        .visible
                        .iter()
                        .any(|name| name.eq_ignore_ascii_case(&rooms[other].name))
                })
                .collect()
        });
        focus.room = room;
    }
    // The ceiling straight over the chest, in the player's room.
    let chest = at + Vec3::Y * 1.2;
    // Off every room's floor (a room whose walkmesh did not read: the
    // Taris cantina's), any room's ceiling over the chest.
    let over = match room {
        Some(room) => surfaces.0.get(room).map_or(&[][..], Vec::as_slice).iter().collect::<Vec<_>>(),
        None => surfaces.0.iter().flatten().collect(),
    }
    .into_iter()
        .filter(|surface| surface.down)
        .filter_map(|surface| ray_hit(chest, Vec3::Y, &surface.corners))
        .map(|distance| chest.y + distance)
        .filter(|height| (at.y + LOWEST_CEILING..=at.y + HIGHEST_CEILING).contains(height))
        .min_by(f32::total_cmp)
        // Sheltered without a ceiling over them (the Taris cantina has
        // none, never seen from below), a room's height is taken.
        .or_else(|| sheltered(&mut focus, chest, &lights, &surfaces).then_some(at.y + ASSUMED_CEILING));
    focus.ceiling = match (focus.ceiling, over) {
        (Some(was), Some(now)) => {
            let ease = (CEILING_EASE * time.delta_secs()).min(1.0);
            Some(was + (now - was) * ease)
        }
        (_, now) => now,
    };
}

/// How far (metres) toward the sun a room's surface shelters the player:
/// farther, it is the land about (a cliff, a distant building).
const SHELTER_REACH: f32 = 30.0;

/// Whether the rooms keep the sky off the player: no sun, or a room's
/// surface between the chest and the strongest sun (any room's: the walls
/// about an open-topped room shade it).
fn sheltered(
    focus: &mut Focus,
    chest: Vec3,
    lights: &Res<ShadowLights>,
    surfaces: &RoomSurfaces,
) -> bool {
    if lights.is_changed() {
        focus.sheltered_at = None;
    }
    if focus
        .sheltered_at
        .is_some_and(|at| at.distance(chest) < CLEARANCE_MOVE)
    {
        return focus.sheltered;
    }
    let sun = lights
        .0
        .iter()
        .filter(|casting| is_sun(&casting.light))
        .max_by(|a, b| {
            let strength = |c: &CastingLight| {
                c.light.color_multiplier.w * c.light.color_multiplier.truncate().max_element()
            };
            strength(a).total_cmp(&strength(b))
        });
    let blocked = sun.is_none_or(|sun| {
        let toward = (sun.light.position_radius.truncate() - chest).normalize_or_zero();
        surfaces.0.iter().flatten().any(|surface| {
            ray_hit(chest, toward, &surface.corners)
                .is_some_and(|distance| (0.1..SHELTER_REACH).contains(&distance))
        })
    });
    focus.sheltered = blocked;
    focus.sheltered_at = Some(chest);
    blocked
}

/// The scene's characters' feet (their bodies' roots), the nearest the
/// player first.
fn gather_feet(
    scene: Option<Res<crate::scene::SceneContent>>,
    own: Option<Res<crate::play_view::OwnBody>>,
    models: Query<(&crate::scene::SceneModel, &GlobalTransform)>,
    focus: Res<Focus>,
    mut feet: ResMut<Feet>,
) {
    feet.0.clear();
    let Some(scene) = scene else { return };
    // The module's creatures, and the player's body (not one of them).
    let bodies: std::collections::BTreeSet<usize> = scene
        .0
        .creature_views
        .iter()
        .map(|view| view.body)
        .chain(own.and_then(|own| own.0))
        .collect();
    feet.0.extend(
        models
            .iter()
            .filter(|(model, _)| bodies.contains(&model.index))
            .map(|(_, transform)| transform.translation()),
    );
    let at = focus.feet;
    feet.0
        .sort_by(|a, b| a.distance(at).total_cmp(&b.distance(at)));
    feet.0.truncate(FEET);
}

/// How strongly a light lights the player's chest, by `odyssey.wgsl`'s
/// attenuation, a flagged light's counting more, a lamp of a room the
/// player's room only sees less; `None` when it is out of reach, a lamp not
/// above the head, or a lamp of a room the player's does not see (a wall
/// would keep its light off them).
fn score(casting: &CastingLight, focus: &Focus) -> Option<f32> {
    let light = &casting.light;
    let position = light.position_radius.truncate();
    let radius = light.position_radius.w;
    let feet = focus.feet;
    let distance = position.distance(feet + Vec3::Y * 1.2);
    if distance >= radius {
        return None;
    }
    let room = if is_sun(light) {
        1.0
    } else if casting.casts_from(focus).y < feet.y + ABOVE_FEET {
        return None;
    } else {
        let high = if casting.casts_from(focus).y > feet.y + HIGH_LAMP {
            HIGH_LAMP_WEIGHT
        } else {
            1.0
        };
        high * match (casting.room, focus.room) {
            (Some(lamp), Some(here)) if lamp == here => 1.0,
            (Some(lamp), Some(_)) if focus.seen.contains(&lamp) => SEEN_ROOM,
            (Some(_), Some(_)) => return None,
            _ => 1.0,
        }
    };
    let attenuation = radius * radius / ((radius + distance) * (radius + distance));
    let colour = light.color_multiplier.truncate();
    let flagged = if casting.flagged { FLAGGED } else { 1.0 };
    let steep = steepness(casting, focus);
    if steep <= 0.0 {
        return None;
    }
    Some(light.color_multiplier.w * attenuation * colour.max_element() * flagged * room * steep)
}

/// How much a light counts by how steeply it shines down on the player:
/// one overhead casts a shadow close about the feet, one off to the side a
/// long one across the floor.
fn steepness(casting: &CastingLight, focus: &Focus) -> f32 {
    if is_sun(&casting.light) {
        return 1.0;
    }
    let along = casting.casts_from(focus) - (focus.feet + Vec3::Y * 1.2);
    let sine = along.y / along.length().max(1e-4);
    // Lower than this, a lamp only lays a long smear across the floor.
    if sine < LOWEST_SINE {
        return 0.0;
    }
    let t = ((sine - LOWEST_SINE) / 0.5).clamp(0.0, 1.0);
    0.15 + 0.85 * t * t * (3.0 - 2.0 * t)
}

/// How far past its radius a lamp's shadows reach: its radius is how far
/// it lights moving things, while the lightmaps hold its light farther
/// (Kashyyyk's landing-building lamps reach 4 m, their floor lies further).
const SHADOW_REACH: f32 = 1.5;

/// The lowest a lamp may stand over the player's chest to cast, as the
/// sine of its angle above the horizon (about 17°).
const LOWEST_SINE: f32 = 0.3;

/// A lamp of a room the player's room sees counts this much of one of
/// theirs.
const SEEN_ROOM: f32 = 0.5;

/// A lamp casting from this high above the feet (metres) counts
/// `HIGH_LAMP_WEIGHT` of itself: no ceiling was found under it, and in
/// most rooms one is lower.
const HIGH_LAMP: f32 = 4.5;
const HIGH_LAMP_WEIGHT: f32 = 0.25;

fn is_sun(light: &GpuLight) -> bool {
    light.position_radius.w >= SUN_RADIUS
}

/// Keeps `slots` filled with the best of `ranked` (best first), the lights
/// held keeping theirs while they rank within the margin.
fn hold(
    slots: &mut [Option<Held>],
    ranked: &[(CastingLight, f32)],
    mut usable: impl FnMut(&CastingLight) -> Option<f32>,
) {
    let weakest_wanted = ranked.get(slots.len() - 1).map_or(0.0, |(_, score)| *score);
    for held in slots.iter_mut().flatten() {
        let held_score = usable(&held.light).unwrap_or(0.0);
        held.wanted = held_score > 0.0 && held_score * HYSTERESIS >= weakest_wanted;
    }
    for &(light, _) in ranked.iter().take(slots.len()) {
        let holding = slots.iter().flatten().any(|held| held.light.id == light.id);
        let wanted = slots.iter().flatten().filter(|held| held.wanted).count();
        if holding || wanted >= slots.len() {
            continue;
        }
        if let Some(free) = slots.iter_mut().find(|slot| slot.is_none()) {
            *free = Some(Held {
                light,
                strength: 0.0,
                wanted: true,
            });
        }
    }
}

/// Fills the slots with the strongest casting lights about the player and
/// fades them.
fn pick(
    (focus, surfaces): (Res<Focus>, Res<RoomSurfaces>),
    lights: Res<ShadowLights>,
    settings: Res<ShadowSettings>,
    time: Res<Time>,
    mut slots: ResMut<Slots>,
    mut clear: Local<Clearance>,
) {
    if lights.is_changed() {
        // A new scene: its lights replace the old ones at once.
        slots.0 = [None; SLOTS];
        clear.0.clear();
    }
    // A lamp casts when it scores and no wall or floor stands between it
    // and the player.
    let mut usable = |casting: &CastingLight| {
        let score = score(casting, &focus)?;
        clear.reaches(casting, &focus, &surfaces).then_some(score)
    };
    // Shadows turned off: no light ranks, and every slot fades out.
    let candidates: Vec<CastingLight> = if settings.on {
        lights
            .0
            .iter()
            .copied()
            .chain(ceiling_lights(&focus))
            .collect()
    } else {
        Vec::new()
    };
    let mut ranked = |sun: bool| {
        let mut ranked: Vec<(CastingLight, f32)> = candidates
            .iter()
            .filter(|casting| is_sun(&casting.light) == sun)
            .filter_map(|casting| Some((*casting, usable(casting)?)))
            .collect();
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
        ranked
    };
    let (ranked_lamps, ranked_sun) = (ranked(false), ranked(true));
    let (lamps, sun) = slots.0.split_at_mut(POINT_SLOTS);
    hold(lamps, &ranked_lamps, &mut usable);
    hold(sun, &ranked_sun, &mut usable);
    let step = FADE_SPEED * time.delta_secs();
    for slot in &mut slots.0 {
        let Some(held) = slot else { continue };
        held.strength = if held.wanted {
            (held.strength + step).min(1.0)
        } else {
            (held.strength - step).max(0.0)
        };
        if !held.wanted && held.strength == 0.0 {
            *slot = None;
        }
    }
}

/// Writes the slots' list and the feet when they change.
fn upload(
    (slots, focus): (Res<Slots>, Res<Focus>),
    feet: Res<Feet>,
    settings: Res<ShadowSettings>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut last: Local<ShadowList>,
    debug: Local<ShadowDebug>,
) {
    // Shadows turned off: no feet are shaded.
    let standing: &[Vec3] = if settings.on { &feet.0 } else { &[] };
    let mut list = ShadowList {
        debug: debug.0,
        soft: u32::from(settings.soft),
        feet_count: standing.len() as u32,
        ..default()
    };
    for (gpu, at) in list.feet.iter_mut().zip(standing) {
        *gpu = at.extend(1.0);
    }
    for (index, (gpu, slot)) in list.slots.iter_mut().zip(&slots.0).enumerate() {
        if let Some(held) = slot {
            let source = if index == SUN { SUN_ANGLE } else { LAMP_RADIUS };
            let weight = if held.light.is_ceiling_light() {
                CEILING_WEIGHT
            } else {
                1.0
            };
            let light = held.light.light.position_radius;
            *gpu = GpuShadow {
                position_radius: held.light.casts_from(&focus).extend(light.w * SHADOW_REACH),
                light_at: light,
                params: Vec4::new(held.strength, source, weight, 0.0),
            };
        }
    }
    if *last != list
        && let Some(mut buffer) = buffers.get_mut(&AREA_SHADOWS)
    {
        buffer.set_data(list);
        *last = list;
    }
}

/// The slots' lights and the candidates about the player, for the dump,
/// when the lights held change.
fn report(
    slots: Res<Slots>,
    lights: Res<ShadowLights>,
    focus: Res<Focus>,
    mut facts: ResMut<console::DumpFacts>,
    mut last: Local<Vec<Vec4>>,
) {
    let held: Vec<&Held> = slots.0.iter().flatten().collect();
    let now: Vec<Vec4> = held
        .iter()
        .map(|held| held.light.light.position_radius)
        .collect();
    if *last == now {
        return;
    }
    *last = now;
    let held: Vec<_> = held
        .iter()
        .map(|held| {
            serde_json::json!({
                "at": held.light.light.position_radius.truncate().to_array(),
                "from": held.light.casts_from(&focus).to_array(),
                "radius": held.light.light.position_radius.w,
                "flagged": held.light.flagged,
                "room": held.light.room,
            })
        })
        .collect();
    facts.0.insert(
        "shadows".into(),
        serde_json::json!({
            "candidates": lights.0.len(),
            "slots": held,
            "near": nearest(&lights.0, &focus),
            "room": focus.room,
        }),
    );
}

/// The casting lights nearest the player, for the dump: where each is from
/// the feet (metres; y up), its radius, room and score.
fn nearest(lights: &[CastingLight], focus: &Focus) -> Vec<serde_json::Value> {
    let from = |casting: &CastingLight| casting.light.position_radius.truncate() - focus.feet;
    let mut near: Vec<&CastingLight> = lights.iter().collect();
    near.sort_by(|a, b| from(a).length().total_cmp(&from(b).length()));
    near.iter()
        .take(12)
        .map(|casting| {
            serde_json::json!({
                "from_feet": from(casting).to_array(),
                "radius": casting.light.position_radius.w,
                "flagged": casting.flagged,
                "room": casting.room,
                "score": score(casting, focus),
            })
        })
        .collect()
}

/// Moves each lamp slot's spot light to its lamp, or parks it; turns the
/// sun's light along the sun to the player.
fn place_shadow_lights(
    slots: Res<Slots>,
    focus: Res<Focus>,
    mut lamps: Query<(&LampLight, &mut SpotLight, &mut Transform), Without<SunLight>>,
    mut suns: Query<(&mut DirectionalLight, &mut Transform), With<SunLight>>,
) {
    for (slot, mut point, mut transform) in &mut lamps {
        let (position, range) = match slots.0[slot.0] {
            Some(held) => (
                held.light.casts_from(&focus),
                held.light.light.position_radius.w * SHADOW_REACH,
            ),
            None => (PARKED, 0.01),
        };
        if transform.translation != position {
            transform.translation = position;
        }
        if point.range != range {
            point.range = range;
        }
    }
    for (mut light, mut transform) in &mut suns {
        let casting = slots.0[SUN].is_some();
        if light.shadow_maps_enabled != casting {
            light.shadow_maps_enabled = casting;
        }
        if let Some(held) = slots.0[SUN] {
            let along = focus.feet - held.light.light.position_radius.truncate();
            let turned = Transform::IDENTITY.looking_to(along, Vec3::Y);
            if transform.rotation != turned.rotation {
                transform.rotation = turned.rotation;
            }
        }
    }
}

/// How far past a light's radius a caster still goes into its map: a mesh
/// is placed by its node, and a body reaches about this far from it.
const CASTER_REACH: f32 = 2.5;

/// The sun's casters: those this near the player (its cascades reach
/// `maximum_distance` from the camera).
const SUN_REACH: f32 = 60.0;

/// Lets a caster into the maps while a casting light reaches it.
fn gate_casters(
    slots: Res<Slots>,
    focus: Res<Focus>,
    casters: Query<(Entity, &GlobalTransform, Has<NotShadowCaster>), With<ShadowCaster>>,
    mut commands: Commands,
    mut facts: ResMut<console::DumpFacts>,
) {
    let mut let_in = 0;
    let mut reaches: Vec<Vec4> = slots.0[..POINT_SLOTS]
        .iter()
        .flatten()
        .map(|held| {
            held.light
                .casts_from(&focus)
                .extend(held.light.light.position_radius.w * SHADOW_REACH)
        })
        .collect();
    if slots.0[SUN].is_some() {
        reaches.push(focus.feet.extend(SUN_REACH));
    }
    for (entity, transform, kept_out) in &casters {
        let at = transform.translation();
        let reached = reaches
            .iter()
            .any(|light| light.truncate().distance(at) < light.w + CASTER_REACH);
        let_in += usize::from(reached);
        if reached == kept_out {
            if reached {
                commands.entity(entity).remove::<NotShadowCaster>();
            } else {
                commands.entity(entity).insert(NotShadowCaster);
            }
        }
    }
    facts.0.insert(
        "shadow_casters".into(),
        serde_json::json!({ "marked": casters.iter().len(), "let_in": let_in, "focus": focus.feet.to_array() }),
    );
}

/// Puts casters and rooms on the layers their maps are drawn from.
fn layer_casters_and_rooms(
    casters: Query<Entity, Added<ShadowCaster>>,
    rooms: Query<Entity, Added<ShadowRoom>>,
    mut commands: Commands,
) {
    for entity in &casters {
        commands
            .entity(entity)
            .insert(RenderLayers::from_layers(&[0, CASTER_LAYER]));
    }
    for entity in &rooms {
        commands
            .entity(entity)
            .insert(RenderLayers::from_layers(&[0, ROOM_LAYER]));
    }
}

/// A mesh the Odyssey material has just begun to draw, not yet kept out of
/// the maps, and not a room's.
type NewlyDrawn = (
    Added<MeshMaterial3d<OdysseyMaterial>>,
    Without<NotShadowCaster>,
    Without<ShadowRoom>,
);

/// Keeps out of the maps whatever the Odyssey material draws that does not
/// cast (effects), and every caster until a light reaches it.
fn keep_out_of_maps(added: Query<Entity, NewlyDrawn>, mut commands: Commands) {
    for entity in &added {
        commands.entity(entity).insert(NotShadowCaster);
    }
}

/// A room model's surfaces in Bevy world space, at rest (`frame` places it
/// in the game's space, `to_bevy` turns that into Bevy's).
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
        .map(|corners| {
            let [a, b, c] = corners;
            Surface {
                corners,
                down: (b - a).cross(c - a).normalize_or_zero().y < -0.5,
            }
        })
        .collect()
}

/// Whether each light reaches the player past the rooms' walls, floors and
/// ceilings, by its id: kept while the player stays near where it was
/// found (a ray through a room's surfaces is not free).
#[derive(Default)]
struct Clearance(std::collections::HashMap<u32, (Vec3, Vec3, bool)>);

/// How far (metres) the player or a light's casting point moves before its
/// clearance is found anew.
const CLEARANCE_MOVE: f32 = 0.5;

impl Clearance {
    /// Whether nothing of the player's room or the light's stands between
    /// the light's casting point and the player's chest. The sun's
    /// clearance is the rooms' maps' to find.
    fn reaches(&mut self, casting: &CastingLight, focus: &Focus, surfaces: &RoomSurfaces) -> bool {
        if is_sun(&casting.light) {
            return true;
        }
        let from = casting.casts_from(focus);
        let chest = focus.feet + Vec3::Y * 1.2;
        if let Some((was_chest, was_from, clear)) = self.0.get(&casting.id)
            && was_chest.distance(chest) < CLEARANCE_MOVE
            && was_from.distance(from) < CLEARANCE_MOVE
        {
            return *clear;
        }
        let rooms = [focus.room, casting.room];
        let mut rooms: Vec<usize> = rooms.into_iter().flatten().collect();
        rooms.dedup();
        let along = from - chest;
        let length = along.length();
        let direction = along / length.max(1e-4);
        // A little short of both ends: the light's own fixture, the floor.
        let blocked = rooms
            .iter()
            .filter_map(|room| surfaces.0.get(*room))
            .flatten()
            .filter_map(|surface| ray_hit(chest, direction, &surface.corners))
            .any(|distance| distance > 0.1 && distance < length - 0.1);
        self.0.insert(casting.id, (chest, from, !blocked));
        !blocked
    }
}

/// Where along `direction` from `origin` the ray meets `triangle`
/// (Möller–Trumbore), either face.
fn ray_hit(origin: Vec3, direction: Vec3, [a, b, c]: &[Vec3; 3]) -> Option<f32> {
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
