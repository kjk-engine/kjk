//! The screen stack: focus by MOVETO and by direction, the pointer through
//! a scaled window, presses, going back, edit boxes, the pulse.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use fmt_odyssey::gui::{Border, Control, Gui, Image, Kind, MoveTo, Rect};

use super::paint::{Fake, button};
use super::{Check, check, expect, near};
use crate::draw::{IMAGE_ASPECT, MenuWidth, TextureId};
use crate::edit::EditBox;
use crate::input::{Key, UiInput};
use crate::intent::Intent;
use crate::paint::{ControlState, ListView, Row};
use crate::screen::{Frame, Screen, ScreenId, Stack};
use crate::vm::ViewModel;

fn root(controls: Vec<Control>) -> Gui {
    Gui {
        tag: "ROOT".into(),
        extent: Rect {
            left: 0,
            top: 0,
            width: 640,
            height: 480,
        },
        border: None,
        color: None,
        alpha: None,
        controls,
        unread: Vec::new(),
    }
}

fn linked(tag: &str, id: i32, rect: [i32; 4], down: i32) -> Control {
    Control {
        id,
        moveto: Some(MoveTo {
            up: -1,
            down,
            left: -1,
            right: -1,
        }),
        ..button(tag, rect, b"x")
    }
}

/// A screen over `layout` that quits on `BTN_C` and edits `EDIT`.
struct Probe {
    layout: &'static str,
    edit: EditBox,
}

impl Screen for Probe {
    fn id(&self) -> ScreenId {
        ScreenId("probe")
    }
    fn layout(&self) -> &str {
        self.layout
    }
    fn press(&mut self, tag: &str, _: &ViewModel, out: &mut Vec<Intent>) {
        if tag == "BTN_C" {
            out.push(Intent::Quit);
        }
    }
    fn edit(&mut self, tag: &str) -> Option<&mut EditBox> {
        (tag == "EDIT").then_some(&mut self.edit)
    }
}

fn no_screens(_: &str, _: &ViewModel) -> Option<Box<dyn Screen>> {
    None
}

fn stack_on(layout: &'static str) -> Stack {
    let mut stack = Stack::new(no_screens);
    stack.push(Box::new(Probe {
        layout,
        edit: EditBox::new(8),
    }));
    stack
}

fn keys(keys: &[Key]) -> UiInput {
    UiInput {
        keys: keys.to_vec(),
        window: (640.0, 480.0),
        dt: 0.016,
        ..UiInput::default()
    }
}

/// A screen over `scroll` whose list `LIST` has `SCROLLED` rows, five
/// shown; the row its cursor goes to is kept in `pointed`. Fitted (`fit`),
/// each row is as tall as its word (a computer's replies).
struct Scrolled {
    pointed: Arc<AtomicUsize>,
    fit: bool,
}

const SCROLLED: usize = 20;

impl Screen for Scrolled {
    fn id(&self) -> ScreenId {
        ScreenId("scrolled")
    }
    fn layout(&self) -> &str {
        "scroll"
    }
    fn state(&self, control: &Control, _: &ViewModel) -> ControlState {
        ControlState {
            list: (control.tag == "LIST").then(|| ListView {
                rows: vec![
                    Row {
                        text: b"x".to_vec(),
                        ..Row::default()
                    };
                    SCROLLED
                ],
                fit: self.fit,
                pick: self.fit,
                ..ListView::default()
            }),
            ..ControlState::default()
        }
    }
    fn press(&mut self, _: &str, _: &ViewModel, _: &mut Vec<Intent>) {}
    fn pointed(&mut self, _: &str, index: usize, _: &ViewModel) {
        self.pointed.store(index, Ordering::Relaxed);
    }
}

/// A list of 20-pixel rows in 100 (five shown) with a scrollbar beside it:
/// its thumb's track from 12 to 88, the thumb 19 long.
fn scroll_layout() -> Gui {
    scroll_layout_of(20)
}

/// The scroll layout with prototype rows `row` pixels tall.
fn scroll_layout_of(row: i32) -> Gui {
    let bar = Control {
        kind: Kind::ScrollBar,
        thumb: Some(Image::default()),
        ..button("SCROLL", [100, 0, 10, 100], b"")
    };
    root(vec![Control {
        kind: Kind::ListBox,
        proto: Some(Box::new(button("PROTOITEM", [0, 0, 100, row], b""))),
        scrollbar: Some(Box::new(bar)),
        ..button("LIST", [0, 0, 100, 100], b"")
    }])
}

/// The first rows shown, and the rows the screen is told are pointed at,
/// as the scrollbar's thumb is taken near its top and dragged by `moves`
/// (layout pixels down), in a 1280 × 720 window (scale 1, the layout 320
/// by 120 in).
fn dragged(moves: &[f32]) -> Vec<(usize, usize)> {
    let pointed = Arc::new(AtomicUsize::new(0));
    let mut stack = Stack::new(no_screens);
    stack.push(Box::new(Scrolled {
        pointed: pointed.clone(),
        fit: false,
    }));
    let mut assets = Fake::new();
    assets.layouts = BTreeMap::from([("scroll", scroll_layout())]);
    let vm = ViewModel::default();
    let at = |y: f32| UiInput {
        window: (1280.0, 720.0),
        pointer: Some([320.0 + 105.0, 120.0 + y]),
        moved: true,
        held: true,
        ..keys(&[])
    };
    stack.step(&mut assets, &vm, &keys(&[]));
    stack.step(
        &mut assets,
        &vm,
        &UiInput {
            click: true,
            ..at(14.0)
        },
    );
    moves
        .iter()
        .map(|y| {
            stack.step(&mut assets, &vm, &at(14.0 + y));
            let first = stack.list_at("LIST").map_or(0, |(first, _)| first);
            (first, pointed.load(Ordering::Relaxed))
        })
        .collect()
}

