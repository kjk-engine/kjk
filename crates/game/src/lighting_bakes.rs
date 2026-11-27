//! The new lighting as a download: kjk's baked lightmaps for a game, checked
//! against the player's files, installed into the store and loaded by the
//! game (`docs/LIGHTING-PLAN.md`, step 10).
//!
//! The bakes are made for KOTOR with the K1 Community Patch and for
//! KOTOR II with TSLRCM. The launcher downloads one zip for a game, with its
//! manifest beside it. kjk downloads nothing. Its commands take the two
//! files from the launcher's folder (`kjk lighting check` and `install`).
//!
//! - The manifest (`Manifest`) is plain text in the style of
//!   `fingerprint.txt`. It names the game, the bake's format, the patch the
//!   bake needs, the zip with its size and its BLAKE3, and one line for each
//!   area with the fingerprint of the files the area was baked from.
//! - The second manifest (`MANIFEST_2`, read by `Manifest::parse_exact`)
//!   holds the same lines and, after each area's line, the area's exact
//!   lines (`bake_fingerprint::Exact`). kjk v0.21.0 does not read it, so the
//!   first manifest stays beside it unchanged.
//! - `check` compares the manifest's fingerprints with the player's files
//!   (`bake_fingerprint`). An area with exact lines is judged lightmap by
//!   lightmap. It matches fully when every baked lightmap matches, partly
//!   when some do, and not at all when none do. An area without them
//!   matches when its rooms match. The install is allowed when at least one
//!   area matches fully or partly (`allowed`). An area whose rooms match but
//!   whose placeables do not counts as a match, as the game then shows its
//!   lightmaps. The answer is kept in the store, keyed by the names, sizes
//!   and times of the files a bake reads (`files_key`) and the manifest, so
//!   a second check is instant.
//! - `refresh` adds the exact lines of a second manifest to an installed
//!   download of the same zip, without the zip.
//! - `restamp` makes the second manifest of a published download from its
//!   zip and the files on this machine (`kjk lighting restamp`).
//! - For an area whose rooms differ, the check notes the mods whose files
//!   replace the game's own among its room models and stored lightmaps: a
//!   Steam Workshop item, the override folder or a `.mod` in the modules
//!   folder (`bake_fingerprint::room_mods`). A refusal names them, so the
//!   player sees which mod stops the install. TSLRCM's own Workshop item
//!   (485537937) is never named, since the KOTOR II bakes match it.
//! - `install` puts the zip's area folders under
//!   `<store>/lighting/<k1|tsl>/<area>/`, with the manifest beside them in
//!   `MANIFEST`. The store's gc never removes them (`store::KEPT`).
//! - The game loads those folders when the options' Lighting is at Enhanced
//!   Lightmaps (`chosen`), and it checks each area's fingerprint again when
//!   the area loads (`viewer::choose_bakes`). A setting changed in the
//!   options takes effect the next time kjk starts.
//!
//! None of this needs the `probes` feature: the player's build has it.
//! `kjk lighting pack`, which makes the zip and the manifest, is a
//! developer's command (`crates/launcher/src/lighting_pack.rs`).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use store::Store;
use vfs::Game;

use crate::bake_fingerprint::{self, Exact, Fingerprint, Fit, Verdict};

/// The file in a game's installed folder that holds its manifest, in the
/// form kjk v0.21.0 reads.
pub const MANIFEST: &str = "manifest.txt";

/// The file in a game's installed folder that holds its second manifest,
/// with the exact lines of each area (`Manifest::exact_text`).
pub const MANIFEST_2: &str = "manifest-2.txt";

/// The first line of a second manifest that is not a comment.
const SECOND: &str = "manifest 2";

/// The value of `display.lighting` that turns the baked lightmaps on.
pub const SETTING_VALUE: &str = "baked";

/// The store's kind for the answers of `check`. Raise its number when the
/// answer for the same files and manifest would change.
const CHECKS: &str = "lighting-check-v4";

/// The files of a bake's set that the game loads, and so the files a
/// download holds: the lightmaps (PNG), `fill.txt`, `floor.txt`,
/// `unlit.txt`, `stored.txt`, `placeables.txt` and `fingerprint.txt`. The
/// probe files are read only by trial switches, and `detail-bake.txt` is
/// the bake's log.
pub fn shipped(file: &str) -> bool {
    let file = file.to_ascii_lowercase();
    file.ends_with(".png")
        || file.ends_with(".klm")
        || matches!(
            file.as_str(),
            "fill.txt"
                | "floor.txt"
                | "unlit.txt"
                | "placeables.txt"
                | crate::stored_fill::STORED_FILE
                | bake_fingerprint::FILE
        )
}

/// The zip of a download, as its manifest names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Archive {
    /// Its file name.
    pub file: String,
    /// Its BLAKE3, hexadecimal.
    pub blake3: String,
    /// Its size in bytes.
    pub bytes: u64,
}

/// One area of a download: its name, its module and the fingerprint of the
/// files it was baked from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Area {
    pub area: String,
    pub module: String,
    pub rooms: String,
    pub placeables: String,
    /// The area's exact lines, from a second manifest. `None` for an area
    /// that is judged by `rooms` and `placeables` alone.
    pub exact: Option<Exact>,
}

/// A download's manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub game: Game,
    /// The bake's format (`bake_fingerprint::FORMAT` when it was made).
    pub format: u32,
    /// The patch the bakes were made with, as the player is told it:
    /// "TSLRCM" or "the K1 Community Patch".
    pub needs: String,
    pub archive: Archive,
    pub areas: Vec<Area>,
}

/// The patch a game's bakes are made with.
pub fn needs(game: Game) -> &'static str {
    match game {
        Game::Kotor1 => "the K1 Community Patch",
        _ => "TSLRCM",
    }
}

/// A game's name as the player is told it. These are the launcher's names
/// for the games, since the player reads these messages in the launcher.
pub fn game_name(game: Game) -> &'static str {
    match game {
        Game::Kotor1 => "KOTOR",
        Game::Kotor2 => "KOTOR II",
        Game::JediAcademy => "Jedi Academy",
    }
}

/// What went wrong, for the launcher to act on. The JSON answers of
/// `kjk lighting` give it as `kind`, beside the sentence `why`. The set is
/// closed: a new value is a change the launcher must learn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// kjk found no folder for its store.
    NoStore,
    /// kjk may not write into its store's folder.
    StoreNotWritable,
    /// The disk of the store is full.
    DiskFull,
    /// Another program holds a file of the store open, as a running kjk
    /// does.
    InUse,
    /// kjk did not find the game.
    GameNotFound,
    /// The game's files could not be read.
    GameUnreadable,
    /// The manifest (the list of areas) could not be read from its file.
    ListUnreadable,
    /// The manifest is not a manifest this kjk reads.
    ListInvalid,
    /// The manifest is for another game than the one asked for.
    WrongGame,
    /// The zip could not be opened.
    ZipUnreadable,
    /// The zip's size or BLAKE3 is not the one the manifest gives, as for a
    /// damaged or incomplete download.
    ZipMismatch,
    /// The zip does not unpack, or holds files the manifest does not list.
    ZipDamaged,
    /// No area of the player's files matches the download.
    FilesDiffer,
    /// The download is of another bake format than this kjk reads.
    BakeFormat,
    /// No download of the zip the manifest names is installed, or an area
    /// folder of it is missing (`kjk lighting refresh`).
    NotInstalled,
    /// Anything else. `why` says what.
    Other,
}

