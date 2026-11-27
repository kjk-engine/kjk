//! Linear HDR `.klm` lightmaps (`KJK1` header, `Rgba16Float` pixels stored
//! per mip as LZ4-compressed raw bytes with island-aware seam dilation).

use std::io;

use crate::texture::{Encoding, TextureFacts, TextureIr};

/// 4-byte file header magic for `.klm` lightmaps.
pub const KLM_MAGIC: [u8; 4] = *b"KJK1";

/// Maximum finite positive IEEE 754 `binary16` value.
const F16_MAX: f32 = 65504.0;

/// Converts an `f32` to IEEE 754 `binary16` with round-to-nearest-even.
pub fn f32_to_f16(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xff) as i32;
    let frac = bits & 0x007f_ffff;
    if exp == 0xff {
        return sign | 0x7c00 | if frac != 0 { ((frac >> 13) as u16).max(1) } else { 0 };
    }
    let unbiased = exp - 127;
    if unbiased > 15 {
        return sign | 0x7c00;
    }
    if unbiased >= -14 {
        let h_exp = (unbiased + 15) as u32;
        let round_bit = 1u32 << 12;
        let sticky = (frac & (round_bit - 1)) != 0;
        let lsb = (frac >> 13) & 1;
        let mut h_frac = frac >> 13;
        if (frac & round_bit) != 0 && (sticky || lsb != 0) {
            h_frac += 1;
            if h_frac == 0x400 {
                return sign | (((h_exp + 1) << 10) as u16);
            }
        }
        return sign | ((h_exp << 10) as u16) | (h_frac as u16);
    }
    if unbiased >= -25 {
        let mant = 0x0080_0000 | frac;
        let shift = (-14 - unbiased) + 13;
        let round_bit = 1u32 << (shift - 1);
        let sticky = (mant & (round_bit - 1)) != 0;
        let lsb = (mant >> shift) & 1;
        let mut h_frac = mant >> shift;
        if (mant & round_bit) != 0 && (sticky || lsb != 0) {
            h_frac += 1;
        }
        return sign | (h_frac as u16);
    }
    sign
}

/// Converts IEEE 754 `binary16` bits to `f32`.
pub fn f16_to_f32(half: u16) -> f32 {
    let sign = u32::from(half & 0x8000) << 16;
    let exp = (half >> 10) & 0x1f;
    let frac = u32::from(half & 0x03ff);
    if exp == 0 {
        if frac == 0 {
            return f32::from_bits(sign);
        }
        let mut e = -14i32;
        let mut m = frac;
        while (m & 0x0400) == 0 {
            m <<= 1;
            e -= 1;
        }
        m &= 0x03ff;
        let f_exp = ((e + 127) as u32) << 23;
        return f32::from_bits(sign | f_exp | (m << 13));
    }
    if exp == 0x1f {
        let f_frac = if frac != 0 { (frac << 13) | 0x0040_0000 } else { 0 };
        return f32::from_bits(sign | 0x7f80_0000 | f_frac);
    }
    let f_exp = ((i32::from(exp) - 15 + 127) as u32) << 23;
    f32::from_bits(sign | f_exp | (frac << 13))
}

/// Clamps an HDR lightmap texel into finite non-negative `binary16` range
/// (and sunlit visibility alpha into `[0.0, 1.0]`) and packs 8 LE bytes.
fn pack_texel(rgba: [f32; 4]) -> [u8; 8] {
    let r = if rgba[0].is_finite() { rgba[0].clamp(0.0, F16_MAX) } else { 0.0 };
    let g = if rgba[1].is_finite() { rgba[1].clamp(0.0, F16_MAX) } else { 0.0 };
    let b = if rgba[2].is_finite() { rgba[2].clamp(0.0, F16_MAX) } else { 0.0 };
    let a = if rgba[3].is_finite() { rgba[3].clamp(0.0, 1.0) } else { 1.0 };
    let mut out = [0u8; 8];
    for (i, ch) in [r, g, b, a].into_iter().enumerate() {
        let bytes = f32_to_f16(ch).to_le_bytes();
        out[i * 2] = bytes[0];
        out[i * 2 + 1] = bytes[1];
    }
    out
}