/// A fitted list whose prototype is 4 pixels tall, so its rows by the
/// prototype would all fit, wheeled far down under the pointer (which
/// stays still): the first row shown, the row the screen was last told is
/// pointed at, and the first of the last rows whole in the list, by the
/// rows' own heights.
fn wheeled_fitted() -> (usize, usize, usize) {
    let pointed = Arc::new(AtomicUsize::new(0));
    let mut stack = Stack::new(no_screens);
    stack.push(Box::new(Scrolled {
        pointed: pointed.clone(),
        fit: true,
    }));
    let mut assets = Fake::new();
    let layout = scroll_layout_of(4);
    let proto = layout.controls[0].proto.clone().expect("a prototype");
    let height = crate::paint::row_height(
        &mut assets,
        crate::skin::Source::Campaign,
        &proto,
        true,
        b"x",
    );
    let last = SCROLLED - (100 / height.max(1)) as usize;
    assets.layouts = BTreeMap::from([("scroll", layout)]);
    let vm = ViewModel::default();
    let over = UiInput {
        window: (1280.0, 720.0),
        pointer: Some([320.0 + 50.0, 120.0 + 50.0]),
        ..keys(&[])
    };
    stack.step(&mut assets, &vm, &over);
    stack.step(
        &mut assets,
        &vm,
        &UiInput {
            wheel: -50.0,
            ..over.clone()
        },
    );
    stack.step(&mut assets, &vm, &over);
    let first = stack.list_at("LIST").map_or(0, |(first, _)| first);
    (first, pointed.load(Ordering::Relaxed), last)
}

/// A screen over `scroll` whose list `LIST` reads one description of
/// `DESCRIBED` lines, taller than the list.
struct Described;

const DESCRIBED: usize = 60;

impl Screen for Described {
    fn id(&self) -> ScreenId {
        ScreenId("described")
    }
    fn layout(&self) -> &str {
        "scroll"
    }
    fn state(&self, control: &Control, _: &ViewModel) -> ControlState {
        if control.tag == "LIST" {
            crate::screens::pages::description(&b"a\n".repeat(DESCRIBED))
        } else {
            ControlState::default()
        }
    }
    fn press(&mut self, _: &str, _: &ViewModel, _: &mut Vec<Intent>) {}
}

/// Where the scrollbar's thumb is (its top and foot, layout pixels) for a
/// description taller than its list: as it opens, a notch of the wheel
/// down, then wheeled far down. None where no thumb is drawn.
fn wheeled_description() -> Vec<Option<(f32, f32)>> {
    let mut stack = Stack::new(no_screens);
    stack.push(Box::new(Described));
    let mut assets = Fake::new();
    // A thumb with an image, to be drawn.
    let mut layout = scroll_layout_of(4);
    if let Some(thumb) = (layout.controls[0].scrollbar.as_mut()).and_then(|bar| bar.thumb.as_mut())
    {
        thumb.image = "thumb".into();
    }
    assets.layouts = BTreeMap::from([("scroll", layout)]);
    let vm = ViewModel::default();
    let over = UiInput {
        window: (1280.0, 720.0),
        pointer: Some([320.0 + 50.0, 120.0 + 50.0]),
        ..keys(&[])
    };
    // The bar's thumb: the quad 2 in from the bar's left (`paint::scrollbar`).
    let thumb = |frame: crate::screen::Frame| {
        frame
            .layers
            .iter()
            .flat_map(|layer| &layer.quads)
            .find(|quad| quad.rect[0] == 102.0 && quad.rect[2] == 6.0)
            .map(|quad| (quad.rect[1], quad.rect[1] + quad.rect[3]))
    };
    [0.0, -1.0, -500.0]
        .into_iter()
        .map(|wheel| {
            thumb(stack.step(
                &mut assets,
                &vm,
                &UiInput {
                    wheel,
                    ..over.clone()
                },
            ))
        })
        .collect()
}

fn grid_layout() -> Gui {
    root(vec![
        button("BTN_A", [0, 0, 50, 20], b"a"),
        button("BTN_B", [0, 100, 50, 20], b"b"),
        button("BTN_C", [200, 0, 50, 20], b"c"),
    ])
}

