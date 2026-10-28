//! The in-game options (`optionsingame`, the menu bar's last tab), where
//! Escape opens the menu: saving and loading (`save_load`), the settings'
//! screens, quitting to the main menu, and a break (P10) in KOTOR's
//! autopause button's place. A guest cannot save; only a game played alone
//! loads during play.

use fmt_odyssey::gui::Control;

use crate::intent::{GameAction, Intent, SessionIntent};
use crate::paint::ControlState;
use crate::screens::tab::{Page, enabled, text};
use crate::strings;
use crate::vm::{BreakButton, ViewModel};

pub struct OptionsInGame;

impl Page for OptionsInGame {
    const NAME: &'static str = "options_in_game";
    const LAYOUT: &'static str = "optionsingame";

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        match control.tag.as_str() {
            "BTN_AUTOPAUSE" => match vm.calls.button {
                BreakButton::Break => text(strings::BREAK),
                BreakButton::Resume => text(strings::RESUME),
                BreakButton::Off => ControlState {
                    disabled: true,
                    ..text(strings::BREAK)
                },
            },
            "BTN_SAVEGAME" => enabled(vm.here.is_some()),
            "BTN_LOADGAME" => enabled(vm.load_in_play),
            "BTN_QUIT" => text(strings::QUIT_TO_MENU),
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, vm: &ViewModel, out: &mut Vec<Intent>) {
        match tag {
            "BTN_AUTOPAUSE" => {
                let action = match vm.calls.button {
                    BreakButton::Break => GameAction::Break,
                    BreakButton::Resume => GameAction::Resume,
                    BreakButton::Off => return,
                };
                out.extend([Intent::Game(action), Intent::CloseAll]);
            }
            "BTN_QUIT" => out.push(Intent::Session(SessionIntent::ToMenu)),
            "BTN_SAVEGAME" => out.push(Intent::Open("save")),
            "BTN_LOADGAME" => out.push(Intent::Open("load")),
            "BTN_GAMEPLAY" => out.push(Intent::Open("options_gameplay")),
            "BTN_FEEDBACK" => out.push(Intent::Open("options_feedback")),
            "BTN_GRAPHICS" => out.push(Intent::Open("options_graphics")),
            "BTN_SOUND" => out.push(Intent::Open("options_sound")),
            _ => {}
        }
    }

    fn first_focus(&self) -> Option<&str> {
        Some("BTN_GAMEPLAY")
    }
}