/// Runs multi-pass 8-neighbor dilation over invalid texels so UV chart gutters
/// never bleed black or stale texels into valid island edges across mips.
fn dilate_invalid(pixels: &mut [[f32; 4]], mask: &mut [bool], width: usize, height: usize, passes: usize) {
    if width == 0 || height == 0 || passes == 0 {
        return;
    }
    let mut next_pixels = pixels.to_vec();
    let mut next_mask = mask.to_vec();
    for _ in 0..passes {
        let mut changed = false;
        for y in 0..height {
            for x in 0..width {
                let idx = y * width + x;
                if mask[idx] {
                    continue;
                }
                let mut sum = [0.0f32; 4];
                let mut count = 0.0f32;
                let y0 = y.saturating_sub(1);
                let y1 = (y + 1).min(height - 1);
                let x0 = x.saturating_sub(1);
                let x1 = (x + 1).min(width - 1);
                for ny in y0..=y1 {
                    for nx in x0..=x1 {
                        let nidx = ny * width + nx;
                        if mask[nidx] {
                            let p = pixels[nidx];
                            sum[0] += p[0];
                            sum[1] += p[1];
                            sum[2] += p[2];
                            sum[3] += p[3];
                            count += 1.0;
                        }
                    }
                }
                if count > 0.0 {
                    next_pixels[idx] = [sum[0] / count, sum[1] / count, sum[2] / count, sum[3] / count];
                    next_mask[idx] = true;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
        pixels.copy_from_slice(&next_pixels);
        mask.copy_from_slice(&next_mask);
    }
}

/// Downsamples an HDR RGBA buffer by 2x2 using only valid texels where known.
fn downsample_level(
    pixels: &[[f32; 4]],
    mask: &[bool],
    width: usize,
    height: usize,
) -> (Vec<[f32; 4]>, Vec<bool>, usize, usize) {
    let nw = (width / 2).max(1);
    let nh = (height / 2).max(1);
    let mut out = vec![[0.0f32; 4]; nw * nh];
    let mut out_mask = vec![false; nw * nh];
    for y in 0..nh {
        let sy0 = (y * 2).min(height - 1);
        let sy1 = (y * 2 + 1).min(height - 1);
        for x in 0..nw {
            let sx0 = (x * 2).min(width - 1);
            let sx1 = (x * 2 + 1).min(width - 1);
            let coords = [(sx0, sy0), (sx1, sy0), (sx0, sy1), (sx1, sy1)];
            let mut sum = [0.0f32; 4];
            let mut valid_count = 0.0f32;
            let mut all_sum = [0.0f32; 4];
            for &(sx, sy) in &coords {
                let p = pixels[sy * width + sx];
                all_sum[0] += p[0];
                all_sum[1] += p[1];
                all_sum[2] += p[2];
                all_sum[3] += p[3];
                if mask[sy * width + sx] {
                    sum[0] += p[0];
                    sum[1] += p[1];
                    sum[2] += p[2];
                    sum[3] += p[3];
                    valid_count += 1.0;
                }
            }
            let dst = y * nw + x;
            if valid_count > 0.0 {
                out[dst] = [
                    sum[0] / valid_count,
                    sum[1] / valid_count,
                    sum[2] / valid_count,
                    sum[3] / valid_count,
                ];
                out_mask[dst] = true;
            } else {
                out[dst] = [
                    all_sum[0] * 0.25,
                    all_sum[1] * 0.25,
                    all_sum[2] * 0.25,
                    all_sum[3] * 0.25,
                ];
            }
        }
    }
    (out, out_mask, nw, nh)
}

/// Encodes linear HDR `[r, g, b, sunlit]` texels into a `.klm` container with
/// island-aware padded mipmaps compressed per level via `lz4_flex`.
pub fn encode_klm(
    width: u32,
    height: u32,
    pixels: &[[f32; 4]],
    valid: Option<&[bool]>,
) -> io::Result<Vec<u8>> {
    let w = width as usize;
    let h = height as usize;
    if w == 0 || h == 0 || pixels.len() != w * h {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid klm dimensions or pixel buffer length",
        ));
    }
    let mut level_pixels = pixels.to_vec();
    let mut level_mask = match valid {
        Some(mask) => {
            if mask.len() != w * h {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "invalid klm valid-texel mask length",
                ));
            }
            mask.to_vec()
        }
        None => vec![true; w * h],
    };

    let mip_count = (width.max(height).ilog2() + 1) as usize;
    let mut out = Vec::with_capacity(16 + w * h * 8);
    out.extend_from_slice(&KLM_MAGIC);
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(&(mip_count as u32).to_le_bytes());

    let mut mw = w;
    let mut mh = h;
    for level in 0..mip_count {
        if valid.is_some() {
            dilate_invalid(&mut level_pixels, &mut level_mask, mw, mh, 8);
        }
        let mut raw = Vec::with_capacity(mw * mh * 8);
        for &px in &level_pixels {
            raw.extend_from_slice(&pack_texel(px));
        }
        let compressed = lz4_flex::compress(&raw);
        let compressed_len = u32::try_from(compressed.len()).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "compressed klm mip exceeds u32")
        })?;
        out.extend_from_slice(&compressed_len.to_le_bytes());
        out.extend_from_slice(&compressed);

        if level + 1 < mip_count {
            let (next_px, next_mask, nw, nh) =
                downsample_level(&level_pixels, &level_mask, mw, mh);
            level_pixels = next_px;
            level_mask = next_mask;
            mw = nw;
            mh = nh;
        }
    }
    Ok(out)
}

