// kjk's temporal upscaler (`upscale.rs`). The scene camera draws a smaller
// frame, jittered, into the top left corner of its main texture. This pass
// places that frame into a history kept at the window's size, guided by the
// motion vectors and the depth of the same smaller frame.
//
// It works on the frame's values as they are stored. Those values are
// gamma-encoded (`rolloff.rs`). Under a float frame they may go above 1.
// They are then brought into 0 to 1 by a curve that can be undone
// (`TONEMAP`), as Bevy's TAA does.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

struct Params {
    // The size of the smaller frame, in its pixels.
    drawn: vec2<f32>,
    // The size of the frame shown, in its pixels.
    shown: vec2<f32>,
    // The jitter of this frame, in the smaller frame's pixels
    // (`TemporalJitter::offset`). The frame was drawn shifted by minus this.
    jitter: vec2<f32>,
    // What a sample's weight is multiplied by, so that the weights of all
    // the jitter's positions average to 1.
    gain: f32,
    _unused: f32,
}

@group(0) @binding(0) var frame: texture_2d<f32>;
@group(0) @binding(1) var history: texture_2d<f32>;
@group(0) @binding(2) var motion_vectors: texture_2d<f32>;
@group(0) @binding(3) var depth: texture_depth_2d;
@group(0) @binding(4) var linear_sampler: sampler;
@group(0) @binding(5) var<uniform> params: Params;

struct Output {
    @location(0) shown: vec4<f32>,
    @location(1) history: vec4<f32>,
}

// The share of this frame a pixel takes while its history is trusted the
// least and the most. These are Bevy's TAA's rates.
const MOST_NEW: f32 = 0.1;
const LEAST_NEW: f32 = 0.015;
// The width of the neighbourhood the history is clamped to, in standard
// deviations of the smaller frame's 3 x 3 pixels around the point.
const CLAMP_WIDTH: f32 = 1.0;
// The width for a pixel that has stood still for 4 frames or more. The 3 x 3
// pixels move with the jitter, and at a third of the window's side they span
// 9 of the shown pixels. A clamp of 1 standard deviation then pulled a still
// pixel to a different colour each frame, which showed as a fine dither on
// the edges of blades and armour. Two standard deviations stop that. A pixel
// that has just stopped, or that something has just moved off, keeps the
// tighter clamp for its first frames. The tight clamp then still removes
// what moved away.
const STILL_CLAMP_WIDTH: f32 = 2.0;
// The confidence (below) a pixel reaches after 4 frames standing still.
const SETTLED: f32 = 41.0;

fn max3(x: vec3<f32>) -> f32 { return max(x.r, max(x.g, x.b)); }
fn tonemap(colour: vec3<f32>) -> vec3<f32> { return colour / (max3(colour) + 1.0); }
fn reverse_tonemap(colour: vec3<f32>) -> vec3<f32> { return colour / max(1.0 - max3(colour), 1e-4); }

fn to_ycocg(rgb: vec3<f32>) -> vec3<f32> {
    let y = (rgb.r / 4.0) + (rgb.g / 2.0) + (rgb.b / 4.0);
    let co = (rgb.r / 2.0) - (rgb.b / 2.0);
    let cg = (-rgb.r / 4.0) + (rgb.g / 2.0) - (rgb.b / 4.0);
    return vec3(y, co, cg);
}

fn to_rgb(ycocg: vec3<f32>) -> vec3<f32> {
    let r = ycocg.x + ycocg.y - ycocg.z;
    let g = ycocg.x + ycocg.z;
    let b = ycocg.x - ycocg.y - ycocg.z;
    return saturate(vec3(r, g, b));
}

// Moves the history's colour toward the box's centre until it is inside.
fn clip_to_box(colour: vec3<f32>, low: vec3<f32>, high: vec3<f32>) -> vec3<f32> {
    let centre = 0.5 * (high + low);
    let extent = 0.5 * (high - low) + 0.00000001;
    let offset = colour - centre;
    let reach = max3(abs(offset / extent));
    if reach > 1.0 {
        return centre + offset / reach;
    }
    return colour;
}

fn history_at(u: f32, v: f32) -> vec3<f32> {
    return textureSampleLevel(history, linear_sampler, vec2(u, v), 0.0).rgb;
}

