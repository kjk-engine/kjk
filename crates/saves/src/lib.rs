//! Saved games on disk, in our own format (`docs/UI.md`):
//! a folder for each in the saves' folder, holding the world as `sim::Save`
//! has it (`save.kjkv`) and what the screens show of it (`meta.json`): its
//! name, the module and the area's name, when it was saved, its seats, and
//! the character made in character creation (whose name and portrait only
//! the view keeps). And when the game may not be saved, as KOTOR refuses.
//!
//! A folder's name is its id: `quick` for the quick save, `0001`, `0002`,
//! … for the rest, `r0001`, `r0002`, … for the copies of a host's
//! saves this machine received (`copies`, I30 P17), and `k1-01`, `tsl-01`,
//! … for the playtests' checkpoints a chain writes as saves (`checkpoint_id`,
//! `kjk chain`, docs/PLAYTESTS.md). Nothing else is read or
//! written, so an id from a script never leaves the folder. No Bevy: the
//! approved tests use it as it is.
//!
//! A save is written whole into `.<id>.new` and renamed into its place; a
//! save it replaces waits as `.<id>.old` until then, and is removed after.
//! Neither name is an id, so neither is listed; a save left aside by a
//! program that stopped between the two renames is put back when its id is
//! free (`restore`, at every listing and write).

pub mod copies;
pub mod safety;

pub use safety::{AUTOS, Formats, backup_on_new_version, write_auto};

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ui::intent::CharacterDesc;

/// The quick save's id.
pub const QUICK: &str = "quick";

const WORLD: &str = "save.kjkv";
const META: &str = "meta.json";
/// The ends of a save's folder being written (`.0001.new`) and of the one
/// it replaces, set aside meanwhile (`.0001.old`).
const FRESH: &str = ".new";
const ASIDE: &str = ".old";

/// What a save is, beside its world (`meta.json`, its fields by name).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Meta {
    pub name: String,
    pub module: String,
    /// The area's name, as the game calls it.
    pub area: String,
    /// When it was saved (Unix seconds).
    pub saved: u64,
    /// The seats the game held when saved: shown, not kept to (a hosted
    /// save holds the seats its host asks for, I31).
    pub seats: u8,
    #[serde(default)]
    pub character: Option<CharacterDesc>,
    /// The world's hash (blake3, hex), checked before it loads; a save
    /// from before it is trusted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blake3: Option<String>,
    /// The commit and release that wrote it, the game it is of, and its
    /// formats: what a refusal names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formats: Option<Formats>,
}

/// `meta.json`: the meta, and where it came from if it is a copy
/// (`copies`; null, or missing in an older save, for one made here).
#[derive(Serialize, Deserialize)]
struct MetaFile {
    #[serde(flatten)]
    meta: Meta,
    #[serde(default)]
    received: Option<copies::Received>,
}

/// The ids of copies received start with it (`r0001`).
pub const RECEIVED: &str = "r";

/// `quick`, `auto` (the autosaves' newest; `auto-2`, `auto-3` are as a
/// checkpoint's), a number, `r` and a number (a copy received), or a game's
/// key and a number (a playtest's checkpoint, `k1-03`): an id that names a
/// folder in the saves' and nothing else.
pub fn valid_id(id: &str) -> bool {
    let number = |digits: &str| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit());
    let checkpoint = id.split_once('-').is_some_and(|(game, digits)| {
        !game.is_empty()
            && game
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            && number(digits)
    });
    id == QUICK
        || id == AUTOS[0]
        || number(id)
        || id.strip_prefix(RECEIVED).is_some_and(number)
        || checkpoint
}

/// The id the `number`th (from 1) of `game`'s playtest checkpoints is
/// saved under (`k1-03`).
pub fn checkpoint_id(game: &str, number: usize) -> String {
    format!("{}-{number:02}", game.to_ascii_lowercase())
}

fn checked(root: &Path, id: &str) -> Result<PathBuf, String> {
    if valid_id(id) {
        Ok(root.join(id))
    } else {
        Err(format!("{id:?} is not a save"))
    }
}

/// What the saves' folder holds.
#[derive(Debug, Default)]
pub struct Listing {
    /// Every save whose meta reads, the latest first.
    pub saves: Vec<(String, Meta)>,
    /// Those whose meta does not, and why.
    pub unread: Vec<String>,
    /// Those that are copies of another's game (I30 P17), by id.
    pub received: BTreeMap<String, copies::Received>,
    /// Those of `saves` whose world is not what their meta's hash says:
    /// shown, not loaded.
    pub damaged: Vec<String>,
}

