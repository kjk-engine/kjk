//! The library's work inside the game, shared by both ports.

#![cfg_attr(not(any(mac_port, windows_port)), allow(dead_code))]

use std::io::Write;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

pub(crate) use original_find::{Found, Image};

use crate::launch;

/// The frames the main menu is drawn before the module is picked. The warp
/// list only needs the menu's first frame. The extra frames let the menu's
/// music and its 3D view settle first.
const FRAMES_BEFORE_PICKING: u32 = 30;

static LOG: Mutex<Option<std::fs::File>> = Mutex::new(None);
static FRAMES: AtomicU32 = AtomicU32::new(0);
static DONE: AtomicBool = AtomicBool::new(false);

/// Writes `line` to the log file that kjk named, when it named one.
pub(crate) fn write_log(line: &str) {
    let mut log = LOG.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if log.is_none() {
        *log = std::env::var_os(launch::LOG_VAR).and_then(|path| std::fs::File::create(path).ok());
    }
    if let Some(file) = log.as_mut() {
        let _ = writeln!(file, "{line}");
    }
}

fn module_to_start() -> Option<String> {
    std::env::var(launch::MODULE_VAR)
        .ok()
        .filter(|module| !module.is_empty())
}

/// The game's functions that the library calls. Each port calls them in its
/// own calling convention.
pub(crate) trait Calls {
    /// Fills `menu`'s warp list.
    unsafe fn populate_modules(&self, found: &Found, menu: *mut u8);
    /// Starts the module picked in `menu`'s warp list, as a click on
    /// `control` would.
    unsafe fn on_module_picked(&self, found: &Found, menu: *mut u8, control: *mut u8);
}

/// The main menu, once the game has made one.
///
/// # Safety
/// `found` must have been found in this game's own code.
unsafe fn main_menu(found: &Found) -> Option<*mut u8> {
    let mut at = found.app_manager as usize as *const usize;
    for offset in found.menu_path {
        // SAFETY: Each pointer on the path is read where the game keeps it,
        // and it is checked for null before it is followed.
        let pointer = unsafe { at.read() };
        if pointer == 0 {
            return None;
        }
        at = (pointer + offset as usize) as *const usize;
    }
    // SAFETY: The last field on the path is the menu pointer, read where the
    // game keeps it.
    let menu = unsafe { at.read() } as *mut u8;
    (!menu.is_null()).then_some(menu)
}

/// The library's part of a frame. Once the main menu has been drawn for
/// `FRAMES_BEFORE_PICKING` frames, the module is picked, once.
///
/// # Safety
/// `found` must have been found in this game's own code, and this must run
/// on the game's main thread.
pub(crate) unsafe fn frame(found: &Found, calls: &impl Calls) {
    if DONE.load(Ordering::Relaxed) {
        return;
    }
    // SAFETY: The caller guarantees that `found` is this game's.
    let Some(menu) = (unsafe { main_menu(found) }) else {
        return;
    };
    // SAFETY: `drawn` is the offset of a 32-bit field inside the menu.
    let drawn = unsafe {
        menu.add(found.drawn as usize)
            .cast::<u32>()
            .read_unaligned()
    } != 0;
    if !drawn || FRAMES.fetch_add(1, Ordering::Relaxed) + 1 < FRAMES_BEFORE_PICKING {
        return;
    }
    DONE.store(true, Ordering::Relaxed);
    match module_to_start() {
        // SAFETY: The caller guarantees that `found` is this game's and that
        // this runs on its main thread, and `menu` is the game's live menu.
        Some(module) => unsafe { pick(found, calls, menu, &module) },
        None => write_log(&format!(
            "{} names no module, so the main menu stays",
            launch::MODULE_VAR
        )),
    }
}

/// Fills `menu`'s warp list, picks `module` in it and starts it.
///
/// # Safety
/// The same as `frame`'s, and `menu` must be the game's live main menu.
unsafe fn pick(found: &Found, calls: &impl Calls, menu: *mut u8, module: &str) {
    // SAFETY: The caller guarantees that `menu` is the game's main menu,
    // on its main thread.
    unsafe { calls.populate_modules(found, menu) };
    // The game's string type, `CExoString`, is a pointer to its text and a
    // 32-bit length, padded to the size of two pointers.
    let stride = 2 * size_of::<usize>();
    // SAFETY: `modules` is the offset of the menu's array of names, a
    // pointer followed by a 32-bit count.
    let (names, count) = unsafe {
        let field = menu.add(found.modules as usize);
        (
            field.cast::<*const u8>().read_unaligned(),
            field.add(size_of::<usize>()).cast::<i32>().read_unaligned(),
        )
    };
    write_log(&format!("the menu lists {count} modules"));
    let entry = (0..count.max(0) as usize).find(|index| {
        // SAFETY: The entry lies inside the array of `count` names that the
        // game has just filled.
        let text = unsafe {
            names
                .add(index * stride)
                .cast::<*const std::ffi::c_char>()
                .read_unaligned()
        };
        // SAFETY: A name's text is ended by a zero byte.
        !text.is_null()
            && unsafe { std::ffi::CStr::from_ptr(text) }
                .to_string_lossy()
                .eq_ignore_ascii_case(module)
    });
    let Some(index) = entry else {
        write_log(&format!("{module} is not in the menu's list of modules"));
        return;
    };
    write_log(&format!("picking {module}, the list's entry {index}"));
    // The click handler only checks that one field of the clicked control
    // is set, so a zeroed block with that field set stands in for it.
    let control = vec![0u8; found.control_check as usize + 8]
        .leak()
        .as_mut_ptr();
    // SAFETY: `picked` is the offset of the menu's 16-bit index, and the
    // control's field lies inside the block just made for it.
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
    write_log("the game is starting the module");
}

/// Finds the warp list's code in `image`, and logs what was found or what
/// was missing.
pub(crate) fn find(image: &Image) -> Result<Found, String> {
    let found = original_find::find(image);
    if let Ok(found) = &found {
        write_log(&format!("found the warp list's code: {found:x?}"));
    }
    found
}
