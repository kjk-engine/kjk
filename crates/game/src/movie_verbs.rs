//! The games' movies (I20, `docs/SOUND.md`, "Movies"), from the install's
//! `movies` folder. A Bink 1 movie, which every KOTOR and TSL movie is, is
//! decoded by kjk as it plays (`film`). A movie of another kind is made
//! ours by `ffmpeg` the first time it is wanted (`movie::ffmpeg`) and kept
//! in the store by its bytes' hash, so it is transcoded once per machine.
//! Without `ffmpeg`, such a movie is skipped, and the reason is said.
//!
//! - `kjk movie check`: the container and the JPEG splitting, on frames
//!   made here;
//! - `kjk movie import <game> [names…]`: the movies made now (every one by
//!   default), with their sizes and times (`facts.json`).

use std::process::ExitCode;
use std::time::Instant;

use fmt_odyssey::ResType;
use movie::Movie;
use movie::ffmpeg::{FORMAT, Ffmpeg};
use serde_json::json;
use store::{Key, Store};
use vfs::Game;
use vfs::resolver::Resolver;

use crate::runtime::Report;

const FOLDER: &str = "movies";

fn bik() -> ResType {
    ResType::BIK
}

/// The movies `resolver`'s install has, by name.
pub(crate) fn names(resolver: &Resolver) -> Vec<String> {
    let bik = bik();
    resolver
        .streams
        .iter()
        .filter(|layer| layer.name.eq_ignore_ascii_case(FOLDER))
        .flat_map(|layer| layer.resources.keys())
        .filter(|(_, restype)| *restype == bik)
        .map(|(name, _)| name.clone())
        .collect()
}

/// Where a movie came from: the store, or made now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Made {
    Stored,
    Now,
}

/// `game`'s movie `name`, ours: from the store, else made by `ffmpeg` and
/// kept there.
pub(crate) fn movie(
    store: Option<&Store>,
    game: Game,
    resolver: &Resolver,
    name: &str,
) -> Result<(Movie, Made), String> {
    let location = resolver
        .stream(FOLDER, name, bik())
        .ok_or(format!("no movie {name}"))?;
    let bytes = location
        .read()
        .map_err(|error| format!("{name}: {error}"))?;
    let key = Key::of(FORMAT, &[&bytes]);
    let kind = crate::import_verbs::kind(game, "movies");
    // One stored that does not read (a format of another build) is made again.
    if let Some(stored) = store
        .and_then(|store| store.get(&kind, &key))
        .and_then(|stored| Movie::decode(stored).ok())
    {
        return Ok((stored, Made::Stored));
    }
    if location.offset != 0 || location.size != bytes.len() as u64 {
        return Err(format!("{name}: not a file of its own"));
    }
    let made = Ffmpeg::find()?
        .transcode(&location.path)
        .map_err(|error| format!("{name}: {error}"))?;
    if let Some(store) = store {
        store
            .put(&kind, &key, &made)
            .map_err(|error| format!("{name}: {error}"))?;
    }
    Ok((Movie::decode(made)?, Made::Now))
}

/// A movie to play: a Bink this build decodes as it plays, or a movie that
/// `ffmpeg` made into the store.
pub(crate) enum Film {
    Bink(movie::Bink),
    Stored(Movie),
}

/// `game`'s movie `name`, to play. Every KOTOR and TSL movie is Bink 1,
/// which kjk decodes itself. Only a movie of another kind (a mod's Bink 2,
/// say) is made by `ffmpeg`, as `movie` does.
pub(crate) fn film(
    store: Option<&Store>,
    game: Game,
    resolver: &Resolver,
    name: &str,
) -> Result<Film, String> {
    let bytes = resolver
        .stream(FOLDER, name, bik())
        .ok_or(format!("no movie {name}"))?
        .read()
        .map_err(|error| format!("{name}: {error}"))?;
    match movie::Bink::parse(bytes) {
        Ok(bink) => Ok(Film::Bink(bink)),
        Err(why) => {
            bevy::log::info!(target: "movie", %name, %why, "made by ffmpeg");
            movie(store, game, resolver, name)
                .map(|(movie, _)| Film::Stored(movie))
                .map_err(|error| format!("{why}, and {error}"))
        }
    }
}

/// `kjk movie check`: a movie made of frames made here reads back as made,
/// and the frames split from one stream are the frames joined.
pub fn check() -> ExitCode {
    let mut report = Report::default();
    let jpeg = |shade: u8, width: u32| {
        let image = image::RgbImage::from_fn(width, 8, |x, _| image::Rgb([shade, x as u8, 0]));
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image)
            .write_to(&mut out, image::ImageFormat::Jpeg)
            .expect("a JPEG is written to memory");
        out.into_inner()
    };
    let frames: Vec<Vec<u8>> = (0..5).map(|n| jpeg(n * 50, 16 + u32::from(n))).collect();
    let stream: Vec<u8> = frames.concat();
    let split = movie::split_jpegs(&stream);
    report.check(
        split
            .as_ref()
            .is_ok_and(|split| split.iter().copied().eq(frames.iter().map(Vec::as_slice))),
        "five JPEGs joined split back into the five".into(),
    );
    report.check(
        movie::split_jpegs(&stream[..stream.len() - 1]).is_err(),
        "a stream cut short does not split".into(),
    );
    let slices: Vec<&[u8]> = frames.iter().map(Vec::as_slice).collect();
    let sound = b"not really an mp3";
    let made = Movie::encode(
        (16, 8),
        (2997, 100),
        &slices,
        Some((movie::Coding::Mp3, sound)),
    );
    let read = Movie::decode(made.clone());
    report.check(
        read.as_ref().is_ok_and(|movie| {
            movie.frames() == 5
                && (0..5).all(|n| movie.frame(n) == Some(&frames[n][..]))
                && movie.sound() == Some((movie::Coding::Mp3, &sound[..]))
                && movie.rate == (2997, 100)
                && (movie.width, movie.height) == (16, 8)
        }),
        "a movie reads back as it was made: frames, sound, rate, size".into(),
    );
    report.check(
        read.as_ref().is_ok_and(|movie| {
            movie.frame_at(0.0) == 0
                && movie.frame_at(0.1) == 2
                && movie.frame_at(99.0) == 4
                && (movie.seconds() - 5.0 * 100.0 / 2997.0).abs() < 1e-9
        }),
        "the frame shown at a time: 0 s the first, 0.1 s the third at 29.97, the last past the end"
            .into(),
    );
    report.check(
        Movie::decode(made[..made.len() - 1].to_vec()).is_err()
            && Movie::decode(b"KJKMOVIX".to_vec()).is_err(),
        "a movie cut short, or not a movie, is refused".into(),
    );
    report.check(
        read.ok()
            .and_then(|movie| movie::picture(movie.frame(1)?).ok())
            .is_some_and(|(w, h, pixels)| (w, h) == (17, 8) && pixels.len() == 17 * 8 * 4),
        "a frame decodes to its picture".into(),
    );
    report.finish(json!({}))
}

