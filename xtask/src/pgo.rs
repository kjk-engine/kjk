//! `cargo xtask pgo`: a profile for the player's build (docs/RELEASE.md, "A
//! profile-guided build"). `kjk` is built instrumented (the `dist` profile,
//! in a target folder of its own), the bench's scenes are played with it on
//! both presets (`kjk bench --exe`, driven by a development build, which has
//! the verb), and the runs' counts are merged into `target/pgo/kjk.profdata`,
//! which `cargo xtask dist --pgo target/pgo/kjk.profdata` builds with. It
//! needs the games (the bench plays them) and rustup's `llvm-tools`.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::repo_root;

fn cargo() -> Command {
    Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
}

fn checked(mut command: Command, what: &str) -> Result<(), String> {
    let status = command
        .status()
        .map_err(|error| format!("pgo: {what}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("pgo: {what} failed"))
    }
}

/// rustup's `llvm-profdata` for this toolchain's host.
fn llvm_profdata(root: &Path) -> Result<PathBuf, String> {
    let rustc = |args: &[&str]| -> Result<String, String> {
        let output = Command::new("rustc")
            .current_dir(root)
            .args(args)
            .output()
            .map_err(|error| format!("pgo: rustc: {error}"))?;
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    };
    let sysroot = rustc(&["--print", "sysroot"])?;
    let version = rustc(&["-vV"])?;
    let host = version
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .ok_or("pgo: rustc names no host")?;
    let tool = PathBuf::from(sysroot.trim())
        .join("lib/rustlib")
        .join(host.trim())
        .join("bin")
        .join(format!("llvm-profdata{}", std::env::consts::EXE_SUFFIX));
    if tool.is_file() {
        Ok(tool)
    } else {
        Err(format!(
            "pgo: {} is missing: rustup component add llvm-tools",
            tool.display()
        ))
    }
}

pub fn run(args: &[String]) -> Result<(), String> {
    if let Some(other) = args.first() {
        return Err(format!("pgo: unknown option {other}"));
    }
    let root = repo_root();
    let target = crate::dist::target_dir(&root);
    let pgo = target.join("pgo");
    let data = pgo.join("data");
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).map_err(|error| format!("pgo: {error}"))?;
    let profdata = llvm_profdata(&root)?;

    // The development build that drives the runs.
    let mut runner_build = cargo();
    runner_build
        .current_dir(&root)
        .args(["build", "--release", "-p", "launcher"]);
    checked(runner_build, "the development build")?;

    // The player's build, counting what it runs.
    let mut instrumented = cargo();
    instrumented
        .current_dir(&root)
        .env("CARGO_TARGET_DIR", pgo.join("gen"))
        .env("CARGO_ENCODED_RUSTFLAGS", {
            // Built as `dist` builds it, so the profile's functions are
            // the ones `dist --pgo` compiles.
            let mut flags = crate::dist::remaps(&root);
            flags.push(format!("-Cprofile-generate={}", data.display()));
            flags.join("\u{1f}")
        })
        .args([
            "build",
            "--profile",
            "dist",
            "-p",
            "launcher",
            "--no-default-features",
        ]);
    checked(instrumented, "the instrumented build")?;

    let exe = format!("kjk{}", std::env::consts::EXE_SUFFIX);
    let runner = target.join("release").join(&exe);
    let counted = pgo.join("gen").join("dist").join(&exe);
    for preset in ["default", "low"] {
        let mut bench = Command::new(&runner);
        bench.current_dir(&root).args([
            "bench",
            "--exe",
            &counted.display().to_string(),
            "--preset",
            preset,
            "--warmup",
            "3",
            "--seconds",
            "10",
            "--label",
            &format!("pgo-train-{preset}"),
        ]);
        checked(bench, &format!("the {preset} preset's runs"))?;
    }

    let merged = pgo.join("kjk.profdata");
    let mut merge = Command::new(profdata);
    merge.arg("merge").arg("-o").arg(&merged).arg(&data);
    checked(merge, "the merge")?;
    println!(
        "pgo: {} (cargo xtask dist --pgo {})",
        merged.display(),
        merged.display()
    );
    Ok(())
}
