//! TPC and TGA textures, decoded into layers of mip chains.
//!
//! TPC header (128 bytes): size of the first compressed image (0 when
//! uncompressed), a float kept as bits, width, height (u16), encoding, mip
//! count, then reserved bytes. Pixel data follows: each layer's mip chain in
//! turn. Text after the pixel data is the texture's TXI.
//!
//! Encodings: uncompressed 1 grey, 2 RGB, 4 RGBA (0x0c, swizzled BGRA, is an
//! Xbox layout and is refused); compressed 2 DXT1, 4 DXT5. An image six
//! times as tall as wide is a cube map of six square faces. A TXI with
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

    /// Whether it has alpha to draw by: an RGBA or DXT5 image (DXT1 is drawn
    /// as RGB) with some texel of its first level less than opaque. One
    /// saved with an alpha channel it leaves opaque throughout draws as one
    /// without, so it is drawn so.
    pub fn has_alpha(&self) -> bool {
        let mut first = self.layers.iter().filter_map(|chain| chain.first());
        match self.format {
            PixelFormat::Rgba8 => first.any(|mip| {
                mip.data
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|texel| texel[3] < 255)
            }),
            PixelFormat::Dxt5 => first.any(|mip| dxt::dxt5_see_through(&mip.data)),
            _ => false,
        }
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

    /// A TPC; one whose TXI's flip-book frames do not fit its data (a mod's
    /// TXI written for another image) is read as the one image it holds.
    pub fn parse_tpc(bytes: &[u8]) -> Result<Texture, FormatError> {
        Texture::tpc(bytes, true).or_else(|_| Texture::tpc(bytes, false))
    }

    fn tpc(bytes: &[u8], flip_book: bool) -> Result<Texture, FormatError> {
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
        let cube = height == width * 6;
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
        if let Some((x, y)) = txi.cycle_frames().filter(|_| flip_book) {
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
        Texture::parse_tga_layers(bytes, 1)
    }

    /// A TGA flip-book: the image is a sheet of `columns` × `rows` equal
    /// frames, which its TXI's `proceduretype cycle` asks for with `numx`
    /// and `numy`. A TPC flip-book stores each frame as its own image; a
    /// TGA keeps them side by side in one picture, and each frame becomes a
    /// layer here, as a TPC's frames are. The frames are numbered as the
    /// sheet is read, left to right along each row of frames, from the top
    /// row down. Each frame's rows run bottom to top, as the whole image's
    /// do. A sheet that does not divide into whole frames is refused.
    pub fn parse_tga_frames(
        bytes: &[u8],
        columns: usize,
        rows: usize,
    ) -> Result<Texture, FormatError> {
        let mut sheet = Texture::parse_tga(bytes)?;
        let (width, height) = (sheet.width(), sheet.height());
        if columns == 0
            || rows == 0
            || !width.is_multiple_of(columns)
            || !height.is_multiple_of(rows)
        {
            return Err(unsupported(
                TGA,
                format!("flip-book of {columns}x{rows} frames in a {width}x{height} image"),
            ));
        }
        let texel = sheet.format.size(1, 1);
        let (frame_width, frame_height) = (width / columns, height / rows);
        let image = &sheet.layers[0][0].data;
        let frame = |index: usize| {
            let (column, row_from_top) = (index % columns, index / columns);
            // The image's rows run bottom to top: the top row of frames is
            // the last band of rows.
            let first_row = (rows - 1 - row_from_top) * frame_height;
            let data = (first_row..first_row + frame_height)
                .flat_map(|y| {
                    let start = (y * width + column * frame_width) * texel;
                    image[start..start + frame_width * texel].iter().copied()
                })
                .collect();
            vec![Mip {
                width: frame_width,
                height: frame_height,
                data,
            }]
        };
        sheet.layers = (0..columns * rows).map(frame).collect();
        sheet.cube = false;
        Ok(sheet)
    }

    /// A TGA whose image stacks `layers` equal layers in its stored order (a
    /// cube map's six faces, which its TXI's `cube` asks for, as xoreos
    /// splits it). Every layout a TGA writer produces is read: true colour
    /// at 15, 16, 24 or 32 bits, grey at 8 or 16 (with alpha), colour-mapped
    /// with any palette depth, each raw or run-length encoded, from any
    /// corner; a colour map on an image that does not use it is skipped.
    /// Rows come out bottom to top, as a TPC stores them, whatever corner the
    /// file starts from (xoreos turns a top-down file over the same way).
    pub fn parse_tga_layers(bytes: &[u8], layers: usize) -> Result<Texture, FormatError> {
        let truncated = |needed: usize| FormatError::Truncated {
            format: TGA,
            needed,
            len: bytes.len(),
        };
        let header = bytes.get(..18).ok_or(truncated(18))?;
        let u16_at = |at: usize| usize::from(u16::from_le_bytes([header[at], header[at + 1]]));
        let (id_len, mapped, kind) = (usize::from(header[0]), header[1] != 0, header[2]);
        let (map_first, map_len, map_depth) = (u16_at(3), u16_at(5), header[7]);
        let (width, height, depth, descriptor) = (u16_at(12), u16_at(14), header[16], header[17]);
        let rle = kind & 8 != 0;
        let mut at = 18 + id_len;
        let map_bytes = map_len * usize::from(map_depth).div_ceil(8);
        let palette = if mapped {
            let palette = bytes
                .get(at..at + map_bytes)
                .ok_or(truncated(at + map_bytes))?;
            at += map_bytes;
            Some(palette)
        } else {
            None
        };
        let colour = |depth: u8| Colour::of_depth(depth, descriptor & 0x0f);
        let refuse = || unsupported(TGA, format!("type {kind} at {depth} bits"));
        let (layout, palette) = match (kind & !8, depth) {
            (1, 8) => {
                let entry = colour(map_depth).ok_or_else(refuse)?;
                let palette = palette.ok_or_else(refuse)?;
                (PixelLayout::Index(entry, map_first), palette)
            }
            (2 | 3, 8) => (PixelLayout::Grey, &[][..]),
            (3, 16) => (PixelLayout::GreyAlpha, &[][..]),
            (2, depth) => (
                PixelLayout::Colour(colour(depth).ok_or_else(refuse)?),
                &[][..],
            ),
            _ => return Err(refuse()),
        };
        if width == 0 || height == 0 || layers == 0 || !height.is_multiple_of(layers) {
            return Err(unsupported(
                TGA,
                format!("{width}x{height} image of {layers} layers"),
            ));
        }
        let pixel = usize::from(depth).div_ceil(8);
        let count = width * height;
        // A run packet of 2 bytes or more expands to at most 128 pixels.
        let room = bytes.len().saturating_sub(at) * if rle { 128 } else { 1 };
        let mut stored = Vec::with_capacity((count * pixel).min(room));
        if rle {
            while stored.len() < count * pixel {
                let head = *bytes.get(at).ok_or(truncated(at + 1))?;
                at += 1;
                let run = usize::from(head & 0x7f) + 1;
                let take = if head & 0x80 != 0 { pixel } else { run * pixel };
                let values = bytes.get(at..at + take).ok_or(truncated(at + take))?;
                if head & 0x80 != 0 {
                    for _ in 0..run {
                        stored.extend_from_slice(values);
                    }
                } else {
                    stored.extend_from_slice(values);
                }
                at += take;
            }
            stored.truncate(count * pixel);
        } else {
            let values = bytes
                .get(at..at + count * pixel)
                .ok_or(truncated(at + count * pixel))?;
            stored.extend_from_slice(values);
        }
        let format = layout.format();
        let out = format.size(1, 1);
        let mut data = Vec::with_capacity(count * out);
        for texel in stored.chunks_exact(pixel) {
            let rgba = layout.rgba(texel, palette).ok_or_else(|| {
                unsupported(TGA, format!("colour index past its {map_len} entries"))
            })?;
            data.extend_from_slice(&rgba[..out]);
        }
        // Right to left: each row mirrored.
        let row = width * out;
        if descriptor & 0x10 != 0 {
            for line in data.chunks_exact_mut(row) {
                let texels = line.len() / out;
                for x in 0..texels / 2 {
                    for byte in 0..out {
                        line.swap(x * out + byte, (texels - 1 - x) * out + byte);
                    }
                }
            }
        }
        let layer_height = height / layers;
        let layers: Vec<Vec<Mip>> = data
            .chunks_exact(row * layer_height)
            .map(|layer| {
                // Top to bottom: each layer turned over, kept in its place.
                let data = if descriptor & 0x20 != 0 {
                    layer.chunks_exact(row).rev().flatten().copied().collect()
                } else {
                    layer.to_vec()
                };
                vec![Mip {
                    width,
                    height: layer_height,
                    data,
                }]
            })
            .collect();
        Ok(Texture {
            format,
            cube: layers.len() == 6,
            layers,
            txi: Txi::default(),
            bottom_up: true,
            missing_mips: 0,
            declared_size: None,
        })
    }
}

