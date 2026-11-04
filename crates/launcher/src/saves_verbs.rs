//! `kjk saves capture|check`: the corpus of real saves each release is
//! checked against before its tag (`saves::corpus`, docs/RELEASE.md).
//! `capture` puts a release's saves into the corpus; `check` loads every
//! one on this build the way a player's load does, and prints what each
//! came to.

use std::collections::BTreeMap;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use saves::corpus::{self, Breaking, Row, Slot, Verdict};
use saves::story::PLACE_SLACK_M;
use saves::{Formats, Meta, Story};
use sim::determinism::{Run, pilot};
use sim::{Level, Save, StepReason, TickRate};

use crate::levels::Installers;
use kjk_game::lifecycle::ItemSearch;

/// The ticks each save is stepped, headless.
const TICKS: u64 = 300;
/// The pilot's seed for them.
const PILOT_SEED: u64 = 1;
/// The versions from which older saves are refused (`bumps/README.md`).
const BREAKING: &str = "bumps/BREAKING.md";

/// The chain checkpoints a release's corpus takes when `capture` is given
/// none, by the name they go under: the Endar Spire's route ends in
/// `taris_start`, then Taris's Undercity, mid-Korriban (the Mandalorian's
/// cache), and TSL's Peragus fuel depot (`data/routes/<game>/`).
fn defaults(game: &str) -> &'static [(&'static str, &'static str)] {
    match game {
        "k1" => &[
            ("endar-spire", "taris_start"),
            ("taris-undercity", "undercity"),
            ("korriban-cache", "kor_cache"),
        ],
        "tsl" => &[("peragus-depot", "peragus_depot")],
        _ => &[],
    }
}

/// `kjk saves capture`: `from` (or the default checkpoints of `game`) as
/// slots of the corpus's `tag`.
pub(crate) fn capture(
    discovery: &vfs::Discovery,
    tag: &str,
    game: &str,
    from: &[PathBuf],
) -> ExitCode {
    match capture_slots(discovery, tag, game, from) {
        Ok(lines) => {
            lines.iter().for_each(|line| outln!("{line}"));
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("saves capture: {error}");
            ExitCode::FAILURE
        }
    }
}

fn capture_slots(
    discovery: &vfs::Discovery,
    tag: &str,
    game: &str,
    from: &[PathBuf],
) -> Result<Vec<String>, String> {
    if tag.is_empty() || tag.starts_with('.') || tag.contains(['/', '\\']) {
        return Err(format!("{tag:?}: a release's tag (v0.5.0, main-646e008)"));
    }
    let sources: Vec<(String, PathBuf)> = if from.is_empty() {
        let folder = crate::runtime::artifacts().join("checkpoints").join(game);
        let found = defaults(game);
        if found.is_empty() {
            return Err(format!("{game}: k1 or tsl"));
        }
        found
            .iter()
            .map(|(name, checkpoint)| {
                (name.to_string(), folder.join(format!("{checkpoint}.kjkv")))
            })
            .collect()
    } else {
        from.iter()
            .map(|path| Ok((slot_name(path, tag)?, path.clone())))
            .collect::<Result<_, String>>()?
    };
    let folder = crate::runtime::artifacts().join(corpus::FOLDER).join(tag);
    let mut installers = Installers::default();
    let mut lines = Vec::new();
    for (name, path) in sources {
        let failed = |error: String| format!("{}: {error}", path.display());
        let slot = read_source(&path).map_err(failed)?;
        let (slot, how) = completed(slot, &mut installers, discovery, &name, tag).map_err(failed)?;
        slot.write(&folder, &name)?;
        lines.push(format!(
            "{} → {} ({how})",
            path.display(),
            folder.join(&name).display()
        ));
    }
    Ok(lines)
}

/// The name a source goes under: its file's or folder's, a zip's without
/// the `kjk-` and `<tag>-` a shared one carries.
fn slot_name(path: &Path, tag: &str) -> Result<String, String> {
    let stem = path
        .file_stem()
        .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
    let stem = stem.strip_prefix("kjk-").unwrap_or(&stem);
    let name = stem.strip_prefix(&format!("{tag}-")).unwrap_or(stem);
    if corpus::valid_name(name) {
        Ok(name.to_string())
    } else {
        Err(format!(
            "{}: no name for a slot comes of it (a file or folder with a name, not `.` or `..`)",
            path.display()
        ))
    }
}

