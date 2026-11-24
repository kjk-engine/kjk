// A robe skirt's or a cape's mesh (`loose_skin.rs`). It is skinned as
// Bevy skins it, then each vertex is pushed out of its wearer's body
// capsules. The robe solver moves only the robe's bones, and the skin
// between two bones is blended between them, so a leg that swings between
// two bones would pass through the cloth.
//
// The push is described in `loose_legs.wgsl`'s `out_of_legs`. Nothing
// moves at rest. The wearer's capsules are found by the model in the mesh's tag
// (`scene::owner_tag`). A mesh with no owner is not pushed.

#import bevy_pbr::{
    mesh_functions,
    skinning,
    view_transformations::position_world_to_clip,
}
#import kjk::loose_legs::{Legs, fold_shade, out_of_legs, rippled, rippled_normal}
#ifndef PREPASS_PIPELINE
#import bevy_pbr::mesh_view_bindings::view
#endif

// How much brighter the cloth gets where it turns away from the eye, with
// `LOOSE_SHEEN` (`KJK_LOOSE_SHEEN=1`): the soft rim that wool and linen
// catch at a grazing angle.
const SHEEN: f32 = 0.35;
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

@vertex
fn vertex(
    vertex: Vertex,
    @location(#{LOOSE_PUSH}) push: vec4<f32>,
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
    var at = skinned.xyz;
#ifdef VERTEX_NORMALS
#ifdef SKINNED
    var normal = skinning::skin_normals(world_from_local, vertex.normal);
#else
    var normal = mesh_functions::mesh_normal_local_to_world(vertex.normal, vertex.instance_index);
#endif
    let skin_normal = normal;
#endif
    if slot != NO_WEARER {
#ifdef VERTEX_NORMALS
        at = rippled(at, normal, push, wearers[slot].now, wearers[slot].ripples);
        normal = rippled_normal(normal, push, wearers[slot].ripples);
#endif
        at = out_of_legs(at, wearers[slot].now, push);
    }
    out.world_position = vec4<f32>(at, 1.0);
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
#ifndef LOOSE_PAINT
    if slot != NO_WEARER {
        var shade = fold_shade(push, wearers[slot].ripples);
#ifdef LOOSE_SHEEN
#ifdef VERTEX_NORMALS
#ifndef PREPASS_PIPELINE
        let grazing = 1.0 - abs(dot(normalize(normal), normalize(view.world_position - at)));
        shade = shade * (1.0 + SHEEN * grazing * grazing * grazing);
#endif
#endif
#endif
        out.color = vec4<f32>(vertex.color.rgb * shade, vertex.color.a);
    }
#endif
#endif
#ifdef PREPASS_PIPELINE
#ifdef NORMAL_PREPASS_OR_DEFERRED_PREPASS
#ifdef VERTEX_NORMALS
    out.world_normal = normal;
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
    var was = before.xyz;
    if slot != NO_WEARER {
#ifdef VERTEX_NORMALS
        was = rippled(was, skin_normal, push, wearers[slot].was, wearers[slot].ripples_was);
#endif
        was = out_of_legs(was, wearers[slot].was, push);
    }
    out.previous_world_position = vec4<f32>(was, 1.0);
#endif
#else
#ifdef VERTEX_NORMALS
    out.world_normal = normal;
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
