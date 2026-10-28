//! KOTOR's cinematic conversation screen (`dialog`, KOTOR 2's `dialog_p`
//! with the skin), drawn under the screens and not one of them: what it
//! takes is the conversation's (`launcher::dialogue_view`).
//!
//! - Black bars above and below, across the whole window: the top one as
//!   tall as `LBL_MESSAGE`, the bottom one from the layout's root down, of
//!   the window's height as of the layout's.
//! - The line and the replies run across the window, the layout's root's
//!   left kept from each edge (as the games draw them in a wide window,
//!   xoreos's reading), not in the layout's 544 wide column; the line no
//!   wider than `READING`, centred (the owner's call: easier to read).
//! - While the line plays, it is in the bottom bar, centred from its top,
//!   and no reply shows; once it has played (the player's turn) it moves
//!   to the top bar, centred down to its bottom, and the replies show in
//!   the bottom bar, numbered (reone's reading of the game).
//! - A bar grows to what it holds (a long line, many replies) up to half
//!   the window, so nothing is cut or scrolled.
//! - Its words are drawn at `SCALE` of the other screens' size (the
//!   owner's call, 2026-10-04, after KOTOR 2 at 1280 × 1024: a line 1.9 %
//!   of the window's height).
//! - The words are the campaign's base colour, the highlighted reply its
//!   highlight (KOTOR 1's blue and yellow); in a session of more than one
//!   seat, the replies keep room at their right for the names of the
//!   seats pointing at each, in green, and a guest sees the replies only
//!   the host takes (the story's) faded.
//! - While the player may leave the conversation (R6), a Leave row stands
//!   under the replies, its key named, faded while the conversation is
//!   locked.
//! - The host's request card (R5: each guest asking the host in, and the
//!   keys that answer the first) stands in the top bar's bottom right
//!   corner, and there over the HUD while the host walks (`Dialog::card`).

use fmt_odyssey::gui::align;
use fmt_odyssey::gui::{Control, Gui, Rect};

use crate::assets::GuiAssets;
use crate::draw::{DrawList, Quad, Textures, fill, window_to_layout};
use crate::paint::{ControlState, ListView, Painter, Row, row_height, wrap};
use crate::skin::Source;
use crate::strings;
use crate::vm::{DialogView, RequestsView};

pub const LAYOUT: &str = "dialog";
/// The words' size beside the other screens': the frame this much larger
/// than the window's shaped layout frame (`hud::shaped`; the bars kept to
/// the window).
pub const SCALE: f32 = 0.5;
const MESSAGE: &str = "LBL_MESSAGE";
const REPLIES: &str = "LB_REPLIES";
/// The widest the line runs (about 80 characters of the dialogue font),
/// centred: across a wide window it is hard to read in one line.
const READING: i32 = 680;
/// Room between a bar's edge and its words.
const PAD: i32 = 8;
/// The room the seats' marks keep at the replies' right.
const MARKS: i32 = 128;
const MARKED: [f32; 4] = [0.55, 1.0, 0.55, 1.0];
/// Centred across; from the top, or down to the bottom.
const CENTRE_TOP: i32 = align::CENTRE | align::TOP;
const CENTRE_BOTTOM: i32 = align::CENTRE | align::BOTTOM;
const RIGHT_TOP: i32 = align::RIGHT | align::TOP;

/// The screen's layout as the screens draw it (the skin's).
pub struct Dialog {
    pub gui: Gui,
    pub source: Source,
}

/// The layout placed for a line in a window: a frame of the window's
/// shape, 480 tall (or 640 wide), holding the line and the replies.
#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    pub gui: Gui,
    /// The top and bottom bars (left, top, width, height).
    pub bars: [[f32; 4]; 2],
    /// Each reply's top and bottom, in order.
    pub rows: Vec<[f32; 2]>,
    /// The replies' left and right.
    pub across: [f32; 2],
    /// The Leave row's top and bottom, under the replies, when shown.
    pub leave: Option<[f32; 2]>,
}

