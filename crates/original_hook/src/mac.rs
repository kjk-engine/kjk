//! The Mac ports. kjk loads the library with `DYLD_INSERT_LIBRARIES`, and
//! dyld's interposing hands the library the game's calls to `glClear`.

use std::sync::OnceLock;

use original_find::Bitness;

use crate::game::{Calls, Found, Image};

#[link(name = "OpenGL", kind = "framework")]
unsafe extern "C" {
    fn glClear(mask: u32);
}

unsafe extern "C" {
    fn _dyld_get_image_header(index: u32) -> *const u8;
    fn getsectiondata(
        header: *const u8,
        segment: *const std::ffi::c_char,
        section: *const std::ffi::c_char,
        size: *mut std::ffi::c_ulong,
    ) -> *const u8;
}

/// A pair of dyld's `__interpose` section. dyld sends the calls to
/// `original` from every other image to `replacement`. It leaves the calls
/// from this library alone, so `gl_clear` still reaches the real `glClear`.
#[repr(C)]
struct Interpose {
    replacement: *const (),
    original: *const (),
}

// SAFETY: The pair holds two function addresses and is never written.
unsafe impl Sync for Interpose {}

#[used]
#[unsafe(link_section = "__DATA,__interpose")]
static GL_CLEAR: Interpose = Interpose {
    replacement: gl_clear as *const (),
    original: glClear as *const (),
};

static FOUND: OnceLock<Option<Found>> = OnceLock::new();

struct Mac;

impl Calls for Mac {
    unsafe fn populate_modules(&self, found: &Found, menu: *mut u8) {
        // SAFETY: The finder found this function in the game's code, and it
        // takes the menu as its only argument.
        unsafe {
            let call: extern "C" fn(*mut u8) = std::mem::transmute(found.populate_modules as usize);
            call(menu);
        }
    }

    unsafe fn on_module_picked(&self, found: &Found, menu: *mut u8, control: *mut u8) {
        // SAFETY: The finder found this function in the game's code, and it
        // takes the menu and a control.
        unsafe {
            let call: extern "C" fn(*mut u8, *mut u8) =
                std::mem::transmute(found.on_module_picked as usize);
            call(menu, control);
        }
    }
}

extern "C" fn gl_clear(mask: u32) {
    if let Some(found) = FOUND.get_or_init(game_code) {
        // SAFETY: `found` was found in the game's code, and the game calls
        // `glClear` from its main thread.
        unsafe { crate::game::frame(found, &Mac) };
    }
    // SAFETY: This is OpenGL's own function, called with the game's
    // argument.
    unsafe { glClear(mask) }
}

/// The warp list's code, found in the game's executable. The executable is
/// the process's first image. Its code is in `__TEXT,__text` and its
/// strings are in `__TEXT,__cstring`.
fn game_code() -> Option<Found> {
    // SAFETY: dyld gives the first image's header and its sections, which
    // stay mapped while the game runs.
    let section = |name: &std::ffi::CStr| unsafe {
        let header = _dyld_get_image_header(0);
        let mut size = 0;
        let at = getsectiondata(header, c"__TEXT".as_ptr(), name.as_ptr(), &mut size);
        (!at.is_null()).then(|| (std::slice::from_raw_parts(at, size as usize), at as u64))
    };
    let (Some((code, code_at)), Some(strings)) = (section(c"__text"), section(c"__cstring")) else {
        crate::game::write_log(
            "the game's executable has no code or strings section, so the library does nothing",
        );
        return None;
    };
    let image = Image {
        code,
        code_at,
        data: vec![strings],
        bitness: Bitness::Bits64,
    };
    match crate::game::find(&image) {
        Ok(found) => Some(found),
        Err(missing) => {
            crate::game::write_log(&format!(
                "the warp list's code was not found ({missing}), so the library does nothing"
            ));
            None
        }
    }
}
