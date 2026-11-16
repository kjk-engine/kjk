//! A message and OK (`confirm`): why a session could not start, a
//! script's message box (`DisplayMessageBox`, its strref), and the like,
//! the box grown to it as KOTOR's grows (`fit`) and what still does not fit
//! scrolled. OK or Back closes it. Every other box on `confirm` (a
//! question, a tutorial's pages) asks through a `Message` for its words, so
//! each grows and scrolls alike.

use fmt_odyssey::gui::{Control, Gui, Rect, align};

use crate::draw::{Sizing, Textures};
use crate::intent::Intent;
use crate::paint::{ControlState, ListView, Row, fill_tokens, rows_that_fit, wrap};
use crate::screen::{Screen, ScreenId};
use crate::screens::pages::description;
use crate::skin::Source;
use crate::strings;
use crate::vm::ViewModel;

/// What a talk-table entry's tokens (`<CUSTOM0>`, by name) are filled with.
pub type Tokens = Vec<(&'static str, Vec<u8>)>;

enum Words {
    Text(String),
    Strref(u32, Tokens),
    /// In the game's code page.
    Said(Vec<u8>),
}

pub struct Message {
    words: Words,
    /// The words wrapped to the grown box, a row each, once its layout is
    /// in (`fit`).
    lines: Option<Vec<Vec<u8>>>,
    /// The box is KOTOR 2's datapad box (`datapad_box`), not its message
    /// box.
    datapad: bool,
}

impl Message {
    fn of(words: Words) -> Message {
        Message {
            words,
            lines: None,
            datapad: false,
        }
    }

    pub fn new(text: &str) -> Message {
        Message::of(Words::Text(text.to_string()))
    }

    /// `words` in the game's code page.
    pub fn said(words: Vec<u8>) -> Message {
        Message::of(Words::Said(words))
    }

    /// The game's text `strref`.
    pub fn told(strref: &str) -> Option<Message> {
        Some(Message::strref(strref.parse().ok()?, Vec::new()))
    }

    /// The game's text `strref`, its `tokens` filled in.
    pub fn strref(strref: u32, tokens: Tokens) -> Message {
        Message::of(Words::Strref(strref, tokens))
    }
}

/// A message box of pages a script showed (`ViewModel::pages`): KOTOR's
/// tutorial window (its `tutorial.2da` row's `Message_PC0` to `2`, a page
/// each) and TSL's datapad (its item's words). OK turns the page, and on
/// the last closes it; there is no cancel. Each page is a box of its own
/// (`pages:<at>`), grown to its words as KOTOR's tutorial box grows for
/// each message. The tutorial's icon is not drawn [verify where KOTOR puts
/// it]. A datapad's pages show in KOTOR 2's datapad box (`datapad`, sized
/// by `datapad_box`).
pub struct Pages {
    at: usize,
    page: Message,
}

impl Pages {
    /// Page `at` of the pages `vm` holds.
    pub fn new(at: &str, vm: &ViewModel) -> Option<Pages> {
        let at = at.parse().ok()?;
        let mut page = Message::said(vm.pages.get(at).cloned().unwrap_or_else(Vec::new));
        page.datapad = vm.pages_datapad;
        Some(Pages { at, page })
    }
}

impl Screen for Pages {
    fn id(&self) -> ScreenId {
        ScreenId("pages")
    }

    fn layout(&self) -> &str {
        if self.page.datapad {
            DATAPAD
        } else {
            "confirm"
        }
    }

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        self.page.state(control, vm)
    }

    fn press(&mut self, tag: &str, vm: &ViewModel, out: &mut Vec<Intent>) {
        if tag == "BTN_OK" {
            out.push(Intent::Close);
            let next = self.at + 1;
            if next < vm.pages.len() {
                out.push(Intent::OpenFor("pages", next.to_string()));
            }
        }
    }

    fn first_focus(&self) -> Option<&str> {
        Some("BTN_OK")
    }

    fn fit_drawn(&mut self, gui: &mut Gui, textures: &mut dyn Textures, source: Source) {
        self.page.fit_drawn(gui, textures, source);
    }
}