pub(super) fn all() -> Vec<Check> {
    let mut assets = Fake::new();
    assets.layouts = BTreeMap::from([
        (
            "chain",
            root(vec![
                linked("BTN_A", 0, [10, 10, 50, 20], 1),
                linked("BTN_B", 1, [300, 300, 50, 20], 2),
                linked("BTN_C", 2, [10, 400, 50, 20], -1),
            ]),
        ),
        ("grid", grid_layout()),
        ("edit", root(vec![button("EDIT", [0, 0, 200, 20], b"")])),
    ]);
    let vm = ViewModel::default();

    let mut chain = stack_on("chain");
    chain.step(&mut assets, &vm, &keys(&[]));
    let first = chain.focused().map(str::to_string);
    chain.step(&mut assets, &vm, &keys(&[Key::Down]));
    let second = chain.focused().map(str::to_string);
    chain.step(&mut assets, &vm, &keys(&[Key::Down]));
    let third = chain.focused().map(str::to_string);
    let pressed = chain
        .step(&mut assets, &vm, &keys(&[Key::Activate]))
        .intents;
    let back = chain.step(&mut assets, &vm, &keys(&[Key::Back])).intents;
    let closed = chain.top().is_none();

    let mut grid = stack_on("grid");
    grid.step(&mut assets, &vm, &keys(&[]));
    grid.step(&mut assets, &vm, &keys(&[Key::Down]));
    let below = grid.focused().map(str::to_string);
    grid.step(&mut assets, &vm, &keys(&[Key::Up, Key::Right]));
    let right = grid.focused().map(str::to_string);

    // The layout in the middle of a window, a layout pixel the window's
    // height over 720: 1.5 in 1920 × 1080 (480 by 180 in); scaled down to
    // fit (0.5625, 60 px from the left) in 480 × 270.
    let hit = |window: (f32, f32), point: [f32; 2]| {
        let mut pointer = stack_on("grid");
        let mut assets = Fake::new();
        assets.layouts = BTreeMap::from([("grid", grid_layout())]);
        let input = UiInput {
            window,
            pointer: Some(point),
            moved: true,
            ..keys(&[])
        };
        pointer.step(&mut assets, &vm, &input);
        let hovered = pointer.focused().map(str::to_string);
        let click = UiInput {
            click: true,
            ..input
        };
        (hovered, pointer.step(&mut assets, &vm, &click).intents)
    };
    let (hovered, clicked) = hit((1920.0, 1080.0), [480.0 + 225.0 * 1.5, 180.0 + 10.0 * 1.5]);
    let (small, _) = hit((480.0, 270.0), [60.0 + 225.0 * 0.5625, 10.0 * 0.5625]);
    // Half the track down, all of it, back to the top.
    let rows = dragged(&[28.5, 57.0, 0.0]);
    let (wheeled, wheel_pointed, last) = wheeled_fitted();
    let thumbs = wheeled_description();

    let mut edit = stack_on("edit");
    edit.step(&mut assets, &vm, &keys(&[]));
    edit.step(
        &mut assets,
        &vm,
        &UiInput {
            text: "abc".into(),
            ..keys(&[])
        },
    );
    edit.step(&mut assets, &vm, &keys(&[Key::Backspace]));
    edit.step(
        &mut assets,
        &vm,
        &UiInput {
            text: "defghijk".into(),
            ..keys(&[])
        },
    );
    let typed = edit.edited("EDIT");

    // Each pulse sampled three times, half its period apart.
    let mut sampled = |pulse: fn(&Stack) -> f32, dt: f32| {
        let mut clock = stack_on("grid");
        (0..3)
            .map(|_| {
                let now = pulse(&clock);
                clock.step(&mut assets, &vm, &UiInput { dt, ..keys(&[]) });
                now
            })
            .collect::<Vec<_>>()
    };
    let pulses = sampled(Stack::pulse, 0.8);
    let pulsing = sampled(Stack::pulsing, 0.75);

    // Falling closes what is open, once: what opens while down stays.
    let mut fall = stack_on("grid");
    fall.fell(false);
    fall.fell(true);
    let closed_on_fall = !fall.is_open();
    fall.push(Box::new(Probe {
        layout: "grid",
        edit: EditBox::new(8),
    }));
    fall.fell(true);
    let open_while_down = fall.is_open();
    fall.fell(false);
    fall.fell(true);
    let closed_again = !fall.is_open();

    vec![
        check("death_closes_once", {
            expect("falling closes the screens", closed_on_fall)
                .and_then(|()| expect("a screen opened while down stays", open_while_down))
                .and_then(|()| expect("falling again closes it", closed_again))
                .map(|()| "on the fall only".into())
        }),
        check("moveto_follows_links", {
            let path = [first, second, third];
            expect(
                &format!("A, B, C: {path:?}"),
                path == [
                    Some("BTN_A".into()),
                    Some("BTN_B".into()),
                    Some("BTN_C".into()),
                ],
            )
            .map(|()| "A → B → C".into())
        }),
        check("activate_emits_intent", {
            expect(&format!("{pressed:?}"), pressed == vec![Intent::Quit]).map(|()| "quit".into())
        }),
        check("back_closes", {
            expect(
                &format!("{back:?}, closed {closed}"),
                back.is_empty() && closed,
            )
            .map(|()| "the screen closed".into())
        }),
        check("nearest_without_link", {
            expect(
                &format!("down to B, then up and right to C: {below:?} {right:?}"),
                below.as_deref() == Some("BTN_B") && right.as_deref() == Some("BTN_C"),
            )
            .map(|()| "by direction".into())
        }),
        check("hit_test_scaled", {
            expect(
                &format!("the pointer on C through a 1920 × 1080 window: {hovered:?}"),
                hovered.as_deref() == Some("BTN_C"),
            )
            .and_then(|()| {
                expect(
                    &format!("and through a 480 × 270 one: {small:?}"),
                    small.as_deref() == Some("BTN_C"),
                )
            })
            .and_then(|()| {
                expect(
                    &format!("a click presses it: {clicked:?}"),
                    clicked == vec![Intent::Quit],
                )
            })
            .map(|()| "hover and click".into())
        }),
        check("scrollbar_thumb_drags", {
            // KOTOR 2's thumb moves the rows only: the selection stays.
            expect(
                &format!("the view starts at 8, 15, then 0, the row pointed at still 0: {rows:?}"),
                rows == [(8, 0), (15, 0), (0, 0)],
            )
            .map(|()| "the thumb follows the pointer, the selection stays".into())
        }),
        check("fitted_list_wheels", {
            // KOTOR 2's wheel moves the rows only: the selection stays.
            expect(
                &format!(
                    "wheeled to its last rows, from row {last}: {wheeled}, the row pointed at still 0: {wheel_pointed}"
                ),
                last > 0 && wheeled == last && wheel_pointed == 0,
            )
            .map(|()| "a fitted list scrolls by its rows' heights, the selection stays".into())
        }),
        check("description_wheels", {
            let shown = |at: usize| thumbs.get(at).copied().flatten();
            let moved = match (shown(0), shown(1), shown(2)) {
                (Some(top), Some(notch), Some(end)) => {
                    near(top.0, 12.0) && notch.0 > top.0 && notch.1 < 88.0 && near(end.1, 88.0)
                }
                _ => false,
            };
            expect(
                &format!(
                    "the thumb at the track's top, down a notch, then at its foot: {thumbs:?}"
                ),
                moved,
            )
            .map(|()| "a description taller than its list scrolls by lines".into())
        }),
        check("edit_box_types", {
            expect(
                &format!("ab then defghi (8 at most): {typed:?}"),
                typed.as_deref() == Some(&b"abdefghi"[..]),
            )
            .map(|()| "typed, erased, capped".into())
        }),
        check("pulse_cycles", {
            expect(
                &format!("0.2, 1, 0.2: {pulses:?}"),
                near(pulses[0], 0.2) && near(pulses[1], 1.0) && near(pulses[2], 0.2),
            )
            .and_then(|()| {
                expect(
                    &format!("a pulsing border 1, 0.4, 1: {pulsing:?}"),
                    near(pulsing[0], 1.0) && near(pulsing[1], 0.4) && near(pulsing[2], 1.0),
                )
            })
            .map(|()| "1.6 s; a pulsing border 1.5 s".into())
        }),
    ]
}

