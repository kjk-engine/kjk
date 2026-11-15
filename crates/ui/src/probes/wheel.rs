//! The power wheel's drawing (`screens::wheel`).

use fmt_odyssey::gui::{Border, Control, Gui};

use super::menus::root;
use super::paint::{Fake, button};
use super::{Check, check, expect};
use crate::draw::{DrawList, Textures};
use crate::paint::{Painter, Palette};
use crate::screens::wheel::{ARC, ARC_ENTRY, draw};
use crate::skin::Source;
use crate::vm::{KeyLabel, WheelEntry, WheelView};

/// The frame's height in the probe's layout (layout pixels).
const TALL: i32 = 600;
/// The textures of an entry's frame and of the label's backing.
const FRAME: &str = "lbl_mibox00";
const BACKING: &str = "lbl_mimidbot";

/// A control with `texture` as its border's corner, edge and fill.
fn bordered(tag: &str, rect: [i32; 4], texture: &str) -> Control {
    Control {
        border: Some(Border {
            corner: texture.into(),
            edge: texture.into(),
            fill: texture.into(),
            dimension: 4,
            ..Border::default()
        }),
        ..button(tag, rect, b"")
    }
}

/// The HUD layout's controls the wheel clones.
fn layout() -> Gui {
    root(
        800,
        TALL,
        vec![
            bordered("BTN_ACTION0", [100, 500, 40, 40], FRAME),
            button("LBL_ACTION0", [100, 500, 40, 40], b""),
            bordered("LBL_MOULDING3", [300, 500, 100, 20], BACKING),
            button("LBL_ACTIONDESC", [300, 500, 100, 20], b""),
            bordered("BTN_ACTIONUP0", [450, 500, 16, 16], "lbl_miarrow"),
            bordered("BTN_ACTIONDOWN0", [470, 500, 16, 16], "lbl_miarrow"),
        ],
    )
}

/// `count` entries whose icons are named `prefix` and their number.
fn entries(prefix: &str, count: usize) -> Vec<WheelEntry> {
    (0..count)
        .map(|n| WheelEntry {
            icon: format!("{prefix}{n}"),
            name: vec![(n % 4) as u8],
            count: Some(n as u32 + 1),
            ..WheelEntry::default()
        })
        .collect()
}

/// The wheel drawn, and the fake textures it was drawn with.
fn drawn(wheel: &WheelView) -> (DrawList, Fake) {
    let mut fake = Fake::new();
    let out = draw(&layout(), wheel, &mut Painter::new(&mut fake, Palette::TSL));
    (out, fake)
}

/// The quads drawn with the texture `name`.
fn quads_of(out: &DrawList, fake: &mut Fake, name: &str) -> Vec<[f32; 4]> {
    let id = fake.texture(Source::Campaign, name);
    out.quads
        .iter()
        .filter(|quad| Some(quad.texture) == id)
        .map(|quad| quad.rect)
        .collect()
}

/// A wheel of 8 entries with entry 2 highlighted and a 5-item arc beyond
/// it draws entry 2's icon 1.2 times as large as the others, and the five
/// arc items beyond the ring's outer edge. A wheel with nothing to offer
/// draws its centre label and its key, and no entry.
pub(super) fn all() -> Vec<Check> {
    let key = KeyLabel {
        words: vec![1],
        badge: None,
    };
    let full = WheelView {
        entries: entries("e", 8),
        at: Some(2),
        arc: entries("a", 5),
        arc_at: Some(2),
        key: key.clone(),
        ..WheelView::default()
    };
    let (out, mut fake) = drawn(&full);
    let side = |name: &str, fake: &mut Fake| quads_of(&out, fake, name).first().map(|rect| rect[2]);
    let (plain, lit) = (side("e0", &mut fake), side("e2", &mut fake));
    let grown = plain
        .zip(lit)
        .is_some_and(|(plain, lit)| (lit / plain - 1.2).abs() < 0.03);
    let tall = TALL as f32;
    let inner_edge = (ARC - ARC_ENTRY / 2.0) * tall;
    let beyond = (0..5)
        .flat_map(|n| quads_of(&out, &mut fake, &format!("a{n}")))
        .filter(|[x, y, w, h]| {
            let (dx, dy) = (x + w / 2.0 - 400.0, y + h / 2.0 - tall / 2.0);
            (dx * dx + dy * dy).sqrt() - w / 2.0 >= inner_edge - 1.0
        })
        .count();
    let empty = WheelView {
        key,
        ..WheelView::default()
    };
    let (bare, mut bare_fake) = drawn(&empty);
    let frames = quads_of(&bare, &mut bare_fake, FRAME).len();
    let backing = quads_of(&bare, &mut bare_fake, BACKING).len();
    // The fake's font draws every letter with its one texture.
    let letters = bare.quads.len() - backing;
    let only_label = frames == 0 && backing > 0 && letters > 0;
    vec![check(
        "power_wheel_drawn",
        expect(
            &format!(
                "entry side {plain:?}, highlighted {lit:?}, arc items beyond the ring {beyond}, empty wheel frames {frames}, backing {backing}, letters {letters}"
            ),
            grown && beyond == 5 && only_label,
        )
        .map(|()| {
            "the highlighted entry 1.2 times as large, 5 arc items beyond the ring, the empty wheel its label and key only".into()
        }),
    )]
}
