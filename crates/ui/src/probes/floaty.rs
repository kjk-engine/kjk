//! Floating words: centred over their point, 32 pixels up, ringed in
//! black, fading; a newer one over the same creature under the older.

use super::paint::Fake;
use super::{Check, check, expect, near};
use crate::draw::{Quad, Textures};
use crate::paint::{Painter, Palette};
use crate::screens::floaty::{XP_COLOR, draw};
use crate::skin::Source;
use crate::vm::FloatyView;

pub(super) fn all() -> Vec<Check> {
    let mut fake = Fake::new();
    let (_, font) = fake.font(Source::Campaign, "grid").expect("the test font");
    let line = font.line_height();
    let floaty = |stacked| FloatyView {
        text: b"\x00\x01".to_vec(),
        color: XP_COLOR,
        alpha: 0.5,
        over: [0.5, 0.5],
        stacked,
    };
    let quads = draw(
        &mut Painter::new(&mut fake, Palette::K1),
        &[floaty(1), floaty(0)],
        (1280.0, 720.0),
    )
    .quads;
    // Each glyph five times: four rings, then the words.
    let glyphs = quads.len() / 10;
    let words = |at: usize| &quads[(at * 5 + 4) * glyphs..(at * 5 + 5) * glyphs];
    let rings = &quads[..4 * glyphs];
    let top = |quads: &[Quad]| quads.iter().map(|q| q.rect[1]).fold(f32::MAX, f32::min);
    let middle = |quads: &[Quad]| {
        let left = quads.iter().map(|q| q.rect[0]).fold(f32::MAX, f32::min);
        let right = quads
            .iter()
            .map(|q| q.rect[0] + q.rect[2])
            .fold(0.0, f32::max);
        (left + right) / 2.0
    };
    let (older, newer) = (words(0), words(1));
    vec![check(
        "floaty_over_its_creature",
        (|| {
            expect(
                &format!("two glyphs, ten draws each: {}", quads.len()),
                glyphs == 2,
            )?;
            expect(
                &format!(
                    "the newer's line 32 px over the point, the older's a line higher: {}, {}",
                    top(newer),
                    top(older)
                ),
                near(top(newer), (360.0 - 32.0 - line).round())
                    && near(top(older), (360.0 - 32.0 - 2.0 * line).round()),
            )?;
            expect(
                &format!("centred across: {}", middle(newer)),
                (middle(newer) - 640.0).abs() <= 1.0,
            )?;
            let ring_at: Vec<[f32; 2]> = (0..4)
                .map(|at| {
                    let ring = &rings[at * glyphs];
                    [
                        ring.rect[0] - older[0].rect[0],
                        ring.rect[1] - older[0].rect[1],
                    ]
                })
                .collect();
            expect(
                &format!("ringed at the diagonals: {ring_at:?}"),
                ring_at == [[-1.0, 1.0], [1.0, 1.0], [1.0, -1.0], [-1.0, -1.0]],
            )?;
            expect(
                "the ring black, the words magenta, both half faded",
                rings.iter().all(|q| q.color == [0.0, 0.0, 0.0, 0.5])
                    && newer.iter().all(|q| q.color == [0.95, 0.0, 0.85, 0.5]),
            )?;
            Ok(format!(
                "line {line}, tops {} and {}",
                top(newer),
                top(older)
            ))
        })(),
    )]
}
