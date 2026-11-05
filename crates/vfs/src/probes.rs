//! The resolver's precedence on installs written here (journal
//! 2026-11-05-override-precedence): `override/` over a module's archives, a
//! `.mod` standing for its RIMs and `_dlg.erf`, `patch.erf` over a module,
//! a module's `.rim` over its `_s.rim`, and TSL's override subfolders.

use std::path::{Path, PathBuf};

use fmt_odyssey::ResType;

use crate::resolver::{Resolver, TextureQuality};
use crate::{DirIndex, Game};

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

/// A fresh install folder for one probe, holding `files` (path, bytes).
fn install(name: &str, files: &[(&str, Vec<u8>)]) -> Result<PathBuf, String> {
    let root = std::env::temp_dir().join(format!("kjk-vfs-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for (path, bytes) in std::iter::once(&("chitin.key", empty_key())).chain(files) {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().expect("a file in a folder"))
            .map_err(|error| error.to_string())?;
        std::fs::write(&path, bytes).map_err(|error| error.to_string())?;
    }
    Ok(root)
}

/// What `name` resolves to in `module` of the install at `root`: its
/// layer and bytes, or `-` when nothing holds it.
fn resolve(root: &Path, game: Game, module: &str, names: &[(&str, ResType)]) -> Vec<String> {
    let resolver = DirIndex::build(root)
        .map_err(|error| error.to_string())
        .and_then(|index| {
            Resolver::open(game, &index, Some(module), TextureQuality::High)
                .map_err(|error| error.to_string())
        });
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

/// Every probe: its name and what it found, or why it failed.
pub fn probes() -> Vec<(&'static str, Result<String, String>)> {
    vec![
        ("override_beats_module", override_beats_module()),
        ("mod_stands_for_rims", mod_stands_for_rims()),
        ("ranks_between_archives", ranks_between_archives()),
        ("override_subfolders", override_subfolders()),
    ]
}
