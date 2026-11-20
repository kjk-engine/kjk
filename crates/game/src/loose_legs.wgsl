#define_import_path kjk::loose_legs

// `loose_skin::CAPSULES`: the thighs, the shins and the feet.
const LEG_CAPSULES: u32 = 6u;

// One leg capsule in the world: its ends, its radius in `a.w` and its
// node's scale in `b.w` (`loose_skin::LegCapsule`).
struct LegCapsule {
    a: vec4<f32>,
    b: vec4<f32>,
}

// A wearer's leg capsules this frame and the frame before.
struct Legs {
    now: array<LegCapsule, LEG_CAPSULES>,
    was: array<LegCapsule, LEG_CAPSULES>,
}

// The point `at` pushed out of each of `capsules`. `push.x` is the share of
// the way out that it is pushed. `push.y` is how far inside the capsule the
// point rests at rest, in the node's space, and the point keeps that much
// less than the capsule's radius.
fn out_of_legs(at: vec3<f32>, capsules: array<LegCapsule, LEG_CAPSULES>, push: vec2<f32>) -> vec3<f32> {
    var p = at;
    if push.x <= 0.0 {
        return p;
    }
    for (var i = 0u; i < LEG_CAPSULES; i += 1u) {
        let capsule = capsules[i];
        let radius = capsule.a.w - push.y * capsule.b.w;
        let a = capsule.a.xyz;
        let ab = capsule.b.xyz - a;
        let t = clamp(dot(p - a, ab) / max(dot(ab, ab), 1e-8), 0.0, 1.0);
        let out = p - (a + ab * t);
        let distance = length(out);
        if distance < radius && distance >= 1e-5 {
            p += out / distance * ((radius - distance) * push.x);
        }
    }
    return p;
}
