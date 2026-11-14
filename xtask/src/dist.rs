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
//! The save editor (`kjk-save-editor`, `crates/save_editor`) is built from
//! the same commit in a cargo run of its own (so kjk's build is the same
//! with or without it) and staged beside kjk: `kjk Save Editor.app` on
//! macOS, with its own `THIRD-PARTY-LICENSES-save-editor.html`.
//!
//! Everything goes to `target/packages/`. `--skip-build` packs the
//! programs the last run built; `--pgo <profile>` builds kjk with a profile
//! `cargo xtask pgo` made (docs/RELEASE.md, "A profile-guided build").
//!
//! `--dlss` (Windows and Linux) builds kjk with the `dlss` feature against
//! NVIDIA's SDK in `DLSS_SDK` (docs/BUILD.md) and stages beside it what
//! NVIDIA lets a program ship: the signed release DLSS library (never the
//! SDK's development ones), the SDK's licence and the third-party notices
//! of its programming guide (section 9.5 on), read out of the PDF by
//! `pdftotext`.

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
Linux:    run ./kjk.
Steam Deck: in Desktop Mode, add kjk to Steam (Games, Add a Non-Steam
          Game), then start it from Game Mode: there Steam hands it the
          Deck's controls as a gamepad. Started from Desktop Mode it gets
          them as a keyboard and mouse instead.
macOS:    the app is not signed by a known developer, so macOS refuses it
          the first time. Open it once, choose Done, then in System
          Settings → Privacy & Security choose Open Anyway (on macOS 14
          and older: right-click kjk.app, choose Open, then Open
          again). Or run this once in Terminal from the folder it is in:
              xattr -dr com.apple.quarantine kjk.app
          The same goes for kjk Save Editor.app:
              xattr -dr com.apple.quarantine \"kjk Save Editor.app\"

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

Save editor
-----------
kjk-save-editor (kjk Save Editor.app on macOS) opens a kjk save to change
it: give a character another weapon, a modded item or a Force power, set
credits, stats, quests and story flags, or fix a save (a stuck quest, a
missing companion, lost gear). It finds kjk's saves and your games by
itself. Every change is listed and can be undone, the save is checked
before it is written, and the save as it was is kept beside it
(<id>.backup-<time>). On the Steam Deck it runs in Desktop Mode.

Movies need ffmpeg on the PATH; without it they are skipped.

Co-op: friends join by the host's address, as on a LAN; over the internet
the host forwards UDP port 29170.
";

