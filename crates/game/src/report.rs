//! Bug reports: F12 (`Action::Report`) takes a screenshot and asks what
//! went wrong (`ui::screens::report`); the report is a zip in the user's
//! folder's `reports` holding the note, the screenshot, the machine, the
//! run's manifest, log, crash and flight recorder, the world now, the
//! settings and the breadcrumbs. Every text in it is scrubbed of the home
//! folder, the user's name and the machine's; `host.id` is left out, and
//! none of the games' files go in. Nothing is sent: the folder and GitHub's
//! new-issue page are opened for the player to attach it.

use std::io::Write;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use ui::intent::Intent;

use crate::settings::{Action, KeyMap};
use crate::ui_view::{Ui, UiIntents};

/// Where a new issue is opened (the bug form, nothing else in the URL).
const ISSUE: &str = "https://github.com/kjk-engine/kjk/issues/new?template=bug.yml";
/// How big a report may be: GitHub's attachment limit.
const LIMIT: usize = 25 * 1024 * 1024;
/// How many reports are kept.
const KEPT: usize = 20;
/// How much of the run's log (or manifest, or crash) a report keeps: its end.
const LOG_KEPT: usize = 4 * 1024 * 1024;
/// The screenshot F12 took, in the run's folder.
const SHOT: &str = "report.png";

/// A file of the report.
pub struct Part {
    pub name: String,
    pub bytes: Vec<u8>,
    /// Text is scrubbed (`scrub`); the rest goes as it is.
    pub text: bool,
}

/// What names this machine's player: scrubbed from every text.
#[derive(Default, Clone)]
pub struct Identity {
    pub home: String,
    pub user: String,
    pub host: String,
}

impl Identity {
    pub fn here() -> Identity {
        let var = |names: [&str; 2]| {
            names
                .iter()
                .find_map(|name| std::env::var(name).ok())
                .unwrap_or_else(String::new)
        };
        Identity {
            home: var(["HOME", "USERPROFILE"]),
            user: var(["USER", "USERNAME"]),
            host: sysinfo::System::host_name().unwrap_or_else(String::new),
        }
    }
}

/// `text` with `from` put as `to`, whatever its case.
fn replace_any_case(text: &str, from: &str, to: &str) -> String {
    if from.is_empty() {
        return text.to_string();
    }
    let (lower, from) = (text.to_lowercase(), from.to_lowercase());
    // Byte places carry over only while lowering kept every length.
    if lower.len() != text.len() {
        return text.replace(&from, to);
    }
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (at, _) in lower.match_indices(&from) {
        out.push_str(&text[last..at]);
        out.push_str(to);
        last = at + from.len();
    }
    out.push_str(&text[last..]);
    out
}

/// `text` with the home folder (either way its separators go, any case)
/// as `~`, then the user's name as `<user>` and the machine's as `<host>`.
pub fn scrub(text: &str, home: &str, user: &str, host: &str) -> String {
    let mut out = text.to_string();
    if !home.is_empty() {
        for home in [
            home.to_string(),
            home.replace('\\', "/"),
            home.replace('/', "\\"),
        ] {
            out = replace_any_case(&out, &home, "~");
        }
    }
    out = replace_any_case(&out, user, "<user>");
    replace_any_case(&out, host, "<host>")
}

/// The settings' file without this install's host id (which would tie
/// every report from one machine together).
pub fn settings_text(json: &str) -> String {
    let mut values: std::collections::BTreeMap<String, String> =
        serde_json::from_str(json).unwrap_or_else(|_| std::collections::BTreeMap::new());
    values.remove(crate::settings::HOST_ID);
    serde_json::to_string_pretty(&values).expect("strings serialize")
}

/// The zip of `parts`, their texts scrubbed of `me`; over `limit`, the
/// oldest flight recorder segments dropped until it fits. The zip, and the
/// parts dropped.
pub fn zip(mut parts: Vec<Part>, me: &Identity, limit: usize) -> (Vec<u8>, Vec<String>) {
    for part in parts.iter_mut().filter(|part| part.text) {
        let text = String::from_utf8_lossy(&part.bytes);
        part.bytes = scrub(&text, &me.home, &me.user, &me.host).into_bytes();
    }
    let mut dropped = Vec::new();
    loop {
        let zipped = packed(&parts, &dropped);
        let oldest = parts
            .iter()
            .enumerate()
            .filter_map(|(at, part)| Some((segment(&part.name)?, at)))
            .min();
        match oldest {
            Some((_, at)) if zipped.len() > limit => dropped.push(parts.remove(at).name),
            _ => return (zipped, dropped),
        }
    }
}

/// A flight recorder segment's number (`flight-3.kjkf`).
pub fn segment(name: &str) -> Option<u32> {
    name.strip_prefix("flight-")?
        .strip_suffix(".kjkf")?
        .parse()
        .ok()
}

