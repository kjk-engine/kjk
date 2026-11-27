//! A render's control tree (`kjk gui screen`, `kjk gui sweep --out`): every
//! control the render drew, where it is in the window's pixels, its words
//! with their body height and colours, and where the focus moves. It is
//! written next to the PNG as JSON, holds no pixels, and is the same for
//! the same state. docs/UI.md describes the format.

use std::collections::BTreeMap;

use asset_odyssey::font::Font;
use serde_json::{Value, json};
use ui::assets::GuiAssets;
use ui::draw::{DrawList, Mark, Quad, TextureId};
use ui::raster::raster;
use ui::screen::FocusLink;

/// The format's version, raised when a field changes meaning.
pub const FORMAT: u32 = 1;

/// The letters a font's body height is measured over: from the top of the
/// tallest to the bottom of the lowest, as the Xbox Accessibility
/// Guidelines measure text (docs/UI-GUIDELINES.md).
const BODY: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
/// A pixel counts as ink from this alpha up.
const INK: u8 = 128;

/// One layer of a render: the screen, its layout and what it drew.
pub struct Layer<'a> {
    pub screen: &'a str,
    pub layout: &'a str,
    pub list: &'a DrawList,
}

/// The tree of a render of `window` pixels made of `layers` (bottom
/// first), of what `shown` names (the game, the screen and the steps taken
/// to it). `links` are the top layer's focus links, `focus` its focused
/// control, and `back` what Back does there.
pub fn tree(
    assets: &GuiAssets,
    shown: Value,
    layers: &[Layer<'_>],
    window: (u32, u32),
    (links, focus): (&[FocusLink], Option<&str>),
    back: Value,
) -> Value {
    let bare = bare_picture(assets, layers, window);
    let mut bodies = BTreeMap::new();
    let count = layers.len();
    let layers: Vec<Value> = layers
        .iter()
        .enumerate()
        .map(|(at, layer)| {
            let placed = layer.list.placed((window.0 as f32, window.1 as f32));
            let links: &[_] = if at + 1 == count { links } else { &[] };
            let controls: Vec<Value> = (layer.list.marks.iter())
                .map(|mark| control(assets, mark, placed, (&bare, window), links, &mut bodies))
                .collect();
            json!({
                "screen": layer.screen,
                "layout": layer.layout,
                "scale": round(placed.0),
                "controls": controls,
            })
        })
        .collect();
    json!({
        "format": FORMAT,
        "shown": shown,
        "window": [window.0, window.1],
        "focus": focus,
        "back": back,
        "layers": layers,
    })
}

fn control(
    assets: &GuiAssets,
    mark: &Mark,
    placed: (f32, [f32; 2]),
    (bare, window): (&[u8], (u32, u32)),
    links: &[FocusLink],
    bodies: &mut BTreeMap<(TextureId, u32, u32), Option<u32>>,
) -> Value {
    let moves = links
        .iter()
        .find(|(tag, _)| *tag == mark.tag)
        .map(|(_, to)| json!({ "up": to[0], "down": to[1], "left": to[2], "right": to[3] }));
    let words: Vec<Value> = (mark.words.iter())
        .map(|words| {
            let rect = in_window(words.rect, placed);
            let key = (
                words.texture,
                words.font.height.to_bits(),
                placed.0.to_bits(),
            );
            let body = *bodies
                .entry(key)
                .or_insert_with(|| body(assets, words.texture, &words.font, placed.0));
            json!({
                "text": fmt_odyssey::code_page(&words.bytes),
                "font": words.font_name,
                "rect": rect.map(round),
                "body_height": body,
                "color": words.color.map(round),
                "background": background(bare, window, rect),
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

/// `rect`, in layout pixels, in the window's pixels.
fn in_window(rect: [f32; 4], (scale, [x, y]): (f32, [f32; 2])) -> [f32; 4] {
    let [l, t, w, h] = rect;
    [x + l * scale, y + t * scale, w * scale, h * scale]
}

/// To a hundredth, without the noise of an `f32` written as JSON.
fn round(value: f32) -> f64 {
    (f64::from(value) * 100.0).round() / 100.0
}

/// The body height of `font` drawn `scale` window pixels to each of its
/// layout pixels, in whole window pixels: the rows from the top of its
/// tallest letter's ink to the bottom of its lowest's (`BODY`), as the
/// renderer draws them on a line that starts on a whole pixel. None when
/// its texture is not loaded or it has none of those letters.
fn body(assets: &GuiAssets, texture: TextureId, font: &Font, scale: f32) -> Option<u32> {
    let pixels = assets.pixels(texture)?;
    let (du, dv) = (
        0.5 / pixels.width.max(1) as f32,
        0.5 / pixels.height.max(1) as f32,
    );
    let mut list = DrawList::default();
    let mut x = 1.0;
    for &byte in BODY {
        if let Some(glyph) = font.glyph(byte).filter(|glyph| glyph.width > 0.0) {
            let [u0, v0, u1, v1] = glyph.uv;
            let rect = [x, 0.0, glyph.width * scale, glyph.height * scale];
            list.quads.push(Quad {
                uv: [u0 + du, v0 + dv, u1 - du, v1 - dv],
                ..Quad::image(rect, texture, [1.0; 4])
            });
            x += (glyph.width * scale).ceil() + 2.0;
        }
    }
    // A strip no taller than `draw::UI_HEIGHT`, so that it is drawn one
    // pixel to each of its own.
    let (width, height) = (x.ceil() as u32, (font.height * scale).ceil() as u32 + 1);
    if list.quads.is_empty() || height > ui::draw::UI_HEIGHT as u32 {
        return None;
    }
    list.size = (width as f32, height as f32);
    let drawn = raster(&list, &|id| assets.pixels(id), width, height, [0, 0, 0, 0]);
    let inked = |y: &u32| {
        let row = (*y * width * 4) as usize..((*y + 1) * width * 4) as usize;
        drawn[row].chunks(4).any(|pixel| pixel[3] >= INK)
    };
    let first = (0..height).find(inked)?;
    let last = (0..height).rev().find(inked)?;
    Some(last + 1 - first)
}

/// The render with no words drawn, its layers over black as the PNG has
/// them: what is behind each word.
fn bare_picture(assets: &GuiAssets, layers: &[Layer<'_>], window: (u32, u32)) -> Vec<u8> {
    let fonts: Vec<TextureId> = (layers.iter())
        .flat_map(|layer| &layer.list.marks)
        .flat_map(|mark| &mark.words)
        .map(|words| words.texture)
        .collect();
    let mut image = vec![0u8; (window.0 * window.1 * 4) as usize];
    for layer in layers {
        let mut list = layer.list.clone();
        list.quads.retain(|quad| !fonts.contains(&quad.texture));
        let drawn = raster(
            &list,
            &|id| assets.pixels(id),
            window.0,
            window.1,
            [0, 0, 0, 0],
        );
        over(&mut image, &drawn);
    }
    image
}

/// The darkest and the lightest colour of `image` inside `rect` (window
/// pixels), by luminance: what words there must stand out against.
fn background(image: &[u8], window: (u32, u32), rect: [f32; 4]) -> Value {
    let [l, t, w, h] = rect;
    let (x0, y0) = (l.max(0.0) as u32, t.max(0.0) as u32);
    let (x1, y1) = (
        ((l + w).min(window.0 as f32)) as u32,
        ((t + h).min(window.1 as f32)) as u32,
    );
    let luminance = |p: &[u8]| {
        let [r, g, b] = ui::draw::LUMINANCE;
        r * f32::from(p[0]) + g * f32::from(p[1]) + b * f32::from(p[2])
    };
    let mut darkest: Option<&[u8]> = None;
    let mut lightest: Option<&[u8]> = None;
    for y in y0..y1 {
        for x in x0..x1 {
            let at = ((y * window.0 + x) * 4) as usize;
            let pixel = &image[at..at + 3];
            if darkest.is_none_or(|seen| luminance(pixel) < luminance(seen)) {
                darkest = Some(pixel);
            }
            if lightest.is_none_or(|seen| luminance(pixel) > luminance(seen)) {
                lightest = Some(pixel);
            }
        }
    }
    match (darkest, lightest) {
        (Some(darkest), Some(lightest)) => json!({ "darkest": darkest, "lightest": lightest }),
        _ => Value::Null,
    }
}

/// `top`, drawn over nothing (its colour already carries its alpha), laid
/// over `image`, which ends opaque.
pub fn over(image: &mut [u8], top: &[u8]) {
    for (pixel, drawn) in image
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(top.as_chunks::<4>().0)
    {
        let alpha = f32::from(drawn[3]) / 255.0;
        for k in 0..3 {
            pixel[k] = (f32::from(drawn[k]) + f32::from(pixel[k]) * (1.0 - alpha)).min(255.0) as u8;
        }
        pixel[3] = 255;
    }
}
