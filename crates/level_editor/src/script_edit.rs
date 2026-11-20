//! The script viewer as a place to write scripts: the project's scripts
//! held as drafts while they are edited, written to the project's folder
//! (`NAME.nss`), compiled (`level_edit::scripts`) and tried
//! (`kjk try-script`, which runs one in the module installed headless and
//! prints its trace) on a thread of its own.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, channel};

/// A script's text as the editor holds it, and as it was last read from or
/// written to the project's folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    pub text: String,
    pub saved: String,
}

impl Draft {
    pub fn changed(&self) -> bool {
        self.text != self.saved
    }
}

/// A new script's text: a `main` to run on an event, or a
/// `StartingConditional` that answers a conversation's question.
pub fn template(name: &str, conditional: bool) -> String {
    if conditional {
        format!(
            "// {name}: a conversation's condition. It answers TRUE to show the\n\
             // line it is set on.\n\
             \n\
             int StartingConditional()\n\
             {{\n\
             \x20   return TRUE;\n\
             }}\n"
        )
    } else {
        format!(
            "// {name}\n\
             \n\
             void main()\n\
             {{\n\
             \x20   object oPC = GetFirstPC();\n\
             \x20   SendMessageToPC(oPC, \"{name} ran for \" + GetTag(OBJECT_SELF));\n\
             }}\n"
        )
    }
}

/// Why `name` cannot be a script's name, if it cannot: KOTOR's names are
/// 1 to 16 lowercase letters, digits and `_`.
pub fn name_problem(name: &str) -> Option<String> {
    if name.is_empty() || name.len() > 16 {
        return Some(format!("{name:?}: a script's name is 1 to 16 characters"));
    }
    if !name
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Some(format!(
            "{name:?}: a script's name is lowercase letters, digits and _"
        ));
    }
    None
}

/// The names of the scripts in the project's folder: each `NAME.nss` and
/// each `NAME.ncs`, lowercase.
pub fn project_scripts(folder: &Path) -> std::collections::BTreeSet<String> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Default::default();
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let extension = path.extension()?.to_str()?.to_ascii_lowercase();
            (extension == "nss" || extension == "ncs")
                .then(|| Some(path.file_stem()?.to_str()?.to_ascii_lowercase()))?
        })
        .collect()
}

/// Writes `draft` to `NAME.nss` in `folder`, beside then over the old
/// file.
pub fn write(folder: &Path, name: &str, draft: &mut Draft) -> Result<PathBuf, String> {
    let path = folder.join(format!("{name}.nss"));
    let fresh = path.with_extension("nss.new");
    // The games' code page: a byte a character.
    let bytes: Vec<u8> = draft
        .text
        .chars()
        .map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?'))
        .collect();
    std::fs::write(&fresh, bytes)
        .and_then(|()| std::fs::rename(&fresh, &path))
        .map_err(|error| format!("{}: {error}", path.display()))?;
    draft.saved = draft.text.clone();
    Ok(path)
}

/// What a try asks for, gathered before its thread starts.
pub struct Plan {
    pub program: PathBuf,
    pub project: String,
    pub game: String,
    pub module: String,
    pub script: String,
    /// The tag of the object it runs for; the player when `None`.
    pub on: Option<String>,
    pub seconds: f32,
}

/// A try running, then its result.
pub struct Try {
    pub script: String,
    pub on: Option<String>,
    pub started: std::time::Instant,
    done: Mutex<Receiver<Result<Outcome, String>>>,
}

/// How a try went: whether the script ran without a fault, and what `kjk
/// try-script` printed.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub script: String,
    pub on: Option<String>,
    pub ok: bool,
    pub lines: Vec<String>,
}

impl Try {
    pub fn start(plan: Plan) -> Try {
        let (send, done) = channel();
        let (script, on) = (plan.script.clone(), plan.on.clone());
        std::thread::spawn(move || {
            let mut command = std::process::Command::new(&plan.program);
            command.args([
                "--project",
                &plan.project,
                "try-script",
                &plan.game,
                &plan.module,
                &plan.script,
                "--seconds",
                &plan.seconds.to_string(),
            ]);
            if let Some(on) = &plan.on {
                command.args(["--on", on]);
            }
            let result = command
                .output()
                .map_err(|error| format!("kjk try-script did not start: {error}"))
                .map(|output| {
                    let mut lines: Vec<String> = String::from_utf8_lossy(&output.stdout)
                        .lines()
                        // The line that names the project opened.
                        .filter(|line| !line.starts_with("project "))
                        .map(str::to_string)
                        .collect();
                    lines.extend(
                        String::from_utf8_lossy(&output.stderr)
                            .lines()
                            .filter(|line| {
                                !["INFO", "WARN", "DEBUG"]
                                    .iter()
                                    .any(|level| line.contains(level))
                            })
                            .map(str::to_string),
                    );
                    Outcome {
                        script: plan.script.clone(),
                        on: plan.on.clone(),
                        ok: output.status.success(),
                        lines,
                    }
                });
            let _ = send.send(result);
        });
        Try {
            script,
            on,
            started: std::time::Instant::now(),
            done: Mutex::new(done),
        }
    }

    /// The result, once the try is over.
    pub fn poll(&self) -> Option<Result<Outcome, String>> {
        match self.done.lock() {
            Ok(done) => done.try_recv().ok(),
            Err(_) => Some(Err("the try's thread failed".into())),
        }
    }
}
