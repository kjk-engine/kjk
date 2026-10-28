//! `kjk trace` and `kjk trace-diff` (`docs/EQUIVALENCE.md`): a sim package's
//! equivalence trace (`sim::trace`), and the first place two differ.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use sim::trace::{Options, Source};

pub(crate) fn trace(pkg: &str, source: &[PathBuf], options: &Options, out: &Path) -> ExitCode {
    let source = if source.len() == 1 && source[0].as_os_str() == "random" {
        Source::Random
    } else {
        Source::Files(source.to_vec())
    };
    let started = Instant::now();
    match sim::trace::run(pkg, &source, options, out) {
        Ok(summary) => {
            let seconds = started.elapsed().as_secs_f64();
            outln!(
                "trace {pkg}: {} scenarios, {} steps in {seconds:.2} s ({:.0} steps/s) -> {}",
                summary.scenarios,
                summary.steps,
                summary.steps as f64 / seconds.max(1e-9),
                out.display()
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("trace {pkg}: {error}");
            ExitCode::FAILURE
        }
    }
}

pub(crate) fn diff(expected: &Path, actual: &Path) -> ExitCode {
    match sim::trace::diff(expected, actual) {
        Ok((None, lines)) => {
            outln!("traces identical ({lines} lines)");
            ExitCode::SUCCESS
        }
        Ok((Some(divergence), _)) => {
            outln!("{divergence}");
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("trace-diff: {error}");
            ExitCode::from(2)
        }
    }
}
