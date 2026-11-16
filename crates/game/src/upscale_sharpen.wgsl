// The upscaler's sharpening (`upscale.rs`), run on the resolved frame at the
// window's size. It is contrast adaptive. A pixel is sharpened against its
// four neighbours by less where they already differ a lot, so an edge is not
// overshot into a halo.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

@group(0) @binding(0) var frame: texture_2d<f32>;
// The strength, from 0 to 1, in x.
@group(0) @binding(1) var<uniform> strength: vec4<f32>;

fn at(pixel: vec2<i32>, last: vec2<i32>) -> vec3<f32> {
    return max(textureLoad(frame, clamp(pixel, vec2(0), last), 0).rgb, vec3(0.0));
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let last = vec2<i32>(textureDimensions(frame)) - 1;
    let pixel = vec2<i32>(floor(in.position.xy));
    let centre = textureLoad(frame, pixel, 0);
    let up = at(pixel + vec2(0, -1), last);
    let left = at(pixel + vec2(-1, 0), last);
    let right = at(pixel + vec2(1, 0), last);
    let down = at(pixel + vec2(0, 1), last);
    let middle = max(centre.rgb, vec3(0.0));
    // The neighbourhood's range, read on values brought into 0 to 1, so a
    // value above 1 under Enhanced lighting does not turn the sharpening
    // off.
    let low = saturate(min(middle, min(min(up, left), min(right, down))));
    let high = saturate(max(middle, max(max(up, left), max(right, down))));
    // How much room there is to sharpen before a channel would leave 0 to
    // 1: none at a hard edge, most in a flat area.
    let room = sqrt(saturate(min(low, vec3(1.0) - high) / max(high, vec3(1e-4))));
    // The neighbours' weight, negative: from 0 at no strength to -1/5 at
    // full strength. At -1/4 the sum below would divide by 0.
    let weight = -room * 0.2 * saturate(strength.x);
    let sharpened = ((up + left + right + down) * weight + middle) / (4.0 * weight + 1.0);
    return vec4(max(sharpened, vec3(0.0)), centre.a);
}
