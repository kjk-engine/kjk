//! `cargo xtask rulebook`: writes `docs/RULEBOOK.md` from the rulebook
//! (`data/rules/*.toml`, through the `rules` crate) and reports what the
//! approved test `rulebook` would fail on. `--list` also writes every
//! literal constant of the sim not yet in `data/rules/not-rules.txt` into it
//! (each to be given its why, or made a rule) and drops the ones gone.

use std::fmt::Write as _;

use rules::audit;

pub fn run(args: &[String]) -> Result<(), String> {
    let root = crate::repo_root();
    let page_path = root.join("docs/RULEBOOK.md");
    std::fs::write(&page_path, rules::page::markdown())
        .map_err(|error| format!("{}: {error}", page_path.display()))?;
    let count = rules::BOOK.iter().filter(|entry| entry.is_rule()).count();
    println!("rulebook: {count} rules → {}", page_path.display());
    if args.iter().any(|arg| arg == "--list") {
        list(&root)?;
    }
    let problems = audit::problems(&root);
    if problems.is_empty() {
        println!("rulebook: whole");
        return Ok(());
    }
    for problem in &problems {
        println!("  {problem}");
    }
    Err(format!("rulebook: {} problems", problems.len()))
}

/// `not-rules.txt` with every literal constant listed: the lines kept as
/// they were (their whys), the gone ones dropped, the new ones added
/// without a why.
fn list(root: &std::path::Path) -> Result<(), String> {
    let path = root.join(audit::NOT_RULES);
    let old = std::fs::read_to_string(&path).unwrap_or_else(|_| String::new());
    let found: Vec<String> = audit::literal_constants(root)
        .iter()
        .map(audit::Literal::key)
        .collect();
    let key_of = |line: &str| {
        line.split('#')
            .next()
            .unwrap_or("")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    };
    let mut lines: Vec<String> = old
        .lines()
        .filter(|line| {
            let key = key_of(line);
            key.is_empty() || found.contains(&key)
        })
        .map(str::to_string)
        .collect();
    let known: Vec<String> = lines.iter().map(|line| key_of(line)).collect();
    let mut added = 0;
    for key in found.iter().filter(|key| !known.contains(key)) {
        lines.push(key.clone());
        added += 1;
    }
    let mut text = String::new();
    for line in lines {
        let _ = writeln!(text, "{line}");
    }
    std::fs::write(&path, text).map_err(|error| format!("{}: {error}", path.display()))?;
    println!(
        "rulebook: {added} literal constants added to {}",
        path.display()
    );
    Ok(())
}
