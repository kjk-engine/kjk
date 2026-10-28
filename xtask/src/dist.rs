//! `cargo xtask dist`: the player's build of this machine's OS, packed
//! (docs/RELEASE.md). `kjk` built with the `dist` profile and without the
//! verbs that check and measure or Steam (`--no-default-features`: nothing
//! but MIT code and the crates' own licences ships), then staged with the
//! player's readme, the licences and the crates' licences (`cargo about`,
//! `about.toml`) and packed:
//! - Windows: `kjk-<version>-windows-<arch>.zip`, the folder inside;
//! - macOS: `kjk.app` with its icon (unsigned: the readme says how to
//!   open it), in
//!   `kjk-<version>-macos-<arch>.tar.gz`;
//! - Linux (the Steam Deck too): `kjk-<version>-linux-<arch>.tar.gz`.
//!
//! Everything goes to `target/packages/`. `--skip-build` packs the `kjk`
//! the last run built.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{git, repo_root};

/// What the player reads first, beside the program.
const PLAYERS_README: &str = "\
kjk
========

Knights of the Old Republic's worlds, story and dialogue with Jedi Academy's
movement, lightsabers and the Force, and co-op.

kjk plays from your own copies of three games, which it only ever
reads:
  - Star Wars: Knights of the Old Republic (the campaign)
  - Star Wars Jedi Knight: Jedi Academy (movement, sabers and the Force)
  - Star Wars: Knights of the Old Republic II (the look of the menus)
No game data comes with kjk.

Starting
--------
Windows:  run kjk.exe.
Linux:    run ./kjk (on the Steam Deck: add it to Steam as a non-Steam
          game, or run it from Desktop Mode).
macOS:    the app is not signed by a known developer, so macOS refuses it
          the first time. Open it once, choose Done, then in System
          Settings → Privacy & Security choose Open Anyway (on macOS 14
          and older: right-click kjk.app, choose Open, then Open
          again). Or run this once in Terminal from the folder it is in:
              xattr -dr com.apple.quarantine kjk.app

The first start opens the setup: Steam and GOG installs are found by
themselves; locate any other. KOTOR's shared data is then read once (a few
seconds), and the game starts. `kjk setup` opens the setup again.

Where things are kept
---------------------
Settings, saves and the last few runs' logs (send these with a bug report):
  Windows  %APPDATA%\\kjk
  macOS    ~/Library/Application Support/kjk
  Linux    ~/.config/kjk
Imported game data (safe to delete; it is made again):
  Windows  %LOCALAPPDATA%\\kjk
  macOS    ~/Library/Caches/kjk
  Linux    ~/.cache/kjk

Movies need ffmpeg on the PATH; without it they are skipped.

Co-op: friends join by the host's address, as on a LAN; over the internet
the host forwards UDP port 29170.

Star Wars, Knights of the Old Republic and Jedi Academy are trademarks of
their respective owners; kjk is not affiliated with or endorsed by
them. kjk is MIT licensed (LICENSE); NOTICE credits the projects read
as references, and THIRD-PARTY-LICENSES.html lists the libraries it is
built from.
";

