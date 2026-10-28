//! What a crash leaves in its run's folder: `crash.txt` with the panic, a
//! backtrace, the build and machine, and the breadcrumbs (where the play
//! was: tick, module, conversation, script, party, screen). A run that ends
//! well writes `ended`; a player's run is marked `player`, so the next
//! start can tell a crash or a kill from a quiet exit and say so once
//! (`seen`).

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use bevy::prelude::Resource;

/// The panic's report in a run's folder.
pub const CRASH: &str = "crash.txt";
/// Written when a run ends through the app's exit.
pub const ENDED: &str = "ended";
/// Written once the player was told about the run's end.
pub const SEEN: &str = "seen";
/// Marks a player's run (`PlayerRun`): only those raise the notice.
pub const PLAYER: &str = "player";

/// How long a crashed run is kept past the pruning, and how many of them.
const CRASH_DAYS: u64 = 30;
const CRASHES_KEPT: usize = 10;

/// Whether the run is a player's: a window and no script (`kjk`, `kjk
/// play`, `host`, `join`). Only those back saves up, autosave and tell of
/// a crash; the panic hook and the breadcrumbs are every run's.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlayerRun(pub bool);

impl PlayerRun {
    /// A probe's run plays a player's with `KJK_PLAYER_RUN=1`.
    pub fn of(headless: bool, script: &str) -> Self {
        let forced = cfg!(feature = "probes") && std::env::var("KJK_PLAYER_RUN").is_ok_and(|value| value == "1");
        Self(forced || !headless && script.trim().is_empty())
    }
}

/// Where the play was, kept fresh by the sim host and the screens.
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct Breadcrumbs {
    pub tick: u64,
    pub module: String,
    pub area: String,
    /// The conversation's dlg and node.
    pub conversation: Option<(String, u32)>,
    pub script: Option<String>,
    pub party: Vec<String>,
    pub screen: Option<String>,
}

impl std::fmt::Display for Breadcrumbs {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(out, "tick {}", self.tick)?;
        writeln!(out, "module {} ({})", self.module, self.area)?;
        match &self.conversation {
            Some((dlg, node)) => writeln!(out, "conversation {dlg} node {node}")?,
            None => writeln!(out, "conversation none")?,
        }
        writeln!(out, "script {}", self.script.as_deref().unwrap_or("none"))?;
        writeln!(out, "party {}", self.party.join(", "))?;
        writeln!(out, "screen {}", self.screen.as_deref().unwrap_or("none"))
    }
}

static CRUMBS: Mutex<Breadcrumbs> = Mutex::new(Breadcrumbs {
    tick: 0,
    module: String::new(),
    area: String::new(),
    conversation: None,
    script: None,
    party: Vec::new(),
    screen: None,
});

/// The run's folder and the machine's facts, for the hook.
static HOOKED: Mutex<Option<(PathBuf, String)>> = Mutex::new(None);

/// Changes the breadcrumbs.
pub fn note(change: impl FnOnce(&mut Breadcrumbs)) {
    if let Ok(mut crumbs) = CRUMBS.lock() {
        change(&mut crumbs);
    }
}

/// The breadcrumbs, or none while they are being changed (a panic inside
/// `note` must not wait on itself).
pub fn crumbs() -> Option<Breadcrumbs> {
    CRUMBS.try_lock().ok().map(|crumbs| crumbs.clone())
}

/// Adds the render adapter to the facts a crash writes.
pub fn set_adapter(adapter: &str) {
    if let Ok(mut hooked) = HOOKED.lock()
        && let Some((_, facts)) = hooked.as_mut()
    {
        facts.push_str(&format!("render adapter {adapter}\n"));
    }
}

/// The facts of this build and machine a crash and a report carry.
pub fn facts() -> String {
    format!(
        "kjk {} ({})\nos {} {}\n",
        env!("KJK_VERSION"),
        env!("KJK_GIT_SHA"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
}

/// Makes a panic write `crash.txt` into `folder` before the default hook
/// prints it. Installed once; a later run moves it to its own folder.
pub fn install_hook(folder: PathBuf, facts: String) {
    let first = {
        let mut hooked = HOOKED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let first = hooked.is_none();
        *hooked = Some((folder, facts));
        first
    };
    if !first {
        return;
    }
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let hooked = HOOKED.try_lock().ok().and_then(|hooked| hooked.clone());
        if let Some((folder, facts)) = hooked {
            let message = info
                .payload()
                .downcast_ref::<&str>()
                .map(|text| (*text).to_owned())
                .or_else(|| info.payload().downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "a panic with no message".into());
            let location = info
                .location()
                .map_or_else(|| "unknown".into(), ToString::to_string);
            let thread = std::thread::current();
            let backtrace = std::backtrace::Backtrace::force_capture().to_string();
            let crumbs = crumbs();
            let written = write_crash(
                &folder,
                &message,
                &location,
                thread.name().unwrap_or("unnamed"),
                &backtrace,
                &facts,
                crumbs.as_ref(),
            );
            if let Err(error) = written {
                eprintln!("cannot write {}: {error}", folder.join(CRASH).display());
            }
        }
        default(info);
    }));
}

