//! Textures ready for upload: mip chains in stored row order, as DXT
//! blocks where the GPU can sample them as they are, else RGBA8.
//!
//! Rows are kept as the file stores them (bottom row first for TPC). The
//! games handed the stored rows to Direct3D, where the first row is v = 0,
//! so uploading the same rows and keeping the models' UVs as they are
//! samples the same texels the games did. DXT blocks are stored in that
//! same row order, so they upload unchanged.

use fmt_odyssey::dxt;
use fmt_odyssey::texture::{Mip, PixelFormat, Texture};
use fmt_odyssey::txi::Txi;

/// TXI `blending`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Blending {
    #[default]
    Default,
    Additive,
    PunchThrough,
}

/// What a texture tells the material, from its format and TXI.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TextureFacts {
    /// The format carries alpha: RGBA and DXT5 (xoreos and reone). DXT1 is
    /// uploaded as RGB by both, so its one-bit alpha is dropped.
    pub alpha: bool,
    pub blending: Blending,
    pub decal: bool,
    /// Drawn from both sides though not a decal (a JA shader's `cull
    /// none`); KOTOR's TXI has no such word.
    pub two_sided: bool,
    pub alpha_mean: Option<f32>,
    /// `envmaptexture`, else `bumpyshinytexture` (xoreos reads both and the
    /// env map wins).
    pub envmap: Option<String>,
    /// `bumpmaptexture`: its alpha is then a mask, not coverage (reone
    /// `MeshSceneNode::isTransparent`).
    pub bumpmap: Option<String>,
    /// A `blending` value that is not one of the three spellings xoreos
    /// matches exactly (it compares case-sensitively).
    pub odd_blending: Option<String>,
    /// A flip-book: `proceduretype cycle`, with `numx × numy` frames at
    /// `fps`.
    pub cycle: Option<(u32, f32)>,
    /// A JA shader's animated map (`animMap`, `oneShotAnimMap`): its
    /// frames stacked down the texture (`FrameStrip`).
    pub strip: Option<FrameStrip>,
    /// Sampled clamped to its edges rather than repeated: a JA shader's
    /// `clampmap` or `clampAnimMap` (`GL_CLAMP`).
    pub clamp: bool,
}

/// An animated map's frames, stacked top to bottom in one texture: how
/// many, how many a second, and whether it plays once and holds its last
/// (`oneShotAnimMap`) rather than looping.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameStrip {
    pub frames: u32,
    pub fps: f32,
    pub once: bool,
}

impl FrameStrip {
    /// The frame showing `seconds` into its time (as JA's
    /// renderer picks it): none before it starts, a one-shot's last
    /// once it is over, else round again.
    pub fn frame(&self, seconds: f32) -> u32 {
        let index = (seconds * self.fps).floor().max(0.0) as u32;
        if self.once {
            index.min(self.frames.saturating_sub(1))
        } else {
            index % self.frames.max(1)
        }
    }
}

impl TextureFacts {
    pub fn from_txi(txi: &Txi, alpha: bool) -> TextureFacts {
        let joined = |keyword: &str| txi.get(keyword).map(|values| values.join(" "));
        let (blending, odd_blending) = match joined("blending").as_deref() {
            None | Some("default") => (Blending::Default, None),
            Some("additive") => (Blending::Additive, None),
            Some("punchthrough") => (Blending::PunchThrough, None),
            Some(other) => (Blending::Default, Some(other.to_string())),
        };
        // A bool as xoreos reads it: "1", or true/yes/y/on in any case.
        let decal = joined("decal").is_some_and(|value| {
            value == "1"
                || ["true", "yes", "y", "on"]
                    .iter()
                    .any(|word| value.eq_ignore_ascii_case(word))
        });
        let name = |keyword: &str| {
            joined(keyword)
                .map(|value| value.trim().to_ascii_lowercase())
                .filter(|value| !value.is_empty())
        };
        TextureFacts {
            alpha,
            blending,
            decal,
            two_sided: false,
            alpha_mean: txi.number("alphamean"),
            envmap: name("envmaptexture").or_else(|| name("bumpyshinytexture")),
            bumpmap: name("bumpmaptexture"),
            odd_blending,
            cycle: txi
                .cycle_frames()
                .and_then(|(x, y)| Some((x.checked_mul(y)?, txi.number("fps")?))),
            strip: None,
            clamp: false,
        }
    }
}

/// How a `TextureIr`'s levels are encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Rgba8,
    /// DXT1 blocks as stored (BC1), none using its transparent texel.
    Bc1,
    /// DXT1 blocks as stored, some using their transparent texel: the games
    /// drew DXT1 as RGB, so that texel is opaque black, as only a sampler
    /// that takes alpha as 1 sees it (the scene's material does; other
    /// uses decode them).
    Bc1Holes,
    /// DXT5 blocks as stored (BC3).
    Bc3,
}

#[derive(Debug, Clone)]
pub struct TextureIr {
    pub width: u32,
    pub height: u32,
    pub encoding: Encoding,
    /// Largest first; each in `encoding`, rows in stored order.
    pub mips: Vec<Vec<u8>>,
    pub cube: bool,
    pub frames: usize,
    pub facts: TextureFacts,
    /// A cube map's six faces (each a mip chain like `mips`), after the
    /// face fix-up; empty otherwise.
    pub faces: Vec<Vec<Vec<u8>>>,
    /// A flip-book's frames (each a mip chain like `mips`; the first is
    /// `mips`); empty otherwise.
    pub frame_mips: Vec<Vec<Vec<u8>>>,
}