pub fn run(args: &[String]) -> Result<(), String> {
    let mut skip_build = false;
    for arg in args {
        match arg.as_str() {
            "--skip-build" => skip_build = true,
            other => return Err(format!("dist: unknown option {other} (--skip-build)")),
        }
    }
    let root = repo_root();
    if !skip_build {
        // The sources' paths the program carries (in its panics) name no
        // machine: the home folder (cargo's registry, the toolchain) as `~`,
        // the checkout as `.`; the later, longer prefix wins where both fit.
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
        let remaps: Vec<String> = home
            .map(PathBuf::from)
            .into_iter()
            .map(|home| (home, "~"))
            .chain([(root.clone(), ".")])
            .map(|(from, to)| format!("--remap-path-prefix={}={to}", from.display()))
            .collect();
        let status = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
            .current_dir(&root)
            .env("CARGO_ENCODED_RUSTFLAGS", remaps.join("\u{1f}"))
            .args([
                "build",
                "--profile",
                "dist",
                "-p",
                "launcher",
                "--no-default-features",
            ])
            .status()
            .map_err(|error| format!("cargo build: {error}"))?;
        if !status.success() {
            return Err("dist: the build failed".to_string());
        }
    }
    let exe = if cfg!(windows) { "kjk.exe" } else { "kjk" };
    let built = target_dir(&root).join("dist").join(exe);
    if !built.is_file() {
        return Err(format!("dist: {} is not built", built.display()));
    }

    let version = version()?;
    let os = match std::env::consts::OS {
        "macos" => "macos",
        "windows" => "windows",
        _ => "linux",
    };
    let name = format!("kjk-{version}-{os}-{}", std::env::consts::ARCH);
    let packages = target_dir(&root).join("packages");
    let stage = packages.join(&name);
    if stage.exists() {
        std::fs::remove_dir_all(&stage).map_err(|error| format!("{}: {error}", stage.display()))?;
    }
    std::fs::create_dir_all(&stage).map_err(|error| format!("{}: {error}", stage.display()))?;

    let program = if os == "macos" {
        let app = stage.join("kjk.app").join("Contents");
        let binaries = app.join("MacOS");
        create(&binaries)?;
        write(&app.join("Info.plist"), &info_plist(&version))?;
        app_icon(&packages, &app.join("Resources"))?;
        binaries.join(exe)
    } else {
        stage.join(exe)
    };
    copy(&built, &program)?;
    if os == "macos" {
        sign_ad_hoc(&stage.join("kjk.app"))?;
    }
    write(&stage.join("README.txt"), PLAYERS_README)?;
    copy(&root.join("LICENSE"), &stage.join("LICENSE.txt"))?;
    copy(&root.join("NOTICE"), &stage.join("NOTICE.txt"))?;
    third_party_licenses(&root, &stage.join("THIRD-PARTY-LICENSES.html"))?;

    let archive = if os == "windows" {
        packages.join(format!("{name}.zip"))
    } else {
        packages.join(format!("{name}.tar.gz"))
    };
    if archive.exists() {
        std::fs::remove_file(&archive)
            .map_err(|error| format!("{}: {error}", archive.display()))?;
    }
    // bsdtar on Windows (10 and later) writes zips with `-a`; elsewhere a
    // gzipped tar keeps the program's executable bit.
    let mut tar = Command::new("tar");
    tar.current_dir(&packages);
    // A tar names no one: its files are owned by number 0 with no user or
    // group name (the builder's login otherwise, in bsdtar on macOS and GNU
    // tar on Linux alike), and macOS adds none of its metadata or extended
    // attributes.
    match os {
        "windows" => {
            tar.args(["-a", "-c"]);
        }
        "macos" => {
            tar.env("COPYFILE_DISABLE", "1").args([
                "-c",
                "-z",
                "--uid=0",
                "--gid=0",
                "--numeric-owner",
                "--no-mac-metadata",
                "--no-xattrs",
                "--no-acls",
            ]);
        }
        _ => {
            tar.args(["-c", "-z", "--owner=0", "--group=0", "--numeric-owner"]);
        }
    }
    tar.arg("-f");
    let status = tar
        .arg(&archive)
        .arg(&name)
        .status()
        .map_err(|error| format!("tar: {error}"))?;
    if !status.success() {
        return Err("dist: tar failed".to_string());
    }
    let bytes = std::fs::metadata(&archive).map_or(0, |meta| meta.len());
    println!(
        "dist: {} ({:.1} MB)",
        archive.display(),
        bytes as f64 / 1_000_000.0
    );
    Ok(())
}

fn target_dir(root: &Path) -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from)
}

