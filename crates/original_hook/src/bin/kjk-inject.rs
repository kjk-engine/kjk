//! `kjk-inject GAME LIBRARY`: starts a Windows build of the original game
//! with `kjk original`'s library in it, waits until the game quits, and
//! exits with its exit code. The game starts in its own folder, with this
//! program's environment.
//!
//! The library is loaded once the game has reached its entry point. A
//! process started suspended has only `ntdll` loaded, so `LoadLibraryA`
//! cannot run in it yet. The game is held at its entry point by a loop of
//! two bytes written there, the library is loaded by a thread of the game's,
//! and the entry point's own bytes are put back before the game goes on.
//!
//! It is a 32-bit program, as the games are, so the `LoadLibraryA` it finds
//! in its own `kernel32` is at the game's address too.

#[cfg(windows_port)]
fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let [_, game, library] = args.as_slice() else {
        eprintln!("usage: kjk-inject GAME LIBRARY");
        return std::process::ExitCode::from(2);
    };
    match inject::run(game, library) {
        Ok(code) => std::process::ExitCode::from(code as u8),
        Err(error) => {
            eprintln!("kjk-inject: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(not(windows_port))]
fn main() {
    eprintln!("kjk-inject runs only as a 32-bit Windows program");
    std::process::exit(2);
}

#[cfg(windows_port)]
mod inject {
    use std::ffi::{CString, c_void};
    use std::time::Duration;

    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError};
    use windows_sys::Win32::System::Diagnostics::Debug::{
        CONTEXT, CONTEXT_FULL_X86, FlushInstructionCache, GetThreadContext, ReadProcessMemory,
        WriteProcessMemory,
    };
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
    use windows_sys::Win32::System::Memory::{
        MEM_COMMIT, PAGE_EXECUTE_READWRITE, PAGE_READWRITE, VirtualAllocEx, VirtualProtectEx,
    };
    use windows_sys::Win32::System::Threading::{
        CREATE_SUSPENDED, CreateProcessA, CreateRemoteThread, GetExitCodeProcess,
        GetExitCodeThread, INFINITE, PROCESS_INFORMATION, ResumeThread, STARTUPINFOA,
        SuspendThread, TerminateProcess, WaitForSingleObject,
    };

    fn failed(what: &str) -> String {
        // SAFETY: no argument.
        format!("{what}: error {}", unsafe { GetLastError() })
    }

    /// Starts `game` with `library` in it, and gives its exit code once it
    /// quits.
    pub fn run(game: &str, library: &str) -> Result<u32, String> {
        let folder = std::path::Path::new(game)
            .parent()
            .ok_or("the game has no folder")?;
        let game_c = CString::new(game).map_err(|error| error.to_string())?;
        let folder_c =
            CString::new(folder.to_string_lossy().as_bytes()).map_err(|error| error.to_string())?;
        let library_c = CString::new(library).map_err(|error| error.to_string())?;
        // SAFETY: Win32 calls on a process this program starts, with
        // buffers of the sizes given.
        unsafe {
            let mut startup: STARTUPINFOA = std::mem::zeroed();
            startup.cb = size_of::<STARTUPINFOA>() as u32;
            let mut process: PROCESS_INFORMATION = std::mem::zeroed();
            if CreateProcessA(
                game_c.as_ptr().cast(),
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                CREATE_SUSPENDED,
                std::ptr::null(),
                folder_c.as_ptr().cast(),
                &startup,
                &mut process,
            ) == 0
            {
                return Err(failed("CreateProcess"));
            }
            let handle = process.hProcess;
            let loaded = load(handle, process.hThread, &library_c);
            if let Err(error) = loaded {
                TerminateProcess(handle, 1);
                return Err(error);
            }
            WaitForSingleObject(handle, INFINITE);
            let mut code = 0;
            GetExitCodeProcess(handle, &mut code);
            CloseHandle(process.hThread);
            CloseHandle(handle);
            Ok(code)
        }
    }

    /// Holds the suspended game at its entry point, loads `library` into
    /// it, and lets it go on.
    ///
    /// # Safety
    /// `game` and `thread` must be a just-started, suspended process and
    /// its first thread.
    unsafe fn load(
        game: *mut c_void,
        thread: *mut c_void,
        library: &CString,
    ) -> Result<(), String> {
        // SAFETY: the caller's, and buffers of the sizes given.
        unsafe {
            let read = |at: usize, into: *mut u8, size: usize| {
                ReadProcessMemory(
                    game,
                    at as *const c_void,
                    into.cast(),
                    size,
                    std::ptr::null_mut(),
                )
            };
            let mut context: CONTEXT = std::mem::zeroed();
            context.ContextFlags = CONTEXT_FULL_X86;
            if GetThreadContext(thread, &mut context) == 0 {
                return Err(failed("GetThreadContext"));
            }
            // The first thread's ebx holds the process's PEB, whose third
            // field is the image's base.
            let mut base = 0u32;
            read(context.Ebx as usize + 8, (&raw mut base).cast(), 4);
            let mut header = 0u32;
            read(base as usize + 0x3c, (&raw mut header).cast(), 4);
            let mut entry = 0u32;
            read(
                base as usize + header as usize + 40,
                (&raw mut entry).cast(),
                4,
            );
            let entry = (base + entry) as *mut c_void;
            let mut kept = [0u8; 2];
            let mut old = 0;
            VirtualProtectEx(game, entry, 2, PAGE_EXECUTE_READWRITE, &mut old);
            read(entry as usize, kept.as_mut_ptr(), 2);
            // `jmp $`: a jump to itself.
            let spin: [u8; 2] = [0xeb, 0xfe];
            WriteProcessMemory(game, entry, spin.as_ptr().cast(), 2, std::ptr::null_mut());
            FlushInstructionCache(game, entry, 2);
            ResumeThread(thread);
            let mut held = false;
            for _ in 0..1000 {
                std::thread::sleep(Duration::from_millis(10));
                SuspendThread(thread);
                GetThreadContext(thread, &mut context);
                if context.Eip as usize == entry as usize {
                    held = true;
                    break;
                }
                ResumeThread(thread);
            }
            if !held {
                return Err(format!(
                    "the game never reached its entry point {entry:p} (image at {base:#x}, the thread at {:#x})",
                    context.Eip
                ));
            }
            let size = library.as_bytes_with_nul().len();
            let remote = VirtualAllocEx(game, std::ptr::null(), size, MEM_COMMIT, PAGE_READWRITE);
            WriteProcessMemory(
                game,
                remote,
                library.as_ptr().cast(),
                size,
                std::ptr::null_mut(),
            );
            let load_library = GetProcAddress(
                GetModuleHandleA(c"kernel32.dll".as_ptr().cast()),
                c"LoadLibraryA".as_ptr().cast(),
            )
            .ok_or("kernel32 has no LoadLibraryA")?;
            let loader = CreateRemoteThread(
                game,
                std::ptr::null(),
                0,
                Some(std::mem::transmute::<
                    unsafe extern "system" fn() -> isize,
                    unsafe extern "system" fn(*mut c_void) -> u32,
                >(load_library)),
                remote,
                0,
                std::ptr::null_mut(),
            );
            if loader.is_null() {
                return Err(failed("CreateRemoteThread"));
            }
            WaitForSingleObject(loader, INFINITE);
            let mut module = 0;
            GetExitCodeThread(loader, &mut module);
            CloseHandle(loader);
            if module == 0 {
                return Err(format!(
                    "the game could not load {}",
                    library.to_string_lossy()
                ));
            }
            WriteProcessMemory(game, entry, kept.as_ptr().cast(), 2, std::ptr::null_mut());
            VirtualProtectEx(game, entry, 2, old, &mut old);
            FlushInstructionCache(game, entry, 2);
            ResumeThread(thread);
            Ok(())
        }
    }
}