/// What `root` holds (nothing, when it is not there yet), a save left
/// aside put back first.
pub fn list(root: &Path) -> Listing {
    restore(root);
    let mut listing = Listing::default();
    let Ok(entries) = std::fs::read_dir(root) else {
        return listing;
    };
    for entry in entries.flatten() {
        let Some(id) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if !valid_id(&id) {
            continue;
        }
        match read_meta_received(&entry.path()) {
            Ok((meta, received)) => {
                if let Some(received) = received {
                    listing.received.insert(id.clone(), received);
                }
                if damaged(&entry.path(), &meta) {
                    listing.damaged.push(id.clone());
                }
                listing.saves.push((id, meta));
            }
            Err(why) => listing.unread.push(format!("save {id}: {why}")),
        }
    }
    listing
        .saves
        .sort_by(|a, b| b.1.saved.cmp(&a.1.saved).then_with(|| b.0.cmp(&a.0)));
    listing
}

/// The save `id`: its world and its meta. A damaged one is refused; one
/// this build does not read is refused naming the build that made it.
pub fn read(root: &Path, id: &str) -> Result<(sim::Save, Meta), String> {
    let folder = checked(root, id)?;
    let meta = read_meta(&folder)?;
    let bytes = whole(&folder, &meta).map_err(|error| format!("save {id}: {error}"))?;
    let save = sim::Save::decode(&bytes).map_err(|error| {
        let made = match (&meta.version, &meta.build) {
            (Some(version), Some(build)) => {
                format!("; it was made with {version} ({})", &build[..8.min(build.len())])
            }
            _ => String::new(),
        };
        format!(
            "save {id}: {error}{made}. Copies from before each release are in {}; an autosave, or the release that made it, may load it",
            safety::BACKUPS
        )
    })?;
    Ok((save, meta))
}

/// What a listing found of a save's world, kept while its file and hash
/// stay as they were: a listing hashes each world once, not on every
/// screen that lists them.
type Verdicts = BTreeMap<(PathBuf, u64, std::time::SystemTime, Option<String>), bool>;
static VERDICTS: std::sync::Mutex<Verdicts> = std::sync::Mutex::new(BTreeMap::new());

/// Whether the world in `folder` is not what `meta`'s hash says.
fn damaged(folder: &Path, meta: &Meta) -> bool {
    let file = std::fs::metadata(folder.join(WORLD));
    let Some(key) = file.as_ref().ok().and_then(|file| {
        Some((
            folder.to_path_buf(),
            file.len(),
            file.modified().ok()?,
            meta.blake3.clone(),
        ))
    }) else {
        return whole(folder, meta).is_err();
    };
    let known = VERDICTS
        .lock()
        .ok()
        .and_then(|verdicts| verdicts.get(&key).copied());
    known.unwrap_or_else(|| {
        let verdict = whole(folder, meta).is_err();
        if let Ok(mut verdicts) = VERDICTS.lock() {
            verdicts.insert(key, verdict);
        }
        verdict
    })
}

/// How many worlds were hashed (`whole`), for the probes.
#[cfg(feature = "probes")]
pub(crate) static HASHED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The world in `folder`, refused as damaged when it is not what `meta`'s
/// hash says.
fn whole(folder: &Path, meta: &Meta) -> Result<Vec<u8>, String> {
    let bytes = std::fs::read(folder.join(WORLD)).map_err(|error| error.to_string())?;
    #[cfg(feature = "probes")]
    HASHED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    match &meta.blake3 {
        Some(hash) if *hash != blake3::hash(&bytes).to_hex().as_str() => {
            Err("the save is damaged: its world is not the one saved".into())
        }
        _ => Ok(bytes),
    }
}

/// `save` written as `slot` (a new number without one), with `meta`; its
/// id.
pub fn write(
    root: &Path,
    slot: Option<&str>,
    save: &sim::Save,
    meta: &Meta,
) -> Result<String, String> {
    write_encoded(root, slot, &save.encode(), meta)
}

