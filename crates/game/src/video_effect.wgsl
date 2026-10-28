// The video effect's pass over the scene's frame (`video_effect.rs`).

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

// Encoded values.
@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var nearest: sampler;
// tint: the modulation (rgb) and saturation (a); noise: its strength (x)
// and the frame's seed (y).
struct Grade {
    tint: vec4<f32>,
    noise: vec4<f32>,
}
@group(0) @binding(2) var<uniform> grade: Grade;

// How much each channel weighs in a pixel's grey.
const GREY: vec3<f32> = vec3<f32>(0.3086, 0.6094, 0.0820);

// A pixel's random channels for a frame, each 0 to 1.
fn random(pixel: vec2<u32>, seed: u32) -> vec3<f32> {
    var h = pixel.x * 1973u + pixel.y * 9277u + seed * 26699u;
    var out = vec3<f32>(0.0);
    for (var channel = 0; channel < 3; channel++) {
        h = (h ^ 61u) ^ (h >> 16u);
        h = h * 9u;
        h = h ^ (h >> 4u);
        h = h * 0x27d4eb2du;
        h = h ^ (h >> 15u);
        out[channel] = f32(h & 0xffffu) / 65535.0;
    }
    return out;
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let drawn = textureSample(scene, nearest, in.uv);
    let grey = vec3<f32>(dot(drawn.rgb, GREY));
    var shown = mix(grey, drawn.rgb, grade.tint.a) * grade.tint.rgb;
    let pixel = vec2<u32>(in.position.xy);
    shown += random(pixel, u32(grade.noise.y)) * grade.noise.x;
    return vec4<f32>(clamp(shown, vec3<f32>(0.0), vec3<f32>(1.0)), drawn.a);
}
