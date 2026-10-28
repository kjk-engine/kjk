// An Odyssey mesh drawn into a shadow map (`shadows`): only an alpha-tested
// surface comes here (one whose texture's alpha is coverage: hair, a grate),
// and it casts where its texture is mostly opaque.

#import bevy_pbr::prepass_io::VertexOutput

@group(#{MATERIAL_BIND_GROUP}) @binding(1) var base_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var base_sampler: sampler;

@fragment
fn fragment(in: VertexOutput) {
#ifdef VERTEX_UVS_A
    if textureSample(base_texture, base_sampler, in.uv).a < 0.5 {
        discard;
    }
#endif
}
