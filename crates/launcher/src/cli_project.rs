//! `--project`: a level editor project opened for play
//! (`docs/LEVEL-EDIT.md`). The editor itself is its own program,
//! `kjk-level-editor` (`crates/level_editor`).

use level_edit::Project;

use crate::discover;

/// Takes what the mods in the games' `override/` hold for kjk as this
/// run's (`level_edit::active::activate_mods`), when no project was named:
/// a mod the level editor exported plays without `--project`. A command
/// that reads no game data does not call it (`reads_mods`).
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
        // The Jedi Academy rooms of the mods' new maps, built into memory
        // before any resolver opens.
        if !active.ja_maps.is_empty() {
            let started = std::time::Instant::now();
            for note in kjk_game::ja_made::make_mods(&discover(), active) {
                eprintln!("kjk mods in {}'s override: {note}", active.project.game);
            }
            eprintln!(
                "kjk mods in {}'s override: built the Jedi Academy rooms of {} new maps in {:.1} s",
                active.project.game,
                active.ja_maps.len(),
                started.elapsed().as_secs_f32()
            );
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
