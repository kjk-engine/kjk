//! An in-game screen under the menu bar: the bar's own behaviour (its
//! icons, its buttons, the shoulders), Exit and Back closing the menu;
//! each page says only what is its own.

use fmt_odyssey::gui::{Control, Gui};

use crate::draw::{Quad, Textures};
use crate::input::Key;
use crate::intent::Intent;
use crate::paint::{ControlState, Painter};
use crate::screen::{Screen, ScreenId};
use crate::screens::tabs;
use crate::vm::ViewModel;

/// One page of the in-game menu.
pub trait Page: Send + Sync {
    /// Its screen's name (as the bar's tabs name it).
    const NAME: &'static str;
    const LAYOUT: &'static str;
    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState;
    fn press(&mut self, _tag: &str, _vm: &ViewModel, _out: &mut Vec<Intent>) {}
    fn row(&mut self, _list: &str, _index: usize, _vm: &ViewModel) {}
    /// A double click on `tag` (the row or button the first click chose).
    fn double(&mut self, _tag: &str, _vm: &ViewModel, _out: &mut Vec<Intent>) {}
    /// The button a double click on a row of `list` presses
    /// (`Screen::primary`).
    fn primary(&self, _list: &str) -> Option<&'static str> {
        None
    }
    fn first_focus(&self) -> Option<&str> {
        None
    }
    fn quads(&self, _gui: &Gui, _vm: &ViewModel, _painter: &mut Painter) -> Vec<Quad> {
        Vec::new()
    }
    fn click(&mut self, _gui: &Gui, _at: [f32; 2], _textures: &mut dyn Textures, _vm: &ViewModel) {}
}

pub struct Tab<P> {
    page: P,
}

impl<P> Tab<P> {
    pub fn new(page: P) -> Tab<P> {
        Tab { page }
    }
}

impl<P: Page> Screen for Tab<P> {
    fn id(&self) -> ScreenId {
        ScreenId(P::NAME)
    }

    fn layout(&self) -> &str {
        P::LAYOUT
    }

    fn under(&self) -> Option<&str> {
        Some(tabs::BAR)
    }

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        tabs::state(&control.tag, P::NAME, vm).unwrap_or_else(|| self.page.state(control, vm))
    }

    fn press(&mut self, tag: &str, vm: &ViewModel, out: &mut Vec<Intent>) {
        if tabs::press(tag, P::NAME, vm, out) {
            return;
        }
        match tag {
            "BTN_EXIT" => out.push(Intent::CloseAll),
            _ => self.page.press(tag, vm, out),
        }
    }

    fn row(&mut self, list: &str, index: usize, vm: &ViewModel, _out: &mut Vec<Intent>) {
        self.page.row(list, index, vm);
    }

    fn double(&mut self, tag: &str, vm: &ViewModel, out: &mut Vec<Intent>) {
        self.page.double(tag, vm, out);
    }

    fn primary(&self, list: &str) -> Option<&'static str> {
        self.page.primary(list)
    }

    fn key(&mut self, key: Key, _: &ViewModel, out: &mut Vec<Intent>) -> bool {
        tabs::key(key, P::NAME, out)
    }

    fn back(&mut self, out: &mut Vec<Intent>) {
        out.push(Intent::CloseAll);
    }

    fn first_focus(&self) -> Option<&str> {
        self.page.first_focus()
    }

    fn quads(&self, gui: &Gui, vm: &ViewModel, painter: &mut Painter) -> Vec<Quad> {
        self.page.quads(gui, vm, painter)
    }

    fn click(&mut self, gui: &Gui, at: [f32; 2], textures: &mut dyn Textures, vm: &ViewModel) {
        self.page.click(gui, at, textures, vm);
    }
}

/// A label's text.
pub fn text(words: impl ToString) -> ControlState {
    ControlState {
        text: Some(crate::strings::bytes(&words.to_string())),
        ..ControlState::default()
    }
}

/// The game's text `strref` (none: the layout's own).
pub fn told(strref: Option<u32>) -> ControlState {
    ControlState {
        strref,
        ..ControlState::default()
    }
}

pub fn shown(show: bool) -> ControlState {
    ControlState {
        hidden: !show,
        ..ControlState::default()
    }
}

pub fn hidden() -> ControlState {
    shown(false)
}

pub fn enabled(on: bool) -> ControlState {
    ControlState {
        disabled: !on,
        ..ControlState::default()
    }
}

pub fn disabled() -> ControlState {
    enabled(false)
}

/// An image in place of the layout's; none known draws none (never the
/// layout's own stand-in).
pub fn image(name: Option<String>) -> ControlState {
    ControlState {
        fill: Some(name.unwrap_or(String::new())),
        ..ControlState::default()
    }
}

/// A signed number: `+2`, `-1`, `0`.
pub fn signed(value: i32) -> String {
    if value > 0 {
        format!("+{value}")
    } else {
        value.to_string()
    }
}

/// Entries as list rows, the selected one `selected`.
pub fn rows(entries: &[crate::vm::Entry], selected: Option<usize>) -> ControlState {
    ControlState {
        list: Some(crate::paint::ListView {
            rows: entries
                .iter()
                .map(|entry| crate::paint::Row {
                    text: entry.name.clone(),
                    icon: entry.icon.clone(),
                    count: entry.count,
                    ..crate::paint::Row::default()
                })
                .collect(),
            selected,
            ..crate::paint::ListView::default()
        }),
        ..ControlState::default()
    }
}