impl Kind {
    /// The kind of an error in writing into the store.
    pub fn of_write(error: &std::io::Error) -> Kind {
        // ERROR_SHARING_VIOLATION and ERROR_LOCK_VIOLATION on Windows;
        // ERROR_HANDLE_DISK_FULL and ERROR_DISK_FULL there, ENOSPC elsewhere.
        let code = error.raw_os_error();
        let (in_use, full) = if cfg!(windows) {
            (
                matches!(code, Some(32 | 33)),
                matches!(code, Some(39 | 112)),
            )
        } else {
            (false, code == Some(28))
        };
        if full || error.kind() == std::io::ErrorKind::StorageFull {
            Kind::DiskFull
        } else if in_use {
            Kind::InUse
        } else if error.kind() == std::io::ErrorKind::PermissionDenied {
            Kind::StoreNotWritable
        } else {
            Kind::Other
        }
    }
}

/// Why a command of the lighting download failed: its kind and a sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub kind: Kind,
    pub why: String,
}

impl Failure {
    pub fn new(kind: Kind, why: impl Into<String>) -> Failure {
        Failure {
            kind,
            why: why.into(),
        }
    }

    /// A failure to write `path` into the store.
    fn write(path: &Path, error: &std::io::Error) -> Failure {
        Failure::new(
            Kind::of_write(error),
            format!("{}: {error}", path.display()),
        )
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.why)
    }
}

impl From<Failure> for String {
    fn from(failure: Failure) -> String {
        failure.why
    }
}

impl Area {
    /// The fingerprint the area was baked from, in `format`.
    pub fn stamp(&self, format: u32) -> Fingerprint {
        Fingerprint {
            area: self.area.clone(),
            format,
            rooms: self.rooms.clone(),
            placeables: self.placeables.clone(),
            exact: self.exact.clone(),
        }
    }
}

/// The lines of a manifest that are not comments, each with its number
/// from 1, its text and its words.
fn manifest_lines(text: &str) -> impl Iterator<Item = (usize, &str, Vec<&str>)> {
    (text.lines().enumerate())
        .map(|(number, line)| (number + 1, line.trim()))
        .filter(|(_, line)| !line.is_empty() && !line.starts_with('#'))
        .map(|(number, line)| (number, line, line.split_whitespace().collect()))
}

/// Why line `number` of a manifest does not read.
fn bad_line(number: usize, line: &str) -> String {
    format!("line {number} of the manifest does not read: {line}")
}

/// The lines of a manifest read so far.
#[derive(Default)]
struct Read {
    game: Option<Game>,
    format: Option<u32>,
    needs: Option<String>,
    archive: Option<Archive>,
    areas: Vec<Area>,
}

