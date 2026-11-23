//! Checks of the lighting download, on manifests, game folders and zips
//! made here (no install): the manifest reads back, nine areas in ten must
//! match, the check's key follows the files a bake reads, and an install
//! unpacks into the store and replaces the last one.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use store::Store;
use vfs::Game;

use super::{
    Archive, Area, FallBack, MANIFEST, Manifest, Report, allowed, digest_of, files_key, folder,
    folders_in, install, needs, remove,
};
use crate::bake_fingerprint;
use crate::probe_kit::{Checks, check};

/// The checks, for `lighting_downloads` in the approved tests.
pub fn probes() -> Checks {
    vec![
        ("the_manifest_reads_back", the_manifest_reads_back()),
        (
            "the_install_needs_nine_areas_in_ten",
            the_install_needs_nine_areas_in_ten(),
        ),
        (
            "the_checks_key_follows_the_mods_and_not_the_voices",
            the_checks_key_follows_the_mods_and_not_the_voices(),
        ),
        (
            "an_install_unpacks_into_the_store_and_replaces_the_last",
            an_install_unpacks_into_the_store_and_replaces_the_last(),
        ),
    ]
}

fn manifest() -> Manifest {
    Manifest {
        game: Game::Kotor2,
        format: bake_fingerprint::FORMAT,
        needs: needs(Game::Kotor2).to_string(),
        archive: Archive {
            file: "kjk-lighting-tsl-1.zip".into(),
            blake3: "0".repeat(64),
            bytes: 0,
        },
        areas: vec![
            Area {
                area: "101per".into(),
                module: "101per".into(),
                rooms: "0123456789abcdef0123456789abcdef".into(),
                placeables: "fedcba9876543210fedcba9876543210".into(),
            },
            Area {
                area: "411dxn".into(),
                module: "411dxn".into(),
                rooms: "00112233445566778899aabbccddeeff".into(),
                placeables: "ffeeddccbbaa99887766554433221100".into(),
            },
        ],
    }
}

