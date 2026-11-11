//! A layout drawn as KOTOR draws it:
//!
//! - a border is a fill stretched inside `DIMENSION`, four edges (the
//!   texture's top facing out: left and right turned a quarter, the bottom
//!   mirrored) and four corners (drawn as the top left, mirrored for the
//!   others), all tinted by the border's colour when the layout gives one
//!   (white else); a picture the screen puts in the fill's place is not;
//! - a focused control draws its `HILIGHT` border when that has textures,
//!   and its text in the palette's highlight colour, pulsing when there is
//!   no highlight border to show the focus;
//! - a glyph is read half a texel inside its cell, so its neighbours on
//!   the sheet never bleed in;
//! - text wraps on spaces (a word too long for a line breaks), lines past
//!   the box are dropped, and `ALIGNMENT` is a bit field: 1, 2, 4 left,
//!   centre, right; 8, 16, 32 top, middle, bottom; lines start on whole
//!   pixels;
//! - a list box draws its prototype row once per item that fits, from the
//!   prototype's own place down by its height and the list's padding (or,
//!   fitting, each row as tall as its text wrapped, cut at the list; a
//!   strref and no rows is one such row); an
//!   image-button row keeps a square at its left for a frame, an icon and
//!   a stack's count (KOTOR's: at the square's foot, right); a chain row
//!   is KOTOR's feat flow (`flow_cells`): up to three feats' squares
//!   joined by arrows; the scroll bar shows arrows and a thumb sized to
//!   what shows;
//! - a progress bar's fill grows from the left (from the bottom when the
//!   layout says not from the left), tinted as a border's fill is.
//!
//! A layout's own textures, fonts and words are read from where the
//! layout came from (`source`), and its colours recoloured with its art;
//! what the screen puts in (pictures, icons, words) from the campaign.
//!
//! Anything that does not resolve (a texture, a font) is a refusal, and
//! the rest of the control is drawn.

use fmt_odyssey::gui::align;
use fmt_odyssey::gui::{Border, Control, Gui, Kind};

use crate::draw::{DrawList, Quad, TextureId, Textures};
use crate::skin::{self, Source};

/// The space between a KOTOR 2 row's icon square and its name.
const ROW_GAP: i32 = 4;
/// The font both games draw numbers over the world and on squares with.
pub const NUMBER_FONT: &str = "fnt_d16x16";

/// A stack's count on its item's square (both games' item entries, in
/// the screens' base colour): its font, and its box at the square's
/// bottom right, two digits centred in the narrow one, more right in the
/// wide one (as KOTOR 2 places them on its 36-pixel square).
const COUNT_FONT: &str = NUMBER_FONT;
const COUNT_BOX: (f32, f32, f32) = (21.0, 55.0, 19.0);
/// KOTOR 2's feat flow (its engine's numbers): a row's width and height,
/// a feat's square (2 down), its icon's share of it, the arrows' size, and
/// its art.
pub const FLOW_HEIGHT: i32 = 44;
const FLOW_WIDTH: f32 = 262.0;
const FLOW_CELL: f32 = 40.0;
const FLOW_ICON: f32 = 0.8;
/// The pad's hints along a screen's bottom (layout pixels): the strip's
/// height, a button's, the room after the last hint, between a hint's
/// buttons and words, and between hints; their font.
const HINT_BAR: f32 = 22.0;
const HINT_ICON: f32 = 17.0;
const HINT_END: f32 = 14.0;
const HINT_GAP: f32 = 4.0;
const HINT_SPACE: f32 = 14.0;
const HINT_FONT: &str = "dialogfont16x16";
/// The room round a tooltip's words (layout pixels).
const TIP_PAD: f32 = 4.0;

/// A pad's hint: the buttons to press (their names in the settings,
/// `South`, `LeftTrigger`) and what they do.
#[derive(Debug, Clone, PartialEq)]
pub struct Hint {
    pub buttons: Vec<&'static str>,
    pub words: Vec<u8>,
}

/// A row's badge's height, of the row's, and the room kept at its right
/// end (layout pixels) after a badge or a value.
const BADGE: f32 = 0.95;
const ROW_END: f32 = 8.0;
/// The outline of the chosen feat, in layout pixels.
const FLOW_OUTLINE: f32 = 2.0;
const FLOW_ARROW: f32 = 32.0;
const FLOW_BACK: &str = "uibit_abi_back";
const FLOW_ARROW_IMAGE: &str = "uibit_abi_arrow";
/// Both games' red (`CGuiInGame::COLOR_RED`): a feat that may not be
/// taken (KOTOR 2's feat flow), an item the member may not wear (the equip
/// list's `CSWGuiInGameItemEntry::SetCanUse`).
const RED: [f32; 3] = [0.74, 0.11, 0.0];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub base: [f32; 3],
    pub hilight: [f32; 3],
    pub disabled: [f32; 3],
}

impl Palette {
    pub const K1: Palette = Palette {
        base: [0.0, 0.639_216, 0.952_941],
        hilight: [0.980_392, 1.0, 0.0],
        disabled: [0.0, 0.349_02, 0.549_02],
    };
    pub const TSL: Palette = Palette {
        base: [0.192_157, 0.768_627, 0.647_059],
        hilight: [0.768_627, 0.768_627, 0.686_275],
        disabled: [0.513_725, 0.513_725, 0.415_686],
    };
}

/// One list row.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Row {
    pub text: Vec<u8>,
    /// An entry of KOTOR 2's talk table, read in place of `text` (an
    /// options row's words).
    pub told: Option<u32>,
    /// A check box row's box is ticked (KOTOR 2's options lists).
    pub checked: bool,
    /// Drawn in the disabled colour, while it is still pointed at and
    /// described: an option that kjk does not do yet.
    pub greyed: bool,
    pub icon: Option<String>,
    /// An image at the row's right end, as tall as the row and as wide as
    /// its own shape (a pad's buttons, `icons`).
    pub badge: Option<String>,
    /// Words at the row's right end, the row's text at its left (a key's
    /// name beside its action).
    pub value: Option<Vec<u8>>,
    pub frame: Option<String>,
    pub count: Option<u32>,
    /// Drawn faded (waiting on the host).
    pub dimmed: bool,
    /// Drawn in red, chosen or not: an item the member may not wear.
    pub refused: bool,
    /// A chain's feats, left to right (KOTOR's feat flow), in place of the
    /// rest.
    pub cells: Vec<Cell>,
}

