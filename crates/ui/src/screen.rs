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
use crate::draw::{
    DrawList, MenuWidth, Quad, Sizing, SmallFonts, TextScale, Textures, fit, image_aspect, sized,
    window_to_layout,
};
use crate::edit::EditBox;
use crate::input::{Key, UiInput};
use crate::intent::{GuiSound, Intent};
use crate::paint::{
    ControlState, Hint, ListView, Painter, Palette, flow_cell_at, proto_height, row_height,
    rows_that_fit,
};
use crate::skin::Source;
use crate::strings;
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
    /// Whether Back throws away what changed on it (its hint says Cancel),
    /// not keeps it.
    fn cancels(&self) -> bool {
        false
    }
    /// The button a pad's Menu presses: its OK, Done or Accept; none found
    /// (`ACCEPTS`), Menu closes it as Back does.
    fn accept(&self) -> Option<&str> {
        None
    }
    /// The button a pad's Y presses: its second (`SECONDS`, Default).
    fn second(&self) -> Option<&str> {
        None
    }
    /// The page's own tabs, in order (an item filter's buttons, a log's):
    /// the bumpers step through them where the screen has no menu bar,
    /// the triggers where it has.
    fn tabs(&self) -> Vec<&'static str> {
        Vec::new()
    }
    /// Whether it is under the menu bar, whose tabs the bumpers step.
    fn tab_bar(&self, _vm: &ViewModel) -> bool {
        false
    }
    /// The button a pad's X presses on it in place of a double click (a
    /// container's Take all), when it is usable.
    fn pad_x(&self) -> Option<&str> {
        None
    }
    /// Whether the list `list` chooses its row as the cursor moves to it
    /// (`pointed`: the save picked): Activate on a row then does its
    /// primary (Load), as a double click does, not choose it again.
    fn activates_primary(&self, _list: &str) -> bool {
        false
    }
    /// The button whose words name a double click on `tag` when it is no
    /// list (a party slot's Add), for the pad's hint.
    fn double_button(&self, _tag: &str) -> Option<&'static str> {
        None
    }
    /// The list that the scroll keys (Page Up, Page Down and the right
    /// stick) always scroll on this screen, whatever has the focus. With
    /// none, they scroll the focused list when it overflows, else the first
    /// read list that does.
    fn scrolls(&self) -> Option<&str> {
        None
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
    /// Measures or adjusts the layout as it was read and shaped, before the
    /// stack stretches it to the window. The stack calls it once for each
    /// layout it loads (the Feedback page's rows made taller, the controls
    /// under the equip page's description box noted).
    fn fit_read(&mut self, _gui: &mut Gui, _textures: &mut dyn Textures, _source: Source) {}
    /// Adjusts the layout at the size it is drawn, measuring its words in
    /// the fonts `source` draws with (a message box grown to them). The
    /// stack calls it after every stretch of a layout it stretches, and
    /// once after `fit_read` for a layout it does not stretch.
    fn fit_drawn(&mut self, _gui: &mut Gui, _textures: &mut dyn Textures, _source: Source) {}
    /// The control focused when the screen opens.
    fn first_focus(&self) -> Option<&str> {
        None
    }
    /// Whether its lists show other rows than when last asked (one slot's
    /// list after another's): their cursors start at the top again.
    fn lists_anew(&mut self) -> bool {
        false
    }
    /// A list whose cursor the screen puts on a row of its own (the
    /// inventory opened on a datapad): the list takes the focus with its
    /// cursor there. Asked each frame; `None` once it has said.
    fn cursor_to(&mut self, _vm: &ViewModel) -> Option<(&'static str, usize)> {
        None
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
    /// Moves the screen's own animations on by `dt` seconds, while it is
    /// the top screen. `focus` is the control focused at the end of the
    /// last frame. The stack calls this before it takes the frame's input,
    /// so a focus that moves in a frame starts to show in the next frame,
    /// as KOTOR 2's growing class tiles do.
    fn animate(&mut self, _dt: f32, _focus: Option<&str>) {}
    /// The controls that the screen draws larger than the layout has them
    /// now (KOTOR 2's class tiles), each with how far it has grown. The
    /// stack draws them and tests the pointer against them at that size.
    fn grown(&self) -> Vec<(&'static str, Grow)> {
        Vec::new()
    }
}

/// How far a control has grown beyond its extent in the layout
/// (`Screen::grown`). It grows about its centre, by the same amount on
/// every side. The amounts are pixels of the layout as it was read. When
/// KOTOR 2's stretch widens the layout (`stretch`), they widen with it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grow {
    /// How far each side has moved out now.
    pub by: f32,
    /// How far each side moves out at the most. A 3D view's image is made
    /// at that size (`ControlState::scene_size`), so that the view is not
    /// read again while the control grows.
    pub most: f32,
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
    /// How far into row `first` a list scrolled by lines is (layout
    /// pixels; `Reach::lines`).
    offset: i32,
    /// The words a list scrolled by lines last showed: other words (another
    /// item's description) start again from their top.
    words: u64,
}

/// How far a list scrolls, and where it is: by rows; or, a fitted list
/// that is read rather than chosen (an item's description, a message,
/// often one row taller than the list), by its text's lines, in layout
/// pixels (`lines`, each row's height and the gap after it).
struct Reach {
    count: usize,
    shown: usize,
    /// The furthest it goes.
    last: usize,
    at: usize,
    /// One step of the wheel or a scroll key.
    step: usize,
    lines: Option<Vec<i32>>,
}

struct Entry {
    screen: Box<dyn Screen>,
    gui: Option<Gui>,
    /// Where `gui` was read.
    source: Source,
    /// The layouts `gui` was loaded from (the screen's, and the one under
    /// it): a screen that changes them is loaded again.
    loaded: (String, Option<String>),
    /// How many times its font's size text was drawn when `gui` was loaded
    /// (`Sizing::text` over `Sizing::pixel`). A layout fitted to its text
    /// when it is read (`Screen::fit_read`) is loaded again when this
    /// changes: the player changed the text scale, or the window crossed a
    /// width where KOTOR 2's text grows.
    text_size: f32,
    /// Set when the layout is a full-screen one (`full_screen`) or one of
    /// KOTOR 2's popups (`popup`): `gui` is then stretched to the window
    /// from it (`stretch`).
    stretched: Option<Stretched>,
    /// How much wider and taller `stretch` made the layout than it was
    /// read: (1, 1) when it was not stretched.
    stretch: (f32, f32),
    /// `gui` as drawn and hit this frame, with the controls the screen
    /// grows (`Screen::grown`) at their size now; none when it grows none.
    drawn: Option<Gui>,
    /// The extent each control the screen grows has at its largest.
    largest: BTreeMap<&'static str, Rect>,
    focus: Option<String>,
    lists: BTreeMap<String, Cursor>,
    /// The last click: its control, row and cell, and when.
    clicked: Option<((String, usize, usize), f32)>,
    /// The list whose scrollbar's thumb the pointer drags, and where on
    /// the thumb it took hold (layout pixels from its top).
    dragged: Option<(String, f32)>,
    clock: f32,
}

/// A full-screen layout or a popup as read and shaped (`Screen::fit_read`),
/// kept to be stretched again when the window's size or the Interface
/// settings change;
/// and the placement it was last stretched for (none yet when it was just
/// read).
struct Stretched {
    read: Gui,
    placed: Option<Placement>,
    /// Its pictures may keep their shape: it is the game's own layout, not
    /// a mod's (`stretch`).
    pictures: bool,
    /// It is a popup that KOTOR 2 centres in the window (`CENTRED`).
    centred: bool,
}

/// What a full-screen layout is stretched for: a window of `window` pixels,
/// and kjk's Interface settings, the menu's width and whether the game's
/// own pictures keep their shape (`draw::IMAGE_ASPECT`).
#[derive(Clone, Copy, PartialEq)]
struct Placement {
    window: (f32, f32),
    width: MenuWidth,
    pictures: bool,
}

