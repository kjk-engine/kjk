//! KOTOR's status summary (`statussummary`, KOTOR 2's `statussummary_p`
//! with the skin): what a conversation or a script told the party (a
//! journal entry added, XP or credits received, light or dark side points,
//! items received or lost, KOTOR 2's influence gained or lost and its most
//! Force points raised or lowered), each a row
//! of the layout, its icon and words; a row its layout lacks (K1's has no
//! influence) is not shown; and ours alike, words alone in the rows' place
//! (a game saved). A datapad the party picks up gets a panel of its own:
//! the datapad's icon, its name and the start of its words in the received
//! row's place, and under them the key that reads it (`SummaryView::prompt`).
//! KOTOR 2 builds the panel from the rows that apply (`place`), centres it
//! in the window and pauses the game under it until OK. Here it pauses
//! nothing and has no OK. It stands where KOTOR 2 puts it and fades a few
//! seconds later (`kjk_game::progress`). Drawn over the HUD in the window's
//! physical pixels, as KOTOR 2 draws it; not a screen.

use fmt_odyssey::gui::{Control, Gui, Rect, align};

use crate::draw::DrawList;
use crate::paint::{ControlState, Painter, wrap};
use crate::skin::Source;
use crate::vm::{KeyLabel, SummaryView};

pub const LAYOUT: &str = "statussummary";

/// A row of the summary: its icon's and words' controls in the layout,
/// and the HUD's status icon that is lit with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Journal,
    Credits,
    Xp,
    DarkSide,
    LightSide,
    Received,
    Lost,
    InfluenceGained,
    InfluenceLost,
    MaxFpGained,
    MaxFpLost,
    /// Words of ours, no icon (`kjk_game::progress::Told`), in the
    /// journal row's place and font.
    Notice,
    /// A datapad the party picked up: its name and the start of its words,
    /// in the received row's place, with the datapad's icon
    /// (`SummaryView::icon`).
    Datapad,
}

impl Row {
    const ALL: [Row; 11] = [
        Row::Journal,
        Row::Credits,
        Row::Xp,
        Row::DarkSide,
        Row::LightSide,
        Row::Received,
        Row::Lost,
        Row::InfluenceGained,
        Row::InfluenceLost,
        Row::MaxFpGained,
        Row::MaxFpLost,
    ];

    /// Its icon's and its words' controls (a notice's icon is not drawn).
    pub(crate) fn tags(self) -> (&'static str, &'static str) {
        match self {
            Row::Journal | Row::Notice => ("LBL_JOURNAL", "LBL_JOURNAL_DESC"),
            Row::Credits => ("LBL_CREDITS", "LBL_CREDITS_DESC"),
            Row::Xp => ("LBL_XP", "LBL_XP_DESC"),
            Row::DarkSide => ("LBL_DARKSIDE", "LBL_DARKSIDE_DESC"),
            Row::LightSide => ("LBL_LIGHTSIDE", "LBL_LIGHTSIDE_DESC"),
            Row::Received | Row::Datapad => ("LBL_RECEIVED", "LBL_RECEIVED_DESC"),
            Row::Lost => ("LBL_LOST", "LBL_LOST_DESC"),
            Row::InfluenceGained => ("LBL_INFLUENCE_RECV", "LBL_INFLUENCE_RECV_DESC"),
            Row::InfluenceLost => ("LBL_INFLUENCE_LOST", "LBL_INFLUENCE_LOST_DESC"),
            Row::MaxFpGained => ("LBL_MAX_FP_GAINED", "LBL_MAX_FP_GAINED_DESC"),
            Row::MaxFpLost => ("LBL_MAX_FP_LOST", "LBL_MAX_FP_LOST_DESC"),
        }
    }

    /// The HUD's status icon lit with it (`hud::STATUS`): none for
    /// influence and the most Force points, which KOTOR 2's HUD has no icon
    /// for, nor for a notice of ours.
    pub fn hud(self) -> Option<&'static str> {
        Some(match self {
            Row::Journal => "LBL_JOURNAL",
            Row::Credits => "LBL_CASH",
            Row::Xp => "LBL_PLOTXP",
            Row::DarkSide => "LBL_DARKSHIFT",
            Row::LightSide => "LBL_LIGHTSHIFT",
            Row::Received => "LBL_ITEMRCVD",
            Row::Lost => "LBL_ITEMLOST",
            Row::InfluenceGained
            | Row::InfluenceLost
            | Row::MaxFpGained
            | Row::MaxFpLost
            | Row::Notice
            | Row::Datapad => {
                return None;
            }
        })
    }
}