/// Writes `crash.txt`. A second panic (another thread's) is added below
/// the first.
pub fn write_crash(
    folder: &Path,
    message: &str,
    location: &str,
    thread: &str,
    backtrace: &str,
    facts: &str,
    crumbs: Option<&Breadcrumbs>,
) -> std::io::Result<()> {
    use std::io::Write;
    let crumbs = crumbs.map_or_else(|| "breadcrumbs: unavailable\n".into(), ToString::to_string);
    let text = format!(
        "panic: {message}\nat {location}\nthread {thread}\n\n{facts}\n{crumbs}\nbacktrace:\n{backtrace}\n"
    );
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(folder.join(CRASH))?;
    file.write_all(text.as_bytes())?;
    file.sync_all()
}

/// Writes the marker `name` (`ENDED`, `SEEN`, `PLAYER`) into `folder`.
pub fn mark(folder: &Path, name: &str) {
    if let Err(error) = std::fs::write(folder.join(name), b"") {
        eprintln!("cannot write {}: {error}", folder.join(name).display());
    }
}

/// The newest player's run in `runs` other than `current` that crashed or
/// never ended, and whose end the player was not yet told of.
pub fn unexpected_end(runs: &Path, current: &Path) -> Option<PathBuf> {
    let mut folders = run_folders(runs);
    folders.sort();
    folders.into_iter().rev().find(|folder| {
        folder != current
            && folder.join(PLAYER).exists()
            && !folder.join(SEEN).exists()
            && (folder.join(CRASH).exists() || !folder.join(ENDED).exists())
    })
}

/// What the main menu says once after a player's run that crashed or was
/// cut off.
pub const NOTICE: &str =
    "kjk stopped unexpectedly last time. Press F12 to make a bug report of it.";

/// Why the saves could not be backed up before this release touched them
/// (`saves::backup_on_new_version`): told on the main menu.
#[derive(Resource, Debug)]
pub struct BackupFailed(pub String);

/// On a player's main menu, tells of the last run that ended unexpectedly,
/// once (`unexpected_end`, then `SEEN`), and of saves not backed up.
pub(crate) fn notice(
    mut commands: bevy::prelude::Commands,
    player: bevy::prelude::Res<PlayerRun>,
    folder: bevy::prelude::Res<console::RunFolder>,
    backup: Option<bevy::prelude::Res<BackupFailed>>,
    ui: Option<bevy::prelude::ResMut<crate::ui_view::Ui>>,
) {
    let Some(mut ui) = ui.filter(|_| player.0) else {
        return;
    };
    let mut told = Vec::new();
    if let Some(previous) = unexpected_end(&crate::runtime::runs(), &folder.0) {
        mark(&previous, SEEN);
        told.push(NOTICE.to_string());
        commands.insert_resource(crate::report::CrashedRun(previous));
    }
    if let Some(backup) = backup {
        told.push(format!("The saves could not be backed up: {}", backup.0));
    }
    if told.is_empty() {
        return;
    }
    let ui = &mut *ui;
    ui.vm.session.error = Some(told.join("\n"));
    ui.stack.open("message", &ui.vm);
}

/// The run folders in `runs`.
pub(crate) fn run_folders(runs: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(runs).map_or_else(
        |_| Vec::new(),
        |entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                .collect()
        },
    )
}

/// Which of `folders` the pruning keeps: the newest `keep`, and of the
/// rest the newest `CRASHES_KEPT` that crashed within `CRASH_DAYS` of `now`.
pub(crate) fn kept(mut folders: Vec<PathBuf>, keep: usize, now: SystemTime) -> Vec<PathBuf> {
    folders.sort();
    let old = folders.len().saturating_sub(keep);
    let recent = |folder: &PathBuf| {
        std::fs::metadata(folder.join(CRASH))
            .and_then(|meta| meta.modified())
            // A crash stamped after `now` (a clock set back) is recent.
            .is_ok_and(|at| {
                now.duration_since(at)
                    .map_or(true, |age| age < Duration::from_secs(CRASH_DAYS * 86_400))
            })
    };
    let mut crashed: Vec<PathBuf> = folders[..old].iter().filter(|folder| recent(folder)).cloned().collect();
    crashed.drain(..crashed.len().saturating_sub(CRASHES_KEPT));
    crashed.extend(folders.drain(old..));
    crashed
}

#[cfg(feature = "probes")]
mod probes;
#[cfg(feature = "probes")]
pub use probes::probes;
