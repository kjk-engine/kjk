//! The conversation screen: the line below while it plays, above with the
//! replies below on the player's turn, bars across the window that grow to
//! what they hold, the pointer finding replies.

use fmt_odyssey::gui::{Control, Gui, Kind, Rect};

use super::menus::root;
use super::paint::{Fake, button};
use super::{Check, check, expect, near};
use crate::draw::{DrawList, Textures};
use crate::intent::Intent;
use crate::paint::{Painter, Palette, wrap};
use crate::screen::Screen;
use crate::screens::computer::Computer;
use crate::screens::dialog::{Dialog, Placed, SCALE, state};
use crate::skin::Source;
use crate::strings;
use crate::vm::{ComputerView, DialogView, RequestsView, ViewModel};

/// A word of the test font's four glyphs.
const WORD: &[u8] = b"\x00\x01\x02\x03";
const WINDOW: (f32, f32) = (1280.0, 720.0);
const NARROW: (f32, f32) = (960.0, 720.0);
/// The frame in a 16:9 window: 853 × 480, made larger by the screen's
/// scale.
fn frame() -> (i32, i32) {
    let larger = |side: f32| (side / SCALE).round() as i32;
    (larger(853.0), larger(480.0))
}

/// K2's `dialog_p`'s shape: the root at (48, 378), 544 × 100; the label as
/// large, the list a little shorter, its rows 24 in and 8 down.
fn layout() -> Gui {
    let mut message = button("LBL_MESSAGE", [0, 0, 544, 100], b"");
    message.kind = Kind::Label;
    let mut proto = button("PROTOITEM", [24, 8, 512, 19], b"");
    proto.kind = Kind::Label;
    let list = Control {
        kind: Kind::ListBox,
        tag: "LB_REPLIES".into(),
        extent: Rect {
            left: 0,
            top: 0,
            width: 544,
            height: 98,
        },
        proto: Some(Box::new(proto)),
        ..Control::default()
    };
    Gui {
        extent: Rect {
            left: 48,
            top: 378,
            width: 544,
            height: 100,
        },
        ..root(544, 100, vec![message, list])
    }
}

/// `tag`'s place, none (zeros) when it is not placed.
fn rect(placed: &Placed, tag: &str) -> [i32; 4] {
    placed.gui.find(tag).map_or([0; 4], |control| {
        let extent = control.extent;
        [extent.left, extent.top, extent.width, extent.height]
    })
}

fn alignment(placed: &Placed) -> i32 {
    placed
        .gui
        .find("LBL_MESSAGE")
        .and_then(|control| control.text.as_ref())
        .map_or(0, |text| text.alignment)
}

fn words(count: usize) -> Vec<u8> {
    vec![WORD; count].join(&b' ')
}

