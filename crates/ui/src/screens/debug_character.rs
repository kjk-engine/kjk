//! The Debug Mode character page (`debug_character`): a list of options on
//! the layout kjk's Interface options use (KOTOR 2's feedback page), each
//! row a value with KOTOR 2's minus and plus arrows. The game fills it from
//! the chosen party member (`ViewModel::debug_character`,
//! `game::debug_terminal`). A click on an arrow, or Left and Right while the
//! list has the focus, steps the row's value down or up; held keys repeat.
//! The default button is Restore, shown only for a player's own character.
//! Back and Escape close the page.

use fmt_odyssey::gui::Control;

use crate::draw::Textures;
use crate::input::Key;
use crate::intent::Intent;
use crate::paint::{ControlState, ListView, ROW_MINUS, Row};
use crate::screen::{Screen, ScreenId};
use crate::screens::pages::description;
use crate::skin::Source;
use crate::strings;
use crate::vm::{DebugCharacterView, ViewModel};

const LIST: &str = "LB_OPTIONS";
const RESTORE: &str = "BTN_DEFAULT";

static NONE: DebugCharacterView = DebugCharacterView {
    title: Vec::new(),
    rows: Vec::new(),
    restore: false,
};

/// The page: the control with the focus, the row last pointed at, and the
/// part of it the pointer was on (`paint::ROW_MINUS`, `ROW_PLUS` or the
/// rest).
#[derive(Default)]
pub struct DebugCharacter {
    focus: Option<String>,
    row: usize,
    part: usize,
}

fn view(vm: &ViewModel) -> &DebugCharacterView {
    vm.debug_character.as_ref().unwrap_or(&NONE)
}

/// Row `row` stepped down (`back`) or up.
fn step(row: usize, back: bool, out: &mut Vec<Intent>) {
    if let Ok(row) = u8::try_from(row) {
        out.push(Intent::DebugStep { row, back });
    }
}

impl Screen for DebugCharacter {
    fn id(&self) -> ScreenId {
        ScreenId("debug_character")
    }

    fn layout(&self) -> &str {
        "optfeedback"
    }

    fn focused(&mut self, tag: &str, _vm: &ViewModel) {
        self.focus = Some(tag.to_string());
    }

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        let page = view(vm);
        match control.tag.as_str() {
            "LBL_TITLE" => ControlState {
                text: Some(page.title.clone()),
                ..ControlState::default()
            },
            LIST => ControlState {
                list: Some(ListView {
                    rows: (page.rows.iter())
                        .map(|row| Row {
                            text: [row.name.as_slice(), b": ", &row.value].concat(),
                            unmarked: true,
                            arrows: true,
                            ..Row::default()
                        })
                        .collect(),
                    ..ListView::default()
                }),
                ..ControlState::default()
            },
            "LB_DESC" => description(
                &page
                    .rows
                    .get(self.row)
                    .map_or(Vec::new(), |row| row.help.clone()),
            ),
            RESTORE => ControlState {
                text: Some(strings::bytes(strings::terminal::RESTORE)),
                hidden: !page.restore,
                ..ControlState::default()
            },
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, vm: &ViewModel, out: &mut Vec<Intent>) {
        match tag {
            RESTORE if view(vm).restore => out.push(Intent::DebugRestore),
            "BTN_BACK" => out.push(Intent::Close),
            _ => {}
        }
    }

    /// A row activated: a click on its minus arrow steps it down, anywhere
    /// else on it up.
    fn row(&mut self, list: &str, index: usize, _vm: &ViewModel, out: &mut Vec<Intent>) {
        if list == LIST {
            step(index, self.part == ROW_MINUS, out);
        }
    }

    fn cell(&mut self, list: &str, _index: usize, part: usize, _vm: &ViewModel) {
        if list == LIST {
            self.part = part;
        }
    }

    fn pointed(&mut self, list: &str, index: usize, _vm: &ViewModel) {
        if list == LIST {
            self.row = index;
        }
    }

    /// Left and Right step the row last pointed at while the list has the
    /// focus; a held key repeats.
    fn key(&mut self, key: Key, _vm: &ViewModel, out: &mut Vec<Intent>) -> bool {
        let back = match key {
            Key::Left => true,
            Key::Right => false,
            _ => return false,
        };
        if self.focus.as_deref() != Some(LIST) {
            return false;
        }
        step(self.row, back, out);
        true
    }

    fn scrolls(&self) -> Option<&str> {
        Some("LB_DESC")
    }

    fn fit_read(
        &mut self,
        gui: &mut fmt_odyssey::gui::Gui,
        textures: &mut dyn Textures,
        _: Source,
    ) {
        crate::screens::options::fit_rows(gui, textures, LIST);
    }

    fn first_focus(&self) -> Option<&str> {
        Some(LIST)
    }
}
