// The area's grass (`grass.rs`): upright quads in game space under the
// scene's root. Each cluster goes by its own point (`base`): within the
// near radius its top edge sways in the area's wind, its alpha fades from
// the near radius to nothing at the far one, and past that it is not drawn.
// The texture is alpha tested at 0.4 (EXE-NOTES `GRASS_BLEND`) and lit by
// the room's lightmap (`GRASS_LIGHT`; the area's colours are not applied,
// see `grass.rs`); then the area's fog, as `odyssey.wgsl` does
// (`lighting.wgsl`).
// Like the scene's material, it multiplies and outputs stored values.
//
// In the motion prepass (`PREPASS_PIPELINE`, the motion twin of
// `grass.rs`) the same quads, swayed the same way, write how far each
// pixel they cover (the alpha test's) moved since the last frame: its sway
// then, by the clock's previous reading `wind.w`, to its sway now, as the
// camera saw both. They write no depth (see `grass.rs`).

#import bevy_pbr::{
    mesh_functions,
    mesh_view_bindings::view,
    view_transformations::position_world_to_clip,
}
#ifdef MOTION_VECTOR_PREPASS
#import bevy_pbr::prepass_bindings::previous_view_uniforms
#endif
#ifndef PREPASS_PIPELINE
#import kjk::lighting::{flash, fog}
#endif

struct GrassParams {
    ambient: vec4<f32>,
    diffuse: vec4<f32>,
    emissive: vec4<f32>,
    fog_color: vec4<f32>,
    fog_range: vec4<f32>,
    shape: vec4<f32>,
    wind: vec4<f32>,
    // The lightning's light (`weather::Flash`): xyz where, w its radius
    // (0: none); its colour.
    flash_at: vec4<f32>,
    flash_colour: vec4<f32>,
}

// `GrassParams::shape.w`: see `grass.rs`.
const FOGGED: u32 = 1u;
const LIGHTMAPPED: u32 = 2u;
const ALPHA_TEST: f32 = 0.4;

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> grass: GrassParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var art_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var art_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var lightmap_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var lightmap_sampler: sampler;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) uv_b: vec2<f32>,
    @location(3) sway: f32,
    @location(4) base: vec3<f32>,
}

#ifdef PREPASS_PIPELINE
struct Varyings {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
#ifdef MOTION_VECTOR_PREPASS
    @location(1) world_position: vec3<f32>,
    @location(2) previous_world_position: vec3<f32>,
#endif
}
#else
struct Varyings {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) uv_b: vec2<f32>,
    @location(2) world_position: vec3<f32>,
    @location(3) fade: f32,
}
#endif

// The sway eases in from this share of the near radius² (12 m of 15 m).
const SWAY_EASE: f32 = 0.64;

/// How far the wind pushes a vertex (`sway` its share) of the cluster at
/// `base`, seen from `eye`, `seconds` into the sway's clock. The wind in
/// Bevy's space: game +X is Bevy's +X, game +Y its -Z; still past the near
/// radius, easing in from inside it.
fn pushed(base: vec3<f32>, eye: vec3<f32>, sway: f32, seconds: f32) -> vec2<f32> {
    let to_eye = base - eye;
    let distance2 = dot(to_eye, to_eye);
    let near2 = grass.shape.z;
    let wind = vec2<f32>(grass.wind.x, -grass.wind.y);
    let near = 1.0 - smoothstep(near2 * SWAY_EASE, near2, distance2);
    let gust = 0.6 + 0.4 * sin(seconds * 1.7 + base.x * 0.3 + base.z * 0.3);
    return wind * sway * grass.shape.x * gust * near;
}

/// The vertex in the world, swayed, of a cluster at `base`.
fn swayed(world_from_local: mat4x4<f32>, vertex: Vertex, base: vec3<f32>, eye: vec3<f32>, seconds: f32) -> vec3<f32> {
    var world = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(vertex.position, 1.0)
    ).xyz;
    let push = pushed(base, eye, vertex.sway, seconds);
    world.x += push.x;
    world.z += push.y;
    return world;
}

/// The cluster's own point, so its four corners agree.
fn base_of(world_from_local: mat4x4<f32>, vertex: Vertex) -> vec3<f32> {
    return mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(vertex.base, 1.0)
    ).xyz;
}

/// The cluster's alpha at `base`: whole to the near radius, nothing at the
/// far one.
fn fade_at(base: vec3<f32>) -> f32 {
    let to_eye = base - view.world_position;
    let distance2 = dot(to_eye, to_eye);
    let far2 = grass.shape.y;
    let near2 = grass.shape.z;
    return clamp(1.0 - (distance2 - near2) / (far2 - near2), 0.0, 1.0);
}

