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
    // Not read. It keeps `lift` and `sun` where `gpu.rs` writes them.
    unused: f32,
    lift: f32,
    // The way to the sun, for `MODE_SKY`. `w` is half the sun's width where
    // the area gives one, and 0 for `SUN_WIDTH`.
    sun: vec4<f32>,
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
// The fixed directions of the wall rays, around +Z. Binding 7 is not used.
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

// Every ray query here calls `rayQueryProceed` once, not in a loop. Every
// triangle of the scene is opaque, so the first call walks the whole
// structure on Vulkan and returns false, and no hit is ever left for the
// shader to confirm. On Metal, wgpu 29's naga writes `rayQueryProceed` as a
// flag that stays true after the walk, so a `while rayQueryProceed` loop
// never ends there. The card then stops the command buffer, and wgpu waits
// for it for ever (`ray_structures.rs` in the game has the same note).

// The instance mask of the triangles that cast shadows. The lamps' glass has
// a mask of its own and casts no shadow on the lights.
const BLOCKERS: u32 = 1u;

const MODE_DIRECT_SOFT: u32 = 1u;
const MODE_BOUNCE_NEW: u32 = 3u;
const MODE_SKY: u32 = 4u;

// How far a ray to the sun or the sky goes before it counts as free (metres).
const SKY_REACH: f32 = 400.0;
// Half the width of the sun as the bake sees it (radians). It is wider than
// the real sun, so a shadow's edge is soft over a few texels.
const SUN_WIDTH: f32 = 0.04;

const REACH: f32 = 20.0;
const FAINTEST: f32 = 1e-4;
const BACK_SHARE: f32 = 0.4;
// How far the back of a surface counts for `counts` (metres).
const WALL_NEAR: f32 = 1.0;
const TAU: f32 = 6.2831853;
// Lambert cosine falloff with zero back-face wrap (`WRAP = 0.0`) for crisp
// fixture directionality and relief in linear HDR lightmaps.
const WRAP: f32 = 0.0;
// Within `NEAR_LAMP_FROM` of a light nothing shadows it, and beyond
// `NEAR_LAMP_TO` everything does (metres).
const NEAR_LAMP_FROM: f32 = 0.5;
const NEAR_LAMP_TO: f32 = 1.0;
// A light in a cell's list with this mark is the own light of the
// placeable whose vertex the cell holds (`OWN_LIGHT` in `gpu.rs`). The
// vertex takes it as if it stood no nearer than `OWN_LIGHT_LEAST` (metres),
// so the vertices a few centimetres from the lamp are not blown out. The
// placeable's own triangles block no ray here. A vertex that they hide from
// its own light has that light left out of its cell's list (`own_seen` in
// `gpu.rs`). Half a metre keeps the top of danm13's glowing table close to
// its look on the probes, and its terminal dark. A third of a metre looked
// the same, and a whole metre dimmed the table.
const OWN_LIGHT: u32 = 0x80000000u;
const OWN_LIGHT_LEAST: f32 = 0.5;

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