impl Read {
    /// Takes one line of the first manifest's form, split into `words`.
    /// `Ok(false)` when it is no such line, and an error when it is one
    /// that does not read.
    fn take(&mut self, words: &[&str], line: &str) -> Result<bool, ()> {
        match words {
            ["game", key] => {
                self.game = Some(
                    Game::from_key(key)
                        .filter(|game| *game != Game::JediAcademy)
                        .ok_or(())?,
                );
            }
            ["format", number] => self.format = Some(number.parse().map_err(|_| ())?),
            ["needs", _, ..] => self.needs = Some(line["needs".len()..].trim().to_string()),
            ["archive", file, "blake3", hex, "bytes", bytes] => {
                self.archive = Some(Archive {
                    file: (*file).to_string(),
                    blake3: hex.to_ascii_lowercase(),
                    bytes: bytes.parse().map_err(|_| ())?,
                });
            }
            [
                "area",
                area,
                "module",
                module,
                "rooms",
                rooms,
                "placeables",
                placeables,
            ] => {
                self.areas.push(Area {
                    area: area.to_ascii_lowercase(),
                    module: module.to_ascii_lowercase(),
                    rooms: (*rooms).to_string(),
                    placeables: (*placeables).to_string(),
                    exact: None,
                });
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// The manifest, or the first line it lacks.
    fn finished(self) -> Result<Manifest, String> {
        let missing = |what: &str| format!("the manifest has no {what} line");
        Ok(Manifest {
            game: self.game.ok_or_else(|| missing("game"))?,
            format: self.format.ok_or_else(|| missing("format"))?,
            needs: self.needs.ok_or_else(|| missing("needs"))?,
            archive: self.archive.ok_or_else(|| missing("archive"))?,
            areas: self.areas,
        })
    }
}

impl Manifest {
    /// The manifest in `text`, or why it does not read. This is the form
    /// kjk v0.21.0 reads, and every line must be one of its lines.
    pub fn parse(text: &str) -> Result<Manifest, String> {
        let mut read = Read::default();
        for (number, line, words) in manifest_lines(text) {
            if read.take(&words, line) != Ok(true) {
                return Err(bad_line(number, line));
            }
        }
        read.finished()
    }

    /// The second manifest in `text` (`MANIFEST_2`), or why it does not
    /// read. Its first line is `manifest 2`. Each area's line may be
    /// followed by its exact lines, from an `exact` line that gives their
    /// version (`bake_fingerprint::Exact::lines`). The exact lines of an
    /// area whose version is not `bake_fingerprint::EXACT` are skipped, and
    /// that area is judged by `rooms` and `placeables` alone.
    pub fn parse_exact(text: &str) -> Result<Manifest, String> {
        let mut lines = manifest_lines(text);
        if lines.next().map(|(_, line, _)| line) != Some(SECOND) {
            return Err(format!(
                "the second manifest does not start with \"{SECOND}\""
            ));
        }
        let mut read = Read::default();
        // The exact lines of the last area, and whether their version is
        // the one this kjk reads.
        let mut exact: Option<(Exact, bool)> = None;
        let close = |read: &mut Read, exact: &mut Option<(Exact, bool)>| {
            if let (Some(area), Some((lines, true))) = (read.areas.last_mut(), exact.take()) {
                area.exact = Some(lines);
            }
        };
        for (number, line, words) in lines {
            let bad = || bad_line(number, line);
            if let ["exact", version] = words.as_slice() {
                if read.areas.is_empty() || exact.is_some() {
                    return Err(bad());
                }
                let version: u32 = version.parse().map_err(|_| bad())?;
                exact = Some((Exact::default(), version == bake_fingerprint::EXACT));
                continue;
            }
            if let Some((lines, known)) = exact.as_mut()
                && words.first() != Some(&"area")
            {
                match lines.read_line(&words) {
                    Some(Ok(())) => {}
                    _ if !*known => {}
                    _ => return Err(bad()),
                }
                continue;
            }
            close(&mut read, &mut exact);
            if read.take(&words, line) != Ok(true) {
                return Err(bad());
            }
        }
        close(&mut read, &mut exact);
        read.finished()
    }

    /// The manifest in the file at `path`.
    pub fn read(path: &Path) -> Result<Manifest, String> {
        Manifest::load(path).map_err(String::from)
    }

    /// The manifest in the file at `path`, of either form, or what went
    /// wrong: the file does not read (`ListUnreadable`), or its text is not
    /// a manifest (`ListInvalid`). A file whose first line is `manifest 2`
    /// is read as a second manifest (`parse_exact`).
    pub fn load(path: &Path) -> Result<Manifest, Failure> {
        let text = std::fs::read_to_string(path).map_err(|error| {
            let kind = if error.kind() == std::io::ErrorKind::InvalidData {
                Kind::ListInvalid
            } else {
                Kind::ListUnreadable
            };
            Failure::new(kind, format!("{}: {error}", path.display()))
        })?;
        let second = manifest_lines(&text).next().map(|(_, line, _)| line) == Some(SECOND);
        let parsed = if second {
            Manifest::parse_exact(&text)
        } else {
            Manifest::parse(&text)
        };
        parsed.map_err(|why| Failure::new(Kind::ListInvalid, format!("{}: {why}", path.display())))
    }

    /// Whether any area has exact lines, as in a second manifest.
    pub fn is_exact(&self) -> bool {
        self.areas.iter().any(|area| area.exact.is_some())
    }

    /// The lines before the areas, after the comment `about`.
    fn head(&self, about: &str) -> String {
        let mut text = about.to_string();
        let _ = writeln!(text, "game {}", self.game.key());
        let _ = writeln!(text, "format {}", self.format);
        let _ = writeln!(text, "needs {}", self.needs);
        let _ = writeln!(
            text,
            "archive {} blake3 {} bytes {}",
            self.archive.file, self.archive.blake3, self.archive.bytes
        );
        text
    }

    /// An area's line.
    fn area_line(text: &mut String, area: &Area) {
        let _ = writeln!(
            text,
            "area {} module {} rooms {} placeables {}",
            area.area, area.module, area.rooms, area.placeables
        );
    }

    /// The manifest's text in the first form, which `parse` and kjk v0.21.0
    /// read back. The exact lines are left out.
    pub fn text(&self) -> String {
        let mut text = self.head(
            "# kjk's lighting download (crates/game/src/lighting_bakes.rs).\n\
             # game is the game the bakes are for, and format the meaning of their files.\n\
             # needs is the patch the bakes were made with. archive is the zip, its BLAKE3 and its size.\n\
             # Each area line gives the area's module and the fingerprint of the files it was baked\n\
             # from (bake_fingerprint.rs). kjk lighting check compares them with the player's files.\n",
        );
        for area in &self.areas {
            Manifest::area_line(&mut text, area);
        }
        text
    }

    /// The manifest's text as a second manifest, which `parse_exact` reads
    /// back: the first form's lines, each area's line followed by its exact
    /// lines.
    pub fn exact_text(&self) -> String {
        let mut text = self.head(
            "# kjk's lighting download, second manifest (crates/game/src/lighting_bakes.rs).\n\
             # It holds the lines of the first manifest. After each area's line come its exact lines,\n\
             # which judge each baked lightmap and each lit placeable by itself (bake_fingerprint.rs).\n\
             # An area with no exact lines is judged by its rooms and placeables digests.\n\
             manifest 2\n",
        );
        for area in &self.areas {
            Manifest::area_line(&mut text, area);
            if let Some(exact) = &area.exact {
                text.push_str(&exact.lines());
            }
        }
        text
    }
}

/// An area of a check that falls back to the game's own lighting, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FallBack {
    pub area: String,
    pub why: String,
    /// The mods whose files replace the game's own among the area's room
    /// models and stored lightmaps, when its rooms differ
    /// (`bake_fingerprint::room_mods`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mods: Vec<String>,
}

/// The Steam Workshop item of TSLRCM.
pub const TSLRCM_WORKSHOP_ITEM: u64 = 485_537_937;

/// A mod as `bake_fingerprint::mod_place` names it, as the player is told
/// it, at the start of a sentence.
pub fn place(name: &str) -> String {
    match name.strip_prefix("workshop/") {
        Some(id) => format!("Steam Workshop item {id}"),
        None if name == "override" => "The override folder".to_string(),
        None if name == "modules" => "The .mod files in the modules folder".to_string(),
        None => name.to_string(),
    }
}

/// `count` followed by `one` when it is 1 and by `many` otherwise, as in
/// "1 area" and "2 areas".
fn counted(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

/// A list of areas for a sentence: the first three by name, then how many
/// more there are.
fn some_areas(areas: &[&str]) -> String {
    const SHOWN: usize = 3;
    match areas {
        [] => String::new(),
        [one] => (*one).to_string(),
        _ if areas.len() <= SHOWN => format!(
            "{} and {}",
            areas[..areas.len() - 1].join(", "),
            areas[areas.len() - 1]
        ),
        _ => format!(
            "{} and {} more",
            areas[..SHOWN].join(", "),
            areas.len() - SHOWN
        ),
    }
}

/// A baked lightmap of a partly matching area whose rooms differ in the
/// player's files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Differs {
    pub lightmap: String,
    /// The room models of the player's files that draw it. None when no
    /// room draws it any more.
    pub rooms: Vec<String>,
}

/// An area of a check where some of the baked lightmaps match and the
/// others are drawn with the game's own lighting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Partial {
    pub area: String,
    /// How many of its baked lightmaps match.
    pub matching: usize,
    /// How many baked lightmaps it has.
    pub lightmaps: usize,
    /// The baked lightmaps that do not match.
    pub differ: Vec<Differs>,
    /// Whether some of its lit placeables differ, so that they keep their
    /// usual light.
    pub placeables_differ: bool,
    /// The mods whose files replace the game's own among the area's room
    /// models and stored lightmaps (`bake_fingerprint::room_mods`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mods: Vec<String>,
}

/// What `check` found.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    /// The game's key (`k1` or `tsl`).
    pub game: String,
    /// The patch the bakes need (`Manifest::needs`).
    pub needs: String,
    /// How many areas the manifest lists.
    pub areas: usize,
    /// The areas whose files all match: the game shows the whole bake.
    pub matched: Vec<String>,
    /// The areas whose rooms match and whose placeables do not: the game
    /// shows all the lightmaps, and the placeables that differ take the
    /// light probes.
    pub placeables_differ: Vec<String>,
    /// The areas where some baked lightmaps match and others do not: the
    /// game shows the ones that match.
    #[serde(default)]
    pub partial: Vec<Partial>,
    /// The areas the game draws with its own lighting.
    pub fall_back: Vec<FallBack>,
    /// Whether the download may be installed (`allowed`).
    pub allowed: bool,
    /// Whether the answer came from the store, from an earlier check.
    #[serde(skip)]
    pub cached: bool,
}

