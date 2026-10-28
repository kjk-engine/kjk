use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

/// Every file under a root, keyed by its lowercased `/`-separated relative
/// path. Linux and macOS installs mix `Override/` and `override/`; lookups
/// through the index behave the same on every OS.
#[derive(Debug, Default)]
pub struct DirIndex {
    files: BTreeMap<String, PathBuf>,
    collisions: Vec<CaseCollision>,
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