/// An 800 × 600 layout whose one button, `BTN_C`, is at 400, 300 and
/// 80 × 60, filled with a texture so that it draws a quad of its size.
fn centred_layout() -> Gui {
    let mut control = button("BTN_C", [400, 300, 80, 60], b"");
    control.border = Some(Border {
        fill: "f".into(),
        ..Border::default()
    });
    Gui {
        extent: Rect {
            left: 0,
            top: 0,
            width: 800,
            height: 600,
        },
        ..root(vec![control])
    }
}

/// A stack on `centred_layout`, a mod's when `modded`.
fn centred_stack(modded: bool) -> (Stack, Fake) {
    stack_over(centred_layout(), modded)
}

/// A stack on `layout`, a mod's when `modded`.
fn stack_over(layout: Gui, modded: bool) -> (Stack, Fake) {
    let mut assets = Fake::new();
    assets.layouts = BTreeMap::from([("centred", layout)]);
    if modded {
        assets.mods.insert("centred");
    }
    (stack_on("centred"), assets)
}

/// An 800 × 600 layout of KOTOR 2's own kinds of controls, by their tags in
/// `PICTURES`: a framed square tab button with a small icon in its top
/// right corner, a portrait (a clear frame round a fill), a 3D view, a
/// framed button with words and a bar of art.
fn pictures_layout() -> Gui {
    let framed = |tag: &str, rect: [i32; 4], words: &[u8], fill: &str| Control {
        border: Some(Border {
            corner: "c".into(),
            edge: "e".into(),
            fill: fill.into(),
            dimension: 16,
            ..Border::default()
        }),
        ..button(tag, rect, words)
    };
    let filled = |tag: &str, rect: [i32; 4]| Control {
        border: Some(Border {
            fill: "f".into(),
            ..Border::default()
        }),
        ..button(tag, rect, b"")
    };
    Gui {
        extent: Rect {
            left: 0,
            top: 0,
            width: 800,
            height: 600,
        },
        ..root(vec![
            framed(PICTURES[0], [356, 56, 44, 44], b"", ""),
            filled(PICTURES[1], [386, 56, 14, 14]),
            framed(PICTURES[2], [118, 135, 80, 80], b"", "po"),
            Control {
                kind: Kind::Label,
                ..button(PICTURES[3], [420, 134, 280, 289], b"")
            },
            framed(PICTURES[4], [78, 470, 322, 28], b"Accept", ""),
            filled(PICTURES[5], [61, 105, 678, 14]),
        ])
    }
}

const PICTURES: [&str; 6] = [
    "BTN_EQU",
    "LBL_EYE",
    "LBL_CHAR0",
    "LBL_3D",
    "BTN_ACCEPT",
    "LBL_BAR1",
];

/// Where each of `PICTURES` is placed in a window of `window` pixels with
/// `factor` physical pixels to each (layout pixels: left, top, width,
/// height), the layout a mod's when `modded`, with `vm`'s settings; and the
/// size of the first framed corner drawn, in window pixels.
fn pictures_in(
    modded: bool,
    vm: &ViewModel,
    window: (f32, f32),
    factor: f32,
) -> (Vec<[i32; 4]>, Option<f32>) {
    let (placed, frame, assets) =
        placed_in(pictures_layout(), &PICTURES, modded, vm, window, factor);
    let corner = frame.layers.first().and_then(|layer| {
        let (scale, _) = layer.placed(window);
        layer
            .quads
            .iter()
            .find(|quad| assets.named(quad.texture).as_deref() == Some("c"))
            .map(|quad| quad.rect[2] * scale)
    });
    (placed, corner)
}

/// A stack over `layout` (a mod's when `modded`) stepped once in a window
/// of `window` pixels with `factor` physical pixels to each, with `vm`'s
/// settings. Returns where each of `tags` is placed (layout pixels: left,
/// top, width, height), the frame the stack drew, and its assets.
fn placed_in(
    layout: Gui,
    tags: &[&str],
    modded: bool,
    vm: &ViewModel,
    window: (f32, f32),
    factor: f32,
) -> (Vec<[i32; 4]>, Frame, Fake) {
    let (mut stack, mut assets) = stack_over(layout, modded);
    let input = UiInput {
        window,
        scale_factor: factor,
        ..keys(&[])
    };
    let frame = stack.step(&mut assets, vm, &input);
    let placed = tags
        .iter()
        .filter_map(|tag| stack.extent_of(tag))
        .map(|rect| [rect.left, rect.top, rect.width, rect.height])
        .collect();
    (placed, frame, assets)
}

