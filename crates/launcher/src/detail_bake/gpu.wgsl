enable wgpu_ray_query;

// The bake from the fitted lights on the graphics card (`gpu.rs`). Each
// invocation works on one new texel of a lightmap, or on the middle of one
// room triangle. The passes are told apart by `params.mode`.

// A room triangle that blocks rays. `normal.w` is its lightmap's number,
// or -1 when it has none. `uv01` holds the lightmap coordinates of its
// first two corners and `uv2.xy` those of the third.
struct Tri {
    normal: vec4<f32>,
    albedo: vec4<f32>,
    glow: vec4<f32>,
    uv01: vec4<f32>,
    uv2: vec4<f32>,
}

// A place that gathers light. `at.w` holds the number of its cell of the
// light grid. `normal.w` is 1 when `shading` holds the mesh's own normal
// there, which is what the lights' facing reads.
struct Point {
    at: vec4<f32>,
    normal: vec4<f32>,
    shading: vec4<f32>,
}

// A fitted light. `at.w` is its radius, `colour.w` half its length along
// `way` (zero for a light at one place) and `way.w` its cone.
struct Light {
    at: vec4<f32>,
    colour: vec4<f32>,
    way: vec4<f32>,
    aim: vec4<f32>,
}

struct Params {
    first: u32,
    count: u32,
    mode: u32,
    samples: u32,
    rays: u32,
    seed: u32,
    short: f32,
    lift: f32,
}

@group(0) @binding(0) var scene: acceleration_structure;
@group(0) @binding(1) var<storage, read> tris: array<Tri>;
@group(0) @binding(2) var<storage, read> points: array<Point>;
@group(0) @binding(3) var<storage, read> lights: array<Light>;
// Each cell of the light grid: where its list starts in `cell_lights` and
// how long it is.
@group(0) @binding(4) var<storage, read> cells: array<vec2<u32>>;
@group(0) @binding(5) var<storage, read> cell_lights: array<u32>;
@group(0) @binding(6) var<uniform> params: Params;
// The fixed directions of the CPU bake's bounce rays and wall rays, around +Z.
@group(0) @binding(7) var<storage, read> dirs: array<vec4<f32>>;
@group(0) @binding(8) var<storage, read> wall_dirs: array<vec4<f32>>;
// Each lightmap: where its texels start in a field, its width, its height,
// and 1 when it clamps.
@group(0) @binding(9) var<storage, read> maps: array<vec4<u32>>;
// The light on each new texel that a bounce gathers from, for `a` and for
// `b`. A texel with no value has `w` 0.
@group(0) @binding(10) var<storage, read> field_a: array<vec4<f32>>;
@group(0) @binding(11) var<storage, read> field_b: array<vec4<f32>>;
// The direct light at the middle of each triangle, for a ray that meets a
// surface whose texel has no value.
@group(0) @binding(12) var<storage, read> tri_light: array<vec4<f32>>;
@group(0) @binding(13) var<storage, read_write> results: array<vec4<f32>>;
// The places on each light's lamp that its shadow rays aim at, `params.samples`
// for each light. `w` is how far short of the place a ray stops.
@group(0) @binding(14) var<storage, read> lamp_points: array<vec4<f32>>;
// The number in `tris` of each triangle of the two instances: the triangles
// that block the lights, then the lamps' own glass. An instance's custom
// data is where its triangles start here.
@group(0) @binding(15) var<storage, read> ids: array<u32>;

// The instance masks: the triangles that cast shadows, and the lamps' glass,
// which casts none on the lights.
const BLOCKERS: u32 = 1u;
const EVERYTHING: u32 = 0xffu;

const MODE_DIRECT_OLD: u32 = 0u;
const MODE_DIRECT_SOFT: u32 = 1u;
const MODE_BOUNCE_OLD: u32 = 2u;
const MODE_BOUNCE_NEW: u32 = 3u;

const REACH: f32 = 20.0;
const FAINTEST: f32 = 1e-4;
const BACK_SHARE: f32 = 0.4;
const TAU: f32 = 6.2831853;
// The share of a light a surface facing away from it still takes
// (`COMBAT_WRAP` in `odyssey.wgsl`).
const WRAP: f32 = 0.3;
// Within `NEAR_LAMP_FROM` of a light nothing shadows it, and beyond
// `NEAR_LAMP_TO` everything does (metres).
const NEAR_LAMP_FROM: f32 = 0.5;
const NEAR_LAMP_TO: f32 = 1.0;

fn falloff(span: f32, radius: f32) -> f32 {
    if span >= radius {
        return 0.0;
    }
    let edge = max(1.0 - pow(span / radius, 4.0), 0.0);
    let core = span / (0.45 * radius);
    return edge * edge / (1.0 + core * core);
}

// The nearest place of a light to `at`: on a strip, the nearest of its line.
fn nearest(light: Light, at: vec3<f32>) -> vec3<f32> {
    let away = light.at.xyz - at;
    let half = light.colour.w;
    return light.at.xyz + light.way.xyz * clamp(-dot(away, light.way.xyz), -half, half);
}

