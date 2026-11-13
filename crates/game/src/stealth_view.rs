//! A character in stealth mode (`Fighter::covert.stealth`), drawn as TSL's
//! executable draws one with Frame Buffer Effects on
//! (`CSWCCreature::SetStealthState`), for every player (`docs/COMBAT.md`).
//!
//! - **Effect 8000** (`VFX_DUR_DISTORTION`, ProgFX 1800): the character's
//!   meshes and the weapons in its hands are not drawn. In their place the
//!   scene behind them shows, displaced by the executable's ring texture
//!   (`glow_shell::Glow::distortion`, `ring_distort.wgsl`). A weapon drawn while
//!   in stealth is covered as it appears, as the executable distorts the
//!   new weapon on a weapon switch.
//! - **Effect 8002** (`VFX_DUR_STEALTH_FIELD`, ProgFX 1426): the
//!   executable's look when Frame Buffer Effects are off or the hardware
//!   lacks them. The character is drawn, and a shell of `fx_tex_stealth` is
//!   added over it, played as the texture's 2 × 2 flip-book at its 8 frames
//!   a second. kjk always has the frame copy the distortion needs, so this
//!   look is shown only when `KJK_STEALTH_SHELL=1` asks for it.
//!
//! The shells go when the stealth ends or the character does.

use std::collections::BTreeMap;

use bevy::prelude::*;
use sim::SimId;

use crate::cast::BodyRoots;
use crate::glow_shell::{Glow, GlowMaterial, SceneMeshes};
use crate::made::Made;
use crate::material::OdysseyMaterial;
use crate::sim_host::PresentedSnapshot;

/// The environment switch that asks for effect 8002's look.
const SHELL_SWITCH: &str = "KJK_STEALTH_SHELL";

/// Which of the executable's two stealth looks this run draws.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Look {
    /// Effect 8000: the ring distortion in the character's shape.
    Distortion,
    /// Effect 8002: the `fx_tex_stealth` shell over the drawn character.
    Shell,
}

impl Look {
    /// `KJK_STEALTH_SHELL`'s choice: the shell when it is set to `1`, else
    /// the distortion.
    fn of_run() -> Look {
        match std::env::var(SHELL_SWITCH).as_deref() {
            Ok("1") => Look::Shell,
            _ => Look::Distortion,
        }
    }
}

/// The look drawn on each character in stealth, by character, and when
/// each began (seconds of game time, for the shell's flip-book).
#[derive(Resource)]
pub struct Cloaks {
    look: Look,
    by: BTreeMap<SimId, (Glow, f64)>,
}

impl Default for Cloaks {
    fn default() -> Cloaks {
        Cloaks {
            look: Look::of_run(),
            by: BTreeMap::new(),
        }
    }
}

/// Each frame: the look on those newly in stealth, and on a weapon one of
/// them has drawn since; none on those out of it.
#[allow(clippy::too_many_arguments)]
pub fn cloaks(
    presented: Res<PresentedSnapshot>,
    roots: Res<BodyRoots>,
    made: Res<Made>,
    (children, meshes): (Query<&Children>, SceneMeshes),
    (odyssey, mut materials): (Res<Assets<OdysseyMaterial>>, ResMut<Assets<GlowMaterial>>),
    mut cloaks: ResMut<Cloaks>,
    mut commands: Commands,
) {
    let stealthed: Vec<SimId> = presented
        .0
        .current()
        .entities
        .iter()
        .filter(|entity| entity.fighter.is_some_and(|fighter| fighter.covert.stealth))
        .map(|entity| entity.id)
        .collect();
    let cloaks = &mut *cloaks;
    cloaks.by.retain(|id, (glow, _)| {
        let kept = stealthed.contains(id) && roots.0.contains_key(id);
        if !kept {
            glow.remove(&mut commands);
        }
        kept
    });
    let now = crate::sim_host::view_ms(&presented.0) / 1000.0;
    for id in stealthed {
        let Some(&root) = roots.0.get(&id) else {
            continue;
        };
        if !cloaks.by.contains_key(&id) {
            let glow = match (cloaks.look, &made.stealth_field) {
                (Look::Distortion, _) => Glow::distortion(&mut materials),
                (Look::Shell, Some((texture, frames))) => {
                    Glow::new(texture, frames.clone(), &mut materials)
                }
                (Look::Shell, None) => continue,
            };
            cloaks.by.insert(id, (glow, now));
        }
        let Some((glow, since)) = cloaks.by.get_mut(&id) else {
            continue;
        };
        glow.cover(root, (&children, &meshes), &odyssey, &mut commands);
        glow.play((now - *since) as f32, &mut materials);
    }
}
