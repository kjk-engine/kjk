//! A draw list painted on the CPU: headless pictures of screens, and the
//! probes' way of seeing what a screen draws. Sampled as the game's UI
//! shader samples: Catmull-Rom where a texture is enlarged, bilinear where
//! not, and a shape's edge (`Pixels::shape`) kept one pixel wide at any
//! scale.

use crate::draw::{DrawList, Quad, TextureId, turn};

/// An upright RGBA8 image, top row first.
#[derive(Debug, Clone, PartialEq)]
pub struct Pixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    /// One colour with its shape in its alpha (a font, line art): drawn
    /// with a sharp edge however large.
    pub shape: bool,
}

/// Mostly white, and not see-through all over: one colour, the shape in
/// the alpha.
pub fn is_shape(rgba: &[u8]) -> bool {
    let (mut seen, mut grey, mut partial) = (0usize, 0usize, 0usize);
    for p in rgba.as_chunks::<4>().0.iter().filter(|p| p[3] > 8) {
        seen += 1;
        grey += usize::from(p[0].min(p[1]).min(p[2]) <= 230);
        partial += usize::from((38..217).contains(&p[3]));
    }
    seen > 0 && grey * 10 < seen && partial * 10 < seen * 6
}

/// A shape's alpha with its edge `per_texel` screen pixels wide made one
/// pixel wide (as the shader's `sharpen`).
pub fn sharpen(alpha: f32, per_texel: f32) -> f32 {
    let width = 0.5 / per_texel.max(1e-3);
    if width >= 0.5 {
        return alpha;
    }
    let t = ((alpha - (0.5 - width)) / (2.0 * width)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Catmull-Rom weights at `t` for the four texels around it.
fn catmull_rom(t: f32) -> [f32; 4] {
    let (t2, t3) = (t * t, t * t * t);
    [
        -0.5 * t3 + t2 - 0.5 * t,
        1.5 * t3 - 2.5 * t2 + 1.0,
        -1.5 * t3 + 2.0 * t2 + 0.5 * t,
        0.5 * t3 - 0.5 * t2,
    ]
}

impl Pixels {
    fn texel(&self, x: i64, y: i64) -> [f32; 4] {
        let x = x.clamp(0, i64::from(self.width.max(1)) - 1) as u32;
        let y = y.clamp(0, i64::from(self.height.max(1)) - 1) as u32;
        let i = ((y * self.width + x) * 4) as usize;
        match self.rgba.get(i..i + 4) {
            Some(p) => [p[0], p[1], p[2], p[3]].map(|c| f32::from(c) / 255.0),
            None => [1.0; 4],
        }
    }

    /// Catmull-Rom, clamped at the edges, in 0..1.
    fn cubic(&self, u: f32, v: f32) -> [f32; 4] {
        let x = u * self.width.max(1) as f32 - 0.5;
        let y = v * self.height.max(1) as f32 - 0.5;
        let (x0, y0) = (x.floor(), y.floor());
        let (wx, wy) = (catmull_rom(x - x0), catmull_rom(y - y0));
        let mut out = [0.0; 4];
        for (j, wy) in wy.iter().enumerate() {
            for (i, wx) in wx.iter().enumerate() {
                let texel = self.texel(x0 as i64 + i as i64 - 1, y0 as i64 + j as i64 - 1);
                for (sum, c) in out.iter_mut().zip(texel) {
                    *sum += c * wx * wy;
                }
            }
        }
        out.map(|c| c.clamp(0.0, 1.0))
    }

    /// Bilinear, clamped at the edges, in 0..1.
    fn sample(&self, u: f32, v: f32) -> [f32; 4] {
        let (w, h) = (self.width.max(1) as f32, self.height.max(1) as f32);
        let x = (u * w - 0.5).clamp(0.0, w - 1.0);
        let y = (v * h - 0.5).clamp(0.0, h - 1.0);
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let at = |x: f32, y: f32| -> [f32; 4] {
            let x = (x as u32).min(self.width.saturating_sub(1));
            let y = (y as u32).min(self.height.saturating_sub(1));
            let i = ((y * self.width + x) * 4) as usize;
            match self.rgba.get(i..i + 4) {
                Some(p) => [p[0], p[1], p[2], p[3]].map(|c| f32::from(c) / 255.0),
                None => [1.0; 4],
            }
        };
        let (a, b) = (at(x0, y0), at(x0 + 1.0, y0));
        let (c, d) = (at(x0, y0 + 1.0), at(x0 + 1.0, y0 + 1.0));
        std::array::from_fn(|k| {
            let top = a[k] + (b[k] - a[k]) * fx;
            let bottom = c[k] + (d[k] - c[k]) * fx;
            top + (bottom - top) * fy
        })
    }
}

/// Where a point of the quad (0..1 across and down) reads its texture,
/// after the quad's quarter turns.
pub fn texel(quad: &Quad, s: f32, t: f32) -> (f32, f32) {
    let (mut s, mut t) = (s, t);
    for _ in 0..quad.turns % 4 {
        (s, t) = (t, 1.0 - s);
    }
    let [u0, v0, u1, v1] = quad.uv;
    (u0 + (u1 - u0) * s, v0 + (v1 - v0) * t)
}

/// `list` drawn into `width` × `height` RGBA8 (top row first) over
/// `clear`, the layout placed as in a window that size
/// (`DrawList::placed`); a quad whose texture
/// `textures` does not give draws white.
pub fn raster<'a>(
    list: &DrawList,
    textures: &dyn Fn(TextureId) -> Option<&'a Pixels>,
    width: u32,
    height: u32,
    clear: [u8; 4],
) -> Vec<u8> {
    let mut out: Vec<f32> = clear
        .map(|c| f32::from(c) / 255.0)
        .repeat((width * height) as usize);
    let (scale, [ox, oy]) = list.placed((width as f32, height as f32));
    let place = |[l, t, w, h]: [f32; 4]| [ox + l * scale, oy + t * scale, w * scale, h * scale];
    for quad in &list.quads {
        let [rx, ry, rw, rh] = place(quad.rect);
        if rw <= 0.0 || rh <= 0.0 {
            continue;
        }
        let [cx, cy, cw, ch] = quad.clip.map_or([rx, ry, rw, rh], place);
        let centre = [rx + rw / 2.0, ry + rh / 2.0];
        // What shows, upright, turned: the box around it.
        let (l, t) = (rx.max(cx), ry.max(cy));
        let (r, b) = ((rx + rw).min(cx + cw), (ry + rh).min(cy + ch));
        let turned = [[l, t], [r, t], [r, b], [l, b]].map(|at| turn(at, centre, quad.spin));
        let most = |pick: fn(f32, f32) -> f32, axis: usize| {
            turned.iter().map(|at| at[axis]).reduce(pick).unwrap_or(0.0)
        };
        let left = most(f32::min, 0).max(0.0).floor() as u32;
        let top = most(f32::min, 1).max(0.0).floor() as u32;
        let right = most(f32::max, 0).min(width as f32).ceil().max(0.0) as u32;
        let bottom = most(f32::max, 1).min(height as f32).ceil().max(0.0) as u32;
        let texture = (quad.texture != TextureId::WHITE)
            .then(|| textures(quad.texture))
            .flatten();
        // Screen pixels per texel, across the quad.
        let per_texel = texture.map_or(1.0, |pixels| {
            let span = (quad.uv[2] - quad.uv[0]).abs().max(1e-6) * pixels.width as f32;
            let across = if quad.turns % 2 == 1 { rh } else { rw };
            across / span
        });
        for y in top..bottom {
            for x in left..right {
                // The pixel's centre in the upright quad.
                let [px, py] = turn([x as f32 + 0.5, y as f32 + 0.5], centre, -quad.spin);
                let across = px >= rx && px < rx + rw && px >= cx && px < cx + cw;
                if !across || py < ry || py >= ry + rh || py < cy || py >= cy + ch {
                    continue;
                }
                let texel = match texture {
                    Some(pixels) => {
                        let (u, v) = texel(quad, (px - rx) / rw, (py - ry) / rh);
                        let mut texel = if per_texel > 1.0 {
                            pixels.cubic(u, v)
                        } else {
                            pixels.sample(u, v)
                        };
                        if pixels.shape {
                            texel[3] = sharpen(texel[3], per_texel);
                        }
                        texel
                    }
                    None => [1.0; 4],
                };
                let src: [f32; 4] = std::array::from_fn(|k| texel[k] * quad.color[k]);
                let alpha = src[3];
                let at = ((y * width + x) * 4) as usize;
                let dst = &mut out[at..at + 4];
                for k in 0..3 {
                    dst[k] = if quad.additive {
                        (dst[k] + src[k] * alpha).min(1.0)
                    } else {
                        src[k] * alpha + dst[k] * (1.0 - alpha)
                    };
                }
                dst[3] = alpha + dst[3] * (1.0 - alpha);
            }
        }
    }
    out.iter()
        .map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8)
        .collect()
}