// What a light gives a surface at `at` (`Fitted::gives` in `detail_bake.rs`).
fn gives(light: Light, at: vec3<f32>, normal: vec3<f32>, has_normal: bool) -> vec3<f32> {
    let to = nearest(light, at) - at;
    let span = length(to);
    let radius = light.at.w;
    if span >= radius {
        return vec3<f32>(0.0);
    }
    let toward = to / max(span, 1e-4);
    var facing = 1.0;
    if has_normal {
        facing = WRAP + (1.0 - WRAP) * max(dot(normal, toward), 0.0);
    }
    var cone = 1.0;
    let narrow = light.way.w;
    if narrow > 0.0 {
        cone = 0.33 + 0.67 * smoothstep(narrow - 0.5, narrow + 0.1, -dot(light.aim.xyz, toward));
    }
    return light.colour.xyz * (falloff(span, radius) * facing * cone);
}

fn hit_any(origin: vec3<f32>, toward: vec3<f32>, reach: f32, mask: u32) -> bool {
    if reach <= 0.0 {
        return false;
    }
    var query: ray_query;
    rayQueryInitialize(&query, scene, RayDesc(RAY_FLAG_TERMINATE_ON_FIRST_HIT, mask, 0.0, reach, origin, toward));
    while rayQueryProceed(&query) {}
    let hit = rayQueryGetCommittedIntersection(&query);
    return hit.kind != RAY_QUERY_INTERSECTION_NONE;
}

// Whether a ray from `at` reaches `goal`. It stops `short` metres short, so
// the light's own fitting casts no shadow on it.
fn reaches(at: vec3<f32>, goal: vec3<f32>, short: f32, mask: u32) -> bool {
    let way = goal - at;
    let far = length(way);
    if far <= short + params.lift {
        return true;
    }
    return !hit_any(at, way / far, far - short, mask);
}

// The number in `tris` of the triangle a ray met.
fn tri_of(hit: RayIntersection) -> u32 {
    return ids[hit.instance_custom_data + hit.primitive_index];
}

// Two directions across `normal` (`across` in `detail_bake.rs`).
fn side_of(normal: vec3<f32>) -> vec3<f32> {
    var axis = vec3<f32>(0.0, 0.0, 1.0);
    if abs(normal.z) >= 0.9 {
        axis = vec3<f32>(1.0, 0.0, 0.0);
    }
    return normalize(cross(normal, axis));
}

// The share of light `light`'s lamp that `at` sees: the share of the
// places on its lamp (`lamp_points`) that rays from `at` reach.
fn soft(light: u32, at: vec3<f32>) -> f32 {
    let count = params.samples;
    var seen = 0u;
    for (var k = 0u; k < count; k++) {
        let goal = lamp_points[light * count + k];
        if reaches(at, goal.xyz, goal.w, BLOCKERS) {
            seen += 1u;
        }
    }
    return f32(seen) / f32(count);
}

// How much of a light reaches `at` whatever stands between, by how near its
// nearest place is. A lamp's fitting is not modelled by its light, which
// stands a quarter metre off the lamp, so the panel around a lamp keeps its
// pool of light as the live look shows it.
fn near_lamp(light: Light, at: vec3<f32>) -> f32 {
    return 1.0 - smoothstep(NEAR_LAMP_FROM, NEAR_LAMP_TO, length(nearest(light, at) - at));
}

fn hash(value: u32) -> u32 {
    var x = value * 747796405u + 2891336453u;
    x = ((x >> ((x >> 28u) + 4u)) ^ x) * 277803737u;
    return (x >> 22u) ^ x;
}

fn random(seed: ptr<function, u32>) -> f32 {
    *seed = hash(*seed);
    return f32(*seed >> 8u) / 16777216.0;
}

// The share of the CPU bake's targets a ray from `at` reaches: the nearest
// place, and on a strip its two ends and the middle of each half.
fn hard(light: Light, at: vec3<f32>) -> f32 {
    var seen = select(0u, 1u, reaches(at, nearest(light, at), params.short, EVERYTHING));
    let half = light.colour.w;
    if half <= 0.0 {
        return f32(seen);
    }
    var places = array<f32, 4>(-1.0, -0.5, 0.5, 1.0);
    for (var k = 0u; k < 4u; k++) {
        if reaches(at, light.at.xyz + light.way.xyz * (places[k] * half), params.short, EVERYTHING) {
            seen += 1u;
        }
    }
    return f32(seen) / 5.0;
}

// Whether a texel is inside a wall: too many of its rays meet the back of a
// surface.
fn counts(at: vec3<f32>, normal: vec3<f32>) -> bool {
    let side = side_of(normal);
    let up = cross(normal, side);
    let total = arrayLength(&wall_dirs);
    var backs = 0u;
    for (var k = 0u; k < total; k++) {
        let d = wall_dirs[k].xyz;
        let direction = side * d.x + up * d.y + normal * d.z;
        var query: ray_query;
        rayQueryInitialize(&query, scene, RayDesc(RAY_FLAG_NONE, 0xffu, 0.0, REACH, at, direction));
        while rayQueryProceed(&query) {}
        let hit = rayQueryGetCommittedIntersection(&query);
        if hit.kind != RAY_QUERY_INTERSECTION_NONE && dot(tris[tri_of(hit)].normal.xyz, direction) >= 0.0 {
            backs += 1u;
        }
    }
    return f32(backs) < BACK_SHARE * f32(total);
}