// The history at `uv`, read with a 5-tap Catmull-Rom filter, as Bevy's TAA
// reads it.
fn history_filtered(uv: vec2<f32>) -> vec3<f32> {
    let size = params.shown;
    let texel = 1.0 / size;
    let position = uv * size;
    let centre = floor(position - 0.5) + 0.5;
    let f = position - centre;
    let w0 = f * (-0.5 + f * (1.0 - 0.5 * f));
    let w1 = 1.0 + f * f * (-2.5 + 1.5 * f);
    let w2 = f * (0.5 + f * (2.0 - 1.5 * f));
    let w3 = f * f * (-0.5 + 0.5 * f);
    let w12 = w1 + w2;
    let p0 = (centre - 1.0) * texel;
    let p3 = (centre + 2.0) * texel;
    let p12 = (centre + (w2 / w12)) * texel;
    var colour = history_at(p12.x, p0.y) * w12.x * w0.y;
    colour += history_at(p0.x, p12.y) * w0.x * w12.y;
    colour += history_at(p12.x, p12.y) * w12.x * w12.y;
    colour += history_at(p3.x, p12.y) * w3.x * w12.y;
    colour += history_at(p12.x, p3.y) * w12.x * w3.y;
    return max(colour, vec3(0.0));
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> Output {
    let uv = in.uv;
    // This pixel's centre on the smaller frame, in that frame's pixels.
    // Bevy adds the jitter to the projection's depth column, and the
    // perspective divide negates it, so the frame was drawn shifted back by
    // the jitter. A point of the scene lies that much further back on it.
    let at = uv * params.drawn - params.jitter;
    let last = vec2<i32>(params.drawn) - 1;
    let middle = vec2<i32>(floor(at));
    // How many of the shown frame's pixels one pixel of the smaller frame
    // spans.
    let spans = params.shown / params.drawn;

    var sum = vec3(0.0);
    var weights = 0.0;
    var moment_1 = vec3(0.0);
    var moment_2 = vec3(0.0);
    var nearest = 1e9;
    var closest_depth = -1.0;
    var closest = clamp(middle, vec2(0), last);
    var alpha = 1.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let pixel = middle + vec2(x, y);
            let read = clamp(pixel, vec2(0), last);
            let texel = textureLoad(frame, read, 0);
            var colour = max(texel.rgb, vec3(0.0));
#ifdef TONEMAP
            colour = tonemap(colour);
#endif
            // A Gaussian of the distance from this pixel's centre, in the
            // smaller frame's pixels.
            let apart = vec2<f32>(pixel) + 0.5 - at;
            let weight = exp(-2.0 * dot(apart, apart));
            sum += colour * weight;
            weights += weight;
            let ycocg = to_ycocg(colour);
            moment_1 += ycocg;
            moment_2 += ycocg * ycocg;
            let distance = dot(apart, apart);
            if distance < nearest {
                nearest = distance;
                alpha = texel.a;
            }
            // Reversed depth: the nearest surface has the greatest value.
            let d = textureLoad(depth, read, 0);
            if d > closest_depth {
                closest_depth = d;
                closest = read;
            }
        }
    }
    var current = sum / weights;

#ifdef RESET
    var confidence = 1.0;
#else
    // The nearest surface's motion, as Bevy's TAA picks it: an edge moves
    // with what is in front.
    let motion = textureLoad(motion_vectors, closest, 0).rg;
    let history_uv = uv - motion;
    var past = to_ycocg(history_filtered(history_uv));
    // The history is trusted more for every frame the pixel stands still.
    var confidence = textureSampleLevel(history, linear_sampler, uv, 0.0).a;
    let moved = abs(motion) * params.shown;
    let still = moved.x < 0.01 && moved.y < 0.01;
    // The clamp widens from 1 to 2 standard deviations over a still pixel's
    // first 4 frames.
    let settled = select(0.0, saturate((confidence - 1.0) / (SETTLED - 1.0)), still);
    let width = mix(CLAMP_WIDTH, STILL_CLAMP_WIDTH, settled);
    let mean = moment_1 / 9.0;
    let deviation = sqrt(max(moment_2 / 9.0 - mean * mean, vec3(0.0)));
    past = clip_to_box(past, mean - width * deviation, mean + width * deviation);
    let kept = to_rgb(past);

    if still {
        confidence += 10.0;
    } else {
        confidence = 1.0;
    }
    let rate = clamp(1.0 / confidence, LEAST_NEW, MOST_NEW);
    // A sample counts for more the nearer it fell to this pixel's centre,
    // measured in the shown frame's pixels.
    let off = sqrt(nearest) * max(spans.x, spans.y);
    let quality = exp(-2.0 * off * off);
    var share = saturate(rate * quality * params.gain);
    // A point that was outside the frame last frame has no history.
    if any(saturate(history_uv) != history_uv) {
        share = 1.0;
        confidence = 1.0;
    }
    current = mix(kept, current, share);
#endif

    var out: Output;
    out.history = vec4(current, confidence);
#ifdef TONEMAP
    current = reverse_tonemap(current);
#endif
    out.shown = vec4(current, alpha);
    return out;
}
