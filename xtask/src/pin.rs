//! `cargo xtask pin [--message <file>]`: what the merge does with the bump
//! files that reached `main` (`bumps/README.md`). Each format bumped gets
//! the next version, in the order the files were added, with its line in
//! the version's history; the files are taken away; then the sim hash is
//! run and pinned in both workflows. The pin workflow commits the result
//! after the approved tests pass on it.

use crate::{git, repo_root};
use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;

const WORKFLOWS: [&str; 2] = [".github/workflows/ci.yml", ".github/workflows/manual.yml"];
const HASH_KEY: &str = "KJK_SIM_HASH:";
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
    for bump in &pending {
        let path = relative(&root, &bump.path);
        for (name, text) in &bump.formats {
            let format = bumps::format(name).expect("parsed formats are known");
            let version = bump_version(&root, format, text)?;
            let text = text.trim_end_matches('.');
            lines.push(format!("{} {version} ({path}): {text}", format.tag));
        }
        if let Some(why) = &bump.sim {
            let why = why.trim_end_matches('.');
            lines.push(format!("the sim moves ({path}): {why}"));
        }
        git(&["rm", "-q", "--", &path])?;
    }

    let before = pinned(&root)?;
    let after = sim_hash(&root)?;
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
    let output = Command::new(std::env::var("CARGO").unwrap_or("cargo".into()))
        .current_dir(root)
        .args([
            "run",
            "--release",
            "-q",
            "-p",
            "approved_tests",
            "--bin",
            "sim_hash",
        ])
        .output()
        .map_err(|error| format!("cargo run sim_hash: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo run sim_hash: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let hash = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("sim_hash printed {hash:?}, not a hash"));
    }
    Ok(hash)
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
