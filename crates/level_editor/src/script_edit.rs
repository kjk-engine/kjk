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
/// file. The games' code page has a byte for each character. A character
/// it does not have (a curly quote, a letter of another alphabet) is
/// written as `?`. The file's path, and how many characters were written
/// so.
pub fn write(folder: &Path, name: &str, draft: &mut Draft) -> Result<(PathBuf, usize), String> {
    let path = folder.join(format!("{name}.nss"));
    let fresh = path.with_extension("nss.new");
    let mut replaced = 0;
    let bytes: Vec<u8> = draft
        .text
        .chars()
        .map(|c| {
            u8::try_from(u32::from(c)).unwrap_or_else(|_| {
                replaced += 1;
                b'?'
            })
        })
        .collect();
    std::fs::write(&fresh, bytes)
        .and_then(|()| std::fs::rename(&fresh, &path))
        .map_err(|error| format!("{}: {error}", path.display()))?;
    draft.saved = draft.text.clone();
    Ok((path, replaced))
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
    /// The script's debug notes (`NAME.ndb`): the run is recorded line by
    /// line (`kjk try-script --debug`).
    pub debug: Option<PathBuf>,
    /// A conversation is tried, not a script (`kjk try-conversation`): the
    /// player talks to `on` and answers by these picks.
    pub conversation: Option<Vec<u8>>,
}

/// A debugged run where a source line starts (`kjk try-script --debug`):
/// the file and line, the function, the calls that led there, and the
/// locals and globals, each as name, type and value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stop {
    pub file: String,
    pub line: u32,
    pub function: String,
    pub calls: String,
    pub locals: Vec<(String, String, String)>,
    pub globals: Vec<(String, String, String)>,
}

/// The stops `kjk try-script --debug` printed, and the lines before them.
pub fn stops_of(lines: &[String]) -> (Vec<String>, Vec<Stop>) {
    let Some(start) = lines.iter().position(|line| line.starts_with("  debug: ")) else {
        return (lines.to_vec(), Vec::new());
    };
    let mut stops: Vec<Stop> = Vec::new();
    let mut rest = Vec::new();
    for line in &lines[start + 1..] {
        if let Some(stop) = line.strip_prefix("    stop ") {
            let mut words = stop.splitn(3, ' ');
            let (Some(file), Some(at), Some(function)) = (words.next(), words.next(), words.next())
            else {
                continue;
            };
            stops.push(Stop {
                file: file.to_string(),
                line: at.parse().unwrap_or(0),
                function: function.to_string(),
                calls: String::new(),
                locals: Vec::new(),
                globals: Vec::new(),
            });
        } else if let Some(last) = stops.last_mut()
            && let Some(said) = line.strip_prefix("      ")
        {
            let named = |said: &str| {
                let (left, value) = said.split_once(" = ")?;
                let (name, ty) = left.split_once(' ')?;
                Some((name.to_string(), ty.to_string(), value.to_string()))
            };
            if let Some(calls) = said.strip_prefix("calls ") {
                last.calls = calls.to_string();
            } else if let Some(local) = said.strip_prefix("local ").and_then(named) {
                last.locals.push(local);
            } else if let Some(global) = said.strip_prefix("global ").and_then(named) {
                last.globals.push(global);
            }
        } else {
            rest.push(line.clone());
        }
    }
    let mut before = lines[..start].to_vec();
    before.extend(rest);
    (before, stops)
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
    /// The stops of a debugged run (`Plan::debug`), else none.
    pub stops: Vec<Stop>,
}

impl Try {
    pub fn start(plan: Plan) -> Try {
        let (send, done) = channel();
        let (script, on) = (plan.script.clone(), plan.on.clone());
        std::thread::spawn(move || {
            let mut command = std::process::Command::new(&plan.program);
            match &plan.conversation {
                Some(picks) => {
                    command.args([
                        "--project",
                        &plan.project,
                        "try-conversation",
                        &plan.game,
                        &plan.module,
                        &plan.script,
                        "--seconds",
                        &(plan.seconds.ceil() as u64).to_string(),
                    ]);
                    if !picks.is_empty() {
                        let picks: Vec<String> = picks.iter().map(u8::to_string).collect();
                        command.args(["--picks", &picks.join(",")]);
                    }
                }
                None => {
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
                }
            }
            if let Some(on) = &plan.on {
                command.args(["--on", on]);
            }
            if let Some(debug) = &plan.debug {
                command.arg("--debug").arg(debug);
            }
            let result = crate::child::output(command)
                .map_err(|error| format!("kjk try-script did not start: {error}"))
                .map(|output| {
                    let mut lines: Vec<String> = String::from_utf8_lossy(&output.stdout)
                        .lines()
                        .map(str::to_string)
                        .collect();
                    lines.extend(
                        String::from_utf8_lossy(&output.stderr)
                            .lines()
                            // The line that names the project opened, and the logs.
                            .filter(|line| !line.starts_with("project "))
                            // A log line names its level as a word of its
                            // own near its start. A line the script printed
                            // that only holds such letters stays.
                            .filter(|line| {
                                !line
                                    .split_whitespace()
                                    .take(3)
                                    .any(|word| ["INFO", "WARN", "DEBUG"].contains(&word))
                            })
                            .map(str::to_string),
                    );
                    let (lines, stops) = stops_of(&lines);
                    Outcome {
                        script: plan.script.clone(),
                        on: plan.on.clone(),
                        ok: output.status.success(),
                        lines,
                        stops,
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
        use std::sync::mpsc::TryRecvError;
        match self.done.lock() {
            Ok(done) => match done.try_recv() {
                Ok(result) => Some(result),
                Err(TryRecvError::Empty) => None,
                // The thread is gone and sent nothing: the try is over, so
                // another can start.
                Err(TryRecvError::Disconnected) => {
                    Some(Err("the try's thread ended without a result".into()))
                }
            },
            Err(_) => Some(Err("the try's thread failed".into())),
        }
    }
}
