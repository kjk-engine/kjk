//! The Force power carousel in play, JA's way of picking among many powers
//! with two keys: while the player steps through its powers ([ and ]) and
//! a moment after, the powers round the chosen one stand in a column at the
//! bottom left of the window, over the HUD's action bar, the chosen one
//! bright between the two before and the two after, with its Force point
//! cost and its place among them; then it fades. Drawn over the HUD and
//! under the screens, and not a screen: nothing on it takes the focus.

use crate::draw::{DrawList, Quad};
use crate::paint::Painter;
use crate::skin::Source;
use crate::vm::PowerPickView;
use fmt_odyssey::gui::align;

const FONT: &str = crate::paint::NUMBER_FONT;
/// The column's width, its left edge, and how far its bottom stands over
/// the window's bottom (a share of the window's height: over KOTOR's
/// action bar).
const WIDTH: f32 = 280.0;
const LEFT: f32 = 8.0;
const ABOVE: f32 = 0.11;
/// How many powers show either side of the chosen one.
const AROUND: usize = 2;
const PANEL: [f32; 4] = [0.0, 0.03, 0.08, 0.72];
const CHOSEN: [f32; 3] = [1.0, 0.9, 0.35];
const OTHERS: [f32; 3] = [0.55, 0.75, 1.0];
/// The others' share of the chosen one's opacity.
const DIM: f32 = 0.6;
const PAD: f32 = 6.0;

/// How far down the window a line's band starts (a share of its height:
/// under the HUD's menu buttons at the top right).
const LINE_TOP: f32 = 0.07;

/// A line of words across the top of a window of `window` pixels on a dark
/// band (the combat arena's keys).
pub fn line(painter: &mut Painter, words: &[u8], window: (f32, f32)) -> DrawList {
    let mut out = DrawList {
        size: window,
        ..DrawList::default()
    };
    painter.source = Source::Campaign;
    let line = painter
        .textures
        .font(Source::Campaign, FONT)
        .map_or(16.0, |(_, font)| font.line_height());
    let top = window.1 * LINE_TOP;
    let band = [0.0, top, window.0, line + 2.0 * PAD];
    out.quads.push(Quad::solid(band, PANEL));
    let [r, g, b] = OTHERS;
    let rect = [0.0, top + PAD, window.0, line];
    painter.text(
        FONT,
        words,
        rect,
        align::CENTRE | align::MIDDLE,
        [r, g, b, 1.0],
        &mut out,
    );
    out
}

/// `pick` for a window of `window` pixels.
pub fn draw(painter: &mut Painter, pick: &PowerPickView, window: (f32, f32)) -> DrawList {
    let mut out = DrawList {
        size: window,
        ..DrawList::default()
    };
    if pick.names.is_empty() {
        return out;
    }
    painter.source = Source::Campaign;
    let line = painter
        .textures
        .font(Source::Campaign, FONT)
        .map_or(16.0, |(_, font)| font.line_height());
    let count = pick.names.len();
    let shown: Vec<usize> = if count <= 2 * AROUND + 1 {
        (0..count).collect()
    } else {
        (0..=2 * AROUND)
            .map(|at| (pick.chosen + count + at - AROUND) % count)
            .collect()
    };
    // The lines, then the chosen one's cost and place under them.
    let rows = shown.len() + 1;
    let height = rows as f32 * line + 2.0 * PAD;
    let left = LEFT;
    let top = window.1 * (1.0 - ABOVE) - height;
    let [r, g, b, a] = PANEL;
    out.quads.push(Quad::solid(
        [left, top, WIDTH, height],
        [r, g, b, a * pick.alpha],
    ));
    let centred = align::CENTRE | align::MIDDLE;
    for (at, row) in shown.iter().enumerate() {
        let chosen = *row == pick.chosen;
        let ([r, g, b], alpha) = if chosen {
            (CHOSEN, pick.alpha)
        } else {
            (OTHERS, pick.alpha * DIM)
        };
        let rect = [left, top + PAD + at as f32 * line, WIDTH, line];
        painter.text(
            FONT,
            &pick.names[*row],
            rect,
            centred,
            [r, g, b, alpha],
            &mut out,
        );
    }
    let rect = [left, top + PAD + shown.len() as f32 * line, WIDTH, line];
    let [r, g, b] = OTHERS;
    painter.text(
        FONT,
        &pick.under,
        rect,
        centred,
        [r, g, b, pick.alpha * DIM],
        &mut out,
    );
    out
}
