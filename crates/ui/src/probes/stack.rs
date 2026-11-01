//! The screen stack: focus by MOVETO and by direction, the pointer through
//! a scaled window, presses, going back, edit boxes, the pulse.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use fmt_odyssey::gui::{Control, Gui, Image, Kind, MoveTo, Rect};

use super::paint::{Fake, button};
use super::{Check, check, expect, near};
use crate::edit::EditBox;
use crate::input::{Key, UiInput};
use crate::intent::Intent;
use crate::paint::{ControlState, ListView, Row};
use crate::screen::{Screen, ScreenId, Stack};
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
/// shown; the row its cursor goes to is kept in `pointed`.
struct Scrolled {
    pointed: Arc<AtomicUsize>,
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
                rows: vec![Row::default(); SCROLLED],
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
    let bar = Control {
        kind: Kind::ScrollBar,
        thumb: Some(Image::default()),
        ..button("SCROLL", [100, 0, 10, 100], b"")
    };
    root(vec![Control {
        kind: Kind::ListBox,
        proto: Some(Box::new(button("PROTOITEM", [0, 0, 100, 20], b""))),
        scrollbar: Some(Box::new(bar)),
        ..button("LIST", [0, 0, 100, 100], b"")
    }])
}

/// The rows the cursor follows to as the scrollbar's thumb is taken
/// near its top and dragged by `moves` (layout pixels down), in a
/// 1280 × 720 window (scale 1, the layout 320 by 120 in).
fn dragged(moves: &[f32]) -> Vec<usize> {
    let pointed = Arc::new(AtomicUsize::new(0));
    let mut stack = Stack::new(no_screens);
    stack.push(Box::new(Scrolled {
        pointed: pointed.clone(),
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
            pointed.load(Ordering::Relaxed)
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
            expect(
                &format!(
                    "the cursor to rows 8, 15, then 4 as the view starts at 8, 15, 0: {rows:?}"
                ),
                rows == [8, 15, 4],
            )
            .map(|()| "the thumb follows the pointer".into())
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
