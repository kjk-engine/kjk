//! KOTOR's floating words over a creature's head, drawn over the HUD and
//! under the screens, and not a screen: a kill's XP ("XP 25") over its
//! dead, in KOTOR's magenta.
//!
//! As KOTOR draws them: in the campaign's `fnt_d16x16`, at the window's
//! own pixels, centred in a box 200 wide and one line tall whose bottom is
//! 32 pixels over the head; a newer one over the same creature stands
//! under the older, each a line higher. Each is ringed in black, drawn
//! first at the four diagonal pixels round it, and fades out evenly over
//! its time.
//!
//! The duel bars over saber users' heads (`bars`, `docs/SABER-RULES.md`
//! section 12) are drawn under the words, in the window's pixels, centred
//! over the head, faded with distance: KOTOR 2's portrait bar art lying on
//! its side, thin, the upper sloped at its top corners and the lower at
//! its bottom ones, touching, one rounded box (`OverheadArt`).

use crate::draw::DrawList;
use crate::paint::Painter;
use crate::screens::hud::{BarArt, Stand, meter};
use crate::skin::Source;
use crate::vm::{FloatyView, OverheadView};
use fmt_odyssey::gui::align;

const FONT: &str = crate::paint::NUMBER_FONT;
const WIDTH: f32 = 200.0;
/// How far over the head its box's bottom stands (pixels).
const OVER: f32 = 32.0;
/// Centred across, in the middle down.
const CENTRED: i32 = align::CENTRE | align::MIDDLE;
/// The ring's colour and where it is drawn round the words.
const RING: [f32; 3] = [0.0; 3];
const RING_AT: [[f32; 2]; 4] = [[-1.0, 1.0], [1.0, 1.0], [1.0, -1.0], [-1.0, -1.0]];

/// A kill's XP: "XP" (the talk table's, the same strref in both games) and
/// the XP, and its colour.
pub const XP_LINE: u32 = 38551;
pub const XP_COLOR: [f32; 3] = [0.95, 0.0, 0.85];

/// A head's duel bars: each bar's length and thickness (pixels: the spec's
/// 60 units at scale 1), and how far over the head the lowest stands.
const BARS: [f32; 2] = [60.0, 8.0];
const BARS_OVER: f32 = 10.0;

/// The art of a head's two duel bars, lying (`Stand::Lying`): the upper
/// the vitality bar's, sloped at its top corners, the lower the Force
/// bar's, sloped at its bottom ones, so the two touching make one rounded
/// box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OverheadArt {
    pub upper: BarArt,
    pub lower: BarArt,
}

/// The duel bars over saber users' heads (`overhead`) for a window of
/// `window` pixels, in the HUD's portrait bar art (`art`) lying down,
/// filling rightward: the block points (MDA; MD's Force) over the
/// stamina, touching; a lone stamina bar the upper art.
pub fn bars(overhead: &[OverheadView], art: OverheadArt, window: (f32, f32)) -> DrawList {
    let mut out = DrawList {
        size: window,
        ..DrawList::default()
    };
    let [wide, thick] = BARS;
    for head in overhead {
        let [x, y] = [head.over[0] * window.0, head.over[1] * window.1];
        let rows = match &head.bars.points {
            Some(points) => vec![(art.upper, points), (art.lower, &head.bars.stamina)],
            None => vec![(art.upper, &head.bars.stamina)],
        };
        let top = y - BARS_OVER - rows.len() as f32 * thick;
        for (nth, (art, bar)) in rows.into_iter().enumerate() {
            let rect = [x - wide / 2.0, top + nth as f32 * thick, wide, thick];
            out.quads
                .extend(meter(art, bar, rect, Stand::Lying, head.alpha));
        }
    }
    out
}

/// `floaties` for a window of `window` pixels.
pub fn draw(painter: &mut Painter, floaties: &[FloatyView], window: (f32, f32)) -> DrawList {
    let mut out = DrawList {
        size: window,
        ..DrawList::default()
    };
    painter.source = Source::Campaign;
    let line = painter
        .textures
        .font(Source::Campaign, FONT)
        .map_or(16.0, |(_, font)| font.line_height());
    for floaty in floaties {
        let [x, y] = [floaty.over[0] * window.0, floaty.over[1] * window.1];
        let top = y - OVER - line * (floaty.stacked + 1) as f32;
        let rect = |[dx, dy]: [f32; 2]| [x - WIDTH / 2.0 + dx, top + dy, WIDTH, line];
        let words = |painter: &mut Painter, at, [r, g, b]: [f32; 3], out: &mut DrawList| {
            let color = [r, g, b, floaty.alpha];
            painter.text(FONT, &floaty.text, rect(at), CENTRED, color, out);
        };
        for at in RING_AT {
            words(painter, at, RING, &mut out);
        }
        words(painter, [0.0; 2], floaty.color, &mut out);
    }
    out
}
