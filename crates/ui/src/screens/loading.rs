//! A session starting: KOTOR's loading screen (`loadscreen`). Its image is
//! the module's when that is known. Its hint line shows what is happening,
//! or a hint. Its bar shows how far the load is, and is hidden while that is
//! not known. The layout is the campaign's own, so its logo is the
//! campaign's.

use fmt_odyssey::gui::Control;

use crate::intent::Intent;
use crate::paint::ControlState;
use crate::screen::{Screen, ScreenId};
use crate::strings;
use crate::vm::ViewModel;

pub struct Loading;

impl Screen for Loading {
    fn id(&self) -> ScreenId {
        ScreenId("loading")
    }

    fn layout(&self) -> &str {
        "loadscreen"
    }

    fn quiet(&self) -> bool {
        true
    }

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        match control.tag.as_str() {
            "TGuiPanel" => ControlState {
                fill: vm.session.image.clone(),
                ..ControlState::default()
            },
            "LBL_HINT" => ControlState {
                text: Some(strings::bytes(&vm.session.status)),
                ..ControlState::default()
            },
            "PB_PROGRESS" => ControlState {
                hidden: vm.session.progress.is_none(),
                value: vm.session.progress,
                ..ControlState::default()
            },
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, _: &str, _: &ViewModel, _: &mut Vec<Intent>) {}

    fn back(&mut self, _: &mut Vec<Intent>) {}
}
