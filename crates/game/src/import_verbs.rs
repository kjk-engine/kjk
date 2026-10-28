//! `kjk import <game>` (`docs/IMPORT.md`, part 2): an install's shared data
//! made once into the store every worktree shares: every item and creature
//! template (`sim::pack::Templates`) and every voiced line's length. Plans
//! then take them (`session::AreaCache::with_imported`) and read only what a
//! module's own archives add.
//!
//! Keys: the templates by the install's world data (`Fingerprint::world`),
//! the voices by the voice folders' digests, each with its format word; an
//! install that changes there gets new products, the old ones left for
//! `kjk store gc`.

use std::collections::BTreeMap;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Instant;

use serde_json::json;
use session::imported::{self, Imported};
use store::{Key, Store};
use vfs::fingerprint::Fingerprint;
use vfs::resolver::Resolver;
use vfs::{Discovery, Game};

use crate::installs::Install;
use crate::lifecycle::Installer;
use crate::modules::ResolverSource;
use crate::runtime::Report;

/// The voice folders (K1's `streamwaves`, TSL's `streamvoice`).
const VOICE_FOLDERS: [&str; 2] = ["streamwaves", "streamvoice"];

pub(crate) fn kind(game: Game, product: &str) -> String {
    format!("import/{}/{product}", game.key())
}

/// The templates' and the voices' keys for an install so fingerprinted.
fn keys(print: &Fingerprint) -> (Key, Key) {
    let world = print.world().digest;
    // The rulebook's edition: its table changes are in the templates read.
    let edition = rules::EDITION.to_le_bytes();
    let templates = Key::of(
        &format!("{}/{}", imported::TEMPLATES_FORMAT, sim::pack::VERSION),
        &[&world, &edition],
    );
    let voices: Vec<&[u8]> = VOICE_FOLDERS
        .iter()
        .map(|folder| {
            print
                .groups
                .get(*folder)
                .map_or(&b""[..], |(digest, _)| digest.as_bytes())
        })
        .collect();
    (templates, Key::of(imported::VOICES_FORMAT, &voices))
}

/// The plans' cache for this install: with the import's products, made
/// first when missing (never imported, or a format's version moved),
/// keeping its areas' collision and navigation in the store (both need its
/// fingerprint from the cache alone: no file is hashed here). `from_store`
/// false: none of it, everything read and built from the install (for
/// comparing).
pub(crate) fn area_cache(install: &Install, from_store: bool) -> session::AreaCache {
    if !from_store {
        return session::AreaCache::default();
    }
    let game = install.game;
    let opened = || Some((Store::open()?, install.fingerprint()?));
    let Some((store, print)) = opened() else {
        return session::AreaCache::default();
    };
    let (templates, voices) = keys(print);
    let loaded = || {
        let base = install.base().ok()?;
        let (_, templates) = product(&store, game, "templates", &templates, false, || {
            templates_of(base)
        })
        .ok()?;
        let (_, voices) =
            product(&store, game, "voices", &voices, false, || voices_of(base)).ok()?;
        Some(Imported {
            templates: sim::pack::Templates::decode(&templates).ok()?,
            voices: imported::decode_voices(&voices)?,
        })
    };
    let cache = match loaded() {
        Some(imported) => session::AreaCache::with_imported(Arc::new(imported)),
        None => session::AreaCache::default(),
    };
    cache.with_bakes(session::Bakes {
        store,
        world: print.world().digest,
    })
}

/// The install's `name` product from the store, made and kept there first
/// when it is missing (`Store::get_or_make`; made again with `force`):
/// whether it was made, and its bytes.
fn product(
    store: &Store,
    game: Game,
    name: &str,
    key: &Key,
    force: bool,
    make: impl FnOnce() -> Vec<u8>,
) -> Result<(bool, Vec<u8>), String> {
    let kind = kind(game, name);
    if force {
        let bytes = make();
        store.put(&kind, key, &bytes).map_err(|e| e.to_string())?;
        return Ok((true, bytes));
    }
    let mut made = false;
    let bytes = store.get_or_make(&kind, key, || {
        made = true;
        Ok::<_, String>(make())
    })?;
    Ok((made, bytes))
}

/// Every item and creature template, encoded.
fn templates_of(base: &Resolver) -> Vec<u8> {
    imported::templates(&ResolverSource(base)).encode()
}

/// Every voiced line's length (ms), by lowercase name, encoded.
fn voices_of(base: &Resolver) -> Vec<u8> {
    let wav = fmt_odyssey::ResType::WAV;
    let mut voices = BTreeMap::new();
    let folders = base
        .streams
        .iter()
        .filter(|layer| VOICE_FOLDERS.contains(&layer.name.to_ascii_lowercase().as_str()));
    for layer in folders {
        for ((name, restype), location) in &layer.resources {
            if *restype != wav {
                continue;
            }
            let length = location
                .read()
                .ok()
                .and_then(|file| imported::voice_ms(&file));
            if let Some(ms) = length {
                voices.entry(name.to_ascii_lowercase()).or_insert(ms);
            }
        }
    }
    imported::encode_voices(&voices)
}

fn megabytes(bytes: usize) -> String {
    format!("{:.1} MB", bytes as f64 / 1_000_000.0)
}

