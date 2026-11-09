//! A blaster's crosshair (`screens::hud::reticle`): each shape's strokes
//! start no nearer the middle than its gap, and the gap is where a shot
//! straying the whole spread lands on the screen, whatever the field of
//! view.

use super::menus::root;
use super::{Check, check, expect, near};
use crate::draw::Quad;
use crate::screens::hud::{Family, MIN_GAP, gap_px, reticle};

const FAMILIES: [(Family, usize); 6] = [
    (Family::Cross, 4),
    (Family::Prongs, 3),
    (Family::Brackets, 6),
    (Family::Lines, 4),
    (Family::Ring, 12),
    (Family::Dashes, 8),
];

/// How near the middle any corner of `quad` comes, turned by its spin.
fn nearest(quad: &Quad, middle: [f32; 2]) -> f32 {
    let [left, top, width, height] = quad.rect;
    let centre = [left + width / 2.0, top + height / 2.0];
    let (sin, cos) = quad.spin.sin_cos();
    [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
        .iter()
        .map(|(x, y)| {
            let (dx, dy) = (x * width / 2.0, y * height / 2.0);
            let at = [
                centre[0] + dx * cos - dy * sin - middle[0],
                centre[1] + dx * sin + dy * cos - middle[1],
            ];
            (at[0] * at[0] + at[1] * at[1]).sqrt()
        })
        .fold(f32::MAX, f32::min)
}

pub(super) fn all() -> Vec<Check> {
    let gui = root(1920, 1080, Vec::new());
    let middle = [960.0, 540.0];
    let gap = 20.0;
    let shapes: Vec<(Family, usize, usize, f32)> = FAMILIES
        .iter()
        .map(|&(family, strokes)| {
            let quads = reticle(gui.frame(), family, gap);
            // The dot's two quads (its edge and itself) lie at the middle;
            // each stroke has an edge under it.
            let nearest = quads
                .iter()
                .skip(2)
                .map(|quad| nearest(quad, middle))
                .fold(f32::MAX, f32::min);
            (family, quads.len(), 2 + 2 * strokes, nearest)
        })
        .collect();
    let shaped = shapes
        .iter()
        .all(|&(_, got, want, nearest)| got == want && nearest >= gap - 3.0);
    let at_64 = gap_px(2.0, 64.94, 1080.0);
    let want_64 = 2f32.to_radians().tan() / 32.47f32.to_radians().tan() * 540.0;
    let at_90 = gap_px(2.0, 90.0, 1080.0);
    let none = gap_px(0.0, 64.94, 1080.0);
    let half_layout = gap_px(0.0, 64.94, 540.0);
    let gaps_ok = near(at_64, want_64)
        && at_90 < at_64
        && near(none, MIN_GAP)
        && near(half_layout, MIN_GAP / 2.0);
    vec![
        check(
            "reticle_shapes",
            expect(
                &format!("(family, quads, want, nearest corner to the middle): {shapes:?}"),
                shaped,
            )
            .map(|()| "each shape's strokes, edged, outside its gap".into()),
        ),
        check(
            "reticle_gap_matches_fov",
            expect(
                &format!("2° at 64.94° {at_64} (want {want_64}), at 90° {at_90}; none {none}, at 540 high {half_layout}"),
                gaps_ok,
            )
            .map(|()| format!("2° spread: {at_64:.1} px at 64.94°, {at_90:.1} px at 90°")),
        ),
    ]
}
