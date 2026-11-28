//! A render of the interface without a window (`kjk gui screen`, `kjk gui
//! story`, `kjk gui sweep --out`): its layers drawn into one picture, and
//! its control tree. The tree holds every control the render drew, where
//! it is in the window's pixels, its words with their body height and
//! colours, and where the focus moves. It holds no pixels and is the same
//! for the same state. docs/UI.md describes the format.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::assets::GuiAssets;
use crate::draw::{DrawList, Mark, Quad, TextureId, Words};
use crate::input::UiInput;
use crate::over_play::Shown;
use crate::paint::{Brush, ControlState, Palette};
use crate::raster::raster;
use crate::screen::{Back, FocusLink, Stack, layout_links};
use crate::skin::Source;
use crate::vm::ViewModel;

/// The tree's format, raised when a field changes meaning.
pub const FORMAT: u32 = 1;

/// The letters a font's body height is measured over: from the top of the
/// tallest to the bottom of the lowest.
const BODY: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
/// A pixel counts in a body height above this contrast with the
/// background (docs/UI-GUIDELINES.md), so that a dark outline on a dark
/// background does not.
const BODY_CONTRAST: f32 = 2.0;

/// The body heights measured, by the font's texture, its height, the
/// scale, the words' colour and the background measured on.
type Bodies = BTreeMap<(TextureId, u32, u32, [u32; 4], [u8; 3]), Option<u32>>;

/// A window of `window` pixels drawn from `layers`, bottom first, with
/// the focus and Back of the screen on top.
pub struct Render<'a> {
    assets: &'a GuiAssets,
    /// What was drawn: the game, and the screen and its steps, or the
    /// layout.
    shown: Value,
    window: (u32, u32),
    layers: Vec<Shown>,
    links: Vec<FocusLink>,
    focus: Option<String>,
    back: Option<Back>,
}

