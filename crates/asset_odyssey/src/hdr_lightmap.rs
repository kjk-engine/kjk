//! HDR lightmap codec (`.klm`): `Rgba16Float` with island-aware padded
//! mipmaps and `lz4_flex` compression.
//!
//! Layout on disk (`EXTENSION = "klm"`):
//! - 4 bytes: magic `"KLM2"`
//! - 4 bytes: `width` (`u32` LE)
//! - 4 bytes: `height` (`u32` LE)
//! - 4 bytes: `mip_count` (`u32` LE)
//! - Remaining bytes: `lz4_flex::compress_prepend_size` payload containing all
//!   `mip_count` levels concatenated largest-first, each texel stored as
//!   `[r, g, b, a]` little-endian IEEE-754 `f16` (`8` bytes per texel).

pub const EXTENSION: &str = "klm";
pub const MAGIC: &[u8; 4] = b"KLM2";
const DILATION_RINGS: usize = 8;

/// Converts a non-negative `f32` value into IEEE-754 `f16` bits (`0.0..=65504.0`).
pub fn f32_to_f16(v: f32) -> u16 {
    if v.is_nan() || v <= 0.0 {
        return 0;
    }
    if v >= 65504.0 {
        return 0x7bff;
    }
    let bits = v.to_bits();
    let exp = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    let mant = bits & ((1 << 23) - 1);
    if exp <= 0 {
        if exp < -10 {
            return 0;
        }
        let shift = (14 - exp) as u32;
        let m = mant | (1 << 23);
        let rounded = (m + (1 << (shift - 1))) >> shift;
        return rounded as u16;
    }
    if exp >= 31 {
        return 0x7bff;
    }
    let mant_rounded = (mant + 0x1000) >> 13;
    if mant_rounded >= 0x0400 {
        let next_exp = exp + 1;
        if next_exp >= 31 {
            0x7bff
        } else {
            (next_exp as u16) << 10
        }
    } else {
        ((exp as u16) << 10) | (mant_rounded as u16)
    }
}

/// Converts IEEE-754 `f16` bits into `f32`.
pub fn f16_to_f32(bits: u16) -> f32 {
    let sign = u32::from(bits & 0x8000) << 16;
    let exp = u32::from((bits >> 10) & 0x1f);
    let mut mant = u32::from(bits & 0x03ff);
    if exp == 0 {
        if mant == 0 {
            return f32::from_bits(sign);
        }
        let mut e = 127 - 14;
        while (mant & 0x0400) == 0 {
            mant <<= 1;
            e -= 1;
        }
        return f32::from_bits(sign | (e << 23) | ((mant & 0x03ff) << 13));
    }
    if exp == 31 {
        return f32::from_bits(sign | 0x7f80_0000 | (mant << 13));
    }
    f32::from_bits(sign | ((exp + 127 - 15) << 23) | (mant << 13))
}

fn dilate(pixels: &mut [[f32; 4]], valid: &mut [bool], width: usize, height: usize) {
    for _ in 0..DILATION_RINGS {
        let prev_px = pixels.to_vec();
        let prev_ok = valid.to_vec();
        let mut any = false;
        for y in 0..height {
            for x in 0..width {
                let idx = y * width + x;
                if prev_ok[idx] {
                    continue;
                }
                let mut sum = [0.0_f32; 4];
                let mut count = 0.0_f32;
                for dy in -1_isize..=1 {
                    for dx in -1_isize..=1 {
                        let (nx, ny) = (x as isize + dx, y as isize + dy);
                        if nx < 0 || ny < 0 || nx >= width as isize || ny >= height as isize {
                            continue;
                        }
                        let nidx = ny as usize * width + nx as usize;
                        if prev_ok[nidx] {
                            for c in 0..4 {
                                sum[c] += prev_px[nidx][c];
                            }
                            count += 1.0;
                        }
                    }
                }
                if count > 0.0 {
                    pixels[idx] = sum.map(|s| s / count);
                    valid[idx] = true;
                    any = true;
                }
            }
        }
        if !any {
            break;
        }
    }
}

fn pack_level(pixels: &[[f32; 4]]) -> Vec<u8> {
    let mut out = Vec::with_capacity(pixels.len() * 8);
    for px in pixels {
        for &c in px {
            out.extend_from_slice(&f32_to_f16(c).to_le_bytes());
        }
    }
    out
}

