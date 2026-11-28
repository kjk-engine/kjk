//! ReShade, the post-processing tool some players run over kjk on Windows
//! (`site/faq.html`).
//!
//! kjk registers itself with ReShade as an add-on, so that ReShade tells it
//! when ReShade's overlay opens and closes. While the overlay is open, the
//! mouse belongs to the overlay. kjk then ignores the mouse and lets the
//! cursor go (`drop_mouse`, `sim_host::grab_cursor`).
//!
//! While ReShade is loaded, the cameras that draw over the world (the
//! loading screen, the HUD, the screens, a movie and the fade) load the
//! window's depth texture instead of clearing it (`overlay_camera`). The
//! world's depth is then still in that texture when the frame is presented,
//! which is where ReShade's depth effects read it.
//!
//! Without ReShade, nothing here changes anything. On Windows kjk looks once
//! for ReShade's module in its own process and finds none. On other systems
//! there is no ReShade to look for.
//!
//! The numbers and signatures of ReShade's add-on API below were copied by
//! hand from ReShade's headers `include/reshade.hpp` and
//! `include/reshade_events.hpp` (crosire/reshade, BSD 3-Clause or MIT), at
//! the tags v6.0.0 and v6.8.0.

use std::sync::atomic::{AtomicBool, Ordering};

use bevy::camera::Camera3dDepthLoadOp;
use bevy::ecs::message::Messages;
use bevy::input::mouse::{
    AccumulatedMouseMotion, AccumulatedMouseScroll, MouseButtonInput, MouseMotion, MouseWheel,
};
use bevy::prelude::*;
use bevy::window::CursorMoved;

use crate::temporal::{Antialias, Scale, Temporal};

/// Finds ReShade once the render device exists, and registers with it.
pub struct ReshadePlugin {
    /// The run has a window. A headless run never looks for ReShade.
    pub window: bool,
}

/// What kjk found out about ReShade at startup.
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct Reshade {
    /// ReShade's module is loaded in kjk's process.
    pub loaded: bool,
    /// ReShade accepted kjk as an add-on and tells it about the overlay.
    pub registered: bool,
    /// The overlay counts as open for the whole run
    /// (`KJK_RESHADE_FAKE_OVERLAY=1`), to try this module without ReShade.
    pub fake_overlay: bool,
}

/// Whether ReShade's overlay is open this frame.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Overlay(pub bool);

/// Written by ReShade's callback on the thread that presents the frame
/// (Bevy's render thread), read once a frame by `sync_overlay`.
static OPEN: AtomicBool = AtomicBool::new(false);

/// The overlay cameras load the window's depth texture (`overlay_camera`).
/// Set once, before the first frame (`ReshadePlugin::finish`).
static KEEPS_DEPTH: AtomicBool = AtomicBool::new(false);

impl Plugin for ReshadePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Reshade>()
            .init_resource::<Overlay>()
            .add_systems(
                PreUpdate,
                (sync_overlay, drop_mouse.run_if(overlay_open))
                    .chain()
                    .after(bevy::input::InputSystems)
                    .before(crate::swallow::hold_up),
            )
            .add_systems(Startup, note_fact)
            .add_systems(
                Update,
                note_depth_limits.run_if(resource_exists_and_changed::<Temporal>),
            )
            .add_systems(Last, unregister_at_exit);
    }

    /// Runs after Bevy's render plugin has made the render device. ReShade's
    /// Vulkan layer is loaded into the process when the Vulkan instance is
    /// made, so this is the first moment it can be found.
    fn finish(&self, app: &mut App) {
        let loaded = self.window && platform::find();
        let registered = loaded && platform::register();
        let env = |name: &str| std::env::var(name).ok();
        let depth = keeps_depth_choice(env("KJK_RESHADE_DEPTH").as_deref(), loaded);
        KEEPS_DEPTH.store(depth, Ordering::Relaxed);
        if loaded {
            info!(registered, keeps_depth = depth, "ReShade is loaded");
        } else {
            debug!("ReShade is not loaded");
        }
        app.insert_resource(Reshade {
            loaded,
            registered,
            fake_overlay: env("KJK_RESHADE_FAKE_OVERLAY").as_deref() == Some("1"),
        });
    }
}

