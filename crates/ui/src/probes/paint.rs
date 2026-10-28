//! Borders, text, lists and states painted as KOTOR draws them.

use std::collections::BTreeMap;
use std::sync::Arc;

use asset_odyssey::font::Font;
use fmt_odyssey::gui::{Border, Control, Gui, Kind, Rect, Text};
use fmt_odyssey::txi::Txi;

use super::formats::GRID;
use super::{Check, check, expect, near};
use crate::draw::{DrawList, TextureId, Textures};
use crate::paint::{ControlState, Painter, Palette, rows_that_fit};
use crate::skin::Source;

/// Every name resolves except those starting `missing`; the font `grid`
/// is the 2 × 2 test font.
pub(super) struct Fake {
    names: BTreeMap<String, TextureId>,
    font: Arc<Font>,
    pub(super) layouts: BTreeMap<&'static str, Gui>,
}

impl Fake {
    pub(super) fn new() -> Fake {
        let font = Font::from_txi("grid", &Txi::parse(GRID.as_bytes()), 32, 32)
            .expect("the test font reads");
        Fake {
            names: BTreeMap::new(),
            font: Arc::new(font),
            layouts: BTreeMap::new(),
        }
    }

    /// The name texture `id` was asked for by, if any.
    pub(super) fn named(&self, id: TextureId) -> Option<String> {
        self.names
            .iter()
            .find_map(|(name, of)| (*of == id).then(|| name.clone()))
    }
}

impl Textures for Fake {
    fn texture(&mut self, _: Source, name: &str) -> Option<TextureId> {
        if name.starts_with("missing") {
            return None;
        }
        let next = TextureId(self.names.len() as u32 + 1);
        Some(*self.names.entry(name.to_string()).or_insert(next))
    }

    fn size(&self, _: TextureId) -> (u32, u32) {
        (16, 16)
    }

    fn additive(&self, _: TextureId) -> bool {
        false
    }

    fn font(&mut self, _: Source, name: &str) -> Option<(TextureId, Arc<Font>)> {
        // The floating words' own font too (`screens::floaty`).
        matches!(name, "grid" | "fnt_d16x16").then(|| (TextureId(99), self.font.clone()))
    }

    fn text(&self, _: Source, _: u32) -> Option<Vec<u8>> {
        None
    }
}

impl crate::assets::Assets for Fake {
    fn layout(&mut self, name: &str) -> Result<(Gui, Source), String> {
        self.layouts
            .get(name)
            .cloned()
            .map(|gui| (gui, Source::Skin))
            .ok_or(format!("{name}: none"))
    }

    fn layout_in(&mut self, name: &str, _: Source) -> Result<Gui, String> {
        self.layout(name).map(|(gui, _)| gui)
    }
}

fn frame(corner: &str) -> Border {
    Border {
        corner: corner.into(),
        edge: "e".into(),
        fill: "f".into(),
        dimension: 4,
        color: Some([1.0; 3]),
        ..Border::default()
    }
}

pub(super) fn button(tag: &str, rect: [i32; 4], words: &[u8]) -> Control {
    Control {
        kind: Kind::Button,
        tag: tag.into(),
        extent: Rect {
            left: rect[0],
            top: rect[1],
            width: rect[2],
            height: rect[3],
        },
        text: Some(Text {
            font: "grid".into(),
            text: words.to_vec(),
            alignment: 9,
            color: Some([0.0, 0.5, 1.0]),
            ..Text::default()
        }),
        ..Control::default()
    }
}

fn inside(rect: [f32; 4], within: [f32; 4]) -> bool {
    rect[0] >= within[0] - 1e-3
        && rect[1] >= within[1] - 1e-3
        && rect[0] + rect[2] <= within[0] + within[2] + 1e-3
        && rect[1] + rect[3] <= within[1] + within[3] + 1e-3
}

