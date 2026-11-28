//! What a control tree is made from (`kjk gui screen`): each control's
//! mark with its words, and where the focus moves from each control. And
//! the design tokens the palettes must match (`data/ui/tokens.json`).

use fmt_odyssey::gui::{Gui, MoveTo, Rect};

use super::paint::{Fake, button};
use super::{Check, check, expect};
use crate::draw::DrawList;
use crate::paint::{Brush, ControlState, Painter, Palette};
use crate::screen::layout_links;
use crate::skin::Source;

fn layout(left: i32, top: i32, controls: Vec<fmt_odyssey::gui::Control>) -> Gui {
    Gui {
        tag: "ROOT".into(),
        extent: Rect {
            left,
            top,
            width: 400,
            height: 300,
        },
        border: None,
        color: None,
        alpha: None,
        controls,
        unread: Vec::new(),
    }
}

pub(super) fn all() -> Vec<Check> {
    let mut fake = Fake::new();
    let gui = layout(
        100,
        50,
        vec![
            button("BTN_A", [10, 20, 80, 30], b"OK"),
            button("BTN_B", [200, 20, 80, 30], b""),
        ],
    );
    let mut marked = DrawList::default();
    let mut painter = Brush::new(Palette::K1, 1.0, true).painter(&mut fake, Source::Campaign);
    painter.layout(&gui, &|_| ControlState::default(), &mut marked);
    let mut plain = DrawList::default();
    let mut painter = Painter::new(&mut fake, Palette::K1);
    painter.layout(&gui, &|_| ControlState::default(), &mut plain);

    let mut a = button("BTN_A", [0, 0, 50, 30], b"");
    a.id = 1;
    let mut b = button("BTN_B", [100, 0, 50, 30], b"");
    b.id = 2;
    let mut c = button("BTN_C", [0, 100, 50, 30], b"");
    c.id = 3;
    c.moveto = Some(MoveTo {
        up: 2,
        down: -1,
        left: -1,
        right: -1,
    });
    let links = layout_links(&layout(0, 0, vec![a, b, c]));
    let to = |from: &str| {
        links
            .iter()
            .find(|(tag, _)| tag == from)
            .map(|(_, to)| to.clone())
    };
    let some = |tag: &str| Some(tag.to_string());
    vec![
        check("palette_tokens", palette_tokens()),
        check("tree_marks", {
            let tags: Vec<&str> = marked.marks.iter().map(|mark| mark.tag.as_str()).collect();
            let a = marked.marks.iter().find(|mark| mark.tag == "BTN_A");
            let words = a.and_then(|mark| mark.words.first());
            (|| {
                expect(
                    "the root and both buttons leave a mark each",
                    tags == ["ROOT", "BTN_A", "BTN_B"],
                )?;
                expect(
                    "a mark sits where its control was drawn, past the root's corner",
                    a.is_some_and(|mark| mark.rect == [110.0, 70.0, 80.0, 30.0]),
                )?;
                expect(
                    "a mark holds the words its control set, where they were set",
                    words.is_some_and(|words| {
                        words.bytes == b"OK" && words.font_name == "grid" && words.rect[0] >= 110.0
                    }),
                )?;
                expect(
                    "a painter not asked for marks leaves none",
                    plain.marks.is_empty(),
                )?;
                expect("marks change nothing drawn", plain.quads == marked.quads)?;
                Ok(format!("{tags:?}"))
            })()
        }),
        check("tree_body_height", tree_body_height()),
        check("tree_focus_links", {
            (|| {
                expect(
                    "with no MOVETO the focus goes to the nearest control that way",
                    to("BTN_A") == Some([None, some("BTN_C"), None, some("BTN_B")]),
                )?;
                expect(
                    "a MOVETO link wins over the nearest control",
                    to("BTN_C").is_some_and(|to| to[0] == some("BTN_B")),
                )?;
                Ok(format!("{links:?}"))
            })()
        }),
    ]
}

/// A body height counts only the rows that stand out from the background,
/// so a dark outline counts on a light background and not on a dark one.
fn tree_body_height() -> Result<String, String> {
    use crate::render::{contrast, ink_rows};
    let (black, grey, white) = ([0u8; 3], [128u8; 3], [255u8; 3]);
    let strip: Vec<u8> = [black, black, white, white, white, black, black]
        .iter()
        .flat_map(|&[r, g, b]| [r, g, b, 255])
        .collect();
    expect(
        "black on white has WCAG's contrast of 21 to 1",
        (contrast(black, white) - 21.0).abs() < 0.01,
    )?;
    let on_black = ink_rows(&strip, 1, black);
    let on_grey = ink_rows(&strip, 1, grey);
    expect(
        &format!("on black, only the 3 white rows count, not {on_black:?}"),
        on_black == Some(3),
    )?;
    expect(
        &format!("on grey, the outline counts too: 7 rows, not {on_grey:?}"),
        on_grey == Some(7),
    )?;
    expect(
        "a strip of the background's own colour has no body",
        ink_rows(&[0, 0, 0, 255], 1, black).is_none(),
    )?;
    Ok(format!("{on_black:?} rows on black, {on_grey:?} on grey"))
}

/// Every palette in the code has the colours `data/ui/tokens.json` gives
/// it, to a 255th.
fn palette_tokens() -> Result<String, String> {
    use crate::paint::Palette;
    let tokens: serde_json::Value =
        serde_json::from_str(include_str!("../../../../data/ui/tokens.json"))
            .map_err(|error| format!("tokens.json: {error}"))?;
    let colour = |path: &str| -> Option<[f32; 3]> {
        let mut at = &tokens["color"];
        for part in path.split('.') {
            at = &at[part];
        }
        let components = at["$value"]["components"].as_array()?;
        let get = |i: usize| components.get(i)?.as_f64().map(|c| c as f32);
        Some([get(0)?, get(1)?, get(2)?])
    };
    let same = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).all(|(a, b)| (a - b).abs() < 0.5 / 255.0);
    let palettes = [
        ("k1", Palette::K1),
        ("tsl", Palette::TSL),
        ("tsl-layout", crate::skin::TSL_LAYOUT),
        ("tsl-engine", crate::skin::TSL_ENGINE),
    ];
    for (name, palette) in palettes {
        for (part, rgb) in [
            ("base", palette.base),
            ("hilight", palette.hilight),
            ("disabled", palette.disabled),
        ] {
            let token =
                colour(&format!("{name}.{part}")).ok_or(format!("no token {name}.{part}"))?;
            expect(
                &format!("{name}.{part} is {token:?} in the tokens and {rgb:?} in the code"),
                same(token, rgb),
            )?;
        }
    }
    let refused = colour("refused").ok_or("no token refused")?;
    expect(
        "the refused colour matches",
        same(refused, crate::paint::RED),
    )?;
    Ok(format!(
        "{} palettes and the refused colour",
        palettes.len()
    ))
}
