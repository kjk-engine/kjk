//! The bark bubble: sized to its words' lines, over its speaker's point
//! (kept in the window), else where the layout puts it.

use fmt_odyssey::gui::{Border, Gui, Kind, Rect};

use super::menus::root;
use super::paint::{Fake, button};
use super::{Check, check, expect};
use crate::draw::Textures;
use crate::paint::wrap;
use crate::screens::bark::{SCALE, place, state};
use crate::skin::Source;
use crate::vm::BarkView;

/// The test font's glyphs, a word of four, eight words.
const WORDS: &[u8] = b"\x00\x01\x02\x03 \x00\x01\x02\x03 \x00\x01\x02\x03 \x00\x01\x02\x03 \
\x00\x01\x02\x03 \x00\x01\x02\x03 \x00\x01\x02\x03 \x00\x01\x02\x03";

/// K1's `barkbubble`'s shape: the root a framed panel at (48, 140), its
/// label 5 in.
fn layout() -> Gui {
    let mut label = button("LBL_BARKTEXT", [5, 5, 160, 108], b"");
    label.kind = Kind::Label;
    Gui {
        extent: Rect {
            left: 48,
            top: 140,
            width: 170,
            height: 118,
        },
        border: Some(Border {
            corner: "c".into(),
            ..Border::default()
        }),
        ..root(170, 118, vec![label])
    }
}

fn rect(gui: &Gui, tag: &str) -> Option<[i32; 4]> {
    let extent = gui.find(tag)?.extent;
    Some([extent.left, extent.top, extent.width, extent.height])
}

pub(super) fn all() -> Vec<Check> {
    let mut fake = Fake::new();
    let (_, font) = fake.font(Source::Skin, "grid").expect("the test font");
    let lines = wrap(&font, WORDS, 160.0).len() as i32;
    let tall = (lines as f32 * font.line_height()).ceil() as i32;
    let bark = |over| BarkView {
        text: WORDS.to_vec(),
        over,
    };
    // 1280 × 720: the HUD's frame 853 × 480, the bubble's that over SCALE.
    let (wide, tall_frame) = (
        (853.0 / SCALE).round() as i32,
        (480.0 / SCALE).round() as i32,
    );
    let mut placed = |over| {
        place(
            &layout(),
            &mut fake,
            Source::Skin,
            &bark(over),
            (1280.0, 720.0),
        )
    };
    let (alone, high, edge) = (
        placed(None),
        placed(Some([0.5, 0.5])),
        placed(Some([0.0, 0.0])),
    );
    vec![
        check(
            "bark_bubble_fits_its_lines",
            (|| {
                let gui = alone.as_ref().ok_or("no bubble")?;
                expect(&format!("several lines ({lines})"), lines > 1)?;
                let (panel, label) = (rect(gui, "PNL_BARK"), rect(gui, "LBL_BARKTEXT"));
                expect(
                    &format!("the panel {panel:?}, label {label:?} for {lines} lines"),
                    panel.is_some_and(|p| p[3] == tall + 10)
                        && label.is_some_and(|l| l[3] == tall && l[1] == panel.unwrap()[1] + 5),
                )?;
                expect(
                    &format!("the layout's place in the {wide} × {tall_frame} frame: {panel:?}"),
                    panel.is_some_and(|p| {
                        p[0] == 48 + (wide - 640) / 2 && p[1] == 140 * tall_frame / 480
                    }),
                )?;
                let blue = Some([0.0, 0.65, 0.97]);
                let words = state(gui.find("LBL_BARKTEXT").expect("placed"), &bark(None), blue);
                let backing = state(gui.find("PNL_BARK").expect("placed"), &bark(None), blue);
                expect(
                    "the label shows the words in the campaign's colour, the panel untinted",
                    words.text.as_deref() == Some(WORDS)
                        && words.tint == blue
                        && backing.tint.is_none(),
                )?;
                Ok(format!("{lines} lines, panel {panel:?}"))
            })(),
        ),
        check(
            "bark_bubble_over_its_speaker",
            (|| {
                let (Some(high), Some(edge)) = (high.as_ref(), edge.as_ref()) else {
                    return Err("no bubble".into());
                };
                let (over, kept) = (rect(high, "PNL_BARK"), rect(edge, "PNL_BARK"));
                // Its bottom centre on the frame's middle.
                let middle = (
                    (wide as f32 * 0.5).round() as i32,
                    (tall_frame as f32 * 0.5).round() as i32,
                );
                expect(
                    &format!("over the point {middle:?}: {over:?}"),
                    over.is_some_and(|p| (p[0] + p[2] / 2, p[1] + p[3]) == middle),
                )?;
                expect(
                    &format!("kept in the frame: {kept:?}"),
                    kept.is_some_and(|p| p[0] == 0 && p[1] == 0),
                )?;
                Ok(format!("{over:?}, {kept:?}"))
            })(),
        ),
    ]
}
