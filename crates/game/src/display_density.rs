//! How many pixels to the inch the owner's display has, from what the
//! operating system knows of its physical size. The catalogue's "as seen"
//! mode uses it (docs/UI.md, "The catalogue in the game"). A setting
//! overrides it, and the catalogue falls back on a default when the system
//! reports nothing.

use winit::window::Window;

/// The density of the monitor `window` is on, in physical pixels to the
/// inch, or of the main display when there is no window. None where the
/// system does not say.
pub fn of(window: Option<&Window>) -> Option<f32> {
    let density = read(window)?;
    (density.is_finite() && density > 1.0).then_some(density)
}

#[cfg(target_os = "macos")]
fn read(window: Option<&Window>) -> Option<f32> {
    use winit::platform::macos::MonitorHandleExtMacOS;

    #[repr(C)]
    struct Size {
        width: f64,
        height: f64,
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGMainDisplayID() -> u32;
        fn CGDisplayScreenSize(display: u32) -> Size;
        fn CGDisplayCopyDisplayMode(display: u32) -> *mut std::ffi::c_void;
        fn CGDisplayModeGetPixelWidth(mode: *mut std::ffi::c_void) -> usize;
        fn CGDisplayModeRelease(mode: *mut std::ffi::c_void);
    }

    let display = match window {
        Some(window) => window.current_monitor()?.native_id(),
        // SAFETY: a plain query with no arguments.
        None => unsafe { CGMainDisplayID() },
    };
    // SAFETY: `display` is an id the system gave. The mode is released
    // after its width is read.
    let (millimetres, pixels) = unsafe {
        let millimetres = CGDisplayScreenSize(display).width;
        let mode = CGDisplayCopyDisplayMode(display);
        if mode.is_null() {
            return None;
        }
        let pixels = CGDisplayModeGetPixelWidth(mode);
        CGDisplayModeRelease(mode);
        (millimetres, pixels)
    };
    (millimetres > 0.0).then(|| (pixels as f64 / (millimetres / 25.4)) as f32)
}

#[cfg(windows)]
fn read(window: Option<&Window>) -> Option<f32> {
    use winit::platform::windows::MonitorHandleExtWindows;

    #[link(name = "shcore")]
    unsafe extern "system" {
        fn GetDpiForMonitor(monitor: isize, kind: i32, x: *mut u32, y: *mut u32) -> i32;
    }
    /// `MDT_RAW_DPI`: the monitor's own size, not the desktop's scaling.
    const RAW_DPI: i32 = 2;

    let monitor = window?.current_monitor()?.hmonitor();
    let (mut x, mut y) = (0u32, 0u32);
    // SAFETY: the pointers are to two live integers.
    let result = unsafe { GetDpiForMonitor(monitor, RAW_DPI, &mut x, &mut y) };
    (result == 0 && x > 0).then_some(x as f32)
}

#[cfg(not(any(target_os = "macos", windows)))]
fn read(_: Option<&Window>) -> Option<f32> {
    None
}
