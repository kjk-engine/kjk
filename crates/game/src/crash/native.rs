//! A crash that is no panic: an access violation or a stack overflow,
//! inside kjk or inside a driver (issue 747). Such a crash ends the process
//! without the panic hook, so it left no `crash.txt` and the report showed
//! nothing. On Windows, an unhandled-exception filter now writes `crash.txt`
//! with the exception's code and name, the address, the module the address
//! is in (kjk itself or a driver's DLL), the render facts of the last frames
//! and the breadcrumbs. Windows then ends the process as before.

/// The name of a Windows exception code, as Windows' headers call it.
pub fn exception_name(code: u32) -> &'static str {
    match code {
        0xC000_0005 => "access violation",
        0xC000_00FD => "stack overflow",
        0xC000_001D => "illegal instruction",
        0xC000_0094 => "integer division by zero",
        0xC000_0096 => "privileged instruction",
        0xC000_0006 => "in-page error",
        0xC000_0409 => "stack buffer overrun (fail fast)",
        0xC000_0374 => "heap corruption",
        0xC000_0017 => "out of memory",
        0x8000_0003 => "breakpoint",
        _ => "unknown exception",
    }
}

/// The first line of `crash.txt` for a native crash.
pub fn native_line(code: u32, address: usize, module: Option<&str>) -> String {
    let module = module.unwrap_or("an unknown module");
    format!(
        "native crash: exception {code:#010X} ({}) at {address:#x} in {module}",
        exception_name(code)
    )
}

/// Installs the filter once. Other platforms keep their own crash reporters.
pub fn install() {
    #[cfg(windows)]
    windows::install();
}

#[cfg(windows)]
mod windows {
    use std::ffi::c_void;

    #[repr(C)]
    struct ExceptionRecord {
        code: u32,
        flags: u32,
        record: *mut ExceptionRecord,
        address: *mut c_void,
        parameters: u32,
        information: [usize; 15],
    }

    #[repr(C)]
    struct ExceptionPointers {
        record: *mut ExceptionRecord,
        context: *mut c_void,
    }

    type Filter = unsafe extern "system" fn(*const ExceptionPointers) -> i32;

    const CONTINUE_SEARCH: i32 = 0;
    const FROM_ADDRESS: u32 = 0x4;
    const UNCHANGED_REFCOUNT: u32 = 0x2;
    const STACK_OVERFLOW: u32 = 0xC000_00FD;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SetUnhandledExceptionFilter(filter: Option<Filter>) -> Option<Filter>;
        fn GetModuleHandleExW(flags: u32, name: *const u16, module: *mut *mut c_void) -> i32;
        fn GetModuleFileNameW(module: *mut c_void, name: *mut u16, size: u32) -> u32;
    }

    pub fn install() {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            // SAFETY: the filter is a plain function that lives as long as
            // the process.
            unsafe { SetUnhandledExceptionFilter(Some(filter)) };
        });
    }

    /// The file name of the module that holds `address`, if any.
    fn module_of(address: *mut c_void) -> Option<String> {
        let mut module = std::ptr::null_mut();
        // SAFETY: with FROM_ADDRESS the name is read as an address, and the
        // handle is not kept past this call.
        let found = unsafe {
            GetModuleHandleExW(
                FROM_ADDRESS | UNCHANGED_REFCOUNT,
                address.cast::<u16>(),
                &mut module,
            )
        };
        if found == 0 {
            return None;
        }
        let mut name = [0u16; 260];
        // SAFETY: the buffer's length is passed with it.
        let len = unsafe { GetModuleFileNameW(module, name.as_mut_ptr(), name.len() as u32) };
        let path = String::from_utf16_lossy(&name[..len as usize]);
        Some(
            path.rsplit(['\\', '/'])
                .next()
                .unwrap_or(path.as_str())
                .to_owned(),
        )
    }

    unsafe extern "system" fn filter(pointers: *const ExceptionPointers) -> i32 {
        // SAFETY: Windows passes valid pointers to the filter; a null one is
        // checked.
        let record = unsafe {
            pointers
                .as_ref()
                .and_then(|pointers| pointers.record.as_ref())
        };
        let Some(record) = record else {
            return CONTINUE_SEARCH;
        };
        let hooked = super::super::HOOKED
            .try_lock()
            .ok()
            .and_then(|hooked| hooked.clone());
        if let Some((folder, facts)) = hooked {
            let line = super::native_line(
                record.code,
                record.address as usize,
                module_of(record.address).as_deref(),
            );
            // A backtrace needs stack that an overflow no longer has.
            let backtrace = if record.code == STACK_OVERFLOW {
                String::new()
            } else {
                std::backtrace::Backtrace::force_capture().to_string()
            };
            let thread = std::thread::current();
            let _ = super::super::write_crash(
                &folder,
                &line,
                "native code",
                thread.name().unwrap_or("unnamed"),
                &backtrace,
                &super::super::with_render_facts(&facts),
                super::super::crumbs().as_ref(),
            );
        }
        CONTINUE_SEARCH
    }
}