pub(super) fn all() -> Vec<Check> {
    let mut fake = Fake::new();
    let mut painter = Painter::new(&mut fake, Palette::K1);
    let mut nine = DrawList::default();
    painter.border(&frame("c"), [0.0, 0.0, 100.0, 50.0], [1.0; 4], &mut nine);
    let mut aligned = Vec::new();
    for (alignment, x, y) in [
        (9, 0.0, 0.0),
        (17, 0.0, 11.0),
        (18, 42.0, 11.0),
        (20, 83.0, 11.0),
        (10, 42.0, 0.0),
        (34, 42.0, 22.0),
    ] {
        let mut out = DrawList::default();
        painter.text(
            "grid",
            &[0],
            [0.0, 0.0, 100.0, 40.0],
            alignment,
            [1.0; 4],
            &mut out,
        );
        aligned.push((alignment, x, y, out.quads.first().map(|quad| quad.rect)));
    }
    let mut long = DrawList::default();
    painter.text(
        "grid",
        &[0; 30],
        [0.0, 0.0, 100.0, 40.0],
        9,
        [1.0; 4],
        &mut long,
    );
    let mut missing = DrawList::default();
    painter.border(
        &frame("missing_c"),
        [0.0, 0.0, 100.0, 50.0],
        [1.0; 4],
        &mut missing,
    );
    let refusals = painter.refusals.clone();
    let mut focused = DrawList::default();
    let state = ControlState {
        focused: true,
        ..ControlState::default()
    };
    painter.control(
        &button("BTN_A", [0, 0, 100, 40], &[0]),
        &state,
        &mut focused,
    );
    let popup = Gui {
        tag: "POPUP".into(),
        extent: Rect {
            left: 100,
            top: 50,
            width: 300,
            height: 200,
        },
        border: None,
        color: None,
        alpha: None,
        controls: vec![Control {
            border: Some(frame("c")),
            ..button("BTN_OK", [10, 10, 50, 20], b"")
        }],
        unread: Vec::new(),
    };
    let mut placed = DrawList::default();
    painter.layout(&popup, &|_| ControlState::default(), &mut placed);
    let backed = Gui {
        border: Some(frame("c")),
        ..popup.clone()
    };
    let mut backdrop = DrawList::default();
    painter.layout(&backed, &|_| ControlState::default(), &mut backdrop);
    let mut fitted = button("LB_MESSAGE", [0, 0, 100, 200], b"");
    fitted.kind = Kind::ListBox;
    fitted.proto = Some(Box::new(Control {
        kind: Kind::Label,
        ..button("PROTOITEM", [0, 0, 100, 18], b"")
    }));
    let essay = ControlState {
        list: Some(crate::paint::ListView {
            rows: vec![crate::paint::Row {
                text: vec![0; 15],
                ..crate::paint::Row::default()
            }],
            fit: true,
            ..crate::paint::ListView::default()
        }),
        ..ControlState::default()
    };
    let mut wrapped = DrawList::default();
    painter.control(&fitted, &essay, &mut wrapped);
    let lowest = wrapped
        .quads
        .iter()
        .map(|quad| quad.rect[1])
        .fold(0.0f32, f32::max);
    // The fake's thumb is square: a tall slider's is as wide as it.
    let thumbs: Vec<Option<[f32; 4]>> = [([0, 0, 200, 20], 0.5), ([0, 0, 64, 256], 0.75)]
        .into_iter()
        .map(|(rect, value)| {
            let slider = Control {
                kind: Kind::Slider,
                thumb: Some(fmt_odyssey::gui::Image {
                    image: "thumb".into(),
                    ..fmt_odyssey::gui::Image::default()
                }),
                ..button("SLD", rect, b"")
            };
            let state = ControlState {
                value: Some(value),
                ..ControlState::default()
            };
            let mut out = DrawList::default();
            painter.control(&slider, &state, &mut out);
            out.quads
                .iter()
                .find(|quad| quad.texture != TextureId(99))
                .map(|quad| quad.rect)
        })
        .collect();
    let mut list = button("LB_ITEMS", [0, 0, 200, 294], b"");
    list.kind = Kind::ListBox;
    list.padding = 5;
    list.proto = Some(Box::new(button("PROTOITEM", [0, 0, 200, 50], b"")));
    vec![
        check("border_nine_slice", {
            let quads = &nine.quads;
            expect(
                &format!("nine quads, not {}", quads.len()),
                quads.len() == 9,
            )
            .and_then(|()| expect("the fill inside", quads[0].rect == [4.0, 4.0, 92.0, 42.0]))
            .and_then(|()| {
                expect(
                    "the left edge turned to face out",
                    quads[1].rect == [0.0, 4.0, 4.0, 42.0] && quads[1].turns == 3,
                )
            })
            .and_then(|()| {
                expect(
                    "the bottom edge mirrored",
                    quads[4].rect == [4.0, 46.0, 92.0, 4.0] && quads[4].uv == [0.0, 1.0, 1.0, 0.0],
                )
            })
            .and_then(|()| {
                expect(
                    "the bottom-right corner mirrored both ways",
                    quads[8].rect == [96.0, 46.0, 4.0, 4.0] && quads[8].uv == [1.0, 1.0, 0.0, 0.0],
                )
            })
            .map(|()| "fill, edges, corners".into())
        }),
        check("text_alignment", {
            aligned
                .iter()
                .try_for_each(|(alignment, x, y, rect)| match rect {
                    Some(rect) if near(rect[0], *x) && near(rect[1], *y) => Ok(()),
                    other => Err(format!("{alignment}: expected ({x}, {y}), drew {other:?}")),
                })
                .map(|()| "9, 17, 18, 20, 10, 34".into())
        }),
        check("text_clips", {
            let within = [0.0, 0.0, 100.0, 40.0];
            expect(
                &format!("ten glyphs on two lines, not {}", long.quads.len()),
                long.quads.len() == 10,
            )
            .and_then(|()| {
                expect(
                    "every glyph inside the box",
                    long.quads.iter().all(|quad| inside(quad.rect, within)),
                )
            })
            .map(|()| "wrapped, then cut at the box".into())
        }),
        check("unknown_texture_refused", {
            expect(
                &format!("one refusal: {refusals:?}"),
                refusals.iter().any(|why| why == "texture missing_c"),
            )
            .and_then(|()| expect("the fill still drawn", missing.quads.len() == 5))
            .map(|()| "refused, the rest drawn".into())
        }),
        check("focus_turns_yellow", {
            let glyph = focused
                .quads
                .iter()
                .find(|quad| quad.texture == TextureId(99));
            match glyph {
                Some(quad) if quad.color[..3] == Palette::K1.hilight => Ok("hilight colour".into()),
                other => Err(format!("{other:?}")),
            }
        }),
        check("layout_root_relative", {
            let fill = placed.quads.first().map(|quad| quad.rect);
            expect(
                &format!("the frame 640 × 480, not {:?}", placed.size),
                placed.size == (640.0, 480.0),
            )
            .and_then(|()| {
                expect(
                    &format!("the fill at 114, 64 (root 100, 50 + 10 + 4): {fill:?}"),
                    fill == Some([114.0, 64.0, 42.0, 12.0]),
                )
            })
            .map(|()| "children placed from the root".into())
        }),
        check("layout_root_border", {
            let first = backdrop.quads.first().map(|quad| quad.rect);
            expect(
                &format!("the root's fill first, inside its border: {first:?}"),
                first == Some([104.0, 54.0, 292.0, 192.0]),
            )
            .map(|()| "the root's own backdrop".into())
        }),
        check("list_fit_rows", {
            expect(
                &format!(
                    "fifteen glyphs, five a line, on three lines: {} quads, lowest at {lowest}",
                    wrapped.quads.len()
                ),
                wrapped.quads.len() == 15 && near(lowest, 36.0),
            )
            .map(|()| "a row as tall as its text".into())
        }),
        check("slider_thumbs", {
            expect(
                &format!("wide: 20 square, halfway along 180: {:?}", thumbs[0]),
                thumbs[0] == Some([90.0, 0.0, 20.0, 20.0]),
            )
            .and_then(|()| {
                expect(
                    &format!("tall: 64 square, a quarter down 192: {:?}", thumbs[1]),
                    thumbs[1] == Some([0.0, 48.0, 64.0, 64.0]),
                )
            })
            .map(|()| "along the slider's long side".into())
        }),
        check("list_rows_fit", {
            let rows = rows_that_fit(&list, None);
            expect(&format!("5 rows of 50 + 5 in 294, not {rows}"), rows == 5).map(|()| "5".into())
        }),
    ]
}
