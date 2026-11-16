//! The ports the library and `kjk-inject` are built for, as cfg names:
//! `mac_port` for x86_64 macOS (the Mac ports run under Rosetta) and
//! `windows_port` for 32-bit Windows.

fn main() {
    println!("cargo::rustc-check-cfg=cfg(mac_port, windows_port)");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or(String::new());
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or(String::new());
    match (os.as_str(), arch.as_str()) {
        ("macos", "x86_64") => println!("cargo::rustc-cfg=mac_port"),
        ("windows", "x86") => println!("cargo::rustc-cfg=windows_port"),
        _ => {}
    }
}