/// KOTOR 2's numbers for the summary, in physical pixels at a text size of
/// 1 (`place`).
mod sizes {
    /// The first row's top, as KOTOR 2 works it out: 1.8 times the text
    /// size, times 10.
    pub const FIRST: (f32, f32) = (1.8, 10.0);
    /// How far each row stands below the one before.
    pub const ROW: f32 = 37.0;
    /// The side of a row's icon.
    pub const ICON: f32 = 32.0;
    /// A row's words' height (one line).
    pub const WORDS: f32 = 31.0;
    /// 1.5 times the text size is KOTOR 2's unit for the widths below.
    pub const UNIT: f32 = 1.5;
    /// The narrowest the words may make the panel, in units.
    pub const NARROWEST: f32 = 150.0;
    /// The narrowest the received row's words make the panel, in units.
    pub const RECEIVED: f32 = 300.0;
    /// How much wider words that wrap are made at a time, in units.
    pub const WIDEN: f32 = 20.0;
    /// Words are widened only while they are this wide or narrower.
    pub const WIDEST: i32 = 439;
    /// Words that need a taller row than this in the received row's place
    /// push the next row down by their own height.
    pub const TALL: i32 = 36;
    /// The gap under the OK (or the prompt) at the panel's bottom.
    pub const BOTTOM: i32 = 20;
    /// How much the words' width narrows at a time while it keeps their
    /// lines (`ideal`).
    pub const NARROW: i32 = 10;
}

/// Between two panels shown at once (kjk's own), at a text size of 1.
const MARGIN: f32 = 12.0;
/// The prompt's height when the layout has no OK in whose place it goes,
/// at a text size of 1, and the gap between its key and its words.
const PROMPT_TALL: f32 = 22.0;
const PROMPT_GAP: f32 = 6.0;

/// The row whose place `row` takes in KOTOR 2's order (a notice the
/// journal's, a datapad the received row's).
fn place_of(row: Row) -> Row {
    match row {
        Row::Notice => Row::Journal,
        Row::Datapad => Row::Received,
        row => row,
    }
}

/// How many lines `words` wrap to (`lines`) in a copy of `control` made
/// `width` wide.
fn lines_at(
    control: &Control,
    words: &[u8],
    width: i32,
    lines: &mut dyn FnMut(&Control, &[u8]) -> usize,
) -> usize {
    let mut measured = control.clone();
    measured.extent.width = width;
    lines(&measured, words)
}

/// The narrowest width for `words` in a copy of `control`, as KOTOR 2
/// measures it: down from `width` 10 pixels at a time while the words keep
/// as many lines (`lines`) as at `width`, and back up to the last width
/// that kept them.
fn ideal(
    control: &Control,
    words: &[u8],
    width: i32,
    lines: &mut dyn FnMut(&Control, &[u8]) -> usize,
) -> i32 {
    let kept = lines_at(control, words, width, lines);
    let mut narrower = width;
    while lines_at(control, words, narrower, lines) == kept {
        narrower -= sizes::NARROW;
        if narrower <= 0 {
            return width + sizes::NARROW;
        }
    }
    narrower + sizes::NARROW
}

