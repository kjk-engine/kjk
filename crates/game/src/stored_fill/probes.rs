//! Checks of the stored fill's files and numbers, on words, lightmaps and
//! a folder made here (no install).

use asset_odyssey::texture::{Encoding, TextureIr};
use bevy::math::Vec3;

use super::{
    STORED_FILE, StoredMap, backdrop, map_line, parse_word, stored_names, stored_text, with_fill,
    word,
};
use crate::probe_kit::{Checks, check};

/// The checks, for `stored_light_fill` in the approved tests.
pub fn probes() -> Checks {
    vec![
        ("a_word_reads_back", a_word_reads_back()),
        (
            "a_stored_backdrop_is_named_and_halved",
            a_stored_backdrop_is_named_and_halved(),
        ),
        ("no_fill_keeps_the_level", no_fill_keeps_the_level()),
    ]
}

/// A word `@<map>:<texel>` reads back, and a number is no word. A map's
/// line reads back with its name in lower case, and its texel 513 is the
/// middle of the second texel of the second row.
fn a_word_reads_back() -> Result<String, String> {
    let read = parse_word(&word(3, 12345));
    check(
        read == Some((3, 12345)),
        format!("the word read as {read:?}"),
    )?;
    let number = parse_word("0.1234");
    check(number.is_none(), format!("a number read as {number:?}"))?;
    let map = StoredMap::parse(&map_line("411DXNA_lm0", 512, 256));
    let want = StoredMap {
        name: "411dxna_lm0".into(),
        width: 512,
        height: 256,
    };
    check(
        map.as_ref() == Some(&want),
        format!("the map's line read as {map:?}"),
    )?;
    let uv = map.and_then(|map| map.uv(513));
    check(
        uv == Some((1.5 / 512.0, 1.5 / 256.0)),
        format!("texel 513 is at {uv:?}"),
    )
}

/// The lightmaps `stored.txt` names read back in lower case, and a folder
/// without it names none. The backdrop of a stored lightmap is each colour
/// at half, rounded, with an alpha of 255, as the bake wrote it as a PNG.
fn a_stored_backdrop_is_named_and_halved() -> Result<String, String> {
    let folder = std::env::temp_dir().join(format!("kjk-stored-{}", std::process::id()));
    std::fs::create_dir_all(&folder).expect("the scratch folder is made");
    std::fs::write(
        folder.join(STORED_FILE),
        stored_text(["M02AB_05a_lm0", "m02ab_06b_lm1"]),
    )
    .expect("stored.txt is written");
    let names: Vec<String> = stored_names(&folder).into_iter().collect();
    let none = stored_names(&folder.join("none"));
    let _ = std::fs::remove_dir_all(&folder);
    check(
        names == ["m02ab_05a_lm0", "m02ab_06b_lm1"],
        format!("stored.txt names {names:?}"),
    )?;
    check(
        none.is_empty(),
        format!("a folder without it names {none:?}"),
    )?;
    let stored = TextureIr {
        width: 2,
        height: 1,
        encoding: Encoding::Rgba8,
        mips: vec![vec![0, 2, 4, 9, 254, 200, 128, 0]],
        cube: false,
        frames: 1,
        facts: Default::default(),
        faces: Vec::new(),
        frame_mips: Vec::new(),
    };
    let halved = backdrop(&stored);
    check(
        halved == Some(vec![0, 1, 2, 255, 127, 100, 64, 255]),
        format!("the backdrop is {halved:?}"),
    )
}

/// A room with no fill and no floor keeps its level.
fn no_fill_keeps_the_level() -> Result<String, String> {
    let level = Vec3::new(0.1, 0.2, 0.3);
    let kept = with_fill(level, Vec3::ONE, None, None);
    check(
        (kept - level).abs().max_element() < 1e-5,
        format!("{level} became {kept}"),
    )
}
