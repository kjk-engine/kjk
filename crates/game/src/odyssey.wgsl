// Odyssey static geometry as the games' fixed-function path draws it: the
// diffuse texture modulated by the lightmap on the second UV set (the
// games multiply the two).
//
// Textures are bound as plain UNORM, so the multiply happens on the stored
// (gamma-encoded) values as on the original hardware, and the result is
// output as it is: the target keeps encoded values and blends them as the
// games' framebuffers did (`scene::encoded_view`).

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::view
#import bevy_pbr::mesh_functions::get_world_from_local
#import bevy_pbr::mesh_view_bindings as view_bindings
#import bevy_render::maths::orthonormalize
#import kjk::lighting::{flash, fog}

struct GpuLight {
    position_radius: vec4<f32>,
    color_multiplier: vec4<f32>,
    kind: vec4<u32>,
}

struct OdysseyParams {
    tint: vec4<f32>,
    self_illum: vec4<f32>,
    ambient: vec4<f32>,
    diffuse: vec4<f32>,
    world_ambient: vec4<f32>,
    fog_color: vec4<f32>,
    fog_range: vec4<f32>,
    uv_offset: vec2<f32>,
    flags: u32,
}

struct LightList {
    // The lightning's light (`weather::Flash`): xyz where, w its radius
    // (0: none); its colour.
    flash_at: vec4<f32>,
    flash_colour: vec4<f32>,
    count: u32,
    lights: array<GpuLight, 32>,
}

// A light casting shadows (`shadows.rs`): x strength (fading), y the
// source's size (a lamp's radius in metres, the sun's angle), z how much
// of a lamp's shadow its shadow is (a ceiling light's less).
struct GpuShadow {
    // Where its maps are cast from, and the light's radius.
    position_radius: vec4<f32>,
    // Where the light is, as the light list has it.
    light_at: vec4<f32>,
    params: vec4<f32>,
}

struct ShadowList {
    // 1: the shadows alone are drawn (`KJK_SHADOW_DEBUG`).
    debug: u32,
    // 1: soft edges, 0: hard ones (the options' Soft Shadows).
    soft: u32,
    feet_count: u32,
    slots: array<GpuShadow, 3>,
    // Where the characters nearest the player stand.
    feet: array<vec4<f32>, 16>,
}