/// The version: the last release tag (`v…`) and the commits since (`git
/// describe`), else the commit.
fn version() -> Result<String, String> {
    let described = git(&["describe", "--tags", "--match", "v*", "--always", "--dirty"])?;
    Ok(String::from_utf8_lossy(&described).trim().to_string())
}

/// `cargo about` writes the licences of every crate the program is built
/// from (`about.toml`, `about.hbs`).
fn third_party_licenses(root: &Path, to: &Path) -> Result<(), String> {
    let status = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .current_dir(root)
        .args([
            "about",
            "generate",
            "--locked",
            "--fail",
            "-c",
            "about.toml",
        ])
        .args(["-m", "crates/launcher/Cargo.toml", "--no-default-features"])
        .args(["about.hbs", "-o"])
        .arg(to)
        .status()
        .map_err(|error| format!("cargo about: {error}"))?;
    if !status.success() {
        return Err("dist: `cargo about generate` failed; it installs with \
             `cargo install --locked cargo-about --features cli`"
            .to_string());
    }
    Ok(())
}

/// `Resources/kjk.icns`, made by `iconutil` from the drawn sizes.
fn app_icon(scratch: &Path, resources: &Path) -> Result<(), String> {
    let iconset = scratch.join("kjk.iconset");
    if iconset.exists() {
        std::fs::remove_dir_all(&iconset)
            .map_err(|error| format!("{}: {error}", iconset.display()))?;
    }
    crate::icon::write_iconset(&iconset)?;
    create(resources)?;
    let status = Command::new("iconutil")
        .args(["-c", "icns", "-o"])
        .arg(resources.join("kjk.icns"))
        .arg(&iconset)
        .status()
        .map_err(|error| format!("iconutil: {error}"))?;
    if !status.success() {
        return Err("dist: iconutil failed".to_string());
    }
    std::fs::remove_dir_all(&iconset).map_err(|error| format!("{}: {error}", iconset.display()))
}

/// Seals the whole bundle with an ad-hoc signature (no identity). The
/// linker signs only the program, so the plist and icon beside it break
/// that signature, and a downloaded app with a broken one is "damaged" to
/// Gatekeeper, which no Open Anyway gets past; sealed, it is merely from
/// an unknown developer.
fn sign_ad_hoc(app: &Path) -> Result<(), String> {
    let status = Command::new("codesign")
        .args(["--force", "--sign", "-"])
        .arg(app)
        .status()
        .map_err(|error| format!("codesign: {error}"))?;
    if !status.success() {
        return Err("dist: codesign failed".to_string());
    }
    Ok(())
}

/// What macOS shows as the app's version: the tag's numbers (`v0.2.1` is
/// `0.2.1`), or 0.0.0 for a build that is not a release.
fn bundle_version(version: &str) -> &str {
    let Some(numbers) = version.strip_prefix('v') else {
        return "0.0.0";
    };
    let end = numbers
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(numbers.len());
    match &numbers[..end] {
        "" => "0.0.0",
        numbers => numbers.trim_end_matches('.'),
    }
}

fn info_plist(version: &str) -> String {
    let bundle = bundle_version(version);
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>kjk</string>
  <key>CFBundleDisplayName</key><string>kjk</string>
  <key>CFBundleIdentifier</key><string>io.github.kjk</string>
  <key>CFBundleExecutable</key><string>kjk</string>
  <key>CFBundleIconFile</key><string>kjk</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>{bundle}</string>
  <key>CFBundleVersion</key><string>{bundle}</string>
  <key>CFBundleGetInfoString</key><string>kjk {version}</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.role-playing-games</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
"#
    )
}

fn create(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|error| format!("{}: {error}", dir.display()))
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    std::fs::write(path, text).map_err(|error| format!("{}: {error}", path.display()))
}

fn copy(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::copy(from, to)
        .map(|_| ())
        .map_err(|error| format!("{} to {}: {error}", from.display(), to.display()))
}
