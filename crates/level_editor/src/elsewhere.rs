//! Templates of the game's other levels.
//!
//! A level's own archives hold templates no other level reads: Korriban's
//! tomb creatures are in the tombs' archives only. The editor lists what
//! each of the game's levels has that the edited level does not
//! (`Elsewhere`), and copies such a template into the project when an
//! object is made from it (`import`).
//!
//! A template is copied with what it names and the edited level does not
//! have either: its items, its conversation, the creatures of an encounter,
//! and the compiled scripts of all of them. The items, conversations and
//! creatures become the project's own, so an undo takes them out again.
//! The scripts are written to the project's folder as the game has them.

use std::collections::{BTreeMap, BTreeSet};

use fmt_odyssey::ResType;
use fmt_odyssey::gff::{Gff, GffStruct, GffValue};
use level_edit::{Edit, Template};
use vfs::Game;
use vfs::resolver::Resolver;

use crate::state::EditorState;

/// The kinds of file looked for in the other levels: the templates objects
/// are made from, and what those name.
const TYPES: [ResType; 11] = [
    ResType::UTP,
    ResType::UTC,
    ResType::UTD,
    ResType::UTW,
    ResType::UTT,
    ResType::UTS,
    ResType::UTM,
    ResType::UTE,
    ResType::UTI,
    ResType::DLG,
    ResType::NCS,
];

/// What the game's other levels have that the edited level does not.
pub(crate) struct Elsewhere {
    /// Each such file by name and kind: the first level, by name, that has
    /// it.
    found: BTreeMap<(String, ResType), String>,
}

impl Elsewhere {
    /// Looks through the archives of every level in `modules`. `own` is the
    /// edited level's resolver: what it reads is left out.
    pub(crate) fn of(game: Game, modules: &[String], own: &Resolver) -> Elsewhere {
        let began = std::time::Instant::now();
        let discovery = kjk_game::discover();
        let mut found = BTreeMap::new();
        for module in modules {
            let Ok(resolver) = kjk_game::viewer::resolver(&discovery, game, Some(module)) else {
                continue;
            };
            for restype in TYPES {
                for name in resolver.module_names(restype) {
                    if own.find(&name, restype).is_none() {
                        found
                            .entry((name, restype))
                            .or_insert_with(|| module.clone());
                    }
                }
            }
        }
        eprintln!(
            "the other levels hold {} templates, scripts and conversations this level has not ({} ms)",
            found.len(),
            began.elapsed().as_millis()
        );
        Elsewhere { found }
    }

    /// The level whose archives hold the file, when only another level's
    /// do.
    pub(crate) fn module_of(&self, name: &str, restype: ResType) -> Option<&str> {
        self.found
            .get(&(name.to_ascii_lowercase(), restype))
            .map(String::as_str)
    }

    /// Every file of `restype` only another level has, with that level, in
    /// the names' order.
    pub(crate) fn names(&self, restype: ResType) -> impl Iterator<Item = (&str, &str)> {
        self.found
            .iter()
            .filter(move |((_, kind), _)| *kind == restype)
            .map(|((name, _), module)| (name.as_str(), module.as_str()))
    }
}

/// A template of another level, with what goes with it.
pub(crate) struct Import {
    /// The template itself.
    pub gff: Gff,
    /// The level it was read from.
    pub module: String,
    /// The edits that make it and what it names the project's own, to be
    /// applied before the object is added.
    pub edits: Vec<Edit>,
    /// The compiled scripts to write to the project's folder: each name
    /// and its bytes.
    pub scripts: Vec<(String, Vec<u8>)>,
    /// How many items, conversations and creatures come with it.
    pub with: (usize, usize, usize),
}

/// Every resource a GFF names, lowercase, once each.
fn names_in(item: &GffStruct, found: &mut BTreeSet<String>) {
    for field in &item.fields {
        match &field.value {
            GffValue::ResRef(name) if !name.is_empty() => {
                found.insert(String::from_utf8_lossy(name).to_ascii_lowercase());
            }
            GffValue::Struct(inner) => names_in(inner, found),
            GffValue::List(items) => items.iter().for_each(|item| names_in(item, found)),
            _ => {}
        }
    }
}