/// A save that cannot be loaded exactly, whose story can be restored
/// (`story_restore:<id>`, docs/superpowers/specs/2026-10-28-save-migrations-design.md
/// 4b): the question (the session's error, `SessionView::error`) as a
/// message's, Restore plays the story (`SessionIntent::Restore`), Cancel or
/// Back closes it. A pad's Menu presses Restore (`BTN_OK`).
pub struct Restore {
    save: String,
    asked: Message,
}

impl Restore {
    pub fn new(save: &str, vm: &ViewModel) -> Restore {
        Restore {
            save: save.to_string(),
            asked: Message::new(vm.session.error.as_deref().unwrap_or("")),
        }
    }
}

impl Screen for Restore {
    fn id(&self) -> ScreenId {
        ScreenId("story_restore")
    }

    fn layout(&self) -> &str {
        "confirm"
    }

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        let words = |words: &str| ControlState {
            text: Some(strings::bytes(words)),
            ..ControlState::default()
        };
        match control.tag.as_str() {
            "BTN_OK" => words(strings::RESTORE),
            "BTN_CANCEL" => words(strings::CANCEL),
            _ => self.asked.state(control, vm),
        }
    }

    fn press(&mut self, tag: &str, _: &ViewModel, out: &mut Vec<Intent>) {
        match tag {
            "BTN_OK" => out.extend([
                Intent::CloseAll,
                Intent::Session(crate::intent::SessionIntent::Restore {
                    save: self.save.clone(),
                }),
            ]),
            "BTN_CANCEL" => out.push(Intent::Close),
            _ => {}
        }
    }

    fn first_focus(&self) -> Option<&str> {
        Some("BTN_OK")
    }

    fn fit_drawn(&mut self, gui: &mut Gui, textures: &mut dyn Textures, source: Source) {
        self.asked.fit_drawn(gui, textures, source);
    }
}

impl Screen for Message {
    fn id(&self) -> ScreenId {
        ScreenId("message")
    }

    fn layout(&self) -> &str {
        "confirm"
    }

