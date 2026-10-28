// The area's grass (`grass.rs`): upright quads in game space under the
// scene's root. Each cluster goes by its own point (`base`): within the
// near radius its top edge sways in the area's wind, its alpha fades from
// the near radius to nothing at the far one, and past that it is not drawn.
// The texture is alpha tested at 0.4 (EXE-NOTES `GRASS_BLEND`) and lit by
// the room's lightmap (`GRASS_LIGHT`; the area's colours are not applied,
// see `grass.rs`); then the area's fog, as `odyssey.wgsl` does
// (`lighting.wgsl`).
// Like the scene's material, it multiplies and outputs stored values.

#import bevy_pbr::{
    mesh_functions,
    mesh_view_bindings::view,
    view_transformations::position_world_to_clip,
}
#import kjk::lighting::{flash, fog}

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

struct Varyings {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) uv_b: vec2<f32>,
    @location(2) world_position: vec3<f32>,
    @location(3) fade: f32,
}

// The sway eases in from this share of the near radius² (12 m of 15 m).
const SWAY_EASE: f32 = 0.64;

@vertex
fn vertex(vertex: Vertex) -> Varyings {
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    var world = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(vertex.position, 1.0)
    ).xyz;
    // Everything goes by the cluster's own point, so its four corners agree.
    let base = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(vertex.base, 1.0)
    ).xyz;
    let to_eye = base - view.world_position;
    let distance2 = dot(to_eye, to_eye);
    let far2 = grass.shape.y;
    let near2 = grass.shape.z;
    var out: Varyings;
    if distance2 >= far2 {
        // Past the far radius: one point for all four corners, nothing drawn.
        out.position = vec4<f32>(0.0, 0.0, 0.0, 1.0);
        return out;
    }
    // The wind in Bevy's space: game +X is Bevy's +X, game +Y its -Z; still
    // past the near radius, easing in from inside it.
    let wind = vec2<f32>(grass.wind.x, -grass.wind.y);
    let near = 1.0 - smoothstep(near2 * SWAY_EASE, near2, distance2);
    let gust = 0.6 + 0.4 * sin(grass.wind.z * 1.7 + base.x * 0.3 + base.z * 0.3);
    let push = wind * vertex.sway * grass.shape.x * gust * near;
    world.x += push.x;
    world.z += push.y;
    out.position = position_world_to_clip(world);
    out.uv = vertex.uv;
    out.uv_b = vertex.uv_b;
    out.world_position = world;
    out.fade = clamp(1.0 - (distance2 - near2) / (far2 - near2), 0.0, 1.0);
    return out;
}

@fragment
fn fragment(in: Varyings) -> @location(0) vec4<f32> {
    let texel = textureSample(art_texture, art_sampler, in.uv);
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