/// The layout as `view` shows it, placed as KOTOR 2 places its summary in a
/// window of `window` physical pixels with Aspyr's text size `text`
/// (`draw::text_size`). `lines` counts the lines words wrap to in a
/// control as wide as it is, and `line` is a line's height. The panel's
/// left and top are where KOTOR 2 puts it: centred in the window.
///
/// KOTOR 2 loads the layout stretched, as every screen, and keeps only
/// each row's left from it. Everything else it works out from the text
/// size `s`. The first row stands 18 × `s` down, and each next row 37 × `s`
/// below the one before. A row's icon is 32 × `s` square. Its words stand a
/// pixel above the icon's top, 31 × `s` tall. Their width is the narrowest
/// that keeps their lines (`ideal`), measured from the layout's width. The
/// panel is as wide as the widest words, and never narrower than 225 × `s`.
/// Words that wrap are widened 30 × `s` at a time, while they are no wider
/// than 439 pixels. The received row's words (a datapad's here) make the
/// panel at least 450 × `s` wide. They are as tall as their lines, and the
/// next row stands that far below them when that is more than 36 pixels.
/// The panel is 30 × `s` × `s` wider than the widest words, plus the first
/// row's words' left. It ends 20 pixels under its OK. kjk draws no OK. A
/// datapad's prompt stands in its place, as tall as KOTOR 2 makes the OK,
/// and without a prompt the panel ends 20 pixels under the last row.
///
/// Returns the placed layout and where its OK's top would be (the rows'
/// end), down from the panel's top.
pub fn place(
    gui: &Gui,
    view: &SummaryView,
    window: (i32, i32),
    text: f32,
    lines: &mut dyn FnMut(&Control, &[u8]) -> usize,
    line: f32,
) -> (Gui, i32) {
    let across = window.0 as f32 / 800.0;
    let down = window.1 as f32 / 600.0;
    let find = |tag: &str| gui.controls.iter().find(|c| c.tag == tag).cloned();
    let unit = sizes::UNIT * text;
    let widen = sizes::WIDEN * unit;
    let mut rows: Vec<(Row, Control, Control, &[u8])> = view
        .rows
        .iter()
        .filter_map(|(row, words)| {
            let (icon, desc) = row.tags();
            Some((*row, find(icon)?, find(desc)?, words.as_slice()))
        })
        .collect();
    rows.sort_by_key(|(row, ..)| Row::ALL.iter().position(|each| *each == place_of(*row)));
    let mut y = ((sizes::FIRST.0 * text) * sizes::FIRST.1) as i32;
    let mut widest = (sizes::NARROWEST * unit) as i32;
    let mut controls = Vec::new();
    for (row, mut icon, mut desc, words) in rows {
        let stretched = |control: &mut Control| {
            control.extent.left = (control.extent.left as f32 * across) as i32;
            control.extent.width = (control.extent.width as f32 * across) as i32;
        };
        stretched(&mut icon);
        stretched(&mut desc);
        // A notice's words stand where the icon would, as wide as both.
        if row == Row::Notice {
            desc.extent.width += desc.extent.left - icon.extent.left;
            desc.extent.left = icon.extent.left;
        }
        let side = (sizes::ICON * text) as i32;
        icon.extent = Rect {
            top: y,
            width: side,
            height: side,
            ..icon.extent
        };
        let tall = (lines(&desc, words) as f32 * line).ceil() as i32;
        let received = place_of(row) == Row::Received;
        desc.extent.top = y - 1;
        desc.extent.height = if received {
            (tall as f32 * text) as i32
        } else {
            (sizes::WORDS * text) as i32
        };
        desc.extent.width = ideal(&desc, words, desc.extent.width, lines);
        widest = widest.max(desc.extent.width);
        if !words.is_empty() {
            if received {
                widest = widest.max((sizes::RECEIVED * unit) as i32);
                desc.extent.width = widest;
            } else {
                while widest <= sizes::WIDEST && lines_at(&desc, words, widest, lines) > 1 {
                    widest += widen as i32;
                    desc.extent.width = widest;
                }
            }
        }
        y = if received {
            y + if tall > sizes::TALL {
                tall
            } else {
                sizes::ROW as i32
            }
        } else {
            (y as f32 + sizes::ROW * text) as i32
        };
        if row != Row::Notice {
            controls.push(icon);
        }
        controls.push(desc);
    }
    let words_left =
        find(Row::Journal.tags().1).map_or(0, |words| (words.extent.left as f32 * across) as i32);
    let width = widest + (text * widen) as i32 + words_left;
    let prompt = view.prompt.as_ref().map_or(0, |_| {
        // KOTOR 2's OK: its stretched height, 10% taller for each step of
        // the text size past 1.
        find("BTN_OK").map_or((PROMPT_TALL * text) as i32, |ok| {
            let stretched = (ok.extent.height as f32 * down) as i32;
            (stretched as f32 * ((text - 1.0) * 0.1 + 1.0)) as i32
        })
    });
    let height = y + prompt + sizes::BOTTOM;
    let placed = Gui {
        extent: Rect {
            left: (window.0 - width) / 2,
            top: (window.1 - height) / 2,
            width,
            height,
        },
        controls,
        ..gui.clone()
    };
    (placed, y)
}

/// The rows' words' colour: K1's own layout's blue, which the skin's
/// recolouring of KOTOR 2's would turn to K1's yellow.
const WORDS: [f32; 3] = [0.0, 0.658_823_55, 0.980_392_16];

/// How the summary shows `control`: a row's words its own, in K1's blue; a
/// datapad's row its datapad's icon.
pub fn state(control: &Control, view: &SummaryView) -> ControlState {
    let words = view
        .rows
        .iter()
        .find(|(row, _)| row.tags().1 == control.tag)
        .map(|(_, words)| words.clone());
    let icon = view
        .rows
        .iter()
        .any(|(row, _)| *row == Row::Datapad && row.tags().0 == control.tag)
        .then(|| view.icon.clone())
        .flatten();
    ControlState {
        tint: words.as_ref().map(|_| WORDS),
        text: words,
        fill: icon,
        ..ControlState::default()
    }
}