impl Placement {
    /// The placement in a window of `window` pixels with the settings `vm`
    /// has now; each setting that is unset has its default.
    fn of(vm: &ViewModel, window: (f32, f32)) -> Placement {
        let setting = |key: &str| vm.settings.get(key).map(String::as_str);
        Placement {
            window,
            width: setting(MenuWidth::SETTING).map_or(MenuWidth::default(), MenuWidth::named),
            pictures: image_aspect(&vm.settings),
        }
    }

    /// How much `stretch` widens and heightens a full-screen layout. The
    /// stretched frame is a whole number of layout pixels, rounded down, so
    /// that `fit` keeps kjk's scale. With `MenuWidth::UpTo16x9` it is no
    /// wider than 16:9, and `fit` centres it in a wider window. With
    /// `MenuWidth::Kept` the layout keeps its own size, which `fit` scales
    /// evenly.
    fn stretch_by(self) -> (f32, f32) {
        let scale = crate::draw::scale(self.window);
        let tall = (self.window.1 / scale).floor();
        let wide = (self.window.0 / scale).floor();
        let wide = match self.width {
            MenuWidth::Fill => wide,
            MenuWidth::UpTo16x9 => wide.min((tall * 16.0 / 9.0).floor()),
            MenuWidth::Kept => return (1.0, 1.0),
        };
        (wide / KOTOR2_FRAME.0 as f32, tall / KOTOR2_FRAME.1 as f32)
    }

