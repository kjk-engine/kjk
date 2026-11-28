//! A test screen made from a standalone kjk layout, `data/ui/layouts/
//! layout_test.toml` (docs/UI.md, "Layout files"). It has no KOTOR layout
//! to start from. It shows that a kjk layout drawn from KOTOR's parts is
//! drawn, focused and pressed as any KOTOR screen is, with the mouse and
//! with a pad.

use crate::intent::Intent;
use crate::paint::{ControlState, ListView, Row};
use crate::screen::{Screen, ScreenId};
use crate::vm::ViewModel;
use fmt_odyssey::gui::Control;

/// The rows of the list, which the screen makes up.
const ITEMS: [&str; 6] = [
    "First test row",
    "Second test row",
    "Third test row",
    "Fourth test row",
    "Fifth test row",
    "Sixth test row",
];

/// A slider is moved by a tenth of its length at a time.
const SLIDERS: [&str; 2] = ["SLI_FIRST", "SLI_SECOND"];

pub struct LayoutTest {
    pointed: usize,
    sliders: [f32; 2],
    checked: bool,
}

impl LayoutTest {
    pub fn new() -> LayoutTest {
        LayoutTest {
            pointed: 0,
            sliders: [0.5, 0.25],
            checked: false,
        }
    }
}

impl Default for LayoutTest {
    fn default() -> LayoutTest {
        LayoutTest::new()
    }
}

impl Screen for LayoutTest {
    fn id(&self) -> ScreenId {
        ScreenId("layout_test")
    }

    fn layout(&self) -> &str {
        "kjk:layout_test"
    }

    fn state(&self, control: &Control, _: &ViewModel) -> ControlState {
        match control.tag.as_str() {
            "LB_ITEMS" => ControlState {
                list: Some(ListView {
                    selected: Some(self.pointed),
                    rows: ITEMS
                        .iter()
                        .map(|words| Row {
                            text: crate::strings::bytes(words),
                            ..Row::default()
                        })
                        .collect(),
                    ..ListView::default()
                }),
                ..ControlState::default()
            },
            "CB_OPTION" => ControlState {
                selected: self.checked,
                ..ControlState::default()
            },
            tag => match SLIDERS.iter().position(|slider| *slider == tag) {
                Some(at) => ControlState {
                    value: Some(self.sliders[at]),
                    ..ControlState::default()
                },
                None => ControlState::default(),
            },
        }
    }

    fn press(&mut self, tag: &str, _: &ViewModel, out: &mut Vec<Intent>) {
        match tag {
            "CB_OPTION" => self.checked = !self.checked,
            "BTN_OK" | "BTN_BACK" => out.push(Intent::Close),
            _ => {}
        }
    }

    fn row(&mut self, _: &str, index: usize, _: &ViewModel, _: &mut Vec<Intent>) {
        self.pointed = index;
    }

    fn pointed(&mut self, _: &str, index: usize, _: &ViewModel) {
        self.pointed = index;
    }

    fn slide(&mut self, tag: &str, value: f32, _: &mut Vec<Intent>) {
        if let Some(at) = SLIDERS.iter().position(|slider| *slider == tag) {
            self.sliders[at] = value;
        }
    }

    fn accept(&self) -> Option<&str> {
        Some("BTN_OK")
    }

    fn first_focus(&self) -> Option<&str> {
        Some("LB_ITEMS")
    }
}
