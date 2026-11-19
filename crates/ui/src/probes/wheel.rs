//! The power wheel's drawing (`screens::wheel`).

use fmt_odyssey::gui::{Border, Control, Gui};

use super::menus::root;
use super::paint::{Fake, button};
use super::{Check, check, expect};
use crate::draw::{DrawList, TextureId, Textures};
use crate::paint::{Painter, Palette};
use crate::screens::wheel::{ARC_ENTRY, Ring, draw};
use crate::skin::Source;
use crate::vm::{WheelEntry, WheelView};

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
    let out = draw(
        &layout(),
        wheel,
        1.0,
        &mut Painter::new(&mut fake, Palette::TSL),
    );
    (out, fake)
}

/// The middle of the solid quads drawn (the pointer's dot), in frame
/// pixels, and how many there are.
fn dot_of(out: &DrawList) -> Option<([f32; 2], usize)> {
    let solid: Vec<[f32; 4]> = out
        .quads
        .iter()
        .filter(|quad| quad.texture == TextureId::WHITE)
        .map(|quad| quad.rect)
        .collect();
    let left = solid.iter().map(|rect| rect[0]).reduce(f32::min)?;
    let right = solid
        .iter()
        .map(|rect| rect[0] + rect[2])
        .reduce(f32::max)?;
    let top = solid.iter().map(|rect| rect[1]).reduce(f32::min)?;
    let bottom = solid
        .iter()
        .map(|rect| rect[1] + rect[3])
        .reduce(f32::max)?;
    Some(([(left + right) / 2.0, (top + bottom) / 2.0], solid.len()))
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
/// draws its centre label and no entry. The pointer's dot stands where
/// the player points.
pub(super) fn all() -> Vec<Check> {
    let full = WheelView {
        entries: entries("e", 8),
        at: Some(2),
        arc: entries("a", 5),
        arc_at: Some(2),
        pointer: [0.1, -0.05],
        ..WheelView::default()
    };
    let (out, mut fake) = drawn(&full);
    let side = |name: &str, fake: &mut Fake| quads_of(&out, fake, name).first().map(|rect| rect[2]);
    let (plain, lit) = (side("e0", &mut fake), side("e2", &mut fake));
    let grown = plain
        .zip(lit)
        .is_some_and(|(plain, lit)| (lit / plain - 1.2).abs() < 0.03);
    let tall = TALL as f32;
    let inner_edge = (Ring::new(&[0; 8]).arc() - ARC_ENTRY / 2.0) * tall;
    let beyond = (0..5)
        .flat_map(|n| quads_of(&out, &mut fake, &format!("a{n}")))
        .filter(|[x, y, w, h]| {
            let (dx, dy) = (x + w / 2.0 - 400.0, y + h / 2.0 - tall / 2.0);
            (dx * dx + dy * dy).sqrt() - w / 2.0 >= inner_edge - 1.0
        })
        .count();
    // The pointer 0.1 H right of the centre and 0.05 H below it, on a frame
    // 800 wide and 600 tall.
    let pointed = dot_of(&out).map(|(middle, _)| middle);
    let want = [400.0 + 0.1 * tall, tall / 2.0 + 0.05 * tall];
    let dot_placed =
        pointed.is_some_and(|[x, y]| (x - want[0]).abs() < 1.0 && (y - want[1]).abs() < 1.0);
    let (bare, mut bare_fake) = drawn(&WheelView::default());
    let frames = quads_of(&bare, &mut bare_fake, FRAME).len();
    let backing = quads_of(&bare, &mut bare_fake, BACKING).len();
    let dot = dot_of(&bare).map_or(0, |(_, quads)| quads);
    // Nothing else is drawn. The wheel's key no longer stands under the
    // label, so no words of a key are drawn.
    let letters = bare.quads.len() - backing - dot;
    let only_label = frames == 0 && backing > 0 && letters == 0 && dot > 0;
    // An entry the character cannot use is drawn faded and grey. The others
    // are drawn in full colour.
    let mut dimmed = entries("d", 3);
    dimmed[1].dim = true;
    let (out, mut fake) = drawn(&WheelView {
        entries: dimmed,
        ..WheelView::default()
    });
    let looks = |name: &str, fake: &mut Fake| {
        let id = fake.texture(Source::Campaign, name);
        let quads: Vec<_> = out
            .quads
            .iter()
            .filter(|quad| Some(quad.texture) == id)
            .collect();
        let grey = !quads.is_empty() && quads.iter().all(|quad| quad.grey && quad.color[3] < 1.0);
        let full = !quads.is_empty() && quads.iter().all(|quad| !quad.grey);
        (grey, full)
    };
    let greyed = [
        looks("d0", &mut fake),
        looks("d1", &mut fake),
        looks("d2", &mut fake),
    ];
    let grey_check = check(
        "power_wheel_dimmed_grey",
        expect(
            &format!("entries 0 to 2 as (grey, full colour): {greyed:?}"),
            greyed == [(false, true), (true, false), (false, true)],
        )
        .map(|()| {
            "an entry the character cannot use is drawn faded and grey, the others in full colour"
                .into()
        }),
    );
    vec![grey_check, check(
        "power_wheel_drawn",
        expect(
            &format!(
                "entry side {plain:?}, highlighted {lit:?}, arc items beyond the ring {beyond}, pointer's dot at {pointed:?} for {want:?}, empty wheel frames {frames}, backing {backing}, letters {letters}, dot quads {dot}"
            ),
            grown && beyond == 5 && dot_placed && only_label,
        )
        .map(|()| {
            "the highlighted entry 1.2 times as large, 5 arc items beyond the ring, the pointer's dot where the player points, the empty wheel its label and the dot, with no key".into()
        }),
    )]
}
