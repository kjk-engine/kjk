// JA's distortion (`effects/refraction` on an `RF_DISTORTION` entity): the
// model shows a square of the screen copied from behind it, its texture
// coordinates already turned into screen coordinates on the CPU, with the
// entity's colour and alpha. The blend (alpha, as `RF_FORCE_ENT_ALPHA` has
// it) is set in the pipeline.

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::{view_transmission_texture, view_transmission_sampler}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
#ifdef VERTEX_UVS_A
    let uv = in.uv;
#else
    let uv = vec2<f32>(0.5);
#endif
    let screen = textureSampleLevel(view_transmission_texture, view_transmission_sampler, uv, 0.0);
#ifdef VERTEX_COLORS
    let colour = in.color;
#else
    let colour = vec4<f32>(1.0);
#endif
    return vec4<f32>(screen.rgb * colour.rgb, colour.a);
}
