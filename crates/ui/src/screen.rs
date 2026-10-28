//! Open screens, one over another, and the focus on the top one.
//!
//! - The focus moves along a control's `MOVETO` links (by `ID`); where a
//!   direction has none, to the nearest focusable control that way.
//! - A focused list box takes up and down itself, row by row, keeping the
//!   row in view, and in a chain row (KOTOR's feat flow) left and right
//!   cell by cell; activating it activates the row.
//! - The pointer focuses what it is over (through the window's scaling),
//!   in a chain row the cell it is over, and a click activates it (a
//!   second soon after on the same place is a double click); a click on no
//!   control is the screen's (the map page's picture); the wheel scrolls
//!   the list under it.
//! - Typed text goes to the focused control when the screen edits it.
//! - The focus pulses 0.2 to 1 and back in 1.6 s; a pulsing border (the
//!   HUD's level-up mark) 1 to 0.4 and back in 1.5 s, as KOTOR's.
//! - Back closes the screen unless it says otherwise; opening and closing
//!   screens the stack does itself, other intents leave it.
//! - A popup (a root smaller than its frame) over other screens darkens
//!   them.
//! - A list whose rows roll (the credits) rises with the frames' time; the
//!   screen hears when its last row has gone.
//! - A double click on a list's row presses the screen's button for it
//!   (`Screen::primary`).
//! - The controls sound as K1's (`GuiSound`): a click clicks, the focus
//!   or a list's cursor coming to a control enters.

use std::collections::BTreeMap;

use fmt_odyssey::gui::{Control, Gui, Kind, Rect};

use crate::assets::Assets;
use crate::draw::{DrawList, Quad, Textures, fit, window_to_layout};
use crate::edit::EditBox;
use crate::input::{Key, UiInput};
use crate::intent::{GuiSound, Intent};
use crate::paint::{
    ControlState, ListView, Painter, Palette, flow_cell_at, proto_height, row_height, rows_that_fit,
};
use crate::skin::Source;
use crate::vm::ViewModel;

/// A screen's name (`main_menu`, `inventory`, …).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ScreenId(pub &'static str);

/// One KOTOR layout and what its controls mean.
pub trait Screen: Send + Sync {
    fn id(&self) -> ScreenId;
    /// The layout's resource name, without `.gui`.
    fn layout(&self) -> &str;
    /// How a control shows this frame (the stack adds the focus).
    fn state(&self, _control: &Control, _vm: &ViewModel) -> ControlState {
        ControlState::default()
    }
    /// A control was activated.
    fn press(&mut self, tag: &str, vm: &ViewModel, out: &mut Vec<Intent>);
    /// A list box's row was activated.
    fn row(&mut self, _list: &str, _index: usize, _vm: &ViewModel, _out: &mut Vec<Intent>) {}
    /// A list box's cursor moved to `index` (what the screen's buttons act
    /// on).
    fn pointed(&mut self, _list: &str, _index: usize, _vm: &ViewModel) {}
    /// A list box's cursor moved to cell `cell` of row `index`, a chain
    /// row's (heard after `pointed`).
    fn cell(&mut self, _list: &str, _index: usize, _cell: usize, _vm: &ViewModel) {}
    /// A click, before the stack acts on it; true when the screen took it
    /// (a line that a click skips).
    fn clicked(&mut self, _vm: &ViewModel, _out: &mut Vec<Intent>) -> bool {
        false
    }
    /// The focus left the list `list` (the pointer went to another control):
    /// what it only pointed at is read no more.
    fn left(&mut self, _list: &str) {}
    /// A second click on `tag` (the list row or the button the first one
    /// chose or pressed), soon after the first.
    fn double(&mut self, _tag: &str, _vm: &ViewModel, _out: &mut Vec<Intent>) {}
    /// The button a double click on a row of the list `list` presses, the
    /// row chosen by its first click (Buy on a store's row, Equip on an
    /// item's): heard after `double`, unless the button is disabled.
    fn primary(&self, _list: &str) -> Option<&'static str> {
        None
    }
    /// Shown by the game, not opened by the player (the loading screen):
    /// not heard opening or closing.
    fn quiet(&self) -> bool {
        false
    }
    /// Back (Escape, gamepad B).
    fn back(&mut self, out: &mut Vec<Intent>) {
        out.push(Intent::Close);
    }
    /// A key before the stack handles it; true when the screen used it.
    fn key(&mut self, _key: Key, _vm: &ViewModel, _out: &mut Vec<Intent>) -> bool {
        false
    }
    /// The text a control edits, when it is an edit box.
    fn edit(&mut self, _tag: &str) -> Option<&mut EditBox> {
        None
    }
    /// The focus moved to `tag` (a description to show for it).
    fn focused(&mut self, _tag: &str, _vm: &ViewModel) {}
    /// A slider moved to `value` (0 to 1).
    fn slide(&mut self, _tag: &str, _value: f32, _out: &mut Vec<Intent>) {}
    /// A key pressed, by name, before anything else sees it; true when the
    /// screen took it (and with it the frame's keys).
    fn raw(&mut self, _key: &str, _vm: &ViewModel, _out: &mut Vec<Intent>) -> bool {
        false
    }
    /// The edit box that takes typing whatever is focused (KOTOR's
    /// `EDITBOX` is a label).
    fn typing(&self) -> Option<&str> {
        None
    }
    /// A layout drawn under this one, its controls this screen's too (the
    /// in-game menu bar).
    fn under(&self) -> Option<&str> {
        None
    }
    /// Adjusts the layout once it is loaded (a box grown for its text).
    fn shape(&self, _gui: &mut Gui) {}
    /// The control focused when the screen opens.
    fn first_focus(&self) -> Option<&str> {
        None
    }
    /// Whether its lists show other rows than when last asked (one slot's
    /// list after another's): their cursors start at the top again.
    fn lists_anew(&mut self) -> bool {
        false
    }
    /// A rolling list's last row has gone (`ListView::roll`); heard each
    /// frame from then on.
    fn rolled(&mut self, _list: &str, _out: &mut Vec<Intent>) {}
    /// A click at `at` (layout pixels, as its controls' extents) on none of
    /// its controls that take one (the map page's picture).
    fn click(&mut self, _gui: &Gui, _at: [f32; 2], _textures: &mut dyn Textures, _vm: &ViewModel) {}
    /// Quads of its own over its layout's controls (the map page's picture
    /// and what lies on it), placed by the layout.
    fn quads(&self, _gui: &Gui, _vm: &ViewModel, _painter: &mut Painter) -> Vec<Quad> {
        Vec::new()
    }
}

