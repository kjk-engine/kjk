//! The resolver's precedence on installs written here (journal
//! 2026-11-05-override-precedence): `override/` over a module's archives, a
//! `.mod` standing for its RIMs and `_dlg.erf`, `patch.erf` over a module,
//! a module's `.rim` over its `_s.rim`, TSL's override subfolders,
//! co-op's check covering just the override files the resolver reads, and
//! the Steam Workshop items TSL loads, in its load order.

use std::path::{Path, PathBuf};

use fmt_odyssey::ResType;

use crate::resolver::{Resolver, TextureQuality};
use crate::{DirIndex, Game, WorkshopFolder};

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
/// is the sound of its name as a `.wav` is.
fn workshop_talk_and_streams() -> Result<String, String> {
    let files = [
        ("dialog.tlk", b"game".to_vec()),
        ("streammusic/a.wav", b"game".to_vec()),
        ("streammusic/c.wav", b"game".to_vec()),
        ("streammusic/d.wav", b"game".to_vec()),
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
        for (folder, name, restype) in [
            ("streammusic", "a", ResType::WAV),
            ("streammusic", "b", ResType::WAV),
            ("streammusic", "c", ResType::WAV),
            ("streammusic", "d", ResType::WAV),
            ("movies", "m", ResType::BIK),
        ] {
            let bytes = base
                .stream(folder, name, restype)
                .map(|location| location.read());
            let text = match bytes {
                Some(Ok(bytes)) => String::from_utf8_lossy(&bytes).into_owned(),
                Some(Err(error)) => error.to_string(),
                None => "-".into(),
            };
            got.push(format!("{folder}/{name}={text}"));
        }
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
    ];
    if got == expected {
        Ok(got.join(", "))
    } else {
        Err(format!("got {got:?}, TSL picks {expected:?}"))
    }
}

/// Co-op compares the Workshop items too: the same items in the same
/// order are the same world, items in another order or an item's changed
/// script are not, and a refusal names the item; an item of movies alone
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
/// K1 reads none, and `KJK_WORKSHOP=off` leaves them out.
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
    ];
    let expected: [&[u64]; 4] = [&[33, 11], &[11, 22, 33], &[], &[]];
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
        told.push(format!("{name}: {note}"));
    }
    Ok(told.join("; "))
}

/// Every probe: its name and what it found, or why it failed.
pub fn probes() -> Vec<(&'static str, Result<String, String>)> {
    vec![
        ("override_beats_module", override_beats_module()),
        ("mod_stands_for_rims", mod_stands_for_rims()),
        ("ranks_between_archives", ranks_between_archives()),
        ("override_subfolders", override_subfolders()),
        ("override_fingerprint", override_fingerprint()),
        ("workshop_override_and_mods", workshop_override_and_mods()),
        ("workshop_module_rims", workshop_module_rims()),
        ("workshop_talk_and_streams", workshop_talk_and_streams()),
        ("workshop_fingerprint", workshop_fingerprint()),
        ("workshop_subscriptions", workshop_subscriptions()),
    ]
}