    /// Whether a full-screen layout may be narrower or shorter than the
    /// window, with black bars beside it: then nothing of it is drawn past
    /// its frame.
    fn barred(self) -> bool {
        self.width != MenuWidth::Fill
    }
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

/// The frame KOTOR 2's full-screen layouts are made for.
const KOTOR2_FRAME: (i32, i32) = (800, 600);

/// Whether `gui` is a full-screen layout of KOTOR 2's: its root fills the
/// 800 × 600 frame. Popups (smaller roots) and KOTOR 1's 640 × 480 layouts
/// are not.
fn full_screen(gui: &Gui) -> bool {
    gui.frame() == KOTOR2_FRAME
        && gui.extent.width >= KOTOR2_FRAME.0
        && gui.extent.height >= KOTOR2_FRAME.1
}

/// Whether `gui` is a popup of KOTOR 2's: its root is smaller than its
/// frame, and the frame is no larger than KOTOR 2's 800 × 600 (a message
/// box, the save name box, the container). KOTOR 2 places a popup in the
/// same 800 × 600 frame as its full-screen layouts, though most of its
/// popups were made for KOTOR 1's 640 × 480 one (`stretch`).
///
/// A layout whose root fills a 640 × 480 frame is not a popup by its shape.
/// These are KOTOR 1's full-screen layouts (the galaxy map the campaign
/// draws itself), which keep kjk's placement, scaled evenly and centred.
/// KOTOR 2's pazaak wager box has the same shape, so it is named in
/// `FRAMED` and is a popup by its name (`popup_named`).
fn popup(gui: &Gui) -> bool {
    let (width, height) = gui.frame();
    width <= KOTOR2_FRAME.0
        && height <= KOTOR2_FRAME.1
        && (gui.extent.width < width || gui.extent.height < height)
}

/// The popups whose root fills a whole 640 × 480 frame, by their layouts'
/// names here: the pazaak wager box. KOTOR 2 stretches such a popup in its
/// 800 × 600 frame like any other, root and all, and does not centre it.
/// Its 640 × 480 root then fills four fifths of the window from the top
/// left corner, and the box, which sits in the middle of that root, is
/// drawn up and to the left of the window's middle.
const FRAMED: &[&str] = &["pazaakwager"];

/// Whether the layout `gui`, named `name`, is placed as a popup: by its
/// shape (`popup`), or by its name (`FRAMED`).
fn popup_named(gui: &Gui, name: &str) -> bool {
    popup(gui) || FRAMED.contains(&name)
}

/// The popups KOTOR 2's executable centres in the window once it has
/// stretched them (`stretch`), by their layouts' names here: the message
/// box (its question and message layout, and the datapad's), the save
/// name box, the resolution list, the container, the skill information box
/// and the debug menus.
const CENTRED: &[&str] = &[
    "confirm",
    "datapad",
    "savename",
    "optresolution",
    "container",
    "skillinfo",
    "debug",
];

/// The frame a layout stretched `by` (`stretch`) is drawn in, in its own
/// pixels: KOTOR 2's 800 × 600 frame stretched.
fn stretched_frame(by: (f32, f32)) -> (i32, i32) {
    let mut frame = Rect {
        left: 0,
        top: 0,
        width: KOTOR2_FRAME.0,
        height: KOTOR2_FRAME.1,
    };
    stretched(&mut frame, by);
    (frame.width, frame.height)
}

/// A full-screen layout or a popup placed as KOTOR 2 places it in a window
/// of `placed.window` pixels: stretched to the window's shape. KOTOR 2's
/// executable makes every screen layout for an 800 × 600 frame. It
/// multiplies the left and width of each panel and of each control on it
/// by the window's width over 800, and their tops and heights by the
/// window's height over 600.
///
/// Here every extent (the root's, every control's, nested ones, list rows'
/// and scrollbars') is multiplied so that the frame becomes the window's
/// shape at kjk's scale (`draw::scale`). `fit` then fills the window with
/// the frame edge to edge, so drawing and hit tests, which both place the
/// layout through `fit`, see the same stretched layout. Text and borders
/// keep their sizes in the display's pixels, as in KOTOR 2
/// (`Sizing::physical`).
///
/// A popup (`popup_named`) is stretched in the same frame, its root's place
/// and size included. KOTOR 2 does this to every popup but the conversation
/// box, which it does not stretch at all. Most popups' layouts were made
/// for a 640 × 480 frame, so KOTOR 2 then moves some of them to the
/// middle of the window (`centred`, the popups in `CENTRED`). The others
/// stay where the stretch puts them. KOTOR 2 scales only the size of four
/// panels, not their place: the container and three of character
/// creation's. Their layouts start at the frame's top left corner, so that
/// comes to the same thing here.
///
/// kjk's Interface option "Menu width" (`placed.width`) may stretch the
/// frame less (`Placement::stretch_by`): no wider than 16:9, or not at all.
/// `fit` then centres the frame, and the stack draws black bars beside it.
/// A popup over such a menu is stretched and centred in that same frame.
///
/// With `own` (the game's own layout, not a mod's) and the Interface option
/// "Maintain image aspect ratio" on (`placed.pictures`), the pictures then
/// keep their shape (`kept_in_shape`), and so do the pieces of art that
/// pictures sit on (`pieces`). That is kjk's deliberate change. A mod's
/// layout is always stretched plainly, as KOTOR 2 does, because widescreen
/// mods are made for the plain stretch.
fn stretch(gui: &mut Gui, placed: Placement, own: bool, centred: bool) {
    /// `kept` is `None` for a list's rows and scrollbar, which are never
    /// pictures, and when no picture keeps its shape.
    fn walk(control: &mut Control, by: (f32, f32), kept: Option<&Kept>) {
        let read = control.extent;
        stretched(&mut control.extent, by);
        if let Some(anchor) = kept.and_then(|kept| kept.anchor(control, read)) {
            control.extent = kept_in_shape(read, anchor, by);
        }
        for inner in &mut control.children {
            walk(inner, by, kept);
        }
        for inner in control
            .proto
            .as_deref_mut()
            .into_iter()
            .chain(control.scrollbar.as_deref_mut())
        {
            walk(inner, by, None);
        }
    }
    let by = placed.stretch_by();
    let kept = (own && placed.pictures).then(|| Kept::of(gui));
    stretched(&mut gui.extent, by);
    for control in &mut gui.controls {
        walk(control, by, kept.as_ref());
    }
    if centred {
        let (width, height) = stretched_frame(by);
        gui.extent.left = (width - gui.extent.width) / 2;
        gui.extent.top = (height - gui.extent.height) / 2;
    }
}

/// `rect` multiplied across by `by.0` and down by `by.1`, as KOTOR 2
/// stretches a layout. Each edge is rounded to a whole pixel on its own, so
/// that controls which meet in the layout still meet when stretched.
pub(crate) fn stretched(rect: &mut Rect, by: (f32, f32)) {
    let edge = |at: i32, by: f32| (at as f32 * by).round() as i32;
    let (right, bottom) = (rect.left + rect.width, rect.top + rect.height);
    rect.left = edge(rect.left, by.0);
    rect.top = edge(rect.top, by.1);
    rect.width = edge(right, by.0) - rect.left;
    rect.height = edge(bottom, by.1) - rect.top;
}

/// `extent` grown about its centre by `by` pixels of the layout as read on
/// every side, at the layout's `stretch` (`Grow`).
pub(crate) fn grown_extent(extent: Rect, by: f32, stretch: (f32, f32)) -> Rect {
    let (dx, dy) = (
        (by * stretch.0).round() as i32,
        (by * stretch.1).round() as i32,
    );
    Rect {
        left: extent.left - dx,
        top: extent.top - dy,
        width: extent.width + 2 * dx,
        height: extent.height + 2 * dy,
    }
}

/// How much longer than it is wide a fill-only picture may be and still
/// keep its shape (`is_picture`). A longer one is a strip of art (a bar, a
/// bevel, a frame's backing) that runs between other controls and is
/// stretched with them, unless it is a panel that pictures sit on
/// (`pieces`).
const STRIP: i32 = 2;

/// Whether `control` is a list, a list's scrollbar, a slider or a progress
/// bar. Its rows and thumb are laid out from its stretched extent, so it is
/// never kept in shape.
fn listed(control: &Control) -> bool {
    matches!(
        control.kind,
        Kind::ListBox | Kind::ScrollBar | Kind::Slider | Kind::Progress
    )
}

/// The art of `control`, of extent `read` in its 800 × 600 layout. It is
/// `None` when the control has words, is `listed`, covers the whole frame
/// (a backdrop) or has no size. Otherwise it says whether the art is a fill
/// image alone, with no corner or edge art.
fn art(control: &Control, read: Rect) -> Option<bool> {
    let worded = control
        .text
        .as_ref()
        .is_some_and(|text| !text.text.is_empty() || text.strref.is_some());
    let backdrop = read.width >= KOTOR2_FRAME.0 && read.height >= KOTOR2_FRAME.1;
    if worded || listed(control) || backdrop || read.width <= 0 || read.height <= 0 {
        return None;
    }
    let border = control.border.as_ref();
    let filled = border.is_some_and(|border| !crate::paint::none(&border.fill));
    let framed = border.is_some_and(|border| {
        !crate::paint::none(&border.corner) || !crate::paint::none(&border.edge)
    });
    Some(filled && !framed)
}

/// Whether `control`, of extent `read` in its 800 × 600 layout, is drawn as
/// a picture (`kept_in_shape`). A picture has no words, is not `listed`,
/// and does not cover the whole frame (a backdrop). It is then one of
/// these:
/// - a square: an icon, a portrait, a logo, or a square button framed
///   round an icon;
/// - a 3D view (`LBL_3D...`);
/// - a fill image with no corner or edge art, at most `STRIP` times as long
///   as it is wide.
///
/// A bordered box, a list, a button with tiled edges and words, a label of
/// words and a strip of art stay stretched.
pub(crate) fn is_picture(control: &Control, read: Rect) -> bool {
    art(control, read).is_some_and(|fill_only| {
        let (long, short) = (read.width.max(read.height), read.width.min(read.height));
        read.width == read.height
            || control.tag.starts_with("LBL_3D")
            || (fill_only && long <= STRIP * short)
    })
}

/// How much longer than it is wide a strip of art may be and still carry
/// the pictures on it as one piece (`pieces`). The equip screen's slot
/// diagram is 2.9 times as long as it is wide. A longer strip is a bar,
/// such as the one under the equip screen's party member, which is 9.6
/// times as long. A bar stays stretched under the controls along it, as
/// they are made to spread along it.
const PANEL: i32 = 4;

/// Whether `control`, of extent `read`, is a panel of art that pictures may
/// sit on: a fill image alone, too long to be a picture (`STRIP`) but no
/// longer than `PANEL` allows.
fn is_panel(control: &Control, read: Rect) -> bool {
    let (long, short) = (read.width.max(read.height), read.width.min(read.height));
    art(control, read) == Some(true) && !is_picture(control, read) && long <= PANEL * short
}

/// Whether `inner` lies wholly within `outer`.
fn within(inner: Rect, outer: Rect) -> bool {
    outer.left <= inner.left
        && outer.top <= inner.top
        && outer.left + outer.width >= inner.left + inner.width
        && outer.top + outer.height >= inner.top + inner.height
}

/// How far apart, in layout pixels, two pictures may stand and still be
/// one piece with the art they sit on (`pieces`). KOTOR 2's equip slots
/// stand 5 to 7 pixels apart.
const NEAR: i32 = 8;

/// Whether `a` and `b` overlap or stand at most `NEAR` pixels apart, across
/// and down.
fn near(a: Rect, b: Rect) -> bool {
    let apart = |a_start: i32, a_size: i32, b_start: i32, b_size: i32| {
        a_start.max(b_start) - (a_start + a_size).min(b_start + b_size)
    };
    apart(a.left, a.width, b.left, b.width) <= NEAR
        && apart(a.top, a.height, b.top, b.height) <= NEAR
}

/// The extent that holds all of `rects`.
fn bounds(rects: &[Rect]) -> Rect {
    let left = rects.iter().map(|rect| rect.left).min().unwrap_or(0);
    let top = rects.iter().map(|rect| rect.top).min().unwrap_or(0);
    let right = rects
        .iter()
        .map(|rect| rect.left + rect.width)
        .max()
        .unwrap_or(0);
    let bottom = rects
        .iter()
        .map(|rect| rect.top + rect.height)
        .max()
        .unwrap_or(0);
    Rect {
        left,
        top,
        width: right - left,
        height: bottom - top,
    }
}

/// The pieces of art in `gui` (as read) that keep their shape as one, each
/// as the extent that holds it. A piece starts from a panel of art
/// (`is_panel`) that a picture of `pictures` lies within, such as the
/// equip screen's diagram under its weapon slots. The pictures near it
/// (`near`) join the piece, and so do the pictures near those in turn,
/// such as the slots above and below the diagram.
///
/// If the panel alone kept its shape, the slots in it would follow it and
/// leave the columns of slots above and below them. If it were stretched,
/// the slots kept in shape would no longer fill the outlines it draws for
/// them. As one piece, the art, its slots and the words written on it all
/// keep their places in it.
fn pieces(gui: &Gui, pictures: &[Rect]) -> Vec<Rect> {
    gui.walk()
        .filter(|control| is_panel(control, control.extent))
        .map(|panel| panel.extent)
        .filter(|&panel| pictures.iter().any(|&picture| within(picture, panel)))
        .map(|panel| {
            let mut piece = vec![panel];
            while let Some(&next) = pictures.iter().find(|&&picture| {
                !piece.contains(&picture) && piece.iter().any(|&member| near(picture, member))
            }) {
                piece.push(next);
            }
            bounds(&piece)
        })
        .collect()
}

/// What keeps its shape in a stretched layout (`stretch`): the extents of
/// its pictures (`is_picture`) and of its pieces of art (`pieces`), as
/// read.
struct Kept {
    pictures: Vec<Rect>,
    pieces: Vec<Rect>,
}

impl Kept {
    fn of(gui: &Gui) -> Kept {
        let pictures: Vec<Rect> = gui
            .walk()
            .filter(|control| is_picture(control, control.extent))
            .map(|control| control.extent)
            .collect();
        let pieces = pieces(gui, &pictures);
        Kept { pictures, pieces }
    }

