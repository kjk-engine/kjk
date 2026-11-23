//! Checks of `kjk lighting pack`, on folders of bakes made here (no
//! install): which sets the pack takes, the set it ships for an area that
//! several modules share, and a set of an old format stopping it.

use std::path::{Path, PathBuf};

use kjk_game::bake_fingerprint;
use sim::probe::{Checks, check};

use super::{MODULES_K1, STORED_BACKDROP_LOG, Set, choose_sets, pack};

/// The checks, for `lighting_pack_sets` in the approved tests.
pub fn probes() -> Checks {
    vec![
        (
            "the_pack_takes_finished_b_sets_alone",
            the_pack_takes_finished_b_sets_alone(),
        ),
        (
            "the_pack_ships_the_set_of_the_module_that_gives_its_settings",
            the_pack_ships_the_set_of_the_module_that_gives_its_settings(),
        ),
        (
            "the_pack_stops_at_a_set_of_an_old_format",
            the_pack_stops_at_a_set_of_an_old_format(),
        ),
    ]
}

/// A scratch folder of bakes, empty.
fn scratch(name: &str) -> PathBuf {
    let folder = std::env::temp_dir().join(format!("kjk-pack-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).expect("the scratch folder is made");
    folder
}

/// Writes set `set` (`a` or `b`) of area `area` of module `module` in
/// the round `c1`, stamped with `format` where it is given, holding the
/// files `files`. The area's folder, which holds the bake's log.
fn set(
    root: &Path,
    (module, area, set): (&str, &str, &str),
    format: Option<u32>,
    files: &[&str],
) -> PathBuf {
    let area_folder = root.join(module).join("bake").join("c1").join(area);
    let folder = area_folder.join(set);
    std::fs::create_dir_all(&folder).expect("the set's folder is made");
    if let Some(format) = format {
        let print = bake_fingerprint::Fingerprint {
            area: area.to_string(),
            format,
            rooms: "1".repeat(64),
            placeables: "2".repeat(64),
        };
        std::fs::write(
            folder.join(bake_fingerprint::FILE),
            bake_fingerprint::text(&print),
        )
        .expect("the fingerprint is written");
    }
    for file in files {
        std::fs::write(folder.join(file), b"x").expect("a set's file is written");
    }
    area_folder
}

/// The names in the zip at `path`, sorted.
fn names_in(path: &Path) -> Result<Vec<String>, String> {
    let file = std::fs::File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut zip = ::zip::ZipArchive::new(file).map_err(|error| error.to_string())?;
    let mut names = Vec::new();
    for index in 0..zip.len() {
        let entry = zip.by_index(index).map_err(|error| error.to_string())?;
        names.push(entry.name().map_err(|error| error.to_string())?.to_string());
    }
    names.sort();
    Ok(names)
}

/// The pack takes the files the game loads from each finished set `b`.
/// It leaves out set `a`, a set with no fingerprint, and a set baked with
/// the stored backdrop before `stored.txt`. A set whose `stored.txt` names
/// its backdrop is packed.
fn the_pack_takes_finished_b_sets_alone() -> Result<String, String> {
    let root = scratch("sets");
    let result = takes_finished_b_sets(&root);
    let _ = std::fs::remove_dir_all(root);
    result
}

/// `the_pack_takes_finished_b_sets_alone` in the scratch folder `root`.
fn takes_finished_b_sets(root: &Path) -> Result<String, String> {
    let bakes = root.join("bakes");
    let format = Some(bake_fingerprint::FORMAT);
    // A finished area: its set `a` stays out, and so do the files of
    // `b` the game does not load.
    set(
        &bakes,
        ("danm13", "m13aa", "a"),
        format,
        &["m13aa_01a_lm0.png"],
    );
    set(
        &bakes,
        ("danm13", "m13aa", "b"),
        format,
        &[
            "m13aa_01a_lm0.png",
            "fill.txt",
            "probes.txt",
            "placeables.txt",
        ],
    );
    // A set with no fingerprint is not finished.
    set(
        &bakes,
        ("danm14aa", "m14aa", "b"),
        None,
        &["m14aa_01a_lm0.png"],
    );
    // A set baked with the stored backdrop before stored.txt holds
    // copies of the games' own lightmaps.
    let copied = set(
        &bakes,
        ("tar_m02ab", "tar_m02ab", "b"),
        format,
        &["tar_m02ab_lm0.png"],
    );
    std::fs::write(
        copied.join("detail-bake.txt"),
        format!("tar_m02ab\n  rooms {STORED_BACKDROP_LOG}\n"),
    )
    .map_err(|error| format!("detail-bake.txt: {error}"))?;
    // The same with stored.txt names its backdrop and is packed.
    let noted = set(
        &bakes,
        ("tar_m08aa", "tar_m08aa", "b"),
        format,
        &["tar_m08aa_lm0.png", kjk_game::stored_fill::STORED_FILE],
    );
    std::fs::write(noted.join("detail-bake.txt"), STORED_BACKDROP_LOG)
        .map_err(|error| format!("detail-bake.txt: {error}"))?;
    let out = root.join("out");
    pack("k1", &bakes, &out, "t", Some("c1"))?;
    let names = names_in(&out.join("kjk-lighting-k1-t.zip"))?;
    check(
        names
            == [
                "m13aa/fill.txt",
                "m13aa/fingerprint.txt",
                "m13aa/m13aa_01a_lm0.png",
                "m13aa/placeables.txt",
                "tar_m08aa/fingerprint.txt",
                "tar_m08aa/stored.txt",
                "tar_m08aa/tar_m08aa_lm0.png",
            ],
        format!("the zip holds {names:?}"),
    )?;
    let manifest = std::fs::read_to_string(out.join("kjk-lighting-k1-t.txt"))
        .map_err(|error| format!("the manifest: {error}"))?;
    check(
        manifest.contains("m13aa") && manifest.contains("tar_m08aa"),
        format!("the manifest leaves out a packed area: {manifest}"),
    )?;
    check(
        !manifest.contains("m14aa") && !manifest.contains("tar_m02ab"),
        format!("the manifest lists an area left out: {manifest}"),
    )
}

/// `modules-k1.txt` bakes `ebo_m40aa` and `ebo_m41aa` with the settings of
/// `ebo_m12aa`, whose area they share. The pack ships the set of
/// `ebo_m12aa` and lists the other two as left out. Two modules the list
/// does not tie together still stop the pack.
fn the_pack_ships_the_set_of_the_module_that_gives_its_settings() -> Result<String, String> {
    let root = scratch("shared");
    let result = ships_the_giver(&root);
    let _ = std::fs::remove_dir_all(root);
    result
}

/// `the_pack_ships_the_set_of_the_module_that_gives_its_settings` in the
/// scratch folder `root`.
fn ships_the_giver(root: &Path) -> Result<String, String> {
    let bakes = root.join("bakes");
    let format = Some(bake_fingerprint::FORMAT);
    for module in ["ebo_m40aa", "ebo_m12aa", "ebo_m41aa"] {
        set(&bakes, (module, "m12aa", "b"), format, &["m12aa_lm0.png"]);
    }
    let mut sets = Vec::new();
    for module in ["ebo_m40aa", "ebo_m12aa", "ebo_m41aa"] {
        let path = (bakes.join(module).join("bake").join("c1"))
            .join("m12aa")
            .join("b");
        let print = bake_fingerprint::read(&path)
            .ok_or_else(|| format!("{module}'s set has no fingerprint"))??;
        sets.push(Set {
            module: module.to_string(),
            path,
            print,
        });
    }
    let (kept, left) = choose_sets(MODULES_K1, sets)?;
    let kept: Vec<&str> = kept.iter().map(|set| set.module.as_str()).collect();
    check(kept == ["ebo_m12aa"], format!("the pack kept {kept:?}"))?;
    let left: Vec<(&str, &str)> = (left.iter())
        .map(|(set, from)| (set.module.as_str(), from.as_str()))
        .collect();
    check(
        left == [("ebo_m40aa", "ebo_m12aa"), ("ebo_m41aa", "ebo_m12aa")],
        format!("the pack left out {left:?}"),
    )?;
    // Two modules the list does not tie together still stop the pack.
    set(&bakes, ("danm13", "m12aa", "b"), format, &["m12aa_lm0.png"]);
    match pack("k1", &bakes, &root.join("out"), "t", Some("c1")) {
        Ok(()) => Err("the pack went on with danm13's set of m12aa".to_string()),
        Err(refused) => check(refused.contains("area m12aa"), refused),
    }
}

/// A set stamped with format 1 stops the pack before it writes anything.
fn the_pack_stops_at_a_set_of_an_old_format() -> Result<String, String> {
    let root = scratch("old");
    let bakes = root.join("bakes");
    set(
        &bakes,
        ("danm13", "m13aa", "b"),
        Some(1),
        &["m13aa_01a_lm0.png"],
    );
    let packed = pack("k1", &bakes, &root.join("out"), "t", None);
    let wrote = root.join("out").exists();
    let _ = std::fs::remove_dir_all(root);
    match packed {
        Ok(()) => Err("the pack went on with a set of format 1".to_string()),
        Err(refused) => check(
            refused.contains("format 1") && !wrote,
            format!("{refused}; the out folder was made: {wrote}"),
        ),
    }
}
