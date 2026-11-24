#define_import_path kjk::loose_legs

// `loose_skin::LEG_CAPSULES`: the thighs, the shins and the feet.
const LEG_CAPSULES: u32 = 6u;
// `loose_skin::CAPSULES`: the legs first, then the pelvis, the belly, the
// chest, the head and the arms.
const BODY_CAPSULES: u32 = 14u;

// One body capsule in the world: its ends, its radius in `a.w` and its
// node's scale in `b.w` (`loose_skin::LegCapsule`).
struct LegCapsule {
    a: vec4<f32>,
    b: vec4<f32>,
}

// `loose_skin::CHAINS`: the most chains of a garment that fold and
// flutter.
const CHAINS: u32 = 16u;

// One chain of a garment (`loose_skin::Ripple`): its way down in the world
// and how far its hem flutters in `a`, the flutter's phase and how much its
// neighbours squeeze it in `b`, and the middle of its points in `c`.
struct Ripple {
    a: vec4<f32>,
    b: vec4<f32>,
    c: vec4<f32>,
}

// A wearer's body capsules this frame and the frame before, then its
// chains this frame and the frame before.
struct Legs {
    now: array<LegCapsule, BODY_CAPSULES>,
    was: array<LegCapsule, BODY_CAPSULES>,
    ripples: array<Ripple, CHAINS>,
    ripples_was: array<Ripple, CHAINS>,
}

const PI: f32 = 3.14159265;
// How far out the cloth between two chains folds when they touch (metres).
// Two chains at their rest distance or farther leave it flat.
const FOLD_DEPTH: f32 = 0.06;
// The deepest a fold gets, as a share of the gap between its two chains.
// A fold 6 cm deep between chains squeezed to 5 cm apart turned the cloth
// to face away from every light, and it showed as black patches. A quarter
// keeps a fold's slopes within about 38 degrees of the flat cloth.
const FOLD_SHARE: f32 = 0.25;
// How far down from the waist a fold grows to its full depth (metres).
const FOLD_GROWS: f32 = 0.3;
// The length of the longer of the hem's two flutter waves (metres). The
// other is 1.7 times shorter and runs at its own pace, so the flutter
// never repeats evenly.
const FLUTTER_LENGTH: f32 = 0.3;
// How far down from the waist the flutter grows to its full height
// (metres). It grows with the square of the distance, so it stays at the
// hem.
const HEM: f32 = 0.9;

// The cloth's move along its normal at a vertex, then its slope down the
// cloth, then its slope from its first chain toward its second. `push.z`
// is how far down the cloth the vertex hangs (metres). `push.w` packs its
// two chains in its low 8 bits, and how far across from the first to the
// second it lies, from 0 to 1 in 255ths, above them. The cloth between two
// chains bulges out when they close in, most in the middle of the gap, as
// cloth buckles under compression. The hem flutters as the air rushes past
// it.
fn cloth_at(push: vec4<f32>, ripples: array<Ripple, CHAINS>) -> vec3<f32> {
    let ids = u32(push.w + 0.5);
    let first = ripples[min(ids & 15u, CHAINS - 1u)];
    let second = ripples[min((ids >> 4u) & 15u, CHAINS - 1u)];
    let across = f32(ids >> 8u) / 255.0;
    let down = push.z;
    let squeeze = 0.5 * (first.b.y + second.b.y);
    let gap = max(distance(first.c.xyz, second.c.xyz), 0.05);
    let fold_height = min(FOLD_DEPTH * squeeze * min(down / FOLD_GROWS, 1.0), FOLD_SHARE * gap);
    let fold = fold_height * sin(PI * across);
    let fold_slope = fold_height * PI * cos(PI * across) / gap;
    let hem = clamp(down / HEM, 0.0, 1.0);
    let height = first.a.w * hem * hem;
    let k = 6.2831853 / FLUTTER_LENGTH;
    let phase = first.b.x;
    let long = k * down - phase;
    let short = 1.7 * k * down - 1.3 * phase + 9.0 * across;
    let flutter = height * (0.6 * sin(long) + 0.4 * sin(short));
    let flutter_slope = height * k * (0.6 * cos(long) + 0.4 * 1.7 * cos(short));
    return vec3<f32>(fold + flutter, flutter_slope, fold_slope);
}

// How much darker the valley between two folds is than their crests, at
// the deepest fold. The cloth is held at its chains, so the valleys lie
// along the chains and the crests between them.
const FOLD_SHADE: f32 = 0.35;

