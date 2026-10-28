//! `cargo xtask refusals`: each refusal a host gives a player who cannot
//! join, as two or more processes on this machine, with the words the
//! player is told:
//! - a full session: a host with 2 seats and one player joined, then
//!   another;
//! - other game data: a player whose KOTOR install is a copy of this one
//!   (hard links, so no data is copied) with a file added to `override/`;
//! - another build: with `--old-build <kjk>`, a player that is another
//!   commit's binary.
//!
//! Each refused player must exit with a failure and say why. The results
//! go to `kjk-artifacts/runs/<time>-refusals/refusals.md`.
//!
//! ```text
//! cargo xtask refusals [--old-build PATH] [--port 29172]
//! ```

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::duo::{build, plain, spawn, wait_for};
use crate::repo_root;

/// What a refused player must say, by case.
const FULL: &str = "seats are all taken";
const OVERRIDE: &str = "k1 override folder differs from the host's";
const BUILD: &str = "the host runs build";

/// The file the copy's `override/` gets.
const ADDED: &str = "kjk_refusal_probe.2da";

fn options(args: &[String]) -> Result<(Option<PathBuf>, u16), String> {
    let (mut old, mut port) = (None, 29172);
    let mut at = 0;
    while at < args.len() {
        let value = args
            .get(at + 1)
            .ok_or(format!("{}: needs a value", args[at]))?;
        match args[at].as_str() {
            "--old-build" => old = Some(PathBuf::from(value)),
            "--port" => port = value.parse().map_err(|_| "--port: a number")?,
            other => return Err(format!("{other}: not a refusals option")),
        }
        at += 2;
    }
    Ok((old, port))
}

/// KOTOR's install folder and data folder, as `kjk games` finds them.
fn k1_install(binary: &Path) -> Result<(PathBuf, PathBuf), String> {
    let output = Command::new(binary)
        .current_dir(repo_root())
        .arg("games")
        .output()
        .map_err(|error| format!("kjk games: {error}"))?;
    let text = String::from_utf8_lossy(&output.stdout);
    let mut lines = text.lines().skip_while(|line| !line.starts_with("k1: "));
    let root = lines
        .next()
        .and_then(|line| line.strip_prefix("k1: "))
        .ok_or("kjk games: no k1 install")?;
    let data = lines
        .take_while(|line| line.starts_with(' '))
        .find_map(|line| line.trim().strip_prefix("data root: "))
        .ok_or("kjk games: no k1 data root")?;
    Ok((PathBuf::from(root), PathBuf::from(data)))
}

/// `from` again at `to`, every file a hard link (symbolic links left out:
/// no game data is one); how many files.
fn link_tree(from: &Path, to: &Path) -> std::io::Result<usize> {
    std::fs::create_dir_all(to)?;
    let mut files = 0;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            files += link_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            std::fs::hard_link(entry.path(), &target)?;
            files += 1;
        }
    }
    Ok(files)
}

/// A player the host must refuse: who it is, what it runs (with `env`
/// added, in `cwd`), and what it must say.
struct Case<'a> {
    name: &'a str,
    program: PathBuf,
    env: Vec<(&'a str, &'a str)>,
    cwd: PathBuf,
    want: &'a str,
}

