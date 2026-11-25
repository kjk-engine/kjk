//! What a control tree is made from (`kjk gui screen`): each control's
//! mark with its words, and where the focus moves from each control.

use fmt_odyssey::gui::{Gui, MoveTo, Rect};

use super::paint::{Fake, button};
use super::{Check, check, expect};
use crate::draw::DrawList;
use crate::paint::{ControlState, Painter, Palette};
use crate::screen::layout_links;

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
    let mut painter = Painter::new(&mut fake, Palette::K1);
    painter.marked = true;
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
