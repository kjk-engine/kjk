use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use crate::Install;

/// Every file under a root, keyed by its lowercased `/`-separated relative
/// path. Linux and macOS installs mix `Override/` and `override/`; lookups
/// through the index behave the same on every OS.
///
/// An install's index (`of_install`) also holds the Steam Workshop items
/// the game loads, each indexed the same way (`workshop`); the lookups here
/// are the game folder's alone, and the resolver ranks the items' files.
/// A Jedi Academy install's names its mod folder (`ja_mod`), whose files
/// are in the index under its name.
#[derive(Debug, Default)]
pub struct DirIndex {
    files: BTreeMap<String, PathBuf>,
    collisions: Vec<CaseCollision>,
    workshop: Vec<WorkshopFolder>,
    ja_mod: Option<String>,
}

/// A Steam Workshop item's folder, indexed.
#[derive(Debug)]
pub struct WorkshopFolder {
    pub id: u64,
    pub files: DirIndex,
}

/// One copy of a file (`DirIndex::copies`): its name (after the item's
/// label, for a Workshop item's), where it is, and whose it is.
#[derive(Debug, Clone)]
pub struct FileCopy<'a> {
    pub name: String,
    pub path: &'a Path,
    /// The Workshop item's id; `None` for the game folder's.
    pub item: Option<u64>,
}

impl WorkshopFolder {
    /// What the item's files are named by, before their path in it
    /// (`workshop/485537937`): its layers' names, its fingerprint group's.
    pub fn label(&self) -> String {
        format!("workshop/{}", self.id)
    }
}

/// Two files whose paths differ only in case. The first one found is kept;
/// which one that is depends on the filesystem, so a collision is reported
/// rather than resolved silently.
#[derive(Debug, Clone)]
pub struct CaseCollision {
    pub key: String,
    pub kept: PathBuf,
    pub ignored: PathBuf,
}

impl DirIndex {
    /// The install's data folder and the Workshop items its game loads
    /// (`Install::workshop_items`), in their load order; Jedi Academy's
    /// mod folder when one is named and found (`Install::ja_mod_folder`).
    pub fn of_install(install: &Install) -> io::Result<DirIndex> {
        let mut index = DirIndex::build(&install.data_root)?;
        for item in install.workshop_items().items {
            let files = DirIndex::build(&item.folder)?;
            index.workshop.push(WorkshopFolder { id: item.id, files });
        }
        if let Ok(Some((folder, _))) = install.ja_mod_folder() {
            index.ja_mod = Some(folder);
        }
        Ok(index)
    }

    /// This index with `folder` (a folder at its root, any case) as Jedi
    /// Academy's mod folder.
    pub fn with_ja_mod(mut self, folder: &str) -> DirIndex {
        self.ja_mod = Some(normalize(folder));
        self
    }

    /// Jedi Academy's mod folder, lowercased (`md`): its archives and files
    /// load after `base/`'s (`ja::JaStack`).
    pub fn ja_mod(&self) -> Option<&str> {
        self.ja_mod.as_deref()
    }

    /// This index with `items` as its Workshop items, in load order.
    pub fn with_workshop(mut self, items: Vec<WorkshopFolder>) -> DirIndex {
        self.workshop = items;
        self
    }

    /// The Workshop items, lowest first: of two holding a file, the last
    /// one's wins (`LoadSteamMods`, journal 2026-11-05-override-precedence).
    pub fn workshop(&self) -> &[WorkshopFolder] {
        &self.workshop
    }

    /// The game folder's file at `relative` (lowercase) and each Workshop
    /// item's, the game folder's first, then the items' in load order.
    pub fn copies(&self, relative: &str) -> Vec<FileCopy<'_>> {
        let game = self.get(relative).map(|path| FileCopy {
            name: relative.to_string(),
            path,
            item: None,
        });
        let items = self.workshop.iter().filter_map(|item| {
            Some(FileCopy {
                name: format!("{}/{relative}", item.label()),
                path: item.files.get(relative)?,
                item: Some(item.id),
            })
        });
        game.into_iter().chain(items).collect()
    }

    pub fn build(root: &Path) -> io::Result<DirIndex> {
        let mut index = DirIndex::default();
        let mut pending = vec![root.to_path_buf()];
        while let Some(dir) = pending.pop() {
            let mut entries = std::fs::read_dir(&dir)?.collect::<io::Result<Vec<_>>>()?;
            entries.sort_by_key(|entry| entry.file_name());
            for entry in entries {
                let path = entry.path();
                if entry.file_type()?.is_dir() {
                    pending.push(path);
                    continue;
                }
                let relative = path
                    .strip_prefix(root)
                    .expect("walked paths stay under the root");
                let key = normalize(&relative.to_string_lossy());
                match index.files.get(&key) {
                    Some(kept) => index.collisions.push(CaseCollision {
                        key,
                        kept: kept.clone(),
                        ignored: path,
                    }),
                    None => {
                        index.files.insert(key, path);
                    }
                }
            }
        }
        Ok(index)
    }

    /// Looks up a relative path in any case, with `/` or `\` separators.
    pub fn get(&self, relative: &str) -> Option<&Path> {
        self.files.get(&normalize(relative)).map(PathBuf::as_path)
    }

    /// Files directly inside `dir` (a relative path in any case), as
    /// (lowercased file name, real path), sorted by name.
    pub fn files_in(&self, dir: &str) -> Vec<(&str, &Path)> {
        let prefix = match normalize(dir) {
            dir if dir.is_empty() => String::new(),
            dir => format!("{dir}/"),
        };
        self.files
            .range(prefix.clone()..)
            .take_while(|(key, _)| key.starts_with(&prefix))
            .filter_map(|(key, path)| {
                let name = &key[prefix.len()..];
                (!name.contains('/')).then_some((name, path.as_path()))
            })
            .collect()
    }

    /// Every file as (lowercased relative path, real path), sorted.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Path)> {
        self.files
            .iter()
            .map(|(key, path)| (key.as_str(), path.as_path()))
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn collisions(&self) -> &[CaseCollision] {
        &self.collisions
    }
}

fn normalize(relative: &str) -> String {
    fmt_ja::game_path(relative).trim_matches('/').to_string()
}