    /// What `control`, of extent `read`, keeps its shape about, if it keeps
    /// it. Everything within a piece of art keeps its place in the piece,
    /// words included, but for a list. A picture elsewhere keeps its place
    /// in the largest picture it lies within (an icon on a portrait, the
    /// headgear eye on its slot), or else its own centre.
    fn anchor(&self, control: &Control, read: Rect) -> Option<Rect> {
        if let Some(&piece) = self.pieces.iter().find(|&&piece| within(read, piece))
            && !listed(control)
        {
            return Some(piece);
        }
        is_picture(control, read).then(|| {
            self.pictures
                .iter()
                .filter(|&&outer| within(read, outer))
                .max_by_key(|outer| outer.width * outer.height)
                .copied()
                .unwrap_or(read)
        })
    }
}

/// Where a picture of extent `read` (in its 800 × 600 layout) is drawn in a
/// layout stretched `by` (`stretch`). This is kjk's deliberate change from
/// KOTOR 2, modelled on the popular "Widescreen UI Fix" mod: KOTOR 2
/// stretches a square picture to the window's shape, so at 16:9 it is a
/// third wider than it is tall. The mod keeps KOTOR 2's stretch but writes
/// each picture narrower in its layouts so that it comes out in shape: by
/// 0.75 at 16:9 (4:3 over 16:9). The menu bar's tab buttons go from 44 × 44
/// to 33 × 44, the party selection's portraits from 80 × 80 to 60 × 80, and
/// the main menu's logo from 330 × 330 to 248 × 330.
///
/// Here a picture is scaled evenly by the smaller of the two stretches, so
/// it keeps its own width to height and fits where the stretch would put
/// it. It is scaled about `anchor`, the extent it keeps its place in
/// (`Kept::anchor`), as read. That extent's centre stays where the stretch
/// puts it. For a picture on its own, the anchor is the picture itself.
fn kept_in_shape(read: Rect, anchor: Rect, by: (f32, f32)) -> Rect {
    let centre = |rect: Rect| {
        [
            rect.left as f32 + rect.width as f32 / 2.0,
            rect.top as f32 + rect.height as f32 / 2.0,
        ]
    };
    let even = by.0.min(by.1);
    let [ax, ay] = centre(anchor);
    let [cx, cy] = centre(read);
    let (x, y) = (ax * by.0 + (cx - ax) * even, ay * by.1 + (cy - ay) * even);
    let (width, height) = (read.width as f32 * even, read.height as f32 * even);
    let left = (x - width / 2.0).round() as i32;
    let top = (y - height / 2.0).round() as i32;
    Rect {
        left,
        top,
        width: (x + width / 2.0).round() as i32 - left,
        height: (y + height / 2.0).round() as i32 - top,
    }
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
            text_size: 0.0,
            stretched: None,
            stretch: (1.0, 1.0),
            drawn: None,
            largest: BTreeMap::new(),
            focus: None,
            lists: BTreeMap::new(),
            clicked: None,
            dragged: None,
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

    /// Whether the top screen's typing goes into an edit box (a name, an
    /// address): a pad's player needs a keyboard on the screen there.
    pub fn typing(&mut self) -> bool {
        let Some(entry) = self.entries.last_mut() else {
            return false;
        };
        let Some(tag) = entry.typing_tag() else {
            return false;
        };
        entry.screen.edit(&tag).is_some()
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

    /// The first row the top screen's list `tag` shows, and its cursor row.
    pub fn list_at(&self, tag: &str) -> Option<(usize, usize)> {
        let cursor = self.entries.last()?.lists.get(tag)?;
        Some((cursor.first, cursor.row))
    }

    /// How far into its first shown row the top screen's list `tag` is
    /// scrolled, in layout pixels, when it is scrolled by lines.
    pub fn list_offset(&self, tag: &str) -> Option<i32> {
        Some(self.entries.last()?.lists.get(tag)?.offset)
    }

    /// Where `tag` sits in the top screen's layout, as shaped and drawn
    /// (grown, `Screen::grown`).
    pub fn extent_of(&self, tag: &str) -> Option<fmt_odyssey::gui::Rect> {
        Some(self.entries.last()?.shown()?.find(tag)?.extent)
    }

    /// The pad's hints for the top screen as its bar shows them.
    pub fn hints(&self, assets: &mut dyn Assets, vm: &ViewModel) -> Vec<Hint> {
        let Some(entry) = self.entries.last() else {
            return Vec::new();
        };
        let Some(gui) = entry.shown() else {
            return Vec::new();
        };
        let mut painter = Painter::new(assets, self.palette);
        painter.source = entry.source;
        entry.hints(gui, vm, &painter)
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
        let gui = entry.shown()?;
        let (width, height) = entry.frame(gui);
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
                    let (width, height) = entry.frame(gui);
                    gui.extent.width >= width && gui.extent.height >= height
                })
        })
    }

    /// Loads the layout of each screen that has none yet, a full-screen one
    /// or a popup stretched for `placed` (`stretch`), and with `prune`
    /// closes those whose layout does not load (a screen opened this frame
    /// waits for the next). A stretched layout is stretched again when the
    /// window's size or the Interface settings change. A layout is loaded
    /// again when the text size changes, with its screen's focus and its
    /// lists' cursors kept.
    fn load(&mut self, assets: &mut dyn Assets, placed: Placement, frame: &mut Frame, prune: bool) {
        let sizing = assets.sizing();
        let text_size = sizing.text / sizing.pixel.max(f32::EPSILON);
        for entry in &mut self.entries {
            let wanted = (
                entry.screen.layout().to_string(),
                entry.screen.under().map(str::to_string),
            );
            if entry.loaded != wanted {
                entry.gui = None;
                entry.stretched = None;
                entry.focus = None;
                entry.lists.clear();
                entry.loaded = wanted;
            }
            if entry.gui.is_some() && entry.text_size != text_size {
                entry.gui = None;
                entry.stretched = None;
            }
            if entry.gui.is_none() {
                entry.text_size = text_size;
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
                        // The conversation is drawn outside the stack, and
                        // KOTOR 2 does not stretch its box either.
                        let name = entry.screen.layout();
                        let popup = popup_named(&gui, name);
                        let stretches =
                            name != crate::screens::dialog::LAYOUT && (full_screen(&gui) || popup);
                        let centred = popup && CENTRED.contains(&name);
                        let pictures = !assets.layout_is_mod(name, source);
                        entry.screen.fit_read(&mut gui, assets, source);
                        entry.stretched = stretches.then(|| Stretched {
                            read: gui.clone(),
                            placed: None,
                            pictures,
                            centred,
                        });
                        // A layout that is not stretched is drawn at the
                        // size it was read. A stretched one is fitted below,
                        // each time it is stretched.
                        if !stretches {
                            entry.screen.fit_drawn(&mut gui, assets, source);
                        }
                        entry.gui = Some(gui);
                        entry.source = source;
                    }
                    Err(why) => frame.refusals.push(format!("layout {why}")),
                }
            }
            if let Some(stretched) = &mut entry.stretched
                && stretched.placed != Some(placed)
            {
                let mut gui = stretched.read.clone();
                stretch(&mut gui, placed, stretched.pictures, stretched.centred);
                // A layout that grows to its words (the message box) is
                // fitted at the size it is drawn, as KOTOR 2's message box
                // grows after its stretch. Its words wrap to the width they
                // are drawn in, and its rows are as tall as their words
                // rather than stretched. Growth fitted before the stretch
                // would be stretched as well, and growth never shrinks.
                entry.screen.fit_drawn(&mut gui, assets, entry.source);
                entry.gui = Some(gui);
                stretched.placed = Some(placed);
            }
            entry.stretch = match &entry.stretched {
                Some(_) => placed.stretch_by(),
                None => (1.0, 1.0),
            };
        }
        // A screen whose layout does not load closes at once: it would
        // draw nothing and take every key.
        if prune {
            self.entries.retain(|entry| entry.gui.is_some());
        }
    }

    /// A frame of the stack. Its words are KOTOR 2's size: a font pixel
    /// Aspyr's text size in the display's physical pixels however large the
    /// window draws the frames (`Sizing::physical`), as KOTOR 2 draws them.
    /// Fonts are read from the sheets KOTOR 2 picks for the window's
    /// physical width and the "Use Small Fonts" setting (`SmallFonts`), here
    /// and in what is drawn outside the stack after it. kjk's text scale
    /// (`TextScale`) multiplies the size of the words, here and outside the
    /// stack too.
    pub fn step(&mut self, assets: &mut dyn Assets, vm: &ViewModel, input: &UiInput) -> Frame {
        let window = input.window;
        let small = vm
            .settings
            .get(SmallFonts::SETTING)
            .map_or(SmallFonts::Auto, |value| SmallFonts::named(value));
        let scale = crate::draw::scale(window);
        let text_scale = TextScale::of(&vm.settings);
        let sizing = Sizing::physical(window, input.scale_factor, scale, small, text_scale);
        let before = assets.sizing();
        let frame = sized(assets, sizing, |assets| self.step_at(assets, vm, input));
        // The font letter and the text scale stay for what is drawn outside
        // the stack.
        assets.set_sizing(before.outside(sizing));
        frame
    }

    fn step_at(&mut self, assets: &mut dyn Assets, vm: &ViewModel, input: &UiInput) -> Frame {
        self.clock += input.dt;
        let mut frame = Frame::default();
        let placed = Placement::of(vm, input.window);
        self.load(assets, placed, &mut frame, true);
        let presses = std::mem::take(&mut self.presses);
        let doubles = std::mem::take(&mut self.doubles);
        let mut intents = Vec::new();
        self.wheel += input.wheel;
        let notches = self.wheel.trunc();
        self.wheel -= notches;
        if let Some(entry) = self.entries.last_mut() {
            entry.clock = self.clock;
            entry.regrow();
            entry.handle(
                assets,
                vm,
                input,
                notches,
                (&presses, &doubles),
                &mut intents,
            );
            // As KOTOR 2's panels draw after the frame's input, the screen
            // moves with the focus the input left.
            entry.screen.animate(input.dt, entry.focus.as_deref());
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
        self.load(assets, placed, &mut frame, false);
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
        let pulsing = self.pulsing();
        let blink = self.clock % 1.0 < 0.5;
        let count = self.entries.len();
        let stepped = assets.sizing();
        for (at, entry) in self.entries.iter_mut().enumerate() {
            // A press this frame may have loaded another layout, or changed
            // what the screen grows.
            entry.regrow();
            let mut out = DrawList::default();
            let size = entry.gui.as_ref().map(|gui| {
                let (width, height) = entry.frame(gui);
                (width as f32, height as f32)
            });
            // Each layout is drawn with a physical pixel of its own frame's
            // scale. Its text stays the size the step set, which reads the
            // window's scale.
            let pixel = size.map_or(stepped.pixel, |size| {
                let (scale, _) = fit(input.window, size);
                stepped.at(input.window, input.scale_factor, scale).pixel
            });
            assets.set_sizing(Sizing { pixel, ..stepped });
            let mut painter = Painter::new(assets, self.palette);
            painter.pulse = pulse;
            painter.pulsing = pulsing;
            painter.source = entry.source;
            entry.draw(vm, at + 1 == count, blink, &mut painter, &mut out);
            // A menu narrower or shorter than the window has black bars
            // beside it, and nothing of it shows on them. Nor does anything
            // of a popup stretched in the same frame.
            let barred = size.filter(|&size| {
                let (_, [x, y]) = fit(input.window, size);
                entry.stretched.is_some() && placed.barred() && (x >= 1.0 || y >= 1.0)
            });
            if let Some(size) = barred {
                kept_in_frame(&mut out, size);
            }
            if let (Some(gui), Some((w, h))) = (&entry.gui, size) {
                // Past the frame, to the window's edges whatever its shape
                // and size: a frame is drawn at its own size, so a window
                // can be many frames wide. A popup's veil over a menu with
                // black bars stays on the menu.
                let veil = |reach: f32| [-reach, -reach, w + 2.0 * reach, h + 2.0 * reach];
                let popup = (gui.extent.width as f32) < w || (gui.extent.height as f32) < h;
                if !popup {
                    // KOTOR 2's frame is opaque: black bars beside it in a
                    // window wider (or taller) than the frame.
                    out.quads
                        .insert(0, Quad::solid(veil(VEIL_REACH), [0.0, 0.0, 0.0, 1.0]));
                } else if at > 0 && at + 1 == count {
                    let reach = if barred.is_some() { 0.0 } else { VEIL_REACH };
                    out.quads
                        .insert(0, Quad::solid(veil(reach), [0.0, 0.0, 0.0, VEIL]));
                } else if at == 0 && self.veil_world {
                    out.quads.insert(
                        0,
                        Quad::solid(veil(VEIL_REACH), [0.0, 0.0, 0.0, WORLD_VEIL]),
                    );
                }
            }
            // The pad's hints over the top screen, while a pad plays.
            if at + 1 == count
                && vm.pad_playing
                && let (Some(gui), Some(size)) = (entry.shown(), size)
            {
                let hints = entry.hints(gui, vm, &painter);
                // In the colour the layout writes its buttons' words in
                // (not an icon's).
                let tint = gui
                    .walk()
                    .filter(|control| control.kind == Kind::Button)
                    .filter_map(|control| control.text.as_ref())
                    .find(|text| text.strref.is_some() || !text.text.is_empty())
                    .and_then(|text| text.color);
                let reach = if barred.is_some() { 0.0 } else { VEIL_REACH };
                painter.hint_bar((&vm.pad_kind, tint), &hints, size, reach, &mut out);
            }
            frame.refusals.append(&mut painter.refusals);
            frame.layers.push(out);
        }
        assets.set_sizing(stepped);
        frame
    }
}