/// The readme's last word: whose names and code the package holds.
const LEGAL_README: &str = "
Star Wars, Knights of the Old Republic and Jedi Academy are trademarks of
their respective owners; kjk is not affiliated with or endorsed by
them. kjk is MIT licensed (LICENSE); NOTICE credits the projects read
as references, and THIRD-PARTY-LICENSES.html lists the libraries it is
built from (THIRD-PARTY-LICENSES-save-editor.html the save editor's).
";

/// `LEGAL_README` in a package with NVIDIA's files, which LICENSE does not
/// cover.
const LEGAL_README_DLSS: &str = "
Star Wars, Knights of the Old Republic and Jedi Academy are trademarks of
their respective owners; kjk is not affiliated with or endorsed by
them. kjk's own code is MIT licensed (LICENSE); the NVIDIA files are not
(see DLSS below). NOTICE credits the projects read as references, and
THIRD-PARTY-LICENSES.html lists the libraries it is built from
(THIRD-PARTY-LICENSES-save-editor.html the save editor's).
";

/// What the readme adds in a package built with `--dlss`.
const DLSS_README: &str = "
DLSS
----
On an NVIDIA RTX card, Options, Graphics, Advanced, Anti-Aliasing offers
DLAA and DLSS (Quality, Balanced, Performance, Ultra Performance) beside
Off, 2x, 4x and TAA. This build draws through Vulkan where the graphics
driver has it (DirectX 12 otherwise, on Windows). Without an RTX card, or
when the driver does not offer DLSS, those choices are not shown, and a
DLSS choice saved on another machine plays as TAA.
The DLSS library beside kjk (nvngx_dlss.dll on Windows,
libnvidia-ngx-dlss.so.<version> on Linux) is NVIDIA's, under the licence in
NVIDIA-RTX-SDK-LICENSE.txt; NVIDIA-DLSS-NOTICES.txt holds the notices of
the code in it. NVIDIA, RTX and DLSS are trademarks of NVIDIA Corporation.
";

pub fn run(args: &[String]) -> Result<(), String> {
    let mut skip_build = false;
    let mut dlss = false;
    let mut profile: Option<PathBuf> = None;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--skip-build" => skip_build = true,
            "--dlss" => dlss = true,
            "--pgo" => {
                let path = args
                    .next()
                    .ok_or("dist: --pgo takes a profile (cargo xtask pgo)")?;
                let path = std::path::absolute(path).map_err(|error| format!("dist: {error}"))?;
                if !path.is_file() {
                    return Err(format!("dist: {} is not a profile", path.display()));
                }
                profile = Some(path);
            }
            other => {
                return Err(format!(
                    "dist: unknown option {other} (--skip-build, --dlss, --pgo <profile>)"
                ));
            }
        }
    }
    // The SDK's folder, checked before a build that would need it.
    let dlss_sdk = dlss.then(Dlss::from_env).transpose()?;
    let kjk_features: &[&str] = if dlss {
        &["--no-default-features", "--features", "dlss"]
    } else {
        &["--no-default-features"]
    };
    // Each program and the features it is built (and its licences read) with.
    let programs = [(KJK, kjk_features), (EDITOR, &[])];
    let root = repo_root();
    let packages = target_dir(&root).join("packages");
    create(&packages)?;
    // The licence pages are written while the programs build: `cargo about`
    // reads the crates' manifests and needs no build.
    let mut licences = Vec::new();
    for (program, features) in programs {
        let page = packages.join(program.licences);
        licences.push((third_party_licenses(&root, program, features, &page)?, page));
    }
    if !skip_build {
        let remaps = remaps(&root);
        // One cargo run each: the editor's crates and features never touch
        // kjk's build. Both run at once, each in a build folder of its own
        // (one folder takes one cargo run at a time): fat LTO optimises kjk
        // on one core for most of its build, and the editor builds on the
        // other cores meanwhile.
        let mut builds = Vec::new();
        for (program, features) in programs {
            // kjk laid out by where a profile says its time goes (`cargo
            // xtask pgo`); a function changed since the profile was made is
            // built as without one, unwarned.
            let mut flags = remaps.clone();
            if let Some(profile) = profile.as_ref().filter(|_| program.package == KJK.package) {
                flags.push(format!("-Cprofile-use={}", profile.display()));
                flags.push("-Cllvm-args=-no-pgo-warn-mismatch".to_string());
            }
            let build = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
                .current_dir(&root)
                .env("CARGO_ENCODED_RUSTFLAGS", flags.join("\u{1f}"))
                .args(["build", "--profile", "dist", "-p", program.package])
                .arg("--target-dir")
                .arg(build_dir(&root, &program))
                .args(features)
                .spawn()
                .map_err(|error| format!("cargo build: {error}"))?;
            builds.push((program, build));
        }
        // Both are waited for before a failure is reported, so neither is
        // left running.
        let mut failed = Vec::new();
        for (program, mut build) in builds {
            let status = build
                .wait()
                .map_err(|error| format!("cargo build: {error}"))?;
            if !status.success() {
                failed.push(program.package);
            }
        }
        if !failed.is_empty() {
            return Err(format!(
                "dist: the build of {} failed",
                failed.join(" and ")
            ));
        }
    }
    let built = |program: &Program| {
        let built = build_dir(&root, program).join("dist").join(program.file());
        if built.is_file() {
            Ok(built)
        } else {
            Err(format!("dist: {} is not built", built.display()))
        }
    };
    let builds = [built(&KJK)?, built(&EDITOR)?];

    let version = version()?;
    let os = match std::env::consts::OS {
        "macos" => "macos",
        "windows" => "windows",
        _ => "linux",
    };
    let name = format!("kjk-{version}-{os}-{}", std::env::consts::ARCH);
    let stage = packages.join(&name);
    if stage.exists() {
        std::fs::remove_dir_all(&stage).map_err(|error| format!("{}: {error}", stage.display()))?;
    }
    std::fs::create_dir_all(&stage).map_err(|error| format!("{}: {error}", stage.display()))?;

    for (program, built) in [KJK, EDITOR].iter().zip(&builds) {
        if os == "macos" {
            let app = stage.join(program.app).join("Contents");
            let binaries = app.join("MacOS");
            create(&binaries)?;
            write(&app.join("Info.plist"), &info_plist(&version, program))?;
            app_icon(&packages, &app.join("Resources"))?;
            copy(built, &binaries.join(program.file()))?;
            sign_ad_hoc(&stage.join(program.app))?;
        } else {
            copy(built, &stage.join(program.file()))?;
        }
    }
    let readme = match &dlss_sdk {
        Some(sdk) => {
            sdk.stage(&stage)?;
            format!("{PLAYERS_README}{LEGAL_README_DLSS}{DLSS_README}")
        }
        None => format!("{PLAYERS_README}{LEGAL_README}"),
    };
    write(&stage.join("README.txt"), &readme)?;
    copy(&root.join("LICENSE"), &stage.join("LICENSE.txt"))?;
    copy(&root.join("NOTICE"), &stage.join("NOTICE.txt"))?;
    for ((program, _), (mut writing, page)) in programs.into_iter().zip(licences) {
        let status = writing
            .wait()
            .map_err(|error| format!("cargo about: {error}"))?;
        if !status.success() {
            return Err("dist: `cargo about generate` failed; it installs with \
                 `cargo install --locked cargo-about --features cli`"
                .to_string());
        }
        std::fs::rename(&page, stage.join(program.licences))
            .map_err(|error| format!("{}: {error}", page.display()))?;
    }

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

/// A program the package holds.
#[derive(Clone, Copy)]
struct Program {
    /// Its binary's name, without `.exe`.
    binary: &'static str,
    /// Its package, and its crate's manifest (which `cargo about` reads).
    package: &'static str,
    manifest: &'static str,
    /// Its licences' page in the package.
    licences: &'static str,
    /// On macOS: its app, the name shown, and the bundle id.
    app: &'static str,
    name: &'static str,
    id: &'static str,
    category: &'static str,
}

impl Program {
    fn file(&self) -> String {
        if cfg!(windows) {
            format!("{}.exe", self.binary)
        } else {
            self.binary.to_string()
        }
    }
}

const KJK: Program = Program {
    binary: "kjk",
    package: "launcher",
    manifest: "crates/launcher/Cargo.toml",
    licences: "THIRD-PARTY-LICENSES.html",
    app: "kjk.app",
    name: "kjk",
    id: "io.github.kjk",
    category: "public.app-category.role-playing-games",
};

const EDITOR: Program = Program {
    binary: "kjk-save-editor",
    package: "save_editor",
    manifest: "crates/save_editor/Cargo.toml",
    licences: "THIRD-PARTY-LICENSES-save-editor.html",
    app: "kjk Save Editor.app",
    name: "kjk Save Editor",
    id: "io.github.kjk.save-editor",
    category: "public.app-category.utilities",
};

/// The sources' paths the program carries (in its panics) name no machine:
/// the home folder (cargo's registry, the toolchain) as `~`, the checkout as
/// `.`; the later, longer prefix wins where both fit.
pub(crate) fn remaps(root: &Path) -> Vec<String> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    home.map(PathBuf::from)
        .into_iter()
        .map(|home| (home, "~"))
        .chain([(root.to_path_buf(), ".")])
        .map(|(from, to)| format!("--remap-path-prefix={}={to}", from.display()))
        .collect()
}

