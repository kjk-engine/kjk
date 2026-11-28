//! The outline view: a frame around every control of the current frame and
//! its tag beside it, drawn from the same marks the control tree is made
//! from (`ui::render`).

use std::sync::Arc;

use asset_odyssey::font::Font;
use ui::draw::{DrawList, Mark, Quad, TextureId};

/// The frame colours of the screens, bottom first.
const LAYERS: [[f32; 3]; 4] = [
    [1.0, 0.35, 0.8],
    [0.2, 0.9, 1.0],
    [1.0, 0.85, 0.2],
    [0.6, 1.0, 0.4],
];

/// How much of its colour the frame of a screen that another screen covers
/// keeps.
const COVERED: f32 = 0.45;

/// The focused control's frame.
const FOCUS: [f32; 3] = [1.0, 0.5, 0.1];

/// The text beside a control's frame. The UI lint of
/// https://github.com/kjk-engine/roadmap/issues/24 adds the numbers of the
/// rules a control breaks here.
fn label(mark: &Mark) -> String {
    mark.tag.clone()
}

/// The outlines of `layers` in a window of `window` logical pixels, each
/// display pixel taking `pixel` of them.
pub fn list(layers: &[DrawList], window: (f32, f32), pixel: f32) -> DrawList {
    let mut out = DrawList {
        size: window,
        ..DrawList::default()
    };
    let font = layers
        .iter()
        .flat_map(|layer| &layer.marks)
        .flat_map(|mark| &mark.words)
        .next()
        .map(|words| (words.texture, Arc::clone(&words.font)));
    let line = (pixel * 2.0).max(1.0);
    for (at, layer) in layers.iter().enumerate() {
        let (scale, [x, y]) = layer.placed(window);
        let alpha = if at + 1 == layers.len() { 1.0 } else { COVERED };
        let [r, g, b] = LAYERS[at % LAYERS.len()];
        for mark in &layer.marks {
            let [left, top, width, height] = mark.rect;
            let rect = [
                x + left * scale,
                y + top * scale,
                width * scale,
                height * scale,
            ];
            if mark.tag.is_empty() || rect[2] < 1.0 || rect[3] < 1.0 {
                continue;
            }
            let [r, g, b] = if mark.focused { FOCUS } else { [r, g, b] };
            let thick = if mark.focused { line * 2.0 } else { line };
            frame(&mut out, rect, thick, [r, g, b, alpha]);
            if let Some((texture, font)) = &font {
                text(
                    &mut out,
                    (*texture, font),
                    &label(mark),
                    [rect[0], rect[1]],
                    pixel * 14.0,
                    [r, g, b, alpha],
                );
            }
        }
    }
    out
}

fn frame(out: &mut DrawList, [x, y, w, h]: [f32; 4], thick: f32, color: [f32; 4]) {
    let thick = thick.min(w / 2.0).min(h / 2.0);
    for rect in [
        [x, y, w, thick],
        [x, y + h - thick, w, thick],
        [x, y, thick, h],
        [x + w - thick, y, thick, h],
    ] {
        out.quads.push(Quad::solid(rect, color));
    }
}

/// `words` in `font`, `height` logical pixels tall, on a dark plate, its
/// top left at `at`.
fn text(
    out: &mut DrawList,
    (texture, font): (TextureId, &Font),
    words: &str,
    at: [f32; 2],
    height: f32,
    [r, g, b, alpha]: [f32; 4],
) {
    let scale = height / font.height.max(1.0);
    let glyphs: Vec<_> = words.bytes().filter_map(|byte| font.glyph(byte)).collect();
    let width: f32 = glyphs.iter().map(|glyph| glyph.width * scale + scale).sum();
    out.quads.push(Quad::solid(
        [at[0], at[1], width + scale * 2.0, height + scale * 2.0],
        [0.0, 0.0, 0.0, 0.78 * alpha],
    ));
    let mut x = at[0] + scale;
    for glyph in glyphs {
        let rect = [x, at[1] + scale, glyph.width * scale, glyph.height * scale];
        out.quads.push(Quad {
            uv: glyph.uv,
            ..Quad::image(
                rect,
                texture,
                [(r + 1.0) / 2.0, (g + 1.0) / 2.0, (b + 1.0) / 2.0, alpha],
            )
        });
        x += glyph.width * scale + scale;
    }
}
