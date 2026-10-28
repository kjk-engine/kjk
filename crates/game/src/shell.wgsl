// A shell over a model (`shell.rs`): KOTOR's ProgFX shell pass, which
// draws a character's skinned meshes again pushed out along their normals
// in a texture of its own (the stealth field's `fx_tex_stealth`), added to
// what is behind. Bevy's mesh vertex stage (skinning and all), the position
// pushed first.

#import bevy_pbr::{
    mesh_functions,
    skinning,
    forward_io::{Vertex, VertexOutput},
    view_transformations::position_world_to_clip,
}

struct ShellParams {
    tint: vec4<f32>,
    // x: how far each vertex is pushed along its normal (metres); y: how
    // far the texture has scrolled.
    push: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> params: ShellParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var shell_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var shell_sampler: sampler;

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
    var position = vertex.position;
#ifdef VERTEX_NORMALS
    position = position + normalize(vertex.normal) * params.push.x;
#ifdef SKINNED
    out.world_normal = skinning::skin_normals(world_from_local, vertex.normal);
#else
    out.world_normal = mesh_functions::mesh_normal_local_to_world(
        vertex.normal,
        vertex.instance_index
    );
#endif
#endif
    out.world_position = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(position, 1.0)
    );
    out.position = position_world_to_clip(out.world_position.xyz);
#ifdef VERTEX_UVS_A
    out.uv = vertex.uv;
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = vertex.instance_index;
#endif
    return out;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
#ifdef VERTEX_UVS_A
    let texel = textureSample(shell_texture, shell_sampler, in.uv + vec2<f32>(params.push.y, 0.0));
#else
    let texel = vec4<f32>(1.0);
#endif
    // Bevy blends `AlphaMode::Add` as premultiplied alpha: an alpha of 0
    // keeps all of what is behind, so the colour is purely added.
    return vec4<f32>(texel.rgb * params.tint.rgb * params.tint.a, 0.0);
}