/// Where a list box's view and cursor are.
#[derive(Debug, Clone, Copy, Default)]
struct Cursor {
    first: usize,
    row: usize,
    /// The cell of a chain row.
    cell: usize,
    /// How far a rolling list's rows have risen (layout pixels).
    risen: f32,
    /// The player scrolled it: a list that follows its end stays put.
    pinned: bool,
}

struct Entry {
    screen: Box<dyn Screen>,
    gui: Option<Gui>,
    /// Where `gui` was read.
    source: Source,
    /// The layouts `gui` was loaded from (the screen's, and the one under
    /// it): a screen that changes them is loaded again.
    loaded: (String, Option<String>),
    focus: Option<String>,
    lists: BTreeMap<String, Cursor>,
    /// The last click: its control, row and cell, and when.
    clicked: Option<((String, usize, usize), f32)>,
    clock: f32,
}

/// What a frame of the stack gives the game.
#[derive(Debug, Default)]
pub struct Frame {
    /// One list per open screen, bottom first.
    pub layers: Vec<DrawList>,
    pub intents: Vec<Intent>,
    /// The interface sounds this frame made (`GuiSound`), in order.
    pub sounds: Vec<GuiSound>,
    /// What did not resolve (layouts, textures, fonts).
    pub refusals: Vec<String>,
}

pub type Factory = fn(&str, &ViewModel) -> Option<Box<dyn Screen>>;

pub struct Stack {
    entries: Vec<Entry>,
    factory: Factory,
    clock: f32,
    pub palette: Palette,
    /// The screens are over a live world: it is dimmed under them.
    pub veil_world: bool,
    /// Tags to activate next frame (the console's `ui press`).
    presses: Vec<String>,
    /// List rows to double-click next frame (the console's `ui double`).
    doubles: Vec<(String, usize)>,
    /// Wheel motion not yet a whole notch (a trackpad's small steps).
    wheel: f32,
    /// The player is down (`fell`).
    down: bool,
    /// A screen as big as its frame was open after the last step.
    covered: bool,
}

const PULSE: f32 = 1.6;
/// How soon after a click another on the same place is a double click (s).
const DOUBLE_CLICK: f32 = 0.4;
/// A pulsing border's lowest alpha, and how long it takes to get there.
const PULSING_LOW: f32 = 0.4;
const PULSING_HALF: f32 = 0.75;
/// How far a key moves a slider.
const SLIDE: f32 = 0.05;
/// How dark what is under a popup gets, and the live world under the
/// in-game screens.
const VEIL: f32 = 0.8;
const WORLD_VEIL: f32 = 0.75;
/// How far past its frame, in layout pixels, a screen's veil reaches.
const VEIL_REACH: f32 = 16384.0;

fn focusable(control: &Control) -> bool {
    matches!(
        control.kind,
        Kind::Button | Kind::CheckBox | Kind::Slider | Kind::ListBox
    )
}

/// `under`'s controls put first among `gui`'s, placed from `gui`'s root.
fn beneath(gui: &mut Gui, under: Gui) {
    fn shift(control: &mut Control, dx: i32, dy: i32) {
        control.extent.left += dx;
        control.extent.top += dy;
        for inner in control
            .children
            .iter_mut()
            .chain(control.proto.as_deref_mut())
            .chain(control.scrollbar.as_deref_mut())
        {
            shift(inner, dx, dy);
        }
    }
    let dx = under.extent.left - gui.extent.left;
    let dy = under.extent.top - gui.extent.top;
    // Its root's frame too, as a panel of its own.
    let root = Control {
        kind: Kind::Panel,
        id: -2,
        tag: under.tag,
        parent: -1,
        extent: Rect {
            left: 0,
            top: 0,
            ..under.extent
        },
        border: under.border,
        color: under.color,
        alpha: under.alpha,
        ..Control::default()
    };
    let mut controls = vec![root];
    controls.extend(under.controls);
    for control in &mut controls {
        shift(control, dx, dy);
    }
    controls.append(&mut gui.controls);
    gui.controls = controls;
}

fn centre(control: &Control) -> [f32; 2] {
    let e = control.extent;
    [
        e.left as f32 + e.width as f32 / 2.0,
        e.top as f32 + e.height as f32 / 2.0,
    ]
}

