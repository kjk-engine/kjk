//! The area transition panel: the destination's words as KOTOR cuts them,
//! and the panel placed as KOTOR 2 places it.

use fmt_odyssey::gui::{Gui, Rect};

use super::menus::root;
use super::paint::button;
use super::{Check, check, expect};
use crate::screens::transition::{place, words};

/// KOTOR 2's `areatrans_p`'s shape: a 400 × 82 panel 80 pixels down its
/// 640 × 480 frame, its strip, icon and label, the label where K1's
/// layout has it rather than KOTOR 2's.
fn layout() -> Gui {
    Gui {
        extent: Rect {
            left: 120,
            top: 80,
            width: 400,
            height: 82,
        },
        ..root(
            400,
            82,
            vec![
                button("LBL_TEXTBG", [0, 0, 400, 32], b""),
                button("LBL_ICON", [175, 27, 50, 50], b""),
                button("LBL_DESCRIPTION", [26, 9, 348, 20], b""),
            ],
        )
    }
}

/// The strip's, the icon's and the label's extents as `place` puts them.
fn placed(window: (i32, i32), text: f32, under: Option<i32>) -> Result<[Rect; 3], String> {
    let gui = place(&layout(), window, text, under);
    let at = |tag: &str| gui.find(tag).map(|c| c.extent).ok_or(format!("no {tag}"));
    Ok([at("LBL_TEXTBG")?, at("LBL_ICON")?, at("LBL_DESCRIPTION")?])
}

/// A rect of `[left, top, width, height]`.
fn rect([left, top, width, height]: [i32; 4]) -> Rect {
    Rect {
        left,
        top,
        width,
        height,
    }
}

pub(super) fn all() -> Vec<Check> {
    vec![
        check(
            "transition_words_drop_the_planet",
            (|| {
                for (name, shown) in [
                    (&b"Taris - Upper City South"[..], &b"Upper City South"[..]),
                    (b"To Anchorhead", b"To Anchorhead"),
                    (b"Peragus Fuel Depot", b"Peragus Fuel Depot"),
                    (b"Taris -", b""),
                ] {
                    let got = words(name);
                    expect(
                        &format!(
                            "{:?} shows {:?}",
                            String::from_utf8_lossy(name),
                            String::from_utf8_lossy(&got)
                        ),
                        got == shown,
                    )?;
                }
                Ok("the part after the first dash and its space; whole without one".into())
            })(),
        ),
        check(
            "transition_panel_placed_as_kotor2",
            (|| {
                // At 1920 × 1080 (text size 1): 400 × 82, centred across,
                // 80 × 1.8 down; the name centred at 6 down whatever the
                // layout says. At 2560 × 1440 (text size 1.5): every size
                // half again, 80 × 2.4 down. Under a bark bubble ending at
                // 300: its top there.
                let full_hd = placed((1920, 1080), 1.0, None)?;
                let wide = placed((2560, 1440), 1.5, None)?;
                let barked = placed((1920, 1080), 1.0, Some(300))?;
                expect(
                    &format!("1080p {full_hd:?}; 1440p {wide:?}; under a bark {barked:?}"),
                    full_hd
                        == [
                            rect([760, 144, 400, 32]),
                            rect([935, 171, 50, 50]),
                            rect([786, 150, 348, 20]),
                        ]
                        && wide
                            == [
                                rect([980, 192, 600, 48]),
                                rect([1243, 232, 75, 75]),
                                rect([1019, 201, 522, 30]),
                            ]
                        && barked[0] == rect([760, 300, 400, 32]),
                )?;
                Ok(format!("strip at {:?}", full_hd[0]))
            })(),
        ),
    ]
}
