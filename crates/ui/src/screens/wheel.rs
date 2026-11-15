//! The power wheel, drawn over the HUD and under the screens while the
//! player holds its key. It is not a screen, and nothing on it takes the
//! focus. Every part is a control of KOTOR 2's own HUD layout (`mipc28x6`),
//! cloned, moved and resized:
//!
//! - Each entry is an action slot's frame (`BTN_ACTION0`) with the entry's
//!   icon in the slot's icon label (`LBL_ACTION0`). The entries stand on a
//!   ring round the middle of the frame. Entry 0 is at the top and the
//!   others follow clockwise.
//! - The highlighted entry is drawn 1.2 times as large, with the frame's
//!   focused border.
//! - When the highlighted entry is a kind of item and the character carries
//!   two or more different items of that kind, those items stand on an arc
//!   outside the ring. The arc item that letting go uses has the focused
//!   border.
//! - The centre label names what letting go uses, in the HUD's own label
//!   look (`hud::label`). Under it stands the wheel's own key.
//! - When the list has more entries than the ring shows, KOTOR 2's action
//!   arrows (`BTN_ACTIONUP0` and `BTN_ACTIONDOWN0`) stand above the label
//!   and below the key, to say the ring turns.
//!
//! All sizes are fractions of the frame's height, called H here.

use fmt_odyssey::gui::{Control, Gui, align};

use crate::draw::DrawList;
use crate::paint::{ControlState, Painter, rect_of};
use crate::screens::hud::{BADGE_TALL, label, ringed};
use crate::vm::{KeyLabel, WheelEntry, WheelView};

/// The ring's radius, to each entry's centre (H).
pub const RING: f32 = 0.20;
/// An entry's side (H).
pub const ENTRY: f32 = 0.075;
/// How much larger the highlighted entry is drawn than the others.
pub const LIT_SIZE: f32 = 1.2;
/// The pointer must be at least this long to highlight an entry (H).
pub const DEAD_IN: f32 = 0.06;
/// A highlighted entry stays highlighted while the pointer is at least this
/// long (H).
pub const DEAD_OUT: f32 = 0.05;
/// The arc's radius, to each arc item's centre (H).
pub const ARC: f32 = 0.33;
/// An arc item's side (H).
pub const ARC_ENTRY: f32 = 0.065;
/// The angle between two arc items (degrees).
pub const ARC_STEP: f32 = 13.0;
/// The most arc items shown at once.
pub const ARC_SHOWN: usize = 7;
/// The pointer reaches the arc beyond this length (H).
pub const ARC_IN: f32 = 0.30;
/// The pointer leaves the arc inside this length (H).
pub const ARC_OUT: f32 = 0.27;
/// The pointer's greatest length (H).
pub const REACH: f32 = 0.38;
/// The most entries the ring shows at once.
pub const RING_SHOWN: usize = 10;

/// What the centre label says when the wheel has nothing to offer. No line
/// of KOTOR 2 fits, so these are kjk's own words.
const EMPTY: &[u8] = b"Nothing to use";
/// The gap between the centre label and the key under it (layout pixels).
const KEY_GAP: f32 = 4.0;
/// The gap between an arrow and the label or key it stands by (layout
/// pixels).
const ARROW_GAP: f32 = 2.0;
/// The height of the key's words under the label (layout pixels).
const KEY_TALL: f32 = 18.0;

/// The angle of ring slot `slot` on a ring of `shown` slots, in degrees
/// clockwise from straight up.
pub fn ring_angle(slot: usize, shown: usize) -> f32 {
    slot as f32 * 360.0 / shown.max(1) as f32
}

/// The ring slot that shows list entry `at`, when the ring shows the list
/// of `entries` from `first` on. None when the entry is not on the ring.
pub fn ring_slot(at: usize, first: usize, entries: usize) -> Option<usize> {
    if entries == 0 {
        return None;
    }
    let slot = (at % entries + entries - first % entries) % entries;
    (slot < entries.min(RING_SHOWN)).then_some(slot)
}

/// The first arc item shown, when the arc of `items` items shows
/// `ARC_SHOWN` of them and the item at `arc_at` should stay in view, in
/// the middle when it can.
pub fn arc_window(arc_at: usize, items: usize) -> usize {
    arc_at
        .saturating_sub(ARC_SHOWN / 2)
        .min(items.saturating_sub(ARC_SHOWN))
}

