// The screens over the world in gamma space (`ui_composite.rs`).

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

// Both encoded; the screens premultiplied.
@group(0) @binding(0) var world: texture_2d<f32>;
@group(0) @binding(1) var screens: texture_2d<f32>;
@group(0) @binding(2) var nearest: sampler;
// x: the brightness's gamma.
@group(0) @binding(3) var<uniform> settings: vec4<f32>;

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let below = textureSample(world, nearest, in.uv);
    let over = textureSample(screens, nearest, in.uv);
    let blended = over.rgb + (1.0 - over.a) * below.rgb;
    let shown = pow(clamp(blended, vec3(0.0), vec3(1.0)), vec3(1.0 / settings.x));
    return vec4(shown, below.a);
}
