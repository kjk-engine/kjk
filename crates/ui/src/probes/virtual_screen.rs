//! The catalogue's virtual screen (`virtual_screen`): how far a pixel of
//! each device looks from the owner's chair, where the image stands in the
//! window in each mode, and that a click in the window lands on the control
//! it shows.

use super::menus::root;
use super::paint::{Fake, button};
use super::{Check, check, expect};
use crate::catalogue::{SIZES, Size};
use crate::input::UiInput;
use crate::intent::GuiSound;
use crate::screen::Stack;
use crate::virtual_screen::{
    Mode, Placement, View, Viewing, pixels_per_inch, place, seen_scale, viewing,
};
use crate::vm::ViewModel;

fn size(name: &str) -> Size {
    Size::named(name).expect("a size of the catalogue")
}

/// The scale of a device seen by the owner of a 254 pixel an inch display,
/// 60 centimetres away.
fn on_laptop(name: &str) -> f32 {
    let size = size(name);
    seen_scale((size, viewing(&size)), (254.0, 60.0))
}

fn seen_scales() -> Check {
    check("virtual_screen_seen_scale", {
        let (deck, tv) = (on_laptop("deck"), on_laptop("4k-tv"));
        let own = Viewing {
            inches: 24.0,
            distance_cm: 60.0,
        };
        let desk = size("1080p");
        let desk_density = pixels_per_inch(desk.pixels, own.inches);
        let same = seen_scale((desk, own), (desk_density, 60.0));
        let doubled = seen_scale((desk, own), (desk_density, 30.0));
        expect(
            &format!("a Steam Deck pixel on a 254 pixel an inch display at 60 cm: {deck}"),
            (deck - 2.02).abs() < 0.01,
        )
        .and_then(|()| {
            expect(
                &format!("a 55 inch 4K TV pixel seen from 2.5 m: {tv}"),
                (tv - 0.76).abs() < 0.01,
            )
        })
        .and_then(|()| {
            expect(
                &format!("a device seen as the owner sees their display: {same}"),
                (same - 1.0).abs() < 1e-4,
            )
        })
        .and_then(|()| {
            expect(
                &format!(
                    "the owner at half the distance sees the device pixel half as big: {doubled}"
                ),
                (doubled - 0.5).abs() < 1e-4,
            )
        })
        .map(|()| {
            format!("the Deck's pixel shows as {deck:.2} display pixels and the TV's as {tv:.2}")
        })
    })
}

fn view(mode: Mode, image: (u32, u32), window: (f32, f32), factor: f32) -> View {
    View {
        mode,
        image,
        window,
        window_factor: factor,
        seen: 2.0,
        pan: [0.0; 2],
    }
}