/// The slot a source is: a slot folder, a zip of one, or a bare `.kjkv`.
fn read_source(path: &Path) -> Result<Slot, String> {
    if path.is_dir() {
        return Slot::open(path);
    }
    if path.extension().is_some_and(|extension| extension == "zip") {
        return read_zip(path);
    }
    let world = std::fs::read(path).map_err(|error| error.to_string())?;
    Ok(Slot {
        world,
        meta: None,
        story: None,
    })
}

/// The slot in `zip`, whatever its folder inside is called.
fn read_zip(zip: &Path) -> Result<Slot, String> {
    let file = std::fs::File::open(zip).map_err(|error| error.to_string())?;
    let mut archive = ::zip::ZipArchive::new(file).map_err(|error| error.to_string())?;
    let mut files = BTreeMap::new();
    for at in 0..archive.len() {
        let mut entry = archive.by_index(at).map_err(|error| error.to_string())?;
        let name = entry.name().map_err(|error| error.to_string())?.into_owned();
        let Some(base) = name.rsplit('/').next().filter(|base| !base.is_empty()) else {
            continue;
        };
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        files.insert(base.to_string(), bytes);
    }
    Slot::from_files(files)
}

/// `slot` with the meta and story a save of the corpus has: its own meta
/// kept, one made from `tag` where it has none; the story made from the
/// save (`Story::of`) where it has none. And what was added, in words.
fn completed(
    mut slot: Slot,
    installers: &mut Installers,
    discovery: &vfs::Discovery,
    name: &str,
    tag: &str,
) -> Result<(Slot, String), String> {
    let (save, found) = match Save::decode_versioned(&slot.world) {
        Ok(decoded) => decoded,
        // Older than this build reads: kept as it is, for the gate to
        // expect it to be refused.
        Err(error) => return Ok((slot, format!("not read by this build, kept as is: {error}"))),
    };
    let mut added = Vec::new();
    if slot.meta.is_none() {
        let (version, build) = release_of(tag);
        slot.meta = Some(Meta {
            name: format!("{tag} {name}"),
            module: save.snapshot.place.as_ref().map_or_else(
                || save.level.name.clone(),
                |place| place.module.clone(),
            ),
            area: name.to_string(),
            saved: saves::now(),
            seats: save.snapshot.seats(),
            build,
            version,
            game: save.snapshot.place.as_ref().map(|place| place.game.clone()),
            formats: Some(Formats {
                save: found.save,
                snapshot: found.snapshot,
                story: saves::STORY_SCHEMA,
            }),
            ..Meta::default()
        });
        added.push("meta made");
    }
    if slot.story.is_none() {
        let level = level_of(installers, discovery, &save)?;
        slot.story = Some(Story::of(&as_loaded(&save, &level), &level));
        added.push("story made");
    }
    let how = if added.is_empty() {
        "kept as it is".to_string()
    } else {
        added.join(", ")
    };
    Ok((slot, how))
}

/// The release and commit a corpus tag names: `v0.2.0` a release, `main-646e008`
/// a commit of main.
fn release_of(tag: &str) -> (Option<String>, Option<String>) {
    let release = tag.strip_prefix('v').filter(|rest| rest.starts_with(|c: char| c.is_ascii_digit()));
    match (release, tag.strip_prefix("main-")) {
        (Some(_), _) => (Some(tag.to_string()), None),
        (None, Some(commit)) => (None, Some(commit.to_string())),
        (None, None) => (None, None),
    }
}

/// The level a save was made in, built from this machine's data (the data
/// of now, whatever it was stamped with).
fn level_of(
    installers: &mut Installers,
    discovery: &vfs::Discovery,
    save: &Save,
) -> Result<Level, String> {
    installers.level(
        discovery,
        &save.level,
        save.snapshot.place.as_ref(),
        save.snapshot.seats(),
    )
}

