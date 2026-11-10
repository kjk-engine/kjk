//! `kjk`: see the library (`src/lib.rs`).

/// Asks a laptop with two graphics chips (NVIDIA Optimus, AMD switchable
/// graphics) to run kjk on the stronger one. Without these, OpenGL in
/// particular starts on the weaker Intel chip. `build.rs` exports them.
#[cfg(windows)]
#[used]
#[unsafe(no_mangle)]
#[expect(non_upper_case_globals, reason = "the drivers look for this name")]
pub static NvOptimusEnablement: u32 = 1;
#[cfg(windows)]
#[used]
#[unsafe(no_mangle)]
#[expect(non_upper_case_globals, reason = "the drivers look for this name")]
pub static AmdPowerXpressRequestHighPerformance: u32 = 1;

fn main() -> std::process::ExitCode {
    kjk::run()
}