/// Whether the cluster at `base` is past the far radius: then its four
/// corners go to one point and nothing is drawn.
fn beyond(base: vec3<f32>) -> bool {
    let to_eye = base - view.world_position;
    return dot(to_eye, to_eye) >= grass.shape.y;
}

#ifdef PREPASS_PIPELINE

@vertex
fn vertex(vertex: Vertex) -> Varyings {
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    let base = base_of(world_from_local, vertex);
    var out: Varyings;
    if beyond(base) {
        out.position = vec4<f32>(0.0, 0.0, 0.0, 1.0);
        return out;
    }
    let world = swayed(world_from_local, vertex, base, view.world_position, grass.wind.z);
    out.position = position_world_to_clip(world);
    out.uv = vertex.uv;
#ifdef MOTION_VECTOR_PREPASS
    out.world_position = world;
    // The last frame's: its transform, its eye (the inverse of its rigid
    // view), its clock.
    let previous_from_local = mesh_functions::get_previous_world_from_local(vertex.instance_index);
    let last_view = previous_view_uniforms.view_from_world;
    let turn = mat3x3<f32>(last_view[0].xyz, last_view[1].xyz, last_view[2].xyz);
    let last_eye = -(transpose(turn) * last_view[3].xyz);
    out.previous_world_position = swayed(
        previous_from_local,
        vertex,
        base_of(previous_from_local, vertex),
        last_eye,
        grass.wind.w,
    );
#endif
    return out;
}

/// Whether the main pass draws this texel at all (its alpha test).
fn covered(in: Varyings) -> bool {
    return textureSampleBias(art_texture, art_sampler, in.uv, view.mip_bias).a > ALPHA_TEST;
}

#ifdef MOTION_VECTOR_PREPASS
struct Motion {
    @location(1) motion_vector: vec2<f32>,
}

/// Where the grass is drawn, the screen motion from the last frame to this
/// one, as Bevy's prepass measures it: the unjittered clip positions'
/// difference, halved, y down.
@fragment
fn fragment(in: Varyings) -> Motion {
    if !covered(in) {
        discard;
    }
    let now = view.unjittered_clip_from_world * vec4<f32>(in.world_position, 1.0);
    let then = previous_view_uniforms.clip_from_world * vec4<f32>(in.previous_world_position, 1.0);
    var out: Motion;
    out.motion_vector = (now.xy / now.w - then.xy / then.w) * vec2<f32>(0.5, -0.5);
    return out;
}
#else
@fragment
fn fragment(in: Varyings) {
    if !covered(in) {
        discard;
    }
}
#endif

#else

@vertex
fn vertex(vertex: Vertex) -> Varyings {
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    let base = base_of(world_from_local, vertex);
    var out: Varyings;
    if beyond(base) {
        out.position = vec4<f32>(0.0, 0.0, 0.0, 1.0);
        return out;
    }
    let world = swayed(world_from_local, vertex, base, view.world_position, grass.wind.z);
    out.position = position_world_to_clip(world);
    out.uv = vertex.uv;
    out.uv_b = vertex.uv_b;
    out.world_position = world;
    out.fade = fade_at(base);
    return out;
}

/// The motion twin's main pass: every corner at one point, nothing drawn
/// (it draws only in the prepass).
@vertex
fn hidden(vertex: Vertex) -> Varyings {
    var out: Varyings;
    out.position = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    return out;
}

@fragment
fn fragment(in: Varyings) -> @location(0) vec4<f32> {
    // The view's mip bias, as on the scene's surfaces (`odyssey.wgsl`); the
    // lightmap takes none.
    let texel = textureSampleBias(art_texture, art_sampler, in.uv, view.mip_bias);
    let mapped = textureSample(lightmap_texture, lightmap_sampler, in.uv_b).rgb;
    if texel.a <= ALPHA_TEST {
        discard;
    }
    let flags = u32(grass.shape.w);
    let lightmap = select(vec3<f32>(1.0), mapped, (flags & LIGHTMAPPED) != 0u);
    // A lightning flash adds its light, as on the scene's surfaces.
    let lit = flash(in.world_position, grass.flash_at, grass.flash_colour.rgb);
    var colour = texel.rgb * min(lightmap + lit, vec3<f32>(1.0));
    if (flags & FOGGED) != 0u {
        colour = fog(colour, in.world_position, grass.fog_range, grass.fog_color.rgb);
    }
    return vec4<f32>(colour, texel.a * in.fade);
}

#endif
