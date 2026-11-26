//! Checks of the lighting download, on manifests, game folders and zips
//! made here (no install): both manifests read back, one matching area is
//! enough, the check's key follows the files a bake reads, an install
//! unpacks into the store and replaces the last one, and a refresh adds the
//! exact lines to an install that kjk v0.21.0 still reads.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use store::Store;
use vfs::Game;

use super::{
    Archive, Area, Differs, FallBack, Kind, MANIFEST, MANIFEST_2, Manifest, Partial, Report,
    allowed, digest_of, files_key, folder, folders_in, install, needs, refresh, remove,
};
use crate::bake_fingerprint::{self, Exact};
use crate::probe_kit::{Checks, check};

/// The checks, for `lighting_downloads` in the approved tests.
pub fn probes() -> Checks {
    vec![
        ("the_manifest_reads_back", the_manifest_reads_back()),
        (
            "the_install_needs_one_matching_area",
            the_install_needs_one_matching_area(),
        ),
        (
            "the_checks_key_follows_the_mods_and_not_the_voices",
            the_checks_key_follows_the_mods_and_not_the_voices(),
        ),
        (
            "an_install_unpacks_into_the_store_and_replaces_the_last",
            an_install_unpacks_into_the_store_and_replaces_the_last(),
        ),
        (
            "a_refusal_names_the_mods_that_differ",
            a_refusal_names_the_mods_that_differ(),
        ),
        (
            "the_second_manifest_reads_back",
            the_second_manifest_reads_back(),
        ),
        (
            "a_refresh_keeps_what_v0_21_reads",
            a_refresh_keeps_what_v0_21_reads(),
        ),
    ]
}

/// Exact lines with one lightmap `lm0` of digest `digest`.
fn exact(digest: &str) -> Exact {
    Exact {
        lightmaps: [("lm0".to_string(), digest.to_string())].into(),
        backdrops: ["sky".to_string()].into(),
        ..Exact::default()
    }
}

/// `manifest()` with exact lines for its first area alone.
fn second_manifest() -> Manifest {
    let mut second = manifest();
    second.areas[0].exact = Some(exact(&"1".repeat(32)));
    second
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
                exact: None,
            },
            Area {
                area: "411dxn".into(),
                module: "411dxn".into(),
                rooms: "00112233445566778899aabbccddeeff".into(),
                placeables: "ffeeddccbbaa99887766554433221100".into(),
                exact: None,
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
    )?;
    // A file that is no manifest, and a file that is not there, each say
    // what went wrong.
    let root = scratch("manifest");
    let other = root.join("other.txt");
    std::fs::write(&other, "SHA256SUMS\n").map_err(|error| format!("other.txt: {error}"))?;
    let kinds = [
        Manifest::load(&other)
            .map(|_| ())
            .map_err(|failure| failure.kind),
        Manifest::load(&root.join("missing.txt"))
            .map(|_| ())
            .map_err(|failure| failure.kind),
    ];
    let _ = std::fs::remove_dir_all(&root);
    check(
        kinds == [Err(Kind::ListInvalid), Err(Kind::ListUnreadable)],
        format!("another file and a missing one read as {kinds:?}"),
    )
}

/// A report of `matched` areas that match fully, `partial` that match
/// partly and `fall_back` that do not, of 10 areas.
fn report_of(matched: usize, partial: usize, fall_back: &[(&str, &[&str])]) -> Report {
    let mut report = Report {
        game: "tsl".into(),
        needs: "TSLRCM".into(),
        areas: 10,
        matched: (0..matched).map(|n| n.to_string()).collect(),
        placeables_differ: Vec::new(),
        partial: (0..partial)
            .map(|n| Partial {
                area: format!("p{n}"),
                matching: 1,
                lightmaps: 2,
                differ: vec![Differs {
                    lightmap: "lm1".into(),
                    rooms: vec!["room01".into()],
                }],
                placeables_differ: false,
                mods: Vec::new(),
            })
            .collect(),
        fall_back: (fall_back.iter())
            .map(|(area, mods)| FallBack {
                area: (*area).into(),
                why: "rooms".into(),
                mods: mods.iter().map(|name| (*name).to_string()).collect(),
            })
            .collect(),
        allowed: false,
        cached: false,
    };
    report.allowed = allowed(report.showing());
    report
}

