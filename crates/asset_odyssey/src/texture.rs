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
    /// The image has alpha to draw by (`Texture::has_alpha`): RGBA and DXT5
    /// (xoreos and reone; DXT1 is uploaded as RGB by both, so its one-bit
    /// alpha is dropped), with some texel less than opaque.
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
    /// A cube map's six faces (each a mip chain like `mips`), as stored;
    /// empty otherwise.
    pub faces: Vec<Vec<Vec<u8>>>,
    /// A flip-book's frames (each a mip chain like `mips`; the first is
    /// `mips`); empty otherwise.
    pub frame_mips: Vec<Vec<Vec<u8>>>,
}

impl TextureIr {
    /// Layer 0 of a parsed texture. `None` when a level does not decode.
    /// A loose `.txi` resource, when there is one, replaces the TPC's own
    /// (as xoreos does).
    pub fn from_texture(texture: &Texture, loose_txi: Option<&Txi>) -> Option<TextureIr> {
        let alpha = texture.has_alpha();
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
        // A cube map's faces go up as stored, in their stored order and
        // rows: so stored, every cube the games ship (the Upper City's
        // skyline, the Hawk's bay, the Endar Spire's halls, Onderon's) reads
        // as one seamless view of the game's world, Z up (+X, -X, +Y, -Y,
        // then the sky, then the ground), which is how the shader looks
        // them up.
        let mut faces = Vec::new();
        if texture.cube && texture.layers.len() == 6 {
            for index in 0..6 {
                faces.push(layer(index)?);
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

    /// The same texture with no side longer than `limit` texels, for a GPU
    /// that takes nothing larger. `None` when it already fits.
    ///
    /// A mod can ship an image larger than any GPU takes. The Swoop Monitor
    /// Replacement Pack's console screen is a sheet of 22 frames, 22528
    /// texels wide, and a GPU with a limit of 16384 refused it. That ended
    /// the game. The texture's largest levels are dropped first, as long as
    /// a smaller level follows. A texture left with one level that is still
    /// too large is decoded to RGBA8. It is then halved until it fits, each
    /// new texel the average of the two by two texels it covers. Every cube
    /// face and every flip-book frame is reduced in the same way.
    pub fn within(&self, limit: u32) -> Option<TextureIr> {
        let fits = |texture: &TextureIr| texture.width.max(texture.height) <= limit.max(1);
        if fits(self) {
            return None;
        }
        let mut fitted = self.clone();
        while !fits(&fitted) && fitted.mips.len() > 1 {
            fitted.each_chain(|chain| {
                chain.remove(0);
            });
            fitted.width = (fitted.width >> 1).max(1);
            fitted.height = (fitted.height >> 1).max(1);
        }
        let whole_blocks = fitted.width.is_multiple_of(4) && fitted.height.is_multiple_of(4);
        if fitted.encoding != Encoding::Rgba8 && (!fits(&fitted) || !whole_blocks) {
            fitted = fitted.decoded();
        }
        while !fits(&fitted) {
            let (width, height) = (fitted.width as usize, fitted.height as usize);
            fitted.each_chain(|chain| {
                chain.truncate(1);
                chain[0] = halved(&chain[0], width, height);
            });
            fitted.width = (fitted.width >> 1).max(1);
            fitted.height = (fitted.height >> 1).max(1);
        }
        Some(fitted)
    }

    /// `change` on the mip chain of the image, each cube face and each
    /// flip-book frame.
    fn each_chain(&mut self, mut change: impl FnMut(&mut Vec<Vec<u8>>)) {
        change(&mut self.mips);
        self.faces.iter_mut().for_each(&mut change);
        self.frame_mips.iter_mut().for_each(&mut change);
    }
}

/// An RGBA8 level of `width` × `height` at half its size (one texel at
/// least), each texel the average of the two by two it covers, rounded
/// down. A last odd row or column is left out.
pub fn halved(rgba: &[u8], width: usize, height: usize) -> Vec<u8> {
    let (half_width, half_height) = ((width / 2).max(1), (height / 2).max(1));
    let texel = |x: usize, y: usize| {
        let at = (y.min(height - 1) * width + x.min(width - 1)) * 4;
        &rgba[at..at + 4]
    };
    let mut out = Vec::with_capacity(half_width * half_height * 4);
    for y in 0..half_height {
        for x in 0..half_width {
            let quad = [
                texel(2 * x, 2 * y),
                texel(2 * x + 1, 2 * y),
                texel(2 * x, 2 * y + 1),
                texel(2 * x + 1, 2 * y + 1),
            ];
            for channel in 0..4 {
                let sum: u32 = quad.iter().map(|texel| u32::from(texel[channel])).sum();
                out.push((sum / 4) as u8);
            }
        }
    }
    out
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