/// `out` kept inside a frame of `size` layout pixels: its wide 3D views as
/// wide as the frame, and each quad that reaches past the frame clipped to
/// it. A menu with black bars beside it (`Placement::barred`) draws nothing
/// on them.
fn kept_in_frame(out: &mut DrawList, size: (f32, f32)) {
    out.wide.clear();
    for quad in &mut out.quads {
        let [x, y, w, h] = quad.clip.unwrap_or(quad.rect);
        if x >= 0.0 && y >= 0.0 && x + w <= size.0 && y + h <= size.1 {
            continue;
        }
        let (left, top) = (x.max(0.0), y.max(0.0));
        let (right, bottom) = ((x + w).min(size.0), (y + h).min(size.1));
        quad.clip = Some([left, top, (right - left).max(0.0), (bottom - top).max(0.0)]);
    }
}

/// The left mouse button's raw name (`UiInput::raw`).
const LEFT_CLICK: &str = "MouseLeft";

/// A screen's OK or Done, as KOTOR's layouts name them, for the pad's
/// Menu where the screen names none (`Screen::accept`; not `BTN_ACCEPT`,
/// which is the party's Add).
const ACCEPTS: &[&str] = &["BTN_OK", "BTN_DONE"];
/// Its second button, for the pad's Y (`Screen::second`).
const SECONDS: &[&str] = &["BTN_DEFAULT"];

