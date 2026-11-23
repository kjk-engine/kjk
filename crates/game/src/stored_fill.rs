//! The fill that a bake's files leave to the player's own lightmaps.
//!
//! A room whose lamps cannot reach its level takes the rest from its stored
//! lightmap, times one number (`kjk detail-bake --fill-stored`). A module
//! may also ask that each texel shows at least a share of its stored light
//! (`--floor`). The bake's files hold none of the stored light, so that the
//! bakes can be given to players. The game reads the player's own stored
//! lightmaps when it loads a bake and adds the fill there.
//!
//! - A baked lightmap holds the lamps alone. `fill.txt` gives the number of
//!   each lightmap whose room takes the fill, and `floor.txt` the share
//!   (`add_stored_fill` in `viewer.rs`).
//! - A probe of `probes.txt` and `probes-sun.txt`, and a vertex of
//!   `placeables.txt`, holds its light without the fill. The word
//!   `@<map>:<texel>` after it names the stored texel that the bake read the
//!   fill from: a lightmap listed in the file's header (`StoredMap`) and a
//!   texel of the bake's grid on it. The game reads the stored lightmap
//!   there and adds the fill and the floor of that lightmap, as it does for
//!   the room's texels (`with_fill`, `with_fill_linear`).
//!
//! - A room with no walkable face, in a module baked with
//!   `--stored-backdrop`, shows its stored light as the game shows it
//!   today. The bake writes no PNG for its lightmaps. It names them in
//!   `stored.txt` (`STORED_FILE`), and the game makes their picture from
//!   the player's own stored lightmap (`backdrop`).
//!
//! The numbers are the bake's (`crates/launcher/src/detail_bake.rs` and
//! `detail_bake/gpu.rs`). A baked level is stored at 1 / `GAIN`. A level is
//! linear light to the power 1 / `GAMMA`, and a level above `KNEE` is eased
//! so that it never passes `KNEE + OVER` (`eased_level`).

use asset_odyssey::texture::{Encoding, TextureIr};
use bevy::math::Vec3;

pub const GAIN: f32 = 2.0;
pub const GAMMA: f32 = 2.2;
pub const KNEE: f32 = 0.8;
pub const OVER: f32 = 0.45;

/// The file of a bake's set `b` that names the lightmaps the game shows from
/// the player's own stored lightmaps at their stored light, one a line
/// (`backdrop`). A set without it has none.
pub const STORED_FILE: &str = "stored.txt";

/// The text of `STORED_FILE` for the lightmaps `names`.
pub fn stored_text<'a>(names: impl IntoIterator<Item = &'a str>) -> String {
    let mut text = String::from(
        "# The lightmaps of rooms with no walkable face. The game shows each one from the\n\
         # player's own stored lightmap at its stored light, and the bake holds no picture of it.\n",
    );
    for name in names {
        text.push_str(&name.to_ascii_lowercase());
        text.push('\n');
    }
    text
}

/// The lightmaps that `STORED_FILE` in the set `folder` names, in lower
/// case. Empty where the set has no such file.
pub fn stored_names(folder: &std::path::Path) -> std::collections::BTreeSet<String> {
    let Ok(text) = std::fs::read_to_string(folder.join(STORED_FILE)) else {
        return std::collections::BTreeSet::new();
    };
    (text.lines())
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_ascii_lowercase)
        .collect()
}

/// The picture a set's lightmap named in `STORED_FILE` stands for: the
/// stored lightmap's texels at 1 / `GAIN`, which the game multiplies back,
/// with an alpha of 1 (no share of a sun). These are the bytes the bake
/// wrote as a PNG before `STORED_FILE`, to the last bit, so the picture is
/// the same. Rows are in the stored order. `None` for a lightmap that does
/// not decode.
pub fn backdrop(stored: &TextureIr) -> Option<Vec<u8>> {
    let texels = StoredTexels::of(stored)?;
    let byte =
        |value: u8| ((f32::from(value) / 255.0 / GAIN).clamp(0.0, 1.0) * 255.0).round() as u8;
    Some(
        (texels.rgba.as_chunks::<4>().0.iter())
            .take(texels.width * texels.height)
            .flat_map(|[r, g, b, _]| [byte(*r), byte(*g), byte(*b), 255])
            .collect(),
    )
}

/// The word that names the same stored texel as the vertex before it on a
/// line of `placeables.txt`. Half of 411dxn's vertices name the same texel
/// as the one before, and the short word keeps the file about 70 KB
/// smaller.
pub const SAME: &str = "@";

/// The word that names texel `texel` of the lightmap numbered `map` in a
/// file's header.
pub fn word(map: usize, texel: usize) -> String {
    format!("@{map}:{texel}")
}

/// The lightmap's number and the texel of a word that `word` wrote. `None`
/// for any other word.
pub fn parse_word(word: &str) -> Option<(usize, usize)> {
    let (map, texel) = word.strip_prefix('@')?.split_once(':')?;
    Some((map.parse().ok()?, texel.parse().ok()?))
}

/// The header line that names a lightmap: its name, and the width and
/// height of the bake's grid of texels on it.
pub fn map_line(name: &str, width: usize, height: usize) -> String {
    format!("map {name} {width} {height}")
}

/// A lightmap named in a bake's file (`map_line`).
#[derive(Debug, Clone, PartialEq)]
pub struct StoredMap {
    /// The lightmap's name in lower case.
    pub name: String,
    pub width: usize,
    pub height: usize,
}

