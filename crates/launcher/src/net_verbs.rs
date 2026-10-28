//! `kjk net probes`: the network core's evidence (`net::probes`), each
//! group printed with its checks, what it measured and how long it took,
//! and written to `facts.json`.

use serde_json::json;
use std::process::ExitCode;
use std::time::Instant;

/// Runs the network core's probes and reports each check.
pub fn probes() -> ExitCode {
    let mut all = net::probes::Probes::default();
    let mut timings = Vec::new();
    for (name, run) in net::probes::groups() {
        let started = Instant::now();
        let probes = run();
        let seconds = started.elapsed().as_secs_f64();
        outln!("{name} ({seconds:.2} s):");
        for probe in &probes.list {
            outln!(
                "  {} {}: {}\n         {}",
                if probe.pass { "ok  " } else { "FAIL" },
                probe.name,
                probe.expect,
                probe.detail
            );
        }
        timings.push(json!({ "group": name, "seconds": seconds }));
        all.extend(probes);
    }
    outln!("measures:");
    for measure in &all.measures {
        outln!("  {} = {:.3} {}", measure.name, measure.value, measure.unit);
    }
    let failed = all.failed();
    outln!("{} checks, {failed} failed", all.list.len());
    crate::runtime::write_facts(json!({
        "verb": "net probes",
        "checks": all.list.iter().map(|probe| json!({
            "name": probe.name,
            "expect": probe.expect,
            "detail": probe.detail,
            "pass": probe.pass,
        })).collect::<Vec<_>>(),
        "measures": all.measures.iter().map(|measure| json!({
            "name": measure.name,
            "value": measure.value,
            "unit": measure.unit,
        })).collect::<Vec<_>>(),
        "timings": timings,
        "failed": failed,
    }));
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
