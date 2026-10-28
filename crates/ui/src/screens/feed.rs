//! The look of a camera feed over the world (a terminal's security camera,
//! not a screen): a cool tint, scanlines, a dark vignette, a bright band
//! rolling down, corner brackets and a blinking red dot. Drawn at the
//! window's own pixels over the view, under the screens.

use crate::draw::{DrawList, Quad};

const TINT: [f32; 4] = [0.0, 0.22, 0.18, 0.14];
const LINE: [f32; 4] = [0.0, 0.0, 0.0, 0.2];
/// Scanlines: one every `LINE_EVERY` pixels, `LINE_HEIGHT` thick.
const LINE_EVERY: f32 = 4.0;
const LINE_HEIGHT: f32 = 2.0;
/// The vignette: rings of the window's short side's share, each darker
/// toward the edge.
const RINGS: usize = 10;
const RING_SHARE: f32 = 0.012;
const RING_ALPHA: f32 = 0.05;
/// The rolling band: its height (share of the window's) and its speed
/// (windows a second).
const BAND_HEIGHT: f32 = 0.09;
const BAND_SPEED: f32 = 0.12;
const BAND: [f32; 4] = [0.8, 1.0, 0.95, 0.045];
const BRACKET: [f32; 4] = [0.85, 1.0, 0.95, 0.7];
const RECORDING: [f32; 3] = [0.9, 0.05, 0.05];

/// The feed's look for a window of `window` pixels at `time` seconds.
pub fn draw(window: (f32, f32), time: f32) -> DrawList {
    let (w, h) = window;
    let mut out = DrawList {
        size: window,
        ..DrawList::default()
    };
    let mut put = |rect: [f32; 4], color: [f32; 4]| out.quads.push(Quad::solid(rect, color));
    put([0.0, 0.0, w, h], TINT);
    let mut y = 0.0;
    while y < h {
        put([0.0, y, w, LINE_HEIGHT], LINE);
        y += LINE_EVERY;
    }
    let side = w.min(h) * RING_SHARE;
    for ring in 0..RINGS {
        let alpha = RING_ALPHA * (RINGS - ring) as f32 / RINGS as f32;
        let inset = side * ring as f32;
        let color = [0.0, 0.0, 0.0, alpha];
        put([inset, inset, w - 2.0 * inset, side], color);
        put([inset, h - inset - side, w - 2.0 * inset, side], color);
        put([inset, inset, side, h - 2.0 * inset], color);
        put([w - inset - side, inset, side, h - 2.0 * inset], color);
    }
    let band = h * BAND_HEIGHT;
    put(
        [
            0.0,
            (time * BAND_SPEED).fract() * (h + band) - band,
            w,
            band,
        ],
        BAND,
    );
    let (margin, arm, thick) = (w.min(h) * 0.05, w.min(h) * 0.05, 3.0);
    for (x, y, dx, dy) in [
        (margin, margin, 1.0, 1.0),
        (w - margin, margin, -1.0, 1.0),
        (margin, h - margin, 1.0, -1.0),
        (w - margin, h - margin, -1.0, -1.0),
    ] {
        let across = |x: f32, dx: f32| if dx > 0.0 { x } else { x - arm };
        let down = |y: f32, dy: f32| if dy > 0.0 { y } else { y - thick };
        put([across(x, dx), down(y, dy), arm, thick], BRACKET);
        let tall = |y: f32, dy: f32| if dy > 0.0 { y } else { y - arm };
        let beside = |x: f32, dx: f32| if dx > 0.0 { x } else { x - thick };
        put([beside(x, dx), tall(y, dy), thick, arm], BRACKET);
    }
    if time.fract() < 0.6 {
        let dot = w.min(h) * 0.02;
        let [r, g, b] = RECORDING;
        put(
            [margin + arm * 0.6, margin + arm * 0.6, dot, dot],
            [r, g, b, 0.95],
        );
    }
    out
}
