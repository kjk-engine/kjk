//! Checks of how the viewer takes a lighting bake, on verdicts and
//! lightmaps made here (no install). The viewer picks one set of an area's
//! bakes by their verdicts. It adds the stored fill to a baked lightmap in
//! linear light, and it raises a texel darker than its floor to the floor.

use asset_odyssey::texture::TextureIr;
use bevy::math::Vec3;

use super::{add_stored_fill, chosen_bake};
use crate::bake_fingerprint::Verdict;
use crate::probe_kit::{Checks, check};

/// The checks, for `lighting_bake_in_the_viewer` in the approved tests.
pub fn probes() -> Checks {
    vec![
        (
            "a_whole_bake_wins_over_lightmaps_only_and_the_later_wins",
            a_whole_bake_wins_over_lightmaps_only_and_the_later_wins(),
        ),
        (
            "the_stored_fill_adds_to_the_lamps_light",
            the_stored_fill_adds_to_the_lamps_light(),
        ),
        (
            "the_floor_is_the_least_a_texel_shows",
            the_floor_is_the_least_a_texel_shows(),
        ),
    ]
}

/// No set is chosen when every set is refused. A set the game may take
/// whole wins over one whose placeables differ, and of two alike the later
/// one wins.
fn a_whole_bake_wins_over_lightmaps_only_and_the_later_wins() -> Result<String, String> {
    let refused = || Verdict::Refused("the rooms differ".into());
    let cases = [
        (chosen_bake(&[]), None),
        (chosen_bake(&[(0, refused()), (1, refused())]), None),
        (
            chosen_bake(&[(0, Verdict::Whole), (1, Verdict::LightmapsOnly)]),
            Some(0),
        ),
        (
            chosen_bake(&[(2, Verdict::LightmapsOnly), (5, Verdict::LightmapsOnly)]),
            Some(5),
        ),
        (
            chosen_bake(&[(1, Verdict::Whole), (3, refused()), (4, Verdict::Whole)]),
            Some(4),
        ),
    ];
    let wrong: Vec<_> = cases.iter().filter(|(got, want)| got != want).collect();
    check(
        wrong.is_empty(),
        format!("chosen and wanted where they differ: {wrong:?}"),
    )
}

/// A lightmap of one texel of grey `level` (0 to 255).
fn one_texel(level: u8) -> TextureIr {
    TextureIr {
        width: 1,
        height: 1,
        encoding: asset_odyssey::texture::Encoding::Rgba8,
        mips: vec![vec![level, level, level, 255]],
        cube: false,
        frames: 1,
        facts: Default::default(),
        faces: Vec::new(),
        frame_mips: Vec::new(),
    }
}

/// The baked level `baked` (0 to 255) after `add_stored_fill` with the
/// stored grey `stored`. Fails when the texel's red and green differ, or
/// when the sun's share in its alpha is not kept.
fn filled(baked: u8, stored: u8, fill: f32, floor: bool) -> Result<u8, String> {
    let mut picture = image::RgbaImage::from_pixel(1, 1, image::Rgba([baked, baked, baked, 77]));
    add_stored_fill(&mut picture, &one_texel(stored), fill, floor);
    let pixel = picture.get_pixel(0, 0);
    check(
        pixel[0] == pixel[1],
        format!("red {} and green {} differ", pixel[0], pixel[1]),
    )?;
    check(
        pixel[3] == 77,
        format!("the sun's share in the alpha is {}, not 77", pixel[3]),
    )?;
    Ok(pixel[0])
}

/// The byte the bake writes for linear light `light` (`level_of`, at
/// half its level).
fn byte_of(light: f32) -> u8 {
    let level = crate::stored_fill::level_of(Vec3::splat(light)).x;
    (level / crate::stored_fill::GAIN * 255.0)
        .clamp(0.0, 255.0)
        .round() as u8
}

/// The linear light of a stored grey (0 to 255), as `add_stored_fill`
/// reads it.
fn stored_light(stored: u8) -> f32 {
    let texels = crate::stored_fill::StoredTexels::of(&one_texel(stored))
        .expect("a texel of one grey reads");
    texels.at(0.5, 0.5).x.powf(crate::stored_fill::GAMMA)
}

/// The linear light of a baked byte.
fn baked_light(baked: u8) -> f32 {
    let level = f32::from(baked) / 255.0 * crate::stored_fill::GAIN;
    crate::stored_fill::linear_of(Vec3::splat(level)).x
}

/// No fill, or a black stored texel, leaves the lamps' light. A room with
/// no lamp light takes the stored light times the fill. The two add in
/// linear light.
fn the_stored_fill_adds_to_the_lamps_light() -> Result<String, String> {
    let no_fill = filled(60, 200, 0.0, false)?;
    check(no_fill == 60, format!("no fill gives {no_fill}, not 60"))?;
    let black = filled(60, 0, 0.5, false)?;
    check(
        black == 60,
        format!("a black stored texel gives {black}, not 60"),
    )?;
    let stored_alone = filled(0, 120, 0.5, false)?;
    let want = byte_of(stored_light(120) * 0.5);
    check(
        stored_alone == want,
        format!("no lamp light gives {stored_alone}, not {want}"),
    )?;
    let both = filled(60, 120, 0.5, false)?;
    let want = byte_of(baked_light(60) + stored_light(120) * 0.5);
    check(both == want, format!("both give {both}, not {want}"))?;
    check(
        both > 60 && both > stored_alone,
        format!("both give {both}, not more than 60 and {stored_alone}"),
    )
}

/// A texel brighter than its floor keeps its own light. A darker one is
/// raised to the floor, and nothing is added.
fn the_floor_is_the_least_a_texel_shows() -> Result<String, String> {
    let bright = filled(120, 60, 0.2, true)?;
    check(
        bright == 120,
        format!("a bright texel gives {bright}, not 120"),
    )?;
    let raised = filled(0, 120, 0.2, true)?;
    let want = byte_of(stored_light(120) * 0.2);
    check(
        raised == want,
        format!("a dark texel gives {raised}, not {want}"),
    )?;
    let one = filled(1, 120, 0.2, true)?;
    check(
        one == raised,
        format!("a texel of 1 gives {one}, not the floor {raised}"),
    )
}
