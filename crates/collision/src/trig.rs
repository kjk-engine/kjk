//! The trigonometry everything that decides sim state uses (the sim's
//! movement, the level's turned doors and placeables, facings read from a
//! module), built from `+ - * /`, `sqrt`, comparisons and rounding to whole
//! numbers alone, so every platform computes the same bits (`docs/SIM.md`:
//! the platform's `sin`, `cos` and `atan2` differ between C libraries in
//! their last bits; these do not). `sim::math` re-exports it.
//!
//! Accuracy (`kjk move trig` measures it against `f64` over every one of
//! the 65 536 angles a command can carry): sine and cosine within 1.2e-7 of
//! the true value, about one unit in the last place of an `f32` near 1.

/// Degrees to radians, rounded to `f32` once.
const RADIANS_PER_DEGREE: f32 = std::f32::consts::PI / 180.0;
/// Radians to degrees, rounded to `f32` once.
const DEGREES_PER_RADIAN: f32 = 180.0 / std::f32::consts::PI;

// Taylor coefficients to x^9 and x^8: on |x| <= pi/4 the first dropped
// terms are below 2e-9, far under an f32 step.
const S3: f32 = -1.0 / 6.0;
const S5: f32 = 1.0 / 120.0;
const S7: f32 = -1.0 / 5040.0;
const S9: f32 = 1.0 / 362_880.0;
const C2: f32 = -1.0 / 2.0;
const C4: f32 = 1.0 / 24.0;
const C6: f32 = -1.0 / 720.0;
const C8: f32 = 1.0 / 40_320.0;

/// Sine and cosine of an angle in degrees.
///
/// The angle is split into a whole number of quarter turns and a rest of
/// at most 45°, which the polynomials take; the quarter turns swap and
/// negate the pair. Not finite: (0, 1).
pub fn sin_cos_deg(degrees: f32) -> (f32, f32) {
    if !degrees.is_finite() {
        return (0.0, 1.0);
    }
    let quarters = (degrees / 90.0).round_ties_even();
    let x = (degrees - quarters * 90.0) * RADIANS_PER_DEGREE;
    let x2 = x * x;
    let sin = x * (1.0 + x2 * (S3 + x2 * (S5 + x2 * (S7 + x2 * S9))));
    let cos = 1.0 + x2 * (C2 + x2 * (C4 + x2 * (C6 + x2 * C8)));
    match (quarters as i64).rem_euclid(4) {
        0 => (sin, cos),
        1 => (cos, -sin),
        2 => (-sin, -cos),
        _ => (-cos, sin),
    }
}

/// Sine and cosine of an angle in radians (`sin_cos_deg` of it in
/// degrees).
pub fn sin_cos(radians: f32) -> (f32, f32) {
    sin_cos_deg(radians * DEGREES_PER_RADIAN)
}

/// Forward, right and up of Euler angles (pitch, yaw, roll in degrees;
/// pitch positive looking down, yaw positive turning left).
pub fn angle_vectors(angles: [f32; 3]) -> ([f32; 3], [f32; 3], [f32; 3]) {
    let (sy, cy) = sin_cos_deg(angles[1]);
    let (sp, cp) = sin_cos_deg(angles[0]);
    let (sr, cr) = sin_cos_deg(angles[2]);
    let forward = [cp * cy, cp * sy, -sp];
    let right = [-sr * sp * cy + cr * sy, -sr * sp * sy - cr * cy, -sr * cp];
    let up = [cr * sp * cy + sr * sy, cr * sp * sy - sr * cy, cr * cp];
    (forward, right, up)
}

/// The direction of (`x`, `y`) in degrees from +X, in (-180, 180]; 0 for
/// the zero vector (`atan2`, which the platform's library rounds its own
/// way). Worked in `f64` and rounded once: the ratio of the smaller to the
/// larger component is halved in angle twice (`atan t = 2 atan(t / (1 +
/// sqrt(1 + t²)))`, to at most 11.25°), then summed as a series to the 19th
/// power, whose first dropped term is under 1e-15.
pub fn atan2_deg(y: f32, x: f32) -> f32 {
    let (ax, ay) = (f64::from(x.abs()), f64::from(y.abs()));
    if ax == 0.0 && ay == 0.0 {
        return 0.0;
    }
    let steep = ay > ax;
    let mut t = if steep { ax / ay } else { ay / ax };
    for _ in 0..2 {
        t /= 1.0 + (1.0 + t * t).sqrt();
    }
    let t2 = t * t;
    let mut term = t;
    let mut sum = 0.0;
    for k in 0..10 {
        let odd = f64::from(2 * k + 1);
        sum += if k % 2 == 0 { term / odd } else { -term / odd };
        term *= t2;
    }
    let mut degrees = sum * 4.0 * (180.0 / std::f64::consts::PI);
    if steep {
        degrees = 90.0 - degrees;
    }
    if x < 0.0 {
        degrees = 180.0 - degrees;
    }
    if y < 0.0 {
        degrees = -degrees;
    }
    degrees as f32
}
