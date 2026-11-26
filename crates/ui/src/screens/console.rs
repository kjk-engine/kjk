//! The console, dropped down from the top of the window over everything
//! else: the HUD, the screens, a conversation. It is not a screen on the
//! stack, so it never changes which screen is open under it.
//!
//! It is drawn in KOTOR 2's own art alone, in KOTOR 2's colours whichever
//! game is played. The frame is the rounded rim of KOTOR 2's option buttons
//! (the combat arena's panel wears it too), round a dark fill. The title is
//! in KOTOR 2's label font, the log in the font of KOTOR 2's Messages log,
//! mistakes in KOTOR 2's red. The log scrolls with the scroll bar of KOTOR
//! 2's Messages page, and the line being typed shows the name entry box's
//! blinking `_` at its caret. Tab's choices show in a box of the same
//! frame above the line being typed, the chosen one lit as a KOTOR 2 list
//! lights its row.

use crate::draw::{DrawList, Quad};
use crate::paint::{Painter, RED};
use crate::skin::Source;
use crate::vm::{ConsoleChoice, ConsoleView};
use fmt_odyssey::gui::{Border, Control, Rect, align};

/// The font of KOTOR 2's Messages log rows, and of its labels (the title).
/// Each has its "b" sheet from 1280 pixels of window width.
pub const FONT: &str = crate::paint::NUMBER_FONT;
const TITLE_FONT: &str = "dialogfont16x16";
/// KOTOR 2's rounded rim, and how wide its art is.
const CORNER: &str = "uibit_brdr_16wc";
const EDGE: &str = "uibit_brdr_16we";
const RIM: i32 = 16;
/// The room left of, right of and above the console (window pixels), and
/// the room inside its rim.
const MARGIN: f32 = 16.0;
const PAD: f32 = 12.0;
/// The gap between the title, the log and the line being typed.
const GAP: f32 = 6.0;
/// How far in from the rim's outer edge the dark fill starts, and how dark
/// it is: the console's own a little see-through, Tab's choices' box over
/// the log not at all.
const INSET: f32 = 4.0;
const FILL: [f32; 4] = [0.0, 0.02, 0.05, 0.92];
const CHOICES_FILL: [f32; 4] = [0.0, 0.02, 0.05, 1.0];
/// The room between a choice's word and its name from the game's data.
const NAME_GAP: f32 = 24.0;
/// The console's height as a share of the window's, and the fewest lines
/// of log it shows.
const SHARE: f32 = 0.4;
const FEWEST_LINES: f32 = 8.0;
/// What the line being typed starts with, and the caret.
const PROMPT: &[u8] = b"> ";
const CARET: &[u8] = b"_";
/// The most of Tab's choices shown at once.
pub const CHOICES: usize = 8;

/// Where the console's parts are, in window pixels.
struct Places {
    panel: [f32; 4],
    title: [f32; 4],
    log: [f32; 4],
    input: [f32; 4],
    /// Tab's choices' box, its first row's top, and where their names start
    /// across it, when there are choices.
    choices: Option<([f32; 4], f32, f32)>,
    line: f32,
}

/// The console's parts for a window of `window` pixels.
fn places(painter: &mut Painter, view: &ConsoleView, window: (f32, f32)) -> Places {
    let line = painter
        .textures
        .font(Source::Skin, FONT)
        .map_or(16.0, |(_, font)| font.line_height().ceil());
    let chrome = 2.0 * PAD + 2.0 * line + 2.0 * GAP;
    let tall = (window.1 * SHARE).max(chrome + FEWEST_LINES * line);
    let panel = [MARGIN, MARGIN, (window.0 - 2.0 * MARGIN).max(0.0), tall];
    let [l, t, w, h] = panel;
    let inner = (w - 2.0 * PAD).max(0.0);
    let title = [l + PAD, t + PAD, inner, line];
    let input = [l + PAD, t + h - PAD - line, inner, line];
    let log_top = title[1] + line + GAP;
    let log = [l + PAD, log_top, inner, (input[1] - GAP - log_top).max(0.0)];
    let shown = view.candidates.len().min(CHOICES);
    let choices = (shown > 0).then(|| {
        let font = painter.textures.font(Source::Skin, FONT);
        let widest = |part: fn(&ConsoleChoice) -> &[u8]| {
            font.as_ref().map_or(0.0, |(_, font)| {
                (view.candidates.iter())
                    .map(|choice| font.measure(part(choice)))
                    .fold(0.0, f32::max)
            })
        };
        let (word, name) = (widest(|choice| &choice.word), widest(|choice| &choice.name));
        let named = if name > 0.0 { NAME_GAP + name } else { 0.0 };
        let wide = (word + named + 2.0 * PAD).min(inner);
        let high = shown as f32 * line + 2.0 * PAD;
        let rect = [input[0], input[1] - GAP - high, wide, high];
        (rect, rect[1] + PAD, rect[0] + PAD + word + NAME_GAP)
    });
    Places {
        panel,
        title,
        log,
        input,
        choices,
        line,
    }
}

/// The first of Tab's choices the box shows, with `chosen` the one chosen:
/// the chosen one stays in the box.
pub fn first_choice(chosen: Option<usize>) -> usize {
    chosen.map_or(0, |chosen| (chosen + 1).saturating_sub(CHOICES))
}

/// Which of Tab's choices is at `point` (window pixels), if one is.
pub fn choice_at(
    painter: &mut Painter,
    view: &ConsoleView,
    window: (f32, f32),
    point: [f32; 2],
) -> Option<usize> {
    painter.source = Source::Skin;
    let places = places(painter, view, window);
    let ([l, t, w, h], first, _) = places.choices?;
    let [x, y] = point;
    if x < l || x > l + w || y < t || y > t + h || y < first {
        return None;
    }
    let row = ((y - first) / places.line) as usize;
    let index = first_choice(view.chosen) + row;
    (row < CHOICES && index < view.candidates.len()).then_some(index)
}

