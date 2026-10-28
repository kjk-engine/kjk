//! A session starting: KOTOR's loading screen (`loadscreen`), its image
//! the module's when known, its hint line what is happening, its bar how
//! far it is (hidden while that is not known), the campaign's logo over
//! KOTOR 2's.

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
            "LBL_LOGO" => ControlState {
                fill: Some(crate::skin::CAMPAIGN_LOGO.into()),
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