/// The prompt's binding and words along the panel's bottom line, `rect`
/// its line: a pad's buttons as drawn (`icons`), else the key's name on a
/// cap; then its words in the rows' font and blue, `gap` after the key.
fn prompt(
    painter: &mut Painter,
    (label, words): &(KeyLabel, Vec<u8>),
    font: &str,
    (rect, gap): ([f32; 4], f32),
    out: &mut DrawList,
) {
    let [x, y, w, h] = rect;
    let wide = match &label.badge {
        Some(badge) => painter.badge_left(badge, [x, y, w, h], out),
        None => {
            let cap = crate::icons::key_texture(label.words.len());
            let wide = painter.badge_left(&cap, [x, y, w, h], out);
            painter.text(
                font,
                &label.words,
                [x, y, wide, h],
                align::CENTRE | align::MIDDLE,
                [1.0; 4],
                out,
            );
            wide
        }
    };
    let left = x + wide + gap;
    let [r, g, b] = WORDS;
    painter.text(
        font,
        words,
        [left, y, (w - wide - gap).max(1.0), h],
        align::LEFT | align::MIDDLE,
        [r, g, b, 1.0],
        out,
    );
}

/// `summaries` drawn in a window of `window` physical pixels with Aspyr's
/// text size `text`, each `gui` (read from `source`) placed as KOTOR 2
/// places it (`place`). KOTOR 2 shows one summary at a time. kjk may show
/// several at once, so they stand one under another, `MARGIN` apart, and
/// the column of them is centred in the window as KOTOR 2 centres one.
/// KOTOR 2's summary does not move in or out. Here each fades in
/// (`SummaryView::slide`) and fades out (`SummaryView::alpha`), and opens
/// or closes its gap in the column as it does.
pub fn draw(
    painter: &mut Painter,
    gui: &Gui,
    source: Source,
    summaries: &[SummaryView],
    window: (i32, i32),
    text: f32,
) -> DrawList {
    let mut out = DrawList {
        size: (window.0 as f32, window.1 as f32),
        ..DrawList::default()
    };
    painter.source = source;
    let line = control_line(painter, gui, source);
    let margin = MARGIN * text;
    let placed: Vec<((Gui, i32), f32)> = summaries
        .iter()
        .map(|view| {
            let mut lines = |control: &Control, words: &[u8]| {
                let font = control.text.as_ref().map_or("", |text| text.font.as_str());
                painter.textures.font(source, font).map_or(1, |(_, font)| {
                    wrap(&font, words, control.extent.width as f32).len()
                })
            };
            let placed = place(gui, view, window, text, &mut lines, line);
            let alpha = view.alpha.clamp(0.0, 1.0) * view.slide.clamp(0.0, 1.0);
            (placed, alpha)
        })
        .collect();
    let column: f32 = placed
        .iter()
        .map(|((gui, _), alpha)| (gui.extent.height as f32 + margin) * alpha)
        .sum::<f32>()
        - margin;
    let mut y = (window.1 as f32 - column) / 2.0;
    for (((mut placed, end), alpha), view) in placed.into_iter().zip(summaries) {
        placed.extent.top = y.round() as i32;
        let mut part = DrawList::default();
        painter.layout(&placed, &|control| state(control, view), &mut part);
        if let Some(said) = &view.prompt {
            // Where KOTOR 2's OK stands: under the rows, from the icons'
            // left to as far in from the panel's right.
            let left = placed.controls.iter().map(|c| c.extent.left).min();
            let font = placed
                .controls
                .iter()
                .find_map(|c| c.text.as_ref().map(|text| text.font.clone()));
            if let (Some(left), Some(font)) = (left, font) {
                let tall = (placed.extent.height - sizes::BOTTOM - end).max(1);
                let rect = [
                    (placed.extent.left + left) as f32,
                    (placed.extent.top + end) as f32,
                    (placed.extent.width - 2 * left) as f32,
                    tall as f32,
                ];
                prompt(painter, said, &font, (rect, PROMPT_GAP * text), &mut part);
            }
        }
        for mut quad in part.quads {
            quad.color[3] *= alpha;
            out.quads.push(quad);
        }
        // The panels under it close up as it goes.
        y += (placed.extent.height as f32 + margin) * alpha;
    }
    out
}

/// A line's height in the rows' words' font.
fn control_line(painter: &mut Painter, gui: &Gui, source: Source) -> f32 {
    gui.controls
        .iter()
        .find(|c| c.tag == Row::Journal.tags().1)
        .and_then(|c| c.text.as_ref())
        .and_then(|text| painter.textures.font(source, &text.font))
        .map_or(16.0, |(_, font)| font.line_height())
}
