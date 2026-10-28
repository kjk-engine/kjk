// A shell (`glow_shell`): the mesh as Bevy draws it (its skin included),
// each vertex pushed out along its normal by `push.x` metres, as KOTOR's
// shell pass does (its vertex program adds the skinned normal times
// `program.env[12].x`, 0.02). A glow's is drawn with its texture by the
// mesh's own UVs at full colour (`program.env[12].w`, 1), unlit (times
// `tint`: white for a shell, a mark's stage's colour, `body_marks`); like the
// scene's material (`odyssey.wgsl`) it samples and outputs stored values.
// A distortion's (`push.y` its bend) shows the scene behind it (Bevy's view
// transmission texture) moved by its normal as seen.

#import bevy_pbr::{
    mesh_functions,
    skinning,
    forward_io::{Vertex, VertexOutput},
    mesh_view_bindings::{view, view_transmission_texture, view_transmission_sampler},
    view_transformations::position_world_to_clip,
}

struct Shell {
    push: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> shell: Shell;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var glow_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var glow_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var<uniform> tint: vec4<f32>;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    if shell.push.y > 0.0 {
        let turned = (view.view_from_world * vec4<f32>(in.world_normal, 0.0)).xyz;
        let seen = turned.xy / max(length(turned), 1e-4);
        let screen = (in.position.xy - view.viewport.xy) / view.viewport.zw;
        let bent = clamp(screen + vec2<f32>(seen.x, -seen.y) * shell.push.y, vec2<f32>(0.0), vec2<f32>(1.0));
        let behind = textureSampleLevel(view_transmission_texture, view_transmission_sampler, bent, 0.0);
        return vec4<f32>(behind.rgb, 1.0);
    }
#ifdef VERTEX_UVS_A
    // A mark (`push.z` set) shows only within its square: its triangles
    // reach past it, and KOTOR's are large beside a mark.
    let outside = any(in.uv < vec2<f32>(0.0)) || any(in.uv > vec2<f32>(1.0));
    if shell.push.z > 0.0 && outside {
        return vec4<f32>(0.0);
    }
    return textureSample(glow_texture, glow_sampler, in.uv) * tint;
#else
    return vec4<f32>(0.0);
#endif
}

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
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
        vec4<f32>(vertex.position, 1.0)
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