pub(super) fn all() -> Vec<Check> {
    let mut fake = Fake::new();
    let (_, font) = fake.font(Source::Skin, "grid").expect("the test font");
    let pitch = font.line_height();
    let dialog = Dialog {
        gui: layout(),
        source: Source::Skin,
    };
    let view = |replies: Vec<Vec<u8>>| DialogView {
        said: words(4),
        replies,
        ..DialogView::default()
    };
    let playing = view(Vec::new());
    let turn = view(vec![words(2), words(3)]);
    let many = view(vec![words(30); 40]);
    let leaving = |leave: Option<bool>| DialogView {
        leave,
        leave_key: b"L".to_vec(),
        ..turn.clone()
    };
    let leaves = [Some(true), Some(false), None].map(leaving);
    // Names and keys of the test font's glyphs, so the words drawn count.
    let glyphs = |bytes: &[u8]| bytes.to_vec();
    let requests = RequestsView {
        asks: vec![
            (glyphs(&WORD[..2]), glyphs(&WORD[2..])),
            (glyphs(&WORD[1..3]), glyphs(&WORD[3..])),
        ],
        accept: glyphs(&WORD[..1]),
        end: glyphs(&WORD[1..2]),
        waiting: None,
    };
    let card = dialog.card(&mut fake, &requests, WINDOW, None);
    let mut place = |view: &DialogView| dialog.place(&mut fake, view, WINDOW);
    let (playing_at, turn_at, many_at) = (place(&playing), place(&turn), place(&many));
    let leaves_at = leaves.each_ref().map(&mut place);
    // A 4:3 window, a long line on the host's turn and two long asks: the
    // card stands under the top bar, clear of the line.
    let long = DialogView {
        said: words(60),
        ..view(vec![words(1)])
    };
    let long_asks = RequestsView {
        asks: vec![(words(12), words(12)); 2],
        ..requests.clone()
    };
    let narrow_at = dialog.place(&mut fake, &long, NARROW);
    let narrow_card = narrow_at
        .as_ref()
        .and_then(|placed| dialog.card(&mut fake, &long_asks, NARROW, Some(placed)));
    // The keys wait: their line says why.
    let waiting = RequestsView {
        waiting: Some(strings::ANSWER_AFTER_READY),
        ..requests.clone()
    };
    let waiting = dialog.card(&mut fake, &waiting, WINDOW, None);
    let (wide, tall) = frame();
    let (wide_f, tall_f) = (wide as f32, tall as f32);
    // The bars of the window's height as of the layout's (100 and 102 of
    // 480); the words across the window, 48 from each edge, the rows 24 in
    // and as much narrower than the list as in the layout.
    let larger = |side: i32| (side as f32 / SCALE).round() as i32;
    let (top_bar, bottom_bar) = (larger(100), larger(102));
    let floor = tall - bottom_bar;
    let floor_f = floor as f32;
    let (left, width) = (48, wide - 96);
    let rows = width - (544 - 512);
    let tall_as = |text: &[u8]| {
        let lines = wrap(&font, text, rows as f32).len();
        (lines as f32 * pitch).ceil().max(19.0)
    };
    let replies_end = floor_f + 27.0 + tall_as(&turn.replies[1]);
    let leave_row = [replies_end, replies_end + tall_as(&strings::leave(b"L"))];
    let listed = |nth: usize| {
        let placed = leaves_at[nth].as_ref()?;
        let list = state(placed.gui.find("LB_REPLIES")?, &leaves[nth], [0.0; 3]).list?;
        Some((
            placed.leave,
            list.rows
                .iter()
                .map(|row| (row.text.clone(), row.dimmed))
                .collect::<Vec<_>>(),
        ))
    };
    let replies = |turn: &DialogView| {
        turn.replies
            .iter()
            .map(|reply| (reply.clone(), false))
            .collect::<Vec<_>>()
    };
    let with_leave = |offered: bool| {
        let mut rows = replies(&turn);
        rows.push((strings::leave(b"L"), !offered));
        Some((Some(leave_row), rows))
    };
    // The computer's screen: its Leave row under the replies, a click on it
    // leaving only while offered.
    let computer = |leave: Option<bool>| {
        let vm = ViewModel {
            computer: Some(ComputerView {
                replies: vec![words(1), words(2)],
                leave,
                leave_key: b"L".to_vec(),
                ..ComputerView::default()
            }),
            ..ViewModel::default()
        };
        let mut screen = Computer;
        let list = screen
            .state(&button("LB_REPLIES", [0, 0, 1, 1], b""), &vm)
            .list
            .map(|list| list.rows.last().map(|row| (row.text.clone(), row.dimmed)));
        let mut out = Vec::new();
        screen.row("LB_REPLIES", 2, &vm, &mut out);
        (list.flatten(), out == [Intent::Leave])
    };
    let computers = [Some(true), Some(false), None].map(computer);
    let mut card_drawn = DrawList::default();
    if let Some(card) = &card {
        let mut painter = Painter::new(&mut fake, Palette::K1);
        card.draw(Source::Skin, &mut painter, &mut card_drawn);
    }
    vec![
        check(
            "dialog_line_below_while_it_plays",
            (|| {
                let placed = playing_at.as_ref().ok_or("not placed")?;
                let message = rect(placed, "LBL_MESSAGE");
                expect(
                    &format!(
                        "bars across the window, {top_bar} and {bottom_bar} tall: {:?}",
                        placed.bars
                    ),
                    placed.bars
                        == [
                            [0.0, 0.0, wide_f, top_bar as f32],
                            [0.0, floor_f, wide_f, bottom_bar as f32],
                        ],
                )?;
                expect(
                    &format!(
                        "the line in the bottom bar, 680 wide, centred from its top: {message:?}"
                    ),
                    message == [(wide - 680) / 2, floor + 8, 680, bottom_bar - 16]
                        && alignment(placed) == 2 | 8,
                )?;
                let list = placed.gui.find("LB_REPLIES").ok_or("no list")?;
                expect(
                    "no reply shows",
                    state(list, &playing, [0.0; 3]).hidden && placed.rows.is_empty(),
                )?;
                Ok(format!("{message:?}"))
            })(),
        ),
        check(
            "dialog_line_above_replies_below_on_the_turn",
            (|| {
                let placed = turn_at.as_ref().ok_or("not placed")?;
                let (message, list) = (rect(placed, "LBL_MESSAGE"), rect(placed, "LB_REPLIES"));
                expect(
                    &format!("the line in the top bar, centred down to its bottom: {message:?}"),
                    message == [(wide - 680) / 2, 0, 680, top_bar - 8]
                        && alignment(placed) == 2 | 32,
                )?;
                let lines = wrap(&font, &turn.replies[1], rows as f32).len();
                let second = (lines as f32 * pitch).ceil().max(19.0);
                expect(
                    &format!(
                        "the replies from the bar's top, 8 in: {list:?}, {:?}",
                        placed.rows
                    ),
                    list[1] == floor
                        && placed.rows
                            == [
                                [floor_f + 8.0, floor_f + 27.0],
                                [floor_f + 27.0, floor_f + 27.0 + second],
                            ]
                        && placed.across == [(left + 24) as f32, (left + 24 + rows) as f32],
                )?;
                let shown = state(
                    placed.gui.find("LB_REPLIES").expect("placed"),
                    &turn,
                    [0.0; 3],
                );
                expect(
                    "the replies show, the first highlighted",
                    !shown.hidden
                        && shown.list.as_ref().is_some_and(|list| {
                            list.rows.len() == 2 && list.focused == Some(0) && list.fit
                        }),
                )?;
                Ok(format!("{} lines in the second reply", lines))
            })(),
        ),
        check(
            "dialog_bars_grow_to_half_the_window",
            (|| {
                let placed = many_at.as_ref().ok_or("not placed")?;
                expect(
                    &format!("the bottom bar half the frame: {:?}", placed.bars[1]),
                    placed.bars[1] == [0.0, tall_f / 2.0, wide_f, tall_f / 2.0],
                )?;
                Ok(format!("{:?}", placed.bars))
            })(),
        ),
        check(
            "dialog_pointer_finds_replies",
            (|| {
                let placed = turn_at.as_ref().ok_or("not placed")?;
                let scale = WINDOW.1 / tall_f;
                let at = |x: f32, y: f32| placed.row_at(WINDOW, [x * scale, y * scale]);
                let x = (left + 100) as f32;
                expect(
                    "the first reply, the second, the bar above, left of the rows",
                    at(x, floor_f + 12.0) == Some(0)
                        && at(x, floor_f + 31.0) == Some(1)
                        && at(x, floor_f - 80.0).is_none()
                        && at(left as f32, floor_f + 12.0).is_none(),
                )?;
                Ok("rows found".into())
            })(),
        ),
        check(
            "dialog_leave_row_under_replies",
            (|| {
                let shown = [0, 1, 2].map(listed);
                expect(
                    &format!("offered, faded locked, none: {shown:?}"),
                    shown
                        == [
                            with_leave(true),
                            with_leave(false),
                            Some((None, replies(&turn))),
                        ],
                )?;
                let placed = leaves_at[0].as_ref().ok_or("not placed")?;
                let scale = WINDOW.1 / tall_f;
                let at = |y: f32| [(left + 100) as f32 * scale, y * scale];
                let on_leave = at(leave_row[0] + 4.0);
                expect(
                    "the pointer finds the Leave row, not a reply there",
                    placed.leave_at(WINDOW, on_leave)
                        && placed.row_at(WINDOW, on_leave).is_none()
                        && !placed.leave_at(WINDOW, at(floor_f + 12.0)),
                )?;
                Ok(format!("{leave_row:?}"))
            })(),
        ),
        check(
            "computer_leave_row",
            (|| {
                let leave = Some(strings::leave(b"L"));
                let wanted = [
                    (leave.clone().map(|text| (text, false)), true),
                    (leave.map(|text| (text, true)), false),
                    (Some((strings::reply(1, &words(2)), false)), false),
                ];
                expect(
                    &format!("offered, faded locked, none; a click leaves: {computers:?}"),
                    computers == wanted,
                )?;
                Ok("the Leave row".into())
            })(),
        ),
        check(
            "dialog_request_card",
            (|| {
                let card = card.as_ref().ok_or("no card")?;
                let wanted: Vec<Vec<u8>> = requests
                    .asks
                    .iter()
                    .map(|(name, npc)| strings::asks(name, npc))
                    .chain([strings::answers(&requests.accept, &requests.end)])
                    .collect();
                expect(
                    &format!("both names and the keys: {:?}", card.lines),
                    card.lines == wanted,
                )?;
                let [x, y, w, h] = card.rect;
                let widest = wanted
                    .iter()
                    .map(|line| font.measure(line))
                    .fold(0.0, f32::max);
                expect(
                    &format!(
                        "in the top bar's bottom right corner, round its words: {:?}",
                        card.rect
                    ),
                    near(x + w, (wide - left) as f32)
                        && near(y + h, (top_bar - 8) as f32)
                        && w >= widest
                        && h >= 3.0 * pitch,
                )?;
                let inside = |q: &[f32; 4]| {
                    q[0] >= x - 1e-3
                        && q[1] >= y - 1e-3
                        && q[0] + q[2] <= x + w + 1e-3
                        && q[1] + q[3] <= y + h + 1e-3
                };
                // Every name and key a glyph: the asks' four and three, the
                // two keys.
                expect(
                    &format!(
                        "drawn on its backing, inside it: {} quads",
                        card_drawn.quads.len()
                    ),
                    card_drawn
                        .quads
                        .first()
                        .is_some_and(|q| q.rect == card.rect)
                        && card_drawn.quads.len() == 1 + 4 + 3 + 2
                        && card_drawn.quads.iter().all(|q| inside(&q.rect)),
                )?;
                Ok(format!("{:?}", card.rect))
            })(),
        ),
        check(
            "dialog_request_card_clear_of_the_line",
            (|| {
                let placed = narrow_at.as_ref().ok_or("not placed")?;
                let card = narrow_card.as_ref().ok_or("no card")?;
                let [x, y, w, h] = card.rect;
                let [l, t, mw, mh] = rect(placed, "LBL_MESSAGE").map(|side| side as f32);
                let apart = x >= l + mw || l >= x + w || y >= t + mh || t >= y + h;
                expect(
                    &format!(
                        "under the top bar {:?}, apart from the line [{l}, {t}, {mw}, {mh}]: {:?}",
                        placed.bars[0], card.rect
                    ),
                    y >= placed.bars[0][3] && apart,
                )?;
                let line = waiting.as_ref().and_then(|card| card.lines.last().cloned());
                expect(
                    &format!("waiting keys say why: {line:?}"),
                    line == Some(strings::bytes(strings::ANSWER_AFTER_READY)),
                )?;
                Ok(format!("{:?}", card.rect))
            })(),
        ),
    ]
}