/// Whether the overlay cameras keep the world's depth: when ReShade is
/// loaded, unless `KJK_RESHADE_DEPTH` says otherwise (`1` on, `0` off).
pub(crate) fn keeps_depth_choice(env: Option<&str>, loaded: bool) -> bool {
    match env {
        Some("1") => true,
        Some("0") => false,
        _ => loaded,
    }
}

/// The `Camera3d` of a camera that draws over the world on the window. It
/// clears the depth texture it shares with the world's camera, as Bevy's
/// default does, unless ReShade is loaded. Then it loads that texture, so the
/// world's depth stays in it until the frame is presented. The overlays draw
/// only blended quads, which neither test nor write depth
/// (`OdysseyMaterial::on_top`), so they look the same either way.
pub(crate) fn overlay_camera() -> Camera3d {
    if KEEPS_DEPTH.load(Ordering::Relaxed) {
        Camera3d {
            depth_load_op: Camera3dDepthLoadOp::Load,
            ..default()
        }
    } else {
        Camera3d::default()
    }
}

/// Copies the overlay's state from ReShade's callback into `Overlay`.
fn sync_overlay(reshade: Res<Reshade>, mut overlay: ResMut<Overlay>) {
    let open = reshade.fake_overlay || OPEN.load(Ordering::Relaxed);
    if overlay.set_if_neq(Overlay(open)) {
        debug!(open, "ReShade's overlay");
    }
}

fn overlay_open(overlay: Res<Overlay>) -> bool {
    overlay.0
}

/// Removes the frame's mouse input while the overlay is open: the motion,
/// the scroll, the buttons, and the messages they were made from. Every
/// reader of the mouse in kjk reads one of these, after this system.
pub(crate) fn drop_mouse(
    motion: Option<ResMut<AccumulatedMouseMotion>>,
    scroll: Option<ResMut<AccumulatedMouseScroll>>,
    buttons: Option<ResMut<ButtonInput<MouseButton>>>,
    moved: Option<ResMut<Messages<MouseMotion>>>,
    wheel: Option<ResMut<Messages<MouseWheel>>>,
    pressed: Option<ResMut<Messages<MouseButtonInput>>>,
    cursor: Option<ResMut<Messages<CursorMoved>>>,
) {
    if let Some(mut motion) = motion {
        motion.delta = Vec2::ZERO;
    }
    if let Some(mut scroll) = scroll {
        scroll.delta = Vec2::ZERO;
    }
    if let Some(mut buttons) = buttons {
        buttons.reset_all();
    }
    if let Some(mut messages) = moved {
        messages.clear();
    }
    if let Some(mut messages) = wheel {
        messages.clear();
    }
    if let Some(mut messages) = pressed {
        messages.clear();
    }
    if let Some(mut messages) = cursor {
        messages.clear();
    }
}

/// What ReShade's overlay does to the cursor this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverlayCursor {
    /// The overlay is open. The cursor is free and shown, and play changes
    /// nothing about it.
    Free,
    /// The overlay closed this frame. Play decides the cursor again from
    /// scratch, as if it had never decided before.
    Decide,
    /// The overlay is closed and was closed the frame before. Play goes on
    /// as it was.
    Leave,
}

/// The overlay's effect on the cursor (`OverlayCursor`). `was_open` is
/// whether the overlay was open the frame before, and is set to `open` for
/// the next frame.
pub(crate) fn overlay_cursor(open: bool, was_open: &mut bool) -> OverlayCursor {
    match (open, std::mem::replace(was_open, open)) {
        (true, _) => OverlayCursor::Free,
        (false, true) => OverlayCursor::Decide,
        (false, false) => OverlayCursor::Leave,
    }
}

/// Adds "ReShade loaded" to the run's facts, next to the render adapter.
fn note_fact(reshade: Res<Reshade>, facts: Option<ResMut<console::DumpFacts>>) {
    if let Some(mut facts) = facts {
        facts.0.insert(
            "reshade".into(),
            serde_json::json!({
                "loaded": reshade.loaded,
                "registered": reshade.registered,
                "keeps_depth": KEEPS_DEPTH.load(Ordering::Relaxed),
            }),
        );
    }
}

