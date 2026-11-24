//! The resolver's precedence on installs written here (journal
//! 2026-11-05-override-precedence): `override/` over a module's archives, a
//! `.mod` standing for its RIMs and `_dlg.erf`, `patch.erf` over a module,
//! a module's `.rim` over its `_s.rim`, TSL's override subfolders, a
//! texture held as both a TPC and a TGA,
//! co-op's check covering just the override files the resolver reads, and
//! the Steam Workshop items TSL loads, in its load order.

use std::path::{Path, PathBuf};

use fmt_odyssey::ResType;

use crate::resolver::{Location, Resolver, TextureQuality};
use crate::{DirIndex, Game, MAX_WORKSHOP_ITEMS, MovieDuels, WorkshopFolder};

/// One resource of an archive written here: its name, type and bytes.
type Entry<'a> = (&'a str, ResType, &'a [u8]);

/// An archive as KOTOR writes one: a RIM (`RIM V1.0`) or an ERF family
/// one (`MOD V1.0`, `ERF V1.0`).
fn archive(tag: &[u8; 8], entries: &[Entry]) -> Vec<u8> {
    let rim = tag == b"RIM V1.0";
    let (header, record) = if rim { (20, 32) } else { (160, 24) };
    let table = header;
    let resources = table + entries.len() * record;
    let mut data = resources + if rim { 0 } else { entries.len() * 8 };
    let mut out = vec![0u8; data];
    out[..8].copy_from_slice(tag);
    let put = |out: &mut Vec<u8>, at: usize, value: u32| {
        out[at..at + 4].copy_from_slice(&value.to_le_bytes());
    };
    if rim {
        put(&mut out, 12, entries.len() as u32);
        put(&mut out, 16, table as u32);
    } else {
        put(&mut out, 16, entries.len() as u32);
        put(&mut out, 24, table as u32);
        put(&mut out, 28, resources as u32);
    }
    let mut blobs = Vec::new();
    for (i, (resref, restype, bytes)) in entries.iter().enumerate() {
        let at = table + i * record;
        out[at..at + resref.len()].copy_from_slice(resref.as_bytes());
        let kind = (restype.0 as u16).to_le_bytes();
        let (offset_at, size_at) = if rim {
            out[at + 16..at + 18].copy_from_slice(&kind);
            (at + 24, at + 28)
        } else {
            out[at + 20..at + 22].copy_from_slice(&kind);
            (resources + i * 8, resources + i * 8 + 4)
        };
        put(&mut out, offset_at, data as u32);
        put(&mut out, size_at, bytes.len() as u32);
        data += bytes.len();
        blobs.extend_from_slice(bytes);
    }
    out.extend_from_slice(&blobs);
    out
}

/// A `chitin.key` with no BIFs: an install's smallest.
fn empty_key() -> Vec<u8> {
    let mut key = vec![0u8; 64];
    key[..8].copy_from_slice(b"KEY V1  ");
    key[16..20].copy_from_slice(&64u32.to_le_bytes());
    key[20..24].copy_from_slice(&64u32.to_le_bytes());
    key
}

/// Files to write: each its path and bytes.
type Files<'a> = [(&'a str, Vec<u8>)];

/// Workshop items to write: each its id and files, in load order.
type Items<'a> = [(u64, Vec<(&'a str, Vec<u8>)>)];

