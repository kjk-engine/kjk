//! What the catalogue draws in its window around the virtual screen's
//! image, never in the image: the bar of the mode and the keys, the notice
//! of a change, and the outline of the device (docs/UI.md, "The status
//! bar and the device frame"). The text is in KOTOR's own font, taken from
//! the screens that are shown.

use std::sync::Arc;

use asset_odyssey::font::Font;
use ui::catalogue::Size;
use ui::draw::{DrawList, Quad, TextureId};
use ui::screens::key_mapping::PAD_RAW;
use ui::virtual_screen::Mode;

use super::{Action, KEYS};

/// How tall the bar's letters are in the display's own pixels, as near as a
/// whole number of the font's pixels gets.
const TEXT_PIXELS: f32 = 34.0;

/// How long a notice stays whole, and how long it then takes to fade, in
/// seconds.
const NOTICE_HELD: f32 = 1.8;
const NOTICE_FADE: f32 = 0.6;

const LABEL: [f32; 4] = [0.6, 0.65, 0.72, 1.0];
const VALUE: [f32; 4] = [0.96, 0.96, 0.96, 1.0];
const KEY: [f32; 4] = [1.0, 0.82, 0.3, 1.0];
const PLATE: [f32; 4] = [0.03, 0.03, 0.04, 1.0];
const DIVIDER: [f32; 4] = [0.35, 0.35, 0.4, 1.0];

/// Which keys the bar shows: those of the last device the owner used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Hints {
    #[default]
    Keyboard,
    Pad,
}

/// What a notice says changed.
#[derive(Debug, Clone, PartialEq)]
struct Notice {
    title: String,
    detail: String,
    age: f32,
}

/// What the owner can see change: the notice says which of them did.
#[derive(Debug, Clone, PartialEq)]
pub struct Look {
    pub size: Size,
    pub mode: Mode,
    pub seen: f32,
    pub backdrop: &'static str,
    pub outlines: bool,
    pub overrides: bool,
}

#[derive(Debug, Default)]
pub struct Chrome {
    /// The bar is hidden, for a clean view.
    pub hidden: bool,
    pub hints: Hints,
    notice: Option<Notice>,
    seen: Option<Look>,
    font: Option<(TextureId, Arc<Font>)>,
}

/// What the bar says about the story on show.
pub struct Facts {
    pub story: String,
    pub game: &'static str,
    /// Which layouts the open screens are drawn from
    /// (`ui::layout_files::describe`).
    pub layouts: String,
    pub look: Look,
}

impl Chrome {
    /// The font to write with, from the screens just drawn. A font once
    /// found is kept.
    pub fn look_for_font(&mut self, layers: &[DrawList]) {
        let found = layers
            .iter()
            .flat_map(|layer| &layer.marks)
            .flat_map(|mark| &mark.words)
            .next()
            .map(|words| (words.texture, Arc::clone(&words.font)));
        if found.is_some() {
            self.font = found;
        }
    }

    /// The font forgotten: it belongs to assets that were replaced.
    pub fn forget_font(&mut self) {
        self.font = None;
    }

    /// Time passed, and what changed since the last frame said in a notice.
    pub fn watch(&mut self, look: Look, dt: f32) {
        if let Some(notice) = &mut self.notice {
            notice.age += dt;
            if notice.age > NOTICE_HELD + NOTICE_FADE {
                self.notice = None;
            }
        }
        if let Some(before) = &self.seen {
            if before.size != look.size {
                self.say(device_title(&look.size), "");
            } else if before.mode != look.mode {
                self.say(mode_title(&look), mode_detail(look.mode));
            } else if before.overrides != look.overrides {
                let (title, detail) = if look.overrides {
                    (
                        "kjk's layouts",
                        "Screens with an override file are drawn from it.",
                    )
                } else {
                    (
                        "KOTOR's layouts",
                        "Overrides are off, so KOTOR's own layouts are drawn.",
                    )
                };
                self.say(title, detail);
            } else if before.backdrop != look.backdrop {
                self.say(
                    format!("Backdrop: {}", look.backdrop),
                    backdrop_detail(look.backdrop),
                );
            }
        }
        self.seen = Some(look);
    }

    pub fn say(&mut self, title: impl Into<String>, detail: &str) {
        self.notice = Some(Notice {
            title: title.into(),
            detail: detail.to_string(),
            age: 0.0,
        });
    }

    /// The bar's key for `action` in the words of the hints shown.
    pub fn key_of(&self, action: Action) -> String {
        let (_, key, chord) = KEYS
            .iter()
            .find(|(each, ..)| *each == action)
            .expect("every action has a key");
        match self.hints {
            Hints::Keyboard => (*key).to_string(),
            Hints::Pad => pad_name(chord),
        }
    }

    /// The height of the bar for a window `width` logical pixels wide with
    /// `factor` display pixels to each, nothing when it is hidden or no font
    /// is known yet.
    pub fn bar_height(&self, facts: &Facts, width: f32, factor: f32) -> f32 {
        if self.hidden {
            return 0.0;
        }
        self.layout(facts, width, factor)
            .map_or(0.0, |layout| layout.height)
    }

