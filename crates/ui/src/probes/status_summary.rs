//! The status summary: only the rows that apply, placed as KOTOR 2 places
//! them, whichever rows those are, and the panel centred in the window.

use fmt_odyssey::gui::{Control, Gui, Rect};

use super::{Check, check, expect};
use crate::screens::status_summary::{Row, place};
use crate::vm::SummaryView;

/// A layout of `rows`, 40 pixels apart from 10 down, each a 32-pixel icon
/// with its words beside it.
fn layout(rows: &[Row]) -> Gui {
    let control = |tag: &str, left, top| Control {
        tag: tag.to_string(),
        extent: Rect {
            left,
            top,
            width: 200,
            height: 32,
        },
        ..Control::default()
    };
    let controls = rows
        .iter()
        .enumerate()
        .flat_map(|(at, row)| {
            let (icon, words) = row.tags();
            let top = 10 + 40 * at as i32;
            [control(icon, 10, top), control(words, 50, top)]
        })
        .collect();
    Gui {
        tag: "statussummary".to_string(),
        extent: Rect {
            left: 0,
            top: 0,
            width: 260,
            height: 132,
        },
        border: None,
        color: None,
        alpha: None,
        controls,
        unread: Vec::new(),
    }
}

/// `rows` (each with `words`) as `gui` places them in a window of `window`
/// physical pixels at text size `text`. Words are 10 pixels a letter, and
/// wrap to as many lines as that needs.
fn placed(gui: &Gui, rows: &[Row], words: &str, window: (i32, i32), text: f32) -> (Gui, i32) {
    let view = SummaryView {
        rows: rows
            .iter()
            .map(|row| (*row, words.as_bytes().to_vec()))
            .collect(),
        slide: 1.0,
        alpha: 1.0,
        ..SummaryView::default()
    };
    let mut lines = |control: &Control, words: &[u8]| {
        (words.len() * 10).div_ceil(control.extent.width.max(1) as usize)
    };
    place(gui, &view, window, text, &mut lines, 16.0)
}

/// `rows` as `gui` places them in an 800 × 600 window, each one short word.
fn shown(gui: &Gui, rows: &[Row]) -> Gui {
    placed(gui, rows, "words", (800, 600), 1.0).0
}

/// A placed panel's left, top, width and height.
fn extent(gui: &Gui) -> [i32; 4] {
    let Rect {
        left,
        top,
        width,
        height,
    } = gui.extent;
    [left, top, width, height]
}

pub(super) fn all() -> Vec<Check> {
    let gui = layout(&[Row::Journal, Row::Credits, Row::Xp]);
    let tops = |placed: &Gui| -> Vec<i32> {
        placed
            .controls
            .iter()
            .step_by(2)
            .map(|c| c.extent.top)
            .collect()
    };
    vec![
        check(
            "status_summary_rows",
            (|| {
                // At text size 1 in an 800 × 600 window: the first row 18
                // down, each next 37 below; the panel 20 under the rows,
                // 225 + 30 + 50 wide, centred.
                for (rows, panel, at) in [
                    (&[Row::Xp][..], [247, 262, 305, 75], vec![18]),
                    (
                        &[Row::Credits, Row::Xp][..],
                        [247, 244, 305, 112],
                        vec![18, 55],
                    ),
                    (
                        &[Row::Xp, Row::Journal, Row::Credits][..],
                        [247, 225, 305, 149],
                        vec![18, 55, 92],
                    ),
                ] {
                    let placed = shown(&gui, rows);
                    expect(
                        &format!(
                            "{rows:?}: panel {:?} (want {panel:?}), rows at {:?} (want {at:?})",
                            extent(&placed),
                            tops(&placed)
                        ),
                        extent(&placed) == panel && tops(&placed) == at,
                    )?;
                }
                Ok("rows 37 apart from 18 down, the panel centred".to_string())
            })(),
        ),
        check("status_summary_scaled", scaled()),
        check("status_summary_influence", influence()),
        check("status_summary_ok", ok()),
    ]
}

