//! Hot reload: the open story is staged again when a file it is made from
//! changes on disk.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// How often the files are looked at, in seconds.
const POLL: f32 = 0.5;

/// What a file or a folder was last changed at, and how many files a folder
/// holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Stamp {
    changed: Option<SystemTime>,
    files: usize,
}

fn stamp(path: &Path) -> Stamp {
    let Ok(meta) = std::fs::metadata(path) else {
        return Stamp::default();
    };
    if !meta.is_dir() {
        return Stamp {
            changed: meta.modified().ok(),
            files: 1,
        };
    }
    let mut stamp = Stamp::default();
    for entry in std::fs::read_dir(path).into_iter().flatten().flatten() {
        let changed = entry.metadata().and_then(|meta| meta.modified()).ok();
        stamp.files += 1;
        stamp.changed = stamp.changed.max(changed);
    }
    stamp
}

/// The files and folders that the story `module/…` is made from. A story
/// file, the captures of play and the design tokens are read when the story
/// is staged. The override layouts of https://github.com/kjk-engine/roadmap/issues/25
/// join this list, and their change reaches the same reload.
pub fn sources(module: &str) -> Vec<PathBuf> {
    let root = ui::catalogue::root();
    vec![
        ui::catalogue::folder().join(format!("{module}.toml")),
        ui::catalogue::captures(),
        root.join("data/ui/tokens.json"),
    ]
}

#[derive(Default)]
pub struct Watch {
    seen: Vec<(PathBuf, Stamp)>,
    waited: f32,
}

impl Watch {
    /// The files of `module` taken as they are now.
    pub fn of(module: &str) -> Watch {
        let mut watch = Watch::default();
        watch.take(module);
        watch
    }

    pub fn take(&mut self, module: &str) {
        self.seen = sources(module)
            .into_iter()
            .map(|path| {
                let at = stamp(&path);
                (path, at)
            })
            .collect();
    }

    /// The names of the files that changed since the last look, looked at
    /// every `POLL` seconds.
    pub fn changed(&mut self, dt: f32) -> Vec<String> {
        self.waited += dt;
        if self.waited < POLL {
            return Vec::new();
        }
        self.waited = 0.0;
        let mut changed = Vec::new();
        for (path, before) in &mut self.seen {
            let now = stamp(path);
            if now != *before {
                *before = now;
                changed.push(
                    path.file_name()
                        .map_or_else(String::new, |name| name.to_string_lossy().into_owned()),
                );
            }
        }
        changed
    }
}
