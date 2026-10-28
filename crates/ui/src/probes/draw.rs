//! The CPU raster: placement, tint, clip, turn.

use std::f32::consts::FRAC_PI_2;

use crate::draw::{DrawList, Quad, TextureId, turn};
use crate::raster::{Pixels, raster};

use super::{Check, check, expect};

fn white_quad(rect: [f32; 4]) -> Quad {
    Quad::solid(rect, [1.0; 4])
}

/// The RGBA of pixel (x, y) in a raster `width` wide.
pub(super) fn pixel(rgba: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let at = ((y * width + x) * 4) as usize;
    [rgba[at], rgba[at + 1], rgba[at + 2], rgba[at + 3]]
}

pub(super) fn rasters() -> Vec<Check> {
    let quarter = DrawList {
        size: (640.0, 480.0),
        fill: false,
        quads: vec![white_quad([0.0, 0.0, 320.0, 240.0])],
    };
    let none = |_: TextureId| -> Option<&Pixels> { None::<&Pixels> };
    let red = Pixels {
        width: 2,
        height: 2,
        rgba: [255u8, 255, 255, 255].repeat(4),
        shape: false,
    };
    let tinted = DrawList {
        size: (640.0, 480.0),
        fill: false,
        quads: vec![Quad {
            texture: TextureId(1),
            color: [1.0, 0.0, 0.0, 1.0],
            ..white_quad([0.0, 0.0, 640.0, 480.0])
        }],
    };
    let clipped = DrawList {
        size: (640.0, 480.0),
        fill: false,
        quads: vec![Quad {
            clip: Some([0.0, 0.0, 100.0, 100.0]),
            ..white_quad([0.0, 0.0, 640.0, 480.0])
        }],
    };
    // A bar 100 by 20 about the middle turned a quarter: 20 by 100; with
    // its left half clipped first, its top half.
    let bar = Quad {
        spin: FRAC_PI_2,
        ..white_quad([270.0, 230.0, 100.0, 20.0])
    };
    let turned = |clip: Option<[f32; 4]>| DrawList {
        size: (640.0, 480.0),
        fill: false,
        quads: vec![Quad { clip, ..bar }],
    };
    let right = turn([1.0, 0.0], [0.0, 0.0], FRAC_PI_2);
    vec![
        check("raster_turned", {
            let whole = raster(&turned(None), &none, 640, 480, [0, 0, 0, 255]);
            let half = raster(
                &turned(Some([270.0, 230.0, 50.0, 20.0])),
                &none,
                640,
                480,
                [0, 0, 0, 255],
            );
            let seen = |rgba: &[u8], x, y| pixel(rgba, 640, x, y)[0] == 255;
            let bar = (seen(&whole, 320, 200), seen(&whole, 360, 240));
            let clipped = (seen(&half, 320, 220), seen(&half, 320, 260));
            expect(
                &format!("right turns to down on the screen: {right:?}"),
                right[0].abs() < 1e-6 && (right[1] - 1.0).abs() < 1e-6,
            )
            .and(expect(
                &format!("standing, not lying: {bar:?}"),
                bar == (true, false),
            ))
            .and(expect(
                &format!("clipped upright, then turned: {clipped:?}"),
                clipped == (true, false),
            ))
            .map(|()| "a quarter turn clockwise, after the clip".into())
        }),
        check("raster_fill_quarter", {
            let rgba = raster(&quarter, &none, 640, 480, [0, 0, 0, 255]);
            let inside = pixel(&rgba, 640, 100, 100);
            let outside = pixel(&rgba, 640, 400, 300);
            expect(&format!("inside {inside:?}"), inside == [255; 4])
                .and(expect(
                    &format!("outside {outside:?}"),
                    outside == [0, 0, 0, 255],
                ))
                .map(|()| "the top-left quarter".into())
        }),
        check("raster_centred", {
            let rgba = raster(&quarter, &none, 1280, 480, [0, 0, 0, 255]);
            let margin = pixel(&rgba, 1280, 100, 100);
            let start = pixel(&rgba, 1280, 330, 10);
            expect(&format!("the margin {margin:?}"), margin == [0, 0, 0, 255])
                .and(expect(
                    &format!("the layout starts at 320: {start:?}"),
                    start == [255; 4],
                ))
                .map(|()| "320 px each side".into())
        }),
        check("raster_tint", {
            let lookup = |id: TextureId| (id == TextureId(1)).then_some(&red);
            let rgba = raster(&tinted, &lookup, 64, 48, [0, 0, 0, 255]);
            let middle = pixel(&rgba, 64, 32, 24);
            expect(&format!("red, not {middle:?}"), middle == [255, 0, 0, 255])
                .map(|()| "white × red".into())
        }),
        check("raster_clip", {
            let rgba = raster(&clipped, &none, 640, 480, [0, 0, 0, 255]);
            let inside = pixel(&rgba, 640, 50, 50);
            let outside = pixel(&rgba, 640, 150, 50);
            expect("inside the clip", inside == [255; 4])
                .and(expect("outside the clip", outside == [0, 0, 0, 255]))
                .map(|()| "100 × 100".into())
        }),
    ]
}
