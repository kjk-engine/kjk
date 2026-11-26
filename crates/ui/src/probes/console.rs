//! The console's overlay: drawn in KOTOR 2's frame, mistakes in red, the
//! caret blinking, Tab's choices lit and found under the pointer, and the
//! log's scroll bar shown once the log is longer than the console.

use std::sync::Arc;

use asset_odyssey::font::Font;
use fmt_odyssey::gui::{Control, Image, Kind};
use fmt_odyssey::txi::Txi;

use super::paint::{Fake, button};
use super::{Check, check, expect};
use crate::draw::{DrawList, Quad};
use crate::paint::{Painter, Palette, RED};
use crate::screens::console::{FONT, choice_at, draw};
use crate::vm::{ConsoleChoice, ConsoleLine, ConsoleView};

const WINDOW: (f32, f32) = (1280.0, 720.0);

/// A font with a glyph for every ASCII byte, all of them the same square.
fn ascii_font() -> Font {
    const LETTERS: usize = 128;
    let mut txi = format!(
        "numchars {LETTERS}\nfontheight 0.16\nspacingR 0.01\nspacingB 0.02\nupperleftcoords {LETTERS}\n"
    );
    txi.push_str(&"0 1 0\n".repeat(LETTERS));
    txi.push_str(&format!("lowerrightcoords {LETTERS}\n"));
    txi.push_str(&"0.5 0.5 0\n".repeat(LETTERS));
    Font::from_txi("ascii", &Txi::parse(txi.as_bytes()), 32, 32).expect("the ASCII font reads")
}

/// A scroll bar with a thumb, as KOTOR 2's Messages page has.
fn bar() -> Control {
    Control {
        kind: Kind::ScrollBar,
        thumb: Some(Image {
            image: "lbt_thumb".into(),
            draw_style: 0,
            flip_style: 0,
            rotate: 0.0,
            alignment: 0,
        }),
        ..button("SCROLLBAR", [0, 0, 16, 100], b"")
    }
}

fn drawn(fake: &mut Fake, view: &ConsoleView, bar: Option<&Control>) -> DrawList {
    draw(&mut Painter::new(fake, Palette::TSL), view, WINDOW, bar)
}

/// How many of `out`'s quads are drawn in `rgb`.
fn coloured(out: &DrawList, rgb: [f32; 3]) -> usize {
    let near = |quad: &&Quad| (0..3).all(|at| (quad.color[at] - rgb[at]).abs() < 1e-3);
    out.quads.iter().filter(near).count()
}

pub(super) fn all() -> Vec<Check> {
    let mut fake = Fake::new();
    fake.font = Arc::new(ascii_font());
    fake.fonts.insert(FONT);
    let line = |words: &[u8], error: bool| ConsoleLine {
        text: words.to_vec(),
        error,
    };
    let mistake = b"Unknown command";
    let view = ConsoleView {
        log: vec![line(b"> help", false), line(mistake, true)],
        input: b"give".to_vec(),
        caret: 1,
        ..ConsoleView::default()
    };
    let still = drawn(&mut fake, &view, None);
    let blinking = drawn(
        &mut fake,
        &ConsoleView {
            blink: true,
            ..view.clone()
        },
        None,
    );
    let rim = still.quads.iter().any(|quad| {
        fake.named(quad.texture)
            .is_some_and(|name| name.starts_with("uibit_brdr_16w"))
    });
    let red = coloured(&still, RED);
    let caret = blinking.quads.len() - still.quads.len();
    let choosing = ConsoleView {
        candidates: ["giveitem", "giveitems", "givecredits"]
            .map(|word| ConsoleChoice {
                word: word.as_bytes().to_vec(),
                name: Vec::new(),
            })
            .to_vec(),
        chosen: Some(1),
        ..view.clone()
    };
    let chosen = drawn(&mut fake, &choosing, None);
    let lit = coloured(&chosen, Palette::TSL.hilight) - coloured(&still, Palette::TSL.hilight);
    // The pointer on each of the three rows, then far below the console.
    let mut painter = Painter::new(&mut fake, Palette::TSL);
    let mut found = Vec::new();
    for y in 0..WINDOW.1 as i32 {
        if let Some(row) = choice_at(&mut painter, &choosing, WINDOW, [40.0, y as f32])
            && found.last() != Some(&row)
        {
            found.push(row);
        }
    }
    let nowhere = choice_at(&mut painter, &choosing, WINDOW, [40.0, 700.0]);
    let long = ConsoleView {
        log: (0..200).map(|_| line(b"x", false)).collect(),
        ..view.clone()
    };
    let thumb = |out: &DrawList, fake: &Fake| {
        (out.quads.iter()).any(|quad| fake.named(quad.texture).as_deref() == Some("lbt_thumb"))
    };
    let short_bar = drawn(&mut fake, &view, Some(&bar()));
    let long_bar = drawn(&mut fake, &long, Some(&bar()));
    let bars = (thumb(&short_bar, &fake), thumb(&long_bar, &fake));
    vec![check(
        "console_overlay",
        expect("the console wears KOTOR 2's rim", rim)
            .and_then(|()| {
                expect(
                    &format!("the mistake's letters in red: {red}"),
                    red == mistake.len(),
                )
            })
            .and_then(|()| {
                expect(
                    &format!("the caret blinks as one more letter: {caret}"),
                    caret == 1,
                )
            })
            .and_then(|()| {
                expect(
                    &format!("the chosen choice's letters lit: {lit}"),
                    lit == b"giveitems".len(),
                )
            })
            .and_then(|()| {
                expect(
                    &format!("the pointer finds the rows in order: {found:?}, below {nowhere:?}"),
                    found == [0, 1, 2] && nowhere.is_none(),
                )
            })
            .and_then(|()| {
                expect(
                    &format!("the thumb shows for a long log alone: {bars:?}"),
                    bars == (false, true),
                )
            })
            .map(|()| "rim, red mistakes, caret, lit choice, pointer, scroll bar".into()),
    )]
}