/// `parts` deflated into a zip, with a note of those `dropped`.
fn packed(parts: &[Part], dropped: &[String]) -> Vec<u8> {
    let mut zip = ::zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = ::zip::write::SimpleFileOptions::default()
        .compression_method(::zip::CompressionMethod::Deflated);
    let note = (!dropped.is_empty()).then(|| Part {
        name: "dropped.txt".into(),
        bytes: format!("left out to fit {LIMIT} bytes: {}\n", dropped.join(", ")).into_bytes(),
        text: true,
    });
    for part in parts.iter().chain(note.as_ref()) {
        let written = zip
            .start_file(part.name.as_str(), options)
            .map_err(std::io::Error::other)
            .and_then(|()| zip.write_all(&part.bytes));
        if let Err(error) = written {
            warn!(target: "report", "{}: {error}", part.name);
        }
    }
    zip.finish()
        .map(std::io::Cursor::into_inner)
        .unwrap_or_else(|error| {
            warn!(target: "report", "the zip: {error}");
            Vec::new()
        })
}

/// `bytes` kept in `reports` as `kjk-report-<UTC time>.zip`, the newest
/// `KEPT` kept; its path.
pub fn save(reports: &Path, bytes: &[u8]) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(reports)?;
    let now = saves::now();
    let (year, month, day) = saves::civil_from_days((now / 86_400) as i64);
    let seconds = now % 86_400;
    let stem = format!(
        "kjk-report-{year:04}{month:02}{day:02}-{:02}{:02}{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    );
    let path = (1..)
        .map(|n| {
            reports.join(if n == 1 {
                format!("{stem}.zip")
            } else {
                format!("{stem}-{n}.zip")
            })
        })
        .find(|path| !path.exists())
        .expect("a free name");
    std::fs::write(&path, bytes)?;
    let mut kept: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(reports)?
        .flatten()
        .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
        .collect();
    kept.sort();
    for (_, old) in &kept[..kept.len().saturating_sub(KEPT)] {
        let _ = std::fs::remove_file(old);
    }
    Ok(path)
}

/// The machine: the build, OS and CPU, memory, and the window.
fn system_text(settings: Option<&crate::settings::Settings>) -> String {
    let mut system = sysinfo::System::new();
    system.refresh_memory();
    system.refresh_cpu_all();
    let cpu = system
        .cpus()
        .first()
        .map_or_else(String::new, |cpu| cpu.brand().to_string());
    let setting = |key: &str| {
        settings
            .and_then(|settings| settings.value(key))
            .unwrap_or("")
            .to_string()
    };
    format!(
        "{}os version {}\ncpu {cpu} ({} cores)\nmemory {} MiB\nwindow {} {}\n",
        crate::crash::facts(),
        sysinfo::System::long_os_version().unwrap_or_else(String::new),
        system.cpus().len(),
        system.total_memory() / (1024 * 1024),
        setting("display.mode"),
        setting("display.size"),
    )
}

/// What `run`'s flight recorder segments are of: each one's level and the
/// digest of its data (another edition or a mod differs).
fn data_text(segments: &[Part]) -> String {
    segments
        .iter()
        .filter_map(|part| {
            let recording = sim::flight::read(&part.bytes).ok()?;
            let digest: String = recording
                .level
                .digest
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            Some(format!("{} {} {digest}\n", part.name, recording.level.name))
        })
        .collect()
}

/// `bytes`' last whole lines within `most` bytes (all of them when they fit).
fn tail(bytes: Vec<u8>, most: usize) -> Vec<u8> {
    if bytes.len() <= most {
        return bytes;
    }
    let cut = bytes.len() - most;
    let start = bytes[cut..]
        .iter()
        .position(|byte| *byte == b'\n')
        .map_or(bytes.len(), |at| cut + at + 1);
    bytes[start..].to_vec()
}

/// The report's parts from `run` (the run's folder), with `note`, the world
/// `now`, the settings' file and the breadcrumbs.
fn gather(
    run: &Path,
    note: &str,
    now: Option<Vec<u8>>,
    settings: Option<&crate::settings::Settings>,
) -> Vec<Part> {
    let text = |name: &str, bytes: Vec<u8>| Part {
        name: name.into(),
        bytes,
        text: true,
    };
    let mut parts = vec![
        text("note.txt", note.as_bytes().to_vec()),
        text("system.txt", system_text(settings).into_bytes()),
    ];
    for name in ["manifest.json", "kjk.log", crate::crash::CRASH] {
        if let Ok(bytes) = std::fs::read(run.join(name)) {
            parts.push(text(name, tail(bytes, LOG_KEPT)));
        }
    }
    let mut segments: Vec<Part> = std::fs::read_dir(run)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter_map(|path| {
            let name = path.file_name()?.to_str()?.to_string();
            segment(&name)?;
            Some(Part {
                bytes: std::fs::read(&path).ok()?,
                name,
                text: false,
            })
        })
        .collect();
    segments.sort_by_key(|part| segment(&part.name));
    parts.push(text("data.txt", data_text(&segments).into_bytes()));
    parts.extend(segments);
    if let Ok(shot) = std::fs::read(run.join(SHOT)) {
        parts.push(Part {
            name: "screenshot.png".into(),
            bytes: shot,
            text: false,
        });
    }
    if let Some(now) = now {
        parts.push(Part {
            name: "now.kjkv".into(),
            bytes: now,
            text: false,
        });
    }
    if let Some(json) = settings
        .and_then(|settings| settings.path())
        .and_then(|path| std::fs::read_to_string(path).ok())
    {
        parts.push(text("settings.json", settings_text(&json).into_bytes()));
    }
    let crumbs = crate::crash::crumbs().map_or_else(String::new, |crumbs| crumbs.to_string());
    parts.push(text("breadcrumbs.txt", crumbs.into_bytes()));
    parts
}

