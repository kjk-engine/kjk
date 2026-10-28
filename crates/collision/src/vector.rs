//! Three-component vector arithmetic over `[f32; 3]`, for the crates that
//! build and trace collision (and those that share its types), so each
//! keeps the same rounding: only `+ - * /` and `sqrt` (`docs/SIM.md`).

pub fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// `v` at unit length, or `None` when it is too short to have a direction.
pub fn normalize(v: [f32; 3]) -> Option<[f32; 3]> {
    let length = dot(v, v).sqrt();
    (length > 1e-6).then(|| [v[0] / length, v[1] / length, v[2] / length])
}
