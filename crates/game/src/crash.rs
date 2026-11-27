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
    /// What the player aims at (`aimed`), kept while a screen is up.
    pub target: Option<String>,
    pub screen: Option<String>,
}

/// A target as the breadcrumbs keep it: its name as the HUD shows it
/// (with " (Empty)" on an emptied container), its tag and its distance.
pub fn aimed(name: &str, tag: &str, metres: f32) -> String {
    format!("{name} (tag {tag}, {metres:.1} m)")
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
        writeln!(out, "target {}", self.target.as_deref().unwrap_or("none"))?;
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
    target: None,
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
    native::install();
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
            if lost.is_some() {
                default(info);
                end_process();
            }
        }
        default(info);
    }));
}

/// Ends the process at once, once the render device is lost. The other
/// threads are not waited for. A lost device can make every later call into
/// the driver hang, for example after the computer wakes from sleep, and the
/// window would then stay on the screen and ignore all input. A dialog is not
/// shown, because it could open behind a borderless window. The main menu
/// tells the player what happened on the next start (`notice`).
fn end_process() -> ! {
    use std::io::Write;
    let _ = std::io::stderr().flush();
    std::process::exit(1)
}

/// What the main menu says once after a run that ended because the graphics
/// driver stopped responding. `{report}` is the key bound to the bug report.
pub const DRIVER_NOTICE: &str = "kjk closed last time because the graphics driver stopped responding. Updating the graphics driver may fix this. If it happens again, set Shadows to Low or Off in the options. Press {report} to create a bug report.";

/// How `crash.txt` begins when the render device was lost. The notice looks
/// for it.
const LOST_MARK: &str = "the graphics driver stopped responding (render device lost):";

/// The driver's reason, once the render device was lost (`watch_device`).
static LOST: Mutex<Option<String>> = Mutex::new(None);

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

/// Ends the game when the render device is lost. Bevy's own callback for a
/// lost device is replaced by `watch_device`, which writes the report and
/// ends the process before anything calls the driver again.
pub struct DeviceLostPlugin;

impl bevy::app::Plugin for DeviceLostPlugin {
    fn build(&self, _app: &mut bevy::app::App) {}

    fn finish(&self, app: &mut bevy::app::App) {
        if let Some(render_app) = app.get_sub_app_mut(bevy::render::RenderApp) {
            render_app.add_systems(bevy::render::RenderStartup, (watch_device, hold_queue));
            #[cfg(feature = "probes")]
            render_app.add_systems(bevy::render::Render, probe_lose_device);
        }
    }
}

/// Replaces Bevy's callback for a lost device, which it set as it made the
/// device. Bevy's callback logs the loss and quits at the next frame's
/// extract. The render systems still run before that, and they call into a
/// driver that may no longer answer. This one writes `crash.txt` and ends
/// the process at once. A device that the game destroys itself, as it quits,
/// is not a loss.
fn watch_device(device: bevy::prelude::Res<bevy::render::renderer::RenderDevice>) {
    device
        .wgpu_device()
        .set_device_lost_callback(|reason, text| {
            if reason == wgpu::DeviceLostReason::Destroyed {
                bevy::log::info!("the render device was destroyed: {text}");
                return;
            }
            on_device_lost(format!("{reason:?} {text}"));
        });
}

/// How long the game waits for the graphics card to finish its last frames
/// as it closes (`QueueAtExit`).
const EXIT_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

/// The render queue, held so that the game closes without a crash when the
/// graphics card has not finished its last frames.
///
/// As the render world is dropped, wgpu waits up to 6.3 seconds for the last
/// frame the game sent, and then panics if the frame is still not done. On
/// macOS this happens when the game closes while its window is hidden behind
/// another app: about two closes in five ended in that panic. The resource
/// holds its own handle to the queue. When it is dropped, it waits up to
/// `EXIT_WAIT` for the graphics card. If the card finishes, the handle is let
/// go and wgpu closes the queue as usual. If it does not, the handle is never
/// let go, so wgpu never closes the queue and never panics. The program ends
/// right after, and the system frees what the queue kept.
#[derive(bevy::prelude::Resource)]
struct QueueAtExit(Option<(wgpu::Device, wgpu::Queue)>);

fn hold_queue(
    mut commands: bevy::prelude::Commands,
    device: bevy::prelude::Res<bevy::render::renderer::RenderDevice>,
    queue: bevy::prelude::Res<bevy::render::renderer::RenderQueue>,
) {
    let held = (device.wgpu_device().clone(), (**queue.0).clone());
    commands.insert_resource(QueueAtExit(Some(held)));
}

