// An emitter's particles drawn from its own list (`model_fx::Spray`): its
// mesh is quads of nothing, and each vertex takes its place, texture
// coordinate and colour from the list by its index in the mesh. The list
// is written in place each frame; the mesh stays as it is.

#import bevy_pbr::{
    mesh_bindings::mesh,
    mesh_functions,
    forward_io::VertexOutput,
    view_transformations::position_world_to_clip,
}

struct GpuParticle {
    position: array<f32, 3>,
    uv: array<f32, 2>,
    colour: array<f32, 4>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(10) var<storage, read> particles: array<GpuParticle>;

// Its mesh's attributes are not read: the list holds all a vertex is.
struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @builtin(vertex_index) index: u32,
}

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    // The mesh's own index: the draw counts from where its vertices start.
    let p = particles[vertex.index - mesh[vertex.instance_index].first_vertex_index];
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    out.world_position = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(p.position[0], p.position[1], p.position[2], 1.0),
    );
    out.position = position_world_to_clip(out.world_position.xyz);
    out.world_normal = mesh_functions::mesh_normal_local_to_world(
        vec3<f32>(0.0, 0.0, 1.0),
        vertex.instance_index,
    );
#ifdef VERTEX_UVS_A
    out.uv = vec2<f32>(p.uv[0], p.uv[1]);
#endif
#ifdef VERTEX_COLORS
    out.color = vec4<f32>(p.colour[0], p.colour[1], p.colour[2], p.colour[3]);
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = vertex.instance_index;
#endif
#ifdef VISIBILITY_RANGE_DITHER
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        vertex.instance_index,
        world_from_local[3],
    );
#endif
    return out;
}
