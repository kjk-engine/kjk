// An Odyssey mesh drawn before the main pass: into a shadow map
// (`shadows`), or into a camera's depth and motion when it resolves frames
// over time (`temporal`).
//
// A shadow map: only an alpha-tested surface comes here (one whose
// texture's alpha is coverage: hair, a grate), and it casts where its
// texture is mostly opaque.
//
// A camera: the main pass tests against this depth, so a surface covers
// exactly what its main pass draws: an opaque one all of itself but where
// it discards alpha 0 (`CUT_ZERO_ALPHA`, as `odyssey.wgsl` does), a motion
// proxy (`MOTION_PROXY`, `motion_proxy`) where its surface is solid. Each
// writes how far it moved on the screen since the last frame.

#import bevy_pbr::prepass_io::VertexOutput
#ifdef PREPASS_FRAGMENT
#import bevy_pbr::prepass_io::FragmentOutput
#endif
#ifdef MOTION_VECTOR_PREPASS
#import bevy_pbr::mesh_view_bindings::view
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

#ifdef MOTION_VECTOR_PREPASS
// The alpha `odyssey.wgsl` draws a fragment with: its texture's (with its
// slot's offset), its tint's and its slot's, its vertex's where it fades
// by it; an env-mapped texture's alpha is its shine's mask, not coverage.
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
#endif

// A shadow map's cut: where the texture is mostly opaque.
fn shadow_cut(in: VertexOutput) {
#ifdef VERTEX_UVS_A
    if textureSample(base_texture, base_sampler, in.uv).a < 0.5 {
        discard;
    }
#endif
}

#ifdef PREPASS_FRAGMENT
@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
#ifdef MOTION_VECTOR_PREPASS
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
#else
    shadow_cut(in);
#endif
    var out: FragmentOutput;
#ifdef NORMAL_PREPASS
    out.normal = vec4<f32>(normalize(in.world_normal) * 0.5 + vec3<f32>(0.5), 1.0);
#endif
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    out.frag_depth = in.unclipped_depth;
#endif
#ifdef MOTION_VECTOR_PREPASS
    // Where it is now, unjittered, less where it was: the screen's span
    // is 2 in clip space and 1 in texture space, whose y runs down.
    let now = view.unjittered_clip_from_world * in.world_position;
    let then = previous_view_uniforms.clip_from_world * in.previous_world_position;
    out.motion_vector = (now.xy / now.w - then.xy / then.w) * vec2<f32>(0.5, -0.5);
#endif
    return out;
}
#else
@fragment
fn fragment(in: VertexOutput) {
    shadow_cut(in);
}
#endif
