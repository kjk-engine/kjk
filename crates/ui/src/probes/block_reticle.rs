//! The marks of targeted blocks (`screens::hud::block_marks`): three marks
//! around the crosshair, the crosshair's side lit, the swing's side red until
//! the crosshair is on it and green then, and all three green for a swing
//! with no side.

use super::{Check, check, expect};
use crate::screens::hud::{
    BLOCK_MARK_CONFIRM, BLOCK_MARK_UNLIT, BLOCK_MARK_WARNING, RETICLE_LIGHT, block_marks,
};
use crate::vm::{BlockReticleView, Side, Swing};

/// The colour of each mark's stroke (the quad over its edge), left, right
/// and above, and how many quads were drawn.
fn colours(view: BlockReticleView) -> (usize, Vec<[f32; 4]>) {
    let quads = block_marks((1920, 1080), view, 4.0);
    let strokes = quads.iter().skip(1).step_by(2).map(|q| q.color).collect();
    (quads.len(), strokes)
}

pub(super) fn all() -> Vec<Check> {
    let (warning, confirm) = (BLOCK_MARK_WARNING, BLOCK_MARK_CONFIRM);
    let (unlit, lit) = (BLOCK_MARK_UNLIT, RETICLE_LIGHT);
    let view = |crosshair, swing| BlockReticleView { crosshair, swing };
    let rows = [
        ("empty", view(None, None), [unlit, unlit, unlit]),
        (
            "crosshair right",
            view(Some(Side::Right), None),
            [unlit, lit, unlit],
        ),
        (
            "swing left, crosshair right",
            view(Some(Side::Right), Some(Swing::Side(Side::Left))),
            [warning, lit, unlit],
        ),
        (
            "swing left, crosshair left",
            view(Some(Side::Left), Some(Swing::Side(Side::Left))),
            [confirm, unlit, unlit],
        ),
        (
            "swing above, crosshair on the enemy",
            view(None, Some(Swing::Side(Side::Above))),
            [unlit, unlit, warning],
        ),
        (
            "swing with no side",
            view(Some(Side::Left), Some(Swing::Any)),
            [confirm, confirm, confirm],
        ),
    ];
    let wrong: Vec<String> = rows
        .iter()
        .filter_map(|(name, view, want)| {
            let (count, got) = colours(*view);
            (count != 6 || got != want).then(|| format!("{name}: {count} quads, {got:?}"))
        })
        .collect();
    vec![check(
        "marks_drawn",
        expect(&wrong.join("; "), wrong.is_empty())
            .map(|()| format!("{} views, three marks each", rows.len())),
    )]
}