/// How a TGA stores a colour, in a texel or a palette entry.
#[derive(Clone, Copy)]
enum Colour {
    Bgra,
    Bgr,
    /// 16 bits, the top one alpha.
    Argb1555,
    /// 15 or 16 bits, no alpha.
    Rgb555,
}

impl Colour {
    fn of_depth(depth: u8, alpha_bits: u8) -> Option<Colour> {
        Some(match depth {
            32 => Colour::Bgra,
            24 => Colour::Bgr,
            16 if alpha_bits > 0 => Colour::Argb1555,
            15 | 16 => Colour::Rgb555,
            _ => return None,
        })
    }

    fn size(self) -> usize {
        match self {
            Colour::Bgra => 4,
            Colour::Bgr => 3,
            Colour::Argb1555 | Colour::Rgb555 => 2,
        }
    }

    fn rgba(self, stored: &[u8]) -> [u8; 4] {
        let five = |bits: u16| {
            let value = (bits & 0x1f) as u8;
            (value << 3) | (value >> 2)
        };
        match self {
            Colour::Bgra => [stored[2], stored[1], stored[0], stored[3]],
            Colour::Bgr => [stored[2], stored[1], stored[0], 255],
            Colour::Argb1555 | Colour::Rgb555 => {
                let bits = u16::from_le_bytes([stored[0], stored[1]]);
                let opaque = matches!(self, Colour::Rgb555) || bits & 0x8000 != 0;
                let alpha = if opaque { 255 } else { 0 };
                [five(bits >> 10), five(bits >> 5), five(bits), alpha]
            }
        }
    }
}