/// Why ReShade's depth effects cannot use the world's depth under `used`,
/// or `None` when they can.
pub(crate) fn depth_limit(used: Antialias) -> Option<&'static str> {
    match used {
        Antialias::Msaa(samples) if samples > 1 => Some(
            "ReShade cannot read a multisampled depth texture. Set Anti-aliasing to TAA, DLAA \
             or Off for ReShade's depth effects.",
        ),
        Antialias::Upscale(_, scale) if scale != Scale::Full => Some(
            "The world is drawn smaller than the window, so its depth does not line up with \
             the picture. Set Anti-aliasing to TAA, DLAA or Off for ReShade's depth effects.",
        ),
        _ => None,
    }
}

/// One log line, at startup and when the anti-aliasing changes, when ReShade
/// is loaded and its depth effects cannot work with the choice.
fn note_depth_limits(reshade: Res<Reshade>, temporal: Res<Temporal>) {
    if !reshade.loaded {
        return;
    }
    if let Some(why) = depth_limit(temporal.used) {
        info!(antialias = ?temporal.used, "ReShade depth: {why}");
    }
}

/// Unregisters from ReShade when the app exits, while ReShade's module is
/// still loaded.
fn unregister_at_exit(mut exits: MessageReader<AppExit>, mut reshade: ResMut<Reshade>) {
    if exits.read().count() > 0 && reshade.registered {
        platform::unregister();
        reshade.registered = false;
    }
}

#[cfg(windows)]
mod platform {
    //! ReShade's add-on API, called through the functions ReShade's module
    //! exports. All of this module's `unsafe` code is here.

    use std::ffi::{CStr, c_void};
    use std::sync::atomic::{AtomicUsize, Ordering};

    use windows_sys::Win32::Foundation::{FARPROC, HMODULE};
    use windows_sys::Win32::System::LibraryLoader::{
        GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
        GetModuleHandleExW, GetProcAddress,
    };
    use windows_sys::Win32::System::ProcessStatus::K32EnumProcessModules;
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    /// The API version kjk asks for. 10 is `RESHADE_API_VERSION` in ReShade
    /// 6.0.0, the first version with the event `reshade_open_overlay`.
    /// ReShade refuses an add-on that asks for a newer version than its own,
    /// so ReShade 5 refuses kjk.
    const API_VERSION: u32 = 10;

    /// `addon_event::reshade_open_overlay`, the same number from ReShade
    /// 6.0.0 to 6.8.0.
    const OPEN_OVERLAY: u32 = 86;

    /// `bool ReShadeRegisterAddon(void *module, uint32_t api_version)`.
    type RegisterAddon = unsafe extern "C" fn(HMODULE, u32) -> bool;
    /// `void ReShadeUnregisterAddon(void *module)`.
    type UnregisterAddon = unsafe extern "C" fn(HMODULE);
    /// `void ReShadeRegisterEvent(addon_event ev, void *callback)`, and
    /// `ReShadeUnregisterEvent`, which has the same signature.
    type RegisterEvent = unsafe extern "C" fn(u32, *mut c_void);

    /// ReShade's module, once found (0 until then).
    static RESHADE: AtomicUsize = AtomicUsize::new(0);
    /// kjk's own module, as registered with ReShade (0 until then).
    static OWN: AtomicUsize = AtomicUsize::new(0);

    /// The callback of `reshade_open_overlay`:
    /// `bool (api::effect_runtime *runtime, bool open, api::input_source source)`.
    /// It records whether the overlay is opening, and returns false, which
    /// lets the overlay open or close.
    extern "C" fn on_open_overlay(_runtime: *mut c_void, open: bool, _source: i32) -> bool {
        super::OPEN.store(open, Ordering::Relaxed);
        false
    }

    /// Looks for ReShade's module among the process's modules, as ReShade's
    /// own header does: the first module that exports both
    /// `ReShadeRegisterAddon` and `ReShadeUnregisterAddon`. ReShade's file
    /// may have any name (`ReShade64.dll`, `dxgi.dll`, …), so the name is
    /// not used.
    pub(super) fn find() -> bool {
        let mut modules: [HMODULE; 1024] = [std::ptr::null_mut(); 1024];
        let mut needed = 0u32;
        let size = std::mem::size_of_val(&modules) as u32;
        // SAFETY: `modules` has room for `size` bytes, and `needed` is a
        // valid place for the byte count.
        let listed = unsafe {
            K32EnumProcessModules(GetCurrentProcess(), modules.as_mut_ptr(), size, &mut needed)
        };
        if listed == 0 {
            return false;
        }
        let count = (needed.min(size) as usize) / std::mem::size_of::<HMODULE>();
        let found = modules[..count].iter().copied().find(|&module| {
            symbol(module, c"ReShadeRegisterAddon").is_some()
                && symbol(module, c"ReShadeUnregisterAddon").is_some()
        });
        if let Some(module) = found {
            RESHADE.store(module as usize, Ordering::Relaxed);
        }
        found.is_some()
    }