/// Whether a download may be installed, when `showing` of its areas show
/// some of the new lighting over the player's files. One area is enough.
/// The player then sees the new lighting where the files match, and the
/// game's own lighting elsewhere.
pub fn allowed(showing: usize) -> bool {
    showing > 0
}

impl Report {
    /// The areas whose lightmaps the game shows in full.
    pub fn matching(&self) -> usize {
        self.matched.len() + self.placeables_differ.len()
    }

    /// The areas whose lightmaps the game shows in full or in part.
    pub fn showing(&self) -> usize {
        self.matching() + self.partial.len()
    }

    /// What the player is told when the download may not be installed:
    /// that no area matches, then which mods replace the rooms of the areas
    /// that do not (`mod_sentences`).
    pub fn refusal(&self) -> Option<String> {
        if self.allowed {
            return None;
        }
        let game = Game::from_key(&self.game).map_or("the game", game_name);
        let mut lines = vec![format!(
            "None of the {} areas of the new lighting can be shown, because your game's files differ from the files the lighting was made from. The new lighting was made for {game} with {}.",
            self.areas, self.needs,
        )];
        lines.extend(self.mod_sentences());
        Some(lines.join(" "))
    }

    /// Which mods replace the room models or stored lightmaps of the areas
    /// that fall back in full or in part, one sentence for each mod, the
    /// mod that replaces the most areas first. TSLRCM's Workshop item is
    /// left out, since the KOTOR II lighting was made with it. Nothing when
    /// no other mod's files are among them.
    pub fn mod_sentences(&self) -> Vec<String> {
        let tslrcm = format!("workshop/{TSLRCM_WORKSHOP_ITEM}");
        let differing = (self.fall_back.iter())
            .map(|fall| (fall.area.as_str(), &fall.mods))
            .chain(
                self.partial
                    .iter()
                    .map(|part| (part.area.as_str(), &part.mods)),
            );
        let mut by_mod: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for (area, mods) in differing {
            for name in mods.iter().filter(|name| **name != tslrcm) {
                by_mod.entry(name).or_default().push(area);
            }
        }
        if by_mod.is_empty() {
            return Vec::new();
        }
        let mut by_mod: Vec<(&str, Vec<&str>)> = by_mod.into_iter().collect();
        by_mod.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(b.0)));
        let mut lines = vec![format!(
            "In {} the rooms or their lightmaps differ from the ones the lighting was made from.",
            counted(self.fall_back.len() + self.partial.len(), "area", "areas")
        )];
        for (name, areas) in by_mod {
            lines.push(format!(
                "{} replaces these files in {} of those areas: {}.",
                place(name),
                areas.len(),
                some_areas(&areas)
            ));
        }
        lines
    }

    /// Why the download may not be installed, when it may not: the
    /// manifest's bake format is not the one this kjk reads, or no area
    /// matches.
    pub fn refusal_kind(&self, format: u32) -> Option<Kind> {
        if self.allowed {
            None
        } else if format != bake_fingerprint::FORMAT {
            Some(Kind::BakeFormat)
        } else {
            Some(Kind::FilesDiffer)
        }
    }

    /// The report as plain sentences, one a line.
    pub fn sentences(&self) -> Vec<String> {
        if let Some(refusal) = self.refusal() {
            return vec![refusal];
        }
        let mut lines = vec![format!(
            "The new lighting can be installed. Of the {}, {} it in full, {} it in part, and {} the game's own lighting.",
            counted(self.areas, "area", "areas"),
            counted(self.matching(), "shows", "show"),
            counted(self.partial.len(), "shows", "show"),
            counted(self.fall_back.len(), "keeps", "keep")
        )];
        let names = |areas: &mut dyn Iterator<Item = &str>| areas.collect::<Vec<_>>().join(", ");
        if !self.partial.is_empty() {
            lines.push(format!(
                "In {} some rooms keep the game's own lighting, because your game's files differ there from the files the lighting was made from: {}.",
                counted(self.partial.len(), "area", "areas"),
                names(&mut self.partial.iter().map(|part| part.area.as_str()))
            ));
        }
        if !self.placeables_differ.is_empty() {
            lines.push(format!(
                "In {} the objects in the room differ from the ones the lighting was made with, so those objects keep their usual light: {}.",
                counted(self.placeables_differ.len(), "area", "areas"),
                self.placeables_differ.join(", ")
            ));
        }
        if !self.fall_back.is_empty() {
            lines.push(format!(
                "{} the game's own lighting, because your game's files differ from the files the lighting was made from: {}.",
                counted(self.fall_back.len(), "area keeps", "areas keep"),
                names(&mut self.fall_back.iter().map(|fall| fall.area.as_str()))
            ));
        }
        lines.extend(self.mod_sentences().into_iter().skip(1));
        lines
    }
}

/// How far a check has come: `done` of `total` areas, and the area just
/// compared.
pub struct Progress<'a> {
    pub done: usize,
    pub total: usize,
    pub area: &'a str,
}

/// The groups of an install's files that a bake reads: the rooms, the
/// placeables and their models, the lightmaps, the layouts and the tables.
/// The music, sounds, voices and movies are left out.
const BAKE_GROUPS: &[&str] = &[
    "chitin.key",
    "patch.erf",
    "data",
    "modules",
    "override",
    "rims",
    "texturepacks",
];

/// The groups of a Steam Workshop item's files that a bake reads.
const BAKE_ITEM_GROUPS: &[&str] = &["modules", "override"];

/// A digest of the names, sizes and modification times of the files of
/// `index` that a bake may read (`BAKE_GROUPS`), with the Workshop items in
/// their load order. It reads no file's contents, so it takes about as long
/// as listing the files. Adding, removing or changing a mod's file changes
/// it. The full digest of the install (`kjk fingerprint`) took 94.5 seconds
/// on its first run, for a check that takes about 0.1 seconds an area.
pub fn files_key(game: Game, index: &vfs::DirIndex) -> String {
    let mut hasher = blake3::Hasher::new();
    let add = |hasher: &mut blake3::Hasher, name: &str, path: &Path| {
        let (bytes, time) = std::fs::metadata(path).map_or((0, 0), |meta| {
            let time = (meta.modified().ok())
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |since| since.as_nanos());
            (meta.len(), time)
        });
        hasher.update(format!("{name} {bytes} {time}\n").as_bytes());
    };
    let group = |key: &str| {
        key.split_once('/')
            .map_or(key, |(first, _)| first)
            .to_string()
    };
    for (key, path) in index.iter() {
        if BAKE_GROUPS.contains(&group(key).as_str()) && vfs::fingerprint::covered(game, key) {
            add(&mut hasher, key, path);
        }
    }
    for item in index.workshop() {
        let label = item.label();
        hasher.update(format!("item {label}\n").as_bytes());
        for (key, path) in item.files.iter() {
            if BAKE_ITEM_GROUPS.contains(&group(key).as_str())
                && vfs::fingerprint::covered_in_item(key)
            {
                add(&mut hasher, &format!("{label}/{key}"), path);
            }
        }
    }
    hasher.finalize().to_hex().to_string()
}

