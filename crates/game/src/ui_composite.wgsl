// The screens over the world in gamma space (`ui_composite.rs`), with an
// optional seamless transition wipe between the previous menu snapshot and the
// new campaign's screens.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

struct CompositeSettings {
    // x: brightness gamma, y: transition progress (< 0.0 when inactive),
    // z: wipe direction (+1.0 left-to-right, -1.0 right-to-left), w: 1.0
    // when the screens are left out (a photo mode screenshot), else 0.0.
    params: vec4<f32>,
    // rgb: incoming campaign's accent glow along the wipe edge.
    tint: vec4<f32>,
};

// Both encoded; the screens premultiplied.
@group(0) @binding(0) var world: texture_2d<f32>;
@group(0) @binding(1) var screens: texture_2d<f32>;
@group(0) @binding(2) var previous: texture_2d<f32>;
@group(0) @binding(3) var nearest: sampler;
@group(0) @binding(4) var<uniform> settings: CompositeSettings;

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let below = textureSample(world, nearest, in.uv);
    let curr = textureSample(screens, nearest, in.uv);
    var over = curr;

    let progress = settings.params.y;
    if progress >= 0.0 && progress < 1.0 {
        let prev = textureSample(previous, nearest, in.uv);
        // Snappy cubic ease-out so the wipe responds immediately on click.
        let inv = 1.0 - progress;
        let t = 1.0 - inv * inv * inv;
        // Subtle cinematic diagonal Star Wars wipe.
        let diag = in.uv.x * 0.85 + in.uv.y * 0.15;
        let coord = select(1.0 - diag, diag, settings.params.z > 0.0);
        let feather = 0.20;
        let edge = mix(-0.05, 1.15, t);
        let reveal = 1.0 - smoothstep(edge - feather, edge + feather, coord);
        over = mix(prev, curr, reveal);

        // Luminous energy frontier tinted in the incoming campaign's colour,
        // fading in quickly and out smoothly at the end of the sweep.
        let dist = (coord - edge) / 0.06;
        let band = exp(-dist * dist);
        let envelope = sin(progress * 3.14159265);
        let glow = settings.tint.rgb * band * envelope * 0.38 * max(over.a, 0.25);
        over = vec4(over.rgb + glow, over.a);
    }

    if settings.params.w > 0.5 {
        over = vec4(0.0);
    }

    let blended = over.rgb + (1.0 - over.a) * below.rgb;
    let shown = pow(clamp(blended, vec3(0.0), vec3(1.0)), vec3(1.0 / settings.params.x));
    return vec4(shown, below.a);
}