/// The host's request card (R5): its lines (each ask, then the keys or why
/// they wait), where it stands, its words' font, and how many rows each
/// line wraps to.
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub lines: Vec<Vec<u8>>,
    pub rect: [f32; 4],
    font: String,
    pitch: f32,
    wrapped: Vec<usize>,
    /// The conversation's frame it stands in.
    frame: (f32, f32),
}

impl Dialog {
    /// `None` when the screens' layout does not read.
    pub fn read(assets: &GuiAssets) -> Option<Dialog> {
        let (gui, source) = assets.screen_layout(LAYOUT).ok()?;
        Some(Dialog { gui, source })
    }

    /// The layout placed for `view` in a window of `window` pixels; `None`
    /// when it has no message label or no reply list.
    pub fn place(
        &self,
        textures: &mut dyn Textures,
        view: &DialogView,
        window: (f32, f32),
    ) -> Option<Placed> {
        let control = |tag: &str| self.gui.controls.iter().find(|c| c.tag == tag).cloned();
        let (mut message, mut list) = (control(MESSAGE)?, control(REPLIES)?);
        let mut proto = list.proto.as_deref()?.clone();
        let (_, frame_height) = self.gui.frame();
        let (wide, tall) = self.frame(window);
        let left = self.gui.extent.left;
        let width = wide - 2 * left;
        proto.extent.width += width - list.extent.width;
        let reading = width.min(READING);
        let font = message
            .text
            .as_ref()
            .and_then(|text| textures.font(self.source, &text.font));
        let said = font.map_or(0, |(_, font)| {
            let lines = wrap(&font, &view.said, reading as f32).len();
            (lines as f32 * font.line_height()).ceil() as i32
        });
        if view.marks.is_some() {
            proto.extent.width -= MARKS;
        }
        // The replies, then the Leave row under them.
        let answering = !view.replies.is_empty();
        let leave = leave_row(view).filter(|_| answering);
        let rows: Vec<i32> = view
            .replies
            .iter()
            .chain(&leave)
            .map(|reply| row_height(textures, self.source, &proto, true, reply) + list.padding)
            .collect();
        let most = tall / 2;
        let top = larger(message.extent.height);
        let top = if answering {
            top.max(said + 2 * PAD).min(most)
        } else {
            top
        };
        let below = if answering {
            proto.extent.top + rows.iter().sum::<i32>() + PAD
        } else {
            said + 2 * PAD
        };
        let bottom = larger(frame_height - self.gui.extent.top)
            .max(below)
            .min(most);
        let floor = tall - bottom;
        let (message_top, message_height, alignment) = if answering {
            (0, top - PAD, CENTRE_BOTTOM)
        } else {
            (floor + PAD, bottom - 2 * PAD, CENTRE_TOP)
        };
        message.extent = Rect {
            left: (wide - reading) / 2,
            top: message_top,
            width: reading,
            height: message_height,
        };
        if let Some(text) = &mut message.text {
            text.alignment = alignment;
        }
        let (dx, dy) = (left - list.extent.left, floor - list.extent.top);
        list.extent = Rect {
            left,
            top: floor,
            width,
            height: bottom,
        };
        proto.extent.left += dx;
        proto.extent.top += dy;
        if let Some(bar) = &mut list.scrollbar {
            bar.extent.left += dx;
            bar.extent.top += dy;
        }
        let mut spans = Vec::with_capacity(rows.len());
        let mut at = proto.extent.top as f32;
        for height in rows {
            spans.push([at, at + height as f32]);
            at += height as f32;
        }
        let leave = leave.and_then(|_| spans.pop());
        let across = [
            proto.extent.left as f32,
            (proto.extent.left + proto.extent.width) as f32,
        ];
        list.proto = Some(Box::new(proto));
        let (wide_f, tall_f) = (wide as f32, tall as f32);
        Some(Placed {
            gui: Gui {
                extent: Rect {
                    left: 0,
                    top: 0,
                    width: wide,
                    height: tall,
                },
                border: None,
                controls: vec![message, list],
                ..self.gui.clone()
            },
            bars: [
                [0.0, 0.0, wide_f, top as f32],
                [0.0, floor as f32, wide_f, tall_f - floor as f32],
            ],
            rows: spans,
            across,
            leave,
        })
    }

