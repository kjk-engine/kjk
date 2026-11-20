#define_import_path kjk::loose_legs

// One leg capsule in the world: its ends, and its radius in `a.w`
// (`loose_skin::Entry`).
struct LegCapsule {
    a: vec4<f32>,
    b: vec4<f32>,
}

// A wearer's six leg capsules this frame and the frame before.
struct Legs {
    now: array<LegCapsule, 6>,
    was: array<LegCapsule, 6>,
}

// `at` pushed out of `capsule` by `push.x` of its depth, `push.y` nearer
// to the axis than the capsule's radius (`loose_skin.wgsl`).
fn out_of(at: vec3<f32>, capsule: LegCapsule, push: vec2<f32>) -> vec3<f32> {
    let radius = capsule.a.w - push.y;
    let a = capsule.a.xyz;
    let ab = capsule.b.xyz - a;
    let t = clamp(dot(at - a, ab) / max(dot(ab, ab), 1e-8), 0.0, 1.0);
    let out = at - (a + ab * t);
    let distance = length(out);
    if distance >= radius || distance < 1e-5 {
        return at;
    }
    return at + out / distance * ((radius - distance) * push.x);
}
