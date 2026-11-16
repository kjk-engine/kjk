//! The Windows builds. `kjk-inject` loads the library into the game. The
//! library takes over the game's calls to `glClear` by rewriting the game's
//! import table in memory.
//!
//! It does this from a thread of its own, once the warp list's code is
//! found. A build wrapped in Steam's DRM decrypts its code after the library
//! is loaded, so the thread looks again each second, for a minute at most.

use std::ffi::c_void;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use windows_sys::Win32::Foundation::HINSTANCE;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Memory::{PAGE_READWRITE, VirtualProtect};
use windows_sys::Win32::System::SystemServices::DLL_PROCESS_ATTACH;
use windows_sys::Win32::System::Threading::CreateThread;

use original_find::Bitness;

use crate::game::{Calls, Found, Image, write_log};

/// How many times the thread looks for the warp list's code, once a second.
const TRIES: u32 = 60;

/// The real `glClear`, taken from the import table.
static REAL_GL_CLEAR: AtomicUsize = AtomicUsize::new(0);
static FOUND: OnceLock<Found> = OnceLock::new();

/// The game's functions are member functions built with MSVC. They take
/// their object in `ecx`, which is the `thiscall` convention.
struct Windows;

impl Calls for Windows {
    unsafe fn populate_modules(&self, found: &Found, menu: *mut u8) {
        // SAFETY: The finder found this member function in the game's code,
        // and it takes the menu as its object.
        unsafe {
            let call: extern "thiscall" fn(*mut u8) =
                std::mem::transmute(found.populate_modules as usize);
            call(menu);
        }
    }

    unsafe fn on_module_picked(&self, found: &Found, menu: *mut u8, control: *mut u8) {
        // SAFETY: The finder found this member function in the game's code,
        // and it takes the menu as its object and a control.
        unsafe {
            let call: extern "thiscall" fn(*mut u8, *mut u8) =
                std::mem::transmute(found.on_module_picked as usize);
            call(menu, control);
        }
    }
}

#[unsafe(no_mangle)]
extern "system" fn DllMain(_: HINSTANCE, reason: u32, _: *mut c_void) -> i32 {
    if reason == DLL_PROCESS_ATTACH {
        // SAFETY: The new thread only starts once the loader lets go of its
        // lock, so it may load and call what it needs.
        unsafe {
            CreateThread(
                std::ptr::null(),
                0,
                Some(prepare),
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
            )
        };
    }
    1
}

/// The game's base address, which is also where its PE header starts.
fn image() -> *const u8 {
    // SAFETY: A null name asks for the process's own executable.
    unsafe { GetModuleHandleW(std::ptr::null()) as *const u8 }
}

/// The 32-bit number at `offset` in the game's image.
///
/// # Safety
/// `offset` must lie inside the image's headers or sections.
unsafe fn word(offset: usize) -> u32 {
    // SAFETY: The caller guarantees that `offset` lies inside the image.
    unsafe { image().add(offset).cast::<u32>().read_unaligned() }
}

/// The game's sections as they are in memory. The code is in `.text`, and
/// the strings are in `.rdata` and `.data`.
fn game_image() -> Image<'static> {
    let base = image();
    let mut code: &[u8] = &[];
    let mut code_at = 0;
    let mut data = Vec::new();
    // SAFETY: The PE header and its table of sections lie at the offsets the
    // PE format fixes, and the sections they describe stay mapped while the
    // game runs.
    unsafe {
        let header = word(0x3c) as usize;
        let sections = base.add(header + 6).cast::<u16>().read_unaligned() as usize;
        let optional = base.add(header + 20).cast::<u16>().read_unaligned() as usize;
        for index in 0..sections {
            let entry = header + 24 + optional + 40 * index;
            let name = std::slice::from_raw_parts(base.add(entry), 8);
            let size = word(entry + 8) as usize;
            let at = word(entry + 12) as usize;
            let bytes = std::slice::from_raw_parts(base.add(at), size);
            let address = base as u64 + at as u64;
            match name {
                b".text\0\0\0" => (code, code_at) = (bytes, address),
                b".rdata\0\0" | b".data\0\0\0" => data.push((bytes, address)),
                _ => {}
            }
        }
    }
    Image {
        code,
        code_at,
        data,
        bitness: Bitness::Bits32,
    }
}

/// Finds the warp list's code once the game has decrypted it, then takes
/// over `glClear`.
unsafe extern "system" fn prepare(_: *mut c_void) -> u32 {
    let image = game_image();
    let mut missing = String::new();
    for _ in 0..TRIES {
        match crate::game::find(&image) {
            Ok(found) => {
                let _ = FOUND.set(found);
                // SAFETY: The game's imports were resolved before the
                // library was loaded.
                if unsafe { hook_import(b"opengl32.dll", b"glClear") } {
                    write_log("glClear is taken over");
                } else {
                    write_log("the game imports no glClear from opengl32.dll");
                }
                return 0;
            }
            Err(error) => missing = error,
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    write_log(&format!(
        "the warp list's code was not found in a minute ({missing}), so the library does nothing"
    ));
    0
}

extern "system" fn gl_clear(mask: u32) {
    if let Some(found) = FOUND.get() {
        // SAFETY: `found` was found in the game's code, and the game calls
        // `glClear` from its main thread.
        unsafe { crate::game::frame(found, &Windows) };
    }
    // SAFETY: This is the `glClear` the import table held before, called
    // with the game's argument.
    unsafe {
        let real: extern "system" fn(u32) =
            std::mem::transmute(REAL_GL_CLEAR.load(Ordering::Relaxed));
        real(mask);
    }
}

/// Points the game's import of `name` from `dll` at `gl_clear`. It returns
/// false when the game imports no such function.
///
/// # Safety
/// The image must be a 32-bit PE image with its imports resolved.
unsafe fn hook_import(dll: &[u8], name: &[u8]) -> bool {
    let base = image();
    // SAFETY: The import directory and the tables it points to lie inside
    // the image, and the caller guarantees they are resolved.
    unsafe {
        let header = word(0x3c) as usize;
        // The import directory is the second of the optional header's data
        // directories.
        let mut descriptor = word(header + 24 + 96 + 8) as usize;
        let text = |offset: usize| std::ffi::CStr::from_ptr(base.add(offset).cast());
        while word(descriptor + 12) != 0 {
            let names = word(descriptor) as usize;
            let slots = word(descriptor + 16) as usize;
            if text(word(descriptor + 12) as usize)
                .to_bytes()
                .eq_ignore_ascii_case(dll)
            {
                let mut index = 0;
                while word(names + 4 * index) != 0 {
                    let entry = word(names + 4 * index);
                    // A name's entry is a hint, 16 bits, then the name. An
                    // entry with its top bit set is an ordinal.
                    if entry & 0x8000_0000 == 0 && text(entry as usize + 2).to_bytes() == name {
                        let slot = base.add(slots + 4 * index) as *mut usize;
                        let mut old = 0;
                        VirtualProtect(slot.cast(), 4, PAGE_READWRITE, &mut old);
                        REAL_GL_CLEAR.store(slot.read(), Ordering::Relaxed);
                        slot.write(gl_clear as *const () as usize);
                        VirtualProtect(slot.cast(), 4, old, &mut old);
                        return true;
                    }
                    index += 1;
                }
            }
            descriptor += 20;
        }
    }
    false
}