    /// The frame for a window of `window` pixels: the window's shape at the
    /// layout's height, made larger by `SCALE`.
    fn frame(&self, window: (f32, f32)) -> (i32, i32) {
        let (wide, tall) = crate::screens::hud::shaped(&self.gui, window);
        (larger(wide), larger(tall))
    }

    /// The host's request card for `view` in a window of `window` pixels
    /// (R5): each ask on a line of the line's font, then the keys (or why
    /// they wait), round them a black backing, at the right as far in as
    /// the replies, no wider than half their width (longer lines wrap).
    /// While the host walks it stands in the top bar's bottom right corner
    /// (the bar as it stands while a line plays), clear of the HUD's
    /// buttons; while the host talks (`under`, the conversation's screen
    /// placed), under the top bar, clear of the host's line. `None` when
    /// the layout has no message label or its font does not read.
    pub fn card(
        &self,
        textures: &mut dyn Textures,
        view: &RequestsView,
        window: (f32, f32),
        under: Option<&Placed>,
    ) -> Option<Card> {
        let message = self.gui.find(MESSAGE)?;
        let font = message.text.as_ref()?.font.clone();
        let bar = larger(message.extent.height) as f32;
        let (_, measured) = textures.font(self.source, &font)?;
        let keys = match view.waiting {
            Some(why) => strings::bytes(why),
            None => strings::answers(&view.accept, &view.end),
        };
        let lines: Vec<Vec<u8>> = view
            .asks
            .iter()
            .map(|(name, npc)| strings::asks(name, npc))
            .chain([keys])
            .collect();
        let (wide, tall) = self.frame(window);
        let pad = PAD as f32;
        let most = (wide - 2 * self.gui.extent.left) as f32 / 2.0 - 2.0 * pad;
        let widest = lines
            .iter()
            .map(|line| measured.measure(line))
            .fold(0.0, f32::max)
            .ceil()
            .min(most);
        let wrapped: Vec<usize> = lines
            .iter()
            .map(|line| wrap(&measured, line, widest).len().max(1))
            .collect();
        let pitch = measured.line_height();
        let rows: usize = wrapped.iter().sum();
        let (width, height) = (widest + 2.0 * pad, (rows as f32 * pitch).ceil() + 2.0 * pad);
        let right = (wide - self.gui.extent.left) as f32;
        let top = match under {
            Some(placed) => placed.bars[0][3] + pad,
            None => (bar - pad - height).max(pad),
        };
        Some(Card {
            rect: [right - width, top, width, height],
            lines,
            font,
            pitch,
            wrapped,
            frame: (wide as f32, tall as f32),
        })
    }
}

/// A layout's length made larger by `SCALE`.
fn larger(side: i32) -> i32 {
    (side as f32 / SCALE).round() as i32
}

/// The Leave row's words, where the view offers one.
fn leave_row(view: &DialogView) -> Option<Vec<u8>> {
    view.leave.map(|_| strings::leave(&view.leave_key))
}

/// The card's backing: black, a little clear.
const CARD_BACKING: [f32; 4] = [0.0, 0.0, 0.0, 0.85];