    fn layout(&self, facts: &Facts, width: f32, factor: f32) -> Option<Layout> {
        let (_, font) = self.font.as_ref()?;
        let pen = Pen::new(font, factor, 1.0);
        let gap = pen.height * 1.4;
        let pad = pen.height * 0.5;
        let row = pen.height * 1.45;
        let status = status(facts);
        let keys: Vec<Item> = KEYS
            .iter()
            .map(|(action, ..)| Item {
                head: self.key_of(*action),
                head_color: KEY,
                tail: key_label(*action).to_string(),
                tail_color: LABEL,
            })
            .collect();
        let mut rows: Vec<Vec<Placed>> = Vec::new();
        for (group, items) in [status, keys].into_iter().enumerate() {
            let mut line: Vec<Placed> = Vec::new();
            let mut x = pad;
            for item in items {
                let size = pen.measure(&item.head) + pen.measure(" ") + pen.measure(&item.tail);
                if x + size > width - pad && !line.is_empty() {
                    rows.push(std::mem::take(&mut line));
                    x = pad;
                }
                line.push(Placed { x, item, group });
                x += size + gap;
            }
            if !line.is_empty() {
                rows.push(line);
            }
        }
        let groups = rows
            .iter()
            .filter_map(|row| row.first().map(|placed| placed.group))
            .collect::<Vec<_>>();
        let split = groups.windows(2).any(|pair| pair[0] != pair[1]);
        let height = pad * 2.0 + row * rows.len() as f32 + if split { pad } else { 0.0 };
        Some(Layout {
            rows,
            height,
            row,
            pad,
            groups,
        })
    }

    /// The bar along the bottom of a window `window` logical pixels large.
    pub fn bar(&self, facts: &Facts, window: (f32, f32), factor: f32) -> Option<DrawList> {
        if self.hidden {
            return None;
        }
        let layout = self.layout(facts, window.0, factor)?;
        let (texture, font) = self.font.as_ref()?;
        let pen = Pen::new(font, factor, 1.0);
        let top = window.1 - layout.height;
        let mut out = DrawList {
            size: window,
            ..DrawList::default()
        };
        out.quads
            .push(Quad::solid([0.0, top, window.0, layout.height], PLATE));
        out.quads.push(Quad::solid(
            [0.0, top, window.0, (1.0 / factor).max(1.0)],
            DIVIDER,
        ));
        let mut y = top + layout.pad;
        let mut last = layout.groups.first().copied();
        for (row, group) in layout.rows.iter().zip(&layout.groups) {
            if last != Some(*group) {
                y += layout.pad;
                out.quads.push(Quad::solid(
                    [
                        layout.pad,
                        y - layout.pad / 2.0,
                        window.0 - 2.0 * layout.pad,
                        (1.0 / factor).max(1.0),
                    ],
                    DIVIDER,
                ));
                last = Some(*group);
            }
            for placed in row {
                let item = &placed.item;
                let mut x = placed.x;
                x = pen.put(&mut out, *texture, &item.head, [x, y], item.head_color);
                x += pen.measure(" ");
                pen.put(&mut out, *texture, &item.tail, [x, y], item.tail_color);
            }
            y += layout.row;
        }
        Some(out)
    }

    /// The notice in the middle of the window, while it lasts.
    pub fn notice(&self, window: (f32, f32), factor: f32) -> Option<DrawList> {
        let notice = self.notice.as_ref()?;
        let (texture, font) = self.font.as_ref()?;
        let big = Pen::new(font, factor, 2.0);
        let small = Pen::new(font, factor, 1.0);
        let fade = ((NOTICE_HELD + NOTICE_FADE - notice.age) / NOTICE_FADE).clamp(0.0, 1.0);
        let mut width = big.measure(&notice.title);
        let mut height = big.height;
        if !notice.detail.is_empty() {
            width = width.max(small.measure(&notice.detail));
            height += small.height * 1.4;
        }
        let pad = big.height * 0.6;
        let (w, h) = (width + pad * 2.0, height + pad * 2.0);
        let (x, y) = ((window.0 - w) / 2.0, (window.1 - h) / 2.0);
        let mut out = DrawList {
            size: window,
            ..DrawList::default()
        };
        out.quads.push(Quad::solid(
            [x, y, w, h],
            [PLATE[0], PLATE[1], PLATE[2], 0.88 * fade],
        ));
        let fade_color = |color: [f32; 4]| [color[0], color[1], color[2], color[3] * fade];
        big.put(
            &mut out,
            *texture,
            &notice.title,
            [x + (w - big.measure(&notice.title)) / 2.0, y + pad],
            fade_color(VALUE),
        );
        if !notice.detail.is_empty() {
            small.put(
                &mut out,
                *texture,
                &notice.detail,
                [
                    x + (w - small.measure(&notice.detail)) / 2.0,
                    y + pad + big.height * 1.25,
                ],
                fade_color(LABEL),
            );
        }
        Some(out)
    }
}