/// NVIDIA's DLSS SDK (`DLSS_SDK`), what a `--dlss` package ships from.
struct Dlss {
    sdk: PathBuf,
}

impl Dlss {
    fn from_env() -> Result<Self, String> {
        if !matches!(std::env::consts::OS, "windows" | "linux")
            || std::env::consts::ARCH != "x86_64"
        {
            return Err("dist --dlss: DLSS is for Windows and Linux on x86-64".to_string());
        }
        let sdk = std::env::var_os("DLSS_SDK")
            .map(PathBuf::from)
            .ok_or("dist --dlss: DLSS_SDK names no folder (docs/BUILD.md)")?;
        if !sdk.join("include").is_dir() {
            return Err(format!(
                "dist --dlss: {} is not the DLSS SDK",
                sdk.display()
            ));
        }
        Ok(Self { sdk })
    }

    /// The signed release library (`lib/<platform>/rel/`; the `dev/` ones,
    /// with NVIDIA's debug overlay, are for development alone).
    /// Its path and file name.
    fn library(&self) -> Result<(PathBuf, String), String> {
        let (platform, name) = if cfg!(windows) {
            ("Windows_x86_64", "nvngx_dlss.dll")
        } else {
            ("Linux_x86_64", "libnvidia-ngx-dlss.so.")
        };
        let rel = self.sdk.join("lib").join(platform).join("rel");
        std::fs::read_dir(&rel)
            .map_err(|error| format!("{}: {error}", rel.display()))?
            .flatten()
            .find_map(|entry| {
                let file = entry.file_name().into_string().ok()?;
                file.starts_with(name).then(|| (entry.path(), file))
            })
            .ok_or_else(|| format!("dist --dlss: no {name}… in {}", rel.display()))
    }