/// `kjk saves check`: every save of `corpus` (the artifacts' `save-corpus`
/// without one) through the ladder, a table of what each came to; failure
/// when any save failed or there are none.
pub(crate) fn check(discovery: &vfs::Discovery, corpus: Option<&Path>) -> ExitCode {
    let folder = corpus.map_or_else(
        || crate::runtime::artifacts().join(corpus::FOLDER),
        Path::to_path_buf,
    );
    let slots = match corpus::slots(&folder) {
        Ok(slots) if !slots.is_empty() => slots,
        Ok(_) => {
            eprintln!("saves check: no saves in {}", folder.display());
            return ExitCode::FAILURE;
        }
        Err(error) => {
            eprintln!("saves check: {error}");
            return ExitCode::FAILURE;
        }
    };
    let breaking = std::fs::read_to_string(BREAKING).map_or_else(
        |error| {
            eprintln!("saves check: {BREAKING}: {error}; no save is expected to be refused");
            Breaking::default()
        },
        |text| Breaking::parse(&text),
    );
    let mut installers = Installers::default();
    let rows: Vec<Row> = slots
        .iter()
        .map(|(release, name, slot)| {
            eprintln!("saves check: {release}/{name}");
            let row = corpus::guarded(name, release, || {
                check_slot(&mut installers, discovery, &breaking, (release, name), slot)
            });
            eprintln!("saves check: {release}/{name}: {}", row.verdict);
            row
        })
        .collect();
    outln!("{}", corpus::table(&rows));
    if rows.iter().any(|row| row.verdict.failed()) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// The slot in `folder` through the ladder: decoded with its migrations,
/// made a world (re-stamped and reconciled), stepped `TICKS` ticks, one
/// module it left entered again, and its story restored and written again.
fn check_slot(
    installers: &mut Installers,
    discovery: &vfs::Discovery,
    breaking: &Breaking,
    (release, name): (&str, &str),
    folder: &Path,
) -> Row {
    let mut row = Row {
        name: name.to_string(),
        release: release.to_string(),
        formats: "-".into(),
        stamps: 0,
        reconcile: "-".into(),
        story: "-".into(),
        verdict: Verdict::Pass,
    };
    let slot = match Slot::open(folder) {
        Ok(slot) => slot,
        Err(error) => {
            row.verdict = Verdict::Fail(error);
            return row;
        }
    };
    let (save, found) = match Save::decode_versioned(&slot.world) {
        Ok(decoded) => decoded,
        Err(error) => {
            // Refused: its story is what rescues the player, so it must hold.
            let mut problems = Vec::new();
            match slot.story.as_ref() {
                None => row.story = "none".into(),
                Some(story) => match restored_story(installers, discovery, story) {
                    Ok(()) => row.story = "OK".into(),
                    Err((short, long)) => {
                        row.story = short;
                        problems.push(long);
                    }
                },
            }
            row.verdict = Verdict::of(Err(&error), breaking, &problems);
            return row;
        }
    };
    row.formats = corpus::migrated(found);
    let mut problems = Vec::new();
    match level_of(installers, discovery, &save) {
        Err(error) => problems.push(format!("level: {error}")),
        Ok(level) => {
            if let Err(problem) = load_and_step(installers, discovery, &save, &level, &mut row) {
                problems.push(problem);
            }
            let story = slot
                .story
                .clone()
                .unwrap_or_else(|| Story::of(&as_loaded(&save, &level), &level));
            match restore_checked(installers, discovery, &story, &level) {
                Ok(()) => row.story = "OK".into(),
                Err((short, long)) => {
                    row.story = short;
                    problems.push(long);
                }
            }
        }
    }
    row.verdict = Verdict::of(Ok(()), breaking, &problems);
    row
}

/// `save` made a world in `level` (as a load does), stepped `TICKS` ticks,
/// and then one module it left entered; `row` gets the stamps moved and
/// the reconcile's counts.
fn load_and_step(
    installers: &mut Installers,
    discovery: &vfs::Discovery,
    save: &Save,
    level: &Level,
    row: &mut Row,
) -> Result<(), String> {
    let rate = TickRate::new(save.hz).ok_or(format!("the save's {} Hz", save.hz))?;
    row.stamps += usize::from(
        save.restamp(level)
            .map_err(|error| format!("re-stamp: {error}"))?
            .is_some(),
    );
    let mut world = save
        .world(level, 1, None)
        .map_err(|error| format!("world: {error}"))?;
    if let Some(report) = world.reconciled() {
        row.reconcile = corpus::counts(report);
    }
    let last = world.snapshot();
    let mut run = Run { world, rate, last };
    let (seats, from) = (save.seats_for(1), run.last.tick);
    caught("steps", || {
        for tick in from + 1..=from + TICKS {
            run.step(&pilot(PILOT_SEED, tick, seats), StepReason::Authority);
        }
    })?;
    let here = run.world.snapshot().place.map(|place| place.module);
    let left = run
        .world
        .stash()
        .iter()
        .map(|stashed| stashed.place.clone())
        .find(|place| here.as_ref().is_none_or(|here| !place.module.eq_ignore_ascii_case(here)));
    let Some(place) = left else {
        return Ok(());
    };
    let back = installers
        .level(discovery, &save.level, Some(&place), save.snapshot.seats())
        .map_err(|error| format!("{}: {error}", place.module))?;
    row.stamps += usize::from(run.world.stale_stamp(&back).is_some());
    caught("entering a module left", || run.world.install(back, None))?
        .map_err(|error| format!("back into {}: {error}", place.module))?;
    Ok(())
}

/// What `step` made of a panic in it, as a problem rather than a stop.
fn caught<T>(what: &str, step: impl FnOnce() -> T) -> Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(step))
        .map_err(|panic| format!("{what} panicked: {}", corpus::panic_words(&*panic)))
}

