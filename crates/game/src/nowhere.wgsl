// A motion proxy's vertex stage in the main pass (`motion_proxy`): every
// vertex beyond the screen's right edge, so each triangle is clipped whole
// and nothing is drawn. Its mesh's attributes are not read.

#import bevy_pbr::forward_io::VertexOutput

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    var out: VertexOutput;
    out.position = vec4<f32>(2.0, 0.0, 0.5, 1.0);
    return out;
}
