//! `pending.rs`: how many of the branch's `bumps/` files bump each of the
//! sim's formats, added to its version so a branch's encoding is refused by
//! `main`'s and the other way round, before its merge numbers it
//! (`bumps/README.md`); and `since`, the version each pending bump of a
//! format saves hold gives it, by the bump's name and its place in the order
//! the files reached `main` (`bumps::arrival`), for decoders to read the
//! layouts before it (`crate::since`); and `oldest`, the version a pending
//! `breaking` bump refuses saves before (0 for none), which raises each
//! format's `OLDEST` before the merge does.

use std::path::Path;

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR");
    let root = Path::new(&manifest).join("../..");
    println!("cargo:rerun-if-changed={}", root.join(bumps::DIR).display());
    println!(
        "cargo:rerun-if-changed={}",
        root.join(bumps::SINCE).display()
    );
    for format in bumps::FORMATS.iter().filter(|format| format.stored) {
        println!(
            "cargo:rerun-if-changed={}",
            root.join(format.file).display()
        );
    }
    let pending = bumps::pending(&root).unwrap_or_else(|error| panic!("{error}"));
    let mut out = String::new();
    for format in bumps::FORMATS {
        out += &format!(
            "pub const {}: u32 = {};\n",
            format.name.to_uppercase(),
            bumps::count(&pending, format.name)
        );
    }
    let since = std::fs::read_to_string(root.join(bumps::SINCE))
        .unwrap_or_else(|error| panic!("{}: {error}", bumps::SINCE));
    bumps::clashes(&pending, &since).unwrap_or_else(|error| panic!("{error}"));
    let (mut named, mut oldest) = (String::new(), String::new());
    for format in bumps::FORMATS.iter().filter(|format| format.stored) {
        let base = bumps::base(&root, format).unwrap_or_else(|error| panic!("{error}"));
        named += &format!("    pub mod {} {{\n", format.name);
        for (name, place) in bumps::pending_since(&pending, format.name) {
            named += &format!(
                "        pub const {name}: crate::Since = crate::Since(crate::Stored::{}, {});\n",
                bumps::stored_variant(format),
                base + place
            );
        }
        named += "    }\n";
        oldest += &format!(
            "    pub const {}: u32 = {};\n",
            format.name.to_uppercase(),
            bumps::breaking_since(&pending, format.name, base).unwrap_or(0)
        );
    }
    out += &format!("pub mod since {{\n{named}}}\n");
    out += &format!("pub mod oldest {{\n{oldest}}}\n");
    let path = Path::new(&std::env::var("OUT_DIR").expect("cargo sets OUT_DIR")).join("pending.rs");
    std::fs::write(&path, out).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
}
