//! Embeds our own module data in the build, so every machine on the same
//! commit plans a module the same: link overrides
//! (`data/links/<game>/<module>.links`) and encounter overrides
//! (`data/encounters/<game>/<module>.encounter`). And stamps it with a hash
//! of the code that gathers a module's script library (`src/library.rs`, the
//! DLG, GFF and NCS readers) as `KJK_LIBRARY_GATHERER`: a stored gathering
//! is keyed by it (`docs/IMPORT.md`, slice 3).

use std::path::Path;

/// Writes `$OUT_DIR/<out>`: `FILES`, every `data/<folder>/<game>/<module>.<extension>`
/// as (game, module, text), sorted.
fn embed(folder: &str, extension: &str, out: &str) {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets it");
    let root = Path::new(&manifest).join("../../data").join(folder);
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
                if file.extension().is_some_and(|e| e == extension) {
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
    let mut text = String::from("pub(crate) const FILES: &[(&str, &str, &str)] = &[\n");
    for (game, module, path) in &files {
        text.push_str(&format!(
            "    ({game:?}, {module:?}, include_str!({:?})),\n",
            path.display().to_string()
        ));
    }
    text.push_str("];\n");
    let target = Path::new(&std::env::var("OUT_DIR").expect("cargo sets it")).join(out);
    std::fs::write(target, text).expect("OUT_DIR is writable");
}

fn main() {
    embed("links", "links", "links.rs");
    embed("encounters", "encounter", "encounters.rs");
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets it");
    let readers = Path::new(&manifest).join("../fmt_odyssey/src");
    let sources = [
        Path::new(&manifest).join("src/library.rs"),
        readers.join("dlg.rs"),
        readers.join("gff.rs"),
        readers.join("ncs.rs"),
    ];
    // FNV-1a over each file's bytes (a stamp, not a security hash).
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for path in &sources {
        println!("cargo:rerun-if-changed={}", path.display());
        for byte in std::fs::read(path).unwrap_or_else(|_| Vec::new()) {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
        }
    }
    println!("cargo:rustc-env=KJK_LIBRARY_GATHERER={hash:016x}");
}