// Flags: see `material.rs`.
const DISCARD_ZERO_ALPHA: u32 = 1u;
const OPAQUE_ALPHA: u32 = 2u;
const ENVMAP: u32 = 4u;
const HAS_LIGHTMAP: u32 = 8u;
const STATIC: u32 = 16u;
const UNLIT: u32 = 32u;
const VERTEX_ALPHA: u32 = 64u;
const FOG: u32 = 128u;
const OPAQUE_TEXTURE: u32 = 256u;
const ADD_FADE: u32 = 512u;
const SHADOWED: u32 = 1024u;
const LIGHT_DYNAMIC_TYPE_ALL: u32 = 1u;

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> params: OdysseyParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var base_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var base_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var lightmap_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var lightmap_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var envmap_texture: texture_cube<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var envmap_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(7) var<storage, read> lights: LightList;
@group(#{MATERIAL_BIND_GROUP}) @binding(8) var<storage, read> shadows: ShadowList;

// `shadows.rs`'s slots: the lamps' spot maps, then the sun's cascades.
const POINT_SLOTS: u32 = 2u;
const SHADOW_SLOTS: u32 = 3u;
const SUN_SLOT: u32 = 2u;
// How much of a room's light a lamp's shadow takes at most, close under a
// bright lamp: the lightmap holds light bounced about the room too, which
// no caster blocks.
const LAMP_SHADOW_DARKNESS: f32 = 0.95;
// How much of a sunlit room's light a character's shadow takes: as much
// as the lightmaps' own shadows of buildings take (Tatooine's shaded
// sand is some 56% of its sunlit sand).
const SUN_SHADOW_DARKNESS: f32 = 0.6;
// How much of a light a character's (or a door's, a placeable's) own
// shadow, or another's, keeps from it: not all, as light bounces about.
const CHARACTER_SHADOW_DARKNESS: f32 = 0.85;
// The rooms-only maps are read this far (metres) toward the light from
// the point, so a floor does not shade itself in them.
const REACH_BIAS: f32 = 0.1;
// A shadow fades out between these gaps (metres) from its caster to where
// it falls: the feet's is whole, a long one cast across the floor faint.
// A lamp's fades sooner: lamps hang low, and their shadows stretch.
const SUN_FADE_NEAR: f32 = 1.0;
const SUN_FADE_FAR: f32 = 6.0;
const LAMP_FADE_NEAR: f32 = 0.5;
const LAMP_FADE_FAR: f32 = 4.5;
// The soft shade every character's feet leave on the floor about them
// (the light the body keeps off it from every side): its radius (metres)
// and its darkness at the middle.
const FOOT_RADIUS: f32 = 0.8;
const FOOT_DARKNESS: f32 = 0.75;
// The sun's cascades' and the lamps' maps' size, texels (`shadows.rs`'s
// `MAP_SIZE`).
const MAP_SIZE: f32 = 2048.0;
// The farthest a sun's caster is looked for above a point (metres).
const SUN_SEARCH: f32 = 3.0;

// A disc of 16 points, for the blocker search and the filter.
const DISC: array<vec2<f32>, 16> = array<vec2<f32>, 16>(
    vec2<f32>(-0.94201624, -0.39906216),
    vec2<f32>(0.94558609, -0.76890725),
    vec2<f32>(-0.09418410, -0.92938870),
    vec2<f32>(0.34495938, 0.29387760),
    vec2<f32>(-0.91588581, 0.45771432),
    vec2<f32>(-0.81544232, -0.87912464),
    vec2<f32>(-0.38277543, 0.27676845),
    vec2<f32>(0.97484398, 0.75648379),
    vec2<f32>(0.44323325, -0.97511554),
    vec2<f32>(0.53742981, -0.47373420),
    vec2<f32>(-0.26496911, -0.41893023),
    vec2<f32>(0.79197514, 0.19090188),
    vec2<f32>(-0.24188840, 0.99706507),
    vec2<f32>(-0.81409955, 0.91437590),
    vec2<f32>(0.19984126, 0.78641367),
    vec2<f32>(0.14383161, -0.14100790),
);

fn max3(v: vec3<f32>) -> f32 {
    return max(v.x, max(v.y, v.z));
}

fn same_place(a: vec3<f32>, b: vec3<f32>) -> bool {
    return all(abs(a - b) < vec3<f32>(1e-3));
}

// A screen-stable angle per pixel (interleaved gradient noise), turning
// the disc so that its few points blur rather than band.
fn shadow_turn(frag: vec2<f32>) -> mat2x2<f32> {
    let angle = 6.2831853 * fract(52.9829189 * fract(dot(frag, vec2<f32>(0.06711056, 0.00583715))));
    let c = cos(angle);
    let s = sin(angle);
    return mat2x2<f32>(c, s, -s, c);
}

// How much of a shadow is left at a gap (metres) between caster and
// receiver.
fn shadow_fade(gap: f32, near: f32, far: f32) -> f32 {
    return 1.0 - smoothstep(near, far, gap);
}

// What the floor keeps of its light about the characters' feet: a soft
// shade, darkest under them, for a surface facing up near their height.
fn feet_shade(position: vec3<f32>, normal: vec3<f32>) -> f32 {
    var kept = 1.0;
    let up = smoothstep(0.5, 0.9, normal.y);
    if up <= 0.0 {
        return 1.0;
    }
    for (var i = 0u; i < min(shadows.feet_count, 16u); i++) {
        let feet = shadows.feet[i].xyz;
        let rise = position.y - feet.y;
        let across = length(position.xz - feet.xz) / FOOT_RADIUS;
        if across >= 1.0 || abs(rise) > 0.5 {
            continue;
        }
        let shade = (1.0 - smoothstep(0.0, 1.0, across)) * (1.0 - smoothstep(0.1, 0.5, abs(rise)));
        kept *= 1.0 - FOOT_DARKNESS * shade * shade * up;
    }
    return kept;
}

// The spot light Bevy renders a lamp's map from, of its casters or of the
// rooms alone: Bevy orders its lights itself, so the lamp's is found by
// its position, and the rooms' by its size (`shadows.rs`'s
// `ROOM_MAP_TAG`). -1 when none renders it.
fn lamp_light(position: vec3<f32>, rooms: bool) -> i32 {
    for (var light = 0u; light < 2u * POINT_SLOTS; light++) {
        let at = view_bindings::clustered_lights.data[light].position_radius;
        if same_place(at.xyz, position) && (at.w > 0.5) == rooms {
            return i32(light);
        }
    }
    return -1;
}

// Where `local` (from the lamp) falls in its map, as Bevy's
// `fetch_spot_shadow` projects it for a spot light facing straight down
// (`shadows.rs`): x, y across the map at a distance of 1 along its axis
// (the tangent of the angle off it), z the distance along it.
fn spot_project(local: vec3<f32>) -> vec3<f32> {
    let projected = local * orthonormalize(vec3<f32>(0.0, 1.0, 0.0));
    let along = -projected.z;
    return vec3<f32>(projected.xy / along, along);
}

// Where a projected point (`spot_project`'s x, y) falls in a lamp's map.
fn spot_uv(across: vec2<f32>, tan_cone: f32) -> vec2<f32> {
    return across / tan_cone * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5);
}