fn placements() -> Check {
    check("virtual_screen_placement", {
        let fit = place(&view(Mode::Fit, (3840, 2160), (1280.0, 800.0), 1.0));
        let pixel = place(&view(Mode::Pixel, (1280, 800), (1280.0, 800.0), 1.0));
        let retina = place(&view(Mode::Pixel, (1280, 800), (1280.0, 800.0), 2.0));
        let panned = place(&View {
            pan: [5000.0, 100.0],
            ..view(Mode::Pixel, (3840, 2160), (1280.0, 800.0), 1.0)
        });
        let seen = place(&view(Mode::Seen, (1280, 800), (1280.0, 800.0), 1.0));
        expect(
            &format!("fit keeps the shape and leaves bars above and below: {fit:?}"),
            fit.size == [1280.0, 720.0] && fit.origin == [0.0, 40.0],
        )
        .and_then(|()| {
            expect(
                &format!("fit finds nothing in the bars: {:?} {:?}", fit.to_image([10.0, 10.0]), fit.to_image([10.0, 100.0])),
                fit.to_image([10.0, 10.0]).is_none() && fit.to_image([10.0, 100.0]) == Some([30.0, 180.0]),
            )
        })
        .and_then(|()| {
            expect(
                &format!("pixel for pixel is one device pixel to one display pixel: {pixel:?}"),
                pixel.scale == 1.0 && pixel.origin == [0.0, 0.0],
            )
        })
        .and_then(|()| {
            expect(
                &format!("on a display of two pixels to a point the image takes half the points: {retina:?}"),
                retina.size == [640.0, 400.0] && retina.origin == [320.0, 200.0],
            )
        })
        .and_then(|()| {
            expect(
                &format!("an image larger than the window is panned within itself: {panned:?}"),
                panned.pan == [2560.0, 100.0] && panned.origin == [-2560.0, -100.0],
            )
        })
        .and_then(|()| {
            expect(
                &format!("as seen scales by the seen factor: {seen:?}"),
                seen.size == [2560.0, 1600.0] && seen.pan == [0.0, 0.0] && seen.origin == [0.0, 0.0],
            )
        })
        .map(|()| "fit, pixel for pixel and as seen put the image where they say".into())
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

/// In each size and mode, a click at the window point that shows a control
/// lands on that control, and a point in the bars lands on none.
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
            for mode in Mode::ALL {
                for tag in ["BTN_A", "BTN_B"] {
                    let mut opened = staged(size);
                    let rect = rects
                        .iter()
                        .find(|(name, _)| *name == tag)
                        .expect("a control")
                        .1;
                    let at = opened.middle_of(rect);
                    let mut shown = View {
                        seen: seen_scale((size, viewing(&size)), (254.0, 60.0)),
                        ..view(mode, size.pixels, window, factor)
                    };
                    // The window is panned to put the control in its middle.
                    let unpanned = place(&shown).to_window(at);
                    shown.pan = [unpanned[0] - window.0 / 2.0, unpanned[1] - window.1 / 2.0];
                    let placement = place(&shown);
                    let point = placement.to_window(at);
                    let Some(pixel) = placement.to_image(point) else {
                        result = Err(format!(
                            "{} {}: the control's point is outside the image",
                            size.name,
                            mode.name()
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
                            "{} {} pan {:?}: a click on {tag} at the window's {point:?}, the image's {pixel:?}, focused {:?} and sounded {:?}",
                            size.name,
                            mode.name(),
                            placement.pan,
                            opened.stack.focused(),
                            frame.sounds
                        ));
                        break 'sizes;
                    }
                    tried += 1;
                }
            }
        }
        result.map(|()| format!("{tried} clicks, two buttons in each of six sizes and three modes, land on the control they show"))
    })
}

/// The point of the window that shows a point of the image shows it again
/// when taken back, in every mode and pan.
fn round_trip() -> Check {
    check("virtual_screen_round_trip", {
        let mut worst = 0.0f32;
        for size in SIZES {
            for mode in Mode::ALL {
                for factor in [1.0, 1.5, 2.0] {
                    for pan in [[0.0, 0.0], [321.0, 77.0], [1e6, 1e6]] {
                        let shown = View {
                            seen: seen_scale((size, viewing(&size)), (254.0, 60.0)),
                            pan,
                            ..view(mode, size.pixels, (1280.0, 800.0), factor)
                        };
                        let placement: Placement = place(&shown);
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
                                    // Out of the window is still in the image's rectangle.
                                    let window = placement.to_window(pixel);
                                    if window[0] < placement.origin[0]
                                        || window[1] < placement.origin[1]
                                    {
                                        return check(
                                            "virtual_screen_round_trip",
                                            Err(format!(
                                                "{} {}: {pixel:?} is left of the image",
                                                size.name,
                                                mode.name()
                                            )),
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        expect(&format!("the point taken back is {worst} pixels off"), worst < 1e-2)
            .map(|()| format!("a point of the image taken to the window and back is off by at most {worst:.4} pixels"))
    })
}

pub(super) fn all() -> Vec<Check> {
    vec![seen_scales(), placements(), clicks_land(), round_trip()]
}
