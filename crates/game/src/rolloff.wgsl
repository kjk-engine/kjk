// The roll-off's pass over the scene's frame (`rolloff.rs`).

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

// Encoded values, which may be above 1.
@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var nearest: sampler;

// Where the bend starts. A channel below it is left as it is.
const KNEE: f32 = 0.9;

// One channel brought into 0 to 1. Below the knee it is returned unchanged,
// bit for bit. Above the knee it approaches 1 and never reaches it, so a
// brighter value always shows brighter.
fn roll_off(value: vec3<f32>) -> vec3<f32> {
    let room = 1.0 - KNEE;
    let over = max(value - vec3<f32>(KNEE), vec3<f32>(0.0));
    return min(value, vec3<f32>(KNEE)) + room * (vec3<f32>(1.0) - exp(-over / room));
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let drawn = textureSample(scene, nearest, in.uv);
    return vec4<f32>(roll_off(max(drawn.rgb, vec3<f32>(0.0))), drawn.a);
}
