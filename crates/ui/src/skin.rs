//! The screens' skin: KOTOR 2's layouts and art over a KOTOR 1 campaign,
//! recoloured to KOTOR 1's colours.
//!
//! A screen names its KOTOR 1 layout; KOTOR 2's comes in its place (KOTOR
//! 2 is required). A layout's own textures, fonts and words come from the
//! install it came from; what a screen puts in it (portraits, icons, item
//! names) from the campaign's.

use crate::paint::Palette;

/// Where a layout, and what it names, is read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Source {
    #[default]
    Campaign,
    Skin,
}

/// KOTOR 2's layouts whose names are not KOTOR 1's with `_p` (`moved`):
/// reone's layout lookup names the same two screens apart (the main
/// menu, the party selection); KOTOR 2 also shortened one options panel's
/// and the area transition panel's.
const RENAMED: &[(&str, &str)] = &[
    ("mainmenu16x12", "mainmenu8x6_p"),
    ("partyselection", "partyselect_p"),
    ("optgraphicsadv", "optgraphicadv_p"),
    ("areatransition", "areatrans_p"),
];

/// Layouts that hold the campaign's own world, read from the campaign and
/// drawn with its art: the galaxy map's buttons are its planets (KOTOR 2's
/// layout holds KOTOR 2's).
const OWN: &[&str] = &["galaxymap"];

/// The campaign's own layout is drawn for `name` (`OWN`).
pub fn own(name: &str) -> bool {
    OWN.contains(&name)
}

/// The campaign's logo, in place of KOTOR 2's on the main menu and the
/// loading screen.
pub const CAMPAIGN_LOGO: &str = "logo_sw_02";

/// The skin's layout in place of the campaign's `name`: KOTOR 2 names its
/// layouts as KOTOR 1's with `_p` (as reone does), but for
/// those `RENAMED`. A layout drawn under a screen's (`under`) is read from
/// where the screen's own came from.
pub fn moved(name: &str) -> String {
    RENAMED
        .iter()
        .find(|(campaign, _)| *campaign == name)
        .map_or_else(|| format!("{name}_p"), |(_, skin)| skin.to_string())
}

fn hsv([r, g, b]: [f32; 3]) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let range = max - r.min(g).min(b);
    let hue = if range <= f32::EPSILON {
        0.0
    } else if max == r {
        60.0 * ((g - b) / range).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / range + 2.0)
    } else {
        60.0 * ((r - g) / range + 4.0)
    };
    let saturation = if max <= f32::EPSILON {
        0.0
    } else {
        range / max
    };
    (hue, saturation, max)
}

fn rgb(hue: f32, saturation: f32, value: f32) -> [f32; 3] {
    let (s, v) = (saturation.clamp(0.0, 1.0), value.clamp(0.0, 1.0));
    let chroma = v * s;
    let x = chroma * (1.0 - ((hue / 60.0).rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match (hue.rem_euclid(360.0) / 60.0) as u32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = v - chroma;
    [r + m, g + m, b + m]
}

/// KOTOR 2's art in KOTOR 1's colours ("blue steel", the owner's pick of
/// three mock-ups): brass and gold become blue metal, KOTOR 2's greens and
/// teals KOTOR 1's blue, the near-black teal of its panels KOTOR 1's navy;
/// greys, whites and reds stay.
pub fn recolour(color: [f32; 3]) -> [f32; 3] {
    let (hue, saturation, value) = hsv(color);
    if (10.0..=75.0).contains(&hue) && saturation > 0.12 {
        return rgb(210.0, saturation * 0.75, value * 0.85);
    }
    if hue > 75.0 && hue <= 200.0 && saturation > 0.15 {
        if value < 0.25 {
            return rgb(238.0, 1.0, (value * 1.25).min(0.12));
        }
        return rgb(200.0, saturation.max(0.85), value.max(0.6));
    }
    color
}

/// The palette KOTOR 2's layouts set (their most used colours after white:
/// base, highlight, disabled), beside its engine's own (`Palette::TSL`).
const TSL_LAYOUT: Palette = Palette {
    base: [0.101_96, 0.698_04, 0.549_02],
    hilight: [0.8, 0.8, 0.698_04],
    disabled: [0.050_98, 0.349_02, 0.270_59],
};

/// The colours KOTOR 2's own code tints with (its text, highlight and
/// dark green): a feat chain's cells.
pub const TSL_ENGINE: Palette = Palette {
    base: [0.1, 0.7, 0.55],
    hilight: [0.7, 0.7, 0.6],
    disabled: [0.05, 0.35, 0.27],
};

/// A layout's colour in KOTOR 1's: KOTOR 2's palette onto KOTOR 1's (its
/// pale highlight becomes KOTOR 1's yellow), any other as art (a neutral
/// grey, as a progress bar's tint, stays grey).
pub fn recolour_layout(color: [f32; 3]) -> [f32; 3] {
    let near = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.03);
    let k1 = Palette::K1;
    [Palette::TSL, TSL_LAYOUT, TSL_ENGINE]
        .into_iter()
        .flat_map(|tsl| {
            [
                (tsl.base, k1.base),
                (tsl.hilight, k1.hilight),
                (tsl.disabled, k1.disabled),
            ]
        })
        .find(|(from, _)| near(color, *from))
        .map_or_else(|| recolour(color), |(_, to)| to)
}

/// `recolour` over RGBA bytes.
pub fn recolour_pixels(rgba: &mut [u8]) {
    for pixel in rgba.as_chunks_mut::<4>().0 {
        let color = [0, 1, 2].map(|k| f32::from(pixel[k]) / 255.0);
        for (byte, value) in pixel.iter_mut().zip(recolour(color)) {
            *byte = (value.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
}
