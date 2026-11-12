// A caster's mesh drawn into its shadow tiles (`shadow_tiles.rs`), one
// instance a tile. Only its depth is written. A skinned mesh is skinned
// from its joint matrices, as the main pass skins it. A shadow mesh
// (`BAKED`) is placed by its model's nodes. A mesh whose texture's alpha is
// its coverage (`CUT`) casts where the alpha is half or more, as the old
// shadow maps' shader had it.

struct Draw {
    // Its tile. A tile has two of `joints`, at twice its number. The first
    // is the light's view and projection, fitted to the caster. The second
    // says where the tile lies in the atlas. Its first column moves the
    // tile's clip space into the atlas's: xy scales it, and zw times w is
    // added. Its second column is the tile's rectangle in the atlas's
    // texels: xy its first corner, zw its far one.
    tile: u32,
    // Its first matrix in `joints`. A mesh with no skin has one, where it
    // is placed. A skinned mesh has one for each joint. A shadow mesh has
    // one for each node that places a vertex.
    first: u32,
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
#ifdef BAKED
    // The vertex as its second, third and fourth nodes place it. `position`
    // is as its first node places it.
    @location(4) place_1: vec3<f32>,
    @location(5) place_2: vec3<f32>,
    @location(6) place_3: vec3<f32>,
#endif
}

@vertex
fn vertex(in: Vertex) -> Out {
    let draw = draws[in.instance];
    let view_proj = joints[2u * draw.tile];
    let fit = joints[2u * draw.tile + 1u];
    let s = draw.first;
#ifdef BAKED
    // Each node places the vertex, and the results are summed by their
    // weights. It is the sum a skin makes, with each inverse bind pose
    // already applied to the vertex.
    let world = in.joint_weights.x * (joints[s + in.joint_indices.x] * vec4<f32>(in.position, 1.0))
        + in.joint_weights.y * (joints[s + in.joint_indices.y] * vec4<f32>(in.place_1, 1.0))
        + in.joint_weights.z * (joints[s + in.joint_indices.z] * vec4<f32>(in.place_2, 1.0))
        + in.joint_weights.w * (joints[s + in.joint_indices.w] * vec4<f32>(in.place_3, 1.0));
#else
#ifdef SKINNED
    let skin = in.joint_weights.x * joints[s + in.joint_indices.x]
        + in.joint_weights.y * joints[s + in.joint_indices.y]
        + in.joint_weights.z * joints[s + in.joint_indices.z]
        + in.joint_weights.w * joints[s + in.joint_indices.w];
    let world = skin * vec4<f32>(in.position, 1.0);
#else
    let world = joints[s] * vec4<f32>(in.position, 1.0);
#endif
#endif
    let clip = view_proj * world;
    var out: Out;
    out.position = vec4<f32>(clip.xy * fit[0].xy + fit[0].zw * clip.w, clip.zw);
    out.bounds = fit[1];
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
