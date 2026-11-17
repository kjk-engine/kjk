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
        let forced = cfg!(feature = "probes")
            && std::env::var("KJK_PLAYER_RUN").is_ok_and(|value| value == "1");
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
        let mut hooked = HOOKED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
            // A panic after the render device was lost is what the lost
            // device left behind, such as a buffer that can no longer be
            // written. The crash says first that the driver stopped.
            let lost = device_lost();
            let message = match &lost {
                Some(reason) => format!("{}\nthen: {message}", lost_line(reason)),
                None => message,
            };
            let facts = match &lost {
                Some(_) => with_render_facts(&facts),
                None => facts,
            };
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
            // A Mac shows a dialog only from its main thread, and the render
            // world may run on another. It tells the player on the next
            // start (`notice`).
            if lost.is_some() && cfg!(not(target_os = "macos")) {
                tell_player();
            }
        }
        default(info);
    }));
}

/// What the player is told when the graphics driver stopped responding.
pub const DRIVER_STOPPED: &str = "The graphics driver stopped responding, so kjk had to close. Updating the graphics driver may fix this. If it happens again, set Shadows to Low or Off in the options.";

/// What the main menu says once after a run that ended that way. `{report}`
/// is the key bound to the bug report.
pub const DRIVER_NOTICE: &str = "kjk closed last time because the graphics driver stopped responding. Updating the graphics driver may fix this. If it happens again, set Shadows to Low or Off in the options. Press {report} to create a bug report.";

/// How `crash.txt` begins when the render device was lost. The notice looks
/// for it.
const LOST_MARK: &str = "the graphics driver stopped responding (render device lost):";

/// The driver's reason, once the render device was lost (`watch_device`).
static LOST: Mutex<Option<String>> = Mutex::new(None);