/// `save` as this build writes it again once it is loaded: its world's
/// state, placements kept (a save from before rosters has none, so its
/// story would hold nothing of its module's objects), else as it is.
fn as_loaded(save: &Save, level: &Level) -> Save {
    match save.world(level, 1, None) {
        Ok(mut world) => Save {
            hz: save.hz,
            level: level.id(),
            snapshot: world.snapshot(),
            script: world.script_state().clone(),
        },
        Err(_) => save.clone(),
    }
}

/// `story` of a refused save restored in its module, planned as the game's
/// restore path plans it (for the players the story has).
fn restored_story(
    installers: &mut Installers,
    discovery: &vfs::Discovery,
    story: &Story,
) -> Result<(), (String, String)> {
    let seats = story.seats();
    let place = sim::Place {
        game: story.game.clone(),
        module: story.place.module.clone(),
        digest: [0; 32],
        players: seats,
    };
    let level = installers
        .module(discovery, &place, seats)
        .map_err(|error| ("no level".to_string(), format!("story: {error}")))?;
    restore_checked(installers, discovery, story, &level)
}

/// `story` restored in `level` with the install's items to look up.
fn restore_checked(
    installers: &mut Installers,
    discovery: &vfs::Discovery,
    story: &Story,
    level: &Level,
) -> Result<(), (String, String)> {
    let installer = installers.get(discovery, &story.game).ok();
    let search = installer.as_deref().map(ItemSearch::new);
    let items = |template: &str| search.as_ref().and_then(|search| search.facts(template));
    story_round_trip(story, level, &items)
}

/// The story restored in `level` and written again, as the saves' probes
/// compare them; on a loss, the table's words and the whole of it.
fn story_round_trip(
    story: &Story,
    level: &Level,
    items: saves::story::ItemLookup,
) -> Result<(), (String, String)> {
    let (mut world, said) = story
        .restore(level, 1, items)
        .map_err(|error| ("restore failed".to_string(), format!("story: {error}")))?;
    let again = Save {
        hz: sim::TICK_HZ,
        level: level.id(),
        snapshot: world.snapshot(),
        script: world.script_state().clone(),
    };
    let (differ, off) = story.compare(&Story::of(&again, level));
    let far = off > PLACE_SLACK_M;
    if differ.is_empty() && !far {
        return Ok(());
    }
    let mut lost: Vec<String> = differ.iter().map(|field| field.to_string()).collect();
    if far {
        lost.push(format!("leader {off:.1} m off"));
    }
    Err((
        format!("lost {}", lost.join(", ")),
        format!("story: lost {}; the restore said {said:?}", lost.join(", ")),
    ))
}
