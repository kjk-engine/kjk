//! `kjk store`: the store every worktree shares (`docs/IMPORT.md`, the
//! `store` crate): where it is, what it holds, removing what no run used
//! for a while, and a check of how it reads and writes.

use std::process::ExitCode;
use std::time::Duration;

use serde_json::json;
use store::{Key, Store};

use crate::runtime::Report;

/// The machine's store, or why there is none.
pub(crate) fn opened() -> Result<Store, String> {
    Store::open().ok_or_else(|| "no store: set KJK_STORE (no home folder found)".to_string())
}

fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_000_000.0)
}

/// `kjk store path`.
pub fn path() -> ExitCode {
    match opened() {
        Ok(store) => {
            println!("{}", store.root().display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

/// `kjk store du`: what each kind of file takes.
pub fn du() -> ExitCode {
    let run = || -> Result<(), String> {
        let store = opened()?;
        let usage = store.usage().map_err(|e| e.to_string())?;
        println!("{}", store.root().display());
        let (mut files, mut bytes) = (0, 0);
        for (kind, used) in &usage {
            println!(
                "  {:>10}  {:>6} files  {kind}",
                megabytes(used.bytes),
                used.files
            );
            files += used.files;
            bytes += used.bytes;
        }
        println!("  {:>10}  {files:>6} files  in all", megabytes(bytes));
        Ok(())
    };
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

/// `kjk store gc`: removes the kinds a newer format superseded, and what
/// no run used for `days` days.
pub fn gc(days: u64, dry_run: bool) -> ExitCode {
    let run = || -> Result<(), String> {
        let store = opened()?;
        let unused = Duration::from_secs(days * 24 * 60 * 60);
        let removed = store.gc(unused, dry_run).map_err(|e| e.to_string())?;
        let verb = if dry_run { "would remove" } else { "removed" };
        for folder in &removed.folders {
            let kind = folder.strip_prefix(store.root()).unwrap_or(folder);
            println!("{verb} {} (a newer format is there)", kind.display());
        }
        let unused = format!("unused for {days} days");
        for (what, used) in [
            ("of superseded formats", removed.superseded),
            (unused.as_str(), removed.unused),
        ] {
            println!(
                "{verb} {} files ({}) {what}",
                used.files,
                megabytes(used.bytes)
            );
        }
        Ok(())
    };
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

/// `kjk store check`: the store's rules on a scratch root (never the
/// machine's): keys, a missing file, put and get, `get_or_make` making
/// once, two writers of one key, usage, superseded formats, and gc.
pub fn check() -> ExitCode {
    let mut report = Report::default();
    let root = std::env::temp_dir().join(format!("kjk-store-check-{}", std::process::id()));
    let run = |report: &mut Report| -> Result<(), String> {
        let store = Store::at(&root);
        let key = Key::of("check-v1", &[b"a", b"b"]);
        report.check(
            key == Key::of("check-v1", &[b"a", b"b"])
                && key != Key::of("check-v2", &[b"a", b"b"])
                && key != Key::of("check-v1", &[b"ab"]),
            format!(
                "a key names its maker's format and each input apart ({}…)",
                &key.as_str()[..12]
            ),
        );
        report.check(
            store.get("check/things", &key).is_none(),
            "a key never put is not there".to_string(),
        );
        store
            .put("check/things", &key, b"made")
            .map_err(|e| e.to_string())?;
        report.check(
            store.get("check/things", &key).as_deref() == Some(&b"made"[..]),
            "put, then got back as written".to_string(),
        );
        let mut made = 0;
        let other = Key::of("check-v1", &[b"c"]);
        for _ in 0..2 {
            let bytes = store.get_or_make("check/things", &other, || {
                made += 1;
                Ok::<_, String>(b"once".to_vec())
            })?;
            report.check(
                bytes == b"once",
                "get_or_make answers the bytes".to_string(),
            );
        }
        report.check(
            made == 1,
            format!("get_or_make made it once in two asks ({made})"),
        );
        // Two writers of one key at once: one whole file, no leftovers.
        let shared = Key::of("check-v1", &[b"shared"]);
        let payload = vec![7u8; 1 << 20];
        std::thread::scope(|scope| {
            for _ in 0..4 {
                scope.spawn(|| store.put("check/shared", &shared, &payload));
            }
        });
        let usage = store.usage().map_err(|e| e.to_string())?;
        let shared_usage = usage
            .iter()
            .find(|(kind, _)| kind == "check/shared")
            .map_or(store::Usage::default(), |(_, used)| *used);
        report.check(
            store.get("check/shared", &shared).as_deref() == Some(&payload[..])
                && shared_usage.files == 1,
            format!(
                "four writers of one key: one whole file of {} bytes, no temporary left",
                shared_usage.bytes
            ),
        );
        // Formats: the kinds a newer one supersedes go whole (v10 is newer
        // than v2, a kind without a version older than both); a lone one
        // stays, and so do folders not the store's kinds.
        let bakes =
            ["area", "area-v2", "area-v10", "library-v1"].map(|kind| format!("check/bake/{kind}"));
        for kind in &bakes {
            store.put(kind, &key, b"baked").map_err(|e| e.to_string())?;
        }
        let foreign = ["node-v16", "node-v18"]
            .map(|tool| store.root().join("check/tools").join(tool).join("bin"));
        for path in &foreign {
            store::write_atomically(path, b"theirs").map_err(|e| e.to_string())?;
        }
        let old = store.superseded().map_err(|e| e.to_string())?;
        let hour = Duration::from_secs(3600);
        let would = store.gc(hour, true).map_err(|e| e.to_string())?;
        let done = store.gc(hour, false).map_err(|e| e.to_string())?;
        let left: Vec<bool> = bakes
            .iter()
            .map(|kind| store.get(kind, &key).is_some())
            .collect();
        report.check(
            old == [&bakes[0], &bakes[1]].map(|kind| store.root().join(kind))
                && old == done.folders
                && would == done
                && done.superseded.files == 2
                && left == [false, false, true, true]
                && foreign.iter().all(|path| path.exists())
                && store.superseded().is_ok_and(|old| old.is_empty()),
            format!(
                "gc drops the kinds a newer format supersedes ({} of {} bakes), keeps the newest, a lone one and others' folders",
                done.superseded.files,
                bakes.len()
            ),
        );
        let all = store.gc(Duration::ZERO, false).map_err(|e| e.to_string())?;
        report.check(
            done.unused.files == 0
                && all.unused.files == 7
                && store.get("check/things", &key).is_none(),
            format!(
                "gc keeps what was used within the hour ({} to go), removes all when asked ({} files)",
                done.unused.files, all.unused.files
            ),
        );
        let machine = Store::root_from(|name| match name {
            "KJK_STORE" => None,
            "HOME" => Some("/home/someone".to_string()),
            "XDG_CACHE_HOME" | "LOCALAPPDATA" => None,
            _ => None,
        });
        let chosen =
            Store::root_from(|name| (name == "KJK_STORE").then(|| "/elsewhere".to_string()));
        report.check(
            machine.is_some_and(|root| root.starts_with("/home/someone"))
                && chosen == Some(std::path::PathBuf::from("/elsewhere")),
            "KJK_STORE wins; else the user's cache folder".to_string(),
        );
        Ok(())
    };
    let outcome = run(&mut report);
    let _ = std::fs::remove_dir_all(&root);
    if let Err(error) = outcome {
        eprintln!("{error}");
        return ExitCode::FAILURE;
    }
    report.finish(json!({ "root": root.display().to_string() }))
}
