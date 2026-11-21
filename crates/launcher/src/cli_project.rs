//! `--project`: a level editor project opened for play
//! (`docs/LEVEL-EDIT.md`). The editor itself is its own program,
//! `kjk-level-editor` (`crates/level_editor`).

use level_edit::Project;

use crate::discover;

/// Makes the project `name` this run's (`level_edit::active`), before any
/// install is read. The project must exist: the level editor makes one.
pub(crate) fn activate(name: &str) -> Result<(), String> {
    let folder = level_edit::folder_of(name, |var| std::env::var(var).ok())?;
    if !Project::exists(&folder) {
        return Err(format!(
            "there is no project {name} in {}: `kjk-level-editor GAME MODULE --project {name}` makes one",
            folder.display()
        ));
    }
    let active = level_edit::active::activate(&discover(), &folder)?;
    let levels: Vec<String> = (active.project.level_list().into_iter())
        .map(|(module, _)| module)
        .collect();
    eprintln!(
        "project {}: {} {} from {}",
        active.project.name,
        active.project.game,
        levels.join(", "),
        folder.display()
    );
    for note in &active.notes {
        eprintln!("project {}: {note}", active.project.name);
    }
    Ok(())
}
