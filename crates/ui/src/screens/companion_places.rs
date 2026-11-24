//! The companions' places on the party selection layout (`partyselection`,
//! KOTOR 2's `partyselect_p`). The party screen and the co-op lobby both
//! lay out their companions with this module.
//!
//! A place is three controls with one number: the button (`BTN_NPCn`), the
//! portrait (`LBL_CHARn`) and the "no companion" art (`LBL_NAn`).
//!
//! KOTOR 2's executable assumes that the Handmaiden and the Disciple never
//! travel together, and that Mira and Hanharr never do. Its layout puts
//! each pair in one place. A mod can recruit both companions of a pair.
//! Each of the two then needs a place of its own. Some mods' layouts
//! already give the stand-in a place of its own. Where the layout puts the
//! two in one place, the stand-in moves to the first free place of the grid
//! the layout's buttons form. Every place keeps the layout's own art.

use fmt_odyssey::gui::{Control, Gui, Rect};

/// The three controls of a place, by the prefix before their number.
pub const PIECES: [&str; 3] = ["BTN_NPC", "LBL_CHAR", "LBL_NA"];

/// KOTOR 2's places that stand in for another, each with the place it
/// takes: Hanharr's (10) takes Mira's (7), and the Disciple's (11) takes
/// the Handmaiden's (4).
pub const STAND_INS: [(u8, u8); 2] = [(10, 7), (11, 4)];

fn extent(gui: &Gui, tag: &str) -> Option<Rect> {
    gui.find(tag).map(|control| control.extent)
}

fn overlaps(a: Rect, b: Rect) -> bool {
    a.left < b.left + b.width
        && b.left < a.left + a.width
        && a.top < b.top + b.height
        && b.top < a.top + a.height
}

/// Moves place `at` to `to`. A piece the layout does not have is made as a
/// copy of the first place's piece, so a place past the layout's last one
/// uses the layout's own button and portrait art. A moved piece has no
/// links to its neighbours, so the keyboard and the gamepad reach the
/// nearest control in each direction.
pub fn put(gui: &mut Gui, at: usize, to: Rect) {
    for prefix in PIECES {
        let tag = format!("{prefix}{at}");
        if gui.find(&tag).is_none() {
            let Some(first) = gui.find(&format!("{prefix}0")).cloned() else {
                continue;
            };
            let id = (gui.controls.iter().map(|control| control.id).max()).map_or(0, |id| id + 1);
            gui.controls.push(Control {
                id,
                tag: tag.clone(),
                ..first
            });
        }
        if let Some(control) = gui.controls.iter_mut().find(|control| control.tag == tag) {
            control.extent = to;
            control.moveto = None;
        }
    }
}

/// Whether `tag` is a place's button: `BTN_NPC` and a number.
fn is_place(tag: &str) -> bool {
    (tag.strip_prefix("BTN_NPC")).is_some_and(|n| n.parse::<u8>().is_ok())
}

/// The buttons of the layout's places, those of zero size left out.
fn buttons(gui: &Gui) -> Vec<Rect> {
    (gui.controls.iter())
        .filter(|control| is_place(&control.tag))
        .map(|control| control.extent)
        .filter(|rect| rect.width > 0 && rect.height > 0)
        .collect()
}

/// The first place of the grid that no button covers, row by row from the
/// top and left to right. The grid's rows are those of the layout's
/// buttons. Its spacing is the smallest gap between two buttons of one row.
/// A free place lies inside the area the buttons cover. KOTOR 2's layout
/// sets its ten places in a ring, and the two free places are in the ring's
/// middle row.
fn free_place(gui: &Gui) -> Option<Rect> {
    let buttons = buttons(gui);
    let first = *buttons.first()?;
    let (width, height) = (first.width, first.height);
    let mut rows: Vec<i32> = buttons.iter().map(|rect| rect.top).collect();
    rows.sort_unstable();
    rows.dedup();
    let pitch = (rows.iter())
        .flat_map(|&top| {
            let row: Vec<i32> = (buttons.iter())
                .filter(|rect| rect.top == top)
                .map(|rect| rect.left)
                .collect();
            (row.iter())
                .flat_map(|a| row.iter().map(move |b| b - a))
                .filter(|gap| *gap >= width)
                .collect::<Vec<_>>()
        })
        .min()?;
    let left = buttons.iter().map(|rect| rect.left).min()?;
    let right = buttons.iter().map(|rect| rect.left + rect.width).max()?;
    rows.iter().find_map(|&top| {
        let anchor = (buttons.iter())
            .filter(|rect| rect.top == top)
            .map(|rect| rect.left)
            .min()?;
        let start = anchor - (anchor - left) / pitch * pitch;
        (0..)
            .map(|k| start + k * pitch)
            .take_while(|x| x + width <= right)
            .map(|x| Rect {
                left: x,
                top,
                width,
                height,
            })
            .find(|cell| !buttons.iter().any(|button| overlaps(*cell, *button)))
    })
}

/// Moves place `at` so that its button's corner is at `to`'s corner. Each
/// piece moves by the same distance and keeps its size, so a portrait the
/// layout sets inside the button stays inside it.
fn shift(gui: &mut Gui, at: u8, from: Rect, to: Rect) {
    let (right, down) = (to.left - from.left, to.top - from.top);
    for prefix in PIECES {
        let tag = format!("{prefix}{at}");
        if let Some(control) = gui.controls.iter_mut().find(|control| control.tag == tag) {
            control.extent.left += right;
            control.extent.top += down;
        }
    }
}

/// Clears the focus links of every place's button. The keyboard and the
/// gamepad then reach the nearest place in each direction, the moved ones
/// too. The layout's links lead around the ring and past the moved places.
fn unlink(gui: &mut Gui) {
    for control in &mut gui.controls {
        if is_place(&control.tag) {
            control.moveto = None;
        }
    }
}

/// The party screen's places, for the stand-ins in `both`: those whose
/// pair is available too. A stand-in whose button lies over its pair's
/// button moves to a free place of the grid. Returns the stand-ins that
/// found no free place. Those two companions still share one place, as on
/// KOTOR 2's own screen.
pub fn separate(gui: &mut Gui, both: &[u8]) -> Vec<u8> {
    let mut crowded = Vec::new();
    let mut moved = false;
    for &(stand_in, pair) in STAND_INS
        .iter()
        .filter(|(stand_in, _)| both.contains(stand_in))
    {
        let (Some(own), Some(theirs)) = (
            extent(gui, &format!("BTN_NPC{stand_in}")),
            extent(gui, &format!("BTN_NPC{pair}")),
        ) else {
            continue;
        };
        if !overlaps(own, theirs) {
            continue;
        }
        match free_place(gui) {
            Some(to) => {
                shift(gui, stand_in, own, to);
                moved = true;
            }
            None => crowded.push(stand_in),
        }
    }
    if moved {
        unlink(gui);
    }
    crowded
}
