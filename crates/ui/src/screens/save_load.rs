//! Saving and loading in play (`saveload`, from the in-game options' Save
//! and Load), as KOTOR's. A click points at a row; a double click or the
//! Save (Load) button acts on it. Saving, the list's first row is a
//! new save and the saves under it are overwritten: an old one is first
//! confirmed (`confirm`, "Are you sure you want to overwrite the save
//! game?"), then the name is typed in KOTOR's box (`savename`; the area's
//! for a new save, the old save's own over it), whose OK saves. Loading,
//! the saves. Delete deletes the save pointed at. The pointed save's area,
//! when it was saved, and its module's loading image show beside the list.

use fmt_odyssey::gui::{Control, Gui};

use crate::edit::EditBox;
use crate::intent::{Intent, SessionIntent};
use crate::paint::{ControlState, ListView, Row};
use crate::screen::{Screen, ScreenId};
use crate::screens::tab::{enabled, hidden, image, text, told};
use crate::strings;
use crate::vm::{SaveView, ViewModel};

/// The longest name typed.
const NAME_LONGEST: usize = 32;
/// "Are you sure you want to overwrite the save game?"
const OVERWRITE: u32 = 1591;

/// The save list's rows: `first` (a new game, a new save) then the saves,
/// a copy marked with whose it is; with neither, one dimmed row saying
/// there are none. `pointed` is lit.
pub(crate) fn list(first: Option<&str>, saves: &[SaveView], pointed: usize) -> ControlState {
    let row = |words: &str, dimmed: bool| Row {
        text: strings::bytes(words),
        dimmed,
        ..Row::default()
    };
    let mut rows: Vec<Row> = first.map(|first| row(first, false)).into_iter().collect();
    rows.extend(saves.iter().map(|save| match &save.from {
        Some(from) => row(&format!("{} (from {from})", save.name), false),
        None => row(&save.name, false),
    }));
    if rows.is_empty() {
        rows.push(row(strings::NO_SAVES, true));
    }
    ControlState {
        list: Some(ListView {
            selected: Some(pointed.min(rows.len() - 1)),
            rows,
            ..ListView::default()
        }),
        ..ControlState::default()
    }
}

/// A module's loading image, where the save's screenshot would be.
pub(crate) fn screenshot(module: Option<&str>) -> ControlState {
    image(module.map(|module| format!("load_{module}")))
}

pub struct SaveLoad {
    saving: bool,
    pointed: usize,
}

impl SaveLoad {
    pub fn saving() -> SaveLoad {
        SaveLoad {
            saving: true,
            pointed: 0,
        }
    }

    pub fn loading() -> SaveLoad {
        SaveLoad {
            saving: false,
            pointed: 0,
        }
    }

    /// The save the pointed row is (saving, the first row is a new one).
    fn save<'v>(&self, vm: &'v ViewModel) -> Option<&'v SaveView> {
        let index = if self.saving {
            self.pointed.checked_sub(1)?
        } else {
            self.pointed
        };
        vm.saves.get(index)
    }

    fn act(&mut self, vm: &ViewModel, out: &mut Vec<Intent>) {
        let save = self.save(vm);
        if !self.saving {
            if let Some(save) = save {
                out.push(Intent::Session(SessionIntent::Solo {
                    save: Some(save.id.clone()),
                    character: None,
                    campaign: None,
                }));
            }
            return;
        }
        out.push(match save {
            Some(save) => Intent::OpenFor(OVERWRITE_SCREEN, asked(Some(&save.id), &save.name)),
            None => Intent::OpenFor(
                NAME_SCREEN,
                asked(None, vm.here.as_ref().map_or("", |here| here.name.as_str())),
            ),
        });
    }
}

/// The overwrite box's and the name box's screens, opened for a save asked
/// for (`asked`).
const OVERWRITE_SCREEN: &str = "save_overwrite";
const NAME_SCREEN: &str = "save_name";

/// A save asked for, as the boxes are opened for it: the slot (empty for a
/// new save) and the name offered, a line apart.
fn asked(slot: Option<&str>, name: &str) -> String {
    format!("{}\n{name}", slot.unwrap_or(""))
}

/// `asked`'s slot and name.
fn parted(asked: &str) -> (Option<String>, &str) {
    let (slot, name) = asked.split_once('\n').unwrap_or(("", asked));
    ((!slot.is_empty()).then(|| slot.to_string()), name)
}

impl Screen for SaveLoad {
    fn id(&self) -> ScreenId {
        ScreenId(if self.saving { "save" } else { "load" })
    }

