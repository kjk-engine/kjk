mod discord;
mod dist;
mod duo;
mod icon;
mod pin;
mod publish_check;
mod refusals;
mod rulebook;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const USAGE: &str = "usage: cargo xtask <publish-check | email-hash | hooks | rulebook [--list] | duo [options] | refusals [options] | pin [--message <file>] | dist [--skip-build] | icon | discord [--dry-run] <caption> <file>...>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let result = match args.get(1).map(String::as_str) {
        Some("publish-check") => publish_check::run(&args[2..]),
        Some("email-hash") => publish_check::email_hashes(),
        Some("hooks") => install_hooks(),
        Some("duo") => duo::run(&args[2..]),
        Some("refusals") => refusals::run(&args[2..]),
        Some("rulebook") => rulebook::run(&args[2..]),
        Some("dist") => dist::run(&args[2..]),
        Some("pin") => pin::run(&args[2..]),
        Some("icon") => icon::run(&args[2..]),
        Some("discord") => discord::run(&args[2..]),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

/// Points git at the tracked `.githooks/`, so the hooks update with the repo.
fn install_hooks() -> Result<(), String> {
    git(&["config", "core.hooksPath", ".githooks"])?;
    println!("hooks: core.hooksPath = .githooks");
    Ok(())
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .expect("xtask lives one level below the repo root")
}

fn git(args: &[&str]) -> Result<Vec<u8>, String> {
    let output = Command::new("git")
        .current_dir(repo_root())
        .args(args)
        .output()
        .map_err(|error| format!("git {}: {error}", args.join(" ")))?;
    if !output.status.success() {
        return Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(output.stdout)
}