impl<'a> Render<'a> {
    /// `layers` drawn over the screens of `stack`, as `shown` names them.
    /// What Back does there is taken last, so the stack is left as Back
    /// left it.
    pub fn stacked(
        assets: &'a mut GuiAssets,
        (stack, vm, quiet): (&mut Stack, &ViewModel, &UiInput),
        layers: Vec<Shown>,
        shown: Value,
        window: (u32, u32),
    ) -> Render<'a> {
        let links = stack.focus_links(vm);
        let focus = stack.focused().map(str::to_string);
        let back = stack.back(assets, vm, quiet);
        Render {
            assets,
            shown,
            window,
            layers,
            links,
            focus,
            back: Some(back),
        }
    }

    /// `layers`, a frame of a stack whose focus moves by `links` and stands
    /// on `focus`, without what Back does.
    pub fn unstacked(
        assets: &'a GuiAssets,
        layers: Vec<Shown>,
        shown: Value,
        window: (u32, u32),
        (links, focus): (Vec<FocusLink>, Option<String>),
    ) -> Render<'a> {
        Render {
            assets,
            shown,
            window,
            layers,
            links,
            focus,
            back: None,
        }
    }

    /// The layout `name` at rest, every control as the layout has it, at
    /// its own size.
    pub fn at_rest(assets: &'a mut GuiAssets, name: &str) -> Result<Render<'a>, String> {
        let gui = assets.layout(name)?;
        let mut list = DrawList::default();
        let brush = Brush::new(Palette::of(assets.game), 1.0, true);
        let mut painter = brush.painter(assets, Source::Campaign);
        painter.layout(&gui, &|_| ControlState::default(), &mut list);
        let window = (list.size.0 as u32, list.size.1 as u32);
        let shown = json!({ "game": assets.game.key(), "layout": name });
        let layer = Shown {
            screen: name.to_string(),
            layout: name.to_string(),
            list,
        };
        Ok(Render {
            assets,
            shown,
            window,
            layers: vec![layer],
            links: layout_links(&gui),
            focus: None,
            back: None,
        })
    }

    pub fn window(&self) -> (u32, u32) {
        self.window
    }

    /// The layers drawn over black, as RGBA8, top row first.
    pub fn rgba(&self) -> Vec<u8> {
        self.drawn(|_| true)
    }

    /// The layers drawn without the quads `keep` refuses.
    fn drawn(&self, keep: impl Fn(&Quad) -> bool) -> Vec<u8> {
        let (width, height) = self.window;
        let mut image = vec![0u8; (width * height * 4) as usize];
        for layer in &self.layers {
            let mut list = layer.list.clone();
            list.quads.retain(&keep);
            let drawn = raster(
                &list,
                &|id| self.assets.pixels(id),
                width,
                height,
                [0, 0, 0, 0],
            );
            over(&mut image, &drawn);
        }
        image
    }

    /// The control tree.
    pub fn tree(&self) -> Value {
        let fonts: Vec<TextureId> = (self.layers.iter())
            .flat_map(|layer| &layer.list.marks)
            .flat_map(|mark| &mark.words)
            .map(|words| words.texture)
            .collect();
        let bare = self.drawn(|quad| !fonts.contains(&quad.texture));
        let mut bodies = BTreeMap::new();
        let count = self.layers.len();
        let window = self.window;
        let layers: Vec<Value> = (self.layers.iter())
            .enumerate()
            .map(|(at, layer)| {
                let placed = layer.list.placed((window.0 as f32, window.1 as f32));
                let links: &[_] = if at + 1 == count { &self.links } else { &[] };
                let controls: Vec<Value> = (layer.list.marks.iter())
                    .map(|mark| self.control(mark, placed, &bare, links, &mut bodies))
                    .collect();
                json!({
                    "screen": layer.screen,
                    "layout": layer.layout,
                    "scale": round(placed.0),
                    "controls": controls,
                })
            })
            .collect();
        let back = self.back.as_ref().map_or(Value::Null, |back| {
            let screens: Vec<&str> = back.screens.iter().map(|id| id.0).collect();
            let intents: Vec<String> = (back.intents.iter())
                .map(|intent| format!("{intent:?}"))
                .collect();
            json!({ "screens": screens, "intents": intents })
        });
        json!({
            "format": FORMAT,
            "shown": self.shown,
            "window": [window.0, window.1],
            "focus": self.focus,
            "back": back,
            "layers": layers,
        })
    }

    fn control(
        &self,
        mark: &Mark,
        placed: (f32, [f32; 2]),
        bare: &[u8],
        links: &[FocusLink],
        bodies: &mut Bodies,
    ) -> Value {
        let moves = links
            .iter()
            .find(|(tag, _)| *tag == mark.tag)
            .map(|(_, to)| json!({ "up": to[0], "down": to[1], "left": to[2], "right": to[3] }));
        let words: Vec<Value> = (mark.words.iter())
            .map(|words| {
                let rect = in_window(words.rect, placed);
                let behind = background(bare, self.window, rect);
                let ground = behind.map_or([0; 3], |behind| behind.most);
                let key = (
                    words.texture,
                    words.font.height.to_bits(),
                    placed.0.to_bits(),
                    words.color.map(f32::to_bits),
                    ground,
                );
                let body = *bodies
                    .entry(key)
                    .or_insert_with(|| body(self.assets, words, placed.0, ground));
                let background = behind.map(
                    |behind| json!({ "darkest": behind.darkest, "lightest": behind.lightest }),
                );
                json!({
                    "text": fmt_odyssey::code_page(&words.bytes),
                    "font": words.font_name,
                    "rect": rect.map(round),
                    "body_height": body,
                    "color": words.color.map(round),
                    "background": background,
                })
            })
            .collect();
        json!({
            "tag": mark.tag,
            "id": mark.id,
            "kind": format!("{:?}", mark.kind).to_lowercase(),
            "rect": in_window(mark.rect, placed).map(round),
            "focusable": moves.is_some(),
            "focused": mark.focused,
            "disabled": mark.disabled,
            "strref": mark.strref,
            "moves": moves,
            "words": words,
        })
    }
}

/// `top`, drawn over nothing (its colour already carries its alpha), laid
/// over `image`, which ends opaque.
pub fn over(image: &mut [u8], top: &[u8]) {
    for (pixel, drawn) in (image.as_chunks_mut::<4>().0.iter_mut()).zip(top.as_chunks::<4>().0) {
        let alpha = f32::from(drawn[3]) / 255.0;
        for k in 0..3 {
            pixel[k] = (f32::from(drawn[k]) + f32::from(pixel[k]) * (1.0 - alpha)).min(255.0) as u8;
        }
        pixel[3] = 255;
    }
}

/// `rect`, in layout pixels, in the window's pixels.
fn in_window(rect: [f32; 4], (scale, [x, y]): (f32, [f32; 2])) -> [f32; 4] {
    let [l, t, w, h] = rect;
    [x + l * scale, y + t * scale, w * scale, h * scale]
}

/// To a hundredth, without the noise of an `f32` written as JSON.
fn round(value: f32) -> f64 {
    (f64::from(value) * 100.0).round() / 100.0
}