fn contains(control: &Control, at: [f32; 2]) -> bool {
    let e = control.extent;
    at[0] >= e.left as f32
        && at[0] <= (e.left + e.width) as f32
        && at[1] >= e.top as f32
        && at[1] <= (e.top + e.height) as f32
}

impl Stack {
    pub fn new(factory: Factory) -> Stack {
        Stack {
            entries: Vec::new(),
            factory,
            clock: 0.0,
            palette: Palette::K1,
            veil_world: false,
            presses: Vec::new(),
            doubles: Vec::new(),
            wheel: 0.0,
            down: false,
            covered: false,
        }
    }

    /// Whether the player is down, each frame: falling closes every
    /// screen, once; what opens while down (the fall's own panel, the
    /// options) stays.
    pub fn fell(&mut self, down: bool) {
        if down && !self.down {
            self.clear();
        }
        self.down = down;
    }

    pub fn push(&mut self, screen: Box<dyn Screen>) {
        self.entries.push(Entry {
            screen,
            gui: None,
            source: Source::Campaign,
            loaded: (String::new(), None),
            focus: None,
            lists: BTreeMap::new(),
            clicked: None,
            clock: 0.0,
        });
    }

    pub fn pop(&mut self) {
        self.entries.pop();
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Whether a screen named `name` is open.
    pub fn has(&self, name: &str) -> bool {
        self.screens().any(|screen| screen.0 == name)
    }

    /// Closes the screens named `name`, wherever they are in the stack;
    /// those over them stay.
    pub fn close(&mut self, name: &str) {
        self.entries.retain(|entry| entry.screen.id().0 != name);
    }

    /// The screen named `name` open when `want`, closed when not (as
    /// `close`), whatever else is open.
    pub fn sync(&mut self, name: &str, want: bool, vm: &ViewModel) {
        match (want, self.has(name)) {
            (true, false) => {
                self.open(name, vm);
            }
            (false, true) => self.close(name),
            _ => {}
        }
    }

    /// The screen named `name` starts over: its focus and its lists' rows
    /// as when it opened (what it shows changed under it).
    pub fn rewind(&mut self, name: &str) {
        for entry in &mut self.entries {
            if entry.screen.id().0 == name {
                entry.focus = None;
                entry.lists.clear();
            }
        }
    }

    /// Opens `name` through the factory; false when there is no such
    /// screen.
    pub fn open(&mut self, name: &str, vm: &ViewModel) -> bool {
        match (self.factory)(name, vm) {
            Some(screen) => {
                self.push(screen);
                true
            }
            None => false,
        }
    }

    /// The top screen given way to `name`; false (and nothing changed)
    /// when there is no such screen.
    pub fn switch(&mut self, name: &str, vm: &ViewModel) -> bool {
        match (self.factory)(name, vm) {
            Some(screen) => {
                self.pop();
                self.push(screen);
                true
            }
            None => false,
        }
    }

    /// The stack's time (s), for what moves with it.
    pub fn clock(&self) -> f32 {
        self.clock
    }

    pub fn top(&self) -> Option<ScreenId> {
        self.entries.last().map(|entry| entry.screen.id())
    }

    pub fn screens(&self) -> impl Iterator<Item = ScreenId> {
        self.entries.iter().map(|entry| entry.screen.id())
    }

    pub fn is_open(&self) -> bool {
        !self.entries.is_empty()
    }

    pub fn focused(&self) -> Option<&str> {
        self.entries.last()?.focus.as_deref()
    }

    /// The text the top screen's control `tag` edits.
    pub fn edited(&mut self, tag: &str) -> Option<Vec<u8>> {
        let entry = self.entries.last_mut()?;
        entry.screen.edit(tag).map(|edit| edit.text.clone())
    }

    /// Activates `tag` on the top screen next frame.
    pub fn press(&mut self, tag: &str) {
        self.presses.push(tag.to_string());
    }

    /// Double-clicks row `row` of the top screen's list `tag` next frame,
    /// as two clicks on it soon after each other.
    pub fn double(&mut self, tag: &str, row: usize) {
        self.doubles.push((tag.to_string(), row));
    }

    /// How the top screen shows `tag` now, before the focus.
    pub fn state_of(&self, tag: &str, vm: &ViewModel) -> Option<ControlState> {
        let entry = self.entries.last()?;
        let control = entry.gui.as_ref()?.find(tag)?;
        Some(entry.state(control, vm))
    }

    /// The text the top screen gives `tag`, when it gives one.
    pub fn text_of(&self, tag: &str, vm: &ViewModel) -> Option<Vec<u8>> {
        self.state_of(tag, vm)?.text
    }

    pub fn hidden(&self, tag: &str, vm: &ViewModel) -> bool {
        self.state_of(tag, vm).is_some_and(|state| state.hidden)
    }

    /// The control of the top screen's layout under the pointer, focusable
    /// or not.
    pub fn under_pointer(&self, vm: &ViewModel, input: &UiInput) -> Option<String> {
        let entry = self.entries.last()?;
        let gui = entry.gui.as_ref()?;
        let (width, height) = gui.frame();
        let size = (width as f32, height as f32);
        let at = window_to_layout(fit(input.window, size), size, input.pointer?)?;
        let at = [
            at[0] - gui.extent.left as f32,
            at[1] - gui.extent.top as f32,
        ];
        let _ = vm;
        gui.walk()
            .filter(|control| contains(control, at))
            .last()
            .map(|control| control.tag.clone())
    }

    /// The focus's alpha now.
    pub fn pulse(&self) -> f32 {
        let phase = (self.clock % PULSE) / PULSE;
        let rise = if phase < 0.5 {
            phase * 2.0
        } else {
            2.0 - phase * 2.0
        };
        0.2 + 0.8 * rise
    }

    /// A pulsing border's alpha now (`ControlState::pulsing`): KOTOR's, 1
    /// down to 0.4 and back up, 0.75 s each way.
    pub fn pulsing(&self) -> f32 {
        let phase = (self.clock % (2.0 * PULSING_HALF)) / PULSING_HALF;
        let fall = if phase < 1.0 { phase } else { 2.0 - phase };
        1.0 - (1.0 - PULSING_LOW) * fall
    }

    /// Whether a screen as big as its frame (not a popup, not one the game
    /// shows by itself) is open.
    fn covers(&self) -> bool {
        self.entries.iter().any(|entry| {
            !entry.screen.quiet()
                && entry.gui.as_ref().is_some_and(|gui| {
                    let (width, height) = gui.frame();
                    gui.extent.width >= width && gui.extent.height >= height
                })
        })
    }

    /// Loads the layout of each screen that has none yet, and with `prune`
    /// closes those whose layout does not load (a screen opened this frame
    /// waits for the next).
    fn load(&mut self, assets: &mut dyn Assets, frame: &mut Frame, prune: bool) {
        for entry in &mut self.entries {
            let wanted = (
                entry.screen.layout().to_string(),
                entry.screen.under().map(str::to_string),
            );
            if entry.loaded != wanted {
                entry.gui = None;
                entry.focus = None;
                entry.lists.clear();
                entry.loaded = wanted;
            }
            if entry.gui.is_none() {
                match assets.layout(entry.screen.layout()) {
                    Ok((mut gui, source)) => {
                        match entry
                            .screen
                            .under()
                            .map(|name| assets.layout_in(name, source))
                        {
                            Some(Ok(under)) => beneath(&mut gui, under),
                            Some(Err(why)) => frame.refusals.push(format!("layout {why}")),
                            None => {}
                        }
                        entry.screen.shape(&mut gui);
                        entry.gui = Some(gui);
                        entry.source = source;
                    }
                    Err(why) => frame.refusals.push(format!("layout {why}")),
                }
            }
        }
        // A screen whose layout does not load closes at once: it would
        // draw nothing and take every key.
        if prune {
            self.entries.retain(|entry| entry.gui.is_some());
        }
    }

    pub fn step(&mut self, assets: &mut dyn Assets, vm: &ViewModel, input: &UiInput) -> Frame {
        self.clock += input.dt;
        let mut frame = Frame::default();
        self.load(assets, &mut frame, true);
        let presses = std::mem::take(&mut self.presses);
        let doubles = std::mem::take(&mut self.doubles);
        let mut intents = Vec::new();
        self.wheel += input.wheel;
        let notches = self.wheel.trunc();
        self.wheel -= notches;
        if let Some(entry) = self.entries.last_mut() {
            entry.clock = self.clock;
            entry.handle(
                assets,
                vm,
                input,
                notches,
                (&presses, &doubles),
                &mut intents,
            );
        }
        for intent in intents {
            match intent {
                Intent::Open(name) => {
                    if !self.open(name, vm) {
                        frame.refusals.push(format!("screen {name}"));
                    }
                }
                Intent::OpenFor(name, what) => {
                    if !self.open(&format!("{name}:{what}"), vm) {
                        frame.refusals.push(format!("screen {name} for {what}"));
                    }
                }
                Intent::Close => self.pop(),
                Intent::Swap(name) => {
                    if !self.switch(name, vm) {
                        frame.refusals.push(format!("screen {name}"));
                    }
                }
                Intent::CloseAll => self.clear(),
                Intent::Sound(sound) => frame.sounds.push(sound),
                other => frame.intents.push(other),
            }
        }
        // A screen opened or swapped to this frame is drawn this frame: a
        // tab's swap must not show the world between.
        self.load(assets, &mut frame, false);
        // KOTOR's in-game screens over the world: the first one opened and
        // the last one closed are heard.
        let covered = self.covers();
        if covered != self.covered && self.veil_world {
            frame.sounds.push(if covered {
                GuiSound::PanelAdded
            } else {
                GuiSound::PanelRemoved
            });
        }
        self.covered = covered;
        let pulse = self.pulse();
        let blink = self.clock % 1.0 < 0.5;
        let count = self.entries.len();
        for (at, entry) in self.entries.iter_mut().enumerate() {
            let mut out = DrawList::default();
            let mut painter = Painter::new(assets, self.palette);
            painter.pulse = pulse;
            painter.source = entry.source;
            entry.draw(vm, at + 1 == count, blink, &mut painter, &mut out);
            if let Some(gui) = &entry.gui {
                // Past the frame, to the window's edges whatever its shape
                // and size: a frame is drawn at its own size, so a window
                // can be many frames wide.
                let (width, height) = gui.frame();
                let (w, h) = (width as f32, height as f32);
                let veil = [
                    -VEIL_REACH,
                    -VEIL_REACH,
                    w + 2.0 * VEIL_REACH,
                    h + 2.0 * VEIL_REACH,
                ];
                let popup = gui.extent.width < width || gui.extent.height < height;
                if !popup {
                    // KOTOR 2's frame is opaque: black bars beside it in a
                    // window wider (or taller) than the frame.
                    out.quads.insert(0, Quad::solid(veil, [0.0, 0.0, 0.0, 1.0]));
                } else if at > 0 && at + 1 == count && popup {
                    out.quads
                        .insert(0, Quad::solid(veil, [0.0, 0.0, 0.0, VEIL]));
                } else if at == 0 && self.veil_world {
                    out.quads
                        .insert(0, Quad::solid(veil, [0.0, 0.0, 0.0, WORLD_VEIL]));
                }
            }
            frame.refusals.append(&mut painter.refusals);
            frame.layers.push(out);
        }
        frame
    }
}

impl Entry {
    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        self.screen.state(control, vm)
    }

