//! Ghoul2's 3×4 matrices (rows of rotation and a translation column), with
//! its own arithmetic order, so a product rounds as Ghoul2's does.

use super::turn::{Local, normalize};
use crate::math::{Vec3, angle_vectors};

pub type Mat34 = [[f32; 4]; 3];

/// Ghoul2's root matrix: it turns the skeleton's −Y (where the model faces)
/// to +X.
pub const ROOT: Mat34 = [
    [0.0, -1.0, 0.0, 0.0],
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
];

/// `n · m`, each entry summed left to right, the translation added last.
pub fn multiply(n: &Mat34, m: &Mat34) -> Mat34 {
    std::array::from_fn(|r| {
        let row = n[r];
        let cell = |c: usize| row[0] * m[0][c] + row[1] * m[1][c] + row[2] * m[2][c];
        [cell(0), cell(1), cell(2), cell(3) + row[3]]
    })
}

/// A compressed bone: a quaternion (w, x, y, z, each `v / 16383 − 2`),
/// made unit, and a translation (`v / 64 − 512`).
pub fn decode(entry: &[u16; 7]) -> Local {
    let q = |i: usize| f32::from(entry[i]) / 16383.0 - 2.0;
    let t = |i: usize| f32::from(entry[i]) / 64.0 - 512.0;
    (normalize([q(0), q(1), q(2), q(3)]), [t(4), t(5), t(6)])
}

/// A bone's local transform as a matrix.
pub fn matrix((q, t): &Local) -> Mat34 {
    let [w, x, y, z] = *q;
    let (x2, y2, z2) = (2.0 * x, 2.0 * y, 2.0 * z);
    let (wx, wy, wz) = (x2 * w, y2 * w, z2 * w);
    let (xx, xy, xz) = (x2 * x, y2 * x, z2 * x);
    let (yy, yz, zz) = (y2 * y, z2 * y, z2 * z);
    [
        [1.0 - (yy + zz), xy - wz, xz + wy, t[0]],
        [xy + wz, 1.0 - (xx + zz), yz - wx, t[1]],
        [xz - wy, yz + wx, 1.0 - (xx + yy), t[2]],
    ]
}

/// Quake angles (pitch, yaw, roll; degrees) as a matrix whose columns are
/// forward, left and up (`AnglesToAxis`), at the origin.
pub fn from_angles(angles: [f32; 3]) -> Mat34 {
    let (forward, right, up) = angle_vectors(angles);
    let left = right.map(|v| 0.0 - v);
    std::array::from_fn(|r| [forward[r], left[r], up[r], 0.0])
}

/// A point through a matrix: each row's dot product, then its translation.
pub fn transform(m: &Mat34, p: Vec3) -> Vec3 {
    std::array::from_fn(|r| m[r][0] * p[0] + m[r][1] * p[1] + m[r][2] * p[2] + m[r][3])
}
