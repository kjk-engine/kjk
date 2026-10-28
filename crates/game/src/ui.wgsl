// The game's screens: a texture tinted by the vertex colour, sampled as
// `ui::raster` samples it. Where the texture is enlarged it is read
// Catmull-Rom (sharper than bilinear), where shrunk from its mipmaps; a
// shape (a font, one-colour line art) keeps its edge one screen pixel wide
// however large it is drawn.

#import bevy_pbr::forward_io::VertexOutput

struct UiParams {
    // The texture's size in texels.
    size: vec2<f32>,
    // 1: a shape.
    shape: u32,
    // 1: a 3D view; drawn at its own size, so read texel for texel.
    view: u32,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> params: UiParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var ui_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var ui_sampler: sampler;

fn at(uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(ui_texture, ui_sampler, uv, 0.0);
}

// Catmull-Rom from nine bilinear reads (the middle two texels of each axis
// read together).
fn catmull_rom(uv: vec2<f32>) -> vec4<f32> {
    let size = params.size;
    let position = uv * size;
    let centre = floor(position - 0.5) + 0.5;
    let f = position - centre;
    let w0 = f * (-0.5 + f * (1.0 - 0.5 * f));
    let w1 = 1.0 + f * f * (-2.5 + 1.5 * f);
    let w2 = f * (0.5 + f * (2.0 - 1.5 * f));
    let w3 = f * f * (-0.5 + 0.5 * f);
    let w12 = w1 + w2;
    let p0 = (centre - 1.0) / size;
    let p12 = (centre + w2 / w12) / size;
    let p3 = (centre + 2.0) / size;
    var sum = at(vec2<f32>(p0.x, p0.y)) * w0.x * w0.y;
    sum += at(vec2<f32>(p12.x, p0.y)) * w12.x * w0.y;
    sum += at(vec2<f32>(p3.x, p0.y)) * w3.x * w0.y;
    sum += at(vec2<f32>(p0.x, p12.y)) * w0.x * w12.y;
    sum += at(vec2<f32>(p12.x, p12.y)) * w12.x * w12.y;
    sum += at(vec2<f32>(p3.x, p12.y)) * w3.x * w12.y;
    sum += at(vec2<f32>(p0.x, p3.y)) * w0.x * w3.y;
    sum += at(vec2<f32>(p12.x, p3.y)) * w12.x * w3.y;
    sum += at(vec2<f32>(p3.x, p3.y)) * w3.x * w3.y;
    return clamp(sum, vec4<f32>(0.0), vec4<f32>(1.0));
}

// A shape's alpha, its edge `per_texel` screen pixels wide made one wide.
fn sharpen(alpha: f32, per_texel: f32) -> f32 {
    let width = 0.5 / max(per_texel, 0.001);
    if width >= 0.5 {
        return alpha;
    }
    return smoothstep(0.5 - width, 0.5 + width, alpha);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
#ifdef VERTEX_UVS_A
    let uv = in.uv;
#else
    let uv = vec2<f32>(0.5);
#endif
    let texels = uv * params.size;
    // Texels per screen pixel.
    let footprint = max(max(length(dpdx(texels)), length(dpdy(texels))), 0.0001);
    var texel: vec4<f32>;
    if params.view == 1u {
        // A 3D view: rendered the size it is drawn, read as it is.
        texel = at(uv);
    } else if footprint < 1.0 {
        texel = catmull_rom(uv);
    } else {
        texel = textureSampleLevel(ui_texture, ui_sampler, uv, log2(footprint));
    }
    if params.shape == 1u {
        texel.a = sharpen(texel.a, 1.0 / footprint);
    }
#ifdef VERTEX_COLORS
    let colour = in.color;
#else
    let colour = vec4<f32>(1.0);
#endif
    return texel * colour;
}