// What a light gives a surface at `at` (`Fitted::gives` in `detail_bake.rs`),
// as if it stood no nearer than `least`.
fn gives(light: Light, at: vec3<f32>, normal: vec3<f32>, has_normal: bool, least: f32) -> vec3<f32> {
    let to = nearest(light, at) - at;
    let span = max(length(to), least);
    let radius = light.at.w;
    if span >= radius {
        return vec3<f32>(0.0);
    }
    let toward = to / max(length(to), 1e-4);
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
    _ = rayQueryProceed(&query);
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

// Whether a texel is inside a wall: too many of its rays meet the back of a
// surface within `WALL_NEAR`. A surface farther off is no wall the texel is
// in. The rays from the sand of tat_m20aa met the backs of the tents' cloth
// metres above it, and whole triangles of sand were left out and filled
// from their neighbours, in hard triangles of light and shade (22 November
// 2026).
fn counts(at: vec3<f32>, normal: vec3<f32>) -> bool {
    let side = side_of(normal);
    let up = cross(normal, side);
    let total = arrayLength(&wall_dirs);
    var backs = 0u;
    for (var k = 0u; k < total; k++) {
        let d = wall_dirs[k].xyz;
        let direction = side * d.x + up * d.y + normal * d.z;
        var query: ray_query;
        rayQueryInitialize(&query, scene, RayDesc(RAY_FLAG_NONE, 0xffu, 0.0, WALL_NEAR, at, direction));
        _ = rayQueryProceed(&query);
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

    if params.mode == MODE_DIRECT_SOFT {
        let cell = cells[bitcast<u32>(point.at.w)];
        let has_shading = point.normal.w > 0.5;
        var lit = vec3<f32>(0.0);
        var open = vec3<f32>(0.0);
        var dir_sum = vec3<f32>(0.0);
        for (var k = 0u; k < cell.y; k++) {
            let listed = cell_lights[cell.x + k];
            let number = listed & ~OWN_LIGHT;
            let light = lights[number];
            let least = select(0.0, OWN_LIGHT_LEAST, (listed & OWN_LIGHT) != 0u);
            let given = gives(light, at, point.shading.xyz, has_shading, least);
            if max(given.x, max(given.y, given.z)) < FAINTEST {
                continue;
            }
            let vis = max(soft(number, at), near_lamp(light, at));
            let contrib = given * vis;
            let to = nearest(light, at) - at;
            let dist = length(to);
            if dist > 1e-4 {
                let w = 0.2126 * contrib.x + 0.7152 * contrib.y + 0.0722 * contrib.z;
                dir_sum += (to / dist) * w;
            }
            open += given;
            lit += contrib;
        }
        results[4u * index] = vec4<f32>(lit, select(0.0, 1.0, counts(at, normal)));
        results[4u * index + 1u] = vec4<f32>(open, 0.0);
        results[4u * index + 2u] = vec4<f32>(dir_sum, 0.0);
        return;
    }

    if params.mode == MODE_SKY {
        // The sun: the share of `params.samples` rays toward it that meet
        // nothing, times how squarely the surface faces it.
        let toward = params.sun.xyz;
        var face = normal;
        if point.normal.w > 0.5 {
            face = point.shading.xyz;
        }
        let facing = dot(face, toward);
        var sun = 0.0;
        if facing > 0.0 && params.samples > 0u {
            let sun_side = side_of(toward);
            let sun_up = cross(toward, sun_side);
            var seen = 0u;
            // A sun with a width of its own is sharp. Its rays start from
            // a grid of places over the texel's square, so a shadow's edge
            // that crosses the texel shades it by the share it covers. A
            // place that a surface cuts off from the texel's middle lies
            // behind a wall, and its ray starts at the middle.
            let sharp = params.sun.w > 0.0;
            let width = select(SUN_WIDTH, params.sun.w, sharp);
            let across = u32(round(sqrt(f32(params.samples))));
            let texel_side = side_of(normal);
            let texel_up = cross(normal, texel_side);
            for (var k = 0u; k < params.samples; k++) {
                var start = at;
                if sharp && point.shading.w > 0.0 {
                    let u = (f32(k % across) + random(&seed)) / f32(across) - 0.5;
                    let v = (f32(k / across) + random(&seed)) / f32(across) - 0.5;
                    let shift = (texel_side * u + texel_up * v) * point.shading.w;
                    let gap = length(shift);
                    if gap > 1e-5 && !hit_any(at, shift / gap, gap, BLOCKERS) {
                        start = at + shift;
                    }
                }
                let r = width * sqrt(random(&seed));
                let phi = TAU * random(&seed);
                let direction = normalize(toward + (sun_side * cos(phi) + sun_up * sin(phi)) * r);
                if !hit_any(start, direction, SKY_REACH, BLOCKERS) {
                    seen += 1u;
                }
            }
            sun = facing * f32(seen) / f32(params.samples);
        }
        // The sky: the share of `params.rays` rays over the surface that go
        // upward and meet nothing.
        var sky = 0.0;
        var sky_dir = vec3<f32>(0.0);
        if params.rays > 0u {
            let sky_side = side_of(normal);
            let sky_up = cross(normal, sky_side);
            let sky_across = u32(round(sqrt(f32(params.rays))));
            let sky_shift = vec2<f32>(random(&seed), random(&seed));
            var free = 0u;
            for (var k = 0u; k < params.rays; k++) {
                let u1 = fract((f32(k / sky_across) + 0.5) / f32(sky_across) + sky_shift.x);
                let u2 = fract((f32(k % sky_across) + 0.5) / f32(sky_across) + sky_shift.y);
                let r = sqrt(u1);
                let phi = TAU * u2;
                let d = vec3<f32>(r * cos(phi), r * sin(phi), sqrt(1.0 - u1));
                let direction = sky_side * d.x + sky_up * d.y + normal * d.z;
                if direction.z > 0.0 && !hit_any(at, direction, SKY_REACH, BLOCKERS) {
                    free += 1u;
                    sky_dir += direction;
                }
            }
            sky = f32(free) / f32(params.rays);
            sky_dir = sky_dir / f32(params.rays);
        }
        results[4u * index] = vec4<f32>(sun, sky, 0.0, 0.0);
        results[4u * index + 1u] = vec4<f32>(sky_dir, 0.0);
        return;
    }

    // A bounce: the light the surfaces the rays meet send back.
    let side = side_of(normal);
    let up = cross(normal, side);
    let total = params.rays;
    // One random shift of the whole grid of directions for each texel, so
    // neighbouring texels do not share the same pattern.
    let shift = vec2<f32>(random(&seed), random(&seed));
    let across = u32(round(sqrt(f32(total))));
    var a = vec3<f32>(0.0);
    var b = vec3<f32>(0.0);
    var glow = vec3<f32>(0.0);
    var dir_b = vec3<f32>(0.0);
    for (var k = 0u; k < total; k++) {
        let u1 = fract((f32(k / across) + 0.5) / f32(across) + shift.x);
        let u2 = fract((f32(k % across) + 0.5) / f32(across) + shift.y);
        let r = sqrt(u1);
        let phi = TAU * u2;
        let d = vec3<f32>(r * cos(phi), r * sin(phi), sqrt(1.0 - u1));
        let direction = side * d.x + up * d.y + normal * d.z;
        var query: ray_query;
        rayQueryInitialize(&query, scene, RayDesc(RAY_FLAG_NONE, 0xffu, 0.0, REACH, at, direction));
        _ = rayQueryProceed(&query);
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
        // A skylight (`w` is 2) gives its own light, whatever its texels hold.
        if at_field >= 0 && tri_light[number].w < 1.5 {
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
        let contrib_b = from_b * tri.albedo.xyz;
        b += contrib_b;
        glow += tri.glow.xyz;
        let lum_b = 0.2126 * (contrib_b.x + tri.glow.x) + 0.7152 * (contrib_b.y + tri.glow.y) + 0.0722 * (contrib_b.z + tri.glow.z);
        dir_b += direction * lum_b;
    }
    let n = f32(max(total, 1u));
    results[4u * index] = vec4<f32>(a / n, 0.0);
    results[4u * index + 1u] = vec4<f32>(b / n, 0.0);
    results[4u * index + 2u] = vec4<f32>(glow / n, 0.0);
    results[4u * index + 3u] = vec4<f32>(dir_b / n, 0.0);
}