    fn layout(&self) -> &str {
        "saveload"
    }

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        let save = self.save(vm);
        match control.tag.as_str() {
            "LBL_PANELNAME" if self.saving => text(strings::SAVE_TITLE),
            "LBL_PANELNAME" => text(strings::LOAD_TITLE),
            "LB_GAMES" => list(
                self.saving.then_some(strings::NEW_SAVE),
                &vm.saves,
                self.pointed,
            ),
            "LBL_PLANETNAME" => text(
                save.or(vm.here.as_ref().filter(|_| self.saving))
                    .map_or("", |save| save.area.as_str()),
            ),
            "LBL_PM1" | "LBL_PM2" | "LBL_PM3" | "LBL_PCNAME" | "LBL_TIMEPLAYED"
            | "CB_CLOUDSAVE" | "BTN_FILTER" => hidden(),
            "LBL_AREANAME" => match (save, &vm.here) {
                (Some(save), _) => text(&save.when),
                (None, Some(here)) if self.saving => text(&here.area),
                _ => text(""),
            },
            "LBL_SCREENSHOT" => screenshot(
                save.or(vm.here.as_ref().filter(|_| self.saving))
                    .map(|save| save.module.as_str()),
            ),
            "BTN_DELETE" => ControlState {
                text: Some(strings::bytes(strings::DELETE)),
                ..enabled(save.is_some())
            },
            "BTN_SAVELOAD" if self.saving => ControlState {
                text: Some(strings::bytes(strings::SAVE)),
                ..enabled(vm.here.is_some())
            },
            "BTN_SAVELOAD" => ControlState {
                text: Some(strings::bytes(strings::LOAD)),
                ..enabled(save.is_some())
            },
            "BTN_BACK" => text(strings::BACK),
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, vm: &ViewModel, out: &mut Vec<Intent>) {
        match tag {
            "BTN_SAVELOAD" => self.act(vm, out),
            "BTN_DELETE" => {
                if let Some(save) = self.save(vm) {
                    out.push(Intent::DeleteSave(save.id.clone()));
                    self.pointed = 0;
                }
            }
            "BTN_BACK" => out.push(Intent::Close),
            _ => {}
        }
    }

    /// A row clicked is pointed at; Save (Load), or a double click
    /// (`primary`), acts on it.
    fn row(&mut self, _: &str, index: usize, _: &ViewModel, _: &mut Vec<Intent>) {
        self.pointed = index;
    }

    fn pointed(&mut self, _: &str, index: usize, _: &ViewModel) {
        self.pointed = index;
    }

    fn primary(&self, list: &str) -> Option<&'static str> {
        (list == LIST).then_some("BTN_SAVELOAD")
    }

    fn first_focus(&self) -> Option<&str> {
        Some(LIST)
    }
}

const LIST: &str = "LB_GAMES";

/// KOTOR's "overwrite the save game?" (`confirm`) for a save asked for over
/// an old one: OK goes on to its name (`SaveName`), Cancel back to the list.
pub struct Overwrite {
    asked: String,
}

impl Overwrite {
    pub fn new(asked: &str) -> Overwrite {
        Overwrite {
            asked: asked.to_string(),
        }
    }
}

impl Screen for Overwrite {
    fn id(&self) -> ScreenId {
        ScreenId(OVERWRITE_SCREEN)
    }

    fn layout(&self) -> &str {
        "confirm"
    }

    fn state(&self, control: &Control, _: &ViewModel) -> ControlState {
        match control.tag.as_str() {
            "LB_MESSAGE" => told(Some(OVERWRITE)),
            "BTN_OK" => text(strings::OK),
            "BTN_CANCEL" => text(strings::CANCEL),
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, _: &ViewModel, out: &mut Vec<Intent>) {
        match tag {
            "BTN_OK" => {
                out.push(Intent::Close);
                out.push(Intent::OpenFor(NAME_SCREEN, self.asked.clone()));
            }
            "BTN_CANCEL" => out.push(Intent::Close),
            _ => {}
        }
    }

    fn first_focus(&self) -> Option<&str> {
        Some("BTN_OK")
    }

    fn shape(&self, gui: &mut Gui) {
        super::message::grow(gui);
    }
}

/// KOTOR's box for a save's name (`savename`): the name offered, typed
/// over; OK saves (an empty name is the area's), Cancel goes back to the
/// list. The game closes the screens once it is saved, or says why not
/// over them.
pub struct SaveName {
    slot: Option<String>,
    name: EditBox,
}

impl SaveName {
    pub fn new(asked: &str) -> SaveName {
        let (slot, name) = parted(asked);
        SaveName {
            slot,
            name: EditBox::with(name, NAME_LONGEST),
        }
    }
}

impl Screen for SaveName {
    fn id(&self) -> ScreenId {
        ScreenId(NAME_SCREEN)
    }

    fn layout(&self) -> &str {
        "savename"
    }

    fn state(&self, control: &Control, _: &ViewModel) -> ControlState {
        match control.tag.as_str() {
            "BTN_OK" => text(strings::OK),
            "BTN_CANCEL" => text(strings::CANCEL),
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, _: &ViewModel, out: &mut Vec<Intent>) {
        match tag {
            "BTN_OK" => out.push(Intent::Save {
                slot: self.slot.clone(),
                name: self.name.as_str(),
            }),
            "BTN_CANCEL" => out.push(Intent::Close),
            _ => {}
        }
    }

    fn edit(&mut self, tag: &str) -> Option<&mut EditBox> {
        (tag == "EDITBOX").then_some(&mut self.name)
    }

    fn typing(&self) -> Option<&str> {
        Some("EDITBOX")
    }

    fn first_focus(&self) -> Option<&str> {
        Some("BTN_OK")
    }
}
