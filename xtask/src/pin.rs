//! `cargo xtask pin [--message <file>]`: what the merge does with the bump
//! files that reached `main` (`bumps/README.md`). Each format bumped gets
//! the next version, in the order the files were added, with its line in
//! the version's history; the files are taken away; then the sim hash is
//! run and pinned in both workflows. A format saves hold that moved gets
//! its bump's named version in `crates/sim/src/since.rs`, its migration in
//! the history (or, `breaking`, a line in `bumps/BREAKING.md`, its oldest
//! version read raised and the fixtures before it retired), and a fixture
//! at the new versions. The pin workflow commits the result after the
//! approved tests pass on it.

use crate::{git, repo_root};
use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;

const WORKFLOWS: [&str; 2] = [".github/workflows/ci.yml", ".github/workflows/manual.yml"];
const HASH_KEY: &str = "KJK_SIM_HASH:";
const SINCE: &str = "crates/sim/src/since.rs";
const BREAKING: &str = "bumps/BREAKING.md";
/// The format fixtures (`sim::fixtures::DIR`) and the order of the
/// versions in their names (`save-<save>-<snapshot>-<pack>.kjkv`).
const FIXTURES: &str = "crates/approved_tests/saves";
const FIXTURE_FORMATS: [&str; 3] = ["save", "snapshot", "pack"];
/// The width the versions' histories are wrapped to, `/// ` included.
const WIDTH: usize = 80;

pub fn run(args: &[String]) -> Result<(), String> {
    let message_file = match args {
        [] => None,
        [flag, file] if flag == "--message" => Some(file.clone()),
        _ => return Err("usage: cargo xtask pin [--message <file>]".into()),
    };
    let root = repo_root();
    let mut pending = bumps::pending(&root)?;
    if pending.is_empty() {
        println!("pin: no bump files");
        return Ok(());
    }
    // The order they reached this branch: the first added is numbered first.
    let mut added = Vec::new();
    for bump in &pending {
        let path = relative(&root, &bump.path);
        let time = String::from_utf8_lossy(&git(&[
            "log",
            "--diff-filter=A",
            "--format=%ct",
            "-1",
            "--",
            &path,
        ])?)
        .trim()
        .parse::<u64>()
        .unwrap_or(u64::MAX);
        added.push((time, path));
    }
    let mut order: Vec<usize> = (0..pending.len()).collect();
    order.sort_by(|a, b| added[*a].cmp(&added[*b]));
    pending = order.iter().map(|at| pending[*at].clone()).collect();

    let mut lines = Vec::new();
    let mut named = Vec::new();
    for bump in &pending {
        let path = relative(&root, &bump.path);
        for (name, text) in &bump.formats {
            let format = bumps::format(name).expect("parsed formats are known");
            let migration = bump
                .migrations
                .iter()
                .find(|(migrated, _)| migrated == name)
                .map(|(_, how)| how);
            let history = match (migration, &bump.breaking) {
                (Some(how), _) => format!("{} Old saves: {how}.", end(text)),
                (None, Some(why)) if format.stored => format!("{} Breaking: {why}.", end(text)),
                _ => text.clone(),
            };
            let version = bump_version(&root, format, &history)?;
            let text = text.trim_end_matches('.');
            lines.push(format!("{} {version} ({path}): {text}", format.tag));
            if format.stored {
                named.push((format.name, bump.name(), version));
                if let (None, Some(why)) = (migration, &bump.breaking) {
                    breaking(&root, format, version, &path, why)?;
                    lines.push(format!("{} {version} breaks older saves", format.tag));
                }
            }
        }
        if let Some(why) = &bump.sim {
            let why = why.trim_end_matches('.');
            lines.push(format!("the sim moves ({path}): {why}"));
        }
        git(&["rm", "-q", "--", &path])?;
    }

    if !named.is_empty() {
        write_since(&root, &named)?;
    }

    let before = pinned(&root)?;
    let after = sim_hash(&root)?;
    if !named.is_empty() {
        let fixture = cargo(&root, &["--bin", "save_fixture"])?;
        git(&["add", "--", SINCE, fixture.trim()])?;
        lines.push(format!(
            "the format fixture {}",
            relative(&root, Path::new(fixture.trim()))
        ));
    }
    for workflow in WORKFLOWS {
        let path = root.join(workflow);
        let text = read(&path)?;
        let mut out = String::new();
        for line in text.split_inclusive('\n') {
            match line.trim_start().strip_prefix(HASH_KEY) {
                Some(_) => {
                    let indent = &line[..line.len() - line.trim_start().len()];
                    writeln!(out, "{indent}{HASH_KEY} {after}").expect("a String takes writes");
                }
                None => out.push_str(line),
            }
        }
        write(&path, &out)?;
    }
    let short = |hash: &str| hash.chars().take(8).collect::<String>();
    if before == after {
        lines.push(format!("the sim hash stays {}", short(&after)));
    } else {
        lines.push(format!(
            "the sim hash {} → {}",
            short(&before),
            short(&after)
        ));
    }
    let mut message = format!("Pinned at merge: {}\n\n", lines.join("; "));
    for line in &lines {
        writeln!(message, "- {line}").expect("a String takes writes");
    }
    print!("{message}");
    if let Some(file) = message_file {
        std::fs::write(&file, &message).map_err(|error| format!("{file}: {error}"))?;
    }
    Ok(())
}