/// Decodes raw `Rgba16Float` bytes (`w * h * 8`) into `[r, g, b, a]` `f32` texels.
pub fn decode_rgba16f_f32(data: &[u8], width: usize, height: usize) -> Option<Vec<[f32; 4]>> {
    let expected = width.checked_mul(height)?.checked_mul(8)?;
    if data.len() < expected || width == 0 || height == 0 {
        return None;
    }
    let mut out = Vec::with_capacity(width * height);
    for chunk in data[..expected].as_chunks::<8>().0 {
        let r = f16_to_f32(u16::from_le_bytes([chunk[0], chunk[1]]));
        let g = f16_to_f32(u16::from_le_bytes([chunk[2], chunk[3]]));
        let b = f16_to_f32(u16::from_le_bytes([chunk[4], chunk[5]]));
        let a = f16_to_f32(u16::from_le_bytes([chunk[6], chunk[7]]));
        out.push([r, g, b, a]);
    }
    Some(out)
}

/// Decodes raw `Rgba16Float` bytes into 8-bit gamma-encoded RGBA bytes for CPU
/// consumers that only accept `u8` RGBA (clamping HDR highlights above 1.0).
pub fn decode_rgba16f(data: &[u8], width: usize, height: usize) -> Option<Vec<u8>> {
    let texels = decode_rgba16f_f32(data, width, height)?;
    let mut out = Vec::with_capacity(texels.len() * 4);
    for [r, g, b, a] in texels {
        for ch in [r, g, b] {
            let x = ch.clamp(0.0, 1.0);
            let r4 = x.sqrt().sqrt();
            let r8 = r4.sqrt();
            let r16 = r8.sqrt();
            let r64 = r16.sqrt().sqrt();
            let gamma = r4 * r8 * r16 * r64;
            out.push((gamma * 255.0).round().clamp(0.0, 255.0) as u8);
        }
        out.push((a.clamp(0.0, 1.0) * 255.0).round().clamp(0.0, 255.0) as u8);
    }
    Some(out)
}

/// Halves raw `Rgba16Float` bytes by box-filtering 2x2 blocks in linear `f32`.
pub fn halve_rgba16f(data: &[u8], width: usize, height: usize) -> Vec<u8> {
    let Some(texels) = decode_rgba16f_f32(data, width, height) else {
        return Vec::new();
    };
    let nw = (width / 2).max(1);
    let nh = (height / 2).max(1);
    let mut out = Vec::with_capacity(nw * nh * 8);
    for y in 0..nh {
        let sy0 = (y * 2).min(height - 1);
        let sy1 = (y * 2 + 1).min(height - 1);
        for x in 0..nw {
            let sx0 = (x * 2).min(width - 1);
            let sx1 = (x * 2 + 1).min(width - 1);
            let p00 = texels[sy0 * width + sx0];
            let p10 = texels[sy0 * width + sx1];
            let p01 = texels[sy1 * width + sx0];
            let p11 = texels[sy1 * width + sx1];
            let avg = [
                (p00[0] + p10[0] + p01[0] + p11[0]) * 0.25,
                (p00[1] + p10[1] + p01[1] + p11[1]) * 0.25,
                (p00[2] + p10[2] + p01[2] + p11[2]) * 0.25,
                (p00[3] + p10[3] + p01[3] + p11[3]) * 0.25,
            ];
            out.extend_from_slice(&pack_texel(avg));
        }
    }
    out
}