impl Card {
    /// The card drawn in its frame (the conversation's, filling the window):
    /// its backing, the asks in the campaign's base colour, the keys in its
    /// highlight.
    pub fn draw(&self, source: Source, painter: &mut Painter, out: &mut DrawList) {
        out.size = self.frame;
        out.fill = true;
        out.quads.push(Quad::solid(self.rect, CARD_BACKING));
        painter.source = source;
        let [x, y, width, _] = self.rect;
        let pad = PAD as f32;
        let keys = self.lines.len() - 1;
        let mut top = y + pad;
        for (nth, (line, rows)) in self.lines.iter().zip(&self.wrapped).enumerate() {
            let [r, g, b] = if nth == keys {
                painter.palette.hilight
            } else {
                painter.palette.base
            };
            let height = *rows as f32 * self.pitch;
            let rect = [x + pad, top, width - 2.0 * pad, height];
            let colour = [r, g, b, 1.0];
            painter.text(
                &self.font,
                line,
                rect,
                align::LEFT | align::TOP,
                colour,
                out,
            );
            top += height;
        }
    }
}

/// How the screen shows `control`: the line in `words`' colour, the
/// replies only on the player's turn, the highlighted one lit, the Leave
/// row under them (faded while the conversation is locked).
pub fn state(control: &Control, view: &DialogView, words: [f32; 3]) -> ControlState {
    match control.tag.as_str() {
        MESSAGE => ControlState {
            text: Some(view.said.clone()),
            tint: Some(words),
            ..ControlState::default()
        },
        REPLIES => ControlState {
            hidden: view.replies.is_empty(),
            list: Some(ListView {
                rows: view
                    .replies
                    .iter()
                    .enumerate()
                    .map(|(nth, reply)| Row {
                        text: reply.clone(),
                        dimmed: view.host_only.get(nth).copied().unwrap_or(false),
                        ..Row::default()
                    })
                    .chain(leave_row(view).map(|text| Row {
                        text,
                        dimmed: view.leave == Some(false),
                        ..Row::default()
                    }))
                    .collect(),
                focused: Some(usize::from(view.focus)),
                fit: true,
                pick: true,
                ..ListView::default()
            }),
            ..ControlState::default()
        },
        _ => ControlState::default(),
    }
}

impl Placed {
    /// The screen drawn: the bars, the line, the replies and the seats'
    /// marks.
    pub fn draw(
        &self,
        view: &DialogView,
        source: Source,
        painter: &mut Painter,
        out: &mut DrawList,
    ) {
        out.quads
            .extend(self.bars.map(|bar| Quad::solid(bar, [0.0, 0.0, 0.0, 1.0])));
        let words = painter.palette.base;
        painter.source = source;
        painter.layout(&self.gui, &|control| state(control, view, words), out);
        let font = self
            .gui
            .find(REPLIES)
            .and_then(|list| list.proto.as_ref()?.text.as_ref())
            .map(|text| text.font.clone());
        let (Some(font), Some(marks)) = (font, &view.marks) else {
            return;
        };
        for (nth, names) in marks {
            if let Some([top, bottom]) = self.rows.get(usize::from(*nth)).copied() {
                let rect = [self.across[1], top, MARKS as f32, bottom - top];
                painter.text(&font, names, rect, RIGHT_TOP, MARKED, out);
            }
        }
    }

    /// How far down the frame `point` (window pixels) is in a window of
    /// `window`, where it is across the replies.
    fn down_at(&self, window: (f32, f32), point: [f32; 2]) -> Option<f32> {
        let (width, height) = self.gui.frame();
        let size = (width as f32, height as f32);
        let [x, y] = window_to_layout(fill(window, size), size, point)?;
        (self.across[0]..self.across[1]).contains(&x).then_some(y)
    }

    /// The reply under `point` (window pixels) in a window of `window`.
    pub fn row_at(&self, window: (f32, f32), point: [f32; 2]) -> Option<u8> {
        let y = self.down_at(window, point)?;
        let nth = self
            .rows
            .iter()
            .position(|[top, bottom]| (*top..*bottom).contains(&y))?;
        u8::try_from(nth).ok()
    }

    /// Whether the Leave row is under `point` (window pixels).
    pub fn leave_at(&self, window: (f32, f32), point: [f32; 2]) -> bool {
        let row = self.leave.zip(self.down_at(window, point));
        row.is_some_and(|([top, bottom], y)| (top..bottom).contains(&y))
    }
}