/// A list fitted to its text (a message) is read, not chosen.
fn read(control: &Control, state: &ControlState) -> bool {
    state.list.as_ref().map_or(
        control.kind == Kind::ListBox && state.strref.is_some(),
        |list| list.fit && !list.pick,
    )
}

impl Entry {
    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        self.screen.state(control, vm)
    }

    /// Its layout as drawn and hit this frame: grown where its screen grows
    /// controls (`drawn`), else as read.
    fn shown(&self) -> Option<&Gui> {
        self.drawn.as_ref().or(self.gui.as_ref())
    }

    /// The frame its layout `gui` is drawn and hit in, in the layout's
    /// pixels. A stretched layout's is KOTOR 2's 800 × 600 frame stretched
    /// (`stretched_frame`), which a stretched popup's root lies inside.
    /// Any other layout's is its own (`Gui::frame`).
    fn frame(&self, gui: &Gui) -> (i32, i32) {
        match self.stretched {
            Some(_) => stretched_frame(self.stretch),
            None => gui.frame(),
        }
    }

    /// Grows the controls its screen grows now (`Screen::grown`) in a copy
    /// of its layout (`drawn`), at the layout's stretch, and notes how large
    /// each one gets (`largest`). A grown control's children stay where the
    /// layout has them.
    fn regrow(&mut self) {
        let grows = self.screen.grown();
        self.largest.clear();
        self.drawn = None;
        let Some(gui) = self.gui.as_ref().filter(|_| !grows.is_empty()) else {
            return;
        };
        let mut drawn = gui.clone();
        let mut controls: Vec<&mut Control> = drawn.controls.iter_mut().collect();
        while let Some(control) = controls.pop() {
            if let Some((tag, grow)) = grows.iter().find(|(tag, _)| control.tag == *tag) {
                let read = control.extent;
                self.largest
                    .insert(*tag, grown_extent(read, grow.most, self.stretch));
                control.extent = grown_extent(read, grow.by, self.stretch);
            }
            controls.extend(control.children.iter_mut());
        }
        self.drawn = Some(drawn);
    }

    /// Takes its layout as drawn this frame (`shown`) out of it, with the
    /// layout as read when the two differ, to be put back (`put_back`).
    fn take_shown(&mut self) -> Option<(Gui, Option<Gui>)> {
        let gui = self.gui.take()?;
        Some(match self.drawn.take() {
            Some(drawn) => (drawn, Some(gui)),
            None => (gui, None),
        })
    }

    fn put_back(&mut self, (shown, read): (Gui, Option<Gui>)) {
        match read {
            Some(read) => {
                self.drawn = Some(shown);
                self.gui = Some(read);
            }
            None => self.gui = Some(shown),
        }
    }

    /// The controls the focus can rest on, in layout order.
    fn focusables<'g>(&self, gui: &'g Gui, vm: &ViewModel) -> Vec<&'g Control> {
        gui.walk()
            .filter(|control| focusable(control))
            .filter(|control| {
                let state = self.state(control, vm);
                !state.hidden && !state.disabled && !read(control, &state)
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
        let Some((gui, read)) = self.take_shown() else {
            return;
        };
        if self.screen.lists_anew() {
            self.lists.clear();
        }
        self.follow_selection(&gui, vm);
        if let Some((tag, row)) = self.screen.cursor_to(vm)
            && let Some((count, shown)) = self.rows(&gui, vm, tag)
            && row < count
        {
            self.focus = Some(tag.to_string());
            let cursor = self.lists.entry(tag.to_string()).or_default();
            cursor.row = row;
            Entry::keep_in_view(cursor, shown);
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
        self.put_back((gui, read));
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
        self.forget_scrolled(gui, vm);
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
        let (width, height) = self.frame(gui);
        let size = (width as f32, height as f32);
        let pointed = input.pointer.and_then(|point| {
            let at = window_to_layout(fit(input.window, size), size, point)?;
            Some([at[0] - origin[0], at[1] - origin[1]])
        });
        let under = pointed.and_then(|at| {
            let control = targets.iter().rev().find(|control| contains(control, at))?;
            Some((control.tag.clone(), at))
        });
        // A left click on a control is the control's, not a press to bind.
        let raw = input
            .raw
            .iter()
            .filter(|key| !(key.as_str() == LEFT_CLICK && under.is_some()));
        if raw.into_iter().any(|key| self.screen.raw(key, vm, out)) {
            return;
        }
        if input.moved
            && self.dragged.is_none()
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
                Key::Activate => match self
                    .focus
                    .clone()
                    .filter(|tag| self.screen.activates_primary(tag))
                {
                    Some(tag) => {
                        let row = self.lists.get(&tag).map_or(0, |cursor| cursor.row);
                        self.double_row(gui, vm, &tag, row, out);
                        self.clicked = None;
                    }
                    None => {
                        self.sound_click(gui, out);
                        self.activate(gui, vm, out);
                    }
                },
                Key::Back => self.screen.back(out),
                Key::Accept => match self.button(gui, vm, self.screen.accept(), ACCEPTS) {
                    Some(tag) => self.press_button(gui, vm, &tag, out),
                    None => self.screen.back(out),
                },
                Key::Second => {
                    if let Some(tag) = self.button(gui, vm, self.screen.second(), SECONDS) {
                        self.press_button(gui, vm, &tag, out);
                    }
                }
                // Two clicks on what has the focus: a list's row (buy, use,
                // equip) or a control (a party slot: add or remove it).
                Key::Primary => {
                    if let Some(tag) = self.button(gui, vm, self.screen.pad_x(), &[]) {
                        self.press_button(gui, vm, &tag, out);
                    } else if let Some(tag) = self.focus.clone() {
                        if self.rows(gui, vm, &tag).is_some() {
                            let row = self.lists.get(&tag).map_or(0, |cursor| cursor.row);
                            self.double_row(gui, vm, &tag, row, out);
                        } else {
                            self.activate(gui, vm, out);
                            self.screen.double(&tag, vm, out);
                        }
                        // Both its clicks spent: a click after it is a first.
                        self.clicked = None;
                    }
                }
                Key::ScrollUp | Key::ScrollDown => {
                    let by = if *key == Key::ScrollUp { -1 } else { 1 };
                    let read = match self.screen.scrolls() {
                        Some(tag) => Some(tag.to_owned()),
                        None => self.read_list(gui, textures, vm),
                    };
                    self.scroll(gui, textures, vm, read.as_deref(), by);
                }
                Key::Backspace => {
                    if let Some(tag) = self.typing_tag()
                        && let Some(edit) = self.screen.edit(&tag)
                    {
                        edit.backspace();
                    }
                }
                // The bumpers step the page's own tabs where no menu bar
                // took them (`Tab`), the triggers always.
                Key::NextTab | Key::NextPage => self.step_tab(gui, vm, 1, out),
                Key::PrevTab | Key::PrevPage => self.step_tab(gui, vm, -1, out),
            }
        }
        if !input.text.is_empty()
            && let Some(tag) = self.typing_tag()
            && let Some(edit) = self.screen.edit(&tag)
        {
            edit.type_text(&input.text);
        }
        // The wheel scrolls the list under the pointer, read (a message)
        // or chosen.
        let wheeled = pointed.filter(|_| notches != 0.0).and_then(|at| {
            gui.walk()
                .filter(|control| control.kind == Kind::ListBox && contains(control, at))
                .filter(|control| !self.state(control, vm).hidden)
                .last()
        });
        if let Some(list) = wheeled {
            self.scroll(gui, textures, vm, Some(list.tag.as_str()), -notches as i32);
        }
        // A press on a scrollbar's track takes its thumb (where it was hit,
        // else by its middle), which follows the pointer while held.
        if !input.held {
            self.dragged = None;
        }
        if input.click
            && let Some(at) = pointed
        {
            self.dragged = self.grab_thumb(gui, textures, vm, at);
        }
        if let (Some((tag, grab)), Some(at)) = (self.dragged.clone(), pointed) {
            self.drag_thumb(gui, textures, vm, &tag, at[1] - grab);
        }
        let click = input.click && self.dragged.is_none();
        if click
            && under.is_none()
            && let Some(at) = pointed
        {
            self.screen.click(gui, at, textures, vm);
        }
        if click
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

    /// A list without the focus has its cursor on the row its screen has
    /// selected, so the focus comes back to that row, as KOTOR 2's list
    /// keeps its selection while another control has the focus. A focused
    /// list leads instead: its screen's selection follows the cursor
    /// (`Screen::pointed`).
    fn follow_selection(&mut self, gui: &Gui, vm: &ViewModel) {
        for control in gui.walk().filter(|control| control.kind == Kind::ListBox) {
            if self.focus.as_deref() == Some(control.tag.as_str()) {
                continue;
            }
            let Some(view) = self.state(control, vm).list else {
                continue;
            };
            let Some(row) = view.selected.filter(|row| *row < view.rows.len()) else {
                continue;
            };
            let shown = rows_that_fit(control, Some(&view)).max(1);
            let cursor = self.lists.entry(control.tag.clone()).or_default();
            if cursor.row != row {
                cursor.row = row;
                Entry::keep_in_view(cursor, shown);
            }
        }
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
        // A cursor the list has shrunk under (its rows changed since it
        // moved) steps from the list's last row.
        let mut at = self.lists.get(&tag).copied().unwrap_or(Cursor::default());
        at.row = at.row.min(count.saturating_sub(1));
        at.first = at.first.min(at.row);
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

    /// A list's rows as the painter draws them: the screen's rows, else
    /// its words (a strref) as one fitted row.
    fn view_of(
        &self,
        list: &Control,
        vm: &ViewModel,
        textures: &dyn Textures,
    ) -> Option<crate::paint::ListView> {
        let state = self.state(list, vm);
        match state.list {
            Some(view) => Some(view),
            None => crate::paint::words_view(textures, &state),
        }
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
        let view = self.view_of(list, vm, textures).filter(|view| view.fit)?;
        let heights = view
            .rows
            .iter()
            .map(|row| row_height(textures, self.source, proto, true, &row.text) + list.padding)
            .collect();
        let room = list.extent.top + list.extent.height - proto.extent.top;
        Some((heights, room))
    }

    /// The list whose scrollbar's track is at `at` (layout pixels), with
    /// where its thumb is held: where the press is on it, else its middle.
    fn grab_thumb(
        &self,
        gui: &Gui,
        textures: &mut dyn Textures,
        vm: &ViewModel,
        at: [f32; 2],
    ) -> Option<(String, f32)> {
        gui.walk()
            .filter(|control| control.kind == Kind::ListBox && !self.state(control, vm).hidden)
            .find_map(|list| {
                let bar = list.scrollbar.as_deref()?;
                let Reach {
                    count,
                    shown,
                    last,
                    at: first,
                    ..
                } = self.reach(gui, textures, vm, &list.tag)?;
                if last == 0 {
                    return None;
                }
                let (top, track, size) = crate::paint::thumb_track(bar, count, shown);
                let [left, _, width, _] = crate::paint::rect_of(bar);
                let across = (left..left + width).contains(&at[0]);
                if !across || !(top..top + track).contains(&at[1]) {
                    return None;
                }
                let thumb = top + (track - size) * first as f32 / last as f32;
                let on = (thumb..thumb + size).contains(&at[1]);
                Some((
                    list.tag.clone(),
                    if on { at[1] - thumb } else { size / 2.0 },
                ))
            })
    }

    /// The list `tag` scrolled so its thumb's top is at `y` (layout pixels).
    fn drag_thumb(
        &mut self,
        gui: &Gui,
        textures: &mut dyn Textures,
        vm: &ViewModel,
        tag: &str,
        y: f32,
    ) {
        let Some(bar) = gui.find(tag).and_then(|list| list.scrollbar.clone()) else {
            return;
        };
        let Some(reach) = self
            .reach(gui, textures, vm, tag)
            .filter(|reach| reach.last > 0)
        else {
            return;
        };
        let (top, track, size) = crate::paint::thumb_track(&bar, reach.count, reach.shown);
        let along = ((y - top) / (track - size).max(1.0)).clamp(0.0, 1.0);
        let to = (along * reach.last as f32).round() as usize;
        if to != reach.at {
            self.place(tag, &reach, to);
        }
    }

    /// How far list `tag` scrolls, and where it is (`Reach`).
    fn reach(
        &self,
        gui: &Gui,
        textures: &mut dyn Textures,
        vm: &ViewModel,
        tag: &str,
    ) -> Option<Reach> {
        let cursor = self.lists.get(tag).copied().unwrap_or(Cursor::default());
        let list = gui.find(tag)?;
        let view = self.view_of(list, vm, textures);
        let by_lines = view
            .as_ref()
            .is_some_and(|view| view.fit && !view.pick && view.roll.is_none());
        if by_lines && let Some((heights, room)) = self.fitted(gui, textures, vm, tag) {
            let total = (heights.iter().sum::<i32>() - list.padding).max(0);
            let above: i32 = heights.iter().take(cursor.first).sum();
            let line = list
                .proto
                .as_deref()
                .and_then(|proto| proto.text.as_ref())
                .and_then(|words| textures.font(self.source, &words.font))
                .map_or(1, |(_, font)| font.line_height().ceil() as i32);
            let last = (total - room).max(0) as usize;
            return Some(Reach {
                count: total as usize,
                shown: room.max(0) as usize,
                last,
                at: ((above + cursor.offset).max(0) as usize).min(last),
                step: line.max(1) as usize,
                lines: Some(heights),
            });
        }
        let (count, shown, last) = self.span(gui, textures, vm, tag, cursor.first)?;
        Some(Reach {
            count,
            shown,
            last,
            at: cursor.first.min(last),
            step: 1,
            lines: None,
        })
    }

    /// List `tag` scrolled to `to` (`Reach::at`'s units).
    fn place(&mut self, tag: &str, reach: &Reach, to: usize) {
        let to = to.min(reach.last);
        let cursor = self.lists.entry(tag.to_string()).or_default();
        if let Some(heights) = &reach.lines {
            // The row the line is in, and how far into it.
            let mut left = to as i32;
            let mut first = 0;
            while first + 1 < heights.len() && left >= heights[first] {
                left -= heights[first];
                first += 1;
            }
            cursor.first = first;
            cursor.offset = left;
            return;
        }
        // KOTOR 2's wheel, scroll arrows and thumb move the rows only: the
        // selected row stays chosen, out of sight if need be.
        cursor.first = to;
    }

    /// A list scrolled by lines starts again from its top when it shows
    /// other words (the next item's description), unless it follows its
    /// end (a log, which keeps where the player scrolled it).
    fn forget_scrolled(&mut self, gui: &Gui, vm: &ViewModel) {
        use std::hash::{Hash, Hasher};
        for control in gui.walk().filter(|control| control.kind == Kind::ListBox) {
            let Some(view) = self.state(control, vm).list else {
                continue;
            };
            if !view.fit || view.pick || view.follow || view.roll.is_some() {
                continue;
            }
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            for row in &view.rows {
                row.text.hash(&mut hasher);
            }
            let words = hasher.finish();
            let cursor = self.lists.entry(control.tag.clone()).or_default();
            if cursor.words != words {
                cursor.words = words;
                cursor.first = 0;
                cursor.offset = 0;
            }
        }
    }

    /// A list's row count, how many rows show from row `first`, and the
    /// furthest it scrolls (the first row of its last rows shown whole). A
    /// fitted list's rows are as tall as their words, as the painter draws
    /// them; any other's are its prototype's.
    fn span(
        &self,
        gui: &Gui,
        textures: &mut dyn Textures,
        vm: &ViewModel,
        tag: &str,
        first: usize,
    ) -> Option<(usize, usize, usize)> {
        let (count, shown) = self.rows(gui, vm, tag)?;
        let Some((heights, room)) = self.fitted(gui, textures, vm, tag) else {
            return Some((count, shown, count.saturating_sub(shown)));
        };
        let padding = gui.find(tag).map_or(0, |list| list.padding);
        // How many of these rows, in turn, are whole in the list (at
        // least one).
        let whole = |rows: &mut dyn Iterator<Item = &i32>| {
            let mut used = 0;
            rows.take_while(|height| {
                used += **height;
                used - padding <= room
            })
            .count()
            .max(1)
        };
        let last = count.saturating_sub(whole(&mut heights.iter().rev()));
        let from = first.min(last);
        Some((count, whole(&mut heights[from..].iter()), last))
    }

    /// The list the scroll keys move when the focus is on none that
    /// overflows: the first shown text read rather than chosen (an item's
    /// description, a message, `read`) that does, which the focus never
    /// rests on. None scrolls the focused list.
    fn read_list(&self, gui: &Gui, textures: &mut dyn Textures, vm: &ViewModel) -> Option<String> {
        let mut overflows = |tag: &str| {
            self.reach(gui, textures, vm, tag)
                .is_some_and(|reach| reach.last > 0)
        };
        if self.focus.as_deref().is_some_and(&mut overflows) {
            return None;
        }
        gui.walk()
            .filter(|control| control.kind == Kind::ListBox)
            .filter(|control| {
                let state = self.state(control, vm);
                !state.hidden && read(control, &state)
            })
            .find(|control| overflows(&control.tag))
            .map(|control| control.tag.clone())
    }

    /// Whether `tag` is a control of its layout shown and usable.
    fn usable(&self, gui: &Gui, vm: &ViewModel, tag: &str) -> bool {
        gui.find(tag).is_some_and(|control| {
            let state = self.state(control, vm);
            !state.hidden && !state.disabled
        })
    }

    /// The button the screen names (`chosen`), else the first of
    /// `usual` its layout has, when it is usable.
    fn button(
        &self,
        gui: &Gui,
        vm: &ViewModel,
        chosen: Option<&str>,
        usual: &[&str],
    ) -> Option<String> {
        let tag = match chosen {
            Some(tag) => Some(tag),
            None => usual.iter().copied().find(|tag| gui.find(tag).is_some()),
        };
        // As the layout spells it (`Gui::find` takes either case).
        tag.filter(|tag| self.usable(gui, vm, tag))
            .and_then(|tag| gui.find(tag))
            .map(|control| control.tag.clone())
    }

    /// `tag` pressed as a click on it presses it.
    fn press_button(&mut self, gui: &Gui, vm: &ViewModel, tag: &str, out: &mut Vec<Intent>) {
        self.focus = Some(tag.to_string());
        self.sound_click(gui, out);
        self.activate(gui, vm, out);
    }

    /// The page's tab `by` on from the chosen one (shown selected; the
    /// first when none is), around.
    fn step_tab(&mut self, gui: &Gui, vm: &ViewModel, by: i32, out: &mut Vec<Intent>) {
        let tabs: Vec<&str> = self
            .screen
            .tabs()
            .into_iter()
            .filter(|tag| self.usable(gui, vm, tag))
            .collect();
        if tabs.is_empty() {
            return;
        }
        let chosen = tabs
            .iter()
            .position(|tag| {
                gui.find(tag)
                    .is_some_and(|control| self.state(control, vm).selected)
            })
            .unwrap_or(0);
        let next = tabs[(chosen as i32 + by).rem_euclid(tabs.len() as i32) as usize];
        self.press_button(gui, vm, next, out);
    }

    /// The pad's hints for what has the focus and the screen: select, its
    /// row's or slot's double click, its second, back or cancel, its
    /// accept, and its tabs; each named by its button's own words.
    fn hints(&self, gui: &Gui, vm: &ViewModel, painter: &Painter) -> Vec<Hint> {
        let words = |tag: &str| {
            let control = gui.find(tag)?;
            let words = painter.words(control, &self.state(control, vm));
            (!words.is_empty()).then_some(words)
        };
        let hint = |buttons: &[&'static str], words: Vec<u8>| Hint {
            buttons: buttons.to_vec(),
            words,
        };
        let mut hints = Vec::new();
        if let Some(words) = self
            .button(gui, vm, self.screen.pad_x(), &[])
            .and_then(|tag| words(&tag))
        {
            hints.push(hint(&["South"], strings::bytes(strings::SELECT)));
            hints.push(hint(&["West"], words));
        } else if let Some(focus) = &self.focus {
            let double = self
                .screen
                .primary(focus)
                .or_else(|| self.screen.double_button(focus))
                .filter(|tag| self.usable(gui, vm, tag))
                .and_then(words);
            // A row chosen by the cursor: A does its primary, X no more.
            match double {
                Some(words) if self.screen.activates_primary(focus) => {
                    hints.push(hint(&["South"], words));
                }
                double => {
                    hints.push(hint(&["South"], strings::bytes(strings::SELECT)));
                    hints.extend(double.map(|words| hint(&["West"], words)));
                }
            }
        }
        if let Some(words) = self
            .button(gui, vm, self.screen.second(), SECONDS)
            .and_then(|tag| words(&tag))
        {
            hints.push(hint(&["North"], words));
        }
        let back = if self.screen.cancels() {
            strings::CANCEL
        } else {
            strings::BACK
        };
        hints.push(hint(&["East"], strings::bytes(back)));
        if let Some(words) = self
            .button(gui, vm, self.screen.accept(), ACCEPTS)
            .and_then(|tag| words(&tag))
        {
            hints.push(hint(&["Start"], words));
        }
        let bar = self.screen.tab_bar(vm);
        if bar {
            hints.push(hint(
                &["LeftTrigger", "RightTrigger"],
                strings::bytes(strings::SCREENS),
            ));
        }
        if !self.screen.tabs().is_empty() {
            let pair: &[&'static str] = if bar {
                &["LeftTrigger2", "RightTrigger2"]
            } else {
                &["LeftTrigger", "RightTrigger"]
            };
            hints.push(hint(pair, strings::bytes(strings::TABS)));
        }
        hints
    }

    fn scroll(
        &mut self,
        gui: &Gui,
        textures: &mut dyn Textures,
        vm: &ViewModel,
        tag: Option<&str>,
        by: i32,
    ) {
        let Some(tag) = tag.map(str::to_string).or_else(|| self.focus.clone()) else {
            return;
        };
        let Some(reach) = self.reach(gui, textures, vm, &tag) else {
            return;
        };
        let follows = self
            .view(gui, vm, &tag)
            .and_then(|(_, view)| view)
            .is_some_and(|view| view.follow);
        let cursor = self.lists.entry(tag.clone()).or_default();
        // Scrolling a list that followed its end starts from there.
        let from = if follows && !cursor.pinned {
            cursor.pinned = true;
            reach.last
        } else {
            reach.at
        };
        let to = (from as i64 + i64::from(by) * reach.step as i64).clamp(0, reach.last as i64);
        self.place(&tag, &reach, to as usize);
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
        let Some((gui, read)) = self.take_shown() else {
            return;
        };
        self.paint(&gui, vm, top, blink, painter, out);
        self.put_back((gui, read));
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
            if state.scene.is_some()
                && let Some(largest) = self.largest.get(control.tag.as_str())
            {
                state.scene_size = Some([largest.width as f32, largest.height as f32]);
            }
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
                } else if list.fit && !list.pick && !list.follow {
                    list.offset = cursor.offset;
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
        let (width, height) = self.frame(gui);
        out.size = (width as f32, height as f32);
        out.quads.extend(self.screen.quads(gui, vm, painter));
    }
}