/// Decodes a `.klm` file buffer into a `TextureIr` (`Encoding::Rgba16Float`).
pub fn decode_klm(bytes: &[u8]) -> Option<TextureIr> {
    if bytes.len() < 16 || bytes[..4] != KLM_MAGIC {
        return None;
    }
    let width = u32::from_le_bytes(bytes[4..8].try_into().ok()?);
    let height = u32::from_le_bytes(bytes[8..12].try_into().ok()?);
    let mip_count = u32::from_le_bytes(bytes[12..16].try_into().ok()?) as usize;
    if width == 0 || height == 0 || mip_count == 0 || mip_count > 16 {
        return None;
    }
    let mut cursor = 16usize;
    let mut mips = Vec::with_capacity(mip_count);
    for level in 0..mip_count {
        let len_end = cursor.checked_add(4)?;
        let comp_len = u32::from_le_bytes(bytes.get(cursor..len_end)?.try_into().ok()?) as usize;
        cursor = len_end;
        let data_end = cursor.checked_add(comp_len)?;
        let compressed = bytes.get(cursor..data_end)?;
        cursor = data_end;
        let mw = ((width as usize) >> level).max(1);
        let mh = ((height as usize) >> level).max(1);
        let expected_raw = mw.checked_mul(mh)?.checked_mul(8)?;
        let raw = lz4_flex::decompress(compressed, expected_raw).ok()?;
        if raw.len() != expected_raw {
            return None;
        }
        mips.push(raw);
    }
    let mut facts = TextureFacts::from_txi(&fmt_odyssey::txi::Txi::default(), true);
    facts.clamp = true;
    Some(TextureIr {
        width,
        height,
        encoding: Encoding::Rgba16Float,
        mips,
        cube: false,
        frames: 1,
        facts,
        faces: Vec::new(),
        frame_mips: Vec::new(),
    })
}

/// Deterministic probes covering `f16` conversion, `.klm` encode/decode round-trip,
/// HDR >1.0 preservation, and island-aware seam dilation.
pub fn probes() -> Vec<(&'static str, bool)> {
    let mut results = Vec::new();

    // f16 round-trip across HDR range and tiny subnormals.
    let f16_ok = [0.0f32, 0.0005, 0.125, 1.0, 2.5, 12.75, 128.0, 1024.0]
        .into_iter()
        .all(|v| {
            let decoded = f16_to_f32(f32_to_f16(v));
            (decoded - v).abs() <= v * 0.01 + 1e-4
        });
    results.push(("f16 round-trip preserves HDR and near-black precision", f16_ok));

    // Island-aware padded mipmaps: 4x4 with top-left 2x2 valid bright HDR and rest invalid 0.
    let mut pixels = vec![[0.0, 0.0, 0.0, 0.0]; 16];
    let mut valid = vec![false; 16];
    for y in 0..2 {
        for x in 0..2 {
            pixels[y * 4 + x] = [6.5, 2.25, 0.5, 0.75];
            valid[y * 4 + x] = true;
        }
    }
    let encoded = encode_klm(4, 4, &pixels, Some(&valid));
    let klm_ok = encoded
        .ok()
        .and_then(|bytes| decode_klm(&bytes))
        .is_some_and(|ir| {
            if ir.width != 4
                || ir.height != 4
                || ir.encoding != Encoding::Rgba16Float
                || ir.mips.len() != 3
            {
                return false;
            }
            let (Some(m0), Some(m1), Some(m2)) = (
                decode_rgba16f_f32(&ir.mips[0], 4, 4),
                decode_rgba16f_f32(&ir.mips[1], 2, 2),
                decode_rgba16f_f32(&ir.mips[2], 1, 1),
            ) else {
                return false;
            };
            // Valid texels and padded neighbors preserve HDR [6.5, 2.25, 0.5, 0.75] without black bleed.
            m0.len() == 16
                && m1.len() == 4
                && m2.len() == 1
                && (m0[0][0] - 6.5).abs() < 0.02
                && (m0[3][0] - 6.5).abs() < 0.02
                && (m1[3][0] - 6.5).abs() < 0.02
                && (m2[0][0] - 6.5).abs() < 0.02
                && (m2[0][3] - 0.75).abs() < 0.01
        });
    results.push(("klm encode/decode preserves HDR > 1.0 and island seam dilation", klm_ok));

    results
}