/// A fresh folder for one probe, holding `files`.
fn folder(name: &str, files: &Files) -> Result<PathBuf, String> {
    let root = std::env::temp_dir().join(format!("kjk-vfs-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for (path, bytes) in files {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().expect("a file in a folder"))
            .map_err(|error| error.to_string())?;
        std::fs::write(&path, bytes).map_err(|error| error.to_string())?;
    }
    Ok(root)
}

/// A fresh install folder for one probe, holding `files` (path, bytes).
fn install(name: &str, files: &Files) -> Result<PathBuf, String> {
    let key = [("chitin.key", empty_key())];
    folder(name, &[&key[..], files].concat())
}

/// An install written for one probe with `items` as its Workshop items,
/// indexed: the folders written, to remove, and the index.
fn workshop_install(
    name: &str,
    files: &Files,
    items: &Items,
) -> Result<(Vec<PathBuf>, DirIndex), String> {
    let mut written = vec![install(name, files)?];
    let mut folders = Vec::new();
    for (id, files) in items {
        let root = folder(&format!("{name}-ws{id}"), files)?;
        let files = DirIndex::build(&root).map_err(|error| error.to_string())?;
        folders.push(WorkshopFolder { id: *id, files });
        written.push(root);
    }
    let index = DirIndex::build(&written[0]).map_err(|error| error.to_string())?;
    Ok((written, index.with_workshop(folders)))
}

fn remove(written: &[PathBuf]) {
    for root in written {
        let _ = std::fs::remove_dir_all(root);
    }
}

/// What `name` resolves to in `module` of the install at `root`: its
/// layer and bytes, or `-` when nothing holds it.
fn resolve(root: &Path, game: Game, module: &str, names: &[(&str, ResType)]) -> Vec<String> {
    match DirIndex::build(root) {
        Ok(index) => resolve_in(&index, game, module, names),
        Err(error) => vec![error.to_string()],
    }
}

/// What `name` resolves to in `module` of `index`'s install.
fn resolve_in(
    index: &DirIndex,
    game: Game,
    module: &str,
    names: &[(&str, ResType)],
) -> Vec<String> {
    let resolver = Resolver::open(game, index, Some(module), TextureQuality::High)
        .map_err(|error| error.to_string());
    names
        .iter()
        .map(|(name, restype)| match &resolver {
            Err(error) => format!("{name}: {error}"),
            Ok(resolver) => match resolver.find(name, *restype) {
                Some(found) => format!(
                    "{name}={}:{}",
                    found.layer.name,
                    found.location.read().map_or_else(
                        |error| format!("unreadable ({error})"),
                        |bytes| String::from_utf8_lossy(&bytes).into_owned()
                    )
                ),
                None => format!("{name}=-"),
            },
        })
        .collect()
}

/// Runs one case: the install written, each name resolved, compared with
/// what the games pick.
fn case(
    name: &str,
    game: Game,
    files: &[(&str, Vec<u8>)],
    names: &[(&str, ResType)],
    expected: &[&str],
) -> Result<String, String> {
    let root = install(name, files)?;
    let got = resolve(&root, game, "m01", names);
    let _ = std::fs::remove_dir_all(&root);
    if got == expected {
        Ok(got.join(", "))
    } else {
        Err(format!("got {got:?}, the games pick {expected:?}"))
    }
}

const DLG: ResType = ResType::DLG;
const NCS: ResType = ResType::NCS;

/// A modded DLG and NCS in `override/` beat the module's own, from its
/// `.rim`, `_s.rim` and a `.mod` alike, in both games.
fn override_beats_module() -> Result<String, String> {
    let module = [
        (
            "modules/m01.rim",
            archive(b"RIM V1.0", &[("a", NCS, b"rim")]),
        ),
        (
            "modules/m01_s.rim",
            archive(b"RIM V1.0", &[("talk", DLG, b"s")]),
        ),
        ("override/talk.dlg", b"mod".to_vec()),
        ("override/a.ncs", b"mod".to_vec()),
    ];
    let named = [("talk", DLG), ("a", NCS)];
    let wins = ["talk=override:mod", "a=override:mod"];
    let rims = case("over-rims", Game::Kotor1, &module, &named, &wins)?;
    let dot_mod = [
        (
            "modules/m01.mod",
            archive(b"MOD V1.0", &[("talk", DLG, b"m"), ("a", NCS, b"m")]),
        ),
        ("override/talk.dlg", b"mod".to_vec()),
        ("override/a.ncs", b"mod".to_vec()),
    ];
    let dot_mod = case("over-mod", Game::Kotor2, &dot_mod, &named, &wins)?;
    Ok(format!("rims: {rims}; .mod: {dot_mod}"))
}

/// A mod's table in `override/` beats the copy in the global RIM (where
/// the Mac edition keeps K1's tables) for a module that has a `.mod`, as a
/// mod installer leaves a Windows install (`Override/`, `Modules/`): rows
/// appended to `appearance.2da` in
/// `override/` (the K1 Community Patch's and the Twi'lek NPC mods') are the
/// rows every module reads.
fn override_table_beats_globals() -> Result<String, String> {
    const TABLE: ResType = ResType::TWO_DA;
    let files = [
        (
            "rims/global.rim",
            archive(b"RIM V1.0", &[("appearance", TABLE, b"rim")]),
        ),
        (
            "Modules/m01.mod",
            archive(b"MOD V1.0", &[("talk", DLG, b"m")]),
        ),
        ("Override/appearance.2da", b"mod".to_vec()),
    ];
    let named = [("appearance", TABLE)];
    case(
        "table-over-globals",
        Game::Kotor1,
        &files,
        &named,
        &["appearance=override:mod"],
    )
}

/// A `.mod` is the whole module: its RIMs and its `_dlg.erf` are not read
/// (TSLRCM's dialogues in a `.mod` beat the `_dlg.erf` it replaces).
fn mod_stands_for_rims() -> Result<String, String> {
    let files = [
        (
            "modules/m01.rim",
            archive(b"RIM V1.0", &[("only_rim", NCS, b"rim")]),
        ),
        (
            "modules/m01_s.rim",
            archive(b"RIM V1.0", &[("only_s", NCS, b"s")]),
        ),
        (
            "modules/m01_dlg.erf",
            archive(b"ERF V1.0", &[("talk", DLG, b"vanilla")]),
        ),
        (
            "modules/m01.mod",
            archive(b"MOD V1.0", &[("talk", DLG, b"restored")]),
        ),
    ];
    case(
        "mod-whole",
        Game::Kotor2,
        &files,
        &[("talk", DLG), ("only_rim", NCS), ("only_s", NCS)],
        &["talk=modules/m01.mod:restored", "only_rim=-", "only_s=-"],
    )
}

/// `patch.erf` (an ERF of priority 1) beats a module's RIMs and `.mod`, and
/// a module's `.rim` its `_s.rim` (`CExoResMan::GetKeyEntry`).
fn ranks_between_archives() -> Result<String, String> {
    let files = [
        (
            "modules/m01.rim",
            archive(
                b"RIM V1.0",
                &[("both", NCS, b"rim"), ("fixed", NCS, b"rim")],
            ),
        ),
        (
            "modules/m01_s.rim",
            archive(b"RIM V1.0", &[("both", NCS, b"s")]),
        ),
        (
            "patch.erf",
            archive(b"ERF V1.0", &[("fixed", NCS, b"patch")]),
        ),
    ];
    case(
        "ranks",
        Game::Kotor1,
        &files,
        &[("both", NCS), ("fixed", NCS)],
        &["both=modules/m01.rim:rim", "fixed=patch.erf:patch"],
    )
}

/// TSL reads `override/`'s subfolders one level down, the top level first,
/// then the subfolders in name order; K1 reads its top level alone.
fn override_subfolders() -> Result<String, String> {
    let files = [
        (
            "modules/m01.rim",
            archive(b"RIM V1.0", &[("deep", NCS, b"rim")]),
        ),
        ("override/top.ncs", b"top".to_vec()),
        ("override/b_mod/top.ncs", b"sub".to_vec()),
        ("override/b_mod/sub.ncs", b"b".to_vec()),
        ("override/a_mod/sub.ncs", b"a".to_vec()),
        ("override/a_mod/x/deep.ncs", b"deep".to_vec()),
    ];
    let named = [("top", NCS), ("sub", NCS), ("deep", NCS)];
    let tsl = case(
        "sub-tsl",
        Game::Kotor2,
        &files,
        &named,
        &[
            "top=override:top",
            "sub=override:a",
            "deep=modules/m01.rim:rim",
        ],
    )?;
    let k1 = case(
        "sub-k1",
        Game::Kotor1,
        &files,
        &named,
        &["top=override:top", "sub=-", "deep=modules/m01.rim:rim"],
    )?;
    Ok(format!("tsl: {tsl}; k1: {k1}"))
}

/// Co-op compares the override files the games read and no others: a TXT
/// with other line endings (Aspyr's `gamepad.txt` on Windows), a readme
/// and a file deeper than the game reads leave the world's data as it
/// was; a script changed there, or in a TSL subfolder, does not.
fn override_fingerprint() -> Result<String, String> {
    let world = |name: &str, game: Game, files: &[(&str, Vec<u8>)]| -> Result<_, String> {
        let root = install(name, files)?;
        let index = DirIndex::build(&root).map_err(|error| error.to_string())?;
        let world = crate::fingerprint::fingerprint(game, &index, None)
            .map_err(|error| error.to_string())?
            .world();
        let _ = std::fs::remove_dir_all(&root);
        Ok(world)
    };
    let mac = |script: &[u8], sub: &[u8]| {
        vec![
            ("modules/m01.rim", archive(b"RIM V1.0", &[])),
            ("override/a.ncs", script.to_vec()),
            ("override/gamepad.txt", b"a\nb\n".to_vec()),
            ("override/mod/b.ncs", sub.to_vec()),
        ]
    };
    let mut windows = mac(b"a", b"b");
    windows[2].1 = b"a\r\nb\r\n".to_vec();
    windows.push(("override/readme.rtf", b"{\\rtf1}".to_vec()));
    windows.push(("override/mod/deep/c.ncs", b"c".to_vec()));
    let mut told = Vec::new();
    for game in [Game::Kotor1, Game::Kotor2] {
        let key = game.key();
        let same = world(&format!("fp-{key}-mac"), game, &mac(b"a", b"b"))?;
        if world(&format!("fp-{key}-win"), game, &windows)? != same {
            return Err(format!(
                "{key}: line endings, a readme or a deep file differ"
            ));
        }
        if world(&format!("fp-{key}-script"), game, &mac(b"A", b"b"))? == same {
            return Err(format!(
                "{key}: a changed override script is the same world"
            ));
        }
        let sub = world(&format!("fp-{key}-sub"), game, &mac(b"a", b"B"))? != same;
        if sub != (game == Game::Kotor2) {
            return Err(format!("{key}: a changed subfolder script differs: {sub}"));
        }
        told.push(format!("{key}: subfolder counted {sub}"));
    }
    Ok(told.join("; "))
}

/// A join hashes the world groups alone (`world_fingerprint`): the same
/// world as the whole fingerprint's, its voices, movies and talk table
/// never read, the progress told up to all of it, and a second join
/// hashing nothing, its cache kept beside the whole fingerprint's rows.
fn world_fingerprint_reads_world() -> Result<String, String> {
    use crate::fingerprint::{fingerprint, world_fingerprint};
    let files = [
        ("chitin.key", empty_key()),
        ("modules/m01.rim", archive(b"RIM V1.0", &[])),
        ("override/a.ncs", b"a".to_vec()),
        ("dialog.tlk", b"talk".to_vec()),
        ("movies/intro.bik", b"movie".to_vec()),
        ("streamvoice/a/b.wav", b"voice".to_vec()),
    ];
    let root = install("fp-world", &files)?;
    let cache = std::env::temp_dir().join("kjk-probe-fp-world.cache");
    let _ = std::fs::remove_file(&cache);
    let found = (|| -> Result<String, String> {
        let index = DirIndex::build(&root).map_err(|error| error.to_string())?;
        let whole = fingerprint(Game::Kotor2, &index, None).map_err(|error| error.to_string())?;
        let mut told = Vec::new();
        let join = |told: &mut Vec<(u64, u64)>| {
            world_fingerprint(Game::Kotor2, &index, &cache, &mut |done, total| {
                told.push((done, total))
            })
            .map_err(|error| error.to_string())
        };
        let first = join(&mut told)?;
        if first.world() != whole.world() {
            return Err("the world groups alone are another world".into());
        }
        if first.hashed != 3 || first.files != 3 {
            return Err(format!(
                "hashed {} of {} files, not the 3 world files",
                first.hashed, first.files
            ));
        }
        match told.last() {
            Some((done, total)) if done == total && *total > 0 => {}
            last => return Err(format!("progress ended at {last:?}")),
        }
        let mut again = Vec::new();
        let second = join(&mut again)?;
        if second.hashed != 0 || !again.is_empty() {
            return Err(format!("a second join hashed {} files", second.hashed));
        }
        Ok(format!(
            "{} of {} files hashed, progress told {} times",
            first.hashed,
            whole.files,
            told.len()
        ))
    })();
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_file(&cache);
    found
}

/// Runs one case on an install with Workshop items: each name resolved
/// in `m01`, compared with what TSL picks.
fn workshop_case(
    name: &str,
    files: &Files,
    items: &Items,
    names: &[(&str, ResType)],
    expected: &[&str],
) -> Result<String, String> {
    let (written, index) = workshop_install(name, files, items)?;
    let got = resolve_in(&index, Game::Kotor2, "m01", names);
    remove(&written);
    if got == expected {
        Ok(got.join(", "))
    } else {
        Err(format!("got {got:?}, TSL picks {expected:?}"))
    }
}

const TPC: ResType = ResType::TPC;
const TGA: ResType = ResType::TGA;

/// Which copy of each texture in `names` the games load from `index`'s
/// install: its layer, its type and its bytes.
fn textures_in(index: &DirIndex, game: Game, names: &[&str]) -> Vec<String> {
    let resolver = match Resolver::open(game, index, Some("m01"), TextureQuality::High) {
        Ok(resolver) => resolver,
        Err(error) => return vec![error.to_string()],
    };
    names
        .iter()
        .map(|name| match resolver.find_any(name, &[TPC, TGA]) {
            Some((restype, found)) => format!(
                "{name}={}:{}:{}",
                found.layer.name,
                restype.extension().unwrap_or("?"),
                found.location.read().map_or_else(
                    |error| format!("unreadable ({error})"),
                    |bytes| String::from_utf8_lossy(&bytes).into_owned()
                )
            ),
            None => format!("{name}=-"),
        })
        .collect()
}

/// A texture held as both a TPC and a TGA loads as both games' executables
/// choose. The copy of each type comes from its best source, and the type
/// whose source is of the better kind wins: a folder, then a RIM, then an
/// ERF, then a BIF. Between two sources of one kind the TPC wins. So a TPC
/// and a TGA both in `override/` load the TPC (Vurt's Visual Resurgence
/// with High Quality Blasters), and a TGA in `override/` loads over a
/// texture pack's TPC. A module's `.mod` and `patch.erf` are ERFs like the
/// texture pack, so their TGA loses to the pack's TPC. A RIM's TGA beats
/// it. In TSL, a TGA in a Workshop item's `override/` loses to a TPC in the
/// game's own `override/`, because both are folders.
fn texture_formats() -> Result<String, String> {
    let pack = archive(
        b"ERF V1.0",
        &[
            ("both", TPC, b"pack"),
            ("loose", TPC, b"pack"),
            ("in_mod", TPC, b"pack"),
            ("in_patch", TPC, b"pack"),
            ("in_rim", TPC, b"pack"),
        ],
    );
    let files = [
        ("texturepacks/swpc_tex_tpa.erf", pack),
        ("override/both.tpc", b"override".to_vec()),
        ("override/both.tga", b"override".to_vec()),
        ("override/loose.tga", b"override".to_vec()),
        (
            "modules/m01.mod",
            archive(b"MOD V1.0", &[("in_mod", TGA, b"mod")]),
        ),
        (
            "patch.erf",
            archive(b"ERF V1.0", &[("in_patch", TGA, b"patch")]),
        ),
        (
            "rims/global.rim",
            archive(b"RIM V1.0", &[("in_rim", TGA, b"rim")]),
        ),
    ];
    let root = install("texture-formats", &files)?;
    let k1 = DirIndex::build(&root)
        .map(|index| {
            textures_in(
                &index,
                Game::Kotor1,
                &["both", "loose", "in_mod", "in_patch", "in_rim"],
            )
        })
        .map_err(|error| error.to_string());
    let _ = std::fs::remove_dir_all(&root);
    let k1 = k1?;
    let picks = [
        "both=override:tpc:override",
        "loose=override:tga:override",
        "in_mod=texturepacks/swpc_tex_tpa.erf:tpc:pack",
        "in_patch=texturepacks/swpc_tex_tpa.erf:tpc:pack",
        "in_rim=rims/global.rim:tga:rim",
    ];
    if k1 != picks {
        return Err(format!("got {k1:?}, the games pick {picks:?}"));
    }
    let files = [
        ("modules/m01.rim", archive(b"RIM V1.0", &[])),
        ("override/game_tpc.tpc", b"game".to_vec()),
        ("override/item_tpc.tga", b"game".to_vec()),
    ];
    let items = [(
        7,
        vec![
            ("override/game_tpc.tga", b"item".to_vec()),
            ("override/item_tpc.tpc", b"item".to_vec()),
        ],
    )];
    let (written, index) = workshop_install("texture-formats-ws", &files, &items)?;
    let tsl = textures_in(&index, Game::Kotor2, &["game_tpc", "item_tpc"]);
    remove(&written);
    let picks = [
        "game_tpc=override:tpc:game",
        "item_tpc=workshop/7/override:tpc:item",
    ];
    if tsl != picks {
        return Err(format!("got {tsl:?}, TSL picks {picks:?}"));
    }
    Ok(format!("k1: {}; tsl: {}", k1.join(", "), tsl.join(", ")))
}

/// The Workshop items' `override/` beat the game's, the last item's
/// winning; an item's subfolders are not read; its `.mod` beats the game's
/// module, and of two items' `.mod`s the last one's, every copy read.
fn workshop_override_and_mods() -> Result<String, String> {
    let files = [
        (
            "modules/m01.rim",
            archive(
                b"RIM V1.0",
                &[("deep", NCS, b"rim"), ("only_rim", NCS, b"rim")],
            ),
        ),
        ("override/both.ncs", b"game".to_vec()),
        ("override/first.ncs", b"game".to_vec()),
    ];
    let items = [
        (
            7,
            vec![
                ("override/both.ncs", b"w7".to_vec()),
                ("override/first.ncs", b"w7".to_vec()),
                (
                    "modules/m01.mod",
                    archive(
                        b"MOD V1.0",
                        &[("talk", DLG, b"w7"), ("only_w7", DLG, b"w7")],
                    ),
                ),
            ],
        ),
        (
            3,
            vec![
                ("override/both.ncs", b"w3".to_vec()),
                ("override/sub/deep.ncs", b"w3".to_vec()),
                (
                    "modules/m01.mod",
                    archive(b"MOD V1.0", &[("talk", DLG, b"w3")]),
                ),
            ],
        ),
    ];
    workshop_case(
        "ws-over",
        &files,
        &items,
        &[
            ("both", NCS),
            ("first", NCS),
            ("deep", NCS),
            ("talk", DLG),
            ("only_w7", DLG),
            ("only_rim", NCS),
        ],
        &[
            "both=workshop/3/override:w3",
            "first=workshop/7/override:w7",
            "deep=-",
            "talk=workshop/3/modules/m01.mod:w3",
            "only_w7=workshop/7/modules/m01.mod:w7",
            "only_rim=-",
        ],
    )
}

/// Without a `.mod`: the module's `.rim` is the last item's, its `_s.rim`
/// the first item's (`AsyncLoad` searches those from the first), and every
/// `_dlg.erf` is read, the last item's winning. A module only an item has
/// is the install's.
fn workshop_module_rims() -> Result<String, String> {
    let rim = |tag: &[u8; 8], entries: &[Entry]| archive(tag, entries);
    let files = [
        (
            "modules/m01.rim",
            rim(
                b"RIM V1.0",
                &[("r", NCS, b"game"), ("only_r", NCS, b"game")],
            ),
        ),
        (
            "modules/m01_s.rim",
            rim(
                b"RIM V1.0",
                &[("s", NCS, b"game"), ("only_s", NCS, b"game")],
            ),
        ),
        (
            "modules/m01_dlg.erf",
            rim(
                b"ERF V1.0",
                &[("d", DLG, b"game"), ("only_d", DLG, b"game")],
            ),
        ),
    ];
    let items = [
        (
            1,
            vec![
                ("modules/m01_s.rim", rim(b"RIM V1.0", &[("s", NCS, b"w1")])),
                (
                    "modules/m01_dlg.erf",
                    rim(b"ERF V1.0", &[("d", DLG, b"w1")]),
                ),
                ("modules/m02.mod", rim(b"MOD V1.0", &[])),
            ],
        ),
        (
            2,
            vec![
                ("modules/m01.rim", rim(b"RIM V1.0", &[("r", NCS, b"w2")])),
                ("modules/m01_s.rim", rim(b"RIM V1.0", &[("s", NCS, b"w2")])),
                (
                    "modules/m01_dlg.erf",
                    rim(b"ERF V1.0", &[("d", DLG, b"w2")]),
                ),
            ],
        ),
    ];
    let picked = workshop_case(
        "ws-rims",
        &files,
        &items,
        &[
            ("r", NCS),
            ("only_r", NCS),
            ("s", NCS),
            ("only_s", NCS),
            ("d", DLG),
            ("only_d", DLG),
        ],
        &[
            "r=workshop/2/modules/m01.rim:w2",
            "only_r=-",
            "s=workshop/1/modules/m01_s.rim:w1",
            "only_s=-",
            "d=workshop/2/modules/m01_dlg.erf:w2",
            "only_d=modules/m01_dlg.erf:game",
        ],
    )?;
    let (written, index) = workshop_install("ws-list", &files, &items)?;
    let modules = Resolver::modules(&index);
    remove(&written);
    if modules != ["m01", "m02"] {
        return Err(format!("modules {modules:?}, TSL has m01 and m02"));
    }
    Ok(format!("{picked}; modules {}", modules.join(" ")))
}

/// `dialog.tlk`, movies and each stream folder's files are the last
/// item's that has them, the game folder's when none has; an item's `.mp3`
/// is the sound of its name as a `.wav` is, to the stream folder and to a
/// sound asked for by name (`Resolver::sound`). A sound asked for by name
/// is the archives' first, the override's here, and a music track is
/// `streammusic`'s first (`Resolver::music`).
fn workshop_talk_and_streams() -> Result<String, String> {
    let files = [
        ("dialog.tlk", b"game".to_vec()),
        ("streammusic/a.wav", b"game".to_vec()),
        ("streammusic/c.wav", b"game".to_vec()),
        ("streammusic/d.wav", b"game".to_vec()),
        ("override/d.wav", b"override".to_vec()),
        ("movies/m.bik", b"game".to_vec()),
    ];
    let items = [
        (
            1,
            vec![
                ("dialog.tlk", b"w1".to_vec()),
                ("streammusic/a.wav", b"w1".to_vec()),
                ("streammusic/b.wav", b"w1".to_vec()),
            ],
        ),
        (
            2,
            vec![
                ("streammusic/b.wav", b"w2".to_vec()),
                ("streammusic/c.mp3", b"w2".to_vec()),
                ("movies/m.bik", b"w2".to_vec()),
            ],
        ),
    ];
    let (written, index) = workshop_install("ws-streams", &files, &items)?;
    let base = Resolver::base(Game::Kotor2, &index, TextureQuality::High)
        .map_err(|error| error.to_string());
    let read =
        |path: &Path| std::fs::read(path).map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
    let got: Result<Vec<String>, String> = base.and_then(|base| {
        let talk = base
            .talk
            .path()
            .map(read)
            .transpose()
            .map_err(|error| error.to_string())?;
        let mut got = vec![format!("dialog.tlk={}", talk.unwrap_or_else(|| "-".into()))];
        let text = |found: Option<&Location>| match found.map(Location::read) {
            Some(Ok(bytes)) => String::from_utf8_lossy(&bytes).into_owned(),
            Some(Err(error)) => error.to_string(),
            None => "-".into(),
        };
        for (folder, name, restype) in [
            ("streammusic", "a", ResType::WAV),
            ("streammusic", "b", ResType::WAV),
            ("streammusic", "c", ResType::WAV),
            ("streammusic", "d", ResType::WAV),
            ("movies", "m", ResType::BIK),
        ] {
            let found = text(base.stream(folder, name, restype));
            got.push(format!("{folder}/{name}={found}"));
        }
        got.push(format!("sound c={}", text(base.sound("c"))));
        got.push(format!("sound d={}", text(base.sound("d"))));
        got.push(format!("music c={}", text(base.music("c"))));
        got.push(format!("music d={}", text(base.music("d"))));
        Ok(got)
    });
    remove(&written);
    let got = got?;
    let expected = [
        "dialog.tlk=w1",
        "streammusic/a=w1",
        "streammusic/b=w2",
        "streammusic/c=w2",
        "streammusic/d=game",
        "movies/m=w2",
        "sound c=w2",
        "sound d=override",
        "music c=w2",
        "music d=game",
    ];
    if got == expected {
        Ok(got.join(", "))
    } else {
        Err(format!("got {got:?}, TSL picks {expected:?}"))
    }
}

/// Co-op compares the Workshop items too: the same items in the same
/// order are the same world, items holding the same file in another order
/// or an item's changed script are not, items with nothing in common are
/// in any order, and a refusal names the item; an item of movies alone
/// leaves the world as without it. Past the groups a handshake carries,
/// the later items share one group.
fn workshop_fingerprint() -> Result<String, String> {
    use crate::fingerprint::{LATER_WORKSHOP_ITEMS, MAX_WORLD_GROUPS, fingerprint};
    let files = [("modules/m01.rim", archive(b"RIM V1.0", &[]))];
    let item = |id: u64, script: &[u8]| (id, vec![("override/a.ncs", script.to_vec())]);
    let world = |name: &str, items: &Items| -> Result<_, String> {
        let (written, index) = workshop_install(name, &files, items)?;
        let print = fingerprint(Game::Kotor2, &index, None).map_err(|error| error.to_string());
        remove(&written);
        Ok(print?.world())
    };
    let names = |world: &crate::fingerprint::WorldData| -> Vec<String> {
        world.groups.iter().map(|(name, _)| name.clone()).collect()
    };
    let same = world("wsfp-a", &[item(1, b"a"), item(2, b"b")])?;
    if world("wsfp-b", &[item(1, b"a"), item(2, b"b")])? != same {
        return Err("the same items are another world".into());
    }
    let moved = world("wsfp-moved", &[item(2, b"b"), item(1, b"a")])?;
    let changed = world("wsfp-changed", &[item(1, b"a"), item(2, b"B")])?;
    for (what, other) in [
        ("in another order", &moved),
        ("with a changed script", &changed),
    ] {
        if other.digest == same.digest {
            return Err(format!("items {what} are the same world"));
        }
    }
    // Items vie only for what both hold: another order of items with no
    // world file in common (TSLRCM and a blaster pack) is the same world,
    // while a texture's TGA under another item's TPC, or one module's
    // `.rim` under another's `_s.rim`, is not.
    let file = |id: u64, path: &'static str| (id, vec![(path, b"x".to_vec())]);
    for (first, second, same_world) in [
        (file(1, "override/a.ncs"), file(2, "override/b.ncs"), true),
        (file(1, "override/a.tga"), file(2, "override/a.tpc"), false),
        (
            file(1, "modules/m02.rim"),
            file(2, "modules/m02_s.rim"),
            false,
        ),
    ] {
        let paths = [first.1[0].0, second.1[0].0];
        let ordered = world("wsfp-order", &[first.clone(), second.clone()])?;
        let swapped = world("wsfp-swap", &[second, first])?;
        if (ordered == swapped) != same_world {
            return Err(format!(
                "items holding {paths:?} in another order are {} world",
                if same_world { "another" } else { "the same" }
            ));
        }
    }
    let differ: Vec<&str> = changed
        .groups
        .iter()
        .filter(|group| !same.groups.contains(group))
        .map(|(name, _)| name.as_str())
        .collect();
    if differ != ["workshop/2"] {
        return Err(format!("a changed item's refusal names {differ:?}"));
    }
    let plain = world("wsfp-plain", &[])?;
    let movies = world("wsfp-movies", &[(5, vec![("movies/m.bik", b"m".to_vec())])])?;
    if movies != plain {
        return Err("an item of movies alone changes the world".into());
    }
    let many: Vec<_> = (1..=MAX_WORLD_GROUPS as u64 + 2)
        .map(|id| item(id, b"x"))
        .collect();
    let crowded = world("wsfp-many", &many)?;
    let crowded_names = names(&crowded);
    if crowded.groups.len() != MAX_WORLD_GROUPS
        || crowded_names.last().map(String::as_str) != Some(LATER_WORKSHOP_ITEMS)
    {
        return Err(format!("{} items' groups: {crowded_names:?}", many.len()));
    }
    Ok(format!(
        "a changed item names {}; {} items: {}",
        differ.join(" "),
        many.len(),
        crowded_names.join(" ")
    ))
}

/// A Steam library holding `game`'s install (`exe` its executable) and
/// Workshop items 11, 22 and 33 under `app`, with `extra` files.
fn steam_library(name: &str, exe: &str, app: u32, extra: &Files) -> Result<PathBuf, String> {
    let install = "steamapps/common/game";
    let content = format!("steamapps/workshop/content/{app}");
    let mut files: Vec<(String, Vec<u8>)> = vec![
        (format!("{install}/chitin.key"), empty_key()),
        (format!("{install}/dialog.tlk"), Vec::new()),
        (
            format!("{install}/modules/m01.rim"),
            archive(b"RIM V1.0", &[]),
        ),
        (format!("{install}/{exe}"), Vec::new()),
    ];
    for id in [11, 22, 33] {
        files.push((format!("{content}/{id}/override/a.ncs"), Vec::new()));
    }
    files.extend(
        extra
            .iter()
            .map(|(path, bytes)| (path.to_string(), bytes.clone())),
    );
    let files: Vec<(&str, Vec<u8>)> = files
        .iter()
        .map(|(path, bytes)| (path.as_str(), bytes.clone()))
        .collect();
    folder(name, &files)
}

/// TSL's Workshop items in Steam's subscription order: the list of the
/// account `appworkshop_208580.acf` names, by its numbering, without the
/// items disabled locally or not installed; in id order without a list.
/// The game loads the first 30 (`MAX_WORKSHOP_ITEMS`), and the note names
/// the installed ones past them. K1 reads none, and `KJK_WORKSHOP=off`
/// leaves them out.
fn workshop_subscriptions() -> Result<String, String> {
    let entry = |number: u32, id: u64, disabled: u8| {
        format!(
            "\t\"{number}\"\n\t{{\n\t\t\"publishedfileid\"\t\t\"{id}\"\n\t\t\"disabled_locally\"\t\t\"{disabled}\"\n\t}}\n"
        )
    };
    let list = |entries: &[(u64, u8)]| {
        let body: String = entries
            .iter()
            .enumerate()
            .map(|(number, (id, disabled))| entry(number as u32, *id, *disabled))
            .collect();
        format!("\"subscribedfiles\"\n{{\n\t\"appid\"\t\t\"208580\"\n{body}}}\n").into_bytes()
    };
    let acf = b"\"AppWorkshop\"\n{\n\t\"WorkshopItemDetails\"\n\t{\n\t\t\"11\"\n\t\t{\n\t\t\t\"subscribedby\"\t\t\"2\"\n\t\t}\n\t}\n}\n";
    let listed = [
        ("steamapps/workshop/appworkshop_208580.acf", acf.to_vec()),
        (
            "userdata/2/ugc/208580_subscriptions.vdf",
            list(&[(33, 0), (44, 0), (22, 1), (11, 0)]),
        ),
        ("userdata/9/ugc/208580_subscriptions.vdf", list(&[(22, 0)])),
    ];
    let unlisted = [("userdata/9/config/localconfig.vdf", Vec::new())];
    // 32 more installed items, with no list: the last 5 by id are past the
    // limit.
    let crowded: Vec<(String, Vec<u8>)> = (100..132)
        .map(|id| {
            (
                format!("steamapps/workshop/content/208580/{id}/override/a.ncs"),
                Vec::new(),
            )
        })
        .collect();
    let crowded: Vec<(&str, Vec<u8>)> = crowded
        .iter()
        .map(|(path, bytes)| (path.as_str(), bytes.clone()))
        .chain(unlisted.iter().cloned())
        .collect();
    let cases = [
        (
            "ws-steam-list",
            "swkotor2.exe",
            Game::Kotor2,
            208580,
            &listed[..],
            true,
        ),
        (
            "ws-steam-ids",
            "swkotor2.exe",
            Game::Kotor2,
            208580,
            &unlisted[..],
            true,
        ),
        (
            "ws-steam-off",
            "swkotor2.exe",
            Game::Kotor2,
            208580,
            &listed[..],
            false,
        ),
        (
            "ws-steam-k1",
            "swkotor.exe",
            Game::Kotor1,
            32370,
            &unlisted[..],
            true,
        ),
        (
            "ws-steam-limit",
            "swkotor2.exe",
            Game::Kotor2,
            208580,
            &crowded[..],
            true,
        ),
    ];
    let limited: Vec<u64> = [11, 22, 33].into_iter().chain(100..127).collect();
    let expected: [&[u64]; 5] = [&[33, 11], &[11, 22, 33], &[], &[], &limited];
    let past = format!(
        "5 more not loaded, past the game's limit of {MAX_WORKSHOP_ITEMS}: 127, 128, 129, 130, 131"
    );
    let mut told = Vec::new();
    for ((name, exe, game, app, extra, on), expected) in cases.into_iter().zip(expected) {
        let library = steam_library(name, exe, app, extra)?;
        let found =
            crate::install_at(game, &library.join("steamapps/common/game")).map(|mut install| {
                install.workshop = on;
                let items = install.workshop_items().items;
                let ids: Vec<u64> = items.iter().map(|item| item.id).collect();
                (
                    ids,
                    install.workshop_note().unwrap_or_else(|| "no note".into()),
                )
            });
        let _ = std::fs::remove_dir_all(&library);
        let (ids, note) = found?;
        if ids != expected {
            return Err(format!(
                "{name}: items {ids:?}, the game loads {expected:?} ({note})"
            ));
        }
        if (name == "ws-steam-limit") != note.contains(&past) {
            return Err(format!(
                "{name}: the note does not name the items past the limit: {note}"
            ));
        }
        told.push(format!("{name}: {note}"));
    }
    Ok(told.join("; "))
}

/// The native Linux port of TSL on Steam: the executable `KOTOR2` in the
/// install folder and the game's data in `steamassets/`. kjk finds it as a
/// TSL install that reads its data from `steamassets/`, and refuses it as
/// K1.
fn linux_tsl_steamassets() -> Result<String, String> {
    let files = [
        ("KOTOR2", Vec::new()),
        ("steamassets/chitin.key", empty_key()),
        ("steamassets/dialog.tlk", Vec::new()),
        ("steamassets/modules/m01.rim", archive(b"RIM V1.0", &[])),
        ("steamassets/streamvoice/a.wav", Vec::new()),
    ];
    let root = folder("linux-tsl", &files)?;
    let tsl = crate::install_at(Game::Kotor2, &root);
    let k1 = crate::install_at(Game::Kotor1, &root);
    let _ = std::fs::remove_dir_all(&root);
    let tsl = tsl?;
    if tsl.root != root || tsl.data_root != root.join("steamassets") {
        return Err(format!(
            "install folder {}, data folder {}",
            tsl.root.display(),
            tsl.data_root.display()
        ));
    }
    let refusal = match k1 {
        Ok(_) => return Err("taken as a K1 install too".into()),
        Err(reason) => reason,
    };
    Ok(format!("TSL from steamassets/; as K1: {refusal}"))
}

/// A PK3 that lists `names` as empty stored files. Only its central
/// directory and end record are written, because discovery reads no more.
fn pk3(names: &[&str]) -> Vec<u8> {
    let mut out = Vec::new();
    for name in names {
        let mut record = vec![0u8; 46];
        record[..4].copy_from_slice(&0x0201_4b50u32.to_le_bytes());
        record[28..30].copy_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&record);
        out.extend_from_slice(name.as_bytes());
    }
    let mut end = vec![0u8; 22];
    end[..4].copy_from_slice(&0x0605_4b50u32.to_le_bytes());
    let count = (names.len() as u16).to_le_bytes();
    end[8..10].copy_from_slice(&count);
    end[10..12].copy_from_slice(&count);
    end[12..16].copy_from_slice(&(out.len() as u32).to_le_bytes());
    out.extend_from_slice(&end);
    out
}

