//! A new game or a saved one (`saveload`), alone or hosted: the list's
//! first row is a new game, the saves under it; hosted, the seats cycle
//! from one to three (KOTOR's party, I30 P1) and the port is typed. Start
//! makes the character (`chargen`) for the new game (asking first which
//! campaign it begins when both games are installed, `campaign`) or loads
//! the save
//! pointed at (hosted for the seats chosen: its guests take its
//! companions, I31); alone, Delete deletes it. Back closes.

use fmt_odyssey::gui::Control;

use crate::edit::EditBox;
use crate::intent::SessionIntent;
use crate::intent::{DEFAULT_PORT, Intent};
use crate::paint::ControlState;
use crate::screen::{Screen, ScreenId};
use crate::screens::campaign;
use crate::screens::main_menu::SEATS;
use crate::screens::save_load::{list, screenshot};
use crate::screens::tab::{enabled, hidden, text};
use crate::strings;
use crate::vm::{SaveView, ViewModel};

/// Seats a hosted game may have here.
const MOST_SEATS: u8 = 3;

pub struct NewOrLoad {
    host: bool,
    seats: u8,
    port: EditBox,
    /// The port typed was refused, and has not changed since.
    refused: Option<Vec<u8>>,
    /// The row pointed at: 0 a new game, then the saves.
    pointed: usize,
}

impl NewOrLoad {
    pub fn solo() -> NewOrLoad {
        NewOrLoad {
            host: false,
            seats: 1,
            port: EditBox::new(5),
            refused: None,
            pointed: 0,
        }
    }

    pub fn host() -> NewOrLoad {
        NewOrLoad {
            host: true,
            seats: SEATS,
            port: EditBox::with(&DEFAULT_PORT.to_string(), 5),
            refused: None,
            pointed: 0,
        }
    }

    fn port(&self) -> Option<u16> {
        self.port
            .as_str()
            .parse::<u16>()
            .ok()
            .filter(|port| *port > 0)
    }

    /// The save pointed at, if a save is.
    fn save<'v>(&self, vm: &'v ViewModel) -> Option<&'v SaveView> {
        vm.saves.get(self.pointed.checked_sub(1)?)
    }

    /// A new game's character made (of the campaign picked when both
    /// games are installed, else of the one that is), or the save pointed
    /// at loaded.
    fn start(&mut self, vm: &ViewModel, out: &mut Vec<Intent>) {
        let save = self.save(vm).map(|save| save.id.clone());
        let new_game = |then: String, out: &mut Vec<Intent>| match vm.campaigns[..] {
            [] => out.push(Intent::OpenFor("chargen", then)),
            [only] => out.extend(campaign::begin(only, &then)),
            _ => out.push(Intent::OpenFor("campaign", then)),
        };
        if !self.host {
            match save {
                Some(save) => out.push(Intent::Session(SessionIntent::Solo {
                    save: Some(save),
                    character: None,
                    campaign: None,
                })),
                None => new_game("solo".into(), out),
            }
            return;
        }
        let Some(port) = self.port() else {
            self.refused = Some(self.port.text.clone());
            return;
        };
        match save {
            Some(save) => out.push(Intent::Session(SessionIntent::Host {
                save: Some(save),
                seats: self.seats,
                port,
                character: None,
                campaign: None,
            })),
            None => new_game(format!("host:{}:{port}", self.seats), out),
        }
    }
}

impl Screen for NewOrLoad {
    fn id(&self) -> ScreenId {
        ScreenId(if self.host { "host" } else { "solo" })
    }

    fn layout(&self) -> &str {
        "saveload"
    }

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        match control.tag.as_str() {
            "LBL_PANELNAME" if self.host => text(strings::HOST_TITLE),
            "LBL_PANELNAME" => text(strings::SOLO_TITLE),
            "LB_GAMES" => list(Some(strings::NEW_GAME), &vm.saves, self.pointed),
            "LBL_PLANETNAME" if self.host => ControlState {
                disabled: self.refused.as_ref() == Some(&self.port.text),
                ..ControlState::default()
            },
            "LBL_PLANETNAME" => text(
                self.save(vm)
                    .map_or(vm.new_game.area.as_str(), |save| save.area.as_str()),
            ),
            "LBL_AREANAME" if self.host => text(strings::PORT_HINT),
            "LBL_AREANAME" => text(self.save(vm).map_or("", |save| save.when.as_str())),
            "LBL_SCREENSHOT" => screenshot(Some(
                self.save(vm)
                    .map_or(vm.new_game.module.as_str(), |save| save.module.as_str()),
            )),
            "LBL_PM1" | "LBL_PM2" | "LBL_PM3" | "LBL_PCNAME" | "LBL_TIMEPLAYED"
            | "CB_CLOUDSAVE" | "BTN_FILTER" => hidden(),
            "BTN_DELETE" if self.host => text(format!("{} {}", strings::SEATS, self.seats)),
            "BTN_DELETE" => ControlState {
                text: Some(strings::bytes(strings::DELETE)),
                ..enabled(self.save(vm).is_some())
            },
            "BTN_SAVELOAD" => text(strings::START),
            "BTN_BACK" => text(strings::BACK),
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, vm: &ViewModel, out: &mut Vec<Intent>) {
        match tag {
            "BTN_DELETE" if self.host => self.seats = self.seats % MOST_SEATS + 1,
            "BTN_DELETE" => {
                if let Some(save) = self.save(vm) {
                    out.push(Intent::DeleteSave(save.id.clone()));
                    self.pointed = 0;
                }
            }
            "BTN_SAVELOAD" => self.start(vm, out),
            "BTN_BACK" => out.push(Intent::Close),
            _ => {}
        }
    }

    /// A row clicked is pointed at; Start, or a double click (`primary`),
    /// starts it.
    fn row(&mut self, _: &str, index: usize, _: &ViewModel, _: &mut Vec<Intent>) {
        self.pointed = index;
    }

    fn pointed(&mut self, _: &str, index: usize, _: &ViewModel) {
        self.pointed = index;
    }

    fn primary(&self, list: &str) -> Option<&'static str> {
        (list == "LB_GAMES").then_some("BTN_SAVELOAD")
    }

    fn edit(&mut self, tag: &str) -> Option<&mut EditBox> {
        (self.host && tag == "LBL_PLANETNAME").then_some(&mut self.port)
    }

    fn typing(&self) -> Option<&str> {
        self.host.then_some("LBL_PLANETNAME")
    }

    fn first_focus(&self) -> Option<&str> {
        Some("BTN_SAVELOAD")
    }
}
