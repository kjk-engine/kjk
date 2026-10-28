//! `pending.rs`: how many of the branch's `bumps/` files bump each of the
//! sim's formats, added to its version so a branch's encoding is refused by
//! `main`'s and the other way round, before its merge numbers it
//! (`bumps/README.md`).

use std::path::Path;

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR");
    let root = Path::new(&manifest).join("../..");
    println!("cargo:rerun-if-changed={}", root.join(bumps::DIR).display());
    let pending = bumps::pending(&root).unwrap_or_else(|error| panic!("{error}"));
    let mut out = String::new();
    for format in bumps::FORMATS {
        out += &format!(
            "pub const {}: u32 = {};\n",
            format.name.to_uppercase(),
            bumps::count(&pending, format.name)
        );
    }
    let path = Path::new(&std::env::var("OUT_DIR").expect("cargo sets OUT_DIR")).join("pending.rs");
    std::fs::write(&path, out).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
}