/// Jedi Outcast picked as Jedi Academy (Discord report of 2026-11-16, a
/// 1.04 CD install): its `base/` has PK3 files too, but no lightsaber files,
/// and a session then failed with "saber single_1: no .sab block". kjk
/// refuses the folder and names the game it found, so the setup screen and
/// the log say so. A Jedi Academy folder without its lightsaber files gets
/// the general message, and Jedi Academy's own layout is taken.
fn ja_refuses_outcast() -> Result<String, String> {
    let outcast = [
        ("GameData/jk2sp.exe", Vec::new()),
        ("GameData/jk2mp.exe", Vec::new()),
        ("GameData/base/assets0.pk3", pk3(&["ext_data/weapons.dat"])),
        ("GameData/base/assets1.pk3", pk3(&["maps/kejim_post.bsp"])),
        ("GameData/base/assets2.pk3", pk3(&["maps/ffa_bespin.bsp"])),
        (
            "GameData/base/assets5.pk3",
            pk3(&["sound/chars/kyle/a.mp3"]),
        ),
    ];
    let stripped = [
        ("GameData/jasp.exe", Vec::new()),
        ("GameData/base/assets0.pk3", pk3(&["ext_data/weapons.dat"])),
    ];
    let academy = [
        ("GameData/jasp.exe", Vec::new()),
        ("GameData/base/assets0.pk3", pk3(&["ext_data/weapons.dat"])),
        (
            "GameData/base/assets1.pk3",
            pk3(&["ext_data/sabers/single.sab"]),
        ),
    ];
    let cases: [(&str, &Files, Option<&str>); 3] = [
        ("ja-outcast", &outcast, Some(crate::discover::OUTCAST)),
        ("ja-stripped", &stripped, Some(crate::discover::NO_SABERS)),
        ("ja-academy", &academy, None),
    ];
    let mut told = Vec::new();
    for (name, files, refusal) in cases {
        let root = folder(name, files)?;
        let found = crate::install_at(Game::JediAcademy, &root);
        remove(&[root]);
        match (found, refusal) {
            (Ok(_), None) => told.push(format!("{name} taken")),
            (Err(reason), Some(expected)) if reason == expected => {
                told.push(format!("{name} refused: {reason}"));
            }
            (Ok(_), Some(_)) => return Err(format!("{name} taken as Jedi Academy")),
            (Err(reason), _) => return Err(format!("{name} refused: {reason}")),
        }
    }
    Ok(told.join("; "))
}