/// What [`prepare`] left in the store for an install.
pub struct Prepared {
    pub fingerprint: String,
    pub templates_key: Key,
    pub voices_key: Key,
    pub templates: sim::pack::Templates,
    pub voice_lines: usize,
}

/// The installer's install fingerprinted (its cache in the store) and its
/// shared data made into the store where missing (all of it with `force`),
/// each step told to `say` as it ends: what `kjk import` does before its
/// checks, and what the first-run setup does (`setup`).
pub fn prepare(
    installer: &Installer,
    force: bool,
    say: &mut dyn FnMut(String),
) -> Result<Prepared, String> {
    let game = installer.game;
    let store = crate::store_verbs::opened()?;
    let started = Instant::now();
    let print = vfs::fingerprint::fingerprint(
        game,
        installer.index(),
        Some(&crate::fingerprint_cache(game)),
    )
    .map_err(|e| e.to_string())?;
    say(format!(
        "fingerprint {}… of {} files ({} hashed, {} from the cache) in {:.1} s",
        &print.digest[..12],
        print.files,
        print.hashed,
        print.cached,
        started.elapsed().as_secs_f64()
    ));
    let (templates_key, voices_key) = keys(&print);
    let base = installer.base_resolver()?;

    let started = Instant::now();
    let (made, bytes) = product(&store, game, "templates", &templates_key, force, || {
        templates_of(base)
    })?;
    let templates = sim::pack::Templates::decode(&bytes).map_err(|e| format!("templates: {e}"))?;
    say(format!(
        "templates: {} items, {} creatures, {} ({} in {:.1} s)",
        templates.items.len(),
        templates.creatures.len(),
        megabytes(bytes.len()),
        if made { "made" } else { "already there" },
        started.elapsed().as_secs_f64()
    ));

    let started = Instant::now();
    let (made, bytes) = product(&store, game, "voices", &voices_key, force, || {
        voices_of(base)
    })?;
    let voices = imported::decode_voices(&bytes).ok_or("the voices do not decode")?;
    say(format!(
        "voices: {} lines' lengths, {} ({} in {:.1} s)",
        voices.len(),
        megabytes(bytes.len()),
        if made { "made" } else { "already there" },
        started.elapsed().as_secs_f64()
    ));
    Ok(Prepared {
        fingerprint: print.digest.clone(),
        templates_key,
        voices_key,
        templates,
        voice_lines: voices.len(),
    })
}

/// `kjk import <game>`: fingerprints the install (its cache in the store),
/// makes what is missing (all of it with `force`), then checks that a plan
/// made from the store (the products, an area's bake) is the plan made from
/// the install.
pub fn import(discovery: &Discovery, game: &str, force: bool, module: &str) -> ExitCode {
    let mut report = Report::default();
    let run = |report: &mut Report| -> Result<serde_json::Value, String> {
        let game = Game::from_key(game).ok_or(format!("{game}: k1 or tsl"))?;
        let anims = Arc::new(sim::motion::AnimTable::stand_in());
        let installer = Installer::without_import(discovery, game, anims.clone())?;
        let Prepared {
            fingerprint,
            templates_key,
            voices_key,
            templates,
            voice_lines,
        } = prepare(&installer, force, &mut |line| outln!("  {line}"))?;
        report.check(
            !templates.items.is_empty() && !templates.creatures.is_empty() && voice_lines > 0,
            "the store holds the templates and the voices".to_string(),
        );

        // A plan from the store is the plan from the install: the products,
        // and the area baked by a first plan and read back by a second.
        let with = Installer::new(discovery, game, anims.clone())?;
        report.check(
            with.imported() && with.bakes(),
            "a new installer finds the products and keeps areas in the store (the fingerprint from its cache)".to_string(),
        );
        let timed = |installer: &Installer| -> Result<(sim::Level, f64, bool), String> {
            let started = Instant::now();
            let prepared = installer.prepare_one(module, None)?;
            let ms = started.elapsed().as_secs_f64() * 1000.0;
            let facts = &prepared.plan.facts;
            Ok((
                prepared.plan.level,
                ms,
                facts.cached && facts.gathered_stored,
            ))
        };
        let (live, live_ms, _) = timed(&installer)?;
        let (first, first_ms, _) = timed(&with)?;
        let again = Installer::new(discovery, game, anims.clone())?;
        let (stored, stored_ms, from_store) = timed(&again)?;
        let digest = |level: &sim::Level| level.scripts().map(|library| library.digest());
        report.check(
            [&first, &stored]
                .iter()
                .all(|level| digest(level) == digest(&live) && level.digest() == live.digest()),
            format!(
                "{module} planned from the store is the same level, collision, navigation and library as from the install"
            ),
        );
        report.check(
            from_store,
            format!(
                "its area and script library read back from the store: {stored_ms:.0} ms a plan, against {live_ms:.0} ms from the install ({first_ms:.0} ms the first, baking it)"
            ),
        );
        Ok(json!({
            "game": game.key(),
            "fingerprint": fingerprint,
            "templates": templates_key.as_str(),
            "voices": voices_key.as_str(),
            "items": templates.items.len(),
            "creatures": templates.creatures.len(),
            "voice_lines": voice_lines,
            "plan_ms": { "live": live_ms, "first": first_ms, "stored": stored_ms },
        }))
    };
    match run(&mut report) {
        Ok(facts) => report.finish(facts),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