/// The body height of `words`' font drawn in their colour, `scale` window
/// pixels to each of its layout pixels, over `ground`, in whole window
/// pixels. It is the rows from the top of the tallest letter to the bottom
/// of the lowest (`BODY`) that hold a pixel standing out from `ground`
/// (`ink_rows`), as the renderer draws them on a line that starts on a
/// whole pixel. None when the font's texture is not loaded or it has none
/// of those letters.
fn body(assets: &GuiAssets, words: &Words, scale: f32, ground: [u8; 3]) -> Option<u32> {
    let pixels = assets.pixels(words.texture)?;
    let (du, dv) = (
        0.5 / pixels.width.max(1) as f32,
        0.5 / pixels.height.max(1) as f32,
    );
    let mut list = DrawList::default();
    let mut x = 1.0;
    for &byte in BODY {
        if let Some(glyph) = (words.font.glyph(byte)).filter(|glyph| glyph.width > 0.0) {
            let [u0, v0, u1, v1] = glyph.uv;
            let rect = [x, 0.0, glyph.width * scale, glyph.height * scale];
            list.quads.push(Quad {
                uv: [u0 + du, v0 + dv, u1 - du, v1 - dv],
                ..Quad::image(rect, words.texture, words.color)
            });
            x += (glyph.width * scale).ceil() + 2.0;
        }
    }
    // A strip no taller than `draw::UI_HEIGHT`, so that it is drawn one
    // pixel to each of its own.
    let (width, height) = (
        x.ceil() as u32,
        (words.font.height * scale).ceil() as u32 + 1,
    );
    if list.quads.is_empty() || height > crate::draw::UI_HEIGHT as u32 {
        return None;
    }
    list.size = (width as f32, height as f32);
    let [r, g, b] = ground;
    let drawn = raster(
        &list,
        &|id| assets.pixels(id),
        width,
        height,
        [r, g, b, 255],
    );
    ink_rows(&drawn, width, ground)
}

/// The rows of `drawn` (RGBA8, `width` pixels wide) from the first to the
/// last that holds a pixel whose contrast with `ground` is above
/// `BODY_CONTRAST`.
pub(crate) fn ink_rows(drawn: &[u8], width: u32, ground: [u8; 3]) -> Option<u32> {
    let rows: Vec<bool> = (drawn.chunks((width * 4).max(1) as usize))
        .map(|row| {
            (row.chunks(4))
                .any(|pixel| contrast([pixel[0], pixel[1], pixel[2]], ground) > BODY_CONTRAST)
        })
        .collect();
    let first = rows.iter().position(|&inked| inked)?;
    let last = rows.iter().rposition(|&inked| inked)?;
    Some((last + 1 - first) as u32)
}

/// The contrast ratio of two colours, from 1 to 21, as WCAG 2 defines it.
pub(crate) fn contrast(a: [u8; 3], b: [u8; 3]) -> f32 {
    let luminance = |rgb: [u8; 3]| {
        let linear = |c: u8| {
            let c = f32::from(c) / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(rgb[0]) + 0.7152 * linear(rgb[1]) + 0.0722 * linear(rgb[2])
    };
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// The colours behind some words.
#[derive(Clone, Copy)]
struct Behind {
    darkest: [u8; 3],
    lightest: [u8; 3],
    /// The commonest, which the words' body height is measured on. The
    /// darkest or the lightest is often a border inside the words' box,
    /// drawn in the words' own colour.
    most: [u8; 3],
}

/// The darkest and the lightest colour of `image` inside `rect` (window
/// pixels), by luminance, and the commonest: what words there must stand
/// out against.
fn background(image: &[u8], window: (u32, u32), rect: [f32; 4]) -> Option<Behind> {
    let [l, t, w, h] = rect;
    let (x0, y0) = (l.max(0.0) as u32, t.max(0.0) as u32);
    let (x1, y1) = (
        ((l + w).min(window.0 as f32)) as u32,
        ((t + h).min(window.1 as f32)) as u32,
    );
    let luminance = |p: &[u8; 3]| {
        let [r, g, b] = crate::draw::LUMINANCE;
        r * f32::from(p[0]) + g * f32::from(p[1]) + b * f32::from(p[2])
    };
    let mut darkest: Option<[u8; 3]> = None;
    let mut lightest: Option<[u8; 3]> = None;
    let mut seen: BTreeMap<[u8; 3], u32> = BTreeMap::new();
    for y in y0..y1 {
        for x in x0..x1 {
            let at = ((y * window.0 + x) * 4) as usize;
            let pixel = [image[at], image[at + 1], image[at + 2]];
            *seen.entry(pixel).or_default() += 1;
            if darkest.is_none_or(|seen| luminance(&pixel) < luminance(&seen)) {
                darkest = Some(pixel);
            }
            if lightest.is_none_or(|seen| luminance(&pixel) > luminance(&seen)) {
                lightest = Some(pixel);
            }
        }
    }
    let most =
        (seen.into_iter()).max_by_key(|&(colour, count)| (count, std::cmp::Reverse(colour)))?;
    Some(Behind {
        darkest: darkest?,
        lightest: lightest?,
        most: most.0,
    })
}
