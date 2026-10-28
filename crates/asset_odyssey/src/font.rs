//! KOTOR's bitmap fonts: a texture whose TXI holds a glyph table, one
//! glyph per byte of the game's code page.
//!
//! - A glyph is the rectangle between its `upperleftcoords` and
//!   `lowerrightcoords` entries, in texture coordinates with v rising
//!   toward the top of the upright sheet; kept here upright (v down).
//! - Sizes are GUI pixels: `fontheight` × 100 tall, as wide as the glyph's
//!   share of the sheet gives at that height; `spacingR` × 100 follows
//!   every glyph and `spacingB` × 100 every line.
//! - A byte past the table advances like `m`, or by the spacing alone.

use fmt_odyssey::txi::Txi;

#[derive(Debug, Clone, PartialEq)]
pub struct Glyph {
    /// u0, v0, u1, v1 on the upright texture (v0 the top).
    pub uv: [f32; 4],
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Font {
    pub texture: String,
    pub glyphs: Vec<Glyph>,
    pub height: f32,
    pub spacing_r: f32,
    pub spacing_b: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontError {
    NoGlyphTable,
    CountMismatch {
        chars: usize,
        upper: usize,
        lower: usize,
    },
}

fn coords(txi: &Txi, keyword: &str) -> Option<Vec<[f32; 2]>> {
    let (_, rows) = txi.blocks.iter().rev().find(|(key, _)| key == keyword)?;
    Some(
        rows.iter()
            .map(|row| {
                let number = |at: usize| {
                    row.get(at)
                        .and_then(|word| word.parse().ok())
                        .unwrap_or(0.0)
                };
                [number(0), number(1)]
            })
            .collect(),
    )
}

impl Font {
    pub fn from_txi(texture: &str, txi: &Txi, width: u32, height: u32) -> Result<Font, FontError> {
        let (Some(upper), Some(lower)) = (
            coords(txi, "upperleftcoords"),
            coords(txi, "lowerrightcoords"),
        ) else {
            return Err(FontError::NoGlyphTable);
        };
        let chars = txi
            .number("numchars")
            .map_or(upper.len(), |count| count as usize);
        if upper.len() != lower.len() || upper.len() < chars {
            return Err(FontError::CountMismatch {
                chars,
                upper: upper.len(),
                lower: lower.len(),
            });
        }
        let line = txi.number("fontheight").unwrap_or(0.0) * 100.0;
        let aspect = width as f32 / height.max(1) as f32;
        let glyphs = upper
            .iter()
            .zip(&lower)
            .take(chars)
            .map(|(ul, lr)| {
                let (du, dv) = ((lr[0] - ul[0]).abs(), (ul[1] - lr[1]).abs());
                Glyph {
                    uv: [ul[0], 1.0 - ul[1], lr[0], 1.0 - lr[1]],
                    width: if dv > 0.0 {
                        line * du / dv * aspect
                    } else {
                        0.0
                    },
                    height: line,
                }
            })
            .collect();
        Ok(Font {
            texture: texture.to_string(),
            glyphs,
            height: line,
            spacing_r: txi.number("spacingr").unwrap_or(0.0) * 100.0,
            spacing_b: txi.number("spacingb").unwrap_or(0.0) * 100.0,
        })
    }

    pub fn glyph(&self, byte: u8) -> Option<&Glyph> {
        self.glyphs.get(usize::from(byte))
    }

    /// How far a byte moves the pen.
    pub fn advance(&self, byte: u8) -> f32 {
        let glyph = self.glyph(byte).or_else(|| self.glyph(b'm'));
        glyph.map_or(0.0, |glyph| glyph.width) + self.spacing_r
    }

    /// From one line's top to the next's.
    pub fn line_height(&self) -> f32 {
        self.height + self.spacing_b
    }

    pub fn measure(&self, text: &[u8]) -> f32 {
        text.iter().map(|&byte| self.advance(byte)).sum()
    }
}
