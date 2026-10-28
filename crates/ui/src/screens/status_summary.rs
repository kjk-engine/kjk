//! KOTOR's status summary (`statussummary`, KOTOR 2's `statussummary_p`
//! with the skin): what a conversation or a script told the party (a
//! journal entry added, XP or credits received, light or dark side points,
//! items received or lost, KOTOR 2's influence gained or lost and its most
//! Force points raised or lowered), each a row
//! of the layout, its icon and words; a row its layout lacks (K1's has no
//! influence) is not shown; and ours alike, words alone in the rows' place
//! (a game saved).
//! KOTOR builds the panel from the rows that apply, in the layout's order,
//! each as tall as its words, and pauses the game under it until OK. In
//! real time it pauses nothing: it stands at the window's right edge under
//! the minimap without its OK, slides in, and fades a few seconds later
//! (`kjk_game::progress`). Drawn over the HUD at the window's own pixels,
//! as the HUD is; not a screen.

use fmt_odyssey::gui::{Control, Gui};

use crate::draw::DrawList;
use crate::paint::{ControlState, Painter, wrap};
use crate::skin::Source;
use crate::vm::SummaryView;

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
            Row::Received => ("LBL_RECEIVED", "LBL_RECEIVED_DESC"),
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
            | Row::Notice => {
                return None;
            }
        })
    }
}

/// Between rows, as the layout spaces them (pixels).
const GAP: i32 = 5;
/// Between the panel and the window's right edge, and between panels.
const MARGIN: f32 = 12.0;

/// The layout as `view` shows it: only its rows, stacked down from the
/// layout's first row's place (whichever rows show), each as tall as its
/// icon or its words (as many `lines` as they wrap to, `line` pixels
/// each), the panel closing under the last as far below it; no OK.
pub fn place(
    gui: &Gui,
    view: &SummaryView,
    lines: &mut dyn FnMut(&Control, &[u8]) -> usize,
    line: f32,
) -> Gui {
    let find = |tag: &str| gui.controls.iter().find(|c| c.tag == tag).cloned();
    let mut rows: Vec<(Row, Control, Control, &[u8])> = view
        .rows
        .iter()
        .filter_map(|(row, words)| {
            let (icon, desc) = row.tags();
            Some((*row, find(icon)?, find(desc)?, words.as_slice()))
        })
        .collect();
    rows.sort_by_key(|(_, icon, ..)| icon.extent.top);
    let first = Row::ALL
        .iter()
        .filter_map(|row| find(row.tags().0))
        .map(|icon| icon.extent.top)
        .min()
        .unwrap_or(GAP);
    let mut y = first;
    let mut controls = Vec::new();
    for (row, mut icon, mut desc, words) in rows {
        // A notice's words stand where the icon would, as wide as both.
        if row == Row::Notice {
            desc.extent.width += desc.extent.left - icon.extent.left;
            desc.extent.left = icon.extent.left;
        }
        // As tall as its words, never less than its icon.
        let shown = (lines(&desc, words) as f32 * line).ceil() as i32;
        let height = shown.max(icon.extent.height);
        // The words stand as the layout sets them against the icon.
        let offset = desc.extent.top - icon.extent.top;
        icon.extent.top = y;
        desc.extent.top = y + offset;
        desc.extent.height = height;
        y += icon.extent.height.max(offset + height) + GAP;
        if row != Row::Notice {
            controls.push(icon);
        }
        controls.push(desc);
    }
    Gui {
        extent: fmt_odyssey::gui::Rect {
            height: y - GAP + first,
            ..gui.extent
        },
        controls,
        ..gui.clone()
    }
}

/// The rows' words' colour: K1's own layout's blue, which the skin's
/// recolouring of KOTOR 2's would turn to K1's yellow.
const WORDS: [f32; 3] = [0.0, 0.658_823_55, 0.980_392_16];

/// How the summary shows `control`: a row's words its own, in K1's blue.
pub fn state(control: &Control, view: &SummaryView) -> ControlState {
    let words = view
        .rows
        .iter()
        .find(|(row, _)| row.tags().1 == control.tag)
        .map(|(_, words)| words.clone());
    ControlState {
        tint: words.as_ref().map(|_| WORDS),
        text: words,
        ..ControlState::default()
    }
}

/// `summaries` for a window of `window` pixels, the first's top at `top`,
/// each `gui` (read from `source`) placed at the right edge.
pub fn draw(
    painter: &mut Painter,
    gui: &Gui,
    source: Source,
    summaries: &[SummaryView],
    window: (f32, f32),
    top: f32,
) -> DrawList {
    let mut out = DrawList {
        size: window,
        ..DrawList::default()
    };
    painter.source = source;
    let mut y = top;
    let line = control_line(painter, gui, source);
    for view in summaries {
        let mut lines = |control: &Control, words: &[u8]| {
            let font = control.text.as_ref().map_or("", |text| text.font.as_str());
            painter.textures.font(source, font).map_or(1, |(_, font)| {
                wrap(&font, words, control.extent.width as f32).len()
            })
        };
        let mut placed = place(gui, view, &mut lines, line);
        let width = placed.extent.width as f32;
        // Eased out: fast in, settling.
        let slide = 1.0 - (1.0 - view.slide.clamp(0.0, 1.0)).powi(3);
        let left = window.0 - MARGIN - width + (1.0 - slide) * (width + MARGIN);
        placed.extent.left = left.round() as i32;
        placed.extent.top = y.round() as i32;
        let mut part = DrawList::default();
        painter.layout(&placed, &|control| state(control, view), &mut part);
        let alpha = view.alpha.clamp(0.0, 1.0);
        for mut quad in part.quads {
            quad.color[3] *= alpha;
            out.quads.push(quad);
        }
        // The panels under it close up as it goes.
        y += (placed.extent.height as f32 + MARGIN) * alpha;
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