/// The install of `game` that `discovery` finds, opened, or why it does
/// not open.
fn open_install(
    discovery: &vfs::Discovery,
    game: Game,
) -> Result<std::sync::Arc<crate::installs::Install>, Failure> {
    crate::installs::open(discovery, game).map_err(|why| {
        let kind = if discovery.get(game).is_some() {
            Kind::GameUnreadable
        } else {
            Kind::GameNotFound
        };
        Failure::new(kind, why)
    })
}

/// Compares `manifest` with the files of the player's install of its game,
/// as the game reads them. `store` keeps the answer, keyed by the names,
/// sizes and times of the files a bake reads (`files_key`) and the
/// manifest, so a second check of the same files is instant. `progress`
/// hears each area as it is compared.
pub fn check(
    store: Option<&Store>,
    manifest: &Manifest,
    progress: &mut dyn FnMut(Progress),
) -> Result<Report, Failure> {
    let game = manifest.game;
    let install = open_install(&crate::discover(), game)?;
    let files = files_key(game, &install.index);
    let key = store::Key::of(
        CHECKS,
        &[
            game.key().as_bytes(),
            files.as_bytes(),
            &bake_fingerprint::FORMAT.to_le_bytes(),
            manifest.exact_text().as_bytes(),
        ],
    );
    if let Some(bytes) = store.and_then(|store| store.get(CHECKS, &key))
        && let Ok(mut report) = serde_json::from_slice::<Report>(&bytes)
    {
        report.cached = true;
        return Ok(report);
    }
    let report = compare(&install, manifest, progress);
    if let Some(store) = store
        && let Ok(bytes) = serde_json::to_vec(&report)
    {
        let _ = store.put(CHECKS, &key, &bytes);
    }
    Ok(report)
}

/// An area as its module holds it in the player's files, with the module's
/// resolver, or why the module does not open or has no such area.
type Found<'a> = Result<
    (
        &'a vfs::resolver::Resolver,
        &'a asset_odyssey::module::AreaIr,
    ),
    String,
>;

