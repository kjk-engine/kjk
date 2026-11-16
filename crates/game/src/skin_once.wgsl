// Skins each character's meshes once a frame (`skin_once.rs`). A job is one
// mesh under one skin. Each of its vertices is read from Bevy's vertex
// buffer, placed in the world by its joints, and written to `skinned`. The
// shadow tiles' pass then draws the mesh from `skinned`, once for each of
// its tiles, without skinning it again.

struct Job {
    // Its first vertex in `source`, in 32-bit words.
    source: u32,
    // A vertex's size in `source`, in words.
    stride: u32,
    // How many vertices it has.
    count: u32,
    // Its first vertex in `skinned`.
    placed: u32,
    // Its first matrix in `joints`.
    first: u32,
    // Where a vertex holds its position, its four joints and its four
    // weights, in words from the vertex's start.
    position: u32,
    indices: u32,
    weights: u32,
    // A shadow mesh's vertex holds its position as its second, third and
    // fourth joints see it (`shadow_tiles::shadow_mesh`). `NONE` for a mesh
    // with a skin of its own.
    place_1: u32,
    place_2: u32,
    place_3: u32,
    unused: u32,
}

const NONE: u32 = 0xffffffffu;

// How many vertices one workgroup places.
const GROUP: u32 = 64u;

@group(0) @binding(0) var<storage, read> joints: array<mat4x4<f32>>;
@group(0) @binding(1) var<storage, read> jobs: array<Job>;
@group(0) @binding(2) var<storage, read_write> skinned: array<vec4<f32>>;

// One of Bevy's vertex buffers, as words.
@group(1) @binding(0) var<storage, read> source: array<u32>;
// Each workgroup's job in its low 16 bits, and in its high 16 bits which
// of the job's workgroups it is.
@group(1) @binding(1) var<storage, read> groups: array<u32>;

fn place(at: u32) -> vec4<f32> {
    return vec4<f32>(
        bitcast<f32>(source[at]),
        bitcast<f32>(source[at + 1u]),
        bitcast<f32>(source[at + 2u]),
        1.0,
    );
}

@compute @workgroup_size(64)
fn skin(
    @builtin(workgroup_id) group: vec3<u32>,
    @builtin(local_invocation_index) local: u32,
) {
    let entry = groups[group.x];
    let job = jobs[entry & 0xffffu];
    let vertex = (entry >> 16u) * GROUP + local;
    if vertex >= job.count {
        return;
    }
    let at = job.source + vertex * job.stride;
    // Four 16-bit joints in two words.
    let low = source[at + job.indices];
    let high = source[at + job.indices + 1u];
    let s = job.first;
    let a = joints[s + (low & 0xffffu)];
    let b = joints[s + (low >> 16u)];
    let c = joints[s + (high & 0xffffu)];
    let d = joints[s + (high >> 16u)];
    let weights = vec4<f32>(
        bitcast<f32>(source[at + job.weights]),
        bitcast<f32>(source[at + job.weights + 1u]),
        bitcast<f32>(source[at + job.weights + 2u]),
        bitcast<f32>(source[at + job.weights + 3u]),
    );
    let position = place(at + job.position);
    var world: vec4<f32>;
    if job.place_1 == NONE {
        // The sum the main pass makes for a skinned mesh.
        let skin = weights.x * a + weights.y * b + weights.z * c + weights.w * d;
        world = skin * position;
    } else {
        // A shadow mesh: each node places the vertex as that node sees it,
        // and the results are summed by their weights.
        world = weights.x * (a * position)
            + weights.y * (b * place(at + job.place_1))
            + weights.z * (c * place(at + job.place_2))
            + weights.w * (d * place(at + job.place_3));
    }
    skinned[job.placed + vertex] = world;
}