    /// The library beside the program, the SDK's licence and the notices.
    fn stage(&self, stage: &Path) -> Result<(), String> {
        let (library, name) = self.library()?;
        copy(&library, &stage.join(name))?;
        copy(
            &self.sdk.join("LICENSE.txt"),
            &stage.join("NVIDIA-RTX-SDK-LICENSE.txt"),
        )?;
        let guide = self
            .sdk
            .join("doc")
            .join("DLSS_Programming_Guide_Release.pdf");
        let output = Command::new("pdftotext")
            .args(["-layout", "-enc", "UTF-8"])
            .arg(&guide)
            .arg("-")
            .output()
            .map_err(|error| {
                format!(
                    "dist --dlss: pdftotext ({error}) reads the notices out of {}; \
                     it comes with poppler (poppler-utils) or xpdf",
                    guide.display()
                )
            })?;
        if !output.status.success() {
            return Err(format!(
                "dist --dlss: pdftotext failed on {}",
                guide.display()
            ));
        }
        let notices = notices(&String::from_utf8_lossy(&output.stdout))
            .ok_or("dist --dlss: the guide has no section 9.5 Notices")?;
        write(&stage.join("NVIDIA-DLSS-NOTICES.txt"), &notices)
    }
}

/// The guide's notices, its "9.5 Notices" through the third-party software
/// up to section 9.7, without the pages' footers. The headings are told
/// from the contents' lines by those having no leader dots.
fn notices(guide: &str) -> Option<String> {
    let heading = |line: &str, number: &str| {
        let line = line.trim();
        line.starts_with(number) && !line.contains("..")
    };
    let start = guide
        .lines()
        .position(|line| heading(line, "9.5 ") && line.contains("Notices"))?;
    let mut text = String::from(
        "NVIDIA DLSS: the notices NVIDIA's DLSS programming guide asks to be shipped\n\
         with the DLSS library (section 9.5 on).\n\n",
    );
    let mut blank = false;
    for line in guide
        .lines()
        .skip(start)
        .take_while(|line| !heading(line, "9.7 "))
    {
        let line = line.trim_end();
        if line.trim_start().starts_with("NVIDIA DLSS |") && line.contains("Page |") {
            continue;
        }
        if line.trim().is_empty() {
            blank = true;
            continue;
        }
        if blank && !text.ends_with("\n\n") {
            text.push('\n');
        }
        blank = false;
        text.push_str(line);
        text.push('\n');
    }
    Some(text)
}

pub(crate) fn target_dir(root: &Path) -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from)
}

/// Where cargo builds `program`: kjk in the target folder, the editor in a
/// folder of its own inside it, so both build at once.
fn build_dir(root: &Path, program: &Program) -> PathBuf {
    let target = target_dir(root);
    if program.package == EDITOR.package {
        target.join("editor")
    } else {
        target
    }
}

/// The version: the last release tag (`v…`) and the commits since (`git
/// describe`), else the commit.
fn version() -> Result<String, String> {
    let described = git(&["describe", "--tags", "--match", "v*", "--always", "--dirty"])?;
    Ok(String::from_utf8_lossy(&described).trim().to_string())
}

/// Starts `cargo about`, which writes the licences of every crate `program`
/// is built from with `features` (`about.toml`, `about.hbs`).
fn third_party_licenses(
    root: &Path,
    program: Program,
    features: &[&str],
    to: &Path,
) -> Result<std::process::Child, String> {
    Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .current_dir(root)
        .args([
            "about",
            "generate",
            "--locked",
            "--fail",
            "-c",
            "about.toml",
        ])
        .args(["-m", program.manifest])
        .args(features)
        .args(["about.hbs", "-o"])
        .arg(to)
        .spawn()
        .map_err(|error| format!("cargo about: {error}"))
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

fn info_plist(version: &str, program: &Program) -> String {
    let bundle = bundle_version(version);
    let Program {
        name,
        id,
        binary,
        category,
        ..
    } = program;
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>{name}</string>
  <key>CFBundleDisplayName</key><string>{name}</string>
  <key>CFBundleIdentifier</key><string>{id}</string>
  <key>CFBundleExecutable</key><string>{binary}</string>
  <key>CFBundleIconFile</key><string>kjk</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>{bundle}</string>
  <key>CFBundleVersion</key><string>{bundle}</string>
  <key>CFBundleGetInfoString</key><string>{name} {version}</string>
  <key>LSApplicationCategoryType</key><string>{category}</string>
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
