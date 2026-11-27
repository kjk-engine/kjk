// Composites gamma-encoded legacy transparents (`accum_colour`,
// `transmittance`) over the single-sample linear HDR scene (`scene`) while
// preserving linear HDR background highlights above 1.0.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
#import kjk::lighting::{decode_22, encode_22}

@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var accum_colour: texture_2d<f32>;
@group(0) @binding(2) var transmittance: texture_2d<f32>;
@group(0) @binding(3) var<uniform> bounds: vec4<u32>;

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let pixel = vec2<u32>(in.position.xy);
    let bg = textureLoad(scene, pixel, 0);
    if any(pixel < bounds.xy) || any(pixel >= bounds.zw) {
        return bg;
    }
    let accum = textureLoad(accum_colour, pixel, 0).rgb;
    let trans = textureLoad(transmittance, pixel, 0).rgb;
    let base_lin = clamp(bg.rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    let over_lin = max(bg.rgb - vec3<f32>(1.0), vec3<f32>(0.0));
    let blended_gamma = max(trans * encode_22(base_lin) + accum, vec3<f32>(0.0));
    let result = decode_22(blended_gamma) + over_lin * max(trans, vec3<f32>(0.0));
    return vec4<f32>(result, bg.a);
}
