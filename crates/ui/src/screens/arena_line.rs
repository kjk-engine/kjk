//! The floors stage's line in play: a line of words across the top of the
//! window on a dark band. Drawn over the HUD and under the screens, and not
//! a screen: nothing on it takes the focus.

use crate::draw::{DrawList, Quad};
use crate::paint::Painter;
use crate::skin::Source;
use fmt_odyssey::gui::align;

const FONT: &str = crate::paint::NUMBER_FONT;
/// The band's colour, the words' colour, and the room round the words.
const PANEL: [f32; 4] = [0.0, 0.03, 0.08, 0.72];
const WORDS: [f32; 3] = [0.55, 0.75, 1.0];
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
    let [r, g, b] = WORDS;
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