// The value of a field where a ray met triangle `tri` at `bary`, from the
// nearest texel (`field_at` in `detail_bake.rs`). `w` is 0 where there is
// none.
fn field_index(tri: Tri, bary: vec2<f32>) -> i32 {
    let map = i32(tri.normal.w);
    if map < 0 {
        return -1;
    }
    let uv = tri.uv01.xy * (1.0 - bary.x - bary.y) + tri.uv01.zw * bary.x + tri.uv2.xy * bary.y;
    let facts = maps[map];
    let w = i32(facts.y);
    let h = i32(facts.z);
    var x = i32(floor(uv.x * f32(w)));
    var y = i32(floor(uv.y * f32(h)));
    if facts.w == 1u {
        x = clamp(x, 0, w - 1);
        y = clamp(y, 0, h - 1);
    } else {
        x = ((x % w) + w) % w;
        y = ((y % h) + h) % h;
    }
    return i32(facts.x) + y * w + x;
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= params.count {
        return;
    }
    let index = params.first + id.x;
    let point = points[index];
    let at = point.at.xyz;
    let normal = point.normal.xyz;
    var seed = hash(index * 9781u + params.seed * 6271u + 1u);

    if params.mode == MODE_DIRECT_OLD || params.mode == MODE_DIRECT_SOFT {
        let cell = cells[bitcast<u32>(point.at.w)];
        let has_shading = point.normal.w > 0.5;
        var lit = vec3<f32>(0.0);
        var open = vec3<f32>(0.0);
        for (var k = 0u; k < cell.y; k++) {
            let number = cell_lights[cell.x + k];
            let light = lights[number];
            let given = gives(light, at, point.shading.xyz, has_shading);
            if max(given.x, max(given.y, given.z)) < FAINTEST {
                continue;
            }
            open += given;
            if params.mode == MODE_DIRECT_OLD {
                lit += given * hard(light, at);
            } else {
                lit += given * max(soft(number, at), near_lamp(light, at));
            }
        }
        results[2u * index] = vec4<f32>(lit, select(0.0, 1.0, counts(at, normal)));
        results[2u * index + 1u] = vec4<f32>(open, 0.0);
        return;
    }

    // A bounce: the light the surfaces the rays meet send back.
    let side = side_of(normal);
    let up = cross(normal, side);
    let total = select(params.rays, arrayLength(&dirs), params.mode == MODE_BOUNCE_OLD);
    // One random shift of the whole grid of directions for each texel, so
    // neighbouring texels do not share the same pattern.
    let shift = vec2<f32>(random(&seed), random(&seed));
    let across = u32(round(sqrt(f32(total))));
    var a = vec3<f32>(0.0);
    var b = vec3<f32>(0.0);
    var glow = vec3<f32>(0.0);
    for (var k = 0u; k < total; k++) {
        var d: vec3<f32>;
        if params.mode == MODE_BOUNCE_OLD {
            d = dirs[k].xyz;
        } else {
            let u1 = fract((f32(k / across) + 0.5) / f32(across) + shift.x);
            let u2 = fract((f32(k % across) + 0.5) / f32(across) + shift.y);
            let r = sqrt(u1);
            let phi = TAU * u2;
            d = vec3<f32>(r * cos(phi), r * sin(phi), sqrt(1.0 - u1));
        }
        let direction = side * d.x + up * d.y + normal * d.z;
        var query: ray_query;
        rayQueryInitialize(&query, scene, RayDesc(RAY_FLAG_NONE, 0xffu, 0.0, REACH, at, direction));
        while rayQueryProceed(&query) {}
        let hit = rayQueryGetCommittedIntersection(&query);
        if hit.kind == RAY_QUERY_INTERSECTION_NONE {
            continue;
        }
        let number = tri_of(hit);
        let tri = tris[number];
        if dot(tri.normal.xyz, direction) >= 0.0 {
            continue;
        }
        var from_a = tri_light[number].xyz;
        var from_b = from_a;
        let at_field = field_index(tri, hit.barycentrics);
        if at_field >= 0 {
            let value_a = field_a[at_field];
            if value_a.w > 0.0 {
                from_a = value_a.xyz;
            }
            let value_b = field_b[at_field];
            if value_b.w > 0.0 {
                from_b = value_b.xyz;
            }
        }
        a += from_a * tri.albedo.xyz;
        b += from_b * tri.albedo.xyz;
        glow += tri.glow.xyz;
    }
    let n = f32(max(total, 1u));
    results[3u * index] = vec4<f32>(a / n, 0.0);
    results[3u * index + 1u] = vec4<f32>(b / n, 0.0);
    results[3u * index + 2u] = vec4<f32>(glow / n, 0.0);
}