struct Item {
    head: String,
    head_color: [f32; 4],
    tail: String,
    tail_color: [f32; 4],
}

struct Placed {
    x: f32,
    item: Item,
    group: usize,
}

struct Layout {
    rows: Vec<Vec<Placed>>,
    height: f32,
    row: f32,
    pad: f32,
    groups: Vec<usize>,
}

/// The font at a size that is a whole number of its pixels to the display's.
struct Pen<'a> {
    font: &'a Font,
    /// Logical pixels to each pixel of the font.
    scale: f32,
    /// The letters' height in logical pixels.
    height: f32,
}

impl<'a> Pen<'a> {
    fn new(font: &'a Font, factor: f32, times: f32) -> Pen<'a> {
        let whole = (TEXT_PIXELS / font.height.max(1.0)).round().max(1.0) * times;
        let scale = whole / factor.max(f32::EPSILON);
        Pen {
            font,
            scale,
            height: font.height * scale,
        }
    }

    fn advance(&self, byte: u8) -> f32 {
        let glyph = self
            .font
            .glyph(byte)
            .map_or_else(|| self.font.advance(byte), |glyph| glyph.width + 1.0);
        glyph * self.scale
    }

    fn measure(&self, words: &str) -> f32 {
        latin1(words)
            .into_iter()
            .map(|byte| self.advance(byte))
            .sum()
    }

    /// `words` with its top left at `at`. The pen's place after it.
    fn put(
        &self,
        out: &mut DrawList,
        texture: TextureId,
        words: &str,
        at: [f32; 2],
        color: [f32; 4],
    ) -> f32 {
        let mut x = at[0];
        for byte in latin1(words) {
            if let Some(glyph) = self.font.glyph(byte)
                && byte != b' '
            {
                out.quads.push(Quad {
                    uv: glyph.uv,
                    ..Quad::image(
                        [
                            x,
                            at[1],
                            glyph.width * self.scale,
                            glyph.height * self.scale,
                        ],
                        texture,
                        color,
                    )
                });
            }
            x += self.advance(byte);
        }
        x
    }
}

/// The font's bytes for `words`: its letters are those of Windows-1252.
fn latin1(words: &str) -> Vec<u8> {
    words
        .chars()
        .map(|letter| u8::try_from(u32::from(letter)).unwrap_or(b'?'))
        .collect()
}

fn item(label: &str, value: String) -> Item {
    Item {
        head: label.to_string(),
        head_color: LABEL,
        tail: value,
        tail_color: VALUE,
    }
}

fn status(facts: &Facts) -> Vec<Item> {
    let look = &facts.look;
    vec![
        item("Story", format!("{} ({})", facts.story, facts.game)),
        item("Size", ui::device_frame::describe(&look.size)),
        item("Mode", mode_title(look)),
        item("Backdrop", look.backdrop.to_string()),
        item("Layouts", facts.layouts.clone()),
        item(
            "Outlines",
            if look.outlines { "on" } else { "off" }.to_string(),
        ),
    ]
}

fn device_title(size: &Size) -> String {
    ui::device_frame::describe(size)
}

/// The mode in words, with the scale when it is as seen.
pub fn mode_title(look: &Look) -> String {
    match look.mode {
        Mode::Fit => "Fit to the window".to_string(),
        Mode::Pixel => "Pixel for pixel".to_string(),
        Mode::Seen => format!("As seen, {:.2} times", look.seen),
    }
}

fn mode_detail(mode: Mode) -> &'static str {
    match mode {
        Mode::Fit => "The whole device is shown as large as the window allows.",
        Mode::Pixel => "One pixel of the screen is one pixel of your display.",
        Mode::Seen => "The screen is as big as it looks from the device's usual distance.",
    }
}

fn backdrop_detail(backdrop: &str) -> &'static str {
    match backdrop {
        "stripes" => "Diagonal stripes show through the see-through parts.",
        "play" => "A frame of play shows through the see-through parts.",
        _ => "Plain black behind the screen.",
    }
}

fn key_label(action: Action) -> &'static str {
    match action {
        Action::NextSize => "Size",
        Action::NextMode => "Mode",
        Action::List => "Story list",
        Action::Layouts => "KOTOR or kjk layout",
        Action::Outlines => "Outlines",
        Action::Backdrop => "Backdrop",
        Action::SaveTryout => "Save tryout as a story",
        Action::HideBar => "Hide this bar",
    }
}

/// A pad chord such as `LB+North`, as a pad's buttons are written.
fn pad_name(chord: &str) -> String {
    let chord = chord.strip_prefix(PAD_RAW).unwrap_or(chord);
    chord
        .split('+')
        .map(|button| match button {
            "North" => "Y",
            "West" => "X",
            "East" => "B",
            "South" => "A",
            "DPadUp" => "D-pad up",
            "DPadDown" => "D-pad down",
            "DPadLeft" => "D-pad left",
            "DPadRight" => "D-pad right",
            other => other,
        })
        .collect::<Vec<_>>()
        .join(" + ")
}
