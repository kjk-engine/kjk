//! `kjk trace` without the game around it, for measuring what the traces
//! cover (`cargo llvm-cov run -p sim --features probes --example trace`,
//! `docs/EQUIVALENCE.md`): the same driver, a smaller build to instrument.
//!
//! `trace PKG OUT (random SEED SEEDS N | FILE...) [--emit FILE]`

use std::path::PathBuf;
use std::process::ExitCode;

use sim::trace::{Options, Source};

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let emit = args.iter().position(|word| word == "--emit").map(|at| {
        let path = args.get(at + 1).cloned().unwrap_or_else(String::new);
        args.drain(at..(at + 2).min(args.len()));
        PathBuf::from(path)
    });
    if args.len() < 3 {
        eprintln!("trace PKG OUT (random SEED SEEDS N | FILE...)");
        return ExitCode::from(2);
    }
    let (pkg, out) = (&args[0], PathBuf::from(&args[1]));
    let mut options = Options {
        seed: 1,
        seeds: 1,
        n: 10_000,
        emit,
    };
    let source = if args[2] == "random" {
        let number = |i: usize, default: u64| {
            args.get(i)
                .and_then(|word| word.parse().ok())
                .unwrap_or(default)
        };
        options.seed = number(3, 1);
        options.seeds = number(4, 1);
        options.n = number(5, 10_000);
        Source::Random
    } else {
        Source::Files(args[2..].iter().map(PathBuf::from).collect())
    };
    match sim::trace::run(pkg, &source, &options, &out) {
        Ok(summary) => {
            println!(
                "trace {pkg}: {} scenarios, {} steps",
                summary.scenarios, summary.steps
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("trace {pkg}: {error}");
            ExitCode::FAILURE
        }
    }
}
