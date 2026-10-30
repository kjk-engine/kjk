//! A message and OK (`confirm`): why a session could not start, a
//! script's message box (`DisplayMessageBox`, its strref), and the like,
//! the box grown to it as KOTOR's grows (`fit`) and what still does not fit
//! scrolled. OK or Back closes it.

use fmt_odyssey::gui::{Control, Gui};

use crate::draw::Textures;
use crate::intent::Intent;
use crate::paint::{ControlState, ListView, Row, rows_that_fit, wrap};
use crate::screen::{Screen, ScreenId};
use crate::screens::pages::description;
use crate::screens::tab::told;
use crate::skin::Source;
use crate::strings;
use crate::vm::ViewModel;

enum Words {
    Text(String),
    Strref(u32),
    /// In the game's code page.
    Said(Vec<u8>),
}

pub struct Message {
    words: Words,
    /// The words wrapped to the grown box, a row each, once its layout is
    /// in (`fit`).
    lines: Option<Vec<Vec<u8>>>,
}

impl Message {
    fn of(words: Words) -> Message {
        Message { words, lines: None }
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
        Some(Message::of(Words::Strref(strref.parse().ok()?)))
    }
}

/// A message box of pages a script showed (`ViewModel::pages`): KOTOR's
/// tutorial window (its `tutorial.2da` row's `Message_PC0` to `2`, a page
/// each) and TSL's datapad (its item's words). OK turns the page, and on
/// the last closes it; there is no cancel. The tutorial's icon is not drawn
/// [verify where KOTOR puts it].
#[derive(Default)]
pub struct Pages {
    at: usize,
}

impl Screen for Pages {
    fn id(&self) -> ScreenId {
        ScreenId("pages")
    }

    fn layout(&self) -> &str {
        "confirm"
    }

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        match control.tag.as_str() {
            "LB_MESSAGE" => description(vm.pages.get(self.at).map_or(&[][..], Vec::as_slice)),
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

    fn press(&mut self, tag: &str, vm: &ViewModel, out: &mut Vec<Intent>) {
        if tag == "BTN_OK" {
            self.at += 1;
            if self.at >= vm.pages.len() {
                out.push(Intent::Close);
            }
        }
    }

    fn first_focus(&self) -> Option<&str> {
        Some("BTN_OK")
    }

    fn shape(&self, gui: &mut Gui) {
        grow(gui);
    }
}

/// Lines of room the box gains for its message.
const GROW: i32 = 3 * 18;

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
                (None, Words::Strref(strref)) => told(Some(*strref)),
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

    fn fit(&mut self, gui: &mut Gui, textures: &mut dyn Textures, source: Source) {
        let words = match &self.words {
            Words::Text(text) => strings::bytes(text),
            Words::Strref(strref) => match textures.text(Source::Campaign, *strref) {
                Some(words) => words,
                None => return,
            },
            Words::Said(words) => words.clone(),
        };
        self.lines = fit(gui, textures, source, &words);
    }
}

/// `confirm` grown by `GROW` for its message, the buttons below moved down.
pub(super) fn grow(gui: &mut Gui) {
    taller(gui, GROW);
}

/// `confirm` `by` taller, its top up half of it, the list as much taller
/// and the buttons below it moved down.
fn taller(gui: &mut Gui, by: i32) {
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

/// KOTOR's message box (`CSWGuiMessageBox`) grows while its words overflow
/// the list: a line taller until the box is `SHORTEST` tall, then `WIDEN`
/// wider until it is `WIDEST` wide, then a line taller again until it is
/// past `TALLEST`; the list scrolls what still overflows. Layout pixels of
/// its 640 × 480 frame.
const SHORTEST: i32 = 161;
const WIDEST: i32 = 440;
const TALLEST: i32 = 279;
const WIDEN: i32 = 40;

/// `confirm` grown to `words` as KOTOR grows it, and `words` wrapped to
/// the grown list, a line a row; none when the list or its font is not
/// there to measure with.
fn fit(
    gui: &mut Gui,
    textures: &mut dyn Textures,
    source: Source,
    words: &[u8],
) -> Option<Vec<Vec<u8>>> {
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
    loop {
        let (wrapped, shown) = lines(gui);
        if wrapped.len() <= shown {
            break;
        }
        if gui.extent.height >= SHORTEST && gui.extent.width < WIDEST {
            wider(gui, at);
        } else if gui.extent.height <= TALLEST {
            taller(gui, pitch);
        } else {
            break;
        }
    }
    Some(lines(gui).0)
}

/// `confirm` `WIDEN` wider about its middle, the list and its rows as much
/// wider, the other controls kept in the middle.
fn wider(gui: &mut Gui, list: usize) {
    gui.extent.width += WIDEN;
    gui.extent.left -= WIDEN / 2;
    for (at, control) in gui.controls.iter_mut().enumerate() {
        if at != list {
            control.extent.left += WIDEN / 2;
            continue;
        }
        control.extent.width += WIDEN;
        if let Some(proto) = control.proto.as_deref_mut() {
            proto.extent.width += WIDEN;
        }
        if let Some(bar) = control.scrollbar.as_deref_mut() {
            bar.extent.left += WIDEN;
        }
    }
}