/// `world` (a save as `sim::Save::encode` makes it) written as `slot` (a
/// new number without one), with `meta`; its id.
pub fn write_encoded(
    root: &Path,
    slot: Option<&str>,
    world: &[u8],
    meta: &Meta,
) -> Result<String, String> {
    let id = match slot {
        Some(id) => id.to_string(),
        None => next_id(root, "")?,
    };
    write_files(root, &id, world, meta, None)?;
    Ok(id)
}

/// `root`'s folder for `id` ending in `end` (`FRESH`, `ASIDE`).
fn beside(root: &Path, id: &str, end: &str) -> PathBuf {
    root.join(format!(".{id}{end}"))
}

fn failed(path: &Path) -> impl Fn(std::io::Error) -> String + '_ {
    move |error| format!("{}: {error}", path.display())
}

/// `world` (a save's bytes) and `meta` written as `id`, with where it came
/// from when it is a copy: whole into a folder of its own, then renamed
/// into its place (the save there before set aside meanwhile, then
/// removed).
fn write_files(
    root: &Path,
    id: &str,
    world: &[u8],
    meta: &Meta,
    received: Option<&copies::Received>,
) -> Result<(), String> {
    let folder = checked(root, id)?;
    restore(root);
    let (fresh, aside) = (beside(root, id, FRESH), beside(root, id, ASIDE));
    // Left by a write that stopped (an aside, once its save is in place).
    let remove = |path: &Path| std::fs::remove_dir_all(path).map_err(failed(path));
    if fresh.exists() {
        remove(&fresh)?;
    }
    if aside.exists() && folder.exists() {
        remove(&aside)?;
    }
    std::fs::create_dir_all(&fresh).map_err(failed(&fresh))?;
    let meta = Meta {
        blake3: Some(blake3::hash(world).to_hex().to_string()),
        ..meta.clone()
    };
    let text = serde_json::to_vec_pretty(&meta_file(&meta, received)).expect("a meta serializes");
    // On the disk before the renames: a power cut leaves the old save or
    // the new one whole.
    for (name, bytes) in [(WORLD, world), (META, text.as_slice())] {
        let path = fresh.join(name);
        // The handle that wrote it: Windows refuses to flush a read-only one.
        let written = std::fs::File::create(&path).and_then(|mut file| {
            std::io::Write::write_all(&mut file, bytes)?;
            file.sync_all()
        });
        if let Err(error) = written {
            // A full disk: nothing half-written is left.
            let _ = remove(&fresh);
            return Err(failed(&path)(error));
        }
    }
    let replacing = folder.exists();
    if replacing {
        std::fs::rename(&folder, &aside).map_err(failed(&folder))?;
    }
    if let Err(error) = std::fs::rename(&fresh, &folder) {
        if replacing {
            let _ = std::fs::rename(&aside, &folder);
        }
        return Err(failed(&folder)(error));
    }
    // Saved: an aside not removed now is at the next write of `id`.
    if replacing {
        let _ = remove(&aside);
    }
    Ok(())
}

/// Each save set aside (`.0001.old`) whose id is free put back: the
/// program stopped before the save replacing it was in place.
fn restore(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(id) = name
            .to_str()
            .and_then(|name| name.strip_prefix('.')?.strip_suffix(ASIDE))
        else {
            continue;
        };
        let place = root.join(id);
        if valid_id(id) && !place.exists() {
            let _ = std::fs::rename(entry.path(), place);
        }
    }
}

/// The save `id` gone.
pub fn delete(root: &Path, id: &str) -> Result<(), String> {
    let folder = checked(root, id)?;
    std::fs::remove_dir_all(&folder).map_err(|error| format!("{}: {error}", folder.display()))
}

/// `prefix` and the number after the highest of the saves named so
/// (`""`: this machine's own, `RECEIVED`: the copies); refused when no
/// number is left after it.
fn next_id(root: &Path, prefix: &str) -> Result<String, String> {
    restore(root);
    let highest = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let digits = name.to_str()?.strip_prefix(prefix)?;
            digits
                .bytes()
                .all(|byte| byte.is_ascii_digit())
                .then(|| digits.parse::<u32>().ok())?
        })
        .max()
        .unwrap_or(0);
    let next = highest
        .checked_add(1)
        .ok_or_else(|| format!("no save number is left after {prefix}{highest}"))?;
    Ok(format!("{prefix}{next:04}"))
}

/// Now, in Unix seconds.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// Days since 1970-01-01 to (year, month, day), proleptic Gregorian.
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// `meta.json` for `meta`, made here or `received`.
fn meta_file(meta: &Meta, received: Option<&copies::Received>) -> MetaFile {
    MetaFile {
        meta: meta.clone(),
        received: received.cloned(),
    }
}