/// `judge` on each area of `manifest` over `install`'s files, run on a few
/// threads, one module at a time on each. The answers are by area name.
/// `progress` hears each area as it is judged.
fn each_area<T: Send>(
    install: &crate::installs::Install,
    manifest: &Manifest,
    progress: &mut dyn FnMut(Progress),
    judge: &(dyn Fn(Found, &Area) -> T + Sync),
) -> BTreeMap<String, T> {
    // The areas by module, in the manifest's order.
    let mut modules: Vec<(&str, Vec<&Area>)> = Vec::new();
    for area in &manifest.areas {
        match modules
            .iter_mut()
            .find(|(module, _)| *module == area.module)
        {
            Some((_, areas)) => areas.push(area),
            None => modules.push((&area.module, vec![area])),
        }
    }
    let next = std::sync::atomic::AtomicUsize::new(0);
    let threads = std::thread::available_parallelism()
        .map_or(1, std::num::NonZeroUsize::get)
        .clamp(1, 8);
    let (send, receive) = std::sync::mpsc::channel::<(String, T)>();
    let mut answers: BTreeMap<String, T> = BTreeMap::new();
    std::thread::scope(|scope| {
        for _ in 0..threads {
            let send = send.clone();
            let (modules, next) = (&modules, &next);
            scope.spawn(move || {
                while let Some((module, areas)) =
                    modules.get(next.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
                {
                    let built = install.resolver(Some(module)).and_then(|resolver| {
                        asset_odyssey::module::ModuleIr::build(
                            module,
                            &crate::modules::ResolverSource(&resolver),
                        )
                        .map(|ir| (resolver, ir))
                        .map_err(|error| format!("{error:?}"))
                    });
                    for area in areas {
                        let found = match &built {
                            Err(why) => {
                                Err(format!("the game's module {module} does not open ({why})"))
                            }
                            Ok((resolver, ir)) => (ir.areas.iter())
                                .find(|found| found.resref.eq_ignore_ascii_case(&area.area))
                                .map(|found| (resolver, found))
                                .ok_or_else(|| {
                                    format!("the game's module {module} has no area {}", area.area)
                                }),
                        };
                        let _ = send.send((area.area.clone(), judge(found, area)));
                    }
                }
            });
        }
        drop(send);
        for (area, answer) in receive {
            answers.insert(area.clone(), answer);
            progress(Progress {
                done: answers.len(),
                total: manifest.areas.len(),
                area: &area,
            });
        }
    });
    answers
}

/// What the check finds for one area.
enum Outcome {
    Matched,
    PlaceablesDiffer,
    Partial(Partial),
    FallBack(FallBack),
}

/// The check of each area of `manifest` over `install`'s files.
fn compare(
    install: &crate::installs::Install,
    manifest: &Manifest,
    progress: &mut dyn FnMut(Progress),
) -> Report {
    let mut outcomes = each_area(install, manifest, progress, &|found, area| {
        outcome(found, area, manifest.format)
    });
    let mut report = Report {
        game: manifest.game.key().to_string(),
        needs: manifest.needs.clone(),
        areas: manifest.areas.len(),
        matched: Vec::new(),
        placeables_differ: Vec::new(),
        partial: Vec::new(),
        fall_back: Vec::new(),
        allowed: false,
        cached: false,
    };
    for area in &manifest.areas {
        let outcome = outcomes.remove(&area.area).unwrap_or_else(|| {
            falls_back(area, "the area was not compared".to_string(), Vec::new())
        });
        match outcome {
            Outcome::Matched => report.matched.push(area.area.clone()),
            Outcome::PlaceablesDiffer => report.placeables_differ.push(area.area.clone()),
            Outcome::Partial(partial) => report.partial.push(partial),
            Outcome::FallBack(fall) => report.fall_back.push(fall),
        }
    }
    report.allowed = allowed(report.showing());
    report
}

fn falls_back(area: &Area, why: String, mods: Vec<String>) -> Outcome {
    Outcome::FallBack(FallBack {
        area: area.area.clone(),
        why,
        mods,
    })
}

/// What the check finds for `area` of the manifest, of bake format
/// `format`, over the area as the player's files hold it. An area with
/// exact lines is judged lightmap by lightmap (`bake_fingerprint::fit`),
/// any other by its `rooms` and `placeables` digests
/// (`bake_fingerprint::verdict`). Where rooms differ, the outcome names the
/// mods that replace the area's room files (`bake_fingerprint::room_mods`).
fn outcome(found: Found, area: &Area, format: u32) -> Outcome {
    let (resolver, found) = match found {
        Ok(found) => found,
        Err(why) => return falls_back(area, why, Vec::new()),
    };
    let stamp = area.stamp(format);
    let mods = || {
        bake_fingerprint::room_mods(resolver, found)
            .into_iter()
            .collect()
    };
    let current = bake_fingerprint::current(resolver, found);
    match bake_fingerprint::fit(&stamp, &current) {
        Some(Err(why)) => falls_back(area, why, Vec::new()),
        Some(Ok(fit)) => fit_outcome(area, &fit, mods),
        None => match bake_fingerprint::verdict(&stamp, &current.print) {
            Verdict::Whole => Outcome::Matched,
            Verdict::LightmapsOnly => Outcome::PlaceablesDiffer,
            Verdict::Refused(why) => {
                let rooms_differ = stamp.format == current.print.format;
                falls_back(area, why, if rooms_differ { mods() } else { Vec::new() })
            }
        },
    }
}

/// What the check finds for `area` whose baked lightmaps and lit
/// placeables fit the player's files as `fit` says.
fn fit_outcome(area: &Area, fit: &Fit, mods: impl Fn() -> Vec<String>) -> Outcome {
    let total = fit.matching.len() + fit.differing.len();
    if !fit.baked() {
        let why =
            format!("none of the area's {total} baked lightmaps fit the rooms in the game's files");
        return falls_back(area, why, mods());
    }
    if fit.differing.is_empty() {
        return if fit.placeables_differ {
            Outcome::PlaceablesDiffer
        } else {
            Outcome::Matched
        };
    }
    Outcome::Partial(Partial {
        area: area.area.clone(),
        matching: fit.matching.len(),
        lightmaps: total,
        differ: (fit.differing.iter())
            .map(|(lightmap, rooms)| Differs {
                lightmap: lightmap.clone(),
                rooms: rooms.iter().cloned().collect(),
            })
            .collect(),
        placeables_differ: fit.placeables_differ,
        mods: mods(),
    })
}

/// A published download's second manifest, made by `restamp`.
pub struct Restamped {
    /// The manifest with the exact lines of each area that was stamped.
    pub manifest: Manifest,
    /// The areas left without exact lines, each with the reason.
    pub skipped: Vec<(String, String)>,
    /// The areas with lightmaps that no room mesh draws, such as a lightmap
    /// baked only for the grass under a walkmesh, each with those
    /// lightmaps.
    pub undrawn: Vec<(String, Vec<String>)>,
    /// Whether the files were read with their Steam Workshop items.
    pub workshop: bool,
}

/// What the zip of a download holds for one area: the lightmaps it has a
/// PNG of, without those its `stored.txt` names, and its fingerprint.
type Zipped = (BTreeSet<String>, Option<Fingerprint>);

/// What the zip of a download holds for each area (`Zipped`).
fn zipped_areas(zip: &Path) -> Result<BTreeMap<String, Zipped>, Failure> {
    let damaged = |error: &dyn std::fmt::Display| {
        Failure::new(Kind::ZipDamaged, format!("{}: {error}", zip.display()))
    };
    let file = std::fs::File::open(zip).map_err(|error| {
        Failure::new(Kind::ZipUnreadable, format!("{}: {error}", zip.display()))
    })?;
    let mut archive = ::zip::ZipArchive::new(file).map_err(|error| damaged(&error))?;
    let mut pngs: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut stored: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut prints: BTreeMap<String, Fingerprint> = BTreeMap::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| damaged(&error))?;
        let name = entry
            .name()
            .map_err(|error| damaged(&error))?
            .to_ascii_lowercase();
        let Some((area, file)) = name.split_once('/') else {
            continue;
        };
        let area = area.to_string();
        if let Some(stem) = file.strip_suffix(".png") {
            pngs.entry(area).or_default().insert(stem.to_string());
            continue;
        }
        let mut text = String::new();
        if file == crate::stored_fill::STORED_FILE || file == bake_fingerprint::FILE {
            std::io::Read::read_to_string(&mut entry, &mut text)
                .map_err(|error| damaged(&format!("{name}: {error}")))?;
        }
        if file == crate::stored_fill::STORED_FILE {
            let names = (text.lines().map(str::trim))
                .filter(|line| !line.is_empty() && !line.starts_with('#'))
                .map(str::to_ascii_lowercase);
            stored.entry(area).or_default().extend(names);
        } else if file == bake_fingerprint::FILE
            && let Some(print) = bake_fingerprint::parse(&text)
        {
            prints.insert(area, print);
        }
    }
    let areas = (pngs.keys().chain(prints.keys()))
        .cloned()
        .collect::<BTreeSet<String>>();
    Ok((areas.into_iter())
        .map(|area| {
            let stored = stored.remove(&area).unwrap_or(BTreeSet::new());
            let lightmaps = (pngs.remove(&area).unwrap_or(BTreeSet::new()).into_iter())
                .filter(|name| !stored.contains(name))
                .collect();
            let print = prints.remove(&area);
            (area, (lightmaps, print))
        })
        .collect())
}

/// The second manifest of the download that `manifest` and the zip at `zip`
/// make, stamped over this machine's files of its game. Each area's
/// lightmaps are the PNGs the zip holds for it. An area is stamped only when
/// this machine's files still give the `rooms` and `placeables` digests of
/// the area's fingerprint in the zip, which are the files it was baked
/// from. Any other area keeps only its old lines, and the game judges it by
/// the old rules. The files are read as a player's build reads them, with
/// the Steam Workshop items unless `KJK_WORKSHOP=off` is set
/// (`discover_as_player`). `progress` hears each area.
pub fn restamp(
    manifest: &Manifest,
    zip: &Path,
    progress: &mut dyn FnMut(Progress),
) -> Result<Restamped, Failure> {
    let (blake3, bytes) = digest_of(zip).map_err(|why| Failure::new(Kind::ZipUnreadable, why))?;
    if bytes != manifest.archive.bytes || blake3 != manifest.archive.blake3 {
        return Err(Failure::new(
            Kind::ZipMismatch,
            format!(
                "{} is not the zip the manifest names: it has {bytes} bytes and the BLAKE3 {blake3}",
                zip.display()
            ),
        ));
    }
    let zipped = zipped_areas(zip)?;
    // A build with the probes leaves the Steam Workshop items out by
    // default. The stamp must be made over the files a player's game reads,
    // so it reads them as a player's build does.
    let discovery = crate::discover_as_player();
    let workshop = (discovery.get(manifest.game)).is_some_and(|found| found.workshop);
    let install = open_install(&discovery, manifest.game)?;
    let mut stamped = each_area(&install, manifest, progress, &|found, area| {
        let (resolver, found) = found?;
        let (pngs, print) = zipped
            .get(&area.area)
            .ok_or_else(|| "the zip has no folder for it".to_string())?;
        let print = print
            .as_ref()
            .ok_or_else(|| format!("the zip has no {} for it", bake_fingerprint::FILE))?;
        let current = bake_fingerprint::current(resolver, found);
        let old = area.stamp(manifest.format);
        if !print.same_digests(&old) {
            return Err("the zip's fingerprint is not the one the manifest gives".to_string());
        }
        if !current.print.same_digests(&old) {
            return Err(
                "the files on this machine are not the ones the area was baked from".to_string(),
            );
        }
        Ok(bake_fingerprint::stamp(&current, pngs))
    });
    let mut restamped = Restamped {
        manifest: manifest.clone(),
        skipped: Vec::new(),
        undrawn: Vec::new(),
        workshop,
    };
    for area in &mut restamped.manifest.areas {
        match stamped.remove(&area.area) {
            Some(Ok((print, undrawn))) => {
                area.exact = print.exact;
                if !undrawn.is_empty() {
                    restamped.undrawn.push((area.area.clone(), undrawn));
                }
            }
            Some(Err(why)) => {
                area.exact = None;
                restamped.skipped.push((area.area.clone(), why));
            }
            None => {
                area.exact = None;
                restamped
                    .skipped
                    .push((area.area.clone(), "the area was not compared".to_string()));
            }
        }
    }
    Ok(restamped)
}