/// Where `centred_layout`'s root and its `BTN_C` are drawn in a window of
/// each of `windows` pixels in turn, with `vm`'s settings, in window pixels
/// (left, top, width, height).
fn drawn_in(modded: bool, vm: &ViewModel, windows: &[(f32, f32)]) -> Vec<([f32; 4], [f32; 4])> {
    let (mut stack, mut assets) = centred_stack(modded);
    windows
        .iter()
        .filter_map(|&window| {
            let input = UiInput {
                window,
                ..keys(&[])
            };
            let frame = stack.step(&mut assets, vm, &input);
            let layer = frame.layers.first()?;
            let (scale, [x, y]) = layer.placed(window);
            let in_window = |[left, top, width, height]: [f32; 4]| {
                [
                    x + left * scale,
                    y + top * scale,
                    width * scale,
                    height * scale,
                ]
            };
            let quad = layer
                .quads
                .iter()
                .find(|quad| quad.texture != TextureId::WHITE)?;
            let root = in_window([0.0, 0.0, layer.size.0, layer.size.1]);
            Some((root, in_window(quad.rect)))
        })
        .collect()
}

/// The control under the pointer at `point` of a window of `window` pixels
/// over `centred_layout`, with `vm`'s settings, and what a click there does.
fn pointed_in(
    modded: bool,
    vm: &ViewModel,
    window: (f32, f32),
    point: [f32; 2],
) -> (Option<String>, Vec<Intent>) {
    let (mut stack, mut assets) = centred_stack(modded);
    let input = UiInput {
        window,
        pointer: Some(point),
        moved: true,
        ..keys(&[])
    };
    stack.step(&mut assets, vm, &input);
    let under = stack.under_pointer(vm, &input);
    let click = UiInput {
        click: true,
        ..input
    };
    (under, stack.step(&mut assets, vm, &click).intents)
}

/// A view model with kjk's Interface settings `settings` (key, value).
fn interface(settings: &[(&str, &str)]) -> ViewModel {
    ViewModel {
        settings: settings
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect(),
        ..ViewModel::default()
    }
}

/// Every 800 × 600 layout stretched to the window as KOTOR 2 stretches it:
/// a mod's plainly, the game's own with its pictures kept in shape (kjk's
/// change, after the "Widescreen UI Fix" mod).
pub(super) fn mod_layouts() -> Vec<Check> {
    let full_hd = (1920.0, 1080.0);
    let unset = ViewModel::default();
    let buttons = |drawn: Vec<([f32; 4], [f32; 4])>| -> Vec<[f32; 4]> {
        drawn.into_iter().map(|(_, button)| button).collect()
    };
    let modded = buttons(drawn_in(true, &unset, &[full_hd, (1280.0, 720.0)]));
    let own = buttons(drawn_in(false, &unset, &[full_hd]));
    // Inside the stretched button, past the one kept in shape.
    let corner = [1145.0, 640.0];
    let (over_mod, clicked) = pointed_in(true, &unset, full_hd, corner);
    let (over_own, _) = pointed_in(false, &unset, full_hd, corner);
    // Inside the picture kept in shape, near its corner.
    let (over_kept, _) = pointed_in(false, &unset, full_hd, [1125.0, 645.0]);
    let (own_pictures, _) = pictures_in(false, &unset, full_hd, 1.0);
    let (mod_pictures, _) = pictures_in(true, &unset, full_hd, 1.0);
    // With "Maintain image aspect ratio" off, the game's own pictures, and
    // with it on a mod's, are stretched plainly.
    let plain_off = interface(&[(IMAGE_ASPECT, "false")]);
    let (own_plain, _) = pictures_in(false, &plain_off, full_hd, 1.0);
    let (mod_with_aspect, _) =
        pictures_in(true, &interface(&[(IMAGE_ASPECT, "true")]), full_hd, 1.0);
    // KOTOR 2's stretch of each, 1.6 × 1.2 in the layout's pixels at kjk's
    // scale of 1.5 (2.4 × 1.8 in window pixels).
    let plain = [
        [570, 67, 70, 53],
        [618, 67, 22, 17],
        [189, 162, 128, 96],
        [672, 161, 448, 347],
        [125, 564, 515, 34],
        [98, 126, 1084, 17],
    ];
    // The tab button, the portrait and the 3D view 1.2 times their size
    // about the stretched centres; the icon kept in the tab's corner; the
    // button with words and the bar stretched.
    let kept = [
        [578, 67, 53, 53],
        [614, 67, 17, 17],
        [205, 162, 96, 96],
        [728, 161, 336, 347],
        plain[4],
        plain[5],
    ];
    vec![
        check("mod_layout_stretched", {
            expect(
                &format!("a mod's button at the centre, 2.4 × 1.8 times its size, then 1.6 × 1.2 in 1280 × 720: {modded:?}"),
                modded == [[960.0, 540.0, 192.0, 108.0], [640.0, 360.0, 128.0, 72.0]],
            )
            .and_then(|()| {
                expect(
                    &format!("a mod's pictures plainly stretched: {mod_pictures:?}"),
                    mod_pictures == plain,
                )
            })
            .and_then(|()| {
                expect(
                    &format!("the pointer at the stretched button's corner is on it: {over_mod:?}"),
                    over_mod.as_deref() == Some("BTN_C"),
                )
            })
            .and_then(|()| {
                expect(
                    &format!("a click there presses it: {clicked:?}"),
                    clicked == vec![Intent::Quit],
                )
            })
            .map(|()| "drawn and hit where KOTOR 2 puts it".into())
        }),
        check("own_pictures_keep_their_shape", {
            expect(
                &format!("KOTOR 2's own 4:3 picture 1.8 times its size about the stretched centre: {own:?}"),
                own == [[984.0, 540.0, 144.0, 108.0]],
            )
            .and_then(|()| {
                expect(
                    &format!("squares, portraits and 3D views in shape, words and bars stretched: {own_pictures:?}"),
                    own_pictures == kept,
                )
            })
            .and_then(|()| {
                expect(
                    &format!("the pointer is on the picture where it is drawn, not past it: {over_kept:?}, {over_own:?}"),
                    over_kept.as_deref() == Some("BTN_C") && over_own.is_none(),
                )
            })
            .map(|()| "pictures in shape at 16:9, as the Widescreen UI Fix draws them".into())
        }),
        check("image_aspect_off_stretches_plainly", {
            expect(
                &format!("the game's own pictures stretched as KOTOR 2 does: {own_plain:?}"),
                own_plain == plain,
            )
            .and_then(|()| {
                expect(
                    &format!("a mod's pictures stretched plainly with it on: {mod_with_aspect:?}"),
                    mod_with_aspect == plain,
                )
            })
            .map(|()| "Off gives KOTOR 2's plain stretch; a mod's layout always has it".into())
        }),
    ]
}

