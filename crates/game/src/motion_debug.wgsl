// The motion vectors the resolve reads (`temporal`, `KJK_MOTION_DEBUG=1`):
// a quad over the whole screen draws each pixel's motion since the frame
// before, grey where still, red for rightward, green for downward, 16
// pixels a frame at the extremes (so a pixel's motion in pixels is
// (colour - 0.5) * 32). Blue is 1 where the motion is under a tenth of a
// pixel. Magenta: the camera has no motion prepass.

#import bevy_pbr::forward_io::{Vertex, VertexOutput}
#import bevy_pbr::mesh_view_bindings::view
#ifdef MOTION_VECTOR_PREPASS
#import bevy_pbr::prepass_utils::prepass_motion_vector
#endif

/// Pixels a frame at the colour's extremes.
const REACH: f32 = 16.0;

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    out.position = vec4<f32>(vertex.position.xy, 1.0, 1.0);
    out.world_position = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    out.world_normal = vec3<f32>(0.0, 0.0, 1.0);
    return out;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
#ifdef MOTION_VECTOR_PREPASS
    // Bevy keeps them in UV units of the frame drawn (current minus
    // previous); in that frame's pixels (smaller than the window's under
    // DLSS, `main_pass_viewport`):
    let motion = prepass_motion_vector(in.position, 0u) * view.main_pass_viewport.zw;
    let shown = clamp(motion / (2.0 * REACH), vec2<f32>(-0.5), vec2<f32>(0.5)) + vec2<f32>(0.5);
    let still = select(0.0, 1.0, length(motion) < 0.1);
    return vec4<f32>(shown, still, 1.0);
#else
    return vec4<f32>(1.0, 0.0, 1.0, 1.0);
#endif
}
