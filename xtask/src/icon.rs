//! kjk's icons, rendered from the logo (`docs/logo/kjk.svg`): a hatched
//! Möbius band, light on near-black. Our own design; no game's art or logo.
//!
//! `cargo xtask icon` writes the rendered copies the repo keeps: the
//! Windows program's `.ico` (`crates/launcher/kjk.ico`, built into
//! `kjk.exe`), the window's icon (`crates/game/icon.png`) and a large PNG
//! (`docs/logo/kjk-1024.png`, for avatars). The macOS app's `.iconset`
//! is rendered by `dist` as it packs.

use std::path::Path;

use image::{Rgba, RgbaImage};
use resvg::{tiny_skia, usvg};

use crate::repo_root;

/// The sizes an `.iconset` holds, each also at twice the pixels (`@2x`).
const ICONSET_SIZES: &[u32] = &[16, 32, 128, 256, 512];

/// The sizes Windows picks from (Explorer's views, the taskbar, Alt-Tab).
const ICO_SIZES: &[u32] = &[16, 24, 32, 48, 64, 128, 256];

/// The window's icon (Windows' title bar and taskbar, Linux's docks).
const WINDOW_SIZE: u32 = 256;

/// Lines drawn at the logo's own width vanish below this many pixels; they
/// thicken towards the smallest sizes so the band still reads.
const FULL_DETAIL: f32 = 96.0;

/// Rewrites the rendered copies the repo keeps from `docs/logo/kjk.svg`.
pub fn run(args: &[String]) -> Result<(), String> {
    if let Some(other) = args.first() {
        return Err(format!("icon: unknown option {other}"));
    }
    let root = repo_root();
    let ico = root.join("crates/launcher/kjk.ico");
    let frames = ICO_SIZES
        .iter()
        .map(|&size| {
            let png = encode_png(&render(size, false)?)?;
            image::codecs::ico::IcoFrame::with_encoded(
                png,
                size,
                size,
                image::ExtendedColorType::Rgba8,
            )
            .map_err(|error| format!("{}: {error}", ico.display()))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let file =
        std::fs::File::create(&ico).map_err(|error| format!("{}: {error}", ico.display()))?;
    image::codecs::ico::IcoEncoder::new(file)
        .encode_images(&frames)
        .map_err(|error| format!("{}: {error}", ico.display()))?;
    for (path, size) in [
        (root.join("crates/game/icon.png"), WINDOW_SIZE),
        (root.join("docs/logo/kjk-1024.png"), 1024),
    ] {
        save(&render(size, false)?, &path)?;
    }
    println!("icon: kjk.ico, icon.png and kjk-1024.png rendered from docs/logo/kjk.svg");
    Ok(())
}

/// Writes `<dir>/icon_<n>x<n>[@2x].png` for every size `iconutil` expects.
pub fn write_iconset(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    for &size in ICONSET_SIZES {
        for (scale, suffix) in [(1, ""), (2, "@2x")] {
            let path = dir.join(format!("icon_{size}x{size}{suffix}.png"));
            save(&render(size * scale, true)?, &path)?;
        }
    }
    Ok(())
}

/// The logo at `size` pixels square; `rounded`, inside macOS's margin and
/// rounded square, else to the edges.
fn render(size: u32, rounded: bool) -> Result<RgbaImage, String> {
    let logo = repo_root().join("docs/logo/kjk.svg");
    let source =
        std::fs::read_to_string(&logo).map_err(|error| format!("{}: {error}", logo.display()))?;
    let inner = if rounded {
        (size as f32 * (1.0 - 2.0 * MARGIN)).round() as u32
    } else {
        size
    };
    let width = (FULL_DETAIL / inner as f32).clamp(1.0, 3.0);
    let source = source.replace(
        "stroke-width=\"1\"",
        &format!("stroke-width=\"{width:.2}\""),
    );
    let tree = usvg::Tree::from_str(&source, &usvg::Options::default())
        .map_err(|error| format!("{}: {error}", logo.display()))?;
    let mut pixmap = tiny_skia::Pixmap::new(size, size).ok_or("icon: empty size")?;
    let scale = inner as f32 / tree.size().width();
    let offset = (size - inner) as f32 / 2.0;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_row(scale, 0.0, 0.0, scale, offset, offset),
        &mut pixmap.as_mut(),
    );
    Ok(RgbaImage::from_fn(size, size, |x, y| {
        let pixel = pixmap.pixel(x, y).expect("inside the pixmap").demultiply();
        let coverage = if rounded {
            square_coverage(x, y, size)
        } else {
            1.0
        };
        Rgba([
            pixel.red(),
            pixel.green(),
            pixel.blue(),
            (pixel.alpha() as f32 * coverage).round() as u8,
        ])
    }))
}

/// macOS icons leave a margin round a rounded square (of the whole width).
const MARGIN: f32 = 0.10;
const RADIUS: f32 = 0.18;

/// How much of pixel (x, y) the rounded square covers, its edge smoothed
/// over one pixel.
fn square_coverage(x: u32, y: u32, size: u32) -> f32 {
    let (u, v) = (
        (x as f32 + 0.5) / size as f32,
        (y as f32 + 0.5) / size as f32,
    );
    let half = 0.5 - MARGIN - RADIUS;
    let dx = ((u - 0.5).abs() - half).max(0.0);
    let dy = ((v - 0.5).abs() - half).max(0.0);
    let outside = ((dx * dx + dy * dy).sqrt() - RADIUS) * size as f32;
    (0.5 - outside).clamp(0.0, 1.0)
}

fn encode_png(image: &RgbaImage) -> Result<Vec<u8>, String> {
    let mut png = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|error| format!("icon: {error}"))?;
    Ok(png)
}

fn save(image: &RgbaImage, path: &Path) -> Result<(), String> {
    image
        .save(path)
        .map_err(|error| format!("{}: {error}", path.display()))
}