/// A feat in a chain row: its icon and where the character stands with it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Cell {
    pub icon: Option<String>,
    pub look: CellLook,
}

/// KOTOR's feat flow's states: had (or granted), picked now, open to pick,
/// not open (the arrow to it hidden).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CellLook {
    Known,
    Picked,
    Open,
    #[default]
    Closed,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ListView {
    pub rows: Vec<Row>,
    /// The first row shown.
    pub first: usize,
    pub selected: Option<usize>,
    pub focused: Option<usize>,
    /// The selected row's chosen cell (a chain row's).
    pub cell: usize,
    /// The chain cell the pointer or the cursor rests on, by row and cell:
    /// read, not chosen.
    pub hover: Option<(usize, usize)>,
    /// Each row as tall as its text wrapped (a message, a description),
    /// at least the prototype's height.
    pub fit: bool,
    /// Fitted rows that are still chosen (a conversation's replies); a
    /// fitted list is otherwise read.
    pub pick: bool,
    /// The rows rise through the list at so many layout pixels a second,
    /// from below it until the last has gone (the credits): the stack
    /// keeps the time and the screen hears when they have gone
    /// (`Screen::rolled`).
    pub roll: Option<f32>,
    /// How far the rows are drawn up from their place (layout pixels).
    pub offset: i32,
    /// The list shows its last rows (a log), until the player scrolls it.
    pub follow: bool,
}

/// What a screen says of a control this frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ControlState {
    pub hidden: bool,
    pub disabled: bool,
    pub focused: bool,
    pub selected: bool,
    /// In place of the layout's text.
    pub text: Option<Vec<u8>>,
    /// In turn in place of `text` where it wraps past the control's box:
    /// the first that fits, else the last.
    pub shorter: Vec<Vec<u8>>,
    /// In place of the layout's text, a talk-table entry's.
    pub strref: Option<u32>,
    /// `strref` is KOTOR 2's (the skin's talk table), not the campaign's.
    pub skin_strref: bool,
    /// What the entry's tokens (`<CUSTOM0>`, by name without the brackets)
    /// read as.
    pub tokens: Vec<(&'static str, Vec<u8>)>,
    /// In place of the border's fill (a portrait, an icon, a map).
    pub fill: Option<String>,
    /// A progress bar's or slider's share, 0 to 1.
    pub value: Option<f32>,
    pub list: Option<ListView>,
    /// A 3D view over the control's fill (the game renders it).
    pub scene: Option<crate::vm::SceneView>,
    /// The 3D view reaches the window's sides past the layout's, at the
    /// layout's scale (the main menu's smoke and mist across a wide window).
    pub wide: bool,
    /// The size in layout pixels that the 3D view's image is made for, when
    /// the control is drawn at another size (a growing class tile,
    /// `Screen::grown`). The stack sets it. The image keeps this size, so
    /// the view's scene is not read again while the control grows. The
    /// image is shown scaled to the control's height and cut to its width,
    /// which is what the same lens sees through a view of the control's
    /// shape.
    pub scene_size: Option<[f32; 2]>,
    /// A colour in place of the layout's: its text's, its border's, a
    /// bar's fill.
    pub tint: Option<[f32; 3]>,
    /// A progress bar's art in place of the layout's, and the colour it is
    /// drawn in in place of the layout's bar colour (KOTOR's own vitality
    /// red and poisoned green): the art kept, unlike `tint`.
    pub bar: (Option<String>, Option<[f32; 3]>),
    /// Its border, or the picture in its place, at the pulsing alpha
    /// (`Painter::pulsing`): KOTOR's pulsing borders.
    pub pulsing: bool,
    /// Its text and border in red whether chosen or not (`Row::refused`).
    pub refused: bool,
    /// Its text and border in the disabled colour while it still takes a
    /// press (K1's map page's return button, refused: a press says why).
    pub greyed: bool,
}

/// Whether a control shows words: the screen's, a strref's, or the
/// layout's own. KOTOR 2 draws a toggle with words as a check box, its mark
/// at the left of the words (`Painter::check_mark`), and one without words
/// as a button whose picture fills it.
fn worded(text: &fmt_odyssey::gui::Text, state: &ControlState) -> bool {
    state.text.as_ref().is_some_and(|words| !words.is_empty())
        || state.strref.is_some()
        || text.strref.is_some()
        || !text.text.is_empty()
}

/// A state's talk-table entry, its tokens filled in.
fn told(textures: &dyn Textures, state: &ControlState) -> Option<Vec<u8>> {
    let source = if state.skin_strref {
        Source::Skin
    } else {
        Source::Campaign
    };
    let words = textures.text(source, state.strref?)?;
    Some(fill_tokens(words, &state.tokens))
}

/// A list's words without rows (its text or its strref's), as one row
/// fitted to them. The painter draws such a list this way, and the stack
/// scrolls it by lines the same way.
pub(crate) fn words_view(textures: &dyn Textures, state: &ControlState) -> Option<ListView> {
    let words = state.text.clone().or_else(|| told(textures, state))?;
    Some(ListView {
        rows: vec![Row {
            text: words,
            ..Row::default()
        }],
        fit: true,
        ..ListView::default()
    })
}

pub struct Painter<'a> {
    pub textures: &'a mut dyn Textures,
    pub palette: Palette,
    /// Where the layout being painted came from.
    pub source: Source,
    /// The focus's alpha, 0.2 to 1 and back.
    pub pulse: f32,
    /// A pulsing border's alpha, 1 to 0.4 and back (`Stack::pulsing`).
    pub pulsing: f32,
    /// It painted a pulsing border: what it drew changes from frame to
    /// frame however still what it shows is.
    pub pulsed: bool,
    pub refusals: Vec<String>,
    /// One physical pixel of the display in layout pixels (1 when they are
    /// the same; `draw::physical`). KOTOR 2 draws a border's corners and
    /// edges at its DIMENSION in physical pixels, however far the control
    /// itself is stretched, so a bigger window makes a button bigger but
    /// its corners no rounder. Text is placed on whole physical pixels.
    pub pixel: f32,
}

/// A texture name that means "none".
pub(crate) fn none(name: &str) -> bool {
    name.is_empty() || name == "0"
}

fn rgba(rgb: [f32; 3], alpha: f32) -> [f32; 4] {
    [rgb[0], rgb[1], rgb[2], alpha]
}

/// A control's extent: left, top, width, height.
pub(crate) fn rect_of(control: &Control) -> [f32; 4] {
    let extent = control.extent;
    [
        extent.left as f32,
        extent.top as f32,
        extent.width as f32,
        extent.height as f32,
    ]
}