/// Builds a full `Rgba16Float` mip chain with UV island-aware gutter padding
/// so mip levels do not bleed invalid background texels across chart seams.
pub fn build_padded_mips(
    width: usize,
    height: usize,
    pixels: &[[f32; 4]],
    valid: &[bool],
) -> Vec<Vec<u8>> {
    let (mut w, mut h) = (width.max(1), height.max(1));
    let mut cur_px = pixels[..w * h].to_vec();
    let mut cur_ok = valid[..w * h].to_vec();
    dilate(&mut cur_px, &mut cur_ok, w, h);

    let mut levels = vec![pack_level(&cur_px)];
    while w > 1 || h > 1 {
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut next_px = vec![[0.0_f32; 4]; nw * nh];
        let mut next_ok = vec![false; nw * nh];
        for y in 0..nh {
            for x in 0..nw {
                let mut valid_sum = [0.0_f32; 4];
                let mut valid_count = 0.0_f32;
                let mut all_sum = [0.0_f32; 4];
                let mut all_count = 0.0_f32;
                for sy in 0..2 {
                    for sx in 0..2 {
                        let px_x = (2 * x + sx).min(w - 1);
                        let px_y = (2 * y + sy).min(h - 1);
                        let src = px_y * w + px_x;
                        for c in 0..4 {
                            all_sum[c] += cur_px[src][c];
                        }
                        all_count += 1.0;
                        if cur_ok[src] {
                            for c in 0..4 {
                                valid_sum[c] += cur_px[src][c];
                            }
                            valid_count += 1.0;
                        }
                    }
                }
                let dst = y * nw + x;
                if valid_count > 0.0 {
                    next_px[dst] = valid_sum.map(|s| s / valid_count);
                    next_ok[dst] = true;
                } else {
                    next_px[dst] = all_sum.map(|s| s / all_count.max(1.0));
                }
            }
        }
        dilate(&mut next_px, &mut next_ok, nw, nh);
        levels.push(pack_level(&next_px));
        cur_px = next_px;
        cur_ok = next_ok;
        w = nw;
        h = nh;
    }
    levels
}

/// Builds a full `Rgba16Float` mip chain treating every texel as valid.
pub fn encode_rgba32f_mips(width: usize, height: usize, pixels: &[[f32; 4]]) -> Vec<Vec<u8>> {
    let valid = vec![true; width.max(1) * height.max(1)];
    build_padded_mips(width, height, pixels, &valid)
}

/// Decodes `Rgba16Float` bytes of `width × height` into linear `[r, g, b, a]` `f32`.
pub fn decode_rgba32f(data: &[u8], width: usize, height: usize) -> Option<Vec<[f32; 4]>> {
    let expected = width.checked_mul(height)?.checked_mul(8)?;
    let level0 = data.get(..expected)?;
    let mut out = Vec::with_capacity(width * height);
    for texel in level0.as_chunks::<8>().0 {
        let mut rgba = [0.0_f32; 4];
        for c in 0..4 {
            let bits = u16::from_le_bytes([texel[c * 2], texel[c * 2 + 1]]);
            rgba[c] = f16_to_f32(bits);
        }
        out.push(rgba);
    }
    Some(out)
}

/// Decodes level 0 of an `Rgba16Float` mip chain into linear `[r, g, b, a]` `f32`.
pub fn decode_level0_rgba32f(mips: &[Vec<u8>], width: u32, height: u32) -> Option<Vec<[f32; 4]>> {
    decode_rgba32f(mips.first()?, width as usize, height as usize)
}