pub(crate) fn read_meta(folder: &Path) -> Result<Meta, String> {
    read_meta_received(folder).map(|(meta, _)| meta)
}

fn read_meta_received(folder: &Path) -> Result<(Meta, Option<copies::Received>), String> {
    let text = std::fs::read_to_string(folder.join(META)).map_err(|error| error.to_string())?;
    parse_meta(&text)
}

/// A meta's text: the meta, and where it came from if it is a copy.
fn parse_meta(text: &str) -> Result<(Meta, Option<copies::Received>), String> {
    let file: MetaFile = serde_json::from_str(text).map_err(|error| format!("{META}: {error}"))?;
    Ok((file.meta, file.received))
}

/// Why the game cannot be saved now, as KOTOR refuses (its rules beside
/// these, a module loading and a movie, are the host's to know): `seat`'s
/// body is down, a conversation is under way, the party is in combat
/// (`Snapshot::in_combat`, the one test), a break holds, a ready check
/// waits at an exit (`paused_by`, `ready`: the party's calls) or a minigame
/// is in play (a swoop race, the turret, a pazaak match: KOTOR saves none
/// [verify in the game]). Another player's conversation or match is told
/// in its body's name, its entity's in `level` (R11: "Carth is in a
/// conversation."); `seat`'s own, a party conversation, the host's, two
/// others', or a body with no name, in KOTOR's words.
pub fn refusal(snapshot: &sim::Snapshot, seat: sim::Seat, level: &sim::Level) -> Option<String> {
    use sim::party::{IN_CONVERSATION, PLAYING_PAZAAK};
    let me = snapshot.body(seat);
    let own = me.map(|me| me.id.0);
    let down = me
        .and_then(|me| me.fighter.as_ref())
        .is_some_and(|fighter| !fighter.alive());
    // `body` is `doing` something: named, or `seat`'s own or nameless in
    // KOTOR's `words`.
    let busy = |body: u32, doing: &str, words: &str| {
        let objects = sim::Instances::in_snapshot(level, snapshot);
        let name = snapshot
            .get(sim::SimId(body))
            .filter(|_| Some(body) != own)
            .and_then(|entity| objects.of_entity(entity))
            .map(|facts| facts.name.as_str())
            .filter(|name| !name.is_empty());
        name.map_or_else(|| words.to_string(), |name| format!("{name} is {doing}."))
    };
    // A conversation under way, named for the first guest's body standing in
    // one, in seat order; in KOTOR's words when `seat`'s body stands in one,
    // a party one holds every seat (`talk_for`), only the host's character
    // talks, or only others do (two others talking, a guest's body none).
    let talking = (!snapshot.talk.conversations.is_empty()).then(|| {
        let mine = snapshot.talk_for(seat).is_some();
        let guest = (1..snapshot.seats())
            .map(sim::Seat)
            .find(|guest| snapshot.stands_in(*guest).is_some());
        guest
            .filter(|_| !mine)
            .and_then(|guest| snapshot.body(guest))
            .map(|body| body.id.0)
    });
    if down {
        Some("You cannot save while you are down.".into())
    } else if let Some(theirs) = talking {
        let words = "You cannot save during a conversation.";
        Some(theirs.map_or_else(|| words.into(), |who| busy(who, IN_CONVERSATION, words)))
    } else if snapshot.in_combat {
        Some("You cannot save during combat.".into())
    } else if snapshot.paused_by.is_some() {
        Some("You cannot save during a break.".into())
    } else if snapshot.ready.is_some() {
        Some("You cannot save while the party waits at an exit.".into())
    } else if snapshot.minigame.is_some() {
        Some(MINIGAME.into())
    } else if let Some(table) = snapshot.pazaak.first() {
        // The match `seat` plays, else another player's.
        let player = snapshot.pazaak_of(seat).unwrap_or(table).player;
        Some(busy(player, PLAYING_PAZAAK, MINIGAME))
    } else {
        None
    }
}

/// KOTOR's refusal during a minigame.
const MINIGAME: &str = "You cannot save during a minigame.";

#[cfg(feature = "probes")]
mod probes;
#[cfg(feature = "probes")]
pub use probes::refusal_probes;
#[cfg(feature = "probes")]
pub use safety::probes as safety_probes;