/// KOTOR 2's OK, 100 × 22 in the layout: in an 800 × 600 window at text
/// size 1 it stands under the one row (55 down), centred across the 305
/// wide panel, and the panel ends 20 under it (97 tall). At 2560 × 1080
/// (text size 1.5) it is 100 stretched by 3.2 (320) wide and 22 stretched by
/// 1.8 (39) and made 5% taller (40) high. A summary without the OK has no
/// button and ends 20 under its row.
fn ok() -> Result<String, String> {
    let mut gui = layout(&[Row::Journal, Row::Credits, Row::Xp]);
    gui.controls.push(Control {
        tag: crate::screens::status_summary::OK.to_string(),
        extent: Rect {
            left: 114,
            top: 554,
            width: 100,
            height: 22,
        },
        ..Control::default()
    });
    let with_ok = |window: (i32, i32), text: f32, ok: Option<bool>| {
        let view = SummaryView {
            rows: vec![(Row::Xp, b"words".to_vec())],
            ok,
            ..SummaryView::default()
        };
        let mut lines = |control: &Control, words: &[u8]| {
            (words.len() * 10).div_ceil(control.extent.width.max(1) as usize)
        };
        let (placed, _) = place(&gui, &view, window, text, &mut lines, 16.0);
        let button = placed
            .controls
            .iter()
            .find(|c| c.tag == crate::screens::status_summary::OK)
            .map(|c| [c.extent.left, c.extent.top, c.extent.width, c.extent.height]);
        (extent(&placed), button)
    };
    let small = with_ok((800, 600), 1.0, Some(false));
    let wide = with_ok((2560, 1080), 1.5, Some(false));
    let none = with_ok((800, 600), 1.0, None);
    expect(
        &format!("800 × 600 {small:?}; 2560 × 1080 {wide:?}; without the OK {none:?}"),
        small == ([247, 251, 305, 97], Some([102, 55, 100, 22]))
            && wide.1.is_some_and(|[left, _, width, height]| {
                (width, height) == (320, 40) && left == (wide.0[2] - 320) / 2
            })
            && none == ([247, 262, 305, 75], None),
    )?;
    Ok(format!(
        "the OK at {:?} in a panel at {:?}",
        small.1, small.0
    ))
}

/// At 2560 × 1080 (text size 1.5) the rows' lefts stretch by 3.2, and the
/// rest grows by 1.5 (icons 48 square); a datapad's row makes the panel at
/// least 675 wide. At 800 × 600 a notice too long for a line is first
/// narrowed from 400, KOTOR 2's measuring width, to the narrowest that keeps
/// its 2 lines (300), then widened 30 at a time until past 439 (450).
fn scaled() -> Result<String, String> {
    let gui = &layout(&[Row::Journal, Row::Credits, Row::Xp, Row::Received]);
    let wide = (2560, 1080);
    let (xp, _) = placed(gui, &[Row::Xp, Row::Credits], "words", wide, 1.5);
    let icon = xp.controls.first().map(|c| c.extent);
    let (notice, _) = placed(gui, &[Row::Notice], &"x".repeat(60), (800, 600), 1.0);
    let words = notice.controls.first().map(|c| c.extent);
    let (datapad, _) = placed(gui, &[Row::Datapad], "words", wide, 1.5);
    let datapad_width = datapad.controls.get(1).map(|c| c.extent.width);
    expect(
        &format!(
            "panel {:?}, first icon {icon:?}, tops {:?}; a long notice's words {words:?}; a datapad's words {datapad_width:?} wide",
            extent(&xp),
            xp.controls
                .iter()
                .step_by(2)
                .map(|c| c.extent.top)
                .collect::<Vec<_>>(),
        ),
        icon.is_some_and(|icon| (icon.left, icon.width, icon.height) == (32, 48, 48))
            && extent(&xp)[0] == (2560 - extent(&xp)[2]) / 2
            && words.is_some_and(|words| words.width == 450)
            && datapad_width.is_some_and(|width| width >= 675),
    )?;
    Ok(format!("panel {:?}", extent(&xp)))
}

/// KOTOR 2's layout shows the influence rows; K1's, which has none, drops
/// them; neither lights a HUD icon.
fn influence() -> Result<String, String> {
    let rows = [Row::Journal, Row::InfluenceGained, Row::InfluenceLost];
    let k2 = shown(&layout(&rows), &rows);
    let k1 = shown(&layout(&[Row::Journal]), &rows);
    let unlit = rows[1..].iter().all(|row| row.hud().is_none());
    expect(
        &format!(
            "KOTOR 2 places {} controls, K1 {}; influence lights a HUD icon: {}",
            k2.controls.len(),
            k1.controls.len(),
            !unlit
        ),
        k2.controls.len() == 6 && k1.controls.len() == 2 && unlit,
    )?;
    Ok("both influence rows in KOTOR 2's layout, none in K1's, no HUD icon".to_string())
}
