// The executable's frame buffer distortion (`RenderForceDistortionARB`):
// the frame shown displaced by its procedural `distortiontex`, a 32 × 32
// ring texture (`ringtexdistort`) stretched over the whole screen, centred
// on it, sampled linearly and clamped. Each texel (i, j) of it holds
// 0.5 + 0.03 cos32(60 r + 25 phase), r its distance from the centre
// ((i + 1) / 32 − 0.5, (j + 1) / 32 − 0.5), cos32 the executable's
// 32-step cosine table, kept as a byte; the phase runs a radian a second,
// round 2π. The frame is moved (2c − 1) / 10 of the screen along x and
// along y alike (its red and green are the same). Every surface that shows
// the distortion reads it here: `heat_haze` (the distortion-class models)
// and `glow_shell` (ProgFX 1800 on a character: effect 8000, stealth).
// Loaded once, by `heat_haze::HeatHazePlugin`.

#define_import_path kjk::ring_distort

#import bevy_pbr::mesh_view_bindings::globals
#import kjk::screen_copy::frame_at

const TAU: f32 = 6.2831855;
/// The table's step (the executable's 0.19634955, 2π / 32).
const STEP: f32 = 0.19634955;
const SIZE: f32 = 32.0;
/// The executable's amplitude, its two frequencies and its speed
/// (`RingDistortTextureController::Control`).
const AMPLITUDE: f32 = 0.03;
const RINGS: f32 = 60.0;
const SWEEP: f32 = 25.0;
const SPEED: f32 = 1.0;
/// How far the program moves the frame for a texel's full swing (the
/// program's 10.0: a tenth of the screen).
const REACH: f32 = 10.0;

/// Texel (i, j)'s value, as the executable's generator writes its byte.
fn ring(texel: vec2<f32>, phase: f32) -> f32 {
    let f = (clamp(texel, vec2<f32>(0.0), vec2<f32>(SIZE - 1.0)) + 1.0) / SIZE - 0.5;
    let angle = RINGS * length(f) + SWEEP * phase;
    let index = f32(u32(angle / STEP) & 31u);
    let value = 0.5 + AMPLITUDE * cos(index * TAU / SIZE);
    return floor(clamp(value * 255.0, 0.0, 255.0)) / 255.0;
}

/// The frame behind the point `screen` (0 to 1 across the drawn frame, as
/// `screen_copy::on_frame` gives it), displaced by the rings.
fn behind_rings(screen: vec2<f32>) -> vec4<f32> {
    // The texture's coordinates run up the screen (GL's).
    let gl = vec2<f32>(screen.x, 1.0 - screen.y);
    let phase = (globals.time * SPEED) % TAU;
    // Linear filtering between texel centres, clamped at the edges.
    let at = gl * SIZE - 0.5;
    let base = floor(at);
    let t = at - base;
    let c = mix(
        mix(ring(base, phase), ring(base + vec2<f32>(1.0, 0.0), phase), t.x),
        mix(ring(base + vec2<f32>(0.0, 1.0), phase), ring(base + vec2<f32>(1.0, 1.0), phase), t.x),
        t.y,
    );
    let moved = (2.0 * c - 1.0) / REACH;
    return frame_at(vec2<f32>(screen.x + moved, screen.y - moved));
}