/// A game started: a report now is of it, not of a run that crashed
/// before.
pub fn game_started(world: &mut World) {
    world.remove_resource::<CrashedRun>();
}

/// The run a report is of when it is not this one: the one that crashed
/// (`crash::notice`), until a report of it is made.
#[derive(Resource)]
pub struct CrashedRun(pub PathBuf);

pub struct ReportPlugin;

impl Plugin for ReportPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (ask, make)
                .chain()
                .after(crate::ui_view::Stepped)
                .run_if(resource_exists::<Ui>),
        );
    }
}

/// F12: the frame as it is (before the box shows) into the run's folder,
/// then the box.
fn ask(
    mut commands: Commands,
    mut ui: ResMut<Ui>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    map: Option<Res<KeyMap>>,
    folder: Res<console::RunFolder>,
    windows: Query<(), With<bevy::window::PrimaryWindow>>,
) {
    let (Some(keys), Some(map)) = (keys, map) else {
        return;
    };
    if !map.pressed_now(&keys, Action::Report) || ui.stack.has("report") {
        return;
    }
    if !windows.is_empty() {
        let path = folder.0.join(SHOT);
        commands
            .spawn(bevy::render::view::screenshot::Screenshot::primary_window())
            .observe(bevy::render::view::screenshot::save_to_disk(path));
    }
    let ui = &mut *ui;
    ui.stack.open("report", &ui.vm);
}

/// The box's Make report: the report made and kept, the folder and the
/// new-issue page opened, and a message saying where it is.
fn make(world: &mut World) {
    let Some(note) = take_report(world) else {
        return;
    };
    let crashed = world.remove_resource::<CrashedRun>();
    let run = crashed.as_ref().map_or_else(
        || world.resource::<console::RunFolder>().0.clone(),
        |crashed| crashed.0.clone(),
    );
    // The world now, unless the report is of a run that crashed.
    let now = crashed
        .is_none()
        .then(|| crate::sim_host::current_save(world))
        .flatten();
    let settings = world.get_resource::<crate::settings::Settings>().cloned();
    let me = Identity::here();
    let (zipped, dropped) = zip(gather(&run, &note, now, settings.as_ref()), &me, LIMIT);
    if !dropped.is_empty() {
        warn!(target: "report", ?dropped, "left out of the report to fit");
    }
    let reports = crate::settings::folder().map_or_else(
        || crate::runtime::artifacts().join("reports"),
        |folder| folder.join("reports"),
    );
    // Only a player's game opens anything on the desktop.
    let player = world
        .get_resource::<crate::crash::PlayerRun>()
        .is_some_and(|player| player.0);
    let told = match save(&reports, &zipped) {
        Ok(path) => {
            info!(target: "report", path = %path.display(), bytes = zipped.len(), "report made");
            if player {
                open_folder(&reports);
                open_page(ISSUE);
            }
            format!(
                "The report is saved as {}. Attach it to the issue opened in your browser.",
                scrub(&path.display().to_string(), &me.home, &me.user, &me.host)
            )
        }
        Err(error) => format!("The report could not be saved: {error}"),
    };
    let mut ui = world.resource_mut::<Ui>();
    let ui = &mut *ui;
    ui.stack.close("report");
    ui.vm.session.error = Some(told);
    ui.stack.open("message", &ui.vm);
}

/// The box's note, if its Make report was pressed (the intent taken).
fn take_report(world: &mut World) -> Option<String> {
    let mut intents = world.get_resource_mut::<UiIntents>()?;
    let at = intents
        .0
        .iter()
        .position(|intent| matches!(intent, Intent::Report(_)))?;
    match intents.0.remove(at) {
        Intent::Report(note) => Some(note),
        _ => None,
    }
}

/// `folder` opened as the system opens one.
fn open_folder(folder: &Path) {
    let program = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    spawn(program, &[folder.as_os_str()]);
}

/// The web page `url` opened in the default browser. Not by `explorer` on
/// Windows: given an address with a query, it opens the Documents folder.
fn open_page(url: &str) {
    let url = std::ffi::OsStr::new(url);
    if cfg!(windows) {
        spawn("rundll32", &["url.dll,FileProtocolHandler".as_ref(), url]);
    } else if cfg!(target_os = "macos") {
        spawn("open", &[url]);
    } else {
        spawn("xdg-open", &[url]);
    }
}

fn spawn(program: &str, args: &[&std::ffi::OsStr]) {
    if let Err(error) = std::process::Command::new(program).args(args).spawn() {
        warn!(target: "report", "{program} {args:?}: {error}");
    }
}

#[cfg(feature = "probes")]
mod probes;
#[cfg(feature = "probes")]
pub use probes::probes;