/// Reads the template `name` of `restype` from the other level that has it,
/// with what it names. `None` when no other level has it. `as_template` is
/// whether the template itself becomes one of the project's (a door and a
/// waypoint are read whole into their object instead).
pub(crate) fn import(
    state: &EditorState,
    name: &str,
    restype: ResType,
    as_template: bool,
) -> Option<Result<Import, String>> {
    let elsewhere = state.elsewhere();
    let name = name.to_ascii_lowercase();
    let module = elsewhere.module_of(&name, restype)?.to_string();
    let discovery = kjk_game::discover();
    let source = match kjk_game::viewer::resolver(&discovery, state.setup.game, Some(&module)) {
        Ok(source) => source,
        Err(why) => return Some(Err(why)),
    };
    let read = |name: &str, restype: ResType| {
        (source.read(name, restype)).and_then(|bytes| Gff::parse(&bytes).ok())
    };
    let Some(top) = read(&name, restype) else {
        return Some(Err(format!("{name} of {module} does not read")));
    };
    let project = &state.editor.project;
    let mut import = Import {
        gff: top.clone(),
        module: module.clone(),
        edits: Vec::new(),
        scripts: Vec::new(),
        with: (0, 0, 0),
    };
    if as_template {
        import.edits.push(Edit::ImportTemplate {
            name: name.clone(),
            template: Box::new(Template {
                restype,
                from: name.clone(),
                gff: top.clone(),
            }),
        });
    }
    let mut seen = BTreeSet::from([(name, restype)]);
    let mut queue = vec![(top, restype)];
    while let Some((gff, parent)) = queue.pop() {
        let mut named = BTreeSet::new();
        names_in(&gff.root, &mut named);
        for name in named {
            // An encounter names its creatures. Nothing else names one.
            let kinds: &[ResType] = if parent == ResType::UTE {
                &[ResType::UTI, ResType::DLG, ResType::NCS, ResType::UTC]
            } else {
                &[ResType::UTI, ResType::DLG, ResType::NCS]
            };
            for &kind in kinds {
                // Only what no file of the edited level or of the project
                // is, and what the template's own level has.
                let had = project.templates.contains_key(&name)
                    || (kind == ResType::DLG && project.conversations.contains_key(&name));
                if had
                    || elsewhere.module_of(&name, kind).is_none()
                    || source.find(&name, kind).is_none()
                    || !seen.insert((name.clone(), kind))
                {
                    continue;
                }
                if kind == ResType::NCS {
                    if let Some(bytes) = source.read(&name, kind) {
                        import.scripts.push((name.clone(), bytes));
                    }
                    continue;
                }
                let Some(gff) = read(&name, kind) else {
                    continue;
                };
                let template = Box::new(Template {
                    restype: kind,
                    from: name.clone(),
                    gff: gff.clone(),
                });
                match kind {
                    ResType::UTI => {
                        import.with.0 += 1;
                        import.edits.push(Edit::SetTemplate {
                            name: name.clone(),
                            template,
                        });
                    }
                    ResType::DLG => {
                        import.with.1 += 1;
                        import.edits.push(Edit::NewConversation {
                            name: name.clone(),
                            base: Some(gff.clone()),
                        });
                    }
                    _ => {
                        import.with.2 += 1;
                        import.edits.push(Edit::ImportTemplate {
                            name: name.clone(),
                            template,
                        });
                    }
                }
                queue.push((gff, kind));
            }
        }
    }
    Some(Ok(import))
}

/// Writes an import's compiled scripts to the project's folder. A script
/// the folder has already is left as it is.
pub(crate) fn write_scripts(state: &EditorState, import: &Import) -> Result<usize, String> {
    let folder = &state.editor.project.folder;
    let mut written = 0;
    for (name, bytes) in &import.scripts {
        let path = folder.join(format!("{name}.ncs"));
        if path.exists() {
            continue;
        }
        std::fs::write(&path, bytes).map_err(|error| format!("{}: {error}", path.display()))?;
        written += 1;
    }
    Ok(written)
}