// The lamp's rooms-only map says whether its light reaches a point at all
// (1) or a wall or floor keeps it off (0), from a few texels about it.
fn lamp_reaches(local: vec3<f32>, light: i32) -> f32 {
    if light < 0 {
        return 1.0;
    }
    let rooms = view_bindings::clustered_lights.data[light];
    let at = spot_project(local * max(0.0, 1.0 - REACH_BIAS / length(local)));
    if at.z <= rooms.shadow_map_near_z || any(abs(at.xy) >= vec2<f32>(rooms.spot_light_tan_angle)) {
        return 1.0;
    }
    let uv = spot_uv(at.xy, rooms.spot_light_tan_angle);
    let depth = rooms.shadow_map_near_z / at.z;
    let layer = light + view_bindings::lights.spot_light_shadowmap_offset;
    var lit = 0.0;
    for (var i = 0u; i < 4u; i++) {
        lit += map_compare(uv + DISC[i * 4u] * 2.0 / MAP_SIZE, depth, layer);
    }
    return lit / 4.0;
}

// How much of a lamp's light its casters keep from `position`: 0 none, 1
// all of it. Percentage-closer soft shadows: the casters found about the
// point set the penumbra (wider the farther the shadow falls from them,
// for a lamp of the slot's size), and the shadow fades as that gap grows.
fn lamp_occlusion(shadow: GpuShadow, position: vec3<f32>, normal: vec3<f32>, frag: vec2<f32>) -> f32 {
    let id = lamp_light(shadow.position_radius.xyz, false);
    if id < 0 {
        return 0.0;
    }
    let light = view_bindings::clustered_lights.data[id];
    let to_light = shadow.position_radius.xyz - position;
    let distance = length(to_light);
    // The lamp faces down: a point is that far below it along its axis.
    let below = to_light.y;
    if below <= light.shadow_map_near_z || distance >= shadow.position_radius.w {
        return 0.0;
    }
    // Bevy's biases: along the normal by the texel's size there, and
    // toward the light.
    let offset = position + light.shadow_normal_bias * below * normal
        + light.shadow_depth_bias * to_light / distance;
    let local = offset - shadow.position_radius.xyz;
    let at = spot_project(local);
    let tan_cone = light.spot_light_tan_angle;
    // Out to the cone's edge the shadow eases away.
    let edge = max(abs(at.x), abs(at.y)) / tan_cone;
    if edge >= 1.0 {
        return 0.0;
    }
    let uv = spot_uv(at.xy, tan_cone);
    let receiver = at.z;
    let depth = light.shadow_map_near_z / receiver;
    let layer = id + view_bindings::lights.spot_light_shadowmap_offset;
    // How far the point is from the lamp along its ray.
    let span = length(local);
    // An angle off the point's ray spans about this much of the map's uv
    // there: the secant of the angle off the axis across, its square along.
    let secant = span / receiver;
    let uv_per_angle = secant * sqrt(secant) / (2.0 * tan_cone);
    let texel = 1.0 / MAP_SIZE;
    let turn = shadow_turn(frag);
    let source = shadow.params.y;

    // The casters about the point: within the lamp's size as seen from
    // casters halfway to it.
    let search = max(source / span * uv_per_angle, 4.0 * texel);
    var found = 0.0;
    var blocker = 0.0;
    for (var i = 0u; i < 16u; i++) {
        let sampled = map_depth(uv + turn * DISC[i] * search, layer);
        if sampled > depth {
            found += 1.0;
            blocker += sampled;
        }
    }
    if found == 0.0 {
        return 0.0;
    }
    // Along the lamp's axis: their ratio is the same along the ray.
    let caster = light.shadow_map_near_z / (blocker / found);
    let gap = max(receiver - caster, 0.0);
    // The penumbra as an angle from the lamp, then across the map.
    let penumbra = select(
        1.5 * texel,
        clamp(source * gap / (max(caster, 0.05) * span) * uv_per_angle, 1.5 * texel, 0.25 * uv_per_angle),
        shadows.soft != 0u,
    );
    var lit = 0.0;
    for (var i = 0u; i < 16u; i++) {
        lit += map_compare(uv + turn * DISC[i] * penumbra, depth, layer);
    }
    let reaches = lamp_reaches(local, lamp_light(shadow.position_radius.xyz, true));
    return (1.0 - lit / 16.0) * shadow_fade(gap * span / receiver, LAMP_FADE_NEAR, LAMP_FADE_FAR)
        * (1.0 - smoothstep(0.85, 1.0, edge)) * reaches;
}

