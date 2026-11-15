//! The arcs of targeted blocks (`screens::hud::block_arcs`). The aim arc
//! is 40 degrees long and 3 pixels thick. It sits at the direction of the
//! last camera turn, white, and dim while the turn points at no side. The
//! swing arc is 2 pixels thick and covers the swing's side, red until the
//! turn points toward that side and green then. A swing with no side turns
//! all three swing arcs green. Both arcs lie on a circle 27 pixels from the
//! middle at a 1080-high layout, and scale with its height.

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

/// The layouts the arcs are drawn at: 1080 and 2160 pixels high.
const FRAMES: [(i32, i32); 2] = [(1920, 1080), (3840, 2160)];

/// The name of `colour` in `COLOURS`, or "other".
fn name(colour: [f32; 4]) -> &'static str {
    (COLOURS.iter())
        .find(|(_, known)| *known == colour)
        .map_or("other", |(name, _)| *name)
}

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

/// One arc as the probe writes it: its colour, the angle where it starts
/// and the angle where it ends (degrees clockwise from straight up, the
/// start from 0 up to 360 and the end after it), and its thickness in
/// pixels.
fn written(colour: &str, (start, end): (f32, f32), thick: f32) -> String {
    format!("{colour} {start:.1} to {end:.1}, {thick:.1} px")
}

/// One arc drawn: its dark edges and then its strokes.
type Drawn<'a> = (Vec<&'a Quad>, Vec<&'a Quad>);

/// The arc of `strokes` as `written` writes it, with "edges missing" added
/// when the arc has not one dark edge for every stroke. The arc reaches
/// from the start of its first stroke to the end of its last.
///
/// A stroke quad is upright before it turns (`hud::stroke`). Its width is
/// the arc's thickness, and its height is the length it covers along the
/// circle. The width is not always the short side: a piece of the aim arc
/// is about 2.4 pixels long and 3 pixels thick at a 1080-high layout.
fn arc_written(frame: (i32, i32), (edges, strokes): &Drawn) -> String {
    let Some(first) = strokes.first() else {
        return "edges with no stroke".into();
    };
    let at = placed(frame, first).0;
    let ends = strokes.iter().flat_map(|stroke| {
        let (angle, distance) = placed(frame, stroke);
        // The stroke's angle, taken within half a turn of the first's.
        let angle = at + (angle - at + 180.0).rem_euclid(360.0) - 180.0;
        let half = (stroke.rect[3] / 2.0 / distance).to_degrees();
        [angle - half, angle + half]
    });
    let (low, high) = ends.fold((f32::MAX, f32::MIN), |(low, high), end| {
        (low.min(end), high.max(end))
    });
    let start = low.rem_euclid(360.0);
    let thick = strokes.iter().map(|stroke| stroke.rect[2]);
    let (thin, thickest) = thick.fold((f32::MAX, f32::MIN), |(thin, thickest), side| {
        (thin.min(side), thickest.max(side))
    });
    let colours: Vec<&str> = strokes.iter().map(|stroke| name(stroke.color)).collect();
    let colour = match colours.iter().all(|colour| *colour == colours[0]) {
        true => colours[0],
        false => "mixed",
    };
    let mut arc = written(colour, (start, start + high - low), thickest);
    if thin != thickest {
        arc += &format!(" and {thin:.1} px");
    }
    if edges.len() != strokes.len() {
        arc += ", edges missing";
    }
    arc
}

/// The arcs of `view` at a layout `frame` big, in the order they are drawn.
/// An arc is a run of dark edges followed by a run of strokes.
fn drawn(frame: (i32, i32), view: BlockReticleView) -> Vec<String> {
    let quads = block_arcs(frame, view);
    let mut arcs: Vec<Drawn> = Vec::new();
    for quad in &quads {
        let edge = quad.color[..3] == [0.0; 3];
        if arcs
            .last()
            .is_none_or(|(_, strokes)| edge && !strokes.is_empty())
        {
            arcs.push(Default::default());
        }
        let (edges, strokes) = arcs.last_mut().expect("an arc was just added");
        match edge {
            true => edges.push(quad),
            false => strokes.push(quad),
        }
    }
    arcs.iter().map(|arc| arc_written(frame, arc)).collect()
}