/// The folder of `game`'s installed download in `store`.
pub fn folder(store: &Store, game: Game) -> PathBuf {
    store.root().join(store::LIGHTING).join(game.key())
}

/// The BLAKE3 and the size of the file at `path`.
pub fn digest_of(path: &Path) -> Result<(String, u64), String> {
    let failed = |error: std::io::Error| format!("{}: {error}", path.display());
    let mut file = std::fs::File::open(path).map_err(failed)?;
    let mut hasher = blake3::Hasher::new();
    let bytes = std::io::copy(&mut file, &mut hasher).map_err(failed)?;
    Ok((hasher.finalize().to_hex().to_string(), bytes))
}

/// Whether `name` may name an area's folder or a file in it: letters,
/// digits, `_`, `-` and `.`, and not only dots.
fn plain_name(name: &str) -> bool {
    !name.is_empty()
        && !name.bytes().all(|byte| byte == b'.')
        && (name.bytes()).all(|byte| byte.is_ascii_alphanumeric() || b"_-.".contains(&byte))
}

/// Installs the download in the zip at `zip`, which `manifest` describes,
/// into `store`, in place of an earlier one of its game. The zip's size and
/// BLAKE3 must be the manifest's. It is unpacked into `<game>.partial`
/// beside the installed folder, each area's fingerprint is compared with
/// the manifest's, and the folder is then renamed into place. `progress`
/// hears each area folder as it is unpacked. This does not check the
/// player's files (`check`). The result is the installed folder.
pub fn install(
    store: &Store,
    manifest: &Manifest,
    zip: &Path,
    progress: &mut dyn FnMut(Progress),
) -> Result<PathBuf, Failure> {
    let (blake3, bytes) = digest_of(zip).map_err(|why| Failure::new(Kind::ZipUnreadable, why))?;
    if bytes != manifest.archive.bytes || blake3 != manifest.archive.blake3 {
        return Err(Failure::new(
            Kind::ZipMismatch,
            format!(
                "{} is not the download the manifest names: it has {bytes} bytes, and the manifest gives {} bytes and another BLAKE3. The download may be incomplete.",
                zip.display(),
                manifest.archive.bytes
            ),
        ));
    }
    let target = folder(store, manifest.game);
    let partial = target.with_extension("partial");
    let old = target.with_extension("old");
    if partial.exists() {
        std::fs::remove_dir_all(&partial).map_err(|error| Failure::write(&partial, &error))?;
    }
    std::fs::create_dir_all(&partial).map_err(|error| Failure::write(&partial, &error))?;
    let unpacked = unpack(manifest, zip, &partial, progress);
    let checked = unpacked.and_then(|()| check_unpacked(manifest, &partial));
    if let Err(why) = checked {
        let _ = std::fs::remove_dir_all(&partial);
        return Err(why);
    }
    write_manifests(manifest, &partial)?;
    // The older install goes aside, the new one into its place, and the
    // older one is removed. A rename that fails puts the older one back.
    if old.exists() {
        std::fs::remove_dir_all(&old).map_err(|error| Failure::write(&old, &error))?;
    }
    if target.exists() {
        std::fs::rename(&target, &old).map_err(|error| Failure::write(&target, &error))?;
    }
    if let Err(error) = std::fs::rename(&partial, &target) {
        if old.exists() {
            let _ = std::fs::rename(&old, &target);
        }
        return Err(Failure::write(&target, &error));
    }
    if old.exists() {
        let _ = std::fs::remove_dir_all(&old);
    }
    Ok(target)
}

/// Unpacks the zip at `zip` into `into`. Each entry must be a file the game
/// loads (`shipped`) in the folder of one of `manifest`'s areas.
fn unpack(
    manifest: &Manifest,
    zip: &Path,
    into: &Path,
    progress: &mut dyn FnMut(Progress),
) -> Result<(), Failure> {
    let damaged = |error: &dyn std::fmt::Display| {
        Failure::new(Kind::ZipDamaged, format!("{}: {error}", zip.display()))
    };
    let file = std::fs::File::open(zip).map_err(|error| {
        Failure::new(Kind::ZipUnreadable, format!("{}: {error}", zip.display()))
    })?;
    let mut archive = ::zip::ZipArchive::new(file).map_err(|error| damaged(&error))?;
    let areas: std::collections::BTreeSet<&str> = (manifest.areas.iter())
        .map(|area| area.area.as_str())
        .collect();
    let mut seen = std::collections::BTreeSet::new();
    let mut bytes = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| damaged(&error))?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().map_err(|error| damaged(&error))?.to_string();
        let parts: Vec<&str> = name.split('/').collect();
        let [area, file] = parts.as_slice() else {
            return Err(damaged(&format!(
                "{name} is not a file in an area's folder"
            )));
        };
        if !plain_name(area) || !plain_name(file) || !areas.contains(area) || !shipped(file) {
            return Err(damaged(&format!(
                "{name} is not a file of an area that the manifest lists"
            )));
        }
        let folder = into.join(area);
        if seen.insert((*area).to_string()) {
            std::fs::create_dir_all(&folder).map_err(|error| Failure::write(&folder, &error))?;
            progress(Progress {
                done: seen.len(),
                total: areas.len(),
                area,
            });
        }
        // The entry is read whole first, so that a zip that does not read
        // is told apart from a disk that does not take the file.
        bytes.clear();
        std::io::Read::read_to_end(&mut entry, &mut bytes)
            .map_err(|error| damaged(&format!("{name}: {error}")))?;
        let path = folder.join(file);
        std::fs::write(&path, &bytes).map_err(|error| Failure::write(&path, &error))?;
    }
    Ok(())
}