/// How a TGA stores one texel.
#[derive(Clone, Copy)]
enum PixelLayout {
    Colour(Colour),
    Grey,
    GreyAlpha,
    /// A byte indexing a palette whose entries are stored so; the first
    /// entry's index.
    Index(Colour, usize),
}

impl PixelLayout {
    fn format(self) -> PixelFormat {
        match self {
            PixelLayout::Colour(Colour::Bgr | Colour::Rgb555)
            | PixelLayout::Index(Colour::Bgr | Colour::Rgb555, _) => PixelFormat::Rgb8,
            PixelLayout::Grey => PixelFormat::Grey8,
            _ => PixelFormat::Rgba8,
        }
    }

    /// The texel as RGBA (grey first for grey); `None` for an index outside
    /// the palette.
    fn rgba(self, texel: &[u8], palette: &[u8]) -> Option<[u8; 4]> {
        Some(match self {
            PixelLayout::Colour(colour) => colour.rgba(texel),
            PixelLayout::Grey => [texel[0], texel[0], texel[0], 255],
            PixelLayout::GreyAlpha => [texel[0], texel[0], texel[0], texel[1]],
            PixelLayout::Index(entry, first) => {
                let at = usize::from(texel[0]).checked_sub(first)? * entry.size();
                entry.rgba(palette.get(at..at + entry.size())?)
            }
        })
    }
}