/// Serializes `width × height` `Rgba16Float` mip levels to a `.klm` buffer.
pub fn encode(width: u32, height: u32, mips: &[Vec<u8>]) -> Vec<u8> {
    let total: usize = mips.iter().map(Vec::len).sum();
    let mut raw = Vec::with_capacity(total);
    for mip in mips {
        raw.extend_from_slice(mip);
    }
    let compressed = lz4_flex::compress_prepend_size(&raw);
    let mut out = Vec::with_capacity(16 + compressed.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(&(mips.len() as u32).to_le_bytes());
    out.extend_from_slice(&compressed);
    out
}

/// Reads a `.klm` buffer into `(width, height, mips)`.
pub fn decode(bytes: &[u8]) -> Result<(u32, u32, Vec<Vec<u8>>), String> {
    if bytes.len() < 16 {
        return Err("klm header truncated".to_string());
    }
    if &bytes[..4] != MAGIC {
        return Err("klm magic mismatch".to_string());
    }
    let width = u32::from_le_bytes(bytes[4..8].try_into().map_err(|_| "bad klm width")?);
    let height = u32::from_le_bytes(bytes[8..12].try_into().map_err(|_| "bad klm height")?);
    let mip_count = u32::from_le_bytes(bytes[12..16].try_into().map_err(|_| "bad klm mips")?);
    if width == 0 || height == 0 || mip_count == 0 {
        return Err("klm zero dimensions or mip count".to_string());
    }
    let raw = lz4_flex::decompress_size_prepended(&bytes[16..])
        .map_err(|err| format!("klm lz4 decode failed: {err}"))?;
    let mut offset = 0usize;
    let mut mips = Vec::with_capacity(mip_count as usize);
    let (mut w, mut h) = (width as usize, height as usize);
    for level in 0..mip_count {
        let len = w
            .checked_mul(h)
            .and_then(|n| n.checked_mul(8))
            .ok_or_else(|| "klm mip byte size overflow".to_string())?;
        let end = offset
            .checked_add(len)
            .ok_or_else(|| "klm mip offset overflow".to_string())?;
        let slice = raw
            .get(offset..end)
            .ok_or_else(|| format!("klm mip {level} truncated"))?;
        mips.push(slice.to_vec());
        offset = end;
        w = (w / 2).max(1);
        h = (h / 2).max(1);
    }
    if offset != raw.len() {
        return Err(format!("klm trailing data: {} bytes", raw.len() - offset));
    }
    Ok((width, height, mips))
}

/// Fallback conversion from `Rgba16Float` bytes to approximate gamma-encoded `Rgba8` bytes
/// using deterministic hardware `sqrt` (gamma ~2.0).
pub fn rgba16f_to_rgba8_approx(data: &[u8], width: usize, height: usize) -> Option<Vec<u8>> {
    let pixels = decode_rgba32f(data, width, height)?;
    let mut out = Vec::with_capacity(width * height * 4);
    for px in pixels {
        for c in px.into_iter().take(3) {
            let encoded = c.clamp(0.0, 1.0).sqrt();
            out.push((encoded * 255.0).round() as u8);
        }
        out.push((px[3].clamp(0.0, 1.0) * 255.0).round() as u8);
    }
    Some(out)
}

/// Probes checking `f16` conversion, `.klm` round-trip, and island-aware mip padding.
pub fn probes() -> Vec<(&'static str, Result<String, String>)> {
    let check = |ok: bool, detail: String| if ok { Ok(detail) } else { Err(detail) };

    let samples = [0.0_f32, 0.001, 0.01, 0.18, 1.0, 4.0, 16.0, 100.0];
    let f16_ok = samples.iter().all(|&v| {
        let rt = f16_to_f32(f32_to_f16(v));
        let rel = if v == 0.0 {
            rt.abs()
        } else {
            (rt - v).abs() / v
        };
        rel < 2e-3
    });

    // 4x4 grid with valid 2x2 bright island at (0..2, 0..2) and invalid black gutters.
    let mut pixels = vec![[0.0_f32; 4]; 16];
    let mut valid = vec![false; 16];
    for y in 0..2 {
        for x in 0..2 {
            pixels[y * 4 + x] = [4.0, 2.0, 1.0, 1.0];
            valid[y * 4 + x] = true;
        }
    }
    let mips = build_padded_mips(4, 4, &pixels, &valid);
    let blob = encode(4, 4, &mips);
    let decoded = decode(&blob);
    let roundtrip_ok = decoded.as_ref().is_ok_and(|(w, h, out_mips)| {
        *w == 4 && *h == 4 && out_mips.len() == 3 && out_mips == &mips
    });

    let padding_ok = decoded.is_ok_and(|(_, _, out_mips)| {
        // Level 2 (1x1) must equal [4.0, 2.0, 1.0, 1.0] (zero bleed from invalid black gutters).
        decode_level0_rgba32f(&out_mips[2..], 1, 1).is_some_and(|px| {
            (px[0][0] - 4.0).abs() < 1e-2
                && (px[0][1] - 2.0).abs() < 1e-2
                && (px[0][2] - 1.0).abs() < 1e-2
                && (px[0][3] - 1.0).abs() < 1e-2
        })
    });

    vec![
        ("f16_roundtrip", check(f16_ok, format!("f16_ok={f16_ok}"))),
        (
            "klm_encode_decode_roundtrip",
            check(roundtrip_ok, format!("roundtrip_ok={roundtrip_ok}")),
        ),
        (
            "island_padded_mips_no_bleed",
            check(padding_ok, format!("padding_ok={padding_ok}")),
        ),
    ]
}
