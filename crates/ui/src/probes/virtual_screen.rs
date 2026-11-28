//! The catalogue's virtual screen (`virtual_screen`): where the image stands
//! in the window, and that a click in the window lands on the control it
//! shows.

use super::menus::root;
use super::paint::{Fake, button};
use super::{Check, check, expect};
use crate::catalogue::{SIZES, Size};
use crate::input::UiInput;
use crate::intent::GuiSound;
use crate::screen::Stack;
use crate::virtual_screen::{Placement, View, place};
use crate::vm::ViewModel;

fn size(name: &str) -> Size {
    Size::named(name).expect("a size of the catalogue")
}

fn view(image: (u32, u32), window: (f32, f32), factor: f32) -> View {
    View {
        image,
        window,
        window_factor: factor,
        margin: [0.0; 4],
    }
}

fn placements() -> Check {
    check("virtual_screen_placement", {
        let wide = place(&view((3840, 2160), (1280.0, 800.0), 1.0));
        let small = place(&view((1280, 800), (3024.0, 1964.0), 1.0));
        let retina = place(&view((1280, 800), (1512.0, 982.0), 2.0));
        expect(
            &format!("an image wider than the window keeps its shape, with bars above and below: {wide:?}"),
            wide.size == [1280.0, 720.0] && wide.origin == [0.0, 40.0],
        )
        .and_then(|()| {
            expect(
                &format!("nothing is found in the bars: {:?} {:?}", wide.to_image([10.0, 10.0]), wide.to_image([10.0, 100.0])),
                wide.to_image([10.0, 10.0]).is_none() && wide.to_image([10.0, 100.0]) == Some([30.0, 180.0]),
            )
        })
        .and_then(|()| {
            expect(
                &format!("an image smaller than the window grows to fill it: {small:?}"),
                (small.scale - 3024.0 / 1280.0).abs() < 1e-4 && small.size[0] == 3024.0,
            )
        })
        .and_then(|()| {
            let corner = [retina.origin[0] * 2.0, retina.origin[1] * 2.0];
            expect(
                &format!("on a display of two pixels to a point the corner is on a whole pixel: {retina:?}"),
                corner[0] == corner[0].round() && corner[1] == corner[1].round(),
            )
        })
        .map(|()| "the image fits the window, keeps its shape and stands in the middle".into())
    })
}

/// A screen of two buttons, opened in a window of the device's size.
struct Opened {
    assets: Fake,
    vm: ViewModel,
    stack: Stack,
    quiet: UiInput,
    size: Size,
}

fn staged(size: Size) -> Opened {
    let mut assets = Fake::new();
    assets.layouts.insert(
        "layout_a",
        root(
            640,
            480,
            vec![
                button("BTN_A", [0, 0, 80, 20], b""),
                button("BTN_B", [0, 40, 80, 20], b""),
            ],
        ),
    );
    let vm = ViewModel::default();
    let mut stack = Stack::new(crate::screens::open);
    stack.push(Box::new(crate::screens::Layout("layout_a".into())));
    let quiet = UiInput {
        window: size.window(),
        scale_factor: size.factor,
        dt: 0.016,
        ..UiInput::default()
    };
    stack.step(&mut assets, &vm, &quiet);
    Opened {
        assets,
        vm,
        stack,
        quiet,
        size,
    }
}

impl Opened {
    /// The middle of the control `rect` of the layout, in the device's pixels.
    fn middle_of(&mut self, rect: [f32; 4]) -> [f32; 2] {
        let layer = self
            .stack
            .step(&mut self.assets, &self.vm, &self.quiet)
            .layers
            .remove(0);
        let (scale, [ox, oy]) = layer.placed(self.size.window());
        let [x, y, w, h] = rect;
        [
            (ox + (x + w / 2.0) * scale) * self.size.factor,
            (oy + (y + h / 2.0) * scale) * self.size.factor,
        ]
    }
}