/// A download installs when at least one area matches fully or partly,
/// and not when none does. A report of 1 matching area and 9 that fall
/// back is allowed, and so is a report of 1 partly matching area. A report
/// of none is refused, and its refusal says that the game's files differ
/// and names the game and its patch.
fn the_install_needs_one_matching_area() -> Result<String, String> {
    let cases = [(0, false), (1, true), (75, true)];
    let wrong: Vec<_> = (cases.iter())
        .filter(|(showing, want)| allowed(*showing) != *want)
        .collect();
    check(
        wrong.is_empty(),
        format!("judged wrong (showing, allowed): {wrong:?}"),
    )?;
    let nine: Vec<(&str, &[&str])> = (0..9).map(|_| ("x", &[][..])).collect();
    let reports = [
        ("one matching area", report_of(1, 0, &nine), true),
        ("one partly matching area", report_of(0, 1, &nine), true),
        ("no matching area", report_of(0, 0, &nine), false),
    ];
    let wrong: Vec<&str> = (reports.iter())
        .filter(|(_, report, want)| report.allowed != *want)
        .map(|(what, _, _)| *what)
        .collect();
    check(wrong.is_empty(), format!("judged wrong: {wrong:?}"))?;
    let refused = &reports[2].1;
    let refusal = refused
        .refusal()
        .ok_or("a report not allowed has no refusal")?;
    check(
        refusal.contains("your game's files differ") && refusal.contains("KOTOR II with TSLRCM"),
        refusal,
    )?;
    check(
        reports[0].1.refusal().is_none(),
        format!("an allowed report refuses: {:?}", reports[0].1.refusal()),
    )?;
    // The counts agree with their words for 1 and for more than 1.
    let counts = [
        (
            report_of(7, 2, &[("701kor", &[])]),
            "7 show it in full, 2 show it in part, and 1 keeps",
            "1 area keeps the game's own lighting",
        ),
        (
            report_of(1, 1, &[("701kor", &[]), ("702tel", &[])]),
            "1 shows it in full, 1 shows it in part, and 2 keep",
            "2 areas keep the game's own lighting",
        ),
    ];
    for (report, counts, fall_back) in counts {
        let sentences = report.sentences().join(" ");
        check(
            sentences.contains(counts) && sentences.contains(fall_back),
            sentences,
        )?;
    }
    // The kind of a refusal: the files differ, or the bake's format is
    // not the one this kjk reads.
    let format = bake_fingerprint::FORMAT;
    let kinds = [
        reports[0].1.refusal_kind(format),
        refused.refusal_kind(format),
        refused.refusal_kind(format + 1),
    ];
    check(
        kinds == [None, Some(Kind::FilesDiffer), Some(Kind::BakeFormat)],
        format!("the kinds of the refusals are {kinds:?}"),
    )
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
    let wrong = install(&store, &manifest, &path, &mut |_| {});
    check(
        wrong
            .as_ref()
            .is_err_and(|failure| failure.kind == Kind::ZipMismatch),
        format!("a download of the wrong size and BLAKE3 installed as {wrong:?}"),
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
    let evil = install(&store, &manifest, &bad, &mut |_| {});
    check(
        evil.as_ref()
            .is_err_and(|failure| failure.kind == Kind::ZipDamaged),
        format!("a zip with ../evil.txt installed as {evil:?}"),
    )?;
    // The errors of a write into the store say what the player can do.
    let full = std::io::Error::from_raw_os_error(if cfg!(windows) { 112 } else { 28 });
    let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
    let kinds = [Kind::of_write(&full), Kind::of_write(&denied)];
    check(
        kinds == [Kind::DiskFull, Kind::StoreNotWritable],
        format!("a full disk and a denied write are {kinds:?}"),
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

/// A refused report of 10 areas. The areas in `fall_back` fall back, each
/// with the mods that replace its room files, and the others match. It is
/// refused whatever matches, so that its refusal can be read.
fn refused_with(fall_back: &[(&str, &[&str])]) -> Report {
    Report {
        allowed: false,
        ..report_of(10 - fall_back.len(), 0, fall_back)
    }
}

/// The resolver's layers name their mod: a Steam Workshop item by its id,
/// the override folder with its subfolders, and a `.mod` in the modules
/// folder. The game's own files name none. A refusal then names each mod
/// that replaces the room files of the areas that differ, the mod with the
/// most areas first, with at most three areas by name. TSLRCM's own
/// Workshop item is not named, since the KOTOR II lighting matches it.
/// Without any other mod's files, the refusal is the sentence about the
/// patch alone.
fn a_refusal_names_the_mods_that_differ() -> Result<String, String> {
    let layers = [
        ("workshop/485537937/override", Some("workshop/485537937")),
        (
            "workshop/1834631667/modules/602dan.mod",
            Some("workshop/1834631667"),
        ),
        ("override", Some("override")),
        ("override/textures", Some("override")),
        ("modules/101per.mod", Some("modules")),
        ("modules/101per.rim", None),
        ("texturepacks/swpc_tex_tpa.erf", None),
        ("chitin.key", None),
    ];
    let wrong: Vec<_> = (layers.iter())
        .filter(|(layer, want)| bake_fingerprint::mod_place(layer).as_deref() != *want)
        .collect();
    check(
        wrong.is_empty(),
        format!("these layers name the wrong mod: {wrong:?}"),
    )?;
    let refusal = refused_with(&[
        ("601dan", &["workshop/111"]),
        ("602dan", &["workshop/111", "override"]),
        ("604dan", &["workshop/111"]),
        ("605dan", &["workshop/111", "workshop/485537937"]),
        ("606dan", &["workshop/485537937"]),
        ("701kor", &[]),
    ])
    .refusal()
    .ok_or("a report not allowed has no refusal")?;
    check(
        refusal
            == "None of the 10 areas of the new lighting can be shown, because your game's \
                files differ from the files the lighting was made from. The new lighting was \
                made for KOTOR II with TSLRCM. In 6 areas the rooms or their lightmaps differ \
                from the ones the lighting was made from. Steam Workshop item 111 replaces \
                these files in 4 of those areas: 601dan, 602dan, 604dan and 1 more. The \
                override folder replaces these files in 1 of those areas: 602dan.",
        refusal,
    )?;
    let alone = refused_with(&[("701kor", &[]), ("702kor", &["workshop/485537937"])])
        .refusal()
        .ok_or("a report not allowed has no refusal")?;
    check(alone.ends_with("made for KOTOR II with TSLRCM."), alone)
}

/// A second manifest reads back from its text, with its first area's exact
/// lines and none for the second. Its text is no manifest of the first
/// form, and the first form's text of it is the first manifest's. Both
/// files load by their first line. A second manifest with exact lines of
/// another version reads with no exact lines for that area.
fn the_second_manifest_reads_back() -> Result<String, String> {
    let second = second_manifest();
    let text = second.exact_text();
    let read = Manifest::parse_exact(&text);
    check(
        read.as_ref() == Ok(&second),
        format!("the second manifest read as {read:?}"),
    )?;
    check(
        Manifest::parse(&text).is_err() && second.text() == manifest().text(),
        "the first form reads the second manifest, or writes exact lines".into(),
    )?;
    let root = scratch("second");
    let loads: Vec<Result<Manifest, Kind>> = [
        ("first.txt", manifest().text()),
        ("second.txt", text.clone()),
    ]
    .iter()
    .map(|(name, text)| {
        let path = root.join(name);
        std::fs::write(&path, text).map_err(|_| Kind::Other)?;
        Manifest::load(&path).map_err(|failure| failure.kind)
    })
    .collect();
    let _ = std::fs::remove_dir_all(&root);
    check(
        loads == [Ok(manifest()), Ok(second.clone())],
        format!("the two files load as {loads:?}"),
    )?;
    let newer = text.replace(
        &format!("exact {}", bake_fingerprint::EXACT),
        &format!("exact {}", bake_fingerprint::EXACT + 1),
    );
    let read = Manifest::parse_exact(&newer).map(|read| read.is_exact());
    check(
        read == Ok(false),
        format!("exact lines of another version read as {read:?}"),
    )
}

/// A download installed from the first manifest is refreshed with the
/// second manifest. The installed `manifest.txt` keeps every byte, and
/// each area's `fingerprint.txt` still gives kjk v0.21.0's reader the same
/// area, format and digests. The game now reads the first area's exact
/// lines, and the second manifest is installed beside the first. A second
/// manifest of another zip is refused, and so is a refresh with an area
/// folder missing.
fn a_refresh_keeps_what_v0_21_reads() -> Result<String, String> {
    let root = scratch("refresh");
    let result = refresh_in(&root);
    let _ = std::fs::remove_dir_all(&root);
    result
}

/// `a_refresh_keeps_what_v0_21_reads` in the scratch folder `root`.
fn refresh_in(root: &Path) -> Result<String, String> {
    let store = Store::at(root.join("store"));
    let (mut manifest, path) = (manifest(), root.join("download.zip"));
    let prints: Vec<String> = (manifest.areas.iter())
        .map(|area| bake_fingerprint::text(&area.stamp(manifest.format)))
        .collect();
    zip(
        &path,
        &[
            ("101per/fingerprint.txt", prints[0].as_bytes()),
            ("101per/lm0.png", b"png"),
            ("411dxn/fingerprint.txt", prints[1].as_bytes()),
        ],
    );
    let (blake3, bytes) = digest_of(&path)?;
    manifest.archive = Archive {
        file: "download.zip".into(),
        blake3,
        bytes,
    };
    let installed = install(&store, &manifest, &path, &mut |_| {})?;
    let first = std::fs::read(installed.join(MANIFEST)).map_err(|error| error.to_string())?;
    let second = Manifest {
        archive: manifest.archive.clone(),
        ..second_manifest()
    };
    refresh(&store, &second)?;
    let after = std::fs::read(installed.join(MANIFEST)).map_err(|error| error.to_string())?;
    check(after == first, "the refresh changed manifest.txt".into())?;
    for area in &second.areas {
        let text = std::fs::read_to_string(installed.join(&area.area).join(bake_fingerprint::FILE))
            .map_err(|error| error.to_string())?;
        let old = bake_fingerprint::probes::read_as_v0_21(&text);
        let want = Some((
            area.area.clone(),
            second.format,
            area.rooms.clone(),
            area.placeables.clone(),
        ));
        check(
            old == want,
            format!("kjk v0.21.0 reads {old:?} for {}", area.area),
        )?;
        let read = bake_fingerprint::read(&installed.join(&area.area)).and_then(Result::ok);
        check(
            read.as_ref().map(|print| &print.exact) == Some(&area.exact),
            format!("{} reads as {read:?}", area.area),
        )?;
    }
    let read = Manifest::read(&installed.join(MANIFEST_2));
    check(
        read.as_ref() == Ok(&second),
        format!("the installed second manifest reads as {read:?}"),
    )?;
    let other = Manifest {
        archive: Archive {
            blake3: "1".repeat(64),
            ..second.archive.clone()
        },
        ..second.clone()
    };
    let refused = refresh(&store, &other).map_err(|failure| failure.kind);
    std::fs::remove_dir_all(installed.join("411dxn")).map_err(|error| error.to_string())?;
    let missing = refresh(&store, &second).map_err(|failure| failure.kind);
    check(
        refused.is_err_and(|kind| kind == Kind::NotInstalled)
            && missing.is_err_and(|kind| kind == Kind::NotInstalled),
        "a refresh of another zip, or with an area folder missing, was not refused".into(),
    )
}
