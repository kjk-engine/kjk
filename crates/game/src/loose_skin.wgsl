// A robe skirt's or a cape's mesh (`loose_skin.rs`). It is skinned as
// Bevy skins it, then each vertex is pushed out of its wearer's leg
// capsules. The robe solver moves only the robe's bones, and the skin
// between two bones is blended between them, so a leg that swings between
// two bones would pass through the cloth.
//
// `push.x` is the vertex's share of the garment, from 0 to 1: the sum of
// its skin weights on the bones the solver moves. It scales the push, so
// the waist, which the pelvis holds, is not pushed. `push.y` is how far
// inside a leg capsule the vertex rests in the rest pose. The vertex keeps
// that much less than each capsule's radius, so that nothing moves at
// rest. The wearer's capsules are found by the model in the mesh's tag
// (`scene::owner_tag`). A mesh with no owner is not pushed.

#import bevy_pbr::{
    mesh_functions,
    skinning,
    view_transformations::position_world_to_clip,
}
#import kjk::loose_legs::{Legs, out_of}
#ifdef PREPASS_PIPELINE
#import bevy_pbr::prepass_io::{Vertex, VertexOutput}
#else
#import bevy_pbr::forward_io::{Vertex, VertexOutput}
#endif

@group(#{MATERIAL_BIND_GROUP}) @binding(20) var<storage, read> wearers: array<Legs>;

// `shadows::OWNER_TAG`.
const OWNER_TAG: u32 = 0x80000000u;
const NO_WEARER: u32 = 0xffffffffu;

fn wearer(instance_index: u32) -> u32 {
    let tag = mesh_functions::get_tag(instance_index);
    if (tag & OWNER_TAG) == 0u || (tag & ~OWNER_TAG) >= arrayLength(&wearers) {
        return NO_WEARER;
    }
    return tag & ~OWNER_TAG;
}

fn pushed(at: vec3<f32>, slot: u32, push: vec2<f32>) -> vec3<f32> {
    var p = at;
    if slot == NO_WEARER || push.x <= 0.0 {
        return p;
    }
    for (var i = 0u; i < 6u; i += 1u) {
        p = out_of(p, wearers[slot].now[i], push);
    }
    return p;
}

fn pushed_before(at: vec3<f32>, slot: u32, push: vec2<f32>) -> vec3<f32> {
    var p = at;
    if slot == NO_WEARER || push.x <= 0.0 {
        return p;
    }
    for (var i = 0u; i < 6u; i += 1u) {
        p = out_of(p, wearers[slot].was[i], push);
    }
    return p;
}

@vertex
fn vertex(
    vertex: Vertex,
    @location(#{LOOSE_PUSH}) push: vec2<f32>,
) -> VertexOutput {
    var out: VertexOutput;
    let mesh_world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
#ifdef SKINNED
    let world_from_local = skinning::skin_model(
        vertex.joint_indices,
        vertex.joint_weights,
        vertex.instance_index
    );
#else
    let world_from_local = mesh_world_from_local;
#endif
    let slot = wearer(vertex.instance_index);
    let skinned = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(vertex.position, 1.0)
    );
    out.world_position = vec4<f32>(pushed(skinned.xyz, slot, push), 1.0);
    out.position = position_world_to_clip(out.world_position.xyz);
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    out.unclipped_depth = out.position.z;
    out.position.z = min(out.position.z, 1.0);
#endif
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
#ifdef SKINNED
    out.world_normal = skinning::skin_normals(world_from_local, vertex.normal);
#else
    out.world_normal = mesh_functions::mesh_normal_local_to_world(
        vertex.normal,
        vertex.instance_index
    );
#endif
#endif
#endif
#ifdef MOTION_VECTOR_PREPASS
#ifdef SKINNED
#ifdef HAS_PREVIOUS_SKIN
    let previous_from_local = skinning::skin_prev_model(
        vertex.joint_indices,
        vertex.joint_weights,
        vertex.instance_index
    );
#else
    let previous_from_local = mesh_functions::get_previous_world_from_local(vertex.instance_index);
#endif
#else
    let previous_from_local = mesh_functions::get_previous_world_from_local(vertex.instance_index);
#endif
    let before = mesh_functions::mesh_position_local_to_world(
        previous_from_local,
        vec4<f32>(vertex.position, 1.0)
    );
    out.previous_world_position = vec4<f32>(pushed_before(before.xyz, slot, push), 1.0);
#endif
#else
#ifdef VERTEX_NORMALS
#ifdef SKINNED
    out.world_normal = skinning::skin_normals(world_from_local, vertex.normal);
#else
    out.world_normal = mesh_functions::mesh_normal_local_to_world(
        vertex.normal,
        vertex.instance_index
    );
#endif
#endif
#ifdef VERTEX_TANGENTS
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(
        world_from_local,
        vertex.tangent,
        vertex.instance_index
    );
#endif
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = vertex.instance_index;
#endif
#ifdef VISIBILITY_RANGE_DITHER
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        vertex.instance_index,
        mesh_world_from_local[3],
    );
#endif
    return out;
}
