//! The status summary: only the rows that apply, stacked from the layout's
//! first row's place, whichever rows those are.

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

/// `rows` as `gui` places them, each one line of words.
fn shown(gui: &Gui, rows: &[Row]) -> Gui {
    let view = SummaryView {
        rows: rows.iter().map(|row| (*row, b"words".to_vec())).collect(),
        slide: 1.0,
        alpha: 1.0,
    };
    place(gui, &view, &mut |_, _| 1, 16.0)
}

pub(super) fn all() -> Vec<Check> {
    let gui = layout(&[Row::Journal, Row::Credits, Row::Xp]);
    let shown = |rows: &[Row]| shown(&gui, rows);
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
                for (rows, height, at) in [
                    (&[Row::Xp][..], 52, vec![10]),
                    (&[Row::Credits, Row::Xp][..], 89, vec![10, 47]),
                    (
                        &[Row::Journal, Row::Credits, Row::Xp][..],
                        126,
                        vec![10, 47, 84],
                    ),
                ] {
                    let placed = shown(rows);
                    expect(
                        &format!(
                            "{rows:?}: the layout's margin round the rows shown: height {} (want {height}), rows at {:?} (want {at:?})",
                            placed.extent.height,
                            tops(&placed)
                        ),
                        placed.extent.height == height && tops(&placed) == at,
                    )?;
                }
                Ok("one row as tall as one row, from the layout's first row's place".to_string())
            })(),
        ),
        check("status_summary_influence", influence()),
    ]
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