/// The border, when it has something to draw.
fn textured(border: &Option<Border>) -> Option<&Border> {
    border.as_ref().filter(|border| has_textures(border))
}

fn has_textures(border: &Border) -> bool {
    !(none(&border.corner) && none(&border.edge) && none(&border.fill))
}

/// How tall a list's fixed rows are: a chain row KOTOR's feat flow's
/// height, any other its prototype's.
pub fn proto_height(proto: &Control, view: Option<&ListView>) -> i32 {
    match view {
        Some(view) if view.rows.iter().any(|row| !row.cells.is_empty()) => FLOW_HEIGHT,
        _ => proto.extent.height,
    }
}

/// How many prototype rows a list box shows.
pub fn rows_that_fit(list: &Control, view: Option<&ListView>) -> usize {
    let Some(proto) = &list.proto else {
        return 0;
    };
    let step = proto_height(proto, view) + list.padding;
    if step <= 0 {
        return 0;
    }
    (list.extent.height / step).max(0) as usize
}

impl<'a> Painter<'a> {
    pub fn new(textures: &'a mut dyn Textures, palette: Palette) -> Painter<'a> {
        Painter {
            textures,
            palette,
            source: Source::Campaign,
            pulse: 1.0,
            pulsing: 1.0,
            pulsed: false,
            refusals: Vec::new(),
            pixel: 1.0,
        }
    }

    /// `border`'s dimension in layout pixels: its DIMENSION in physical
    /// pixels (`pixel`).
    fn dimension(&self, border: &Border) -> f32 {
        border.dimension.max(0) as f32 * self.pixel
    }

    fn refuse(&mut self, why: String) {
        if !self.refusals.contains(&why) {
            self.refusals.push(why);
        }
    }

    fn texture(&mut self, source: Source, name: &str) -> Option<TextureId> {
        if none(name) {
            return None;
        }
        let found = self.textures.texture(source, name);
        if found.is_none() {
            self.refuse(format!("texture {name}"));
        }
        found
    }

    /// A layout colour as drawn: white when unset, recoloured with its art.
    fn color(&self, color: Option<[f32; 3]>) -> [f32; 3] {
        let color = color.unwrap_or([1.0; 3]);
        if self.textures.recoloured(self.source) {
            skin::recolour_layout(color)
        } else {
            color
        }
    }

    /// `name` (read from `source`) stretched over `rect`.
    fn image(
        &mut self,
        source: Source,
        name: &str,
        rect: [f32; 4],
        color: [f32; 4],
        out: &mut DrawList,
    ) {
        if let Some(id) = self.texture(source, name) {
            out.quads.push(Quad {
                additive: self.textures.additive(id),
                ..Quad::image(rect, id, color)
            });
        }
    }

    /// A border over `rect`: fill, edges, corners; `tint` colours the
    /// edges and corners.
    pub fn border(&mut self, border: &Border, rect: [f32; 4], tint: [f32; 4], out: &mut DrawList) {
        self.frame(border, None, rect, tint, out);
    }

    fn frame(
        &mut self,
        border: &Border,
        fill: Option<&str>,
        rect: [f32; 4],
        tint: [f32; 4],
        out: &mut DrawList,
    ) {
        let [l, t, w, h] = rect;
        // A border without edges or corners is its fill alone, whatever
        // its dimension (KOTOR 2's minus arrows: 32 on a 28 button).
        let d = if border.edge.is_empty() && border.corner.is_empty() {
            0.0
        } else {
            self.dimension(border)
        };
        let (source, fill, fill_tint) = match fill {
            Some(picture) => (Source::Campaign, picture, [1.0, 1.0, 1.0, tint[3]]),
            None if border.color.is_some() => (self.source, border.fill.as_str(), tint),
            None => (self.source, border.fill.as_str(), [1.0, 1.0, 1.0, tint[3]]),
        };
        if let Some(id) = self.texture(source, fill) {
            let inner = [l + d, t + d, w - 2.0 * d, h - 2.0 * d];
            if inner[2] > 0.0 && inner[3] > 0.0 {
                out.quads.push(Quad {
                    additive: self.textures.additive(id),
                    ..Quad::image(inner, id, fill_tint)
                });
            }
        }
        if d <= 0.0 {
            return;
        }
        if let Some(edge) = self.texture(self.source, &border.edge) {
            let plain = Quad::image([0.0; 4], edge, tint);
            if h - 2.0 * d > 0.0 {
                out.quads.push(Quad {
                    rect: [l, t + d, d, h - 2.0 * d],
                    turns: 3,
                    ..plain
                });
                out.quads.push(Quad {
                    rect: [l + w - d, t + d, d, h - 2.0 * d],
                    turns: 1,
                    ..plain
                });
            }
            if w - 2.0 * d > 0.0 {
                out.quads.push(Quad {
                    rect: [l + d, t, w - 2.0 * d, d],
                    ..plain
                });
                out.quads.push(Quad {
                    rect: [l + d, t + h - d, w - 2.0 * d, d],
                    uv: [0.0, 1.0, 1.0, 0.0],
                    ..plain
                });
            }
        }
        if let Some(corner) = self.texture(self.source, &border.corner) {
            let plain = Quad::image([0.0; 4], corner, tint);
            for (x, y, uv) in [
                (l, t, [0.0, 0.0, 1.0, 1.0]),
                (l + w - d, t, [1.0, 0.0, 0.0, 1.0]),
                (l, t + h - d, [0.0, 1.0, 1.0, 0.0]),
                (l + w - d, t + h - d, [1.0, 1.0, 0.0, 0.0]),
            ] {
                out.quads.push(Quad {
                    rect: [x, y, d, d],
                    uv,
                    ..plain
                });
            }
        }
    }

    /// `bytes` in `font` inside `rect`, wrapped, aligned and cut at the
    /// box (at least one line shows).
    /// The image `name` as large as fits `area` with its own shape,
    /// centred across it at its top (a pad's buttons over an action slot).
    pub fn badge(&mut self, name: &str, area: [f32; 4], out: &mut DrawList) {
        self.image_in(name, area, true, out);
    }

    /// The image `name` as large as fits `area` with its own shape, at its
    /// top left (a prompt's key before its words). Returns how wide it was
    /// drawn (0 when there is no such image).
    pub fn badge_left(&mut self, name: &str, area: [f32; 4], out: &mut DrawList) -> f32 {
        self.image_in(name, area, false, out)
    }

    fn image_in(&mut self, name: &str, area: [f32; 4], centred: bool, out: &mut DrawList) -> f32 {
        let Some(id) = self.texture(Source::Campaign, name) else {
            return 0.0;
        };
        let [x, y, w, h] = area;
        let (iw, ih) = self.textures.size(id);
        let scale = (w / iw.max(1) as f32).min(h / ih.max(1) as f32);
        let (wide, tall) = (iw as f32 * scale, ih as f32 * scale);
        let left = if centred { x + (w - wide) / 2.0 } else { x };
        out.quads
            .push(Quad::image([left, y, wide, tall], id, [1.0; 4]));
        wide
    }

    /// The pad's `hints` along the bottom of a frame `w` × `h` wide (the
    /// strip reaching `reach` past its sides, as the screens' veil does),
    /// right to left from its right: each hint's buttons as `kind`'s pad
    /// draws them (`icons`), then its words in the layout's buttons'
    /// colour (`tint`), as console games show them.
    pub fn hint_bar(
        &mut self,
        (kind, tint): (&str, Option<[f32; 3]>),
        hints: &[Hint],
        (w, h): (f32, f32),
        reach: f32,
        out: &mut DrawList,
    ) {
        if hints.is_empty() {
            return;
        }
        let top = h - HINT_BAR;
        out.quads.push(Quad::solid(
            [-reach, top, w + 2.0 * reach, HINT_BAR],
            [0.0, 0.0, 0.0, 0.8],
        ));
        let font = self
            .textures
            .font(self.source, HINT_FONT)
            .map(|(_, font)| font);
        let color = rgba(
            tint.map_or(self.palette.base, |tint| self.color(Some(tint))),
            1.0,
        );
        let mut x = w - HINT_END;
        for hint in hints.iter().rev() {
            let width = font
                .as_ref()
                .map_or(0.0, |font| font.measure(&hint.words))
                .ceil();
            x -= width;
            let words = [x, top, width + 1.0, HINT_BAR];
            self.text(
                HINT_FONT,
                &hint.words,
                words,
                align::LEFT | align::MIDDLE,
                color,
                out,
            );
            x -= HINT_GAP;
            for button in hint.buttons.iter().rev() {
                let name = crate::icons::pad_texture(kind, button);
                let Some(id) = self.texture(Source::Campaign, &name) else {
                    continue;
                };
                let (iw, ih) = self.textures.size(id);
                let wide = HINT_ICON * iw as f32 / ih.max(1) as f32;
                x -= wide;
                let rect = [x, top + (HINT_BAR - HINT_ICON) / 2.0, wide, HINT_ICON];
                out.quads.push(Quad::image(rect, id, [1.0; 4]));
                x -= 2.0;
            }
            x -= HINT_SPACE;
        }
    }

    /// A pointer's tip: `words` on a dark box with a rim in the layout's
    /// colour, its lower left corner at `at`.
    pub fn tooltip(&mut self, words: &[u8], at: [f32; 2], out: &mut DrawList) {
        let Some((_, font)) = self.textures.font(self.source, HINT_FONT) else {
            return;
        };
        let (wide, tall) = (font.measure(words).ceil(), font.line_height().ceil());
        let (pad, rim) = (TIP_PAD, 1.0);
        let rect = [
            at[0],
            at[1] - tall - 2.0 * pad,
            wide + 2.0 * pad,
            tall + 2.0 * pad,
        ];
        let [l, t, w, h] = rect;
        let color = rgba(self.palette.base, 1.0);
        out.quads.push(Quad::solid(rect, color));
        out.quads.push(Quad::solid(
            [l + rim, t + rim, w - 2.0 * rim, h - 2.0 * rim],
            [0.0, 0.0, 0.0, 0.85],
        ));
        let inner = [l + pad, t + pad, wide + 1.0, tall];
        self.text(
            HINT_FONT,
            words,
            inner,
            align::LEFT | align::MIDDLE,
            color,
            out,
        );
    }

    pub fn text(
        &mut self,
        font: &str,
        bytes: &[u8],
        rect: [f32; 4],
        alignment: i32,
        color: [f32; 4],
        out: &mut DrawList,
    ) {
        if bytes.is_empty() {
            return;
        }
        let Some((id, font)) = self.textures.font(self.source, font) else {
            self.refuse(format!("font {font}"));
            return;
        };
        let [l, t, w, h] = rect;
        let (tw, th) = self.textures.size(id);
        let (du, dv) = (0.5 / tw.max(1) as f32, 0.5 / th.max(1) as f32);
        let pitch = font.line_height().max(1.0);
        let mut lines = wrap(&font, bytes, w);
        let fit = ((h / pitch).floor() as usize).max(1);
        lines.truncate(fit);
        let n = lines.len() as f32;
        let top = match alignment & align::DOWN {
            align::MIDDLE => t + h / 2.0 - n * pitch / 2.0,
            align::BOTTOM => t + h - n * pitch,
            _ => t,
        };
        let clip = [l, t, w, h.max(pitch)];
        // On whole physical pixels of the display, as KOTOR 2 draws its
        // text at the display's own resolution.
        let grain = self.pixel;
        let snap = |at: f32| (at / grain).round() * grain;
        for (row, line) in lines.iter().enumerate() {
            let width = font.measure(line);
            let mut x = snap(match alignment & align::ACROSS {
                align::CENTRE => l + (w - width) / 2.0,
                align::RIGHT => l + w - width,
                _ => l,
            });
            let y = snap(top + row as f32 * pitch);
            for &byte in line {
                if let Some(glyph) = font.glyph(byte)
                    && glyph.width > 0.0
                {
                    let [u0, v0, u1, v1] = glyph.uv;
                    out.quads.push(Quad {
                        uv: [u0 + du, v0 + dv, u1 - du, v1 - dv],
                        clip: Some(clip),
                        ..Quad::image([x, y, glyph.width, glyph.height], id, color)
                    });
                }
                x += font.advance(byte);
            }
        }
    }

    /// The words a control shows (`words`), the shorter ones in turn
    /// (`ControlState::shorter`) where they wrap past its box.
    pub fn shown_words(&mut self, control: &Control, state: &ControlState) -> Vec<u8> {
        let words = self.words(control, state);
        let (Some(text), false) = (&control.text, state.shorter.is_empty()) else {
            return words;
        };
        let Some((_, font)) = self.textures.font(self.source, &text.font) else {
            return words;
        };
        let [_, _, width, height] = rect_of(control);
        let lines = ((height / font.line_height().max(1.0)).floor() as usize).max(1);
        let fits = |words: &[u8]| wrap(&font, words, width).len() <= lines;
        std::iter::once(words)
            .chain(state.shorter.iter().cloned())
            .reduce(|chosen, next| if fits(&chosen) { chosen } else { next })
            .unwrap_or(Vec::new())
    }

    /// The state's talk-table entry, its tokens filled in.
    fn told(&self, state: &ControlState) -> Option<Vec<u8>> {
        told(&*self.textures, state)
    }

    /// The words a control shows: the screen's, else its strref's, else
    /// the layout's own.
    pub(crate) fn words(&self, control: &Control, state: &ControlState) -> Vec<u8> {
        if let Some(text) = &state.text {
            return text.clone();
        }
        if let Some(words) = self.told(state) {
            return words;
        }
        let Some(text) = &control.text else {
            return Vec::new();
        };
        text.strref
            .and_then(|strref| self.textures.text(self.source, strref))
            .unwrap_or_else(|| text.text.clone())
    }

    fn text_color(
        &self,
        control: &Control,
        state: &ControlState,
        base: Option<[f32; 3]>,
    ) -> [f32; 4] {
        if state.disabled || state.greyed {
            return rgba(self.palette.disabled, 1.0);
        }
        let chosen = state.selected && control.kind != Kind::CheckBox;
        if state.focused || chosen {
            let lit = control.hilight.as_ref().is_some_and(has_textures);
            let alpha = if state.focused && !lit {
                self.pulse
            } else {
                1.0
            };
            let color = if state.refused {
                self.color(Some(RED))
            } else {
                self.palette.hilight
            };
            return rgba(color, alpha);
        }
        if state.refused {
            return rgba(self.color(Some(RED)), 1.0);
        }
        rgba(state.tint.unwrap_or_else(|| self.color(base)), 1.0)
    }

    /// One control as `state` says, without its children.
    pub fn control(&mut self, control: &Control, state: &ControlState, out: &mut DrawList) {
        if state.hidden {
            return;
        }
        let rect = rect_of(control);
        let border = if control.kind == Kind::CheckBox {
            match (state.selected, state.focused) {
                (true, true) => textured(&control.hilight_selected).or(textured(&control.selected)),
                (true, false) => textured(&control.selected),
                (false, true) => textured(&control.hilight),
                (false, false) => None,
            }
            .or(control.border.as_ref())
        } else {
            (state.focused || state.selected)
                .then(|| textured(&control.hilight))
                .flatten()
                .or(control.border.as_ref())
        };
        self.pulsed |= state.pulsing;
        let alpha = if state.pulsing { self.pulsing } else { 1.0 };
        if let Some(border) = border {
            let tint = if state.disabled || state.greyed {
                self.palette.disabled
            } else if state.refused {
                self.color(Some(RED))
            } else {
                state.tint.unwrap_or_else(|| self.color(border.color))
            };
            let mut border = border.clone();
            let (rect, tint) = match &control.text {
                // KOTOR 2's mark is white art in its row's words' colour.
                // A toggle with no words (the HUD's weapon swap, solo and
                // stealth buttons) is a picture over its whole box.
                Some(text) if control.kind == Kind::CheckBox && worded(text, state) => {
                    border.color.get_or_insert([1.0; 3]);
                    let color = self.text_color(control, state, text.color);
                    (self.check_mark(&border, rect), color)
                }
                _ => (rect, rgba(tint, alpha)),
            };
            self.frame(&border, state.fill.as_deref(), rect, tint, out);
        } else if let Some(fill) = &state.fill {
            self.image(Source::Campaign, fill, rect, rgba([1.0; 3], alpha), out);
        }
        if let Some(texture) = state
            .scene
            .as_ref()
            .and_then(|view| self.textures.scene(view))
        {
            let uv = match state.scene_size {
                Some(size) => {
                    out.sized.push((texture, size));
                    // The share of the image's width that the control's
                    // shape shows at the control's height, about its middle.
                    let shown = (rect[2] * size[1] / (rect[3] * size[0]).max(1.0)).min(1.0);
                    let left = (1.0 - shown) / 2.0;
                    [left, 0.0, left + shown, 1.0]
                }
                None => [0.0, 0.0, 1.0, 1.0],
            };
            out.quads.push(Quad {
                texture,
                uv,
                ..Quad::solid(rect, [1.0; 4])
            });
            if state.wide {
                out.wide.push(texture);
            }
        }
        match control.kind {
            Kind::ListBox => self.list(control, state, out),
            Kind::Progress => self.progress(control, state, out),
            Kind::Slider => self.thumb(control, state, out),
            _ => {}
        }
        if control.kind != Kind::ListBox
            && let Some(text) = &control.text
        {
            let words = self.shown_words(control, state);
            let color = self.text_color(control, state, text.color);
            let font = text.font.clone();
            let inner = control
                .border
                .as_ref()
                .map_or(rect, |border| inner(rect, self.dimension(border)));
            self.text(&font, &words, inner, text.alignment, color, out);
        }
    }

    /// Where a check box's mark goes in its row `rect`: at the row's left,
    /// centred down it, half the row tall and three quarters as wide as
    /// tall, the words across the row. KOTOR 2 at 1280 × 1024 draws
    /// `uibit_checkbxon` (a 32 × 18 pill on 32 × 32) 24 × 19 pixels in a
    /// 64-pixel row, nearly round; stretched over the row it is a wide
    /// ring. A mark with edges keeps the row.
    fn check_mark(&self, border: &Border, rect: [f32; 4]) -> [f32; 4] {
        let [l, t, w, h] = rect;
        if border.dimension > 0 && !border.edge.is_empty() {
            return rect;
        }
        let tall = h / 2.0;
        [l, t + (h - tall) / 2.0, (tall * 0.75).min(w), tall]
    }

    /// A slider's thumb, as thick as the slider, along it by its value: a
    /// wide slider's runs left to right, a tall one's (the character page's
    /// alignment) bottom to top, its image kept to its own shape.
    fn thumb(&mut self, control: &Control, state: &ControlState, out: &mut DrawList) {
        let Some(name) = control.thumb.as_ref().map(|thumb| thumb.image.clone()) else {
            return;
        };
        let Some(id) = self.texture(self.source, &name) else {
            return;
        };
        let [l, t, w, h] = rect_of(control);
        let (tw, th) = self.textures.size(id);
        let (tw, th) = (tw.max(1) as f32, th.max(1) as f32);
        let value = state.value.unwrap_or(0.0).clamp(0.0, 1.0);
        let rect = if h > w {
            let height = (w * th / tw).min(h);
            [l, t + (h - height) * (1.0 - value), w, height]
        } else {
            let width = (h * tw / th).min(w);
            [l + (w - width) * value, t, width, h]
        };
        out.quads.push(Quad::image(rect, id, [1.0; 4]));
    }

    fn progress(&mut self, control: &Control, state: &ControlState, out: &mut DrawList) {
        let Some((fill, color)) = control.progress.as_ref().map(|bar| {
            (
                state.bar.0.clone().unwrap_or_else(|| bar.fill.clone()),
                state.bar.1.unwrap_or_else(|| self.color(bar.color)),
            )
        }) else {
            return;
        };
        let share = state
            .value
            .or(control
                .value
                .map(|(current, maximum)| current as f32 / maximum.max(1) as f32))
            .unwrap_or(0.0)
            .clamp(0.0, 1.0);
        if share <= 0.0 {
            return;
        }
        let [l, t, w, h] = rect_of(control);
        let rect = if control.start_from_left == Some(false) {
            [l, t + h * (1.0 - share), w, h * share]
        } else {
            [l, t, w * share, h]
        };
        match state.tint {
            // Its art is coloured already: a tint over it only darkens it.
            Some(tint) => out.quads.push(Quad::solid(rect, rgba(tint, 1.0))),
            None => self.image(self.source, &fill, rect, rgba(color, 1.0), out),
        }
    }

    fn list(&mut self, list: &Control, state: &ControlState, out: &mut DrawList) {
        let told = state
            .list
            .is_none()
            .then(|| words_view(&*self.textures, state))
            .flatten();
        let (Some(proto), Some(view)) =
            (list.proto.as_deref(), state.list.as_ref().or(told.as_ref()))
        else {
            return;
        };
        let height = proto_height(proto, Some(view));
        let clip = rect_of(list);
        let mut top = -view.offset;
        let mut first = view.first;
        let room = list.extent.top + list.extent.height - proto.extent.top;
        let mut each = |row: &Row| {
            if view.fit {
                row_height(&mut *self.textures, self.source, proto, true, &row.text)
            } else {
                height
            }
        };
        if view.follow {
            let mut used = 0;
            first = view.rows.len();
            while first > 0 {
                let step = each(&view.rows[first - 1]) + list.padding;
                if used + step > room {
                    break;
                }
                used += step;
                first -= 1;
            }
        }
        let shown = match (view.fit, view.roll) {
            // Rising rows are cut where they leave the list.
            (true, Some(_)) => view.rows.len(),
            // Read words, scrolled by lines (`offset` into the first): from
            // the first, every row that starts inside the list, cut at its
            // foot.
            (true, None) if !view.pick => {
                let mut above = -view.offset;
                let starting = view.rows[first.min(view.rows.len())..]
                    .iter()
                    .take_while(|row| {
                        let starts = above < room;
                        above += each(row) + list.padding;
                        starts
                    })
                    .count();
                starting.max(1)
            }
            // From the first, the rows whole in the list (at least that one).
            (true, None) => {
                let mut used = 0;
                let whole = view.rows[first.min(view.rows.len())..]
                    .iter()
                    .take_while(|row| {
                        used += each(row) + list.padding;
                        used - list.padding <= room
                    })
                    .count();
                whole.max(1)
            }
            (false, _) => rows_that_fit(list, Some(view)),
        };
        for index in (first..view.rows.len()).take(shown) {
            let row = &view.rows[index];
            let mut placed = proto.clone();
            placed.extent.top += top;
            placed.extent.height = if view.fit {
                row_height(&mut *self.textures, self.source, proto, true, &row.text)
            } else {
                height
            };
            top += placed.extent.height + list.padding;
            // KOTOR 2 lights one row at most: the selected one, while its
            // list has the focus (the cursor, which hover and keys move and
            // the screen's selection follows); without the focus the row
            // stays chosen but is drawn plain.
            let row_state = ControlState {
                focused: view.focused == Some(index),
                disabled: state.disabled,
                selected: row.checked,
                greyed: row.greyed,
                text: row.told.is_none().then(|| row.text.clone()),
                strref: row.told,
                skin_strref: row.told.is_some(),
                refused: row.refused,
                ..ControlState::default()
            };
            let start = out.quads.len();
            if row.cells.is_empty() {
                self.row(&placed, row, &row_state, out);
            } else {
                let chosen = (view.selected == Some(index)).then_some(view.cell);
                let hover = view
                    .hover
                    .filter(|(at, _)| *at == index)
                    .map(|(_, cell)| cell);
                self.flow(&placed, &row.cells, (chosen, hover), out);
            }
            for quad in &mut out.quads[start..] {
                quad.clip = Some(quad.clip.map_or(clip, |inner| intersect(inner, clip)));
                if row.dimmed {
                    quad.color[3] *= 0.45;
                }
            }
        }
        if let Some(bar) = list.scrollbar.as_deref() {
            if view.fit && !view.pick && view.roll.is_none() {
                // Read words: the bar measures them in layout pixels.
                let heights: Vec<i32> = view
                    .rows
                    .iter()
                    .map(|row| {
                        row_height(&mut *self.textures, self.source, proto, true, &row.text)
                            + list.padding
                    })
                    .collect();
                let total = (heights.iter().sum::<i32>() - list.padding).max(0);
                let above: i32 = heights.iter().take(first).sum::<i32>() + view.offset;
                let last = (total - room).max(0);
                self.scrollbar(
                    bar,
                    total as usize,
                    room.max(0) as usize,
                    above.clamp(0, last) as usize,
                    out,
                );
            } else {
                self.scrollbar(bar, view.rows.len(), shown, view.first, out);
            }
        }
    }

    /// A list's row: a KOTOR 1 item row (`ProtoItem`) its frame and icon
    /// at its left; a KOTOR 2 row with an icon the icon in a square of the
    /// row's own border, beside it; any other the prototype.
    fn row(&mut self, proto: &Control, row: &Row, state: &ControlState, out: &mut DrawList) {
        self.row_body(proto, row, state, out);
        if let Some(badge) = &row.badge
            && let Some(id) = self.texture(Source::Campaign, badge)
        {
            let [l, t, w, h] = rect_of(proto);
            let (width, height) = self.textures.size(id);
            let tall = h * BADGE;
            let wide = tall * width as f32 / height.max(1) as f32;
            let rect = [l + w - wide - ROW_END, t + (h - tall) / 2.0, wide, tall];
            out.quads.push(Quad::image(rect, id, [1.0; 4]));
        }
        if let Some(value) = &row.value {
            // The row's own words and colours, at its other end, unframed.
            let mut right = proto.clone();
            right.border = None;
            right.hilight = None;
            right.selected = None;
            right.hilight_selected = None;
            right.extent.width -= ROW_END as i32;
            if let Some(text) = right.text.as_mut() {
                text.alignment = (text.alignment & align::DOWN) | align::RIGHT;
            }
            let words = ControlState {
                text: Some(value.clone()),
                ..state.clone()
            };
            self.control(&right, &words, out);
        }
    }

    fn row_body(&mut self, proto: &Control, row: &Row, state: &ControlState, out: &mut DrawList) {
        let item = proto.kind == Kind::ProtoItem;
        if !item && row.icon.is_none() && row.frame.is_none() {
            self.control(proto, state, out);
            return;
        }
        let [l, t, w, h] = rect_of(proto);
        let mut gap = 0;
        if !item {
            let mut square = proto.clone();
            square.extent.width = h as i32;
            let blank = ControlState {
                text: Some(Vec::new()),
                ..state.clone()
            };
            self.control(&square, &blank, out);
            gap = ROW_GAP;
        }
        let lit = proto.hilight.as_ref().filter(|border| has_textures(border));
        let tint = if state.refused {
            self.color(Some(RED))
        } else if state.focused || state.selected {
            lit.or(proto.border.as_ref())
                .map_or(self.palette.hilight, |border| self.color(border.color))
        } else {
            self.color(proto.border.as_ref().and_then(|border| border.color))
        };
        if let Some(frame) = &row.frame {
            self.image(Source::Campaign, frame, [l, t, h, h], rgba(tint, 1.0), out);
        }
        if let Some(icon) = &row.icon {
            self.image(Source::Campaign, icon, [l, t, h, h], [1.0; 4], out);
        }
        if let Some(count) = row.count.filter(|count| *count > 1) {
            let digits = count.to_string().into_bytes();
            let (narrow, wide, tall) = COUNT_BOX;
            let (width, alignment) = if digits.len() < 3 {
                (narrow, 34)
            } else {
                (wide, 36)
            };
            let color = rgba(self.palette.base, 1.0);
            self.text(
                COUNT_FONT,
                &digits,
                [l + h - width, t + h - tall, width, tall],
                alignment,
                color,
                out,
            );
        }
        let mut rest = proto.clone();
        rest.kind = Kind::Button;
        rest.extent.left += h as i32 + gap;
        rest.extent.width = (w - h) as i32 - gap;
        self.control(&rest, state, out);
    }

    /// A chain row as KOTOR 2's feat flow draws it: each feat's back tinted
    /// by its look (the one under the pointer pulsing, the chosen one
    /// outlined), its icon over it (faint until had or picked), an arrow
    /// before each but the first (none before one not open).
    fn flow(
        &mut self,
        proto: &Control,
        cells: &[Cell],
        (chosen, hover): (Option<usize>, Option<usize>),
        out: &mut DrawList,
    ) {
        let [l, t, ..] = rect_of(proto);
        let squares = flow_cells(l, t);
        let arrows = flow_arrows(l, t);
        for (at, cell) in cells.iter().enumerate().take(squares.len()) {
            let tint = match cell.look {
                CellLook::Known => skin::TSL_ENGINE.base,
                CellLook::Picked => skin::TSL_ENGINE.hilight,
                CellLook::Open => skin::TSL_ENGINE.disabled,
                CellLook::Closed => RED,
            };
            let alpha = if hover == Some(at) { self.pulse } else { 1.0 };
            let square = squares[at];
            let back = rgba(self.color(Some(tint)), alpha);
            self.image(self.source, FLOW_BACK, square, back, out);
            if let Some(icon) = &cell.icon {
                let side = square[2] * FLOW_ICON;
                let inset = (square[2] - side) / 2.0;
                let faint = matches!(cell.look, CellLook::Open | CellLook::Closed);
                let alpha = if faint { 0.25 } else { 1.0 };
                let rect = [square[0] + inset, square[1] + inset, side, side];
                self.image(Source::Campaign, icon, rect, [1.0, 1.0, 1.0, alpha], out);
            }
            if at > 0 && cell.look != CellLook::Closed {
                self.image(self.source, FLOW_ARROW_IMAGE, arrows[at - 1], [1.0; 4], out);
            }
            if chosen == Some(at) {
                let color = rgba(self.palette.hilight, 1.0);
                let [x, y, w, h] = square;
                let edge = FLOW_OUTLINE;
                for side in [
                    [x, y, w, edge],
                    [x, y + h - edge, w, edge],
                    [x, y, edge, h],
                    [x + w - edge, y, edge, h],
                ] {
                    out.quads.push(Quad::solid(side, color));
                }
            }
        }
    }

    fn scrollbar(
        &mut self,
        bar: &Control,
        count: usize,
        shown: usize,
        first: usize,
        out: &mut DrawList,
    ) {
        if count <= shown || shown == 0 {
            return;
        }
        let [l, t, w, h] = rect_of(bar);
        if let Some(arrow) = bar.dir.as_ref().map(|image| image.image.clone()) {
            if first > 0 {
                self.image(self.source, &arrow, [l, t, w, w], [1.0; 4], out);
            }
            if first + shown < count
                && let Some(id) = self.texture(self.source, &arrow)
            {
                out.quads.push(Quad {
                    uv: [0.0, 1.0, 1.0, 0.0],
                    ..Quad::image([l, t + h - w, w, w], id, [1.0; 4])
                });
            }
        }
        if let Some(thumb) = bar.thumb.as_ref().map(|image| image.image.clone()) {
            let (top, track, size) = thumb_track(bar, count, shown);
            let at = (track - size) * first as f32 / (count - shown) as f32;
            self.image(
                self.source,
                &thumb,
                [l + 2.0, top + at, w - 4.0, size],
                [1.0; 4],
                out,
            );
        }
    }

    /// Every control of `gui`, each as `states` says, parents before
    /// their children, later siblings over earlier ones, placed from the
    /// root's top left in its frame; the root's own border (a backdrop)
    /// first, as a panel tagged as the layout is.
    pub fn layout(
        &mut self,
        gui: &Gui,
        states: &dyn Fn(&Control) -> ControlState,
        out: &mut DrawList,
    ) {
        let (width, height) = gui.frame();
        out.size = (width as f32, height as f32);
        let start = out.quads.len();
        let root = Control {
            kind: Kind::Panel,
            tag: gui.tag.clone(),
            extent: fmt_odyssey::gui::Rect {
                left: 0,
                top: 0,
                ..gui.extent
            },
            border: gui.border.clone(),
            ..Control::default()
        };
        self.control(&root, &states(&root), out);
        for control in gui.walk() {
            let state = states(control);
            self.control(control, &state, out);
        }
        let (x, y) = (gui.extent.left as f32, gui.extent.top as f32);
        for quad in &mut out.quads[start..] {
            quad.rect[0] += x;
            quad.rect[1] += y;
            if let Some(clip) = &mut quad.clip {
                clip[0] += x;
                clip[1] += y;
            }
        }
    }
}

/// A feat flow row's squares from its top left: at its left, middle and
/// right, each 40 square, 2 below its top.
pub fn flow_cells(l: f32, t: f32) -> [[f32; 4]; 3] {
    let step = ((FLOW_WIDTH - FLOW_HEIGHT as f32) / 2.0).floor();
    [0.0, step, 2.0 * step].map(|x| [l + x, t + 2.0, FLOW_CELL, FLOW_CELL])
}

/// Its two arrows: centred in the gaps between the squares, as KOTOR 2
/// rounds them, and on the row's middle.
fn flow_arrows(l: f32, t: f32) -> [[f32; 4]; 2] {
    let (w, h) = (FLOW_WIDTH, FLOW_HEIGHT as f32);
    let half = FLOW_ARROW / 2.0;
    let top = t + (h - FLOW_ARROW) / 2.0;
    [((h + w) / 4.0).floor(), ((3.0 * w - h) / 4.0).floor()]
        .map(|x| [l + x - half, top, FLOW_ARROW, FLOW_ARROW])
}

/// The chain cell under `x` in a row of `cells` at the prototype's place:
/// the square it is over, else the first (KOTOR's hit check).
pub fn flow_cell_at(proto: &Control, x: f32, cells: usize) -> usize {
    let [l, t, ..] = rect_of(proto);
    flow_cells(l, t)
        .iter()
        .take(cells)
        .position(|square| x >= square[0] && x <= square[0] + square[2])
        .unwrap_or(0)
}

/// A list's scrollbar `bar` for `count` rows, `shown` of them in view:
/// where its thumb's track begins, how long it is, and the thumb's length
/// (layout pixels; the track lies between the arrows, a bar's width each).
pub(crate) fn thumb_track(bar: &Control, count: usize, shown: usize) -> (f32, f32, f32) {
    let [_, t, w, h] = rect_of(bar);
    let track = h - 2.0 * w - 4.0;
    (
        t + w + 2.0,
        track,
        track * shown as f32 / count.max(1) as f32,
    )
}

/// Where a bordered control's text goes, as KOTOR 2's executable places
/// it: `rect` less the border's dimension `dim` (layout pixels) on each
/// side, along each axis that has room for it (more than half a dimension
/// left), else the whole axis.
fn inner(rect: [f32; 4], dim: f32) -> [f32; 4] {
    let axis = |start: f32, size: f32| {
        let rest = size - 2.0 * dim;
        if rest > 0.0 && rest >= dim / 2.0 {
            (start + dim, rest)
        } else {
            (start, size)
        }
    };
    let ((left, width), (top, height)) = (axis(rect[0], rect[2]), axis(rect[1], rect[3]));
    [left, top, width, height]
}

fn intersect(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let left = a[0].max(b[0]);
    let top = a[1].max(b[1]);
    let right = (a[0] + a[2]).min(b[0] + b[2]);
    let bottom = (a[1] + a[3]).min(b[1] + b[3]);
    [left, top, (right - left).max(0.0), (bottom - top).max(0.0)]
}

/// How tall a list's row is: its prototype's height, and in a fitted list
/// at least its words wrapped within the prototype's border, as they are
/// drawn (`inner`), and that border (as drawn and as the pointer finds
/// rows).
pub fn row_height(
    textures: &mut dyn Textures,
    source: Source,
    proto: &Control,
    fit: bool,
    text: &[u8],
) -> i32 {
    let height = proto.extent.height;
    let font = proto.text.as_ref().map(|words| words.font.clone());
    let Some((_, font)) = font
        .filter(|_| fit)
        .and_then(|font| textures.font(source, &font))
    else {
        return height;
    };
    let rect = rect_of(proto);
    let [_, _, width, inner_height] = proto.border.as_ref().map_or(rect, |border| {
        inner(rect, border.dimension.max(0) as f32 * textures.pixel())
    });
    let lines = wrap(&font, text, width).len();
    let frame = rect[3] - inner_height;
    height.max((lines as f32 * font.line_height() + frame).ceil() as i32)
}

/// A talk-table entry's `words` with its `tokens` (`<CUSTOM0>`, by name)
/// filled in.
pub fn fill_tokens(mut words: Vec<u8>, tokens: &[(&'static str, Vec<u8>)]) -> Vec<u8> {
    for (token, value) in tokens {
        let token = format!("<{token}>");
        let mut from = 0;
        while let Some(at) = words[from..]
            .windows(token.len())
            .position(|window| window == token.as_bytes())
        {
            let at = from + at;
            words.splice(at..at + token.len(), value.iter().copied());
            from = at + value.len();
        }
    }
    words
}

/// `bytes` broken into lines no wider than `width`: on new lines, on
/// spaces between words, and inside a word too long for a line.
pub fn wrap(font: &asset_odyssey::font::Font, bytes: &[u8], width: f32) -> Vec<Vec<u8>> {
    let mut lines = Vec::new();
    for paragraph in bytes.split(|&byte| byte == b'\n') {
        let mut line: Vec<u8> = Vec::new();
        for word in paragraph.split(|&byte| byte == b' ') {
            let joined = if line.is_empty() {
                font.measure(word)
            } else {
                font.measure(&line) + font.advance(b' ') + font.measure(word)
            };
            if joined <= width {
                if !line.is_empty() {
                    line.push(b' ');
                }
                line.extend_from_slice(word);
                continue;
            }
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            for &byte in word {
                if !line.is_empty() && font.measure(&line) + font.advance(byte) > width {
                    lines.push(std::mem::take(&mut line));
                }
                line.push(byte);
            }
        }
        lines.push(line);
    }
    lines
}
