//! What the launcher's view-layer probes share: the sim's checks and the
//! check that makes one (`sim::probe`), what a check skipped for a missing
//! install says, and a headless app with what spawning and dumping a
//! scene's models needs.

use bevy::prelude::*;
use console::{AnimationTime, DumpDue, DumpFacts};

pub(crate) use sim::probe::{Checks, check};

/// What a check says when it is skipped because `game` is not installed.
pub(crate) fn not_installed(game: vfs::Game) -> String {
    format!("skipped: no KOTOR install (set {} in .env)", game.env_var())
}

/// A headless app (`MinimalPlugins`) with the stores models are spawned
/// into, the dump's facts (none due) and the animations' clock held at 0.
pub(crate) fn scene_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<crate::material::OdysseyMaterial>>()
        .init_resource::<Assets<Image>>()
        .init_resource::<Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>>()
        .init_resource::<Assets<crate::heat_haze::HeatHazeMaterial>>()
        .init_resource::<DumpFacts>()
        .insert_resource(DumpDue(false))
        .insert_resource(AnimationTime(Some(0.0)));
    app
}
