//! The project's own scripts: each `NAME.nss` at the top of the project's
//! folder that has a body (`main` or `StartingConditional`) compiled to
//! `NAME.ncs` beside it (`script_ncs::compile`), which the game then runs
//! over the install's script of that name, as the folder is the highest
//! layer of the install's files, and its debug notes to `NAME.ndb`
//! (`script_ncs::compile::Debug`, which `kjk try-script --debug` reads).
//! A file of functions alone is an include,
//! read by the scripts that include it and compiled with them.

use std::path::{Path, PathBuf};

use script_ncs::Catalog;

/// The longest name a KOTOR resource has.
const NAME_LENGTH: usize = 16;

/// One script of the project, compiled or not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compiled {
    /// The script's name, lowercase, without `.nss`.
    pub name: String,
    /// The `.ncs` written, or why there is none: a mistake with its file
    /// and line, or a write that failed. A script that does not compile
    /// keeps the `.ncs` it had.
    pub outcome: Result<PathBuf, String>,
}

/// The `.nss` files at the top of `folder`, by lowercase name.
pub fn sources(folder: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut found: Vec<(String, PathBuf)> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .filter_map(|path| {
            let extension = path.extension()?.to_str()?.to_ascii_lowercase();
            let name = path.file_stem()?.to_str()?.to_ascii_lowercase();
            (extension == "nss").then_some((name, path))
        })
        .collect();
    found.sort();
    found
}

/// Compiles every script of `folder` that has a body. An include is read
/// from the folder first, then from `install` (the game's own includes).
pub fn compile_all(
    folder: &Path,
    install: &dyn Fn(&str) -> Option<Vec<u8>>,
    catalog: &Catalog,
) -> Vec<Compiled> {
    let own = sources(folder);
    let read = |name: &str| {
        let name = name.to_ascii_lowercase();
        own.iter()
            .find(|(own_name, _)| *own_name == name)
            .and_then(|(_, path)| std::fs::read(path).ok())
            .or_else(|| install(&name))
    };
    let mut compiled = Vec::new();
    for (name, path) in &own {
        let Ok(source) = std::fs::read(path) else {
            continue;
        };
        if !script_ncs::compile::has_entry(&source) {
            continue;
        }
        let outcome = if name.len() > NAME_LENGTH {
            Err(format!(
                "{name}.nss: a script's name is at most {NAME_LENGTH} characters"
            ))
        } else {
            script_ncs::compile::compile_debug(name, &source, &read, catalog)
                .map_err(|error| error.to_string())
                .and_then(|(ncs, debug)| {
                    // The debug notes first (`kjk try-script --debug`), then
                    // the program, each written beside and moved over the
                    // old file, so a failed write leaves the old one whole.
                    let notes = folder.join(format!("{name}.ndb"));
                    let out = folder.join(format!("{name}.ncs"));
                    let put = |path: &Path, bytes: &[u8]| {
                        let fresh = path.with_extension("new");
                        std::fs::write(&fresh, bytes)
                            .and_then(|()| std::fs::rename(&fresh, path))
                            .map_err(|error| format!("{}: {error}", path.display()))
                    };
                    put(&notes, debug.write().as_bytes())?;
                    put(&out, &ncs.write())?;
                    Ok(out)
                })
        };
        compiled.push(Compiled {
            name: name.clone(),
            outcome,
        });
    }
    compiled
}