impl StoredMap {
    /// The lightmap of a line that `map_line` wrote.
    pub fn parse(line: &str) -> Option<StoredMap> {
        let mut words = line.strip_prefix("map ")?.split_whitespace();
        let name = words.next()?.to_ascii_lowercase();
        let width = words.next()?.parse().ok()?;
        let height = words.next()?.parse().ok()?;
        Some(StoredMap {
            name,
            width,
            height,
        })
    }

    /// The middle of texel `texel` of the bake's grid, as a lightmap's
    /// coordinates from 0 to 1. `None` for a texel off the grid.
    pub fn uv(&self, texel: usize) -> Option<(f32, f32)> {
        (self.width > 0 && texel < self.width * self.height).then(|| {
            (
                ((texel % self.width) as f32 + 0.5) / self.width as f32,
                ((texel / self.width) as f32 + 0.5) / self.height as f32,
            )
        })
    }
}

/// A stored lightmap's texels, read between texels as the bake reads them
/// (`Stored::sample` in `crates/launcher/src/detail_bake.rs`).
pub struct StoredTexels {
    rgba: Vec<u8>,
    width: usize,
    height: usize,
    clamp: bool,
}

impl StoredTexels {
    /// The texels of `stored`, decoded. `None` for a lightmap that does not
    /// decode.
    pub fn of(stored: &TextureIr) -> Option<StoredTexels> {
        let (width, height) = (stored.width as usize, stored.height as usize);
        let data = stored.mips.first()?;
        let rgba = match stored.encoding {
            Encoding::Rgba8 => (data.len() >= width * height * 4).then(|| data.clone()),
            Encoding::Bc1 | Encoding::Bc1Holes => {
                fmt_odyssey::dxt::decode(data, width, height, false)
            }
            Encoding::Bc3 => fmt_odyssey::dxt::decode(data, width, height, true),
        };
        let rgba =
            rgba.filter(|rgba| width > 0 && height > 0 && rgba.len() >= width * height * 4)?;
        Some(StoredTexels {
            rgba,
            width,
            height,
            clamp: stored.facts.clamp,
        })
    }

    /// The stored level at `u`, `v`, from 0 to 1, read between the four
    /// nearest texels.
    pub fn at(&self, u: f32, v: f32) -> Vec3 {
        let texel = |tx: i64, ty: i64| {
            let (w, h) = (self.width as i64, self.height as i64);
            let (tx, ty) = if self.clamp {
                (tx.clamp(0, w - 1), ty.clamp(0, h - 1))
            } else {
                (tx.rem_euclid(w), ty.rem_euclid(h))
            };
            let at = (ty * w + tx) as usize * 4;
            Vec3::new(
                f32::from(self.rgba[at]),
                f32::from(self.rgba[at + 1]),
                f32::from(self.rgba[at + 2]),
            ) / 255.0
        };
        let sx = u * self.width as f32 - 0.5;
        let sy = v * self.height as f32 - 0.5;
        let (x0, y0) = (sx.floor(), sy.floor());
        let (fx, fy) = (sx - x0, sy - y0);
        let (x0, y0) = (x0 as i64, y0 as i64);
        let top = texel(x0, y0).lerp(texel(x0 + 1, y0), fx);
        let bottom = texel(x0, y0 + 1).lerp(texel(x0 + 1, y0 + 1), fx);
        top.lerp(bottom, fy)
    }
}

fn luma(c: Vec3) -> f32 {
    0.2126 * c.x + 0.7152 * c.y + 0.0722 * c.z
}

/// The linear light of a whole level (not over `GAIN`), with the bake's
/// easing undone.
pub fn linear_of(mut level: Vec3) -> Vec3 {
    let brightness = luma(level);
    if brightness > KNEE {
        let eased = (brightness - KNEE).min(OVER * 0.999);
        level *= (KNEE + eased * OVER / (OVER - eased)) / brightness;
    }
    level.powf(GAMMA)
}

/// The whole level of linear light `light`, eased as the bake eases it.
pub fn level_of(light: Vec3) -> Vec3 {
    let mut level = light.max(Vec3::ZERO).powf(1.0 / GAMMA);
    let brightness = luma(level);
    if brightness > KNEE {
        let over = brightness - KNEE;
        level *= (KNEE + over * OVER / (OVER + over)) / brightness;
    }
    level
}

/// Linear light `light` with the fill of stored level `stored`: `fill`
/// times its linear light added, and at least `floor` times it.
pub fn with_fill_linear(light: Vec3, stored: Vec3, fill: Option<f32>, floor: Option<f32>) -> Vec3 {
    let stored = stored.powf(GAMMA);
    let mut light = light;
    if let Some(fill) = fill {
        light += stored * fill;
    }
    if let Some(floor) = floor {
        light = light.max(stored * floor);
    }
    light
}

/// A level as a bake's file stores it, at 1 / `GAIN`, with the fill of
/// stored level `stored` (`with_fill_linear`), stored so again.
pub fn with_fill(level: Vec3, stored: Vec3, fill: Option<f32>, floor: Option<f32>) -> Vec3 {
    let light = linear_of(level * GAIN);
    level_of(with_fill_linear(light, stored, fill, floor)) / GAIN
}

#[cfg(feature = "probes")]
pub mod probes;
