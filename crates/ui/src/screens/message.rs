//! A message and OK (`confirm`): why a session could not start, a
//! script's message box (`DisplayMessageBox`, its strref), and the like,
//! the box grown to it as KOTOR's grows (`fit`) and what still does not fit
//! scrolled. OK or Back closes it. Every other box on `confirm` (a
//! question, a tutorial's pages) asks through a `Message` for its words, so
//! each grows and scrolls alike.

use fmt_odyssey::gui::{Control, Gui};

use crate::draw::Textures;
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
/// it].
pub struct Pages {
    at: usize,
    page: Message,
}

impl Pages {
    /// Page `at` of the pages `vm` holds.
    pub fn new(at: &str, vm: &ViewModel) -> Option<Pages> {
        let at = at.parse().ok()?;
        Some(Pages {
            at,
            page: Message::said(vm.pages.get(at).cloned().unwrap_or_else(Vec::new)),
        })
    }
}

impl Screen for Pages {
    fn id(&self) -> ScreenId {
        ScreenId("pages")
    }

    fn layout(&self) -> &str {
        "confirm"
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

    fn fit(&mut self, gui: &mut Gui, textures: &mut dyn Textures, source: Source) {
        self.page.fit(gui, textures, source);
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

    fn fit(&mut self, gui: &mut Gui, textures: &mut dyn Textures, source: Source) {
        self.asked.fit(gui, textures, source);
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

    fn fit(&mut self, gui: &mut Gui, textures: &mut dyn Textures, source: Source) {
        let words = match &self.words {
            Words::Text(text) => strings::bytes(text),
            Words::Strref(strref, tokens) => match textures.text(Source::Campaign, *strref) {
                Some(words) => fill_tokens(words, tokens),
                None => return,
            },
            Words::Said(words) => words.clone(),
        };
        self.lines = fit(gui, textures, source, &words);
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
    /// The limits in layout pixels, at the text scale and physical pixel
    /// `textures` has now (`draw::physical`).
    fn of(textures: &dyn Textures) -> Growth {
        // The text scale is the text size in layout pixels already.
        let (text, pixel) = (textures.text_scale(), textures.pixel());
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
