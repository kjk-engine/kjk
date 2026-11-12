// A caster's mesh drawn into its shadow tiles (`shadow_tiles.rs`), one
// instance a tile. Only its depth is written. A skinned mesh is skinned
// from its joint matrices, as the main pass skins it. A mesh whose
// texture's alpha is its coverage (`CUT`) casts where the alpha is half or
// more, as the old shadow maps' shader had it.

struct Draw {
    // Where a mesh with no skin is placed.
    world: mat4x4<f32>,
    // The light's view and projection, fitted to the caster.
    view_proj: mat4x4<f32>,
    // Moves the tile's clip space into the atlas's: xy scales it, and zw
    // times w is added.
    place: vec4<f32>,
    // The tile's rectangle in the atlas's texels: xy its first corner, zw
    // its far one.
    bounds: vec4<f32>,
    // A skinned mesh's first joint matrix in `joints`.
    skin: u32,
}

struct Out {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) bounds: vec4<f32>,
#ifdef CUT
    @location(1) uv: vec2<f32>,
#endif
}

@group(0) @binding(0) var<storage, read> joints: array<mat4x4<f32>>;
@group(0) @binding(1) var<storage, read> draws: array<Draw>;

#ifdef CUT
@group(1) @binding(0) var base_texture: texture_2d<f32>;
@group(1) @binding(1) var base_sampler: sampler;
#endif

struct Vertex {
    @builtin(instance_index) instance: u32,
    @location(0) position: vec3<f32>,
#ifdef SKINNED
    @location(1) joint_indices: vec4<u32>,
    @location(2) joint_weights: vec4<f32>,
#endif
#ifdef CUT
    @location(3) uv: vec2<f32>,
#endif
}

@vertex
fn vertex(in: Vertex) -> Out {
    let draw = draws[in.instance];
#ifdef SKINNED
    let s = draw.skin;
    let world = in.joint_weights.x * joints[s + in.joint_indices.x]
        + in.joint_weights.y * joints[s + in.joint_indices.y]
        + in.joint_weights.z * joints[s + in.joint_indices.z]
        + in.joint_weights.w * joints[s + in.joint_indices.w];
#else
    let world = draw.world;
#endif
    let clip = draw.view_proj * world * vec4<f32>(in.position, 1.0);
    var out: Out;
    out.position = vec4<f32>(clip.xy * draw.place.xy + draw.place.zw * clip.w, clip.zw);
    out.bounds = draw.bounds;
#ifdef CUT
    out.uv = in.uv;
#endif
    return out;
}

// What falls outside the instance's tile is thrown away: it would draw
// over the tile beside it. So is what the texture's alpha cuts away.
@fragment
fn fragment(in: Out) {
    if any(in.position.xy < in.bounds.xy) || any(in.position.xy >= in.bounds.zw) {
        discard;
    }
#ifdef CUT
    if textureSample(base_texture, base_sampler, in.uv).a < 0.5 {
        discard;
    }
#endif
}
