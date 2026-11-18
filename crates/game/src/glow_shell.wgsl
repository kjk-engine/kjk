// A shell (`glow_shell`): the mesh as Bevy draws it (its skin included),
// each vertex pushed out along its normal by `push.x` metres, as KOTOR's
// shell pass does (its vertex program adds the skinned normal times
// `program.env[12].x`, 0.02). A glow's is drawn with its texture by the
// mesh's own UVs at full colour (`program.env[12].w`, 1), unlit (times
// `tint`: white for a shell, a mark's stage's colour, `body_marks`); like the
// scene's material (`odyssey.wgsl`) it samples and outputs stored values.
// A distortion's (`push.y` set) shows the scene behind it (Bevy's view
// transmission texture) moved by the executable's ring texture.
// A hologram's (`push.w` set) is TSL's hologram pass: the luminance of the
// mesh's own texture times the filter texture, the filter's alpha, the
// filter sampled at the length of the game's world x and z (u) and the
// game's world height (v), in metres (`glow_shell::Glow::hologram`).

#import bevy_pbr::{
    mesh_functions,
    skinning,
    forward_io::{Vertex, VertexOutput},
    mesh_view_bindings::view,
    view_transformations::position_world_to_clip,
}
#import kjk::screen_copy::on_frame
#import kjk::ring_distort::behind_rings
#ifdef DANGLY
#import kjk::sway::{SwayVertex, last_slot}
#endif

struct Shell {
    push: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> shell: Shell;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var glow_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var glow_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var<uniform> tint: vec4<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var filter_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var filter_sampler: sampler;

#ifdef DANGLY
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var<storage, read> sway: array<SwayVertex>;
#endif

// The weights TSL's hologram fragment programs take luminance by
// (`glow_shell::HOLOGRAM_LUMINANCE`).
const HOLOGRAM_LUMINANCE: vec3<f32> = vec3<f32>(0.30, 0.59, 0.11);

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // A distortion (`push.y` set): the scene behind, displaced by the
    // executable's ring texture alone (`ring_distort.wgsl`).
    if shell.push.y > 0.0 {
        return vec4<f32>(behind_rings(on_frame(in.position.xy)).rgb, 1.0);
    }
#ifdef VERTEX_UVS_A
    // A hologram (`push.w` set). The executable's hologram vertex program
    // takes the game's world x and z (its env[16] is 0, 2, 3, 1) for u and
    // the game's height z for v. Bevy's world x is the game's x, its y the
    // game's z.
    if shell.push.w > 0.0 {
        let world = in.world_position.xyz;
        let at = vec2<f32>(length(world.xy), world.y);
        let screen = textureSample(filter_texture, filter_sampler, at);
        let texel = textureSampleBias(glow_texture, glow_sampler, in.uv, view.mip_bias);
        let luminance = dot(texel.rgb, HOLOGRAM_LUMINANCE);
        return vec4<f32>(luminance * screen.rgb, screen.a);
    }
    // A mark (`push.z` set) shows only within its square: its triangles
    // reach past it, and KOTOR's are large beside a mark.
    let outside = any(in.uv < vec2<f32>(0.0)) || any(in.uv > vec2<f32>(1.0));
    if shell.push.z > 0.0 && outside {
        return vec4<f32>(0.0);
    }
    // The view's mip bias, as on the scene's surfaces (`odyssey.wgsl`).
    return textureSampleBias(glow_texture, glow_sampler, in.uv, view.mip_bias) * tint;
#else
    return vec4<f32>(0.0);
#endif
}

@vertex
fn vertex(
    vertex: Vertex,
#ifdef DANGLY
    // `dangly::SLOT_LOCATION`.
    @location(8) slot: u32,
#endif
) -> VertexOutput {
    var out: VertexOutput;
#ifdef DANGLY
    let now = sway[min(slot, last_slot(arrayLength(&sway)))].now;
    let position = vec3<f32>(now[0], now[1], now[2]);
#else
    let position = vertex.position;
#endif
#ifdef SKINNED
    let world_from_local = skinning::skin_model(
        vertex.joint_indices,
        vertex.joint_weights,
        vertex.instance_index
    );
#else
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
#endif
    let world = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(position, 1.0)
    );
    var push = vec3<f32>(0.0);
#ifdef VERTEX_NORMALS
#ifdef SKINNED
    out.world_normal = skinning::skin_normals(world_from_local, vertex.normal);
#else
    out.world_normal = mesh_functions::mesh_normal_local_to_world(
        vertex.normal,
        vertex.instance_index
    );
#endif
    push = normalize(out.world_normal) * shell.push.x;
#endif
    out.world_position = vec4<f32>(world.xyz + push, 1.0);
    out.position = position_world_to_clip(out.world_position.xyz);
#ifdef VERTEX_UVS_A
    out.uv = vertex.uv;
#endif
#ifdef VERTEX_UVS_B
    out.uv_b = vertex.uv_b;
#endif
#ifdef VERTEX_COLORS
    out.color = vertex.color;
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = vertex.instance_index;
#endif
    return out;
}