// 1 where the map `layer` (a sun's cascade, or a lamp's map after them)
// at `uv` is farther from its light than `depth`.
fn map_compare(uv: vec2<f32>, depth: f32, layer: i32) -> f32 {
#ifdef NO_ARRAY_TEXTURES_SUPPORT
    return textureSampleCompareLevel(
        view_bindings::directional_shadow_textures,
        view_bindings::directional_shadow_textures_comparison_sampler,
        uv,
        depth,
    );
#else
    return textureSampleCompareLevel(
        view_bindings::directional_shadow_textures,
        view_bindings::directional_shadow_textures_comparison_sampler,
        uv,
        layer,
        depth,
    );
#endif
}

// The depth the map `layer` holds at `uv`; 0 where it cannot be read.
fn map_depth(uv: vec2<f32>, layer: i32) -> f32 {
#ifdef PCSS_SAMPLERS_AVAILABLE
#ifdef NO_ARRAY_TEXTURES_SUPPORT
    return textureSampleLevel(
        view_bindings::directional_shadow_textures,
        view_bindings::directional_shadow_textures_linear_sampler,
        uv,
        0,
    );
#else
    return textureSampleLevel(
        view_bindings::directional_shadow_textures,
        view_bindings::directional_shadow_textures_linear_sampler,
        uv,
        layer,
        0,
    );
#endif
#else
    return 0.0;
#endif
}

// Bevy's directional light for the sun's casters, or for the rooms alone
// (told apart by colour: `shadows.rs`'s `SUN_TAG_ILLUMINANCE`); -1 when
// none casts.
fn sun_light(rooms: bool) -> i32 {
    for (var i = 0u; i < view_bindings::lights.n_directional_lights; i++) {
        let colour = view_bindings::lights.directional_lights[i].color.rgb;
        if (colour.g > colour.r) == rooms {
            return i32(i);
        }
    }
    return -1;
}

// Where a point falls in a sun light's cascade for its distance from the
// eye: (u, v, depth, cascade), the cascade -1 when it is in none.
fn sun_at(light: u32, position: vec3<f32>, normal: vec3<f32>) -> vec4<f32> {
    let sun = view_bindings::lights.directional_lights[light];
    let view_z = (view.view_from_world * vec4<f32>(position, 1.0)).z;
    var cascade = 0u;
    for (; cascade < sun.num_cascades; cascade++) {
        if -view_z < sun.cascades[cascade].far_bound {
            break;
        }
    }
    if cascade >= sun.num_cascades {
        return vec4<f32>(0.0, 0.0, 0.0, -1.0);
    }
    let offset = position + sun.shadow_normal_bias * sun.cascades[cascade].texel_size * normal
        + sun.shadow_depth_bias * sun.direction_to_light;
    let clip = sun.cascades[cascade].clip_from_world * vec4<f32>(offset, 1.0);
    let ndc = clip.xyz / clip.w;
    if any(ndc.xy < vec2<f32>(-1.0)) || any(ndc > vec3<f32>(1.0)) || ndc.z < 0.0 {
        return vec4<f32>(0.0, 0.0, 0.0, -1.0);
    }
    return vec4<f32>(ndc.xy * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5), ndc.z, f32(cascade));
}