/// Waits for a process to end, at most `limit`.
fn finish(child: &mut Child, limit: Duration) -> Result<ExitStatus, String> {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            return Ok(status);
        }
        if started.elapsed() > limit {
            let _ = child.kill();
            return Err(format!("still running after {} s", limit.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

pub fn run(args: &[String]) -> Result<(), String> {
    let (old_build, port) = options(args)?;
    let binary = build()?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let folder = repo_root()
        .join("kjk-artifacts")
        .join("runs")
        .join(format!("{stamp}-refusals"));
    std::fs::create_dir_all(&folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    println!("refusals: {}", folder.display());

    // The copy of KOTOR's data, with a file only it has in `override/`. It
    // fingerprints in a folder of its own, so the real install's cache is
    // left as it is.
    let (root, data) = k1_install(&binary)?;
    let copy = folder.join("k1-copy");
    let files = link_tree(&root, &copy).map_err(|error| {
        format!(
            "linking {} into {}: {error} (the copy needs the same disk)",
            root.display(),
            copy.display()
        )
    })?;
    let relative = data.strip_prefix(&root).unwrap_or(Path::new(""));
    let override_folder = copy.join(relative).join("override");
    std::fs::create_dir_all(&override_folder).map_err(|error| error.to_string())?;
    std::fs::write(override_folder.join(ADDED), "2DA V2.b\n").map_err(|error| error.to_string())?;
    let elsewhere = folder.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).map_err(|error| error.to_string())?;

    let address = format!("127.0.0.1:{port}");
    let host_args: Vec<String> = [
        "host",
        "--stage",
        "end_m01aa",
        "--arrive",
        "wp_player01",
        "--seats",
        "2",
        "--port",
        &port.to_string(),
        "--headless",
        "--cmds",
        "wait 120s; quit",
    ]
    .iter()
    .map(ToString::to_string)
    .collect();
    let join_args = |name: &str| -> Vec<String> {
        [
            "join",
            &address,
            "--name",
            name,
            "--headless",
            "--cmds",
            "wait 100s; quit",
        ]
        .iter()
        .map(ToString::to_string)
        .collect()
    };
    let mut host = spawn(&binary, &host_args, &[], &repo_root(), &folder, "host")?;
    let mut report = String::new();
    let mut failed = Vec::new();
    let mut running = Vec::new();
    let outcome = (|| -> Result<(), String> {
        // The host says it listens once its stage is loaded.
        wait_for(&folder, "host", "listening on", Duration::from_secs(120))?;
        // The second seat taken.
        running.push(spawn(
            &binary,
            &join_args("first"),
            &[],
            &repo_root(),
            &folder,
            "first",
        )?);
        wait_for(
            &folder,
            "first",
            "joined as seat 1",
            Duration::from_secs(60),
        )?;
        let copy_root = copy.display().to_string();
        let mut cases = vec![
            Case {
                name: "full",
                program: binary.clone(),
                env: Vec::new(),
                cwd: repo_root(),
                want: FULL,
            },
            Case {
                name: "other data",
                program: binary.clone(),
                env: vec![("KJK_KOTOR1", &copy_root)],
                cwd: elsewhere.clone(),
                want: OVERRIDE,
            },
        ];
        if let Some(old) = &old_build {
            cases.push(Case {
                name: "other build",
                program: old.clone(),
                env: Vec::new(),
                cwd: repo_root(),
                want: BUILD,
            });
        }
        for Case {
            name: case,
            program,
            env,
            cwd,
            want,
        } in cases
        {
            let name = case.replace(' ', "-");
            let mut child = spawn(&program, &join_args(&name), &env, &cwd, &folder, &name)?;
            let status = finish(&mut child, Duration::from_secs(120));
            let log = std::fs::read_to_string(folder.join(format!("{name}.log")))
                .unwrap_or(String::new());
            let said = log
                .lines()
                .find(|line| line.starts_with("join:"))
                .unwrap_or("(nothing)")
                .to_string();
            let refused = status.as_ref().is_ok_and(|status| !status.success());
            let ok = refused && said.contains(want);
            let _ = writeln!(
                report,
                "- {case}: {} — the player said `{said}`",
                match &status {
                    Ok(status) => status.to_string(),
                    Err(error) => error.clone(),
                }
            );
            if !ok {
                failed.push(case.to_string());
            }
        }
        Ok(())
    })();
    for mut child in running {
        let _ = child.kill();
    }
    let _ = host.kill();
    let _ = host.wait();
    let host_log = std::fs::read_to_string(folder.join("host.log")).unwrap_or(String::new());
    let host_said: Vec<String> = host_log
        .lines()
        .map(plain)
        .filter_map(|line| Some(format!("refused{}", line.split_once(": refused")?.1)))
        .collect();
    let _ = writeln!(
        report,
        "- the host logged {} refusals:\n{}",
        host_said.len(),
        host_said
            .iter()
            .map(|line| format!("  - `{line}`"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    let _ = std::fs::remove_dir_all(&copy);
    outcome?;
    let verdict = if failed.is_empty() {
        "passed".to_string()
    } else {
        format!("FAILED: {}", failed.join(", "))
    };
    let text = format!(
        "# refusals {stamp}\n\nhost: end_m01aa, 2 seats, on {address}; the copy linked {files} files of {}\n\n{report}\n{verdict}\n",
        root.display()
    );
    std::fs::write(folder.join("refusals.md"), &text).map_err(|error| error.to_string())?;
    print!("{text}");
    if failed.is_empty() {
        Ok(())
    } else {
        Err(verdict)
    }
}
