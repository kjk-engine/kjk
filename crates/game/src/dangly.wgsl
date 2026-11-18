// A danglymesh's vertex stage (`material`, `dangly`), in the main pass and
// in a camera's prepass. Each vertex reads where it is now, and where it was
// the frame before, from the sway list (`dangly::SWAY`) at its slot
// (`dangly::SLOT`), which `dangly::sway` writes as it moves them. The
// mesh's own positions are not read. Placed otherwise as Bevy places an
// unskinned mesh, so the prepass's depth matches the main pass's. Its motion
// is the sway's as well as the node's.

#import bevy_pbr::{
    mesh_functions,
    view_transformations::position_world_to_clip,
}
#ifdef PREPASS_PIPELINE
#import bevy_pbr::prepass_io::{Vertex, VertexOutput}
#else
#import bevy_pbr::forward_io::{Vertex, VertexOutput}
#endif

struct SwayVertex {
    now: array<f32, 3>,
    was: array<f32, 3>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(19) var<storage, read> sway: array<SwayVertex>;

fn place(at: array<f32, 3>) -> vec4<f32> {
    return vec4<f32>(at[0], at[1], at[2], 1.0);
}

@vertex
fn vertex(
    vertex: Vertex,
    // `dangly::SLOT_LOCATION`.
    @location(8) slot: u32,
) -> VertexOutput {
    let swayed = sway[min(slot, max(arrayLength(&sway), 1u) - 1u)];
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    out.world_position = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        place(swayed.now),
    );
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
#ifdef PREPASS_PIPELINE
#ifdef NORMAL_PREPASS_OR_DEFERRED_PREPASS
#ifdef VERTEX_NORMALS
    out.world_normal = mesh_functions::mesh_normal_local_to_world(
        vertex.normal,
        vertex.instance_index,
    );
#endif
#endif
#ifdef MOTION_VECTOR_PREPASS
    out.previous_world_position = mesh_functions::mesh_position_local_to_world(
        mesh_functions::get_previous_world_from_local(vertex.instance_index),
        place(swayed.was),
    );
#endif
#else
#ifdef VERTEX_NORMALS
    out.world_normal = mesh_functions::mesh_normal_local_to_world(
        vertex.normal,
        vertex.instance_index,
    );
#endif
#ifdef VERTEX_TANGENTS
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(
        world_from_local,
        vertex.tangent,
        vertex.instance_index,
    );
#endif
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
