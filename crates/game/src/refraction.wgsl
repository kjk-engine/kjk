// JA's distortion (`effects/refraction` on an `RF_DISTORTION` entity): the
// model shows a square of the screen copied from behind it, its texture
// coordinates already turned into screen coordinates on the CPU, with the
// entity's colour and alpha. The blend (alpha, as `RF_FORCE_ENT_ALPHA` has
// it) is set in the pipeline. The coordinates are the frame's (0 to 1
// across it), wherever the main pass draws it (`screen_copy.wgsl`).

#import bevy_pbr::forward_io::VertexOutput
#import kjk::screen_copy::frame_at

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
#ifdef VERTEX_UVS_A
    let uv = in.uv;
#else
    let uv = vec2<f32>(0.5);
#endif
    let screen = frame_at(uv);
#ifdef VERTEX_COLORS
    let colour = in.color;
#else
    let colour = vec4<f32>(1.0);
#endif
    return vec4<f32>(screen.rgb * colour.rgb, colour.a);
}