// How much light the cloth at a vertex gets for its folds, from 1 on a
// crest or on flat cloth down to 1 less `FOLD_SHADE` in the deepest valley.
fn fold_shade(push: vec4<f32>, ripples: array<Ripple, CHAINS>) -> f32 {
    if push.x <= 0.0 {
        return 1.0;
    }
    let ids = u32(push.w + 0.5);
    let first = ripples[min(ids & 15u, CHAINS - 1u)];
    let second = ripples[min((ids >> 4u) & 15u, CHAINS - 1u)];
    let across = f32(ids >> 8u) / 255.0;
    let squeeze = 0.5 * (first.b.y + second.b.y);
    let grown = min(push.z / FOLD_GROWS, 1.0);
    return 1.0 - FOLD_SHADE * squeeze * grown * (1.0 - sin(PI * across));
}

// The vertex `at` with the normal `normal`, moved along the normal by the
// fold and the flutter of its cloth.
fn rippled(at: vec3<f32>, normal: vec3<f32>, push: vec4<f32>, body: array<LegCapsule, BODY_CAPSULES>, ripples: array<Ripple, CHAINS>) -> vec3<f32> {
    if push.x <= 0.0 {
        return at;
    }
    return at + away_from_body(at, normal, body) * cloth_at(push, ripples).x;
}

// How sharply the ripple's way turns from the cloth's back to its face as
// the cloth turns to face away from the pelvis.
const FACING: f32 = 4.0;

// `normal` turned to face away from the pelvis, and shortened where the
// cloth is edge-on to it. A robe is a closed shell: its inside is a second
// layer whose normals point the other way. Moved along its own normal, each
// layer rippled the other way, the layers crossed in a fast fall, and the
// inside showed through the outside. The two layers at one place get the
// same way here, and the way fades out instead of turning over.
fn away_from_body(at: vec3<f32>, normal: vec3<f32>, body: array<LegCapsule, BODY_CAPSULES>) -> vec3<f32> {
    let pelvis = body[LEG_CAPSULES];
    let off = at - 0.5 * (pelvis.a.xyz + pelvis.b.xyz);
    if length(off) < 1e-4 {
        return vec3<f32>(0.0);
    }
    return normal * clamp(dot(normal, normalize(off)) * FACING, -1.0, 1.0);
}

// The normal `normal` tilted by the slopes of the fold and the flutter, so
// that the light shows them.
fn rippled_normal(normal: vec3<f32>, push: vec4<f32>, ripples: array<Ripple, CHAINS>) -> vec3<f32> {
    if push.x <= 0.0 {
        return normal;
    }
    let ids = u32(push.w + 0.5);
    let first = ripples[min(ids & 15u, CHAINS - 1u)];
    let second = ripples[min((ids >> 4u) & 15u, CHAINS - 1u)];
    let cloth = cloth_at(push, ripples);
    var tilted = normal;
    let down = first.a.xyz - normal * dot(first.a.xyz, normal);
    if length(down) > 1e-4 {
        tilted -= normalize(down) * cloth.y;
    }
    let toward = second.c.xyz - first.c.xyz;
    let across = toward - normal * dot(toward, normal);
    if length(across) > 1e-4 {
        tilted -= normalize(across) * cloth.z;
    }
    return normalize(tilted);
}

// The point `at` pushed out of each of `capsules`. A `push.x` of 0 pushes
// nothing. Otherwise `push.x` less 1 is a mask of the capsules after the
// legs that the point rests inside at rest, such as the chest for the top
// of a robe, and the point is never pushed out of those. `push.y` is how
// far inside a leg capsule the point rests at rest, in the node's space,
// and the point keeps that much less than each leg's radius.
fn out_of_legs(at: vec3<f32>, capsules: array<LegCapsule, BODY_CAPSULES>, push: vec4<f32>) -> vec3<f32> {
    var p = at;
    if push.x <= 0.0 {
        return p;
    }
    let inside = u32(push.x + 0.5) - 1u;
    for (var i = 0u; i < BODY_CAPSULES; i += 1u) {
        var rests = 0.0;
        if i < LEG_CAPSULES {
            rests = push.y;
        } else if ((inside >> (i - LEG_CAPSULES)) & 1u) == 1u {
            continue;
        }
        let capsule = capsules[i];
        let radius = capsule.a.w - rests * capsule.b.w;
        let a = capsule.a.xyz;
        let ab = capsule.b.xyz - a;
        let t = clamp(dot(p - a, ab) / max(dot(ab, ab), 1e-8), 0.0, 1.0);
        let out = p - (a + ab * t);
        let distance = length(out);
        if distance < radius && distance >= 1e-5 {
            p += out / distance * (radius - distance);
        }
    }
    return p;
}