/// The rim of KOTOR 2's option buttons round `rect`, on `fill`.
fn framed(painter: &mut Painter, rect: [f32; 4], fill: [f32; 4], out: &mut DrawList) {
    let [l, t, w, h] = rect;
    out.quads.push(Quad::solid(
        [l + INSET, t + INSET, w - 2.0 * INSET, h - 2.0 * INSET],
        fill,
    ));
    let base = painter.palette.base;
    let border = Border {
        corner: CORNER.into(),
        edge: EDGE.into(),
        dimension: RIM,
        color: Some(base),
        ..Border::default()
    };
    painter.border(&border, rect, rgba(base), out);
}

fn rgba([r, g, b]: [f32; 3]) -> [f32; 4] {
    [r, g, b, 1.0]
}

/// The console over a window of `window` pixels. `bar` is the scroll bar of
/// KOTOR 2's Messages page, whose art the log's scroll bar is drawn with.
pub fn draw(
    painter: &mut Painter,
    view: &ConsoleView,
    window: (f32, f32),
    bar: Option<&Control>,
) -> DrawList {
    let mut out = DrawList {
        size: window,
        ..DrawList::default()
    };
    painter.source = Source::Skin;
    let places = places(painter, view, window);
    let palette = painter.palette;
    framed(painter, places.panel, FILL, &mut out);
    let line = places.line;
    painter.text(
        TITLE_FONT,
        crate::strings::CONSOLE_TITLE.as_bytes(),
        places.title,
        align::CENTRE | align::MIDDLE,
        rgba(palette.hilight),
        &mut out,
    );
    let [l, t, w, _] = places.title;
    out.quads.push(Quad::solid(
        [l, t + line + GAP / 2.0 - 1.0, w, 1.0],
        [palette.base[0], palette.base[1], palette.base[2], 0.6],
    ));
    log(painter, view, &places, bar, &mut out);
    input(painter, view, &places, &mut out);
    if let Some((rect, first, names)) = places.choices {
        framed(painter, rect, CHOICES_FILL, &mut out);
        let start = first_choice(view.chosen);
        let right = rect[0] + rect[2] - PAD;
        for (row, choice) in view.candidates.iter().enumerate().skip(start).take(CHOICES) {
            let lit = view.chosen == Some(row);
            let colour = rgba(if lit { palette.hilight } else { palette.base });
            let top = first + (row - start) as f32 * line;
            for (words, left) in [(&choice.word, rect[0] + PAD), (&choice.name, names)] {
                painter.text(
                    FONT,
                    words,
                    [left, top, (right - left).max(0.0), line],
                    align::LEFT | align::MIDDLE,
                    colour,
                    &mut out,
                );
            }
        }
    }
    out
}

/// The log's rows, each line wrapped to the log's width, the last ones that
/// fit above the rows scrolled away; and the scroll bar when the log does
/// not fit.
fn log(
    painter: &mut Painter,
    view: &ConsoleView,
    places: &Places,
    bar: Option<&Control>,
    out: &mut DrawList,
) {
    let Some((_, font)) = painter.textures.font(Source::Skin, FONT) else {
        return;
    };
    let [l, t, w, h] = places.log;
    let line = places.line;
    let bar_wide = bar.map_or(0.0, |bar| bar.extent.width as f32);
    let rows: Vec<(Vec<u8>, bool)> = (view.log.iter())
        .flat_map(|entry| {
            crate::paint::wrap(&font, &entry.text, w - bar_wide - GAP)
                .into_iter()
                .map(|row| (row, entry.error))
        })
        .collect();
    let fit = ((h / line).floor() as usize).max(1);
    let last = rows.len().saturating_sub(fit);
    let scrolled = view.scrolled.min(last);
    let first = last - scrolled;
    for (at, (words, error)) in rows.iter().skip(first).take(fit).enumerate() {
        let colour = if *error { RED } else { painter.palette.base };
        painter.text(
            FONT,
            words,
            [l, t + at as f32 * line, w - bar_wide, line],
            align::LEFT | align::MIDDLE,
            rgba(colour),
            out,
        );
    }
    if let Some(bar) = bar {
        let mut placed = bar.clone();
        placed.extent = Rect {
            left: (l + w - bar_wide) as i32,
            top: t as i32,
            width: bar_wide as i32,
            height: h as i32,
        };
        painter.scrollbar(&placed, rows.len(), fit, first, out);
    }
}

/// The line being typed after its prompt, the caret's `_` blinking at the
/// caret, and a line under the log.
fn input(painter: &mut Painter, view: &ConsoleView, places: &Places, out: &mut DrawList) {
    let [l, t, w, h] = places.input;
    let base = painter.palette.base;
    out.quads.push(Quad::solid(
        [l, t - GAP / 2.0, w, 1.0],
        [base[0], base[1], base[2], 0.6],
    ));
    let typed = [PROMPT, &view.input].concat();
    let colour = rgba(painter.palette.hilight);
    painter.text(
        FONT,
        &typed,
        places.input,
        align::LEFT | align::MIDDLE,
        colour,
        out,
    );
    if !view.blink {
        return;
    }
    let Some((_, font)) = painter.textures.font(Source::Skin, FONT) else {
        return;
    };
    let before = font.measure(&typed[..PROMPT.len() + view.caret.min(view.input.len())]);
    painter.text(
        FONT,
        CARET,
        [l + before, t, (w - before).max(0.0), h],
        align::LEFT | align::MIDDLE,
        colour,
        out,
    );
}