/// `kjk movie compare <game> [names…] [--frames N]`: kjk's Bink decoder
/// against `ffmpeg` on the install's movies (every one when none is named).
/// Each picture must match exactly and each sound sample closely
/// (`movie::compare`).
pub fn compare(
    discovery: &vfs::Discovery,
    game: &str,
    names: &[String],
    frames: Option<usize>,
) -> ExitCode {
    let mut report = Report::default();
    let run = |report: &mut Report| -> Result<serde_json::Value, String> {
        let game = Game::from_key(game).ok_or(format!("{game}: k1 or tsl"))?;
        let resolver = crate::viewer::resolver(discovery, game, None)?;
        let mut wanted = names.to_vec();
        if wanted.is_empty() {
            wanted = self::names(&resolver);
            wanted.sort();
        }
        let mut facts = Vec::new();
        for name in &wanted {
            let started = Instant::now();
            let compared = (|| {
                let location = resolver
                    .stream(FOLDER, name, bik())
                    .ok_or(format!("no movie {name}"))?;
                let bink = movie::Bink::parse(location.read().map_err(|e| e.to_string())?)?;
                let video =
                    movie::compare::video(&location.path, &bink, frames.unwrap_or(usize::MAX))?;
                let audio = movie::compare::audio(&location.path, &bink)?;
                Ok::<_, String>((bink.width, bink.height, video, audio))
            })();
            let seconds = started.elapsed().as_secs_f64();
            match &compared {
                Ok((width, height, video, audio)) => outln!(
                    "  {name}: {width}×{height}, {video} frames match, sound {} ({seconds:.1} s)",
                    audio.map_or("none".into(), |largest| format!("within {largest}")),
                ),
                Err(why) => outln!("  {name}: {why}"),
            }
            report.check(
                compared.is_ok(),
                format!("{name} decodes as ffmpeg decodes it"),
            );
            facts.push(match compared {
                Ok((width, height, video, audio)) => json!({
                    "name": name, "size": [width, height], "frames": video,
                    "largest_sample_difference": audio, "seconds": seconds,
                }),
                Err(why) => json!({ "name": name, "error": why }),
            });
        }
        Ok(json!({ "game": game.key(), "movies": facts }))
    };
    match run(&mut report) {
        Ok(facts) => report.finish(facts),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

/// `kjk movie import <game> [names…]`: each movie made ours now (every one
/// when none is named), from the store when it is already there.
pub fn import(discovery: &vfs::Discovery, game: &str, names: &[String]) -> ExitCode {
    let mut report = Report::default();
    let run = |report: &mut Report| -> Result<serde_json::Value, String> {
        let game = Game::from_key(game).ok_or(format!("{game}: k1 or tsl"))?;
        let resolver = crate::viewer::resolver(discovery, game, None)?;
        let store = crate::store_verbs::opened()?;
        let mut wanted = names.to_vec();
        if wanted.is_empty() {
            wanted = self::names(&resolver);
        }
        let mut facts = Vec::new();
        let mut failed = 0;
        for name in &wanted {
            let started = Instant::now();
            match movie(Some(&store), game, &resolver, name) {
                Ok((movie, made)) => {
                    let ms = started.elapsed().as_secs_f64() * 1000.0;
                    outln!(
                        "  {name}: {}×{}, {} frames ({:.1} s), sound {:?}; {} in {ms:.0} ms",
                        movie.width,
                        movie.height,
                        movie.frames(),
                        movie.seconds(),
                        movie.sound().map(|(coding, bytes)| (coding, bytes.len())),
                        if made == Made::Now {
                            "made"
                        } else {
                            "from the store"
                        },
                    );
                    facts.push(json!({
                        "name": name,
                        "size": [movie.width, movie.height],
                        "frames": movie.frames(),
                        "seconds": movie.seconds(),
                        "made": made == Made::Now,
                        "ms": ms,
                    }));
                }
                Err(error) => {
                    failed += 1;
                    outln!("  {name}: {error}");
                }
            }
        }
        report.check(
            failed == 0,
            format!(
                "{} of {} movies made or read",
                wanted.len() - failed,
                wanted.len()
            ),
        );
        Ok(json!({ "game": game.key(), "movies": facts }))
    };
    match run(&mut report) {
        Ok(facts) => report.finish(facts),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