/// Whether the player was told already, by the panic hook or by
/// `end_on_lost_device`.
static TOLD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether this run is a player's, who is shown a dialog (`PlayerRun`).
static PLAYER_DIALOG: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Makes a lost render device show a dialog in this run (a player's).
pub fn dialog_on_lost_device(player: PlayerRun) {
    PLAYER_DIALOG.store(player.0, std::sync::atomic::Ordering::Relaxed);
}

/// Records that the render device was lost, and why.
pub fn lose_device(reason: String) {
    let mut lost = LOST
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    lost.get_or_insert(reason);
}

/// The driver's reason, when the render device was lost.
pub fn device_lost() -> Option<String> {
    LOST.try_lock().ok().and_then(|lost| lost.clone())
}

fn lost_line(reason: &str) -> String {
    format!("{LOST_MARK} {reason}")
}

/// Whether the crash in `folder` was the render device's loss.
pub fn lost_device_in(folder: &Path) -> bool {
    std::fs::read_to_string(folder.join(CRASH)).is_ok_and(|text| text.contains(LOST_MARK))
}

/// Shows `DRIVER_STOPPED` in a dialog, once, in a player's run.
fn tell_player() {
    use std::sync::atomic::Ordering;
    if !PLAYER_DIALOG.load(Ordering::Relaxed) || TOLD.swap(true, Ordering::Relaxed) {
        return;
    }
    rfd::MessageDialog::new()
        .set_title("kjk")
        .set_description(DRIVER_STOPPED)
        .set_level(rfd::MessageLevel::Error)
        .show();
}

/// Ends the game when the render device is lost. Bevy's own callback for a
/// lost device is replaced by `watch_device`, which records the loss for
/// the panic hook. A loss that no panic follows ends the game here, with
/// the same report.
pub struct DeviceLostPlugin;

impl bevy::app::Plugin for DeviceLostPlugin {
    fn build(&self, app: &mut bevy::app::App) {
        app.add_systems(bevy::app::First, end_on_lost_device);
    }

    fn finish(&self, app: &mut bevy::app::App) {
        if let Some(render_app) = app.get_sub_app_mut(bevy::render::RenderApp) {
            render_app.add_systems(bevy::render::RenderStartup, watch_device);
            #[cfg(feature = "probes")]
            render_app.add_systems(bevy::render::Render, probe_lose_device);
        }
    }
}

/// Replaces Bevy's callback for a lost device, which it set as it made the
/// device. Bevy's callback logs the loss and quits at the next frame's extract, but
/// the render systems still run before that and panic on the first buffer
/// they write. This one logs the loss too, and records it before any of
/// that, so the panic's report and the player are told what happened.
fn watch_device(device: bevy::prelude::Res<bevy::render::renderer::RenderDevice>) {
    device
        .wgpu_device()
        .set_device_lost_callback(|reason, text| on_device_lost(format!("{reason:?} {text}")));
}

fn on_device_lost(reason: String) {
    bevy::log::error!("Caught DeviceLost error: {reason}");
    bevy::log::error!("{}", crate::render_facts::text());
    lose_device(reason);
}

/// `facts` with the render facts of the last frames below them
/// (`render_facts`), for the crash of a lost render device.
pub fn with_render_facts(facts: &str) -> String {
    format!("{facts}{}", crate::render_facts::text())
}

/// Ends the game once the render device is lost, with `crash.txt` and the
/// dialog, unless a panic did so first.
fn end_on_lost_device(
    folder: Option<bevy::prelude::Res<console::RunFolder>>,
    mut exit: bevy::prelude::MessageWriter<bevy::app::AppExit>,
    mut ended: bevy::prelude::Local<bool>,
) {
    let Some(reason) = device_lost().filter(|_| !*ended) else {
        return;
    };
    *ended = true;
    bevy::log::error!("the render device was lost, so the game ends");
    if let Some(folder) = folder
        && !folder.0.join(CRASH).exists()
    {
        let facts = HOOKED
            .try_lock()
            .ok()
            .and_then(|hooked| hooked.as_ref().map(|(_, facts)| facts.clone()))
            .unwrap_or_else(facts);
        let thread = std::thread::current();
        if let Err(error) = write_crash(
            &folder.0,
            &lost_line(&reason),
            "the render device",
            thread.name().unwrap_or("unnamed"),
            "",
            &with_render_facts(&facts),
            crumbs().as_ref(),
        ) {
            bevy::log::error!("cannot write {}: {error}", folder.0.join(CRASH).display());
        }
    }
    tell_player();
    exit.write(bevy::app::AppExit::error());
}

/// `KJK_LOSE_DEVICE=N` (probes only): loses the render device on the render
/// world's Nth frame, as a driver that stops responding loses it. wgpu then
/// calls the callback at once and makes the device invalid. Here the
/// callback is called first, and destroying the device makes it invalid.
#[cfg(feature = "probes")]
fn probe_lose_device(
    device: bevy::prelude::Res<bevy::render::renderer::RenderDevice>,
    mut frames: bevy::prelude::Local<u32>,
) {
    let Some(at) = std::env::var("KJK_LOSE_DEVICE")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
    else {
        return;
    };
    *frames += 1;
    if *frames == at {
        bevy::log::warn!("KJK_LOSE_DEVICE: the render device is lost");
        on_device_lost("Unknown KJK_LOSE_DEVICE".into());
        device.wgpu_device().destroy();
    }
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
/// cut off. `{report}` is the key bound to the bug report.
pub const NOTICE: &str = "kjk crashed last time. Press {report} to create a bug report.";

/// Why the saves could not be backed up before this release touched them
/// (`saves::backup_on_new_version`): told on the main menu.
#[derive(Resource, Debug)]
pub struct BackupFailed(pub String);

/// On the main menu, tells why the game came back to it, when it was told
/// (`session_flow::back_to_menu_saying`, any run given `kjk --notice`);
/// and on a player's, of the last run that ended unexpectedly, once
/// (`unexpected_end`, then `SEEN`), of saves not backed up, and of a
/// display put back to a window (`display::recover`).
pub(crate) fn notice(
    mut commands: bevy::prelude::Commands,
    player: bevy::prelude::Res<PlayerRun>,
    folder: bevy::prelude::Res<console::RunFolder>,
    backup: Option<bevy::prelude::Res<BackupFailed>>,
    display: Option<bevy::prelude::Res<crate::display::DisplayReset>>,
    map: bevy::prelude::Res<crate::settings::KeyMap>,
    ui: Option<bevy::prelude::ResMut<crate::ui_view::Ui>>,
) {
    let Some(mut ui) = ui else {
        return;
    };
    let mut told: Vec<String> = crate::session_flow::take_notice().into_iter().collect();
    if player.0 {
        if let Some(previous) = unexpected_end(&crate::runtime::runs(), &folder.0) {
            mark(&previous, SEEN);
            let key = map.shown(crate::settings::Action::Report);
            let notice = if lost_device_in(&previous) {
                DRIVER_NOTICE
            } else {
                NOTICE
            };
            told.push(notice.replace("{report}", &String::from_utf8_lossy(&key)));
            commands.insert_resource(crate::report::CrashedRun(previous));
        }
        if let Some(backup) = backup {
            told.push(format!("The saves could not be backed up: {}", backup.0));
        }
        if let Some(display) = display {
            told.push(display.notice());
        }
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
    let mut crashed: Vec<PathBuf> = folders[..old]
        .iter()
        .filter(|folder| recent(folder))
        .cloned()
        .collect();
    crashed.drain(..crashed.len().saturating_sub(CRASHES_KEPT));
    crashed.extend(folders.drain(old..));
    crashed
}

#[cfg(feature = "probes")]
mod probes;
#[cfg(feature = "probes")]
pub use probes::probes;