/// The angle of the arc item at place `place` of the `shown` arc items,
/// when the arc stands beyond the entry at angle `entry` (degrees).
pub fn arc_angle(entry: f32, place: usize, shown: usize) -> f32 {
    entry + (place as f32 - (shown as f32 - 1.0) / 2.0) * ARC_STEP
}

/// The point `radius` from `centre` at `degrees` clockwise from straight
/// up, in frame pixels.
fn point([x, y]: [f32; 2], radius: f32, degrees: f32) -> [f32; 2] {
    let turn = degrees.to_radians();
    [x + radius * turn.sin(), y - radius * turn.cos()]
}

/// What the centre label says of `entry`: its name, and then its count as
/// "x3" two spaces after it.
fn named(entry: &WheelEntry) -> Vec<u8> {
    let mut words = entry.name.clone();
    if let Some(count) = entry.count {
        words.extend(format!("  x{count}").into_bytes());
    }
    words
}

/// `control` moved to `rect` (left, top, width, height).
fn placed(control: &Control, rect: [f32; 4]) -> Control {
    let mut control = control.clone();
    control.extent.left = rect[0].round() as i32;
    control.extent.top = rect[1].round() as i32;
    control.extent.width = rect[2].round() as i32;
    control.extent.height = rect[3].round() as i32;
    control
}

/// The two controls an entry is drawn with: an action slot's frame and its
/// icon label.
struct Slot<'a> {
    frame: &'a Control,
    icon: &'a Control,
}

impl Slot<'_> {
    /// Draws `entry` as a square of `side` pixels centred on `at`, its
    /// frame focused when `focused`, and its count at its bottom right.
    fn draw(
        &self,
        entry: &WheelEntry,
        at: [f32; 2],
        side: f32,
        focused: bool,
        painter: &mut Painter,
        out: &mut DrawList,
    ) {
        let [x, y] = [at[0] - side / 2.0, at[1] - side / 2.0];
        let frame = ControlState {
            focused,
            selected: entry.lit,
            dimmed: entry.dim,
            ..ControlState::default()
        };
        painter.control(&placed(self.frame, [x, y, side, side]), &frame, out);
        // The icon's width is the same fraction of its frame's width as the
        // HUD's own icon label is of its slot's frame.
        let share = (rect_of(self.icon)[2] / rect_of(self.frame)[2].max(1.0)).min(1.0);
        let inner = side * share;
        let rect = [at[0] - inner / 2.0, at[1] - inner / 2.0, inner, inner];
        let icon = ControlState {
            fill: Some(entry.icon.clone()),
            text: Some(Vec::new()),
            dimmed: entry.dim,
            ..ControlState::default()
        };
        painter.control(&placed(self.icon, rect), &icon, out);
        if let Some(count) = entry.count {
            let base = painter.palette.base;
            let words = count.to_string();
            let rect = [x, y + side - 20.0, side - 4.0, 18.0];
            ringed(
                painter,
                words.as_bytes(),
                rect,
                align::RIGHT | align::BOTTOM,
                base,
                out,
            );
        }
    }
}

/// Draws the wheel's key with its top centred on `top`: a controller's
/// buttons, or the key's words. Returns the bottom of what it drew.
fn key_hint(key: &KeyLabel, [x, top]: [f32; 2], painter: &mut Painter, out: &mut DrawList) -> f32 {
    match key {
        KeyLabel {
            badge: Some(badge), ..
        } => {
            let wide = BADGE_TALL * 6.0;
            painter.badge(badge, [x - wide / 2.0, top, wide, BADGE_TALL], out);
            top + BADGE_TALL
        }
        KeyLabel { words, .. } if !words.is_empty() => {
            let hilight = painter.palette.hilight;
            let rect = [x - 100.0, top, 200.0, KEY_TALL];
            ringed(
                painter,
                words,
                rect,
                align::CENTRE | align::TOP,
                hilight,
                out,
            );
            top + KEY_TALL
        }
        _ => top,
    }
}