    fn state(&self, control: &Control, _: &ViewModel) -> ControlState {
        match control.tag.as_str() {
            "LB_MESSAGE" => match (&self.lines, &self.words) {
                (Some(lines), _) => ControlState {
                    list: Some(ListView {
                        rows: lines
                            .iter()
                            .map(|line| Row {
                                text: line.clone(),
                                ..Row::default()
                            })
                            .collect(),
                        fit: true,
                        ..ListView::default()
                    }),
                    ..ControlState::default()
                },
                (None, Words::Text(text)) => description(&strings::bytes(text)),
                (None, Words::Strref(strref, tokens)) => ControlState {
                    strref: Some(*strref),
                    tokens: tokens.clone(),
                    ..ControlState::default()
                },
                (None, Words::Said(words)) => description(words),
            },
            "BTN_OK" => ControlState {
                text: Some(strings::bytes(strings::OK)),
                ..ControlState::default()
            },
            "BTN_CANCEL" => ControlState {
                hidden: true,
                ..ControlState::default()
            },
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, _: &ViewModel, out: &mut Vec<Intent>) {
        if tag == "BTN_OK" {
            out.push(Intent::Close);
        }
    }

    fn first_focus(&self) -> Option<&str> {
        Some("BTN_OK")
    }

    fn fit_drawn(&mut self, gui: &mut Gui, textures: &mut dyn Textures, source: Source) {
        let words = match &self.words {
            Words::Text(text) => strings::bytes(text),
            Words::Strref(strref, tokens) => match textures.text(Source::Campaign, *strref) {
                Some(words) => fill_tokens(words, tokens),
                None => return,
            },
            Words::Said(words) => words.clone(),
        };
        if self.datapad {
            datapad_box(gui, textures);
        }
        self.lines = fit(gui, textures, source, &words);
    }
}

/// KOTOR 2's datapad box's layout.
pub const DATAPAD: &str = "datapad";

/// KOTOR 2's numbers for its datapad box, in physical pixels at a text size
/// of 1 (`datapad_box`): the box's width and height, the list's left, top,
/// width and height in the box, the list's scrollbar's width, and the OK
/// button's top in the box, its height and its width.
mod datapad {
    pub const BOX: (f32, f32) = (400.0, 300.0);
    pub const LIST: [f32; 4] = [33.0, 27.0, 334.0, 208.0];
    pub const BAR: f32 = 16.0;
    pub const OK: (f32, f32, f32) = (245.0, 28.0, 120.0);
    /// How far a button stands below the one above it, in physical pixels
    /// whatever the text size.
    pub const BUTTON_GAP: f32 = 2.0;
}

/// The datapad box (`datapad`, already stretched and centred) sized as
/// KOTOR 2 sizes it before it grows the box to its words (`fit`). Each size
/// is KOTOR 2's own number times the text size, whatever the layout says.
/// The box is 400 × 300, about the same middle. The list stands 33 across
/// and 27 down in it, 334 × 208, with its scrollbar 16 wide along its right
/// side. The words are wrapped inside the list's border and padding, left of
/// the scrollbar, and start at the top left, as KOTOR 2 sets them for a
/// datapad. The OK button is 120 × 28, 245 down and centred across. The
/// Cancel button, which a datapad never shows, stands 2 pixels under it.
/// Then the box grows as every message box grows (`fit`): a datapad's box
/// is already as tall as a message box may grow, so it can only widen.
fn datapad_box(gui: &mut Gui, textures: &dyn Textures) {
    let Sizing { text, pixel, .. } = textures.sizing();
    let sized = |at: f32| (at * text) as i32;
    let (width, height) = (sized(datapad::BOX.0), sized(datapad::BOX.1));
    let left = gui.extent.left + gui.extent.width / 2 - width / 2;
    let top = gui.extent.top + gui.extent.height / 2 - height / 2;
    gui.extent = Rect {
        left,
        top,
        width,
        height,
    };
    let [list_left, list_top, list_width, list_height] = datapad::LIST.map(sized);
    let (ok_top, ok_height, ok_width) = (
        sized(datapad::OK.0),
        sized(datapad::OK.1),
        sized(datapad::OK.2),
    );
    // The controls stand in the box's own pixels, from its top left.
    let button = |down: i32| Rect {
        left: width / 2 - ok_width / 2,
        top: down,
        width: ok_width,
        height: ok_height,
    };
    let gap = (datapad::BUTTON_GAP * pixel).round() as i32;
    for control in &mut gui.controls {
        match control.tag.as_str() {
            "LB_MESSAGE" => {
                let list = Rect {
                    left: list_left,
                    top: list_top,
                    width: list_width,
                    height: list_height,
                };
                control.extent = list;
                let bar = sized(datapad::BAR).min(list.width);
                if let Some(scrollbar) = control.scrollbar.as_deref_mut() {
                    scrollbar.extent = Rect {
                        left: list.left + list.width - bar,
                        width: bar,
                        ..list
                    };
                }
                let border = control.border.as_ref();
                let inner = border.map_or(0, |border| border.inner_offset);
                let inner_y = border.map_or(0, |border| border.inner_offset_y.unwrap_or(inner));
                let at = |pixels: i32| (pixels as f32 * pixel).round() as i32;
                let padding = at(control.padding);
                if let Some(proto) = control.proto.as_deref_mut() {
                    proto.extent.left = list.left + at(inner) + padding;
                    proto.extent.top = list.top + at(inner_y) + padding;
                    proto.extent.width = list.width - bar - 2 * (at(inner) + padding);
                    if let Some(words) = proto.text.as_mut() {
                        words.alignment = align::LEFT | align::TOP;
                    }
                }
            }
            "BTN_OK" => control.extent = button(ok_top),
            "BTN_CANCEL" => control.extent = button(ok_top + ok_height + gap),
            _ => {}
        }
    }
}

/// `confirm` `by` taller, its top up half of it, the list as much taller
/// and the buttons below it moved down.
pub(super) fn taller(gui: &mut Gui, by: i32) {
    gui.extent.height += by;
    gui.extent.top -= by / 2;
    let list_bottom = gui
        .find("LB_MESSAGE")
        .map(|list| list.extent.top + list.extent.height);
    for control in &mut gui.controls {
        if control.tag == "LB_MESSAGE" {
            control.extent.height += by;
            if let Some(bar) = control.scrollbar.as_deref_mut() {
                bar.extent.height += by;
            }
        } else if list_bottom.is_some_and(|bottom| control.extent.top >= bottom) {
            control.extent.top += by;
        }
    }
}

/// How far KOTOR 2's message box grows, in layout pixels. KOTOR 2 grows the
/// box after it has stretched the layout, and its numbers count the
/// window's physical pixels. The widest and tallest grow with Aspyr's text
/// size (`draw::text_size`): the box widens while it is narrower than 440
/// times the text size, and grows taller while it is shorter than 280
/// times the text size. It only widens once it is at least 161 pixels
/// tall, and it widens 40 pixels at a time. Those two numbers do not grow
/// with the text size.
struct Growth {
    widest: f32,
    tallest: f32,
    shortest: f32,
    widen: i32,
}

impl Growth {
    /// The limits in layout pixels, at the sizing `textures` has now
    /// (`Sizing::physical`).
    fn of(textures: &dyn Textures) -> Growth {
        // The sizing's text is the text size in layout pixels already.
        let Sizing { text, pixel, .. } = textures.sizing();
        Growth {
            widest: 440.0 * text,
            tallest: 280.0 * text,
            shortest: 161.0 * pixel,
            widen: (40.0 * pixel).round().max(1.0) as i32,
        }
    }
}

/// `confirm` grown to `words` as KOTOR 2 grows its message box, and `words`
/// wrapped to the grown list, a line a row; none when the list or its font
/// is not there to measure with.
///
/// While the words overflow the list, and the box is narrower or shorter
/// than it may grow (`Growth`), KOTOR 2 repeats two steps. First, if the
/// box is at least `Growth::shortest` tall and narrower than its widest,
/// it widens by `Growth::widen`, and it stops there if the words now fit.
/// Then, if the box is shorter than its tallest, it grows a line taller.
/// The list scrolls what still overflows.
fn fit(
    gui: &mut Gui,
    textures: &mut dyn Textures,
    source: Source,
    words: &[u8],
) -> Option<Vec<Vec<u8>>> {
    let growth = Growth::of(textures);
    let at = gui
        .controls
        .iter()
        .position(|control| control.tag == "LB_MESSAGE")?;
    let font = gui.controls[at].proto.as_ref()?.text.as_ref()?.font.clone();
    let (_, font) = textures.font(source, &font)?;
    let pitch = font.line_height().ceil().max(1.0) as i32;
    let proto = gui.controls[at].proto.as_deref_mut()?;
    proto.extent.height = pitch;
    let lines = |gui: &Gui| {
        let list = &gui.controls[at];
        let width = list.proto.as_ref().map_or(0, |proto| proto.extent.width);
        (wrap(&font, words, width as f32), rows_that_fit(list, None))
    };
    let fits = |gui: &Gui| {
        let (wrapped, shown) = lines(gui);
        wrapped.len() <= shown
    };
    let narrow = |gui: &Gui| (gui.extent.width as f32) < growth.widest;
    let short = |gui: &Gui| (gui.extent.height as f32) < growth.tallest;
    while !fits(gui) && (narrow(gui) || short(gui)) {
        if gui.extent.height as f32 >= growth.shortest && narrow(gui) {
            wider(gui, at, growth.widen);
            if fits(gui) {
                break;
            }
        }
        if short(gui) {
            taller(gui, pitch);
        }
    }
    Some(lines(gui).0)
}

/// `confirm` `by` wider about its middle, the list and its rows as much
/// wider, the other controls kept in the middle.
fn wider(gui: &mut Gui, list: usize, by: i32) {
    gui.extent.width += by;
    gui.extent.left -= by / 2;
    for (at, control) in gui.controls.iter_mut().enumerate() {
        if at != list {
            control.extent.left += by / 2;
            continue;
        }
        control.extent.width += by;
        if let Some(proto) = control.proto.as_deref_mut() {
            proto.extent.width += by;
        }
        if let Some(bar) = control.scrollbar.as_deref_mut() {
            bar.extent.left += by;
        }
    }
}
