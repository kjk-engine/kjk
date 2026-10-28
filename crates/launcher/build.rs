#[path = "../game/stamp.rs"]
mod stamp;

use std::path::{Path, PathBuf};

/// Stamps the commit into the binary as `KJK_GIT_SHA` (`game/stamp.rs`),
/// stamped again when any crate's source changes; on Windows, builds in the
/// icon (`kjk.rc`); with Steam, puts Steam's library beside the binary.
fn main() {
    stamp::git_sha(&[".."]);
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=kjk.ico");
        embed_resource::compile("kjk.rc", embed_resource::NONE)
            .manifest_optional()
            .expect("kjk.rc compiles");
    }
    if std::env::var_os("CARGO_FEATURE_STEAM").is_some() {
        steam_beside_binary();
    }
}

/// Steam's library (Steamworks' redistributable, from `steamworks-sys`)
/// copied into the profile's folder, where `kjk` loads it from (macOS's
/// `@loader_path`, Windows' own folder; Linux reads `LD_LIBRARY_PATH` or
/// the rpath, `$ORIGIN` here). Without it the binary does not start.
fn steam_beside_binary() {
    let (folder, file) = match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("macos") => ("osx", "libsteam_api.dylib"),
        Ok("windows") => ("win64", "steam_api64.dll"),
        Ok("linux") => ("linux64", "libsteam_api.so"),
        _ => return,
    };
    if file.ends_with(".so") {
        println!("cargo:rustc-link-arg-bins=-Wl,-rpath,$ORIGIN");
    }
    let Some(source) = steamworks_sys()
        .map(|sys| {
            sys.join("lib/steam/redistributable_bin")
                .join(folder)
                .join(file)
        })
        .filter(|path| path.is_file())
    else {
        println!("cargo:warning=Steam's {file} not found: kjk will not start without it");
        return;
    };
    // OUT_DIR is <target>/<profile>/build/launcher-<hash>/out.
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
    let Some(profile) = out.ancestors().nth(3) else {
        return;
    };
    let target = profile.join(file);
    if std::fs::copy(&source, &target).is_err() {
        println!(
            "cargo:warning=could not copy {file} to {}",
            target.display()
        );
    }
    println!("cargo:rerun-if-changed={}", source.display());
}

/// `steamworks-sys`'s sources in cargo's registry, at the version the
/// lock file holds.
fn steamworks_sys() -> Option<PathBuf> {
    let home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
            Some(Path::new(&home).join(".cargo"))
        })?;
    let version = locked_version("steamworks-sys")?;
    std::fs::read_dir(home.join("registry/src"))
        .ok()?
        .flatten()
        .map(|index| index.path().join(format!("steamworks-sys-{version}")))
        .find(|path| path.is_dir())
}

/// A package's version in the workspace's `Cargo.lock`.
fn locked_version(package: &str) -> Option<String> {
    let lock = std::fs::read_to_string("../../Cargo.lock").ok()?;
    let name = format!("name = \"{package}\"");
    let mut lines = lock.lines();
    lines.find(|line| *line == name)?;
    let version = lines.next()?.strip_prefix("version = \"")?;
    Some(version.trim_end_matches('"').to_string())
}
