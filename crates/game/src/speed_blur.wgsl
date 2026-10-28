// The swoop's speed blur (`speed_blur.rs`): a quad over the whole screen
// at the near plane shows the scene behind it (Bevy's view transmission
// texture, the scene before transparent surfaces) smeared toward the
// screen's middle, more toward the edges, by the course's blur ratio. The
// blend (alpha) is set in the pipeline.

#import bevy_pbr::forward_io::{Vertex, VertexOutput}
#import bevy_pbr::mesh_view_bindings::{view, view_transmission_texture, view_transmission_sampler}

// x: the ratio, y and z: where the smear begins and is whole (a share of
// the screen's height from its middle), w: the farthest a sample reaches
// (a share of the way to the middle).
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> strength: vec4<f32>;

const SAMPLES: i32 = 12;

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    // The quad's corners (-1 to 1) straight to the screen, at the near plane.
    out.position = vec4<f32>(vertex.position.xy, 1.0, 1.0);
    out.world_position = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    out.world_normal = vec3<f32>(0.0, 0.0, 1.0);
    return out;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let screen = (in.position.xy - view.viewport.xy) / view.viewport.zw;
    let from_middle = screen - vec2<f32>(0.5);
    let aspect = view.viewport.z / view.viewport.w;
    let reach = length(from_middle * vec2<f32>(aspect, 1.0));
    let ratio = clamp(strength.x, 0.0, 1.0);
    // Whole by a third of the ratio's range; its samples reach on with it.
    let mask = smoothstep(strength.y, strength.z, reach) * min(1.0, ratio * 3.0);
    var sum = vec3<f32>(0.0);
    for (var i = 0; i < SAMPLES; i++) {
        let f = f32(i) / f32(SAMPLES - 1);
        let at = clamp(screen - from_middle * strength.w * ratio * f, vec2<f32>(0.0), vec2<f32>(1.0));
        sum += textureSampleLevel(view_transmission_texture, view_transmission_sampler, at, 0.0).rgb;
    }
    return vec4<f32>(sum / f32(SAMPLES), mask);
}