/// Quarter turns for each cube map face, counted after faces 0 and 1 have
/// swapped places: the games store the faces in this orientation.
const CUBE_TURNS: [usize; 6] = [1, 3, 0, 2, 2, 0];

/// Turns a square `n` by `n` RGBA8 image a quarter turn `steps` times
/// (modulo 4), in place. Each turn sends the texel at row `r`, column `c`
/// to row `c`, column `n - 1 - r`.
fn rotate90(data: &mut [u8], n: usize, steps: usize) {
    let mut scratch = vec![0u8; data.len()];
    for _ in 0..steps % 4 {
        for r in 0..n {
            for c in 0..n {
                let from = (r * n + c) * 4;
                let to = (c * n + (n - 1 - r)) * 4;
                scratch[to..to + 4].copy_from_slice(&data[from..from + 4]);
            }
        }
        data.copy_from_slice(&scratch);
    }
}

impl TextureIr {
    /// Layer 0 of a parsed texture. `None` when a level does not decode.
    /// A loose `.txi` resource, when there is one, replaces the TPC's own
    /// (as xoreos does).
    pub fn from_texture(texture: &Texture, loose_txi: Option<&Txi>) -> Option<TextureIr> {
        let alpha = matches!(texture.format, PixelFormat::Rgba8 | PixelFormat::Dxt5);
        let encoding = encoding(texture);
        let layer = |index: usize| -> Option<Vec<Vec<u8>>> {
            let mut mips = Vec::new();
            for (level, mip) in texture.layers.get(index)?.iter().enumerate() {
                if encoding != Encoding::Rgba8 {
                    mips.push(blocks(texture.format, mip)?.to_vec());
                    continue;
                }
                let mut rgba = texture.stored_rgba(index, level)?;
                if !alpha {
                    for pixel in rgba.as_chunks_mut::<4>().0 {
                        pixel[3] = 255;
                    }
                }
                mips.push(rgba);
            }
            Some(mips)
        };
        let mips = layer(0)?;
        let mut faces = Vec::new();
        if texture.cube && texture.layers.len() == 6 {
            for index in 0..6 {
                faces.push(layer(index)?);
            }
            faces.swap(0, 1);
            for (face, turns) in faces.iter_mut().zip(CUBE_TURNS) {
                for (level, data) in face.iter_mut().enumerate() {
                    let n = texture.layers[0][level].width;
                    if n != texture.layers[0][level].height {
                        return None;
                    }
                    rotate90(data, n, turns);
                }
            }
        }
        let mut frame_mips = Vec::new();
        if !texture.cube && texture.layers.len() > 1 {
            for index in 0..texture.layers.len() {
                frame_mips.push(layer(index)?);
            }
        }
        Some(TextureIr {
            faces,
            frame_mips,
            width: texture.width() as u32,
            height: texture.height() as u32,
            encoding,
            mips,
            cube: texture.cube,
            frames: if texture.cube {
                1
            } else {
                texture.layers.len()
            },
            facts: TextureFacts::from_txi(loose_txi.unwrap_or(&texture.txi), alpha),
        })
    }

    /// The same texture with its levels as RGBA8, for a GPU without BC
    /// formats.
    pub fn decoded(&self) -> TextureIr {
        let dxt5 = match self.encoding {
            Encoding::Rgba8 => return self.clone(),
            Encoding::Bc1 | Encoding::Bc1Holes => false,
            Encoding::Bc3 => true,
        };
        let chain = |mips: &Vec<Vec<u8>>| -> Vec<Vec<u8>> {
            mips.iter()
                .enumerate()
                .map(|(level, data)| {
                    let width = (self.width as usize >> level).max(1);
                    let height = (self.height as usize >> level).max(1);
                    let mut rgba = dxt::decode(data, width, height, dxt5)
                        .expect("block levels are sized when they are kept");
                    // DXT1 has no alpha: a transparent texel is opaque black.
                    if !dxt5 {
                        for pixel in rgba.as_chunks_mut::<4>().0 {
                            pixel[3] = 255;
                        }
                    }
                    rgba
                })
                .collect()
        };
        TextureIr {
            encoding: Encoding::Rgba8,
            mips: chain(&self.mips),
            frame_mips: self.frame_mips.iter().map(chain).collect(),
            faces: self.faces.clone(),
            facts: self.facts.clone(),
            ..*self
        }
    }
}

/// A level's blocks: the first level's data can run past them when the
/// TPC declares a larger first image.
fn blocks(format: PixelFormat, mip: &Mip) -> Option<&[u8]> {
    mip.data.get(..format.size(mip.width, mip.height))
}

/// DXT blocks go to the GPU as they are where it samples them as the games
/// did. Not for a cube map (its faces are turned texel by texel), nor a
/// size that is not whole blocks (wgpu asks it of the first level). A DXT1
/// with a transparent texel is marked (`Bc1Holes`): the games drew DXT1 as
/// RGB.
fn encoding(texture: &Texture) -> Encoding {
    let whole_blocks = texture.width().is_multiple_of(4) && texture.height().is_multiple_of(4);
    let every_level = |keep: fn(&[u8]) -> bool| {
        let mut levels = texture.layers.iter().flatten();
        levels.all(|mip| blocks(texture.format, mip).is_some_and(keep))
    };
    match texture.format {
        _ if texture.cube || !whole_blocks => Encoding::Rgba8,
        PixelFormat::Dxt1 if every_level(|data| !dxt::has_holes(data)) => Encoding::Bc1,
        PixelFormat::Dxt1 if every_level(|_| true) => Encoding::Bc1Holes,
        PixelFormat::Dxt5 if every_level(|_| true) => Encoding::Bc3,
        _ => Encoding::Rgba8,
    }
}
