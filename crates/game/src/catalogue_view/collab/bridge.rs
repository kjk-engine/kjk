//! The agent bridge: a state file the catalogue keeps up to date, a command
//! file it reads, and a log of every change with its source (docs/UI.md,
//! "Working with the catalogue"). An agent uses them on a catalogue that is
//! open in a window without touching the window.

use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::{Value, json};

/// How often the command file is looked at, in seconds.
const POLL: f32 = 0.1;

pub struct Bridge {
    /// This catalogue's entry among the pointers, removed when it exits.
    pointer: PathBuf,
    pub state: PathBuf,
    pub commands: PathBuf,
    pub log: PathBuf,
    started: Instant,
    /// How far into the command file the catalogue has read.
    offset: u64,
    /// How many commands it has read.
    pub read: u64,
    waited: f32,
    /// What the last command did, as the state file says it.
    pub last: Value,
    /// Lines written to the log so far.
    logged: u64,
}

impl Bridge {
    /// The three files in `folder`, empty, and a pointer to them in
    /// `pointers`, a file named for this process. A pointer of a process
    /// that is gone is removed, as a crash leaves it.
    pub fn open(folder: &Path, pointers: &Path) -> Result<Bridge, String> {
        let why = |path: &Path, error: std::io::Error| format!("{}: {error}", path.display());
        std::fs::create_dir_all(folder).map_err(|error| why(folder, error))?;
        let folder = std::fs::canonicalize(folder).map_err(|error| why(folder, error))?;
        let pointer = pointers.join(format!("{}.json", std::process::id()));
        let bridge = Bridge {
            pointer,
            state: folder.join("state.json"),
            commands: folder.join("commands.txt"),
            log: folder.join("log.jsonl"),
            started: Instant::now(),
            offset: 0,
            read: 0,
            waited: 0.0,
            last: Value::Null,
            logged: 0,
        };
        for path in [&bridge.commands, &bridge.log] {
            std::fs::write(path, "").map_err(|error| why(path, error))?;
        }
        let paths = json!({
            "state": bridge.state,
            "commands": bridge.commands,
            "log": bridge.log,
            "pid": std::process::id(),
        });
        std::fs::create_dir_all(pointers).map_err(|error| why(pointers, error))?;
        forget_dead(pointers);
        std::fs::write(&bridge.pointer, paths.to_string() + "\n")
            .map_err(|error| why(&bridge.pointer, error))?;
        Ok(bridge)
    }

    /// One line of the log: the change, where it came from and its details.
    pub fn log(&mut self, source: &str, change: &str, detail: Value) {
        let line = json!({
            "n": self.logged,
            "seconds": (self.started.elapsed().as_secs_f64() * 100.0).round() / 100.0,
            "source": source,
            "change": change,
            "detail": detail,
        });
        self.logged += 1;
        let written = std::fs::OpenOptions::new()
            .append(true)
            .open(&self.log)
            .and_then(|mut file| writeln!(file, "{line}"));
        if let Err(error) = written {
            bevy::log::warn!(target: "catalogue", "{}: {error}", self.log.display());
        }
    }

    /// The command lines written since the last look, at most every
    /// `POLL` seconds. A last line without its newline is left for the next
    /// look, and a file that became shorter is read from its start.
    pub fn new_commands(&mut self, dt: f32) -> Vec<String> {
        self.waited += dt;
        if self.waited < POLL {
            return Vec::new();
        }
        self.waited = 0.0;
        let Ok(mut file) = std::fs::File::open(&self.commands) else {
            return Vec::new();
        };
        let length = file.metadata().map_or(0, |meta| meta.len());
        if length < self.offset {
            self.offset = 0;
        }
        if length == self.offset || file.seek(std::io::SeekFrom::Start(self.offset)).is_err() {
            return Vec::new();
        }
        let mut text = Vec::new();
        if file.read_to_end(&mut text).is_err() {
            return Vec::new();
        }
        let whole = text
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |at| at + 1);
        self.offset += whole as u64;
        let lines: Vec<String> = String::from_utf8_lossy(&text[..whole])
            .lines()
            .map(|line| line.trim().to_string())
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .collect();
        self.read += lines.len() as u64;
        lines
    }

    /// `state` written whole, so that a reader never sees half of it.
    pub fn write_state(&self, state: &Value) {
        let text = serde_json::to_string_pretty(state).unwrap_or_else(|_| "{}".into()) + "\n";
        let temporary = self.state.with_extension("json.tmp");
        let written = std::fs::write(&temporary, text)
            .and_then(|()| std::fs::rename(&temporary, &self.state));
        if let Err(error) = written {
            bevy::log::warn!(target: "catalogue", "{}: {error}", self.state.display());
        }
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.pointer);
    }
}

/// The pointers in `pointers` whose process is not running any more, taken
/// away.
fn forget_dead(pointers: &Path) {
    use sysinfo::{Pid, ProcessesToUpdate, System};
    let Ok(entries) = std::fs::read_dir(pointers) else {
        return;
    };
    let mut system = System::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(pid) = (path.file_stem().and_then(|stem| stem.to_str()))
            .filter(|_| path.extension().is_some_and(|ext| ext == "json"))
            .and_then(|stem| stem.parse::<u32>().ok())
        else {
            continue;
        };
        let pid = Pid::from_u32(pid);
        system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
        if system.process(pid).is_none() {
            let _ = std::fs::remove_file(&path);
        }
    }
}