/// In each size, a click at the window point that shows a control lands on
/// that control.
fn clicks_land() -> Check {
    let rects = [
        ("BTN_A", [0.0, 0.0, 80.0, 20.0]),
        ("BTN_B", [0.0, 40.0, 80.0, 20.0]),
    ];
    // A laptop's window: 1280 × 800 points of two physical pixels each.
    let (window, factor) = ((1280.0, 800.0), 2.0);
    check("virtual_screen_clicks_land", {
        let mut tried = 0;
        let mut result = Ok(());
        'sizes: for size in SIZES {
            for tag in ["BTN_A", "BTN_B"] {
                let mut opened = staged(size);
                let rect = rects
                    .iter()
                    .find(|(name, _)| *name == tag)
                    .expect("a control")
                    .1;
                let at = opened.middle_of(rect);
                let placement = place(&view(size.pixels, window, factor));
                let point = placement.to_window(at);
                let Some(pixel) = placement.to_image(point) else {
                    result = Err(format!(
                        "{}: the control's point is outside the image",
                        size.name
                    ));
                    break 'sizes;
                };
                let pointer = [pixel[0] / size.factor, pixel[1] / size.factor];
                let click = UiInput {
                    pointer: Some(pointer),
                    moved: true,
                    click: true,
                    ..opened.quiet.clone()
                };
                let frame = opened.stack.step(&mut opened.assets, &opened.vm, &click);
                let landed = opened.stack.focused() == Some(tag)
                    && frame.sounds.contains(&GuiSound::Clicked);
                if !landed {
                    result = Err(format!(
                        "{}: a click on {tag} at the window's {point:?}, the image's {pixel:?}, focused {:?} and sounded {:?}",
                        size.name,
                        opened.stack.focused(),
                        frame.sounds
                    ));
                    break 'sizes;
                }
                tried += 1;
            }
        }
        result.map(|()| {
            format!(
                "{tried} clicks, two buttons in each of five sizes, land on the control they show"
            )
        })
    })
}

/// The point of the window that shows a point of the image shows it again
/// when taken back.
fn round_trip() -> Check {
    check("virtual_screen_round_trip", {
        let mut worst = 0.0f32;
        for size in SIZES {
            for factor in [1.0, 1.5, 2.0] {
                let placement: Placement = place(&view(size.pixels, (1280.0, 800.0), factor));
                for pixel in [
                    [0.5, 0.5],
                    [size.pixels.0 as f32 / 2.0, size.pixels.1 as f32 / 2.0],
                    [size.pixels.0 as f32 - 0.5, size.pixels.1 as f32 - 0.5],
                ] {
                    let back = placement.to_image(placement.to_window(pixel));
                    match back {
                        Some(back) => {
                            worst = worst
                                .max((back[0] - pixel[0]).abs())
                                .max((back[1] - pixel[1]).abs())
                        }
                        None => {
                            return check(
                                "virtual_screen_round_trip",
                                Err(format!(
                                    "{}: {pixel:?} is not in the image once taken back",
                                    size.name
                                )),
                            );
                        }
                    }
                }
            }
        }
        expect(&format!("the point taken back is {worst} pixels off"), worst < 1e-2)
            .map(|()| format!("a point of the image taken to the window and back is off by at most {worst:.4} pixels"))
    })
}

/// Fit leaves room for the device's frame, the frame has the device's real
/// proportions, and a point on the frame is not a point of the image.
fn frames() -> Check {
    check("virtual_screen_frame", {
        let deck = size("deck");
        let [left, top, right, bottom] = crate::device_frame::margin(&deck);
        let screen = crate::device_frame::screen_mm(&deck);
        let per_mm = deck.pixels.0 as f32 / screen[0];
        let body = [
            (deck.pixels.0 as f32 + left + right) / per_mm,
            (deck.pixels.1 as f32 + top + bottom) / per_mm,
        ];
        let mut result = expect(
            &format!("a Steam Deck's body is 298 × 117 mm: {body:?}"),
            (body[0] - 298.0).abs() < 0.5 && (body[1] - 117.0).abs() < 0.5,
        );
        for size in SIZES {
            let margin = crate::device_frame::margin(&size);
            let window = (1500.0, 900.0);
            let placed = place(&View {
                margin,
                ..view(size.pixels, window, 1.0)
            });
            let reach = [
                placed.origin[0] - margin[0] * placed.scale,
                placed.origin[1] - margin[1] * placed.scale,
                placed.origin[0] + placed.size[0] + margin[2] * placed.scale,
                placed.origin[1] + placed.size[1] + margin[3] * placed.scale,
            ];
            let inside = reach[0] >= -1.0
                && reach[1] >= -1.0
                && reach[2] <= window.0 + 1.0
                && reach[3] <= window.1 + 1.0;
            let on_frame = placed.to_image([placed.origin[0] - 2.0, placed.origin[1] + 5.0]);
            result = result.and_then(|()| {
                expect(
                    &format!(
                        "{}: fit keeps the whole frame in the window: {reach:?}",
                        size.name
                    ),
                    inside,
                )
            });
            result = result.and_then(|()| {
                expect(
                    &format!(
                        "{}: the frame beside the image is not the image: {on_frame:?}",
                        size.name
                    ),
                    on_frame.is_none(),
                )
            });
        }
        result.map(|()| {
            "a frame keeps the device's size, fits the window, and takes no clicks".into()
        })
    })
}

pub(super) fn all() -> Vec<Check> {
    vec![placements(), clicks_land(), round_trip(), frames()]
}
