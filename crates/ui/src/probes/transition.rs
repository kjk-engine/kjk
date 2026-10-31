//! The area transition panel: the destination's words as KOTOR cuts them,
//! and the panel centred under the target block.

use fmt_odyssey::gui::Gui;

use super::menus::root;
use super::paint::button;
use super::{Check, check, expect};
use crate::screens::transition::{place, words};

/// K1's `areatransition`'s shape: a 400 × 82 panel, its strip and label.
fn layout() -> Gui {
    root(
        400,
        82,
        vec![
            button("LBL_TEXTBG", [0, 0, 400, 32], b""),
            button("LBL_DESCRIPTION", [26, 9, 348, 20], b""),
        ],
    )
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
            "transition_panel_centred_under_the_block",
            (|| {
                let gui = place(&layout(), (853, 480), 120);
                let strip = gui.find("LBL_TEXTBG").ok_or("no strip")?.extent;
                let label = gui.find("LBL_DESCRIPTION").ok_or("no label")?.extent;
                expect(
                    &format!("strip at {strip:?}, label at {label:?}"),
                    strip.left == (853 - 400) / 2
                        && strip.top > 120
                        && label.left == strip.left + 26
                        && label.top == strip.top + 9
                        && (gui.extent.width, gui.extent.height) == (853, 480),
                )?;
                Ok(format!("strip at ({}, {})", strip.left, strip.top))
            })(),
        ),
    ]
}
