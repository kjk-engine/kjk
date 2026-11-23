//! `--project`: a level editor project opened for play
//! (`docs/LEVEL-EDIT.md`). The editor itself is its own program,
//! `kjk-level-editor` (`crates/level_editor`).

use level_edit::Project;

use crate::discover;

/// Takes what the mods in the games' `override/` hold for kjk as this
/// run's (`level_edit::active::activate_mods`), when no project was named:
/// a mod the level editor exported plays without `--project`.
pub(crate) fn activate_mods() {
    for active in level_edit::active::activate_mods(&discover()) {
        let levels: Vec<String> = (active.project.level_list().into_iter())
            .map(|(module, _)| module)
            .filter(|module| !module.is_empty())
            .collect();
        eprintln!(
            "kjk mods in {}'s override: {} levels ({}), {} kinds of fitting, {} bakes",
            active.project.game,
            levels.len(),
            levels.join(", "),
            active.lighting.kinds.len(),
            active.bakes.len()
        );
        for found in &active.mods {
            eprintln!(
                "kjk mods in {}'s override: {} {}, {} files, {} quests",
                active.project.game,
                found.name,
                found.version,
                found.files.len(),
                found.quests.len()
            );
        }
        for note in &active.notes {
            eprintln!("kjk mods in {}'s override: {note}", active.project.game);
        }
    }
}

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
    // The Jedi Academy rooms its maps place, built into memory before any
    // resolver opens.
    for note in kjk_game::ja_made::make_project(&discover(), &active.project) {
        eprintln!("project {}: {note}", active.project.name);
    }
    Ok(())
}
