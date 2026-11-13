// A distortion (`heat_haze`), as the executable draws its distortion-class
// models: where the particle's art passes the alpha test (GREATER 0) the
// frame is shown displaced by the executable's ring texture
// (`ring_distort.wgsl`).

#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::view
#import kjk::screen_copy::on_frame
#import kjk::ring_distort::behind_rings

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var art: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var art_sampler: sampler;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
#ifdef VERTEX_UVS_A
    let uv = in.uv;
#else
    let uv = vec2<f32>(0.5);
#endif
#ifdef VERTEX_COLORS
    let fade = in.color.a;
#else
    let fade = 1.0;
#endif
    if textureSampleBias(art, art_sampler, uv, view.mip_bias).a * fade <= 0.0 {
        discard;
    }
    return vec4<f32>(behind_rings(on_frame(in.position.xy)).rgb, 1.0);
}