// The sun's rooms-only cascades say whether it reaches a point at all.
fn sun_reaches(position: vec3<f32>, normal: vec3<f32>) -> f32 {
    let light = sun_light(true);
    if light < 0 {
        return 1.0;
    }
    let sun = view_bindings::lights.directional_lights[light];
    let at = sun_at(u32(light), position + REACH_BIAS * sun.direction_to_light, normal);
    if at.w < 0.0 {
        return 1.0;
    }
    let cascade = u32(at.w);
    let layer = i32(sun.depth_texture_base_index + cascade);
    let step = 1.5 / MAP_SIZE;
    var lit = 0.0;
    for (var i = 0u; i < 4u; i++) {
        lit += map_compare(at.xy + DISC[i * 4u] * step, at.z, layer);
    }
    return lit / 4.0;
}

// The sun's `lamp_occlusion`, from the cascade the point's distance from
// the eye falls in.
fn sun_occlusion(shadow: GpuShadow, position: vec3<f32>, normal: vec3<f32>, frag: vec2<f32>) -> f32 {
    let light = sun_light(false);
    if light < 0 {
        return 0.0;
    }
    let at = sun_at(u32(light), position, normal);
    if at.w < 0.0 {
        return 0.0;
    }
    let sun = view_bindings::lights.directional_lights[light];
    let cascade = u32(at.w);
    let texel = sun.cascades[cascade].texel_size;
    let uv = at.xy;
    let depth = at.z;
    let layer = i32(sun.depth_texture_base_index + cascade);
    // The cascade's depth per metre toward the sun.
    let clip_from_world = sun.cascades[cascade].clip_from_world;
    let rise = dot(
        vec3<f32>(clip_from_world[0].z, clip_from_world[1].z, clip_from_world[2].z),
        sun.direction_to_light,
    );
    // Metres across the map to its uv.
    let per_metre = 1.0 / (texel * MAP_SIZE);
    let turn = shadow_turn(frag);
    let angle = shadow.params.y;

    let search = max(angle * SUN_SEARCH, 4.0 * texel) * per_metre;
    var found = 0.0;
    var blocker = 0.0;
    for (var i = 0u; i < 16u; i++) {
        let sampled = map_depth(uv + turn * DISC[i] * search, layer);
        if sampled > depth {
            found += 1.0;
            blocker += sampled;
        }
    }
    if found == 0.0 {
        return 0.0;
    }
    let gap = max((blocker / found - depth) / rise, 0.0);
    let penumbra = select(1.5 * texel, max(angle * gap, 1.5 * texel), shadows.soft != 0u) * per_metre;
    var lit = 0.0;
    for (var i = 0u; i < 16u; i++) {
        lit += map_compare(uv + turn * DISC[i] * penumbra, depth, layer);
    }
    return (1.0 - lit / 16.0) * shadow_fade(gap, SUN_FADE_NEAR, SUN_FADE_FAR) * sun_reaches(position, normal);
}

// How much of a slot's light its casters keep from `position`, by its
// strength.
fn shadow_occlusion(slot: u32, position: vec3<f32>, normal: vec3<f32>, frag: vec2<f32>) -> f32 {
    let shadow = shadows.slots[slot];
    if shadow.params.x <= 0.0 {
        return 0.0;
    }
    if slot == SUN_SLOT {
        return sun_occlusion(shadow, position, normal, frag) * shadow.params.x;
    }
    return lamp_occlusion(shadow, position, normal, frag) * shadow.params.x;
}

// The slot casting from the light at `position`, or -1.
fn shadow_slot(position: vec3<f32>) -> i32 {
    for (var slot = 0u; slot < SHADOW_SLOTS; slot++) {
        let held = shadows.slots[slot];
        if held.params.x > 0.0 && same_place(held.light_at.xyz, position) {
            return i32(slot);
        }
    }
    return -1;
}

// How much a lamp's shadow may take where it falls: all of
// `LAMP_SHADOW_DARKNESS` close under a bright lamp the surface faces,
// easing out to nothing at the lamp's radius (a shadow would otherwise
// end at a hard circle).
fn lamp_presence(held: GpuShadow, position: vec3<f32>, normal: vec3<f32>) -> f32 {
    // The lamp's brightness as the list has it.
    var brightness = 1.0;
    for (var i = 0u; i < lights.count; i++) {
        let light = lights.lights[i];
        if same_place(light.position_radius.xyz, held.light_at.xyz) {
            // A dim lamp's shadow is lighter, never gone: in a dim room it
            // is still what lights the floor.
            brightness = mix(0.7, 1.0, smoothstep(0.0, 0.6, light.color_multiplier.w * max3(light.color_multiplier.rgb)));
        }
    }
    // From where the shadows are cast (the lamp, or under the ceiling
    // beneath it).
    let to_light = held.position_radius.xyz - position;
    let distance = length(to_light);
    let facing = smoothstep(0.0, 0.3, dot(normal, to_light / max(1e-4, distance)));
    let radius = held.position_radius.w;
    return held.params.z * brightness * facing * (1.0 - smoothstep(0.7 * radius, radius, distance));
}

