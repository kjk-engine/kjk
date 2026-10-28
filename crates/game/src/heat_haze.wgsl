// A distortion (`heat_haze`), as the executable draws its distortion-class
// models: where the particle's art passes the alpha test (GREATER 0) the
// frame is shown displaced by its procedural `distortiontex`, a 32 × 32
// ring texture (`ringtexdistort`) stretched over the whole screen, centred
// on it, sampled linearly and clamped. Each texel (i, j) of it holds
// 0.5 + 0.03 cos32(60 r + 25 phase), r its distance from the centre
// ((i + 1) / 32 − 0.5, (j + 1) / 32 − 0.5), cos32 the executable's
// 32-step cosine table, kept as a byte; the phase runs a radian a second,
// round 2π. The frame is moved (2c − 1) / 10 of the screen along x and
// along y alike (its red and green are the same).

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::{view, globals, view_transmission_texture, view_transmission_sampler}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var art: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var art_sampler: sampler;

const TAU: f32 = 6.2831855;
/// The table's step (the executable's 0.19634955, 2π / 32).
const STEP: f32 = 0.19634955;
const SIZE: f32 = 32.0;

/// Texel (i, j)'s value, as the executable's generator writes its byte.
fn ring(texel: vec2<f32>, phase: f32) -> f32 {
    let f = (clamp(texel, vec2<f32>(0.0), vec2<f32>(SIZE - 1.0)) + 1.0) / SIZE - 0.5;
    let angle = 60.0 * length(f) + 25.0 * phase;
    let index = f32(u32(angle / STEP) & 31u);
    let value = 0.5 + 0.03 * cos(index * TAU / SIZE);
    return floor(clamp(value * 255.0, 0.0, 255.0)) / 255.0;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
#ifdef VERTEX_UVS_A
    let uv = in.uv;
#else
    let uv = vec2<f32>(0.5);
#endif
#ifdef VERTEX_COLORS
    let fade = in.color.a;
#else
    let fade = 1.0;
#endif
    if textureSample(art, art_sampler, uv).a * fade <= 0.0 {
        discard;
    }
    let screen = (in.position.xy - view.viewport.xy) / view.viewport.zw;
    // The texture's coordinates run up the screen (GL's).
    let gl = vec2<f32>(screen.x, 1.0 - screen.y);
    let phase = globals.time % TAU;
    // Linear filtering between texel centres, clamped at the edges.
    let at = gl * SIZE - 0.5;
    let base = floor(at);
    let t = at - base;
    let c = mix(
        mix(ring(base, phase), ring(base + vec2<f32>(1.0, 0.0), phase), t.x),
        mix(ring(base + vec2<f32>(0.0, 1.0), phase), ring(base + vec2<f32>(1.0, 1.0), phase), t.x),
        t.y,
    );
    let moved = (2.0 * c - 1.0) / 10.0;
    let shown = clamp(vec2<f32>(screen.x + moved, screen.y - moved), vec2<f32>(0.0), vec2<f32>(1.0));
    let behind = textureSampleLevel(view_transmission_texture, view_transmission_sampler, shown, 0.0);
    return vec4<f32>(behind.rgb, 1.0);
}
