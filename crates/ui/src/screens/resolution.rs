//! The screen resolution (`optresolution`, from the graphics page): the
//! window's modes and sizes the launcher offers (`vm.displays`), the one
//! the window is in chosen when it opens. A row chosen is kept; OK shows the window
//! so (`display.mode`, `display.size`), Cancel leaves it as it was.

use fmt_odyssey::gui::Control;

use crate::intent::Intent;
use crate::paint::{ControlState, ListView, Row};
use crate::screen::{Screen, ScreenId};
use crate::strings;
use crate::vm::ViewModel;

pub struct Resolution {
    chosen: Option<usize>,
}

impl Resolution {
    pub fn new(vm: &ViewModel) -> Resolution {
        Resolution {
            chosen: vm.displays.iter().position(|choice| choice.current),
        }
    }
}

impl Screen for Resolution {
    fn id(&self) -> ScreenId {
        ScreenId("options_resolution")
    }

    fn layout(&self) -> &str {
        "optresolution"
    }

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        match control.tag.as_str() {
            "LB_RESOLUTIONS" => ControlState {
                list: Some(ListView {
                    rows: vm
                        .displays
                        .iter()
                        .map(|choice| Row {
                            text: strings::bytes(&choice.label),
                            ..Row::default()
                        })
                        .collect(),
                    selected: self.chosen,
                    ..ListView::default()
                }),
                ..ControlState::default()
            },
            "BTN_OK" => ControlState {
                disabled: self.chosen.is_none(),
                ..ControlState::default()
            },
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, vm: &ViewModel, out: &mut Vec<Intent>) {
        match tag {
            "BTN_OK" => {
                if let Some(choice) = self.chosen.and_then(|at| vm.displays.get(at)) {
                    out.extend([
                        Intent::Setting("display.size".into(), choice.size.clone()),
                        Intent::Setting("display.mode".into(), choice.mode.clone()),
                    ]);
                }
                out.push(Intent::Close);
            }
            "BTN_CANCEL" => out.push(Intent::Close),
            _ => {}
        }
    }

    fn row(&mut self, _: &str, index: usize, vm: &ViewModel, _: &mut Vec<Intent>) {
        if index < vm.displays.len() {
            self.chosen = Some(index);
        }
    }

    fn first_focus(&self) -> Option<&str> {
        Some("LB_RESOLUTIONS")
    }
}