/// Draws KOTOR 2's action arrows that say the ring turns: the arrow that
/// turns it back with its bottom at `above`, and the arrow that turns it on
/// with its top at `below`, both centred on `x`. A controller's bumper
/// stands left of each arrow.
fn arrows(
    gui: &Gui,
    wheel: &WheelView,
    x: f32,
    (above, below): (f32, f32),
    painter: &mut Painter,
    out: &mut DrawList,
) {
    let tags = ["BTN_ACTIONUP0", "BTN_ACTIONDOWN0"];
    for (nth, tag) in tags.into_iter().enumerate() {
        let Some(arrow) = gui.find(tag) else {
            continue;
        };
        let [_, _, w, h] = rect_of(arrow);
        let top = if nth == 0 {
            above - ARROW_GAP - h
        } else {
            below + ARROW_GAP
        };
        let left = x - w / 2.0;
        let from = out.quads.len();
        painter.control(
            &placed(arrow, [left, top, w, h]),
            &ControlState::default(),
            out,
        );
        if nth == 1 {
            // Both of KOTOR 2's arrows draw the same picture, which points
            // up. The arrow that turns the ring on is turned upside down,
            // as `Painter::scrollbar` turns its lower arrow.
            for quad in &mut out.quads[from..] {
                let [u0, v0, u1, v1] = quad.uv;
                quad.uv = [u0, v1, u1, v0];
                quad.rect[1] = 2.0 * top + h - quad.rect[1] - quad.rect[3];
            }
        }
        if let Some(badge) = wheel.bumpers.as_ref().map(|bumpers| &bumpers[nth]) {
            // Measured at its own shape first, then moved flush to the arrow.
            let tall = [top + (h - BADGE_TALL) / 2.0, BADGE_TALL];
            let wide = painter.badge_left(
                badge,
                [0.0, tall[0], BADGE_TALL * 3.0, tall[1]],
                &mut DrawList::default(),
            );
            let area = [left - ARROW_GAP - wide, tall[0], wide, tall[1]];
            painter.badge_left(badge, area, out);
        }
    }
}

/// The wheel in the frame of the HUD's layout `gui`, as placed for the
/// window.
pub fn draw(gui: &Gui, wheel: &WheelView, painter: &mut Painter) -> DrawList {
    let (width, height) = gui.frame();
    let mut out = DrawList {
        size: (width as f32, height as f32),
        ..DrawList::default()
    };
    let tall = height as f32;
    let centre = [width as f32 / 2.0, tall / 2.0];
    let entries = wheel.entries.len();
    let shown = entries.min(RING_SHOWN);
    let lit = wheel.at.and_then(|at| ring_slot(at, wheel.first, entries));
    if let (Some(frame), Some(icon)) = (gui.find("BTN_ACTION0"), gui.find("LBL_ACTION0")) {
        let slot = Slot { frame, icon };
        let on_ring = |at: usize| {
            let entry = &wheel.entries[(wheel.first + at) % entries];
            (entry, point(centre, RING * tall, ring_angle(at, shown)))
        };
        for at in (0..shown).filter(|at| Some(*at) != lit) {
            let (entry, place) = on_ring(at);
            slot.draw(entry, place, ENTRY * tall, false, painter, &mut out);
        }
        if let Some(at) = lit {
            let (entry, place) = on_ring(at);
            let side = ENTRY * LIT_SIZE * tall;
            slot.draw(entry, place, side, true, painter, &mut out);
            let items = wheel.arc.len();
            let from = arc_window(wheel.arc_at.unwrap_or(0), items);
            let arc_shown = items.min(ARC_SHOWN);
            for place in 0..arc_shown {
                let angle = arc_angle(ring_angle(at, shown), place, arc_shown);
                let item = from + place;
                let focused = wheel.arc_at == Some(item);
                let at = point(centre, ARC * tall, angle);
                slot.draw(
                    &wheel.arc[item],
                    at,
                    ARC_ENTRY * tall,
                    focused,
                    painter,
                    &mut out,
                );
            }
        }
    }
    let words = if entries == 0 {
        Some(EMPTY.to_vec())
    } else if wheel.arc.is_empty() {
        wheel.at.and_then(|at| wheel.entries.get(at)).map(named)
    } else {
        wheel.arc_at.and_then(|at| wheel.arc.get(at)).map(named)
    };
    let centred = |[wide, high]: [f32; 2]| [centre[0] - wide / 2.0, centre[1] - high / 2.0];
    let said = words.and_then(|words| label(gui, &words, &centred, painter, &mut out));
    let (above, under) = said.map_or((centre[1], centre[1]), |[_, top, _, high]| {
        (top, top + high)
    });
    let below = key_hint(&wheel.key, [centre[0], under + KEY_GAP], painter, &mut out);
    if entries > RING_SHOWN && wheel.arc.is_empty() {
        arrows(gui, wheel, centre[0], (above, below), painter, &mut out);
    }
    out
}
