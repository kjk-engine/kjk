//! `cargo xtask console`: writes `docs/CONSOLE.md` from the console's table
//! of commands (`crates/commands`). The approved test `console_page` fails
//! when the page in the repository is not this page.

pub fn run() -> Result<(), String> {
    let path = crate::repo_root().join("docs/CONSOLE.md");
    std::fs::write(&path, commands::page::markdown())
        .map_err(|error| format!("{}: {error}", path.display()))?;
    println!(
        "console: {} commands → {}",
        commands::table().len(),
        path.display()
    );
    Ok(())
}