/// kjk's Interface option "Menu width" in a 21:9 and a 16:9 window: where
/// the frame is drawn (window pixels), and the pointer on the button where
/// it is drawn, on nothing in the black bars.
pub(super) fn menu_widths() -> Vec<Check> {
    let windows = [(2560.0, 1080.0), (1920.0, 1080.0)];
    // Each menu width, and its frame in each window in turn: Fill the screen
    // to the window's edges (2559 pixels, 1706 layout pixels at 1.5); Up to
    // 16:9 as wide as a 16:9 window and centred; 4:3 the layout's own 800 ×
    // 600 at kjk's scale of 1.5, centred.
    let widths: [(&str, [[f32; 4]; 2]); 3] = [
        (
            "fill",
            [[0.0, 0.0, 2559.0, 1080.0], [0.0, 0.0, 1920.0, 1080.0]],
        ),
        (
            "16x9",
            [[320.0, 0.0, 1920.0, 1080.0], [0.0, 0.0, 1920.0, 1080.0]],
        ),
        (
            "4x3",
            [[680.0, 90.0, 1200.0, 900.0], [360.0, 90.0, 1200.0, 900.0]],
        ),
    ];
    let wrong: Vec<String> = widths
        .iter()
        .flat_map(|(width, frames)| {
            let vm = interface(&[(MenuWidth::SETTING, width)]);
            let drawn = drawn_in(false, &vm, &windows);
            windows
                .iter()
                .zip(frames)
                .zip(drawn)
                .filter_map(move |((&window, frame), (root, button))| {
                    let centre = [button[0] + button[2] / 2.0, button[1] + button[3] / 2.0];
                    let (on_button, clicked) = pointed_in(false, &vm, window, centre);
                    let (in_bar, _) = pointed_in(false, &vm, window, [root[0] / 2.0, 540.0]);
                    let barred = root[0] >= 1.0;
                    let ok = root == *frame
                        && on_button.as_deref() == Some("BTN_C")
                        && clicked == vec![Intent::Quit]
                        && (!barred || in_bar.is_none());
                    (!ok).then(|| {
                        format!("{width} in {window:?}: frame {root:?}, button {button:?} hit {on_button:?}, bar hit {in_bar:?}")
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect();
    vec![check("menu_widths", {
        expect(&format!("frames and hits: {wrong:?}"), wrong.is_empty())
            .map(|()| "fill, up to 16:9 and 4:3 at 21:9 and 16:9, drawn where they are hit".into())
    })]
}

/// A border's corners at its DIMENSION in the display's physical pixels:
/// half as many window pixels on a Retina display (2 physical pixels to
/// each).
pub(super) fn physical_borders() -> Vec<Check> {
    let full_hd = (1920.0, 1080.0);
    let unset = ViewModel::default();
    let corners = [1.0, 2.0].map(|factor| pictures_in(false, &unset, full_hd, factor).1);
    vec![check("border_corners_in_physical_pixels", {
        expect(
            &format!(
                "a 16-pixel corner 16 window pixels wide, then 8 on a Retina display: {corners:?}"
            ),
            matches!(corners, [Some(one), Some(two)] if near(one, 16.0) && near(two, 8.0)),
        )
        .map(|()| "DIMENSION in physical pixels".into())
    })]
}

/// A control of `tag` at `rect` drawn by a fill image alone (`fill`), with
/// no words.
fn fill_only(tag: &str, rect: [i32; 4], fill: &str) -> Control {
    Control {
        kind: Kind::Label,
        border: Some(Border {
            fill: fill.into(),
            ..Border::default()
        }),
        ..button(tag, rect, b"")
    }
}

/// A square slot of `tag` at `rect`: a frame of corners and edges round
/// nothing, as KOTOR 2's equip slots are.
fn slot(tag: &str, rect: [i32; 4]) -> Control {
    Control {
        border: Some(Border {
            corner: "c".into(),
            edge: "e".into(),
            dimension: 16,
            ..Border::default()
        }),
        ..button(tag, rect, b"")
    }
}

/// KOTOR 2's equip screen as its layout has it, in `EQUIP`'s order: the
/// slot diagram (fill-only art 312 × 108) with the two weapon slots on it,
/// the belt's slot across its top edge, a slot above and one below in the
/// left weapon's column, words on the diagram, the button under the slots,
/// and the bar under the party member (fill-only art 307 × 32) with its
/// arrow on it.
fn equip_layout() -> Gui {
    Gui {
        extent: Rect {
            left: 0,
            top: 0,
            width: 800,
            height: 600,
        },
        ..root(vec![
            fill_only(EQUIP[0], [400, 267, 312, 108], "cnfg"),
            slot(EQUIP[1], [472, 321, 54, 54]),
            slot(EQUIP[2], [594, 321, 54, 54]),
            slot(EQUIP[3], [533, 260, 54, 54]),
            slot(EQUIP[4], [472, 200, 54, 54]),
            slot(EQUIP[5], [472, 381, 54, 54]),
            Control {
                kind: Kind::Label,
                ..button(EQUIP[6], [418, 271, 100, 38], b"Attack Modifier")
            },
            button(EQUIP[7], [410, 442, 300, 28], b"Switch Weapons"),
            fill_only(EQUIP[8], [409, 476, 307, 32], "bar"),
            fill_only(EQUIP[9], [510, 480, 14, 24], "arrow"),
        ])
    }
}

const EQUIP: [&str; 10] = [
    "LBL_BACK1",
    "BTN_INV_WEAP_L",
    "BTN_INV_WEAP_R",
    "BTN_INV_BELT",
    "BTN_INV_ARM_L",
    "BTN_INV_WEAP_L2",
    "LBL_ATTACKMOD",
    "BTN_SWAPWEAPONS",
    "LBL_BAR5",
    "BTN_PREVNPC",
];

/// A panel of art that pictures sit on keeps its shape with them at 16:9
/// (kjk's change, after the "Widescreen UI Fix" mod): the equip screen's
/// slots keep their places on the slot diagram and in their columns. A
/// bar stays stretched as KOTOR 2 stretches it, and the arrow on it keeps
/// its own shape where the stretch puts it.
pub(super) fn picture_pieces() -> Vec<Check> {
    let unset = ViewModel::default();
    let (placed, ..) = placed_in(equip_layout(), &EQUIP, false, &unset, (1920.0, 1080.0), 1.0);
    let read: Vec<[i32; 4]> = equip_layout()
        .controls
        .iter()
        .map(|control| {
            let e = control.extent;
            [e.left, e.top, e.width, e.height]
        })
        .collect();
    // Each of the slots and the words on the diagram, at the same share of
    // the diagram's width and height from its corner, and of the same share
    // of its size, as in the layout, to within a layout pixel.
    let out_of_place: Vec<&str> = match placed.as_slice() {
        [diagram, ..] if placed.len() == EQUIP.len() => (1..7)
            .filter(|&at| {
                let (now, was, base) = (placed[at], read[at], read[0]);
                (0..4).any(|edge| {
                    let (start, size) = (edge % 2, 2 + edge % 2);
                    let share = |rect: [i32; 4], of: [i32; 4]| {
                        let from = if edge < 2 {
                            rect[start] - of[start]
                        } else {
                            rect[edge]
                        };
                        from as f32 / of[size] as f32
                    };
                    (share(now, *diagram) - share(was, base)).abs() * diagram[size] as f32 > 1.0
                })
            })
            .map(|at| EQUIP[at])
            .collect(),
        _ => EQUIP.to_vec(),
    };
    let column: Vec<[i32; 2]> = [1, 4, 5]
        .iter()
        .filter_map(|&at| placed.get(at).map(|rect| [rect[0], rect[2]]))
        .collect();
    vec![check("picture_pieces", {
        expect(
            &format!("the diagram in shape, its slots and words in proportion on it: {placed:?}, out of place {out_of_place:?}"),
            out_of_place.is_empty()
                && placed
                    .first()
                    .is_some_and(|d| (d[2] * 108 - d[3] * 312).abs() <= 312),
        )
        .and_then(|()| {
            expect(
                &format!("the slots above and below in the weapon's column: {column:?}"),
                column.len() == 3 && column.iter().all(|left_width| *left_width == column[0]),
            )
        })
        .and_then(|()| {
            expect(
                &format!(
                    "the button under the slots and the bar stretched 1.6 × 1.2, the arrow in shape: {:?}",
                    &placed[7..]
                ),
                placed[7..]
                    == [
                        [656, 530, 480, 34],
                        [654, 571, 492, 39],
                        [819, 576, 17, 29],
                    ],
            )
        })
        .map(|()| "the equip slots line up with their diagram at 1920 × 1080".into())
    })]
}

/// KOTOR 2's message box as its layout has it (`confirm_p`): a 290 × 100
/// box at 174, 200 of a 640 × 480 frame, filled with `box`, its button
/// `BTN_C` filled with `button` and a square icon `LBL_ICON` filled with
/// `icon`.
fn confirm_layout() -> Gui {
    Gui {
        tag: "CONFIRM".into(),
        extent: Rect {
            left: 174,
            top: 200,
            width: 290,
            height: 100,
        },
        border: Some(Border {
            fill: "box".into(),
            ..Border::default()
        }),
        ..root(vec![
            Control {
                border: Some(Border {
                    fill: "button".into(),
                    ..Border::default()
                }),
                ..button("BTN_C", [84, 41, 120, 24], b"OK")
            },
            fill_only("LBL_ICON", [8, 8, 24, 24], "icon"),
        ])
    }
}

/// What a popup drew and hit over a menu (`centred_layout`).
struct Popup {
    /// The box, its button and its icon, in window pixels (left, top,
    /// width, height).
    drawn: Vec<[f32; 4]>,
    /// The control under the pointer at the button's centre, and what a
    /// click there does.
    under: Option<String>,
    clicked: Vec<Intent>,
    /// The popup's frame in window pixels, and whether everything it drew
    /// (its veil and the pad's hint bar too) lies inside it.
    frame: [f32; 4],
    inside: bool,
}

/// `confirm_layout` as the popup layout `name` (a mod's when `modded`)
/// over a menu, in a window of `window` pixels, with `vm`'s settings and a
/// pad playing.
fn popup_in(name: &'static str, modded: bool, vm: &ViewModel, window: (f32, f32)) -> Popup {
    let mut assets = Fake::new();
    assets.layouts = BTreeMap::from([("centred", centred_layout()), (name, confirm_layout())]);
    if modded {
        assets.mods.insert(name);
    }
    let vm = ViewModel {
        pad_playing: true,
        ..vm.clone()
    };
    let mut stack = stack_on("centred");
    stack.push(Box::new(Probe {
        layout: name,
        edit: EditBox::new(8),
    }));
    let input = UiInput {
        window,
        ..keys(&[])
    };
    let frame = stack.step(&mut assets, &vm, &input);
    let Some(layer) = frame.layers.get(1) else {
        return Popup {
            drawn: Vec::new(),
            under: None,
            clicked: Vec::new(),
            frame: [0.0; 4],
            inside: false,
        };
    };
    let (scale, [x, y]) = layer.placed(window);
    let in_window = |[left, top, width, height]: [f32; 4]| {
        [
            x + left * scale,
            y + top * scale,
            width * scale,
            height * scale,
        ]
    };
    let drawn: Vec<[f32; 4]> = ["box", "button", "icon"]
        .iter()
        .filter_map(|fill| {
            let quad = layer
                .quads
                .iter()
                .find(|quad| assets.named(quad.texture).as_deref() == Some(*fill))?;
            Some(in_window(quad.rect))
        })
        .collect();
    let (w, h) = layer.size;
    let inside = layer.quads.iter().all(|quad| {
        let [left, top, width, height] = quad.clip.unwrap_or(quad.rect);
        left >= 0.0 && top >= 0.0 && left + width <= w && top + height <= h
    }) && !stack.hints(&mut assets, &vm).is_empty();
    let centre = drawn
        .get(1)
        .map_or([0.0; 2], |b| [b[0] + b[2] / 2.0, b[1] + b[3] / 2.0]);
    let pointed = UiInput {
        pointer: Some(centre),
        moved: true,
        ..input
    };
    stack.step(&mut assets, &vm, &pointed);
    let under = stack.under_pointer(&vm, &pointed);
    let clicked = stack
        .step(
            &mut assets,
            &vm,
            &UiInput {
                click: true,
                ..pointed
            },
        )
        .intents;
    Popup {
        drawn,
        under,
        clicked,
        frame: in_window([0.0, 0.0, w, h]),
        inside,
    }
}

/// Popups stretched as KOTOR 2 stretches them: the message box's place
/// and size times the window's width over 800 and height over 600, then
/// centred in the window; a popup KOTOR 2 does not centre left where the
/// stretch puts it; in a 21:9 window with the menu up to 16:9, centred in
/// the menu's frame, its veil and the pad's hints on the menu and not on
/// the black bars beside it. Each is hit where it is drawn.
pub(super) fn popups() -> Vec<Check> {
    let unset = ViewModel::default();
    let full_hd = (1920.0, 1080.0);
    let wide = (2560.0, 1080.0);
    let centred = popup_in("confirm", false, &unset, full_hd);
    let modded = popup_in("confirm", true, &unset, full_hd);
    let left = popup_in("areatrans", false, &unset, full_hd);
    let capped = popup_in(
        "confirm",
        false,
        &interface(&[(MenuWidth::SETTING, "16x9")]),
        wide,
    );
    let kept = popup_in(
        "confirm",
        false,
        &interface(&[(MenuWidth::SETTING, "4x3")]),
        full_hd,
    );
    let hit = |popup: &Popup| {
        popup.under.as_deref() == Some("BTN_C") && popup.clicked == vec![Intent::Quit]
    };
    // KOTOR 2's 2.4 × 1.8: the box 696 × 180, at (1920 - 696) / 2 and
    // (1080 - 180) / 2; its button 288 × 43.5 at 84, 41 of it stretched;
    // the icon 42 × 42 in shape, a mod's 57 × 42 as KOTOR 2 stretches it.
    let full_hd_box = [612.0, 450.0, 696.0, 180.0];
    let button = [813.0, 523.5, 288.0, 43.5];
    vec![
        check("popups_stretched_and_centred", {
            expect(
                &format!("the box, its button and its icon: {:?}", centred.drawn),
                centred.drawn == [full_hd_box, button, [639.0, 465.0, 42.0, 42.0]] && hit(&centred),
            )
            .and_then(|()| {
                expect(
                    &format!("a mod's icon stretched plainly: {:?}", modded.drawn),
                    modded.drawn.get(2) == Some(&[631.5, 465.0, 57.0, 42.0]) && hit(&modded),
                )
            })
            .and_then(|()| {
                expect(
                    &format!(
                        "a popup KOTOR 2 does not centre at 174 × 2.4, 200 × 1.8: {:?}",
                        left.drawn
                    ),
                    left.drawn.first() == Some(&[417.0, 360.0, 696.0, 180.0]) && hit(&left),
                )
            })
            .and_then(|()| {
                expect(
                    &format!(
                        "at 4:3 the box its own size at 1.5, centred: {:?}",
                        kept.drawn
                    ),
                    kept.drawn.first() == Some(&[742.5, 465.0, 435.0, 150.0]) && hit(&kept),
                )
            })
            .map(|()| "drawn and hit where KOTOR 2 puts the message box".into())
        }),
        check("popup_veil_inside_capped_menu", {
            expect(
                &format!(
                    "in the 16:9 frame {:?}, the box {:?}, all inside: {}",
                    capped.frame, capped.drawn, capped.inside
                ),
                capped.frame == [320.0, 0.0, 1920.0, 1080.0]
                    && capped.drawn.first() == Some(&[932.0, 450.0, 696.0, 180.0])
                    && capped.inside
                    && hit(&capped),
            )
            .map(|()| "the veil and the hints stay on the menu, off the black bars".into())
        }),
    ]
}