    /// The controls the focus can rest on, in layout order.
    fn focusables<'g>(&self, gui: &'g Gui, vm: &ViewModel) -> Vec<&'g Control> {
        gui.walk()
            .filter(|control| focusable(control))
            .filter(|control| {
                let state = self.state(control, vm);
                // A list fitted to its text (a message) is read, not chosen.
                let read = state.list.as_ref().map_or(
                    control.kind == Kind::ListBox && state.strref.is_some(),
                    |list| list.fit && !list.pick,
                );
                !state.hidden && !state.disabled && !read
            })
            .collect()
    }

    fn handle(
        &mut self,
        textures: &mut dyn Textures,
        vm: &ViewModel,
        input: &UiInput,
        notches: f32,
        (presses, doubles): (&[String], &[(String, usize)]),
        out: &mut Vec<Intent>,
    ) {
        let Some(gui) = self.gui.take() else {
            return;
        };
        if self.screen.lists_anew() {
            self.lists.clear();
        }
        let before = self.focus.clone();
        let rows: BTreeMap<String, (usize, usize)> = self
            .lists
            .iter()
            .map(|(tag, cursor)| (tag.clone(), (cursor.row, cursor.cell)))
            .collect();
        let start = out.len();
        self.respond(&gui, textures, vm, input, notches, presses, out);
        for (tag, row) in doubles {
            self.double_row(&gui, vm, tag, *row, out);
        }
        self.roll(&gui, textures, vm, input.dt, out);
        // KOTOR's controls sound as the focus comes to them (each list row
        // is a control of its own there); not the first focus a screen
        // takes as it opens.
        let entered = before.is_some() && self.focus != before
            || self
                .lists
                .iter()
                .any(|(tag, cursor)| rows.get(tag).map_or(0, |(row, _)| *row) != cursor.row);
        if entered {
            out.insert(start, Intent::Sound(GuiSound::Entered));
        }
        if self.focus != before {
            if let Some(tag) = &before {
                self.screen.left(tag);
            }
            if let Some(tag) = &self.focus {
                self.screen.focused(tag, vm);
                // A list taken up again points at where its cursor stands.
                if let Some(cursor) = self.lists.get(tag) {
                    self.screen.pointed(tag, cursor.row, vm);
                    self.screen.cell(tag, cursor.row, cursor.cell, vm);
                }
            }
        }
        for (tag, cursor) in &self.lists {
            let (row, cell) = rows.get(tag).copied().unwrap_or((0, 0));
            if row != cursor.row {
                self.screen.pointed(tag, cursor.row, vm);
            }
            if (row, cell) != (cursor.row, cursor.cell) {
                self.screen.cell(tag, cursor.row, cursor.cell, vm);
            }
        }
        self.gui = Some(gui);
    }

    #[allow(clippy::too_many_arguments)]
    fn respond(
        &mut self,
        gui: &Gui,
        textures: &mut dyn Textures,
        vm: &ViewModel,
        input: &UiInput,
        notches: f32,
        presses: &[String],
        out: &mut Vec<Intent>,
    ) {
        if input.raw.iter().any(|key| self.screen.raw(key, vm, out)) {
            return;
        }
        let targets = self.focusables(gui, vm);
        let valid = |tag: &str| targets.iter().any(|control| control.tag == tag);
        if !self.focus.as_deref().is_some_and(valid) {
            self.focus = self
                .screen
                .first_focus()
                .filter(|tag| valid(tag))
                .map(str::to_string)
                .or_else(|| targets.first().map(|control| control.tag.clone()));
        }
        let origin = [gui.extent.left as f32, gui.extent.top as f32];
        let (width, height) = gui.frame();
        let size = (width as f32, height as f32);
        let pointed = input.pointer.and_then(|point| {
            let at = window_to_layout(fit(input.window, size), size, point)?;
            Some([at[0] - origin[0], at[1] - origin[1]])
        });
        let under = pointed.and_then(|at| {
            let control = targets.iter().rev().find(|control| contains(control, at))?;
            Some((control.tag.clone(), at))
        });
        if input.moved
            && let Some((tag, at)) = &under
        {
            self.focus = Some(tag.clone());
            self.point_row(gui, textures, vm, tag, *at);
        }
        for key in &input.keys {
            if self.screen.key(*key, vm, out) {
                continue;
            }
            match key {
                Key::Left | Key::Right if self.slide_by(gui, vm, *key, out) => {}
                Key::Up | Key::Down | Key::Left | Key::Right => {
                    if !self.list_step(gui, textures, vm, *key) {
                        self.move_focus(gui, &targets, *key);
                    }
                }
                Key::Activate => {
                    self.sound_click(gui, out);
                    self.activate(gui, vm, out);
                }
                Key::Back => self.screen.back(out),
                Key::ScrollUp => self.scroll(gui, vm, None, -1),
                Key::ScrollDown => self.scroll(gui, vm, None, 1),
                Key::Backspace => {
                    if let Some(tag) = self.typing_tag()
                        && let Some(edit) = self.screen.edit(&tag)
                    {
                        edit.backspace();
                    }
                }
                Key::NextTab | Key::PrevTab => {}
            }
        }
        if !input.text.is_empty()
            && let Some(tag) = self.typing_tag()
            && let Some(edit) = self.screen.edit(&tag)
        {
            edit.type_text(&input.text);
        }
        if notches != 0.0
            && let Some((tag, _)) = &under
        {
            self.scroll(gui, vm, Some(tag.as_str()), -notches as i32);
        }
        if input.click
            && under.is_none()
            && let Some(at) = pointed
        {
            self.screen.click(gui, at, textures, vm);
        }
        if input.click
            && !self.screen.clicked(vm, out)
            && let Some((tag, at)) = under
        {
            self.focus = Some(tag.clone());
            match gui
                .find(&tag)
                .filter(|control| control.kind == Kind::Slider)
            {
                Some(slider) => {
                    let e = slider.extent;
                    let value = (at[0] - e.left as f32) / e.width.max(1) as f32;
                    self.screen.slide(&tag, value.clamp(0.0, 1.0), out);
                }
                None => {
                    self.point_row(gui, textures, vm, &tag, at);
                    self.sound_click(gui, out);
                    self.activate(gui, vm, out);
                    self.double_click(gui, vm, &tag, out);
                }
            }
        }
        for tag in presses {
            if gui.find(tag).is_some() {
                self.focus = Some(tag.clone());
                self.sound_click(gui, out);
                self.activate(gui, vm, out);
            }
        }
    }

    /// A click on `tag` after the one before it on the same row and cell:
    /// the screen hears a double click, and a list's row presses the
    /// screen's button for it (`Screen::primary`).
    fn double_click(&mut self, gui: &Gui, vm: &ViewModel, tag: &str, out: &mut Vec<Intent>) {
        let (row, cell) = self
            .lists
            .get(tag)
            .map_or((0, 0), |cursor| (cursor.row, cursor.cell));
        let place = (tag.to_string(), row, cell);
        match self.clicked.take() {
            Some((last, when)) if last == place && self.clock - when <= DOUBLE_CLICK => {
                self.screen.double(tag, vm, out);
                let button = self.screen.primary(tag).filter(|button| {
                    gui.find(button)
                        .is_some_and(|control| !self.state(control, vm).disabled)
                });
                if let Some(button) = button {
                    self.screen.press(button, vm, out);
                }
            }
            _ => self.clicked = Some((place, self.clock)),
        }
    }

    /// Two clicks on row `row` of the list `tag`, as the pointer makes
    /// them (the console's `ui double`).
    fn double_row(
        &mut self,
        gui: &Gui,
        vm: &ViewModel,
        tag: &str,
        row: usize,
        out: &mut Vec<Intent>,
    ) {
        let Some((count, shown)) = self.rows(gui, vm, tag) else {
            return;
        };
        if row >= count {
            return;
        }
        self.focus = Some(tag.to_string());
        let cursor = self.lists.entry(tag.to_string()).or_default();
        cursor.row = row;
        Entry::keep_in_view(cursor, shown);
        for _ in 0..2 {
            self.sound_click(gui, out);
            self.activate(gui, vm, out);
            self.double_click(gui, vm, tag, out);
        }
    }

    /// The focused control's click: a check box's, else a button's or a
    /// list row's (`guisounds.2da`, as KOTOR's controls are made).
    fn sound_click(&self, gui: &Gui, out: &mut Vec<Intent>) {
        let kind = self
            .focus
            .as_deref()
            .and_then(|tag| gui.find(tag))
            .map(|control| control.kind);
        match kind {
            Some(Kind::CheckBox) => out.push(Intent::Sound(GuiSound::Check)),
            Some(Kind::Button | Kind::ListBox) => out.push(Intent::Sound(GuiSound::Clicked)),
            _ => {}
        }
    }

    /// Fitted lists whose rows roll rise by `dt`'s share; the screen hears
    /// of each whose last row has gone above it.
    fn roll(
        &mut self,
        gui: &Gui,
        textures: &mut dyn Textures,
        vm: &ViewModel,
        dt: f32,
        out: &mut Vec<Intent>,
    ) {
        for list in gui.walk().filter(|control| control.kind == Kind::ListBox) {
            let Some(speed) = self.state(list, vm).list.and_then(|view| view.roll) else {
                continue;
            };
            let Some((heights, _)) = self.fitted(gui, textures, vm, &list.tag) else {
                continue;
            };
            let cursor = self.lists.entry(list.tag.clone()).or_default();
            cursor.risen += dt * speed;
            if cursor.risen >= (heights.iter().sum::<i32>() + list.extent.height) as f32 {
                self.screen.rolled(&list.tag, out);
            }
        }
    }

    /// Left and right on a focused slider: a twentieth of its way.
    fn slide_by(&mut self, gui: &Gui, vm: &ViewModel, key: Key, out: &mut Vec<Intent>) -> bool {
        let Some(tag) = self.focus.clone() else {
            return false;
        };
        let Some(slider) = gui
            .find(&tag)
            .filter(|control| control.kind == Kind::Slider)
        else {
            return false;
        };
        let now = self.state(slider, vm).value.unwrap_or(0.0);
        let step = if key == Key::Right { SLIDE } else { -SLIDE };
        self.screen.slide(&tag, (now + step).clamp(0.0, 1.0), out);
        true
    }

    /// Where typing goes: the screen's typing target, else the focus.
    fn typing_tag(&self) -> Option<String> {
        self.screen
            .typing()
            .map(str::to_string)
            .or_else(|| self.focus.clone())
    }

    /// A list box's rows as the screen shows them.
    fn view(&self, gui: &Gui, vm: &ViewModel, tag: &str) -> Option<(Control, Option<ListView>)> {
        let control = gui
            .find(tag)
            .filter(|control| control.kind == Kind::ListBox)?;
        Some((control.clone(), self.state(control, vm).list))
    }

    fn rows(&self, gui: &Gui, vm: &ViewModel, tag: &str) -> Option<(usize, usize)> {
        let (control, view) = self.view(gui, vm, tag)?;
        let count = view.as_ref().map_or(0, |list| list.rows.len());
        Some((count, rows_that_fit(&control, view.as_ref()).max(1)))
    }

    /// How many cells row `row` of a list has (none but in a chain row).
    fn cells(&self, gui: &Gui, vm: &ViewModel, tag: &str, row: usize) -> usize {
        self.view(gui, vm, tag)
            .and_then(|(_, view)| Some(view?.rows.get(row)?.cells.len()))
            .unwrap_or(0)
    }

    fn keep_in_view(cursor: &mut Cursor, shown: usize) {
        if cursor.row < cursor.first {
            cursor.first = cursor.row;
        } else if cursor.row >= cursor.first + shown {
            cursor.first = cursor.row + 1 - shown;
        }
    }

    /// Up and down inside a focused list; false when the key is not the
    /// list's (another direction, an empty list, or past its end).
    fn list_step(
        &mut self,
        gui: &Gui,
        textures: &mut dyn Textures,
        vm: &ViewModel,
        key: Key,
    ) -> bool {
        let Some(tag) = self.focus.clone() else {
            return false;
        };
        let Some((count, shown)) = self.rows(gui, vm, &tag) else {
            return false;
        };
        let fitted = self.fitted(gui, textures, vm, &tag);
        let at = self.lists.get(&tag).copied().unwrap_or(Cursor::default());
        let cells = self.cells(gui, vm, &tag, at.row);
        let next_cells = |row: usize| self.cells(gui, vm, &tag, row);
        let (next, cell) = match key {
            Key::Up if at.row > 0 => (at.row - 1, at.cell.min(next_cells(at.row - 1).max(1) - 1)),
            Key::Down if at.row + 1 < count => {
                (at.row + 1, at.cell.min(next_cells(at.row + 1).max(1) - 1))
            }
            Key::Left if at.cell > 0 => (at.row, at.cell - 1),
            Key::Right if at.cell + 1 < cells => (at.row, at.cell + 1),
            _ => return false,
        };
        let cursor = self.lists.entry(tag).or_default();
        cursor.row = next;
        cursor.cell = cell;
        match fitted {
            // Rows as tall as their words: from the first shown, as many
            // as the list's height holds.
            Some((heights, room)) => {
                cursor.first = cursor.first.min(cursor.row);
                while cursor.first < cursor.row
                    && heights[cursor.first..=cursor.row].iter().sum::<i32>() > room
                {
                    cursor.first += 1;
                }
            }
            None => Entry::keep_in_view(cursor, shown),
        }
        true
    }

    /// A fitted list's rows' heights with the gap after each, and the
    /// height they are shown in; none for a list of fixed rows.
    fn fitted(
        &self,
        gui: &Gui,
        textures: &mut dyn Textures,
        vm: &ViewModel,
        tag: &str,
    ) -> Option<(Vec<i32>, i32)> {
        let list = gui
            .find(tag)
            .filter(|control| control.kind == Kind::ListBox)?;
        let proto = list.proto.as_deref()?;
        let view = self.state(list, vm).list.filter(|view| view.fit)?;
        let heights = view
            .rows
            .iter()
            .map(|row| row_height(textures, self.source, proto, true, &row.text) + list.padding)
            .collect();
        let room = list.extent.top + list.extent.height - proto.extent.top;
        Some((heights, room))
    }

    fn scroll(&mut self, gui: &Gui, vm: &ViewModel, tag: Option<&str>, by: i32) {
        let Some(tag) = tag.map(str::to_string).or_else(|| self.focus.clone()) else {
            return;
        };
        let Some((count, shown)) = self.rows(gui, vm, &tag) else {
            return;
        };
        let cursor = self.lists.entry(tag).or_default();
        let last = count.saturating_sub(shown) as i32;
        if !cursor.pinned {
            // Scrolling a list that followed its end starts from there.
            cursor.first = last.max(0) as usize;
            cursor.pinned = true;
        }
        cursor.first = (cursor.first as i32 + by).clamp(0, last.max(0)) as usize;
        cursor.row = cursor.row.clamp(cursor.first, cursor.first + shown - 1);
    }

    fn point_row(
        &mut self,
        gui: &Gui,
        textures: &mut dyn Textures,
        vm: &ViewModel,
        tag: &str,
        at: [f32; 2],
    ) {
        let Some(list) = gui
            .find(tag)
            .filter(|control| control.kind == Kind::ListBox)
        else {
            return;
        };
        let (Some(proto), Some((count, _))) = (list.proto.as_deref(), self.rows(gui, vm, tag))
        else {
            return;
        };
        let view = self.view(gui, vm, tag).and_then(|(_, view)| view);
        let height = proto_height(proto, view.as_ref());
        let below = at[1] - proto.extent.top as f32;
        if below < 0.0 {
            return;
        }
        let fitted = self.fitted(gui, textures, vm, tag);
        let cursor = self.lists.entry(tag.to_string()).or_default();
        let row = match fitted {
            Some((heights, _)) => {
                let mut top = 0.0;
                let pointed = heights.iter().skip(cursor.first).position(|height| {
                    top += *height as f32;
                    below < top
                });
                let Some(offset) = pointed else {
                    return;
                };
                cursor.first + offset
            }
            None => {
                let step = (height + list.padding).max(1) as f32;
                cursor.first + (below / step).floor() as usize
            }
        };
        if row < count {
            cursor.row = row;
            let cells = view
                .as_ref()
                .and_then(|view| view.rows.get(row))
                .map_or(0, |row| row.cells.len());
            cursor.cell = if cells > 0 {
                flow_cell_at(proto, at[0], cells)
            } else {
                0
            };
        }
    }

    fn move_focus(&mut self, gui: &Gui, targets: &[&Control], key: Key) {
        let Some(from) = self.focus.as_deref().and_then(|tag| gui.find(tag)) else {
            return;
        };
        let linked = from.moveto.map(|links| match key {
            Key::Up => links.up,
            Key::Down => links.down,
            Key::Left => links.left,
            _ => links.right,
        });
        if let Some(id) = linked.filter(|id| *id >= 0)
            && let Some(to) = targets.iter().find(|control| control.id == id)
        {
            self.focus = Some(to.tag.clone());
            return;
        }
        let direction = match key {
            Key::Up => [0.0, -1.0],
            Key::Down => [0.0, 1.0],
            Key::Left => [-1.0, 0.0],
            _ => [1.0, 0.0],
        };
        let [x, y] = centre(from);
        let nearest = targets
            .iter()
            .filter(|control| control.tag != from.tag)
            .filter_map(|control| {
                let [cx, cy] = centre(control);
                let (dx, dy) = (cx - x, cy - y);
                let along = dx * direction[0] + dy * direction[1];
                let across = (dx * direction[1] - dy * direction[0]).abs();
                (along > 0.0).then_some((along + 2.0 * across, control))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((_, to)) = nearest {
            self.focus = Some(to.tag.clone());
        }
    }

    fn activate(&mut self, gui: &Gui, vm: &ViewModel, out: &mut Vec<Intent>) {
        let Some(tag) = self.focus.clone() else {
            return;
        };
        match self.rows(gui, vm, &tag) {
            Some((count, _)) => {
                let row = self.lists.get(&tag).map_or(0, |cursor| cursor.row);
                if row < count {
                    // The screen knows the cell before it hears the row.
                    let cell = self.lists.get(&tag).map_or(0, |cursor| cursor.cell);
                    self.screen.pointed(&tag, row, vm);
                    self.screen.cell(&tag, row, cell, vm);
                    self.screen.row(&tag, row, vm, out);
                }
            }
            None => self.screen.press(&tag, vm, out),
        }
    }

    fn draw(
        &mut self,
        vm: &ViewModel,
        top: bool,
        blink: bool,
        painter: &mut Painter,
        out: &mut DrawList,
    ) {
        let Some(gui) = self.gui.take() else {
            return;
        };
        self.paint(&gui, vm, top, blink, painter, out);
        self.gui = Some(gui);
    }

    fn paint(
        &mut self,
        gui: &Gui,
        vm: &ViewModel,
        top: bool,
        blink: bool,
        painter: &mut Painter,
        out: &mut DrawList,
    ) {
        let mut states: BTreeMap<String, ControlState> = BTreeMap::new();
        let root = Control {
            kind: Kind::Panel,
            tag: gui.tag.clone(),
            ..Control::default()
        };
        states.insert(gui.tag.clone(), self.state(&root, vm));
        for control in gui.walk() {
            let mut state = self.state(control, vm);
            let focused = top && self.focus.as_deref() == Some(control.tag.as_str());
            state.focused = focused && control.kind != Kind::ListBox;
            if let Some(list) = &mut state.list {
                let cursor = self
                    .lists
                    .get(&control.tag)
                    .copied()
                    .unwrap_or(Cursor::default());
                list.first = cursor.first.min(list.rows.len().saturating_sub(1));
                list.follow &= !cursor.pinned;
                list.focused = focused.then_some(cursor.row);
                if list.roll.is_some() {
                    list.offset = cursor.risen as i32 - control.extent.height;
                }
            }
            let typing = top && self.typing_tag().as_deref() == Some(control.tag.as_str());
            if let Some(edit) = self.screen.edit(&control.tag) {
                let mut text = edit.text.clone();
                if typing && blink {
                    text.push(b'_');
                }
                state.text = Some(text);
            }
            states.insert(control.tag.clone(), state);
        }
        let blank = ControlState::default();
        painter.layout(
            gui,
            &|control| states.get(&control.tag).cloned().unwrap_or(blank.clone()),
            out,
        );
        out.quads.extend(self.screen.quads(gui, vm, painter));
    }
}
