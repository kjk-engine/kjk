//! The macOS app's icon, drawn here rather than kept as an image: a steel
//! ring on a dark rounded square, crossed by a glowing blue blade. Our own
//! design; no game's art or logo.

use image::{Rgba, RgbaImage};

/// The sizes an `.iconset` holds, each also at twice the pixels (`@2x`).
const SIZES: &[u32] = &[16, 32, 128, 256, 512];

/// Samples per pixel along each axis, for smooth edges.
const SAMPLES: u32 = 4;

const BACKGROUND_TOP: [f32; 3] = [0.10, 0.16, 0.27];
const BACKGROUND_BOTTOM: [f32; 3] = [0.03, 0.05, 0.10];
const RING: [f32; 3] = [0.56, 0.64, 0.75];
const GLOW: [f32; 3] = [0.30, 0.76, 1.00];
const CORE: [f32; 3] = [0.92, 0.98, 1.00];

/// Writes `<dir>/icon_<n>x<n>[@2x].png` for every size `iconutil` expects.
pub fn write_iconset(dir: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    for &size in SIZES {
        for (scale, suffix) in [(1, ""), (2, "@2x")] {
            let path = dir.join(format!("icon_{size}x{size}{suffix}.png"));
            draw(size * scale)
                .save(&path)
                .map_err(|error| format!("{}: {error}", path.display()))?;
        }
    }
    Ok(())
}

fn draw(size: u32) -> RgbaImage {
    RgbaImage::from_fn(size, size, |x, y| {
        let mut sum = [0.0_f32; 4];
        for sy in 0..SAMPLES {
            for sx in 0..SAMPLES {
                let u = (x as f32 + (sx as f32 + 0.5) / SAMPLES as f32) / size as f32;
                let v = (y as f32 + (sy as f32 + 0.5) / SAMPLES as f32) / size as f32;
                let [r, g, b, a] = shade(u, v);
                sum = [sum[0] + r * a, sum[1] + g * a, sum[2] + b * a, sum[3] + a];
            }
        }
        let n = (SAMPLES * SAMPLES) as f32;
        let alpha = sum[3] / n;
        let channel = |c: f32| {
            let straight = if alpha > 0.0 { c / n / alpha } else { 0.0 };
            (straight.clamp(0.0, 1.0) * 255.0).round() as u8
        };
        Rgba([
            channel(sum[0]),
            channel(sum[1]),
            channel(sum[2]),
            (alpha * 255.0).round() as u8,
        ])
    })
}

/// The colour and coverage at a point of the unit square (y down).
fn shade(u: f32, v: f32) -> [f32; 4] {
    // macOS icons leave a margin round a rounded square.
    if rounded_square(u, v, 0.10, 0.18) > 0.0 {
        return [0.0; 4];
    }
    let mut colour = mix(BACKGROUND_TOP, BACKGROUND_BOTTOM, v);

    let from_centre = ((u - 0.5).powi(2) + (v - 0.5).powi(2)).sqrt();
    let ring = 1.0 - smooth(((from_centre - 0.27).abs() - 0.025) / 0.006);
    colour = mix(colour, RING, ring);

    // The blade, lower left to upper right, with a soft glow round its core.
    let blade = segment_distance(u, v, (0.27, 0.73), (0.73, 0.27));
    let glow = (-blade / 0.045).exp() * 0.85;
    colour = mix(colour, GLOW, glow);
    let core = 1.0 - smooth((blade - 0.016) / 0.006);
    colour = mix(colour, CORE, core);

    [colour[0], colour[1], colour[2], 1.0]
}

/// Signed distance outside a square inset by `margin`, corners of `radius`.
fn rounded_square(u: f32, v: f32, margin: f32, radius: f32) -> f32 {
    let half = 0.5 - margin - radius;
    let dx = ((u - 0.5).abs() - half).max(0.0);
    let dy = ((v - 0.5).abs() - half).max(0.0);
    (dx * dx + dy * dy).sqrt() - radius
}

fn segment_distance(u: f32, v: f32, a: (f32, f32), b: (f32, f32)) -> f32 {
    let (abx, aby) = (b.0 - a.0, b.1 - a.1);
    let t = (((u - a.0) * abx + (v - a.1) * aby) / (abx * abx + aby * aby)).clamp(0.0, 1.0);
    ((u - a.0 - abx * t).powi(2) + (v - a.1 - aby * t).powi(2)).sqrt()
}

/// 0 below -1, 1 above 1, smooth between.
fn smooth(x: f32) -> f32 {
    let t = ((x + 1.0) / 2.0).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}
