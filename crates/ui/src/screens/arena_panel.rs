//! The combat arena's panel in play: what its keys do, in a box in KOTOR 2's
//! frame (the rounded rim its option buttons wear, in the campaign's
//! colour) under the HUD's minimap at the top left. A title, then a row per
//! key: the key in the highlight colour, what it does, and its state on the
//! right (the foe it calls in, god mode on or off). Drawn over the HUD and
//! under the screens, and not a screen: nothing on it takes the focus.

use crate::draw::{DrawList, Quad};
use crate::paint::Painter;
use crate::skin::Source;
use crate::vm::ArenaPanelView;
use fmt_odyssey::gui::{Border, align};

const FONT: &str = "dialogfont16x16";
/// The HUD's control the panel stands under (its minimap's frame).
pub const UNDER: &str = "LBL_MAPBORDER";
/// KOTOR 2's rounded rim (its option buttons'), and how wide its art is.
const CORNER: &str = "uibit_brdr_16wc";
const EDGE: &str = "uibit_brdr_16we";
const RIM: i32 = 16;
/// The panel's width, the room inside its rim, the key column's width, the
/// gap under the minimap and between the title and the rows (layout pixels).
const WIDTH: f32 = 250.0;
const PAD: f32 = 12.0;
const KEY: f32 = 34.0;
const GAP: f32 = 6.0;
/// How far in from the rim's outer edge the dark fill starts (inside the
/// corners' curve), and how dark it is.
const INSET: f32 = 4.0;
const FILL: [f32; 4] = [0.0, 0.02, 0.05, 0.8];
/// A state that is off, as a share of the words' colour.
const OFF: f32 = 0.55;

/// `panel` in a frame of `frame` layout pixels (the HUD's), its top left at
/// `at`.
pub fn draw(
    painter: &mut Painter,
    panel: &ArenaPanelView,
    frame: (f32, f32),
    at: [f32; 2],
) -> DrawList {
    let mut out = DrawList {
        size: frame,
        ..DrawList::default()
    };
    painter.source = Source::Skin;
    let line = painter
        .textures
        .font(Source::Skin, FONT)
        .map_or(16.0, |(_, font)| font.line_height().ceil());
    let rows = panel.rows.len() as f32;
    let tall = 2.0 * PAD + line + GAP + rows * line;
    let rect = [at[0], at[1] + GAP, WIDTH, tall];
    let [l, t, w, h] = rect;
    out.quads.push(Quad::solid(
        [l + INSET, t + INSET, w - 2.0 * INSET, h - 2.0 * INSET],
        FILL,
    ));
    let palette = painter.palette;
    let rgba = |[r, g, b]: [f32; 3], a: f32| [r, g, b, a];
    let border = Border {
        corner: CORNER.into(),
        edge: EDGE.into(),
        dimension: RIM,
        color: Some(palette.base),
        ..Border::default()
    };
    painter.border(&border, rect, rgba(palette.base, 1.0), &mut out);
    let inner = w - 2.0 * PAD;
    let mut y = t + PAD;
    painter.text(
        FONT,
        &panel.title,
        [l + PAD, y, inner, line],
        align::CENTRE | align::MIDDLE,
        rgba(palette.hilight, 1.0),
        &mut out,
    );
    y += line + GAP / 2.0;
    out.quads.push(Quad::solid(
        [l + PAD, y - 1.0, inner, 1.0],
        rgba(palette.base, 0.6),
    ));
    y += GAP / 2.0;
    for row in &panel.rows {
        let cell = |x: f32, wide: f32| [x, y, wide, line];
        painter.text(
            FONT,
            &row.key,
            cell(l + PAD, KEY),
            align::LEFT | align::MIDDLE,
            rgba(palette.hilight, 1.0),
            &mut out,
        );
        painter.text(
            FONT,
            &row.words,
            cell(l + PAD + KEY, inner - KEY),
            align::LEFT | align::MIDDLE,
            rgba(palette.base, 1.0),
            &mut out,
        );
        let state = if row.on { 1.0 } else { OFF };
        painter.text(
            FONT,
            &row.state,
            cell(l + PAD + KEY, inner - KEY),
            align::RIGHT | align::MIDDLE,
            rgba([state; 3], 1.0),
            &mut out,
        );
        y += line;
    }
    out
}
