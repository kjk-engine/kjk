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
use crate::skin::Source;
use crate::vm::ViewModel;

/// One page of the in-game menu.
pub trait Page: Send + Sync {
    /// Its screen's name (as the bar's tabs name it).
    const NAME: &'static str;
    const LAYOUT: &'static str;
    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState;
    /// Its own tabs, in order (`Screen::tabs`: the triggers step them).
    fn tabs(&self) -> Vec<&'static str> {
        Vec::new()
    }
    fn press(&mut self, _tag: &str, _vm: &ViewModel, _out: &mut Vec<Intent>) {}
    fn row(&mut self, _list: &str, _index: usize, _vm: &ViewModel) {}
    /// A list's cursor moved to `index` (`Screen::pointed`).
    fn pointed(&mut self, _list: &str, _index: usize, _vm: &ViewModel) {}
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
    /// Its lists show other rows than when last asked (`Screen::lists_anew`).
    fn lists_anew(&mut self) -> bool {
        false
    }
    /// A list's cursor put on a row of its own (`Screen::cursor_to`).
    fn cursor_to(&mut self, _vm: &ViewModel) -> Option<(&'static str, usize)> {
        None
    }
    fn quads(&self, _gui: &Gui, _vm: &ViewModel, _painter: &mut Painter) -> Vec<Quad> {
        Vec::new()
    }
    fn click(&mut self, _gui: &Gui, _at: [f32; 2], _textures: &mut dyn Textures, _vm: &ViewModel) {}
    /// Its layout changed once loaded (`Screen::shape`).
    fn shape(&self, _gui: &mut Gui) {}
    /// Its layout once shaped, in KOTOR 2's 800 × 600 frame before the
    /// stack stretches it to the window (`Screen::fit_read`).
    fn fit_read(&mut self, _gui: &Gui) {}
    /// The focus (the pointer's, or the keys') came to `tag`, or left it
    /// (`Screen::focused`, `Screen::left`).
    fn focused(&mut self, _tag: &str, _vm: &ViewModel) {}
    fn left(&mut self, _tag: &str) {}
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

    fn pointed(&mut self, list: &str, index: usize, vm: &ViewModel) {
        self.page.pointed(list, index, vm);
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

    fn tab_bar(&self, _: &ViewModel) -> bool {
        true
    }

    fn tabs(&self) -> Vec<&'static str> {
        self.page.tabs()
    }

    fn back(&mut self, out: &mut Vec<Intent>) {
        out.push(Intent::CloseAll);
    }

    fn first_focus(&self) -> Option<&str> {
        self.page.first_focus()
    }

    fn lists_anew(&mut self) -> bool {
        self.page.lists_anew()
    }

    fn cursor_to(&mut self, vm: &ViewModel) -> Option<(&'static str, usize)> {
        self.page.cursor_to(vm)
    }

    fn quads(&self, gui: &Gui, vm: &ViewModel, painter: &mut Painter) -> Vec<Quad> {
        self.page.quads(gui, vm, painter)
    }

    fn click(&mut self, gui: &Gui, at: [f32; 2], textures: &mut dyn Textures, vm: &ViewModel) {
        self.page.click(gui, at, textures, vm);
    }

    fn shape(&self, gui: &mut Gui) {
        self.page.shape(gui);
    }

    fn fit_read(&mut self, gui: &mut Gui, _: &mut dyn Textures, _: Source) {
        self.page.fit_read(gui);
    }

    fn focused(&mut self, tag: &str, vm: &ViewModel) {
        self.page.focused(tag, vm);
    }

    fn left(&mut self, tag: &str) {
        self.page.left(tag);
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

/// Line `strref` of KOTOR 2's talk table (the skin's), for a label of a
/// KOTOR 2 layout whose words KOTOR 2's own code sets, as its skills page
/// and its store do for each of their modes. The layout's own words there
/// are a stand-in, and the campaign's talk table may hold another line at
/// that number (K1's "Buy" where KOTOR 2's is "Buying Items").
pub fn skin_line(strref: u32) -> ControlState {
    ControlState {
        strref: Some(strref),
        skin_strref: true,
        ..ControlState::default()
    }
}

/// The line of KOTOR 2's talk table that `table` gives label `tag` in a
/// screen's first mode, or in its `second` (buying or selling, making or
/// breaking down); none for a label the table does not name.
pub fn mode_line(table: &[(&str, (u32, u32))], tag: &str, second: bool) -> Option<ControlState> {
    let (first, other) = table.iter().find(|(label, _)| *label == tag)?.1;
    Some(skin_line(if second { other } else { first }))
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
