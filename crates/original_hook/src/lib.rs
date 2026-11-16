//! The library `kjk original` loads into the original KOTOR 1 or KOTOR 2
//! (the launcher's `original_verbs`). It starts the module that
//! `KJK_ORIGINAL_MODULE` names from the main menu, without a click, the way
//! the developers' hidden warp list does. It fills the menu's module list
//! (`PopulateModules`), picks the module in it, and calls the menu's
//! `OnModulePicked`. The game then starts a session in that module with its
//! default player, named "test".
//!
//! The library acts on the game's own thread, once a frame, from the game's
//! call to OpenGL's `glClear`, which it takes over. On macOS dyld's
//! interposing hands it the call (`mac`). On Windows the library rewrites the
//! game's import table in memory (`windows`). The game's files are never
//! changed.
//!
//! No address or offset of a game is written in kjk. The library finds the
//! functions it calls and the fields it touches in the game's own code, each
//! run (`find`). When something is not found, the library does nothing, and
//! its log says what was missing. `KJK_ORIGINAL_LOG` names the log's file.

#![cfg_attr(not(any(mac_port, windows_port)), allow(dead_code))]

mod find;
#[cfg(mac_port)]
mod mac;
#[cfg(windows_port)]
mod windows;

use std::io::Write;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use find::Found;

/// The frames the menu is drawn before the module is picked. The warp list
/// waits for the menu's first frame. The extra frames let its music and
/// its 3D view settle first.
const FRAMES_BEFORE_PICKING: u32 = 30;

static LOG: Mutex<Option<std::fs::File>> = Mutex::new(None);
static FRAMES: AtomicU32 = AtomicU32::new(0);
static DONE: AtomicBool = AtomicBool::new(false);

/// A line in the log, when there is one.
fn say(line: &str) {
    let mut log = LOG.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if log.is_none() {
        *log =
            std::env::var_os("KJK_ORIGINAL_LOG").and_then(|path| std::fs::File::create(path).ok());
    }
    if let Some(file) = log.as_mut() {
        let _ = writeln!(file, "{line}");
    }
}

/// The module to start, from `KJK_ORIGINAL_MODULE`.
fn target() -> Option<String> {
    std::env::var("KJK_ORIGINAL_MODULE")
        .ok()
        .filter(|module| !module.is_empty())
}

/// The calls into the game, which each port makes in its own calling
/// convention.
trait Calls {
    /// Calls `PopulateModules` on `menu`.
    unsafe fn populate_modules(&self, found: &Found, menu: *mut u8);
    /// Calls `OnModulePicked` on `menu` with `control`.
    unsafe fn on_module_picked(&self, found: &Found, menu: *mut u8, control: *mut u8);
}

/// The main menu, once the game has made one.
///
/// # Safety
/// `found` must be the game's.
unsafe fn main_menu(found: &Found) -> Option<*mut u8> {
    let mut at = found.app_manager as usize as *const usize;
    for offset in found.menu_path {
        // SAFETY: every pointer on the path is the game's own, read where
        // the game keeps it, and checked before it is followed.
        let pointer = unsafe { at.read() };
        if pointer == 0 {
            return None;
        }
        at = (pointer + offset as usize) as *const usize;
    }
    // SAFETY: as above.
    let menu = unsafe { at.read() } as *mut u8;
    (!menu.is_null()).then_some(menu)
}

/// The library's part of a frame: once the main menu has been drawn for
/// `FRAMES_BEFORE_PICKING` frames, the target module is picked, once.
///
/// # Safety
/// `found` must be the game's, and this must run on its main thread.
unsafe fn frame(found: &Found, calls: &impl Calls) {
    if DONE.load(Ordering::Relaxed) {
        return;
    }
    // SAFETY: the caller's.
    let Some(menu) = (unsafe { main_menu(found) }) else {
        return;
    };
    // SAFETY: `drawn` is a field of the menu.
    let drawn = unsafe {
        menu.add(found.drawn as usize)
            .cast::<u32>()
            .read_unaligned()
    } != 0;
    if !drawn || FRAMES.fetch_add(1, Ordering::Relaxed) + 1 < FRAMES_BEFORE_PICKING {
        return;
    }
    DONE.store(true, Ordering::Relaxed);
    match target() {
        // SAFETY: the caller's, and the menu is the game's.
        Some(module) => unsafe { pick(found, calls, menu, &module) },
        None => say("KJK_ORIGINAL_MODULE names no module, so the main menu stays"),
    }
}

/// Fills `menu`'s module list, picks `module` in it and starts it.
///
/// # Safety
/// As `frame`'s.
unsafe fn pick(found: &Found, calls: &impl Calls, menu: *mut u8, module: &str) {
    // SAFETY: the caller's.
    unsafe { calls.populate_modules(found, menu) };
    // A `CExoString` is a pointer to its text and a 32-bit length, padded
    // to two pointers.
    let stride = 2 * size_of::<usize>();
    // SAFETY: `modules` is the menu's array of names, a pointer and a count.
    let (names, count) = unsafe {
        let field = menu.add(found.modules as usize);
        (
            field.cast::<*const u8>().read_unaligned(),
            field.add(size_of::<usize>()).cast::<i32>().read_unaligned(),
        )
    };
    say(&format!("the menu lists {count} modules"));
    let entry = (0..count.max(0) as usize).find(|index| {
        // SAFETY: each name is a `CExoString` of the array, its text ended
        // by a zero byte.
        let text = unsafe {
            names
                .add(index * stride)
                .cast::<*const std::ffi::c_char>()
                .read_unaligned()
        };
        !text.is_null()
            && unsafe { std::ffi::CStr::from_ptr(text) }
                .to_string_lossy()
                .eq_ignore_ascii_case(module)
    });
    let Some(index) = entry else {
        say(&format!("{module} is not in the menu's list of modules"));
        return;
    };
    say(&format!("picking {module}, the list's entry {index}"));
    // A stand-in for the picked button. `OnModulePicked` only checks that
    // one of its fields is set.
    let control = vec![0u8; found.control_check as usize + 8]
        .leak()
        .as_mut_ptr();
    // SAFETY: `picked` is the menu's 16-bit index, and the control's field
    // lies inside the stand-in.
    unsafe {
        menu.add(found.picked as usize)
            .cast::<i16>()
            .write_unaligned(index as i16);
        control
            .add(found.control_check as usize)
            .cast::<u32>()
            .write_unaligned(1);
        calls.on_module_picked(found, menu, control);
    }
    say("the game is starting the module");
}