/// `format`'s constant `N + crate::pending::…` becomes `N + 1 + …`, and its
/// history above it gets `/// N+1: text`; the new version.
fn bump_version(root: &Path, format: &bumps::Format, text: &str) -> Result<u32, String> {
    let path = root.join(format.file);
    let source = read(&path)?;
    let pending = format!(" + crate::pending::{};", format.name.to_uppercase());
    let mut out = String::new();
    let mut version = None;
    for line in source.split_inclusive('\n') {
        match bumps::declared(line, format) {
            Some(old) if version.is_none() => {
                let new = old + 1;
                out.push_str(&history(&format!("{new}: {text}")));
                let prefix = &line[..line.find("= ").expect("matched above") + 2];
                writeln!(out, "{prefix}{new}{pending}").expect("a String takes writes");
                version = Some(new);
            }
            _ => out.push_str(line),
        }
    }
    let version = version.ok_or(format!(
        "{}: no `const {}: u32 = N{pending}`",
        format.file, format.constant
    ))?;
    write(&path, &out)?;
    Ok(version)
}

/// `text` as `///` lines no wider than `WIDTH`.
fn history(text: &str) -> String {
    let mut out = String::new();
    let mut line = String::from("///");
    for word in text.split_whitespace() {
        if line.len() > 3 && line.len() + 1 + word.len() > WIDTH {
            out.push_str(&line);
            out.push('\n');
            line = String::from("///");
        }
        line.push(' ');
        line.push_str(word);
    }
    out.push_str(&line);
    out.push('\n');
    out
}

fn pinned(root: &Path) -> Result<String, String> {
    let path = root.join(WORKFLOWS[0]);
    read(&path)?
        .lines()
        .find_map(|line| line.trim().strip_prefix(HASH_KEY))
        .map(|hash| hash.trim().to_string())
        .ok_or(format!("{}: no {HASH_KEY}", path.display()))
}

/// The pinned run's final hash, built with the bumps applied.
fn sim_hash(root: &Path) -> Result<String, String> {
    let hash = cargo(root, &["--bin", "sim_hash"])?.trim().to_string();
    if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("sim_hash printed {hash:?}, not a hash"));
    }
    Ok(hash)
}

/// `cargo run --release -q -p approved_tests <args>`'s output, built with
/// the bumps applied.
fn cargo(root: &Path, args: &[&str]) -> Result<String, String> {
    let what = format!("cargo run approved_tests {}", args.join(" "));
    let output = Command::new(std::env::var("CARGO").unwrap_or("cargo".into()))
        .current_dir(root)
        .args(["run", "--release", "-q", "-p", "approved_tests"])
        .args(args)
        .output()
        .map_err(|error| format!("{what}: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "{what}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// `text` ending in a full stop.
fn end(text: &str) -> String {
    format!("{}.", text.trim_end_matches('.'))
}

/// Each `(format, name, version)` as `pub const <name>: u32 = <version>;`
/// in its format's module of `since.rs`, before the pending ones' export.
fn write_since(root: &Path, named: &[(&str, String, u32)]) -> Result<(), String> {
    let path = root.join(SINCE);
    let source = read(&path)?;
    let mut out = String::new();
    let mut module = None;
    for line in source.split_inclusive('\n') {
        if let Some(name) = line
            .strip_prefix("pub mod ")
            .and_then(|rest| rest.strip_suffix(" {\n"))
        {
            module = Some(name.to_string());
        }
        if line.trim_start().starts_with("#[allow(unused_imports)]") {
            for (format, name, version) in named {
                if module.as_deref() == Some(*format) {
                    writeln!(out, "    pub const {name}: u32 = {version};")
                        .expect("a String takes writes");
                }
            }
        }
        out.push_str(line);
    }
    write(&path, &out)
}

/// A `breaking` bump of `format` to `version`: its line in `BREAKING.md`,
/// the format's `OLDEST` raised to it, and the fixtures before it retired.
fn breaking(
    root: &Path,
    format: &bumps::Format,
    version: u32,
    bump: &str,
    why: &str,
) -> Result<(), String> {
    let path = root.join(BREAKING);
    let mut text = read(&path)?;
    writeln!(text, "- {} {version} ({bump}): {}", format.tag, end(why))
        .expect("a String takes writes");
    write(&path, &text)?;
    let file = root.join(format.file);
    let source = read(&file)?;
    let oldest = "pub const OLDEST: u32 = ";
    if !source.lines().any(|line| line.starts_with(oldest)) {
        return Err(format!("{}: no `{oldest}N;`", format.file));
    }
    let source: String = source
        .split_inclusive('\n')
        .map(|line| match line.starts_with(oldest) {
            true => format!("{oldest}{version};\n"),
            false => line.to_string(),
        })
        .collect();
    write(&file, &source)?;
    let at = FIXTURE_FORMATS
        .iter()
        .position(|name| *name == format.name)
        .expect("stored formats have fixtures");
    let dir = root.join(FIXTURES);
    let entries = std::fs::read_dir(&dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    for entry in entries {
        let name = entry
            .map_err(|error| error.to_string())?
            .file_name()
            .to_string_lossy()
            .into_owned();
        let older = name
            .strip_prefix("save-")
            .and_then(|rest| rest.strip_suffix(".kjkv"))
            .and_then(|versions| versions.split('-').nth(at))
            .and_then(|found| found.parse::<u32>().ok())
            .is_some_and(|found| found < version);
        if older {
            std::fs::create_dir_all(dir.join("retired")).map_err(|error| error.to_string())?;
            git(&[
                "mv",
                &format!("{FIXTURES}/{name}"),
                &format!("{FIXTURES}/retired/{name}"),
            ])?;
        }
    }
    Ok(())
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    std::fs::write(path, text).map_err(|error| format!("{}: {error}", path.display()))
}