impl Drop for QueueAtExit {
    fn drop(&mut self) {
        let Some((device, queue)) = self.0.take() else {
            return;
        };
        let waited = device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(EXIT_WAIT),
        });
        if let Err(error) = waited {
            bevy::log::warn!(
                "the graphics card did not finish its last frames within {} s as the game \
                 closed ({error}), so the game leaves the render queue open",
                EXIT_WAIT.as_secs()
            );
            std::mem::forget(queue);
        }
    }
}

fn on_device_lost(reason: String) -> ! {
    bevy::log::error!("Caught DeviceLost error: {reason}");
    bevy::log::error!("{}", crate::render_facts::text());
    lose_device(reason.clone());
    bevy::log::error!("the render device was lost, so the game ends");
    let hooked = HOOKED.try_lock().ok().and_then(|hooked| hooked.clone());
    if let Some((folder, facts)) = hooked
        && !folder.join(CRASH).exists()
    {
        let thread = std::thread::current();
        if let Err(error) = write_crash(
            &folder,
            &lost_line(&reason),
            "the render device",
            thread.name().unwrap_or("unnamed"),
            "",
            &with_render_facts(&facts),
            crumbs().as_ref(),
        ) {
            eprintln!("cannot write {}: {error}", folder.join(CRASH).display());
        }
    }
    end_process()
}

/// `facts` with the render facts of the last frames below them
/// (`render_facts`), for the crash of a lost render device.
pub fn with_render_facts(facts: &str) -> String {
    format!("{facts}{}", crate::render_facts::text())
}

/// What the player is told when the only graphics adapter that Vulkan,
/// DirectX 12 or Metal offered is a software renderer.
pub const SOFTWARE_ONLY: &str = "kjk could not find a graphics card that supports Vulkan or DirectX 12, so it has to close. The only graphics adapter it found is a software renderer, which draws on the processor and is far too slow to run the game. Updating the graphics driver may help. A graphics card with no Vulkan or DirectX 12 driver cannot run kjk.";

/// The variable that lets a run with a window draw on a software renderer
/// anyway (`refuses_adapter`).
const ALLOW_SOFTWARE: &str = "KJK_SOFTWARE_RENDERER";

/// Whether a run must refuse its render adapter. A run with a window
/// refuses a software renderer (`DeviceType::Cpu`), such as Windows'
/// WARP, which wgpu picks when no Vulkan or DirectX 12 driver reaches a
/// graphics card. It would show a black window for minutes. A headless
/// run draws on any adapter, because a test machine may have only a
/// software one. `KJK_SOFTWARE_RENDERER=1` lets a run with a window try.
pub(crate) fn refuses_adapter(device: wgpu::DeviceType, headless: bool, allowed: bool) -> bool {
    device == wgpu::DeviceType::Cpu && !headless && !allowed
}

/// Ends a run with a window whose render adapter is a software renderer
/// (`refuses_adapter`). The log says why, and a player's run shows
/// `SOFTWARE_ONLY` in a dialog.
pub fn refuse_software_adapter(
    adapter: bevy::prelude::Res<bevy::render::renderer::RenderAdapterInfo>,
    headless: bevy::prelude::Res<crate::scene::Headless>,
    player: Option<bevy::prelude::Res<PlayerRun>>,
    mut exit: bevy::prelude::MessageWriter<bevy::app::AppExit>,
) {
    let allowed = std::env::var(ALLOW_SOFTWARE).is_ok_and(|value| value == "1");
    if !refuses_adapter(adapter.device_type, headless.0, allowed) {
        return;
    }
    bevy::log::error!(
        adapter = %adapter.name,
        backend = ?adapter.backend,
        "the only graphics adapter found is a software renderer, so the game ends \
         ({ALLOW_SOFTWARE}=1 runs on it anyway)"
    );
    if player.is_some_and(|player| player.0) {
        rfd::MessageDialog::new()
            .set_title("kjk")
            .set_description(SOFTWARE_ONLY)
            .set_level(rfd::MessageLevel::Error)
            .show();
    }
    exit.write(bevy::app::AppExit::error());
}

/// `KJK_LOSE_DEVICE=N` (probes only): loses the render device on the render
/// world's Nth frame, as a driver that stops responding loses it: the
/// callback that wgpu calls then runs, and it ends the process.
#[cfg(feature = "probes")]
fn probe_lose_device(mut frames: bevy::prelude::Local<u32>) {
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

pub mod native;
#[cfg(feature = "probes")]
mod probes;
#[cfg(feature = "probes")]
pub use probes::probes;
