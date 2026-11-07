//! JA's footsteps (`sound/player/footsteps/`), by the ground a step lands
//! on ([`collision::Ground`], from the floor's texture): a walking step or a
//! running one, one of four at random, as JA's client steps (its walk and
//! run animations' events pick which). Humanoids walk with these
//! (`footstepsounds.2da`'s `Soft` and `Hard` rows); droids and beasts keep
//! KOTOR's own (`ambience`).

use collision::Ground;

const FOLDER: &str = "sound/player/footsteps/";

/// Each ground's walking and running files' stems (four of each).
fn stems(ground: Ground) -> Option<(&'static str, &'static str)> {
    Some(match ground {
        Ground::Unknown => return None,
        Ground::Dirt => ("dirt_step", "dirt_run"),
        Ground::Sand => ("sand_walk", "sand_run"),
        Ground::Snow => ("snow_step", "snow_run"),
        Ground::Mud => ("mud_walk", "mud_run"),
        Ground::Gravel => ("gravel_walk", "gravel_run"),
        Ground::Grass => ("grass_step", "grass_run"),
        Ground::Stone => ("stone_step", "stone_run"),
        Ground::Metal => ("metal_step", "metal_run"),
        Ground::Wood => ("wood_walk", "wood_run"),
        Ground::Rug => ("rug_step", "rug_run"),
        Ground::Water => ("water_walk", "water_run"),
    })
}

fn file(stem: &str, n: i32) -> String {
    format!("{FOLDER}{stem}{n}.mp3")
}

/// The step on `ground`, running or not, the `n`th of four (1 to 4); none
/// on a ground not known.
pub fn step(ground: Ground, running: bool, n: i32) -> Option<String> {
    let (walk, run) = stems(ground)?;
    Some(file(if running { run } else { walk }, n))
}

/// Every footstep this plays (`sound::load`).
pub fn sounds() -> impl Iterator<Item = String> {
    Ground::ALL
        .into_iter()
        .filter_map(stems)
        .flat_map(|(walk, run)| [walk, run])
        .flat_map(|stem| (1..=4).map(move |n| file(stem, n)))
}
