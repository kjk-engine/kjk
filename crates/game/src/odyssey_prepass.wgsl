// An Odyssey mesh drawn before the main pass, into a camera's depth and
// motion when it resolves frames over time (`temporal`).
//
// The main pass tests against this depth, so a surface covers exactly what
// its main pass draws. An opaque surface covers all of itself, except where
// it discards alpha 0 (`CUT_ZERO_ALPHA`, as `odyssey.wgsl` does). A motion
// proxy (`MOTION_PROXY`, `motion_proxy`) covers where its surface is solid.
// Each writes how far it moved on the screen since the last frame. While the
// camera has ambient occlusion (`ambient_occlusion`), each also writes its
// normal.

#import bevy_pbr::prepass_io::VertexOutput
#ifdef PREPASS_FRAGMENT
#import bevy_pbr::prepass_io::FragmentOutput
#endif
#import bevy_pbr::mesh_view_bindings::view
#ifdef MOTION_VECTOR_PREPASS
#import bevy_pbr::prepass_bindings::previous_view_uniforms
#endif

// As `odyssey.wgsl` has them.
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
    slot: u32,
    glow: vec4<f32>,
}

struct Animated {
    illum_alpha: vec4<f32>,
    uv: vec4<f32>,
}

// Flags: see `material.rs`.
const OPAQUE_ALPHA: u32 = 2u;
const ENVMAP: u32 = 4u;
const VERTEX_ALPHA: u32 = 64u;
const OPAQUE_TEXTURE: u32 = 256u;

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> params: OdysseyParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var base_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var base_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(9) var<storage, read> animated: array<Animated, 1024>;

// The alpha `odyssey.wgsl` draws a fragment with. It is the texture's alpha
// (read at its slot's offset), times the tint's and the slot's, times the
// vertex's where the surface fades by it. An env-mapped texture's alpha is
// its shine's mask, not coverage.
fn coverage(in: VertexOutput) -> f32 {
    let moving = animated[params.slot];
    let tint = params.tint.a * moving.illum_alpha.a;
#ifdef VERTEX_UVS_A
    let uv = in.uv + params.uv_offset + moving.uv.xy;
#else
    let uv = vec2<f32>(0.0);
#endif
    var texel = textureSample(base_texture, base_sampler, uv).a;
    if (params.flags & OPAQUE_TEXTURE) != 0u {
        texel = 1.0;
    }
    var alpha = select(texel * tint, tint, (params.flags & ENVMAP) != 0u);
    alpha = select(alpha, 1.0, (params.flags & OPAQUE_ALPHA) != 0u);
#ifdef VERTEX_COLORS
    if (params.flags & VERTEX_ALPHA) != 0u {
        alpha = alpha * in.color.a;
    }
#endif
    return alpha;
}

// Leaves out what the surface's main pass does not draw.
fn cut(in: VertexOutput) {
#ifdef MOTION_PROXY
    if coverage(in) < f32(#{PROXY_SOLID_PERMILLE}) / 1000.0 {
        discard;
    }
#else
#ifdef CUT_ZERO_ALPHA
    if coverage(in) == 0.0 {
        discard;
    }
#endif
#endif
}

#ifdef NORMAL_PREPASS
// The normal of the side the camera sees, for ambient occlusion
// (`ambient_occlusion`). A mesh without normals takes its face's own, from
// the slopes of its position over the screen. A face seen from behind (a
// mesh drawn from both sides) takes its normal turned round.
fn seen_normal(in: VertexOutput, front: bool) -> vec3<f32> {
    let flat = cross(dpdy(in.world_position.xyz), dpdx(in.world_position.xyz));
    let to_eye = view.world_position - in.world_position.xyz;
    var normal = select(flat, -flat, dot(flat, to_eye) < 0.0);
#ifdef VERTEX_NORMALS
    if dot(in.world_normal, in.world_normal) > 1e-8 {
        normal = select(-in.world_normal, in.world_normal, front);
    }
#endif
    return normalize(normal);
}
#endif

#ifdef PREPASS_FRAGMENT
@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) front: bool) -> FragmentOutput {
#ifdef NORMAL_PREPASS
    // Before the cut, so the slopes are taken where every fragment runs.
    let normal = seen_normal(in, front);
#endif
    cut(in);
    var out: FragmentOutput;
#ifdef NORMAL_PREPASS
    out.normal = vec4<f32>(normal * 0.5 + vec3<f32>(0.5), 1.0);
#endif
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    out.frag_depth = in.unclipped_depth;
#endif
#ifdef MOTION_VECTOR_PREPASS
    // Where it is now, unjittered, less where it was. The screen's span is
    // 2 in clip space and 1 in texture space, whose y runs down.
    let now = view.unjittered_clip_from_world * in.world_position;
    let then = previous_view_uniforms.clip_from_world * in.previous_world_position;
    out.motion_vector = (now.xy / now.w - then.xy / then.w) * vec2<f32>(0.5, -0.5);
#endif
    return out;
}
#else
// A prepass of depth alone.
@fragment
fn fragment(in: VertexOutput) {
    cut(in);
}
#endif
