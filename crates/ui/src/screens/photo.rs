//! Photo mode's corner lines: what photo mode does now, and the keys that
//! leave it and take a screenshot. They stand in the window's lower left
//! corner at the window's own pixels, one over the other, over the world.
//! Everything else drawn over play hides while photo mode is on
//! (`crate::over_play::OverPlay::compose`).

use crate::draw::DrawList;
use crate::paint::Painter;

/// The corner lines, the first on top, and how much of them shows, from 0
/// to 1. The lines fade once the camera is left alone, and a screenshot is
/// taken without them.
#[derive(Debug, Clone, PartialEq)]
pub struct Lines {
    pub words: [Vec<u8>; 2],
    pub alpha: f32,
}

/// How far the lines stand from the window's left and lower edges, and
/// apart (pixels).
const MARGIN: f32 = 24.0;
const GAP: f32 = 4.0;

/// The lines for a window of `window` pixels, while any of them shows.
pub fn draw(painter: &mut Painter<'_>, lines: &Lines, window: (f32, f32)) -> Option<DrawList> {
    if lines.alpha <= 0.0 {
        return None;
    }
    let mut out = DrawList {
        size: window,
        ..DrawList::default()
    };
    let mut bottom = window.1 - MARGIN;
    for words in lines.words.iter().rev() {
        let first = out.quads.len();
        painter.tooltip(words, [MARGIN, bottom], &mut out);
        // The tip's box is its first quad: the next line stands over it.
        bottom = out.quads.get(first).map_or(bottom, |tip| tip.rect[1]) - GAP;
    }
    for quad in &mut out.quads {
        quad.color[3] *= lines.alpha;
    }
    Some(out)
}
