// Filmic tonemapping + sRGB encode (`rolloff.rs`), and pre-bloom linear
// decode (`decode_linear`) for Enhanced cameras using multi-sample MSAA.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
#import kjk::lighting::{decode_22, encode, encode_22}

@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var nearest: sampler;
// `params.x`: 0 = AgX, 1 = AgX Punchy, 2 = ACES Narkowicz, 3 = legacy KNEE roll-off.
@group(0) @binding(2) var<uniform> params: vec4<u32>;

const KNEE: f32 = 0.9;
const MIN_EV: f32 = -12.47393;
const MAX_EV: f32 = 4.026069;

const AGX_INSET: mat3x3<f32> = mat3x3<f32>(
    vec3<f32>(0.842479062253094, 0.0423282422610123, 0.0423756549057051),
    vec3<f32>(0.0784335999999992, 0.878468636469772, 0.0784336),
    vec3<f32>(0.0792237451477643, 0.0791661274605434, 0.879142973793104),
);

const AGX_OUTSET: mat3x3<f32> = mat3x3<f32>(
    vec3<f32>(1.19687900512017, -0.0528968517574562, -0.0529716355144438),
    vec3<f32>(-0.0980208811401368, 1.15190312990417, -0.0980434501171241),
    vec3<f32>(-0.0990297440797205, -0.0989611768448433, 1.15107367264116),
);

fn agx_default_contrast_approx(x: vec3<f32>) -> vec3<f32> {
    let x2 = x * x;
    let x4 = x2 * x2;
    return 15.5 * x4 * x2
        - 40.14 * x4 * x
        + 31.96 * x4
        - 6.868 * x2 * x
        + 0.4298 * x2
        + 0.1191 * x
        - 0.00232;
}

fn agx(value: vec3<f32>, punchy: bool) -> vec3<f32> {
    var v = AGX_INSET * value;
    v = max(v, vec3<f32>(1e-10));
    v = log2(v);
    v = clamp((v - vec3<f32>(MIN_EV)) / (MAX_EV - MIN_EV), vec3<f32>(0.0), vec3<f32>(1.0));
    v = agx_default_contrast_approx(v);
    if punchy {
        let luma = dot(v, vec3<f32>(0.2126, 0.7152, 0.0722));
        v = pow(max(v, vec3<f32>(0.0)), vec3<f32>(1.35));
        v = mix(vec3<f32>(luma), v, 1.4);
    }
    v = AGX_OUTSET * v;
    v = clamp(v, vec3<f32>(0.0), vec3<f32>(1.0));
    return encode(decode_22(v));
}

fn aces(x: vec3<f32>) -> vec3<f32> {
    let mapped = saturate((x * (2.51 * x + vec3<f32>(0.03))) / (x * (2.43 * x + vec3<f32>(0.59)) + vec3<f32>(0.14)));
    return encode(mapped);
}

fn roll_off(value: vec3<f32>) -> vec3<f32> {
    let room = 1.0 - KNEE;
    let over = max(value - vec3<f32>(KNEE), vec3<f32>(0.0));
    return min(value, vec3<f32>(KNEE)) + room * (vec3<f32>(1.0) - exp(-over / room));
}

@fragment
fn decode_linear(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let drawn = textureSample(scene, nearest, in.uv);
    return vec4<f32>(decode_22(max(drawn.rgb, vec3<f32>(0.0))), drawn.a);
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let drawn = textureSample(scene, nearest, in.uv);
    let linear = max(drawn.rgb, vec3<f32>(0.0));
    var out: vec3<f32>;
    if params.x == 0u {
        out = agx(linear, false);
    } else if params.x == 1u {
        out = agx(linear, true);
    } else if params.x == 2u {
        out = aces(linear);
    } else {
        out = roll_off(encode_22(linear));
    }
    return vec4<f32>(out, drawn.a);
}
