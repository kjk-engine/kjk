//! The sim's vector helpers and JA's short angles, built from `+ - * /`,
//! `sqrt`, comparisons and rounding to whole numbers alone, so every
//! platform computes the same bits (`docs/SIM.md`). The trigonometry is
//! `collision::trig`'s, re-exported: one implementation for the sim and
//! the level it moves in.

pub use collision::trig::{angle_vectors, atan2_deg, sin_cos_deg};

pub type Vec3 = [f32; 3];

pub fn dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub fn scale(a: Vec3, s: f32) -> Vec3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

/// `a + b * s` (`VectorMA`), rounded after the product as two operations.
pub fn mad(a: Vec3, s: f32, b: Vec3) -> Vec3 {
    [a[0] + b[0] * s, a[1] + b[1] * s, a[2] + b[2] * s]
}

pub fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub fn length(a: Vec3) -> f32 {
    dot(a, a).sqrt()
}

/// `a`'s length seen from above (its x and y).
pub fn flat_length(a: Vec3) -> f32 {
    (a[0] * a[0] + a[1] * a[1]).sqrt()
}

/// How far apart `a` and `b` are seen from above: across the floor.
pub fn flat_distance(a: Vec3, b: Vec3) -> f32 {
    flat_length(sub(a, b))
}

/// Scales `a` to unit length in place and returns its length; a zero
/// vector stays zero (`VectorNormalize`: one reciprocal, three products).
pub fn normalize(a: &mut Vec3) -> f32 {
    let length = length(*a);
    if length != 0.0 {
        let inverse = 1.0 / length;
        *a = scale(*a, inverse);
    }
    length
}

/// A JA short angle (65 536 to the turn) in degrees (`SHORT2ANGLE`).
pub fn short_to_degrees(short: i16) -> f32 {
    f32::from(short) * (360.0 / 65_536.0)
}

/// Degrees to a JA short angle, truncated and wrapped (`ANGLE2SHORT`).
pub fn degrees_to_short(degrees: f32) -> i16 {
    ((degrees * 65_536.0 / 360.0) as i32 & 0xffff) as u16 as i16
}