/// A scratch folder of its own under the system's temporary folder.
fn scratch(name: &str) -> PathBuf {
    let folder = std::env::temp_dir().join(format!("kjk-lighting-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).expect("the scratch folder is made");
    folder
}

/// A KOTOR 2 and a KOTOR 1 manifest read back from their text. A manifest
/// of another game, one that names the wrong patch, and one with no game
/// do not read.
fn the_manifest_reads_back() -> Result<String, String> {
    let manifest = manifest();
    let read = Manifest::parse(&manifest.text());
    check(
        read.as_ref() == Ok(&manifest),
        format!("the KOTOR 2 manifest read as {read:?}"),
    )?;
    let kotor1 = Manifest {
        game: Game::Kotor1,
        needs: needs(Game::Kotor1).to_string(),
        ..manifest
    };
    let read = Manifest::parse(&kotor1.text());
    check(
        read.as_ref() == Ok(&kotor1),
        format!("the KOTOR 1 manifest read as {read:?}"),
    )?;
    let wrong: Vec<&str> = [
        "game ja\nformat 2",
        "game tsl\nformat 2\nneeds TSLRCM\n",
        "area 101per",
    ]
    .into_iter()
    .filter(|text| Manifest::parse(text).is_ok())
    .collect();
    check(
        wrong.is_empty(),
        format!("these texts read as manifests: {wrong:?}"),
    )
}

/// A download installs when 9 areas in 10 match, and not with fewer. Of
/// 171 areas, 154 must match. A download of no areas does not install.
/// A report of 7 matched areas and 2 whose placeables differ counts 9
/// matching, and its refusal names the game and its patch.
fn the_install_needs_nine_areas_in_ten() -> Result<String, String> {
    let cases = [
        ((9, 10), true),
        ((10, 10), true),
        ((8, 10), false),
        ((154, 171), true),
        ((153, 171), false),
        ((0, 0), false),
    ];
    let wrong: Vec<_> = (cases.iter())
        .filter(|((matching, total), want)| allowed(*matching, *total) != *want)
        .collect();
    check(
        wrong.is_empty(),
        format!("judged wrong (matching, total): {wrong:?}"),
    )?;
    let report = Report {
        game: "tsl".into(),
        needs: "TSLRCM".into(),
        areas: 10,
        matched: (0..7).map(|n| n.to_string()).collect(),
        placeables_differ: vec!["7".into(), "8".into()],
        fall_back: vec![FallBack {
            area: "9".into(),
            why: "rooms".into(),
        }],
        allowed: true,
        cached: false,
    };
    check(
        report.matching() == 9,
        format!("the report counts {} matching, not 9", report.matching()),
    )?;
    check(
        report.refusal().is_none(),
        format!("an allowed report refuses: {:?}", report.refusal()),
    )?;
    let refused = Report {
        allowed: false,
        ..report
    };
    let refusal = refused
        .refusal()
        .ok_or("a report not allowed has no refusal")?;
    check(refusal.contains("KOTOR 2 with TSLRCM"), refusal)
}

/// The check's key stays the same when nothing changes and when a voice
/// line is added. It changes when a mod's texture is added or changed,
/// and it is the first key again once the texture is removed.
fn the_checks_key_follows_the_mods_and_not_the_voices() -> Result<String, String> {
    let root = scratch("files-key");
    std::fs::create_dir_all(root.join("override")).expect("override is made");
    std::fs::create_dir_all(root.join("streamvoice")).expect("streamvoice is made");
    std::fs::write(root.join("chitin.key"), b"key").expect("chitin.key is written");
    let key = || {
        files_key(
            Game::Kotor1,
            &vfs::DirIndex::build(&root).expect("the folder is indexed"),
        )
    };
    let before = key();
    let again = key();
    // A voice line is no file a bake reads.
    std::fs::write(root.join("streamvoice").join("line.wav"), b"voice")
        .expect("the voice line is written");
    let voiced = key();
    // A mod's texture is.
    let texture = root.join("override").join("m13aa_01a_lm0.tga");
    std::fs::write(&texture, b"tga").expect("the texture is written");
    let added = key();
    std::fs::write(&texture, b"a longer tga").expect("the texture is changed");
    let changed = key();
    std::fs::remove_file(&texture).expect("the texture is removed");
    let removed = key();
    let _ = std::fs::remove_dir_all(&root);
    check(
        again == before
            && voiced == before
            && added != before
            && changed != added
            && removed == before,
        format!(
            "keys: first {before}, again {again}, with a voice line {voiced}, with a texture {added}, with it changed {changed}, with it removed {removed}"
        ),
    )
}

/// A zip of `entries` (name and bytes) at `path`.
fn zip(path: &Path, entries: &[(&str, &[u8])]) {
    let mut zip = ::zip::ZipWriter::new(std::fs::File::create(path).expect("the zip is made"));
    let options = ::zip::write::SimpleFileOptions::default()
        .compression_method(::zip::CompressionMethod::Stored);
    for (name, bytes) in entries {
        zip.start_file(*name, options).expect("an entry starts");
        zip.write_all(bytes).expect("an entry is written");
    }
    zip.finish().expect("the zip is finished");
}

/// A download of the wrong size or BLAKE3 is refused before anything is
/// written. A right one unpacks into the store's lighting folder, area by
/// area, with its manifest, and a second install replaces the first
/// whole. The store's gc leaves it alone. A zip with a file outside the
/// areas' folders is refused, and the download can be removed.
fn an_install_unpacks_into_the_store_and_replaces_the_last() -> Result<String, String> {
    let root = scratch("install");
    let result = install_in(&root);
    let _ = std::fs::remove_dir_all(&root);
    result
}

/// `an_install_unpacks_into_the_store_and_replaces_the_last` in the
/// scratch folder `root`.
fn install_in(root: &Path) -> Result<String, String> {
    let store = Store::at(root.join("store"));
    let mut manifest = manifest();
    let prints: Vec<String> = (manifest.areas.iter())
        .map(|area| bake_fingerprint::text(&area.stamp(manifest.format)))
        .collect();
    let path = root.join("download.zip");
    zip(
        &path,
        &[
            ("101per/fingerprint.txt", prints[0].as_bytes()),
            ("101per/101per_lm0.png", b"png"),
            ("411dxn/fingerprint.txt", prints[1].as_bytes()),
            ("411dxn/fill.txt", b"411dxna_lm0 0.5\n"),
        ],
    );
    // The wrong size or BLAKE3 is refused before anything is written.
    check(
        install(&store, &manifest, &path, &mut |_| {}).is_err(),
        "a download of the wrong size and BLAKE3 installed".into(),
    )?;
    check(
        !folder(&store, Game::Kotor2).exists(),
        "a refused download left a folder".into(),
    )?;
    let (blake3, bytes) = digest_of(&path)?;
    manifest.archive = Archive {
        file: "download.zip".into(),
        blake3,
        bytes,
    };
    let mut seen = Vec::new();
    let installed = install(&store, &manifest, &path, &mut |progress| {
        seen.push(progress.area.to_string());
    })?;
    check(
        seen == ["101per", "411dxn"],
        format!("the install went through {seen:?}"),
    )?;
    let want = root.join("store").join("lighting").join("tsl");
    check(
        installed == want,
        format!(
            "installed in {}, not {}",
            installed.display(),
            want.display()
        ),
    )?;
    check(
        installed.join("101per").join("101per_lm0.png").is_file(),
        "101per's lightmap was not unpacked".into(),
    )?;
    let read = Manifest::read(&installed.join(MANIFEST));
    check(
        read.as_ref() == Ok(&manifest),
        format!("the installed manifest reads as {read:?}"),
    )?;
    let count = folders_in(&store).len();
    check(count == 2, format!("{count} area folders, not 2"))?;
    // A second install replaces the first whole.
    std::fs::write(installed.join("101per").join("stale.png"), b"x")
        .map_err(|error| format!("stale.png: {error}"))?;
    install(&store, &manifest, &path, &mut |_| {})?;
    check(
        !installed.join("101per").join("stale.png").exists(),
        "the second install kept a file of the first".into(),
    )?;
    check(
        !installed.with_extension("partial").exists() && !installed.with_extension("old").exists(),
        "the second install left its partial or old folder".into(),
    )?;
    // The store's gc leaves it alone, however old.
    store
        .gc(std::time::Duration::ZERO, false)
        .map_err(|error| format!("gc: {error}"))?;
    let count = folders_in(&store).len();
    check(count == 2, format!("{count} area folders after gc, not 2"))?;
    // A zip with a file outside the areas' folders is refused.
    let bad = root.join("bad.zip");
    zip(&bad, &[("../evil.txt", b"x")]);
    let (blake3, bytes) = digest_of(&bad)?;
    manifest.archive = Archive {
        file: "bad.zip".into(),
        blake3,
        bytes,
    };
    check(
        install(&store, &manifest, &bad, &mut |_| {}).is_err(),
        "a zip with ../evil.txt installed".into(),
    )?;
    let count = folders_in(&store).len();
    check(
        count == 2,
        format!("{count} area folders after the bad zip, not 2"),
    )?;
    let removed = remove(&store, Game::Kotor2)?;
    let left = folders_in(&store);
    check(
        removed && left.is_empty(),
        format!("removed {removed}, area folders left {left:?}"),
    )
}
