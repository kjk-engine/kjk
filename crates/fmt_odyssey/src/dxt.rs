//! DXT1 (BC1) and DXT5 (BC3) block decoding to RGBA8. Used for PNG exports
//! and checks, and by the renderer only where it cannot upload the blocks
//! as they are (no BC support, cube maps, odd sizes, DXT1 holes).

fn rgb565(value: u16) -> [u8; 3] {
    let r = ((value >> 11) & 0x1f) as u32;
    let g = ((value >> 5) & 0x3f) as u32;
    let b = (value & 0x1f) as u32;
    [
        ((r * 255 + 15) / 31) as u8,
        ((g * 255 + 31) / 63) as u8,
        ((b * 255 + 15) / 31) as u8,
    ]
}

/// The four colours of a colour block, and whether it uses 1-bit alpha.
fn palette(block: &[u8], allow_alpha: bool) -> [[u8; 4]; 4] {
    let c0 = u16::from_le_bytes([block[0], block[1]]);
    let c1 = u16::from_le_bytes([block[2], block[3]]);
    let (a, b) = (rgb565(c0), rgb565(c1));
    let mix = |x: u8, y: u8, wx: u32, wy: u32| {
        ((u32::from(x) * wx + u32::from(y) * wy) / (wx + wy)) as u8
    };
    let mut colours = [[0u8; 4]; 4];
    colours[0] = [a[0], a[1], a[2], 255];
    colours[1] = [b[0], b[1], b[2], 255];
    if c0 > c1 || !allow_alpha {
        colours[2] = [
            mix(a[0], b[0], 2, 1),
            mix(a[1], b[1], 2, 1),
            mix(a[2], b[2], 2, 1),
            255,
        ];
        colours[3] = [
            mix(a[0], b[0], 1, 2),
            mix(a[1], b[1], 1, 2),
            mix(a[2], b[2], 1, 2),
            255,
        ];
    } else {
        colours[2] = [
            mix(a[0], b[0], 1, 1),
            mix(a[1], b[1], 1, 1),
            mix(a[2], b[2], 1, 1),
            255,
        ];
        colours[3] = [0, 0, 0, 0];
    }
    colours
}

fn alpha_palette(a0: u8, a1: u8) -> [u8; 8] {
    let (x, y) = (u32::from(a0), u32::from(a1));
    let mut values = [a0, a1, 0, 0, 0, 0, 0, 0];
    if a0 > a1 {
        for i in 1..7u32 {
            values[i as usize + 1] = (((7 - i) * x + i * y) / 7) as u8;
        }
    } else {
        for i in 1..5u32 {
            values[i as usize + 1] = (((5 - i) * x + i * y) / 5) as u8;
        }
        values[6] = 0;
        values[7] = 255;
    }
    values
}

/// Decodes `data` holding `width` × `height` pixels of DXT1 (`dxt5 = false`)
/// or DXT5 blocks into RGBA8, row by row from the top.
pub fn decode(data: &[u8], width: usize, height: usize, dxt5: bool) -> Option<Vec<u8>> {
    let block_size = if dxt5 { 16 } else { 8 };
    let blocks_x = width.div_ceil(4);
    let blocks_y = height.div_ceil(4);
    if data.len() < blocks_x * blocks_y * block_size {
        return None;
    }
    let mut out = vec![0u8; width * height * 4];
    for by in 0..blocks_y {
        for bx in 0..blocks_x {
            let block = &data[(by * blocks_x + bx) * block_size..][..block_size];
            let (alpha, colour) = if dxt5 {
                (Some(&block[..8]), &block[8..])
            } else {
                (None, block)
            };
            let colours = palette(colour, !dxt5);
            let indices = u32::from_le_bytes([colour[4], colour[5], colour[6], colour[7]]);
            let alphas = alpha.map(|a| {
                let bits = a[2..8]
                    .iter()
                    .enumerate()
                    .fold(0u64, |acc, (i, &byte)| acc | (u64::from(byte) << (8 * i)));
                (alpha_palette(a[0], a[1]), bits)
            });
            for py in 0..4 {
                for px in 0..4 {
                    let (x, y) = (bx * 4 + px, by * 4 + py);
                    if x >= width || y >= height {
                        continue;
                    }
                    let i = py * 4 + px;
                    let mut pixel = colours[((indices >> (2 * i)) & 3) as usize];
                    if let Some((values, bits)) = alphas {
                        pixel[3] = values[((bits >> (3 * i)) & 7) as usize];
                    }
                    out[(y * width + x) * 4..][..4].copy_from_slice(&pixel);
                }
            }
        }
    }
    Some(out)
}

/// Whether any DXT1 block in `data` uses its transparent texel: a block
/// whose first colour is not above its second has three colours and
/// transparent black, and a texel indexing 3 picks the latter. The games
/// draw DXT1 as RGB, so such a texel is opaque black, which the GPU's BC1
/// would sample as transparent instead.
pub fn has_holes(data: &[u8]) -> bool {
    data.as_chunks::<8>().0.iter().any(|block| {
        let c0 = u16::from_le_bytes([block[0], block[1]]);
        let c1 = u16::from_le_bytes([block[2], block[3]]);
        let indices = u32::from_le_bytes([block[4], block[5], block[6], block[7]]);
        // Both bits of some 2-bit index set: index 3.
        c0 <= c1 && indices & (indices >> 1) & 0x5555_5555 != 0
    })
}
