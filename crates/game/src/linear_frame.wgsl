// The linear frame through the temporal pass (`linear_frame.rs`). Two
// passes over the scene's frame. `decode_frame` turns stored (gamma-encoded) values into
// linear light, and `encode_frame` turns linear light back into stored
// values. Both follow the sRGB curve, as `lighting.wgsl` does, and a value
// above 1 follows the same curve on.
//
// Each pass reads the pixel it writes. The frame it reads and the frame it
// writes are the same size, so the pixel's own place names the texel.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

@group(0) @binding(0) var scene: texture_2d<f32>;

fn decode(value: vec3<f32>) -> vec3<f32> {
    let v = max(value, vec3<f32>(0.0));
    let low = v / 12.92;
    let high = pow((v + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(high, low, v <= vec3<f32>(0.04045));
}

fn encode(value: vec3<f32>) -> vec3<f32> {
    let v = max(value, vec3<f32>(0.0));
    let low = v * 12.92;
    let high = 1.055 * pow(v, vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055);
    return select(high, low, v <= vec3<f32>(0.0031308));
}

@fragment
fn decode_frame(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let drawn = textureLoad(scene, vec2<i32>(in.position.xy), 0);
    return vec4<f32>(decode(drawn.rgb), drawn.a);
}

@fragment
fn encode_frame(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let drawn = textureLoad(scene, vec2<i32>(in.position.xy), 0);
    return vec4<f32>(encode(drawn.rgb), drawn.a);
}