/// Writes into the download's folder `folder` the manifest in the form
/// kjk v0.21.0 reads (`MANIFEST`), and for a manifest with exact lines the
/// second manifest (`MANIFEST_2`) and each area's fingerprint with its
/// exact lines. The `rooms` and `placeables` lines of each fingerprint stay
/// as they were.
fn write_manifests(manifest: &Manifest, folder: &Path) -> Result<(), Failure> {
    let written = |path: PathBuf, text: String| {
        std::fs::write(&path, text).map_err(|error| Failure::write(&path, &error))
    };
    written(folder.join(MANIFEST), manifest.text())?;
    if !manifest.is_exact() {
        return Ok(());
    }
    for area in &manifest.areas {
        let set = folder.join(&area.area);
        bake_fingerprint::write(&set, &area.stamp(manifest.format))
            .map_err(|why| Failure::new(Kind::Other, why))?;
    }
    written(folder.join(MANIFEST_2), manifest.exact_text())
}

/// Adds the exact lines of the second manifest `manifest` to the download
/// of its game installed in `store`, without its zip. The installed
/// download must be of the zip `manifest` names, by its BLAKE3 and size,
/// and must hold every area folder of `manifest` with the fingerprint the
/// manifest gives. The installed `MANIFEST` stays as it is, so kjk v0.21.0
/// reads the download as before. The result is the installed folder.
pub fn refresh(store: &Store, manifest: &Manifest) -> Result<PathBuf, Failure> {
    let target = folder(store, manifest.game);
    let not_installed = |why: String| Failure::new(Kind::NotInstalled, why);
    let installed = Manifest::read(&target.join(MANIFEST)).map_err(|why| {
        not_installed(format!(
            "No lighting download for {} is installed that reads: {why}",
            game_name(manifest.game)
        ))
    })?;
    if installed.archive.blake3 != manifest.archive.blake3
        || installed.archive.bytes != manifest.archive.bytes
    {
        return Err(not_installed(format!(
            "The lighting download installed for {} is {}, not {}. Install the new one in full.",
            game_name(manifest.game),
            installed.archive.file,
            manifest.archive.file
        )));
    }
    check_unpacked(manifest, &target).map_err(|failure| not_installed(failure.why))?;
    write_manifests(manifest, &target)?;
    Ok(target)
}

/// Whether every area of `manifest` was unpacked into `folder` with the
/// manifest's fingerprint. Only the `rooms` and `placeables` digests are
/// compared, since a zip's fingerprints may lack the exact lines.
fn check_unpacked(manifest: &Manifest, folder: &Path) -> Result<(), Failure> {
    for area in &manifest.areas {
        match bake_fingerprint::read(&folder.join(&area.area)) {
            Some(Ok(print)) if print.same_digests(&area.stamp(manifest.format)) => {}
            _ => {
                return Err(Failure::new(
                    Kind::ZipDamaged,
                    format!(
                        "the download's folder of area {} does not hold the fingerprint its manifest gives",
                        area.area
                    ),
                ));
            }
        }
    }
    Ok(())
}

/// One game's installed download.
#[derive(Debug, Clone)]
pub struct Installed {
    pub game: Game,
    pub folder: PathBuf,
    /// Its manifest, or why it does not read.
    pub manifest: Result<Manifest, String>,
}

/// What is installed for each KOTOR game, K1 first: `None` for a game with
/// no download installed.
pub fn status(store: &Store) -> Vec<(Game, Option<Installed>)> {
    [Game::Kotor1, Game::Kotor2]
        .into_iter()
        .map(|game| {
            let folder = folder(store, game);
            let installed = folder.is_dir().then(|| Installed {
                game,
                manifest: Manifest::read(&folder.join(MANIFEST)),
                folder,
            });
            (game, installed)
        })
        .collect()
}

/// Removes `game`'s installed download from `store`, with what an install
/// left half done. Whether there was one.
pub fn remove(store: &Store, game: Game) -> Result<bool, Failure> {
    let target = folder(store, game);
    let mut removed = false;
    for path in [
        target.with_extension("partial"),
        target.with_extension("old"),
        target.clone(),
    ] {
        if path.exists() {
            std::fs::remove_dir_all(&path).map_err(|error| Failure::write(&path, &error))?;
            removed |= path == target;
        }
    }
    Ok(removed)
}

/// The installed area folders in `store`, K1's first, each game's by name.
/// Only a game whose manifest reads counts, and only a folder that holds a
/// fingerprint.
pub fn folders_in(store: &Store) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for (_, installed) in status(store) {
        let Some(installed) = installed.filter(|installed| installed.manifest.is_ok()) else {
            continue;
        };
        let Ok(entries) = std::fs::read_dir(&installed.folder) else {
            continue;
        };
        let mut folders: Vec<PathBuf> = (entries.flatten())
            .map(|entry| entry.path())
            .filter(|path| path.join(bake_fingerprint::FILE).is_file())
            .collect();
        folders.sort();
        found.extend(folders);
    }
    found
}

/// The games with a download installed in the machine's store, looked for
/// once a run.
fn installed_games() -> &'static [Game] {
    static GAMES: OnceLock<Vec<Game>> = OnceLock::new();
    GAMES.get_or_init(|| {
        let Some(store) = Store::open() else {
            return Vec::new();
        };
        (status(&store).into_iter())
            .filter(|(_, installed)| installed.as_ref().is_some_and(|i| i.manifest.is_ok()))
            .map(|(game, _)| game)
            .collect()
    })
}

/// The installed area folders the game loads (`viewer::baked_folders`).
pub fn folders() -> Vec<PathBuf> {
    if installed_games().is_empty() {
        return Vec::new();
    }
    Store::open().map_or_else(Vec::new, |store| folders_in(&store))
}

/// Whether the player chose the baked lightmaps (the options' Lighting at
/// Enhanced Lightmaps) and a download is installed, as this run started. A
/// change in the options takes effect the next time kjk starts, in both
/// directions (`settings::Settings::lighting`, which asks this first as
/// the settings load).
pub fn chosen() -> bool {
    static CHOSEN: OnceLock<bool> = OnceLock::new();
    *CHOSEN.get_or_init(|| {
        let stored = crate::settings::stored();
        stored.get("display.lighting").map(String::as_str) == Some(SETTING_VALUE)
            && !installed_games().is_empty()
    })
}

/// The game whose area loaded last (`note_played`).
static PLAYED: Mutex<Option<Game>> = Mutex::new(None);

/// Notes that an area of `game` loaded, for `offered`.
pub fn note_played(game: Game) {
    *PLAYED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(game);
}

/// Whether the options offer Enhanced Lightmaps: a download is installed
/// for the game being played, or before any area has loaded, for either
/// game.
pub fn offered() -> bool {
    let games = installed_games();
    match *PLAYED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
    {
        Some(game) => games.contains(&game),
        None => !games.is_empty(),
    }
}

#[cfg(feature = "probes")]
pub mod probes;