    /// The function `name` that `module` exports.
    fn symbol(module: HMODULE, name: &CStr) -> FARPROC {
        // SAFETY: `module` is a module handle the system listed, and `name`
        // ends in a zero byte.
        unsafe { GetProcAddress(module, name.as_ptr().cast()) }
    }

    /// The function `name` that ReShade's module exports, as type `F`.
    ///
    /// # Safety
    ///
    /// `F` must be an `extern "C"` function pointer type that matches the
    /// function's signature in ReShade's header.
    unsafe fn reshade_fn<F: Copy>(name: &CStr) -> Option<F> {
        let module = RESHADE.load(Ordering::Relaxed) as HMODULE;
        if module.is_null() {
            return None;
        }
        // SAFETY: the caller promises `F` is the function's own type, and a
        // function pointer has the size of `FARPROC`'s.
        symbol(module, name).map(|function| unsafe { std::mem::transmute_copy(&function) })
    }

    /// The module this code is in (kjk's executable), the one ReShade finds
    /// from the callback's address when it registers the callback.
    fn own_module() -> HMODULE {
        let mut module: HMODULE = std::ptr::null_mut();
        // SAFETY: with these flags the second argument is an address in the
        // module, and `module` is a valid place for the handle.
        unsafe {
            GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS
                    | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                on_open_overlay as *const c_void as *const u16,
                &mut module,
            );
        }
        module
    }

    /// Registers kjk with ReShade and listens for the overlay. Whether
    /// ReShade accepted kjk.
    pub(super) fn register() -> bool {
        // SAFETY: the types are the signatures in ReShade's header.
        let (register_addon, register_event) = unsafe {
            (
                reshade_fn::<RegisterAddon>(c"ReShadeRegisterAddon"),
                reshade_fn::<RegisterEvent>(c"ReShadeRegisterEvent"),
            )
        };
        let (Some(register_addon), Some(register_event)) = (register_addon, register_event) else {
            return false;
        };
        let own = own_module();
        if own.is_null() {
            return false;
        }
        // SAFETY: ReShade's function, called with a module handle and a
        // version number, as its header does.
        if !unsafe { register_addon(own, API_VERSION) } {
            bevy::log::info!(
                "ReShade refused kjk as an add-on. ReShade 6.0 or newer is needed for kjk to \
                 hear when ReShade's overlay opens."
            );
            return false;
        }
        OWN.store(own as usize, Ordering::Relaxed);
        // SAFETY: the callback has the event's signature, and lives as long
        // as the process.
        unsafe { register_event(OPEN_OVERLAY, on_open_overlay as *mut c_void) };
        bevy::log::info!("kjk is registered with ReShade as an add-on");
        true
    }

    /// Stops listening for the overlay and unregisters kjk from ReShade.
    pub(super) fn unregister() {
        let own = OWN.swap(0, Ordering::Relaxed) as HMODULE;
        if own.is_null() {
            return;
        }
        // SAFETY: the types are the signatures in ReShade's header.
        let (unregister_event, unregister_addon) = unsafe {
            (
                reshade_fn::<RegisterEvent>(c"ReShadeUnregisterEvent"),
                reshade_fn::<UnregisterAddon>(c"ReShadeUnregisterAddon"),
            )
        };
        // SAFETY: ReShade's functions, called with the callback and the
        // module registered before.
        unsafe {
            if let Some(unregister_event) = unregister_event {
                unregister_event(OPEN_OVERLAY, on_open_overlay as *mut c_void);
            }
            if let Some(unregister_addon) = unregister_addon {
                unregister_addon(own);
            }
        }
        super::OPEN.store(false, Ordering::Relaxed);
    }
}

#[cfg(not(windows))]
mod platform {
    //! ReShade runs only on Windows.

    pub(super) fn find() -> bool {
        false
    }

    pub(super) fn register() -> bool {
        false
    }

    pub(super) fn unregister() {}
}

#[cfg(feature = "probes")]
#[path = "reshade_probes.rs"]
mod probes;

#[cfg(feature = "probes")]
pub use probes::probes;
