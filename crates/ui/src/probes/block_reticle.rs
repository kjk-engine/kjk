//! The arcs of targeted blocks (`screens::hud::block_arcs`). The aim arc
//! sits at the crosshair's angle around the chest, white, and dim while the
//! crosshair points at no side. The swing arc covers the swing's side, red
//! until the crosshair is on that side and green then. A swing with no side
//! turns all three swing arcs green. Both arcs lie on a circle 27 pixels
//! from the middle at a 1080-high layout, and scale with its height.

use std::collections::BTreeSet;

use super::{Check, check, expect};
use crate::draw::Quad;
use crate::screens::hud::{
    BLOCK_MARK_CONFIRM, BLOCK_MARK_UNLIT, BLOCK_MARK_WARNING, RETICLE_LIGHT, block_arcs,
};
use crate::vm::{BlockReticleView, Side, Swing};

/// The colours the arcs may use, by name.
const COLOURS: [(&str, [f32; 4]); 4] = [
    ("light", RETICLE_LIGHT),
    ("unlit", BLOCK_MARK_UNLIT),
    ("warning", BLOCK_MARK_WARNING),
    ("confirm", BLOCK_MARK_CONFIRM),
];

/// Where the middle of `quad` is around the middle of a layout `frame` big:
/// its angle in degrees clockwise from straight up, and its distance in
/// pixels.
fn placed(frame: (i32, i32), quad: &Quad) -> (f32, f32) {
    let [x, y, width, height] = quad.rect;
    let dx = x + width / 2.0 - frame.0 as f32 / 2.0;
    let dy = y + height / 2.0 - frame.1 as f32 / 2.0;
    let angle = dx.atan2(-dy).to_degrees().rem_euclid(360.0);
    (angle, dx.hypot(dy))
}

/// The part of the circle an angle falls in: the three sides of a swing,
/// and the bottom 40 degrees, which belongs to no side.
fn region(angle: f32) -> &'static str {
    match angle {
        a if !(45.0..315.0).contains(&a) => "above",
        a if a < 160.0 => "right",
        a if a < 200.0 => "bottom",
        _ => "left",
    }
}

/// What the arcs of `view` draw at a 1080-high layout: the region and the
/// colour of every stroke, and the colour of the last quad drawn. A dark
/// edge is drawn under every stroke, so there must be as many edges (black
/// quads) as strokes. When the counts differ, the set holds the pair
/// ("edges", "missing").
fn drawn(view: BlockReticleView) -> (BTreeSet<(&'static str, &'static str)>, Option<&'static str>) {
    let frame = (1920, 1080);
    let quads = block_arcs(frame, view);
    let name = |colour: [f32; 4]| {
        (COLOURS.iter())
            .find(|(_, known)| *known == colour)
            .map_or("other", |(name, _)| *name)
    };
    let (edges, strokes): (Vec<&Quad>, Vec<&Quad>) =
        quads.iter().partition(|quad| quad.color[..3] == [0.0; 3]);
    let mut got: BTreeSet<_> = strokes
        .iter()
        .map(|stroke| (region(placed(frame, stroke).0), name(stroke.color)))
        .collect();
    if edges.len() != strokes.len() {
        got.insert(("edges", "missing"));
    }
    (got, quads.last().map(|quad| name(quad.color)))
}

/// The quads of `view` whose middle is not between 24 and 30 pixels from
/// the middle of a 1080-high layout, and between 48 and 60 at a 2160-high
/// one.
fn off_the_circle(view: BlockReticleView) -> Vec<String> {
    [(1920, 1080), (3840, 2160)]
        .into_iter()
        .flat_map(|frame| {
            let scale = frame.1 as f32 / 1080.0;
            block_arcs(frame, view)
                .iter()
                .map(|quad| placed(frame, quad).1)
                .filter(|&distance| !(24.0 * scale..=30.0 * scale).contains(&distance))
                .map(|distance| format!("{} high: {distance}", frame.1))
                .collect::<Vec<_>>()
        })
        .collect()
}

pub(super) fn all() -> Vec<Check> {
    let view = |aim, crosshair, swing| BlockReticleView {
        aim,
        crosshair,
        swing,
    };
    let (left, right) = (Some(Side::Left), Some(Side::Right));
    let swing = |side| Some(Swing::Side(side));
    type Want = &'static [(&'static str, &'static str)];
    let rows: [(&str, BlockReticleView, Want); 7] = [
        ("nothing", view(None, None, None), &[]),
        (
            "aim only",
            view(Some(270.0), left, None),
            &[("left", "light")],
        ),
        (
            "aim below the chest",
            view(Some(180.0), None, None),
            &[("bottom", "unlit")],
        ),
        (
            "left swing, crosshair left",
            view(Some(270.0), left, swing(Side::Left)),
            &[("left", "confirm"), ("left", "light")],
        ),
        (
            "left swing, crosshair right",
            view(Some(90.0), right, swing(Side::Left)),
            &[("left", "warning"), ("right", "light")],
        ),
        (
            "swing above, crosshair on the chest",
            view(None, None, swing(Side::Above)),
            &[("above", "warning")],
        ),
        (
            "swing with no side",
            view(Some(270.0), left, Some(Swing::Any)),
            &[
                ("above", "confirm"),
                ("right", "confirm"),
                ("left", "confirm"),
                ("left", "light"),
            ],
        ),
    ];
    let wrong: Vec<String> = rows
        .iter()
        .filter_map(|(name, view, want)| {
            let (got, last) = drawn(*view);
            let want: BTreeSet<_> = want.iter().copied().collect();
            // The aim arc is drawn last, over the swing arc.
            let aim_colour = if view.crosshair.is_some() {
                "light"
            } else {
                "unlit"
            };
            let aim_on_top = view.aim.is_none() || last == Some(aim_colour);
            let off = off_the_circle(*view);
            (got != want || !aim_on_top || !off.is_empty()).then(|| {
                format!("{name}: drew {got:?}, the last quad {last:?}, off the circle {off:?}")
            })
        })
        .collect();
    vec![check(
        "arcs_drawn",
        expect(&wrong.join("; "), wrong.is_empty()).map(|()| {
            format!(
                "{} views, each arc where and in the colour expected",
                rows.len()
            )
        }),
    )]
}
