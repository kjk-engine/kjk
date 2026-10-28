//! TPC and TGA textures, decoded into layers of mip chains.
//!
//! TPC header (128 bytes): size of the first compressed image (0 when
//! uncompressed), a float kept as bits, width, height (u16), encoding, mip
//! count, then reserved bytes. Pixel data follows: each layer's mip chain in
//! turn. Text after the pixel data is the texture's TXI.
//!
//! Encodings: uncompressed 1 grey, 2 RGB, 4 RGBA (0x0c, swizzled BGRA, is an
//! Xbox layout and is refused); compressed 2 DXT1, 4 DXT5. A compressed image
//! six times as tall as wide is a cube map of six square faces. A TXI with
//! `proceduretype cycle` splits the image into `numx` × `numy` frames. Both
//! rules follow the xoreos reference, which reads these files more completely
//! than reone.
//!
//! TPC rows are stored bottom to top, like an uncompressed TGA without its
//! top-left flag, for DXT and raw data alike.

use crate::txi::Txi;
use crate::{FormatError, dxt};

const TPC: &str = "TPC";
const TGA: &str = "TGA";
const TPC_HEADER: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    Grey8,
    Rgb8,
    Rgba8,
    Dxt1,
    Dxt5,
}

impl PixelFormat {
    /// Bytes for one mip level, with the block formats' minimum of one block.
    pub fn size(self, width: usize, height: usize) -> usize {
        let blocks = width.div_ceil(4) * height.div_ceil(4);
        match self {
            PixelFormat::Grey8 => width * height,
            PixelFormat::Rgb8 => width * height * 3,
            PixelFormat::Rgba8 => width * height * 4,
            PixelFormat::Dxt1 => blocks.max(1) * 8,
            PixelFormat::Dxt5 => blocks.max(1) * 16,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Mip {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Texture {
    pub format: PixelFormat,
    /// One entry per layer: six faces for a cube map, frames for a
    /// flip-book, else one. Each is a mip chain, largest first.
    pub layers: Vec<Vec<Mip>>,
    pub cube: bool,
    pub txi: Txi,
    /// Rows run bottom to top in the stored data.
    pub bottom_up: bool,
    /// Mip levels the header claims but the data does not hold.
    pub missing_mips: usize,
    /// A TPC's declared size of its first compressed image, when it differs
    /// from the size its dimensions give.
    pub declared_size: Option<(usize, usize)>,
}

fn unsupported(format: &'static str, what: String) -> FormatError {
    FormatError::Unsupported { format, what }
}

impl Texture {
    pub fn width(&self) -> usize {
        self.layers[0][0].width
    }

    pub fn height(&self) -> usize {
        self.layers[0][0].height
    }

    /// One mip level as RGBA8, rows from the top.
    pub fn rgba(&self, layer: usize, mip: usize) -> Option<Vec<u8>> {
        let rgba = self.stored_rgba(layer, mip)?;
        if !self.bottom_up {
            return Some(rgba);
        }
        let row = self.layers[layer][mip].width * 4;
        Some(rgba.chunks_exact(row).rev().flatten().copied().collect())
    }

    /// One mip level as RGBA8, rows in stored order.
    pub fn stored_rgba(&self, layer: usize, mip: usize) -> Option<Vec<u8>> {
        let level = self.layers.get(layer)?.get(mip)?;
        let (w, h) = (level.width, level.height);
        Some(match self.format {
            PixelFormat::Dxt1 => dxt::decode(&level.data, w, h, false)?,
            PixelFormat::Dxt5 => dxt::decode(&level.data, w, h, true)?,
            PixelFormat::Rgba8 => level.data.get(..w * h * 4)?.to_vec(),
            PixelFormat::Rgb8 => level
                .data
                .get(..w * h * 3)?
                .as_chunks::<3>()
                .0
                .iter()
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect(),
            PixelFormat::Grey8 => level
                .data
                .get(..w * h)?
                .iter()
                .flat_map(|&g| [g, g, g, 255])
                .collect(),
        })
    }

    pub fn parse_tpc(bytes: &[u8]) -> Result<Texture, FormatError> {
        if bytes.len() < TPC_HEADER {
            return Err(FormatError::Truncated {
                format: TPC,
                needed: TPC_HEADER,
                len: bytes.len(),
            });
        }
        let u16_at = |at: usize| usize::from(u16::from_le_bytes([bytes[at], bytes[at + 1]]));
        let first_size = u32::from_le_bytes(bytes[0..4].try_into().expect("4 bytes")) as usize;
        let compressed = first_size != 0;
        let mut width = u16_at(8);
        let mut height = u16_at(10);
        let encoding = bytes[12];
        let mips = usize::from(bytes[13]).max(1);
        let format = match (compressed, encoding) {
            (false, 1) => PixelFormat::Grey8,
            (false, 2) => PixelFormat::Rgb8,
            (false, 4) => PixelFormat::Rgba8,
            (true, 2) => PixelFormat::Dxt1,
            (true, 4) => PixelFormat::Dxt5,
            (false, 0x0c) => return Err(unsupported(TPC, "swizzled BGRA (Xbox) encoding".into())),
            (compressed, other) => {
                return Err(unsupported(
                    TPC,
                    format!("encoding {other} (compressed: {compressed})"),
                ));
            }
        };
        if width == 0 || height == 0 {
            return Err(unsupported(TPC, format!("{width}x{height} image")));
        }
        let cube = compressed && height == width * 6;
        let mut layer_count = if cube { 6 } else { 1 };
        if cube {
            height = width;
        }

        // The TXI starts after the whole claimed pixel data.
        let chain = |w: usize, h: usize, first: Option<usize>| -> usize {
            (0..mips)
                .map(|level| {
                    let (lw, lh) = ((w >> level).max(1), (h >> level).max(1));
                    match (level, first) {
                        (0, Some(size)) => size,
                        _ => format.size(lw, lh),
                    }
                })
                .sum()
        };
        let pixel_bytes = chain(width, height, compressed.then_some(first_size)) * layer_count;
        let txi_at = (TPC_HEADER + pixel_bytes).min(bytes.len());
        let txi = Txi::parse(&bytes[txi_at..]);

        let mut first = compressed.then_some(first_size);
        // A flip-book stores each frame as its own full mip chain, and the
        // declared size covers all of them (measured on the games' files;
        // the header's mip count does not describe the frames).
        let mut frame_budget = None;
        if let Some((x, y)) = txi.cycle_frames() {
            let (x, y) = (x as usize, y as usize);
            if x > width || y > height {
                return Err(unsupported(
                    TPC,
                    format!("flip-book of {x}x{y} frames in a {width}x{height} image"),
                ));
            }
            layer_count = x * y;
            width /= x;
            height /= y;
            frame_budget = first.map(|total| total / layer_count);
            first = None;
        }

        let mut at = TPC_HEADER;
        let end = txi_at.max(TPC_HEADER + first_size.min(bytes.len() - TPC_HEADER));
        // Every layer holds at least one byte.
        let mut layers = Vec::with_capacity(layer_count.min(bytes.len()));
        let mut missing_mips = 0;
        for _ in 0..layer_count {
            let mut chain = Vec::with_capacity(mips);
            let frame_end = frame_budget.map(|budget| at + budget);
            let levels = if frame_budget.is_some() {
                usize::BITS as usize
            } else {
                mips
            };
            for level in 0..levels {
                let (w, h) = ((width >> level).max(1), (height >> level).max(1));
                let size = match (level, first) {
                    (0, Some(size)) => size.max(format.size(w, h)),
                    _ => format.size(w, h),
                };
                let limit = frame_end.unwrap_or(end).min(bytes.len());
                if at + size > limit {
                    if frame_budget.is_none() {
                        missing_mips += mips - level;
                    }
                    break;
                }
                chain.push(Mip {
                    width: w,
                    height: h,
                    data: bytes[at..at + size].to_vec(),
                });
                at += size;
                if (w, h) == (1, 1) {
                    break;
                }
            }
            if chain.is_empty() {
                return Err(FormatError::Truncated {
                    format: TPC,
                    needed: at + format.size(width, height),
                    len: bytes.len(),
                });
            }
            if let Some(frame_end) = frame_end
                && at != frame_end
            {
                return Err(unsupported(
                    TPC,
                    format!("flip-book frame ends at {at}, its share ends at {frame_end}"),
                ));
            }
            layers.push(chain);
        }
        let computed = format.size(width, height);
        let declared_size = first
            .filter(|&size| size != computed)
            .map(|size| (size, computed));
        Ok(Texture {
            format,
            layers,
            cube,
            txi,
            // Measured: font glyphs come out upside down when read top-down.
            bottom_up: true,
            missing_mips,
            declared_size,
        })
    }

    pub fn parse_tga(bytes: &[u8]) -> Result<Texture, FormatError> {
        if bytes.len() < 18 {
            return Err(FormatError::Truncated {
                format: TGA,
                needed: 18,
                len: bytes.len(),
            });
        }
        let id_len = usize::from(bytes[0]);
        let colour_map = bytes[1];
        let kind = bytes[2];
        let width = usize::from(u16::from_le_bytes([bytes[12], bytes[13]]));
        let height = usize::from(u16::from_le_bytes([bytes[14], bytes[15]]));
        let depth = bytes[16];
        let descriptor = bytes[17];
        if colour_map != 0 {
            return Err(unsupported(TGA, "colour-mapped image".into()));
        }
        let (format, pixel) = match (kind & !8, depth) {
            (2, 32) => (PixelFormat::Rgba8, 4),
            (2, 24) => (PixelFormat::Rgb8, 3),
            (3, 8) => (PixelFormat::Grey8, 1),
            (other, depth) => {
                return Err(unsupported(TGA, format!("type {other} at {depth} bits")));
            }
        };
        let rle = kind & 8 != 0;
        let mut at = 18 + id_len;
        let count = width * height;
        let truncated = |needed: usize| FormatError::Truncated {
            format: TGA,
            needed,
            len: bytes.len(),
        };
        // A run packet of 2 bytes or more expands to at most 128 pixels.
        let room = bytes.len().saturating_sub(at) * if rle { 128 } else { 1 };
        let mut raw = Vec::with_capacity((count * pixel).min(room));
        if rle {
            while raw.len() < count * pixel {
                let head = *bytes.get(at).ok_or(truncated(at + 1))?;
                at += 1;
                let run = usize::from(head & 0x7f) + 1;
                if head & 0x80 != 0 {
                    let value = bytes.get(at..at + pixel).ok_or(truncated(at + pixel))?;
                    for _ in 0..run {
                        raw.extend_from_slice(value);
                    }
                    at += pixel;
                } else {
                    let values = bytes
                        .get(at..at + run * pixel)
                        .ok_or(truncated(at + run * pixel))?;
                    raw.extend_from_slice(values);
                    at += run * pixel;
                }
            }
            raw.truncate(count * pixel);
        } else {
            raw.extend_from_slice(
                bytes
                    .get(at..at + count * pixel)
                    .ok_or(truncated(at + count * pixel))?,
            );
        }
        // Stored as BGR(A); turn into RGB(A).
        if pixel >= 3 {
            for chunk in raw.chunks_exact_mut(pixel) {
                chunk.swap(0, 2);
            }
        }
        Ok(Texture {
            format,
            layers: vec![vec![Mip {
                width,
                height,
                data: raw,
            }]],
            cube: false,
            txi: Txi::default(),
            bottom_up: descriptor & 0x20 == 0,
            missing_mips: 0,
            declared_size: None,
        })
    }
}
