//! Writes the palettes' colours from `data/ui/tokens.json`, the one source
//! of the design tokens (docs/UI.md, "Design tokens"), and the names of the
//! screen modules, which every story file of the catalogue must match
//! (`catalogue::check`).

use std::fmt::Write;
use std::path::Path;

/// Each palette of the code: its token, and the constant written for it.
const PALETTES: [(&str, &str); 4] = [
    ("k1", "K1"),
    ("tsl", "TSL"),
    ("tsl-layout", "TSL_LAYOUT"),
    ("tsl-engine", "TSL_ENGINE"),
];

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets it");
    let out = std::env::var("OUT_DIR").expect("cargo sets it");
    let tokens = Path::new(&manifest).join("../../data/ui/tokens.json");
    let screens = Path::new(&manifest).join("src/screens");
    println!("cargo:rerun-if-changed={}", tokens.display());
    println!("cargo:rerun-if-changed={}", screens.display());
    std::fs::write(Path::new(&out).join("tokens.rs"), palettes(&tokens)).expect("OUT_DIR writes");
    std::fs::write(Path::new(&out).join("screen_modules.rs"), modules(&screens))
        .expect("OUT_DIR writes");
}

fn palettes(tokens: &Path) -> String {
    let text = std::fs::read_to_string(tokens).expect("data/ui/tokens.json reads");
    let tokens: serde_json::Value = serde_json::from_str(&text).expect("tokens.json is JSON");
    // A colour is written as the file writes it, so `0.639216` is the same
    // f32 as the literal `0.639_216` the palettes held before.
    let colour = |path: &str| -> String {
        let mut at = &tokens["color"];
        for part in path.split('.') {
            at = &at[part];
        }
        let components = at["$value"]["components"]
            .as_array()
            .unwrap_or_else(|| panic!("tokens.json has no colour {path}"));
        let components: Vec<String> = (components.iter())
            .map(|component| {
                let component = component
                    .as_f64()
                    .unwrap_or_else(|| panic!("{path}: a component is not a number"));
                format!("{component:?}")
            })
            .collect();
        assert_eq!(components.len(), 3, "{path}: not three components");
        format!("[{}]", components.join(", "))
    };
    let mut rust = String::new();
    for (token, name) in PALETTES {
        let _ = writeln!(
            rust,
            "pub const {name}: Palette = Palette {{ base: {}, hilight: {}, disabled: {}, panel: {} }};",
            colour(&format!("{token}.base")),
            colour(&format!("{token}.hilight")),
            colour(&format!("{token}.disabled")),
            colour(&format!("{token}.panel")),
        );
    }
    let _ = writeln!(rust, "pub const REFUSED: [f32; 3] = {};", colour("refused"));
    rust
}

fn modules(screens: &Path) -> String {
    let mut names: Vec<String> = std::fs::read_dir(screens)
        .expect("src/screens reads")
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.strip_suffix(".rs")
                .filter(|module| *module != "mod")
                .map(str::to_string)
        })
        .collect();
    names.sort();
    format!(
        "/// The modules of `screens`, which every story file must match.\npub const SCREEN_MODULES: [&str; {}] = {names:?};\n",
        names.len()
    )
}
