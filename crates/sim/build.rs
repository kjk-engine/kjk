//! `pending.rs`: how many of the branch's `bumps/` files bump each of the
//! sim's formats, added to its version so a branch's encoding is refused by
//! `main`'s and the other way round, before its merge numbers it
//! (`bumps/README.md`); and `since`, the version each pending bump of a
//! format saves hold gives it, by the bump's name, for decoders to read the
//! layouts before it (`crate::since`).

use std::path::Path;

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR");
    let root = Path::new(&manifest).join("../..");
    println!("cargo:rerun-if-changed={}", root.join(bumps::DIR).display());
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
    out += "pub mod since {\n";
    for format in bumps::FORMATS.iter().filter(|format| format.stored) {
        let base = bumps::base(&root, format).unwrap_or_else(|error| panic!("{error}"));
        out += &format!("    pub mod {} {{\n", format.name);
        for (name, place) in bumps::pending_since(&pending, format.name) {
            out += &format!("        pub const {name}: u32 = {};\n", base + place);
        }
        out += "    }\n";
    }
    out += "}\n";
    let path = Path::new(&std::env::var("OUT_DIR").expect("cargo sets OUT_DIR")).join("pending.rs");
    std::fs::write(&path, out).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
}