// What a room's lightmapped light keeps where the casting lights' shadows
// fall: each lamp takes as much as it is present there, the sun as much
// as the lightmaps' own sun shadows take.
fn room_shadow(position: vec3<f32>, normal: vec3<f32>, frag: vec2<f32>) -> f32 {
    var kept = feet_shade(position, normal);
    kept *= 1.0 - SUN_SHADOW_DARKNESS * shadow_occlusion(SUN_SLOT, position, normal, frag);
    for (var slot = 0u; slot < POINT_SLOTS; slot++) {
        let held = shadows.slots[slot];
        if held.params.x <= 0.0 {
            continue;
        }
        let presence = lamp_presence(held, position, normal);
        if presence > 0.0 {
            kept *= 1.0 - LAMP_SHADOW_DARKNESS * presence * shadow_occlusion(slot, position, normal, frag);
        }
    }
    return kept;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
#ifdef VERTEX_UVS_A
    let uv = in.uv + params.uv_offset;
#else
    let uv = vec2<f32>(0.0);
#endif
#ifdef VERTEX_UVS_B
    let uv2 = in.uv_b;
#else
    let uv2 = vec2<f32>(0.0);
#endif
    var texel = textureSample(base_texture, base_sampler, uv);
    // A texture without alpha (DXT1 kept in blocks): its transparent texel
    // is opaque black, as the games drew DXT1.
    if (params.flags & OPAQUE_TEXTURE) != 0u {
        texel.a = 1.0;
    }
    let base = texel * params.tint;
    // The second UV set's lightmap, read on every fragment so the sample
    // stays in uniform control flow (a mesh without one binds a stand-in).
    let lightmap = textureSample(lightmap_texture, lightmap_sampler, uv2).rgb;
    let self_lit = params.self_illum.rgb;
    // A lightmapped surface starts from its baked light plus its own glow;
    // anything else from full light. An unlit surface keeps this as it is.
    var lighting = vec3<f32>(1.0);
    if (params.flags & HAS_LIGHTMAP) != 0u {
        lighting = min(vec3<f32>(1.0), self_lit + lightmap);
    }
    if (params.flags & UNLIT) == 0u {
        // Fixed-function lighting, per fragment: an ambient sum, which the
        // material's ambient colour scales, and a diffuse sum, which its
        // diffuse colour scales, starting from the surface's own glow.
        let is_static = (params.flags & STATIC) != 0u;
        var ambient_sum = vec3<f32>(0.0);
        var diffuse_sum = self_lit;
        if (params.flags & HAS_LIGHTMAP) != 0u {
            diffuse_sum = self_lit + lightmap;
        } else if !is_static {
            // The area's ambient lights a moving mesh (a creature, a
            // placeable) that has no baked light of its own.
            ambient_sum = ambient_sum + params.ambient.rgb * params.world_ambient.rgb;
        }
        let normal = normalize(in.world_normal);
        let position = in.world_position.xyz;
        // Whether a light reaches a mesh is judged once for the whole mesh,
        // from its origin, so a creature is lit or not as one piece. The
        // light then fades out over the last tenth of its radius. (The
        // Endar Spire's storage box: the owner's screenshot reads (61, 71,
        // 98), this rule (56, 65, 89), a cut-off at the radius from each
        // fragment (143, 148, 171).)
        let origin = get_world_from_local(in.instance_index)[3].xyz;
        for (var i = 0u; i < lights.count; i++) {
            let light = lights.lights[i];
            // A static mesh takes only the lights meant for everything.
            if is_static && light.kind.y != LIGHT_DYNAMIC_TYPE_ALL {
                continue;
            }
            let at = light.position_radius.xyz;
            let radius = light.position_radius.w;
            let to_light = at - position;
            let distance = length(to_light);
            let reach = 1.0 - smoothstep(0.9 * radius, radius, length(at - origin));
            if reach <= 0.0 {
                continue;
            }
            // The fragment's own distance sets only the falloff and the
            // direction: there is no cut-off but the reach above.
            let direction = to_light / max(1e-4, distance);
            let facing = max(0.0, dot(normal, direction));
            let falloff = (radius * radius) / ((radius + distance) * (radius + distance));
            let strength = ((reach * light.color_multiplier.w) * falloff) * light.color_multiplier.rgb;
            if light.kind.x != 0u {
                // A light that only brightens the ambient: no direction.
                ambient_sum = ambient_sum + strength * params.ambient.rgb;
            } else {
                // A character's shadow (or another caster's) keeps part of
                // this light off the side that faces it.
                var kept = 1.0;
                if (params.flags & SHADOWED) != 0u && facing > 0.0 {
                    let slot = shadow_slot(at);
                    if slot >= 0 {
                        kept = 1.0 - CHARACTER_SHADOW_DARKNESS * shadow_occlusion(u32(slot), position, normal, in.position.xy);
                    }
                }
                diffuse_sum = diffuse_sum + ((strength * facing) * kept) * params.diffuse.rgb;
            }
        }
        // The diffuse sum never goes below black; the total stops at full.
        lighting = min(vec3<f32>(1.0), ambient_sum + max(vec3<f32>(0.0), diffuse_sum));
    } else if (params.flags & SHADOWED) != 0u {
        // A room: its light is baked, so a shadow takes the casting
        // lights' share of it.
        // A mesh without normals (some rooms' walls) takes no shadow.
        var shade = 1.0;
        if dot(in.world_normal, in.world_normal) > 1e-8 {
            shade = room_shadow(in.world_position.xyz, normalize(in.world_normal), in.position.xy);
        }
        if shadows.debug != 0u {
            return vec4<f32>(vec3<f32>(shade), 1.0);
        }
        lighting *= shade;
    }
    // A lightning flash lights everything near it, rooms too, which the
    // light list leaves to their lightmaps (EXE-NOTES `LIGHTNING_LIGHT`).
    // Fixed function's light sum stops at full: the flash's light (colour
    // keys up to 1 × multiplier 100) shows a surface's own texture, not
    // white.
    // Without a flash nothing here runs: the look is as it was.
    if lights.flash_at.w > 0.0 {
        lighting = min(
            vec3<f32>(1.0),
            lighting + flash(in.world_position.xyz, lights.flash_at, lights.flash_colour.rgb)
        );
    }
    var colour = lighting * base.rgb;
    if (params.flags & ENVMAP) != 0u {
        // Reflection mapping as fixed function sets it up, done per
        // fragment: the direction from the eye to the point, mirrored about
        // the surface normal, both in eye space, picks the cube map's texel.
        // The base texture's alpha masks the shine: an opaque texel has
        // none.
        let eye_position = (view.view_from_world * vec4<f32>(in.world_position.xyz, 1.0)).xyz;
        let eye_normal = normalize((view.view_from_world * vec4<f32>(in.world_normal, 0.0)).xyz);
        let mirrored = reflect(normalize(eye_position), eye_normal);
        let shine = textureSample(envmap_texture, envmap_sampler, mirrored).rgb;
        colour = min(colour + shine * (1.0 - base.a), vec3<f32>(1.0));
    }
    if (params.flags & FOG) != 0u {
        colour = fog(colour, in.world_position.xyz, params.fog_range, params.fog_color.rgb);
    }
    var alpha = select(base.a, 1.0, (params.flags & OPAQUE_ALPHA) != 0u);
#ifdef VERTEX_COLORS
    // Effect primitives carry a colour and fading alpha per vertex (the
    // saber trail); on additive surfaces the alpha scales what is added,
    // on vertex-alpha ones it fades the blend.
    if (params.flags & VERTEX_ALPHA) != 0u {
        colour = colour * in.color.rgb;
        alpha = alpha * in.color.a;
    } else {
        colour = colour * in.color.rgb * in.color.a;
    }
#endif
    // Alpha 0 (the vertex's included: a punch-through particle faded out)
    // hides nothing behind it.
    if (params.flags & DISCARD_ZERO_ALPHA) != 0u && alpha == 0.0 {
        discard;
    }
    // An added surface a clip fades (the swoop's thruster cards, its start
    // lights' flares) adds by the alpha it keys (`faded_additive`).
    if (params.flags & ADD_FADE) != 0u {
        colour = colour * clamp(params.tint.a, 0.0, 1.0);
    }
    return vec4<f32>(colour, alpha);
}
