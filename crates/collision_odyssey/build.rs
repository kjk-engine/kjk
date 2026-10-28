//! Embeds every clip-brush override file (`data/clip/<game>/<module>.clip`)
//! in the build, so every machine on the same commit clips the same; and
//! stamps the build with a hash of the code that builds an area's
//! collision and navigation (this crate's and `collision`'s sources but
//! their checks, the readers of the walkmeshes, models, layouts and tables
//! it is built from in `fmt_odyssey` and `asset_odyssey`, the clip files)
//! as `KJK_AREA_BUILDER`: a stored bake is keyed by it, so changed code
//! never reads an old bake (`docs/IMPORT.md`, slice 3).

use std::path::Path;

/// Sources that check a build but take no part in it (the `probes`
/// feature's): left out of the hash, so editing them keeps the stored bakes.
const CHECKS: [&str; 2] = ["audit.rs", "probes.rs"];

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets it");
    let root = Path::new(&manifest).join("../../data/clip");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut files = Vec::new();
    if let Ok(games) = std::fs::read_dir(&root) {
        for game in games.flatten() {
            let path = game.path();
            println!("cargo:rerun-if-changed={}", path.display());
            let Ok(entries) = std::fs::read_dir(&path) else {
                continue;
            };
            for entry in entries.flatten() {
                let file = entry.path();
                if file.extension().is_some_and(|e| e == "clip") {
                    println!("cargo:rerun-if-changed={}", file.display());
                    let game = game.file_name().to_string_lossy().to_ascii_lowercase();
                    let module = file
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_ascii_lowercase())
                        .unwrap_or_else(String::new);
                    files.push((game, module, file));
                }
            }
        }
    }
    files.sort();
    let mut out = String::from("pub(crate) const FILES: &[(&str, &str, &str)] = &[\n");
    for (game, module, path) in &files {
        out.push_str(&format!(
            "    ({game:?}, {module:?}, include_str!({:?})),\n",
            path.display().to_string()
        ));
    }
    out.push_str("];\n");
    let target = Path::new(&std::env::var("OUT_DIR").expect("cargo sets it")).join("overrides.rs");
    std::fs::write(target, out).expect("OUT_DIR is writable");

    let mut sources = Vec::new();
    for folder in [
        "src",
        "../collision/src",
        "../fmt_odyssey/src",
        "../asset_odyssey/src",
    ] {
        // Each source is watched by name, not the folder (a folder watches
        // everything in it, the checks too); a new source is named in a
        // `mod`, which changes a watched one.
        let mut found = vec![Path::new(&manifest).join(folder)];
        while let Some(folder) = found.pop() {
            for entry in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    found.push(path);
                } else if path.extension().is_some_and(|e| e == "rs")
                    && !CHECKS.iter().any(|name| path.ends_with(name))
                {
                    println!("cargo:rerun-if-changed={}", path.display());
                    sources.push(path);
                }
            }
        }
    }
    sources.extend(files.iter().map(|(.., path)| path.clone()));
    sources.sort();
    // FNV-1a over each file's bytes (a stamp, not a security hash).
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for path in &sources {
        for byte in std::fs::read(path).unwrap_or_else(|_| Vec::new()) {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
        }
    }
    println!("cargo:rustc-env=KJK_AREA_BUILDER={hash:016x}");
}
