//! A bone's turn between two keys and into a blend, along arcs at full size
//! (QUIRKS.md Q-0080): the renderer's way (`asset_ja::g2`), so the blade
//! the sim cuts with is the blade the view draws. Ghoul2 mixes the 3×4
//! matrices value by value, which shrinks a bone turned far between two
//! keys (to nothing at half a turn) and whips its axes round; a hand's tag
//! built from a shrunken chain points its blade anywhere.
//!
//! Quaternions are (w, x, y, z); only `+ − × ÷ sqrt` and `sim::math`'s
//! trigonometry, so every platform turns them on the same bits.

use crate::math::{atan2_deg, sin_cos_deg};

pub type Quat = [f32; 4];

/// A bone's local transform: its turn and its place (units).
pub type Local = (Quat, [f32; 3]);

fn dot(a: Quat, b: Quat) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3]
}

fn scale(a: Quat, s: f32) -> Quat {
    a.map(|v| v * s)
}

fn add(a: Quat, b: Quat) -> Quat {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2], a[3] + b[3]]
}

pub fn normalize(a: Quat) -> Quat {
    let length = dot(a, a).sqrt();
    if length > 0.0 {
        scale(a, 1.0 / length)
    } else {
        a
    }
}

/// The angle (degrees) whose cosine is `d`, in [0, 180].
fn acos_deg(d: f32) -> f32 {
    let d = d.clamp(-1.0, 1.0);
    atan2_deg((1.0 - d * d).sqrt(), d)
}

fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// From `a` to `b` along their great circle at an even pace, the long way
/// round when their dot product is negative.
fn arc(a: Quat, b: Quat, t: f32) -> Quat {
    let d = dot(a, b).clamp(-1.0, 1.0);
    if d > 0.9999 {
        return normalize(add(scale(a, 1.0 - t), scale(b, t)));
    }
    if d < -0.9999 {
        // A full turn: the same rotation at both ends.
        return a;
    }
    let theta = acos_deg(d);
    let sin = sin_cos_deg(theta).0;
    normalize(add(
        scale(a, sin_cos_deg((1.0 - t) * theta).0 / sin),
        scale(b, sin_cos_deg(t * theta).0 / sin),
    ))
}

/// `v` turned by `q`.
fn rotate(q: Quat, v: [f32; 3]) -> [f32; 3] {
    let [w, x, y, z] = q;
    let cross = |a: [f32; 3], b: [f32; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let u = [x, y, z];
    let c = cross(u, v);
    let cc = cross(u, c);
    [0, 1, 2].map(|i| v[i] + 2.0 * (w * c[i] + cc[i]))
}

/// The place that puts the joint of a bone whose base place is `pivot`
/// where `a`'s and `b`'s, lerped, put it, once the bone is turned by `q`
/// (the renderer's `g2::joint`): a key turns its bone about the model's
/// origin, so a place lerped apart from the turn would pull the joint off
/// its parent by the chord between the two. Ghoul2's lerped matrix keeps
/// the joint in place.
fn joint(a: Local, b: Local, t: f32, pivot: [f32; 3], q: Quat) -> [f32; 3] {
    let at = |(turn, place): Local| {
        let turned = rotate(turn, pivot);
        [0, 1, 2].map(|i| place[i] + turned[i])
    };
    let lerped = lerp3(at(a), at(b), t);
    let turned = rotate(q, pivot);
    [0, 1, 2].map(|i| lerped[i] - turned[i])
}

/// Two keys of one animation mixed: the joint at `pivot` linearly
/// (`joint`), the turn along the shorter arc at an even pace.
pub fn mix_frames(a: Local, b: Local, t: f32, pivot: [f32; 3]) -> Local {
    let near = if dot(a.0, b.0) < 0.0 {
        scale(b.0, -1.0)
    } else {
        b.0
    };
    let q = arc(a.0, near, t);
    (q, joint(a, b, t, pivot, q))
}

/// A blend from the pose it starts from (`a`) into the moving animation
/// (`b`), which stood at `start` when the blend began: on the side that was
/// the shorter at the start, which stays (a side chosen afresh would flip
/// as the animation swings past half a turn from `a`), at the pace Ghoul2's
/// lerped matrix turns while the two are within a quarter turn, easing into
/// an even pace by half a turn (where Ghoul2's bone shrinks to nothing).
pub fn mix_blend(a: Local, b: Local, start: Quat, t: f32, pivot: [f32; 3]) -> Local {
    let side = if dot(a.0, start) < 0.0 { -1.0 } else { 1.0 };
    let follow = if dot(b.0, start) < 0.0 {
        scale(b.0, -1.0)
    } else {
        b.0
    };
    let target = scale(follow, side);
    let theta = 2.0 * acos_deg(dot(a.0, target));
    let even = t * theta;
    let turned = if theta >= 180.0 {
        even
    } else {
        let (sin, cos) = sin_cos_deg(theta);
        let polar = atan2_deg(t * sin, 1.0 - t + t * cos);
        let w = ((theta - 90.0) / 90.0).clamp(0.0, 1.0);
        let w = w * w;
        (1.0 - w) * polar + w * even
    };
    let f = if theta > 1e-4 { turned / theta } else { t };
    let q = arc(a.0, target, f);
    (q, joint(a, b, t, pivot, q))
}
