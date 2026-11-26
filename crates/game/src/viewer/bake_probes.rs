//! Checks of how the viewer takes a lighting bake, on verdicts and
//! lightmaps made here (no install). The viewer picks one set of an area's
//! bakes by their verdicts. It adds the stored fill to a baked lightmap in
//! linear light, and it raises a texel darker than its floor to the floor.
//! A set with exact lines is taken lightmap by lightmap and placeable by
//! placeable.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use asset_odyssey::texture::TextureIr;
use bevy::math::Vec3;

use super::{
    BakeFolder, FolderUse, LitPlaceables, Picture, Used, add_stored_fill, chosen_bake,
    detail_baked, judge, light_numbers, lightmap_source,
};
use crate::bake_fingerprint::{
    Current, Facing, Fingerprint, Fit, Placed, Verdict, fit, stamp as stamp_of,
};
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

/// The checks of sets with exact lines, for
/// `lighting_bake_by_lightmap_in_the_viewer` in the approved tests.
pub fn fit_probes() -> Checks {
    vec![
        (
            "a_room_lightmap_without_a_fitting_png_shows_its_stored_light",
            a_room_lightmap_without_a_fitting_png_shows_its_stored_light(),
        ),
        (
            "an_area_where_no_lightmap_matches_is_not_baked",
            an_area_where_no_lightmap_matches_is_not_baked(),
        ),
        (
            "a_placeable_keeps_its_light_by_its_place",
            a_placeable_keeps_its_light_by_its_place(),
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

/// A set of one area, in a folder that is never read, with a PNG of each
/// lightmap of `pngs`, the lightmaps of `stored` in `stored.txt`, and a fill
/// for every lightmap of both.
fn folder_of(pngs: &[&str], stored: &[&str]) -> BakeFolder {
    let path = PathBuf::from("/nowhere/101per/a");
    let names = || pngs.iter().chain(stored).map(|name| (*name).to_string());
    BakeFolder {
        lightmaps: (pngs.iter())
            .map(|name| ((*name).to_string(), path.join(format!("{name}.png"))))
            .collect(),
        fills: names().map(|name| (name, 0.5)).collect(),
        floors: BTreeMap::new(),
        unlit: false,
        stored: stored.iter().map(|name| (*name).to_string()).collect(),
        stamp: None,
        path,
    }
}

/// The fit of a set whose PNGs `matching` match and `differing` do not,
/// with the backdrops `backdrops`.
fn fit_of(matching: &[&str], differing: &[&str], backdrops: &[&str]) -> Fit {
    let set = |names: &[&str]| names.iter().map(|name| (*name).to_string()).collect();
    Fit {
        matching: set(matching),
        differing: (differing.iter())
            .map(|name| ((*name).to_string(), BTreeSet::new()))
            .collect(),
        backdrops: set(backdrops),
        ..Fit::default()
    }
}

/// The lightmaps of the area's room meshes in the cases below.
const ROOM_LIGHTMAPS: [&str; 6] = ["lm0", "lm1", "added", "sky", "stored", "renamed"];

/// In a set with exact lines, a lightmap whose PNG matches shows its PNG. A
/// room lightmap whose PNG does not match, one that a mod adds, and one
/// that a mod renames are drawn from the stored lightmap at half its light,
/// which the shader doubles back to the stored light. They take no fill,
/// although `fill.txt` lists one of them. A backdrop the bake left alone is
/// kept as it is, and a `stored.txt` backdrop is drawn as before. A PNG that
/// no room mesh draws shows as it did, and a set without exact lines leaves
/// a lightmap it does not know alone.
fn a_room_lightmap_without_a_fitting_png_shows_its_stored_light() -> Result<String, String> {
    let folder = folder_of(&["lm0", "lm1", "grass0"], &["stored"]);
    let fitted = Arc::new(fit_of(&["lm0"], &["lm1", "grass0"], &["sky", "stored"]));
    let used = |fit: Option<Arc<Fit>>| {
        vec![Used {
            folder: &folder,
            using: FolderUse::Whole,
            fit,
        }]
    };
    let (with_fit, without_fit) = (used(Some(fitted)), used(None));
    let rooms: BTreeSet<String> = ROOM_LIGHTMAPS.map(String::from).into();
    let png = |name: &str| folder.lightmaps.get(name).map(|path| Picture::Png(path));
    let cases = [
        ("a matching PNG", &with_fit, "lm0", png("lm0")),
        (
            "a PNG that differs",
            &with_fit,
            "lm1",
            Some(Picture::FallBack),
        ),
        (
            "a lightmap a mod adds",
            &with_fit,
            "added",
            Some(Picture::FallBack),
        ),
        (
            "a lightmap a mod renames",
            &with_fit,
            "renamed",
            Some(Picture::FallBack),
        ),
        ("an approved backdrop", &with_fit, "sky", None),
        (
            "a stored.txt backdrop",
            &with_fit,
            "stored",
            Some(Picture::Stored),
        ),
        (
            "a PNG no room mesh draws",
            &with_fit,
            "grass0",
            png("grass0"),
        ),
        ("a set without exact lines", &without_fit, "added", None),
    ];
    let mut wrong = Vec::new();
    for (what, used, name, want) in cases {
        let found = lightmap_source(used, &rooms, name).map(|(_, picture)| picture);
        if found != want {
            wrong.push(format!("{what}: {found:?}, not {want:?}"));
        }
    }
    // The pictures drawn at their stored light are the backdrop's bytes
    // exactly, with no fill, although the fill of lm1 is 0.5.
    let stored = one_texel(200);
    let backdrop = crate::stored_fill::backdrop(&stored).ok_or("the backdrop does not decode")?;
    for name in ["lm1", "added"] {
        let drawn = detail_baked(&with_fit, &rooms, name, &stored).map(|texture| texture.mips);
        if drawn.as_ref() != Some(&vec![backdrop.clone()]) {
            wrong.push(format!("{name} is drawn as {drawn:?}, not {backdrop:?}"));
        }
    }
    let kept = detail_baked(&with_fit, &rooms, "sky", &stored);
    if kept.is_some() {
        wrong.push("the approved backdrop is replaced".into());
    }
    check(wrong.is_empty(), wrong.join("; "))
}

/// A lit placeable `instance` of model `model` at `x` along X, at the
/// bearing `bearing`.
fn placed(instance: usize, model: &str, x: f32, bearing: f32) -> Placed {
    Placed {
        instance,
        model: model.into(),
        position: [x.to_bits(), 5f32.to_bits(), 1f32.to_bits()],
        facing: Facing::Bearing(bearing.to_bits()),
    }
}

/// The player's files of area 101per: the lightmaps `lightmaps` with their
/// digests, and the lit placeables `placed` of models `crate01` and
/// `table01`.
fn files(lightmaps: &[(&str, &str)], placed: Vec<Placed>) -> Current {
    Current {
        print: Fingerprint {
            area: "101per".into(),
            format: crate::bake_fingerprint::FORMAT,
            rooms: "rooms".into(),
            placeables: "placeables".into(),
            exact: None,
        },
        lightmaps: (lightmaps.iter())
            .map(|(name, digest)| ((*name).to_string(), (*digest).to_string()))
            .collect(),
        rooms_of: BTreeMap::new(),
        named: lightmaps
            .iter()
            .map(|(name, _)| (*name).to_string())
            .collect(),
        placed,
        models: [("crate01", "c"), ("table01", "t")]
            .map(|(name, digest)| (name.to_string(), digest.to_string()))
            .into(),
    }
}

/// A set baked over lightmaps `lm0` and `lm1` and no placeables is taken in
/// full over the same files, and in part when one lightmap differs. It is
/// refused when no lightmap matches, so the area is drawn with the game's
/// own lighting.
fn an_area_where_no_lightmap_matches_is_not_baked() -> Result<String, String> {
    let baked = files(&[("lm0", "a"), ("lm1", "b")], Vec::new());
    let (stamped, _) = stamp_of(&baked, &["lm0", "lm1"].map(String::from).into());
    let cases = [
        (
            "the same files",
            files(&[("lm0", "a"), ("lm1", "b")], Vec::new()),
            "whole",
        ),
        (
            "lm1 moved",
            files(&[("lm0", "a"), ("lm1", "x")], Vec::new()),
            "part",
        ),
        (
            "both moved",
            files(&[("lm0", "x"), ("lm1", "y")], Vec::new()),
            "refused",
        ),
    ];
    let mut wrong = Vec::new();
    for (what, current, want) in cases {
        let judged = judge(&stamped, &current);
        let found = match (&judged.verdict, &judged.fit) {
            (Verdict::Whole, Some(_)) => "whole",
            (Verdict::LightmapsOnly, Some(_)) => "part",
            (Verdict::Refused(_), None) => "refused",
            _ => "something else",
        };
        if found != want {
            wrong.push(format!("{what}: {found} ({judged:?}), not {want}"));
        }
    }
    check(wrong.is_empty(), wrong.join("; "))
}

/// The scene's lit placeables, by object, of the models `models`.
fn lit_of(models: &[&str]) -> LitPlaceables {
    (models.iter().enumerate())
        .map(|(object, name)| {
            (
                object as u32,
                (object, (*name).to_string(), Arc::new(vec![true])),
            )
        })
        .collect()
}

/// A crate that a mod adds first shifts the instance numbers of the others.
/// They keep their baked light under their old numbers, and the added crate
/// keeps its usual light. A table turned in place loses its baked light. A
/// set without exact lines reads each placeable under its own number.
fn a_placeable_keeps_its_light_by_its_place() -> Result<String, String> {
    let baked = files(
        &[("lm0", "a")],
        vec![
            placed(0, "crate01", 1.0, 0.5),
            placed(1, "table01", 2.0, 0.5),
        ],
    );
    let (stamped, _) = stamp_of(&baked, &["lm0"].map(String::from).into());
    let numbers = |placeables: Vec<Placed>, models: &[&str], exact: bool| {
        let current = files(&[("lm0", "a")], placeables);
        let fitted = fit(&stamped, &current).and_then(Result::ok);
        let lit = lit_of(models);
        let numbers = light_numbers(&lit, fitted.as_ref().filter(|_| exact));
        (numbers.into_iter())
            .map(|(baked, (object, _))| (baked, object))
            .collect::<Vec<(u32, u32)>>()
    };
    let cases = [
        (
            "a crate added first",
            numbers(
                vec![
                    placed(0, "crate01", 9.0, 0.5),
                    placed(1, "crate01", 1.0, 0.5),
                    placed(2, "table01", 2.0, 0.5),
                ],
                &["crate01", "crate01", "table01"],
                true,
            ),
            vec![(0, 1), (1, 2)],
        ),
        (
            "the table turned in place",
            numbers(
                vec![
                    placed(0, "crate01", 1.0, 0.5),
                    placed(1, "table01", 2.0, 0.7),
                ],
                &["crate01", "table01"],
                true,
            ),
            vec![(0, 0)],
        ),
        (
            "a set without exact lines",
            numbers(
                vec![
                    placed(0, "crate01", 9.0, 0.5),
                    placed(1, "crate01", 1.0, 0.5),
                ],
                &["crate01", "crate01"],
                false,
            ),
            vec![(0, 0), (1, 1)],
        ),
    ];
    let wrong: Vec<String> = (cases.iter())
        .filter(|(_, found, want)| found != want)
        .map(|(what, found, want)| format!("{what}: {found:?}, not {want:?}"))
        .collect();
    check(wrong.is_empty(), wrong.join("; "))
}
