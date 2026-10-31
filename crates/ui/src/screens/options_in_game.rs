//! The in-game options (`optionsingame`, the menu bar's last tab), where
//! Escape opens the menu: saving and loading (`save_load`), the settings'
//! screens, quitting to the main menu, and a break (P10) in KOTOR's
//! autopause button's place. A guest cannot save; only a game played alone
//! loads during play.
//!
//! Quit asks first, KOTOR's box (`CSWGuiInGameOptions::OnQuit`: its 42348,
//! OK and Cancel), and OK ends the game for the main menu (`ReallyQuit`).
//! A second button under it quits to the desktop, asking the same (issue
//! 48; the games leave that to the main menu's Exit).

use fmt_odyssey::gui::{Control, Gui};

use crate::intent::{GameAction, Intent, SessionIntent};
use crate::paint::ControlState;
use crate::screen::{Screen, ScreenId};
use crate::screens::tab::{Page, enabled, text, told};
use crate::strings;
use crate::vm::{BreakButton, ViewModel};

/// The button that quits to the desktop, made from `BTN_QUIT` (`shape`).
const DESKTOP: &str = "BTN_QUITDESKTOP";
/// The box that asks before quitting, and what it quits to.
const ASK: &str = "quit_confirm";
const TO_MENU: &str = "menu";
const TO_DESKTOP: &str = "desktop";
/// "Do you really want to quit?" (TSL adds that the progress is not saved).
const REALLY_QUIT: u32 = 42348;

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
            DESKTOP => text(strings::QUIT_TO_DESKTOP),
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
            "BTN_QUIT" => out.push(Intent::OpenFor(ASK, TO_MENU.into())),
            DESKTOP => out.push(Intent::OpenFor(ASK, TO_DESKTOP.into())),
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

    /// `BTN_QUIT`'s copy one button's pitch below it (the gap above it,
    /// from the button over it), its focus threaded between Quit and what
    /// Quit led down to.
    fn shape(&self, gui: &mut Gui) {
        let Some(quit) = gui.find("BTN_QUIT").cloned() else {
            return;
        };
        let above = gui
            .controls
            .iter()
            .filter(|control| control.extent.left == quit.extent.left)
            .map(|control| control.extent.top)
            .filter(|top| *top < quit.extent.top)
            .max();
        let pitch = above.map_or(quit.extent.height, |top| quit.extent.top - top);
        let id = gui
            .controls
            .iter()
            .map(|control| control.id)
            .max()
            .unwrap_or(0)
            + 1;
        let mut desktop = Control {
            id,
            tag: DESKTOP.into(),
            ..quit.clone()
        };
        desktop.extent.top += pitch;
        if let Some(links) = desktop.moveto.as_mut() {
            links.up = quit.id;
        }
        for control in &mut gui.controls {
            let Some(links) = control.moveto.as_mut() else {
                continue;
            };
            if control.id == quit.id {
                links.down = id;
            } else if links.up == quit.id {
                links.up = id;
            }
        }
        let at = gui
            .controls
            .iter()
            .position(|control| control.tag == quit.tag);
        gui.controls
            .insert(at.map_or(gui.controls.len(), |at| at + 1), desktop);
    }
}

/// KOTOR's box before quitting (`confirm`, 42348): OK quits to the main
/// menu or the desktop, Cancel goes back to the menu.
pub struct Quit {
    desktop: bool,
}

impl Quit {
    pub fn new(to: &str) -> Option<Quit> {
        match to {
            TO_MENU => Some(Quit { desktop: false }),
            TO_DESKTOP => Some(Quit { desktop: true }),
            _ => None,
        }
    }
}

impl Screen for Quit {
    fn id(&self) -> ScreenId {
        ScreenId(ASK)
    }

    fn layout(&self) -> &str {
        "confirm"
    }

    fn state(&self, control: &Control, _: &ViewModel) -> ControlState {
        match control.tag.as_str() {
            "LB_MESSAGE" => told(Some(REALLY_QUIT)),
            "BTN_OK" => text(strings::OK),
            "BTN_CANCEL" => text(strings::CANCEL),
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, _: &ViewModel, out: &mut Vec<Intent>) {
        match tag {
            "BTN_OK" if self.desktop => out.push(Intent::Quit),
            "BTN_OK" => out.extend([Intent::CloseAll, Intent::Session(SessionIntent::ToMenu)]),
            "BTN_CANCEL" => out.push(Intent::Close),
            _ => {}
        }
    }

    fn first_focus(&self) -> Option<&str> {
        Some("BTN_CANCEL")
    }

    fn shape(&self, gui: &mut Gui) {
        super::message::grow(gui);
    }
}