/// The quads of `view` whose middle is not between 24 and 30 pixels from
/// the middle of a 1080-high layout, and between 48 and 60 at a 2160-high
/// one.
fn off_the_circle(view: BlockReticleView) -> Vec<String> {
    FRAMES
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
    let view = |aim, turned, swing| BlockReticleView { aim, turned, swing };
    let (left, right) = (Some(Side::Left), Some(Side::Right));
    let swing = |side| Some(Swing::Side(side));
    // The arcs expected, in the order they are drawn: the colour, the start
    // and the end, and the thickness at a 1080-high layout. The swing arcs
    // above, right and left run from 315 to 405, 45 to 135 and 225 to 315,
    // the angles that the block rule gives each side. An aim arc is 40
    // degrees long, centred on the view's angle.
    type Want = &'static [(&'static str, (f32, f32), f32)];
    const ABOVE: (f32, f32) = (315.0, 405.0);
    const RIGHT: (f32, f32) = (45.0, 135.0);
    const LEFT: (f32, f32) = (225.0, 315.0);
    let rows: [(&str, BlockReticleView, Want); 10] = [
        ("nothing", view(None, None, None), &[]),
        (
            "aim only",
            view(Some(270.0), left, None),
            &[("light", (250.0, 290.0), 3.0)],
        ),
        (
            "a turn down",
            view(Some(180.0), None, None),
            &[("unlit", (160.0, 200.0), 3.0)],
        ),
        (
            "left swing, a turn down",
            view(Some(180.0), None, swing(Side::Left)),
            &[("warning", LEFT, 2.0), ("unlit", (160.0, 200.0), 3.0)],
        ),
        (
            "left swing, a turn left",
            view(Some(270.0), left, swing(Side::Left)),
            &[("confirm", LEFT, 2.0), ("light", (250.0, 290.0), 3.0)],
        ),
        (
            "left swing, a turn right",
            view(Some(90.0), right, swing(Side::Left)),
            &[("warning", LEFT, 2.0), ("light", (70.0, 110.0), 3.0)],
        ),
        (
            "right swing, a turn right",
            view(Some(90.0), right, swing(Side::Right)),
            &[("confirm", RIGHT, 2.0), ("light", (70.0, 110.0), 3.0)],
        ),
        (
            "swing above, a turn up",
            view(Some(0.0), Some(Side::Above), swing(Side::Above)),
            &[("confirm", ABOVE, 2.0), ("light", (340.0, 380.0), 3.0)],
        ),
        (
            "swing above, no turn",
            view(None, None, swing(Side::Above)),
            &[("warning", ABOVE, 2.0)],
        ),
        (
            "swing with no side",
            view(Some(270.0), left, Some(Swing::Any)),
            &[
                ("confirm", ABOVE, 2.0),
                ("confirm", RIGHT, 2.0),
                ("confirm", LEFT, 2.0),
                ("light", (250.0, 290.0), 3.0),
            ],
        ),
    ];
    let wrong: Vec<String> = rows
        .iter()
        .flat_map(|(name, view, want)| {
            let off = off_the_circle(*view);
            let off = (!off.is_empty()).then(|| format!("{name}: off the circle {off:?}"));
            let arcs = FRAMES.into_iter().filter_map(move |frame| {
                let scale = frame.1 as f32 / 1080.0;
                let want: Vec<String> = (want.iter())
                    .map(|(colour, span, thick)| written(colour, *span, thick * scale))
                    .collect();
                let got = drawn(frame, *view);
                (got != want)
                    .then(|| format!("{name}, {} high: drew {got:?}, expected {want:?}", frame.1))
            });
            off.into_iter().chain(arcs)
        })
        .collect();
    vec![check(
        "arcs_drawn",
        expect(&wrong.join("; "), wrong.is_empty()).map(|()| {
            format!(
                "{} views at 1080 and 2160 pixels high, each arc in the place, the length, the thickness, the colour and the order expected",
                rows.len()
            )
        }),
    )]
}