/// Movie Duels with some of its archives (issue 548): a player had Movie
/// Duels' folder, but no archive of it with its characters' skeletons or
/// its hilts. kjk took it as installed and offered its stances and hilts,
/// which then did nothing. Such an install now counts as incomplete, and
/// only one with both counts as installed, its archives in `MD/` or
/// copied into `base/`.
fn movie_duels_incomplete() -> Result<String, String> {
    let ja = |extra: &[(&'static str, Vec<u8>)]| {
        let mut files = vec![
            ("GameData/base/assets0.pk3", pk3(&["ext_data/weapons.dat"])),
            (
                "GameData/base/assets1.pk3",
                pk3(&["ext_data/sabers/single.sab"]),
            ),
        ];
        files.extend_from_slice(extra);
        files
    };
    let skeletons = || pk3(&["models/players/_humanoid_kotor/_humanoid.gla"]);
    let hilts = || pk3(&["ext_data/sabers/vader.sab"]);
    let models = || pk3(&["models/players/vader/model.glm"]);
    type Files<'a> = Vec<(&'a str, Vec<u8>)>;
    let cases: [(&str, Files, MovieDuels); 5] = [
        ("md-none", ja(&[]), MovieDuels::Absent),
        (
            "md-models-only",
            ja(&[("GameData/MD/MD_Models_PT.pk3", models())]),
            MovieDuels::Incomplete {
                skeletons: false,
                hilts: false,
            },
        ),
        (
            "md-no-hilts",
            ja(&[("GameData/MD/MD_Assets2.pk3", skeletons())]),
            MovieDuels::Incomplete {
                skeletons: true,
                hilts: false,
            },
        ),
        (
            "md-whole",
            ja(&[
                ("GameData/MD/MD_Assets2.pk3", skeletons()),
                ("GameData/MD/MD_Hilts.pk3", hilts()),
                ("GameData/MD/MD_Models_PT.pk3", models()),
            ]),
            MovieDuels::Installed,
        ),
        (
            "md-in-base",
            ja(&[
                ("GameData/base/MD_Assets2.pk3", skeletons()),
                ("GameData/base/md_hilts.PK3", hilts()),
            ]),
            MovieDuels::Installed,
        ),
    ];
    let mut told = Vec::new();
    for (name, files, expected) in cases {
        let root = folder(name, &files)?;
        let found = crate::install_at(Game::JediAcademy, &root).map(|install| {
            (
                install.movie_duels_found(),
                install.movie_duels(),
                install.ja_mod_note(),
            )
        });
        remove(&[root]);
        let (state, offered, note) = found?;
        if state != expected || offered != (expected == MovieDuels::Installed) {
            return Err(format!("{name}: {state:?}, offered {offered}"));
        }
        let says_missing = note
            .as_deref()
            .is_some_and(|note| note.contains("missing or damaged"));
        if says_missing != matches!(expected, MovieDuels::Incomplete { .. }) {
            return Err(format!("{name}: the log says {note:?}"));
        }
        told.push(format!("{name} {state:?}"));
    }
    Ok(told.join("; "))
}

/// Every probe: its name and what it found, or why it failed.
pub fn probes() -> Vec<(&'static str, Result<String, String>)> {
    vec![
        ("override_beats_module", override_beats_module()),
        (
            "override_table_beats_globals",
            override_table_beats_globals(),
        ),
        ("mod_stands_for_rims", mod_stands_for_rims()),
        ("ranks_between_archives", ranks_between_archives()),
        ("override_subfolders", override_subfolders()),
        ("texture_formats", texture_formats()),
        ("override_fingerprint", override_fingerprint()),
        (
            "world_fingerprint_reads_world",
            world_fingerprint_reads_world(),
        ),
        ("workshop_override_and_mods", workshop_override_and_mods()),
        ("workshop_module_rims", workshop_module_rims()),
        ("workshop_talk_and_streams", workshop_talk_and_streams()),
        ("workshop_fingerprint", workshop_fingerprint()),
        ("workshop_subscriptions", workshop_subscriptions()),
        ("linux_tsl_steamassets", linux_tsl_steamassets()),
        ("ja_refuses_outcast", ja_refuses_outcast()),
        ("movie_duels_incomplete", movie_duels_incomplete()),
    ]
}
