//! Destruction's props as blows land on them (`sim::destruction`), drawn
//! with KOTOR's own effects and sounds only:
//!
//! - a blow that tells plays the impact the bolt-miss path picks by surface
//!   family (`shot_fx`): blaster sparks on metal, electronics and plastic,
//!   the ground's splash on the rest, where it landed (`prop_looks`);
//! - a prop that breaks (look `DEAD`) bursts at its middle, the burst sized
//!   by its size (blaster sparks, a droid's explosion, a frag grenade's),
//!   and sounds its material's `placeableobjsnds.2da` destroyed sound; one
//!   whose model has no `dead` clip flies apart into pieces of itself
//!   (`prop_looks`), one that has plays its `die` and `dead` clips
//!   (`own_fx`);
//! - a damaged prop's lasting smoke, sparks and fire are `prop_damage`'s,
//!   from the games' own effect placeables (`visualeffects.2da` has no
//!   lasting smoke).

use std::collections::BTreeMap;

use bevy::prelude::*;
use sim::destruction::{Destructible, Material, Size};
use sim::look;
use sim::module::ObjectKind;

use crate::combat_fx::FightDrawn;
use crate::scene::SceneContent;
use crate::script_fx::{HookVisual, HookVisuals, Place};
use crate::sim_host::{PresentedSnapshot, SceneSwap};
use crate::sound::{Cue, Cues};
use crate::viewer::Plays;

/// `visualeffects.2da` rows (both games'): blaster sparks
/// (`VFX_COM_SPARKS_BLASTER`), the ground's splash
/// (`VFX_COM_BLASTER_IMPACT_GROUND`), a droid's explosions (`VFX_COM_DROID_EXPLOSION_1`, `_2`) and a frag
/// grenade's (`VFX_FNF_GRENADE_FRAGMENTATION`).
const SPARKS: u16 = 4033;
const SPLASH: u16 = 4032;
const DROID_BURST: u16 = 4034;
const DROID_BURST_LARGE: u16 = 4035;
const FRAG_BURST: u16 = 3003;

/// How high a prop's middle stands over its feet, by size (metres).
const MIDDLE_M: [f32; 3] = [0.3, 0.6, 1.2];
/// What a material looks and sounds like breaking.
struct Looks {
    /// A blow's impact.
    hit: u16,
    /// The burst at its middle by size.
    burst: [u16; 3],
    /// `placeableobjsnds.2da`'s destroyed sound of its kind.
    destroyed: &'static str,
}

const METALLIC: [u16; 3] = [SPARKS, DROID_BURST, DROID_BURST_LARGE];
const EARTHY: [u16; 3] = [SPLASH, SPLASH, FRAG_BURST];

/// Each material's looks and sounds.
fn looks(material: Material) -> Looks {
    let (metallic, destroyed) = match material {
        Material::Metal => (true, "cb_bu_metalmed"),
        Material::Electronic => (true, "cb_bu_complrg"),
        Material::Plastic => (true, "cb_bu_plstmed"),
        Material::Wood | Material::Organic | Material::Cloth => (false, "cb_bu_woodmed"),
        Material::Ceramic | Material::Stone | Material::Debris => (false, "cb_bu_stonemed"),
    };
    match metallic {
        true => Looks {
            hit: SPARKS,
            burst: METALLIC,
            destroyed,
        },
        false => Looks {
            hit: SPLASH,
            burst: EARTHY,
            destroyed,
        },
    }
}

/// The impact a blow on `material` plays (`visualeffects.2da` row).
pub(crate) fn hit_row(material: Material) -> u16 {
    looks(material).hit
}

/// The KOTOR sounds this plays, for the sound bank.
pub fn sounds() -> Vec<&'static str> {
    let materials = [
        Material::Metal,
        Material::Electronic,
        Material::Plastic,
        Material::Wood,
        Material::Stone,
    ];
    materials.map(|material| looks(material).destroyed).to_vec()
}

/// What a prop's look and lost hit points changing from `was` to `now`
/// (`(look, damage)`) shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Change {
    Nothing,
    Hit,
    Broke,
}

pub(crate) fn change(was: (u8, u16), now: (u8, u16)) -> Change {
    if now.0 == look::DEAD && was.0 != look::DEAD {
        Change::Broke
    } else if now.1 > was.1 {
        Change::Hit
    } else {
        Change::Nothing
    }
}

/// What has been seen of each destructible prop, by object index: its look
/// and lost hit points.
#[derive(Resource, Default)]
struct Breaks {
    generation: u32,
    seen: BTreeMap<u32, (u8, u16)>,
}

/// A destructible prop first seen, or whose look or lost hit points
/// changed, this frame: for `prop_looks`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PropChange {
    pub index: u32,
    pub d: Destructible,
    /// `(look, damage)` before (`None`: first seen) and now.
    pub was: Option<(u8, u16)>,
    pub now: (u8, u16),
    /// Where it stands (game space, metres).
    pub position: [f32; 3],
    /// Its model has a `dead` clip of its own, which it plays instead.
    pub own_dead: bool,
}

/// This frame's prop changes, and where the creatures stand (game space,
/// metres), newest snapshot last.
#[derive(Resource, Default)]
pub(crate) struct PropChanges {
    pub changes: Vec<PropChange>,
    pub creatures: Vec<Vec3>,
}

/// The system that fills `PropChanges`; `prop_looks` runs after it.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct PropsSeen;

pub struct PropBreaksPlugin;

impl Plugin for PropBreaksPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Breaks>()
            .init_resource::<PropChanges>()
            .add_systems(
                PostUpdate,
                breaks
                    .in_set(PropsSeen)
                    .run_if(resource_exists::<PresentedSnapshot>)
                    .after(bevy::transform::TransformSystems::Propagate)
                    .before(FightDrawn),
            );
    }
}

/// Whether the scene's model of object `index` has a `dead` clip.
fn has_dead(scene: &SceneContent, index: u32) -> bool {
    let dead = look::clip_name(look::DEAD);
    scene.0.own_fx.iter().any(|own| match &own.plays {
        Plays::Look { object, clips, .. } => *object == index && clips.contains_key(&dead),
        _ => false,
    })
}

fn breaks(
    presented: Res<PresentedSnapshot>,
    swap: Res<SceneSwap>,
    scene: Res<SceneContent>,
    (visuals, mut cues): (Option<ResMut<HookVisuals>>, ResMut<Cues>),
    mut breaks: ResMut<Breaks>,
    mut seen: ResMut<PropChanges>,
) {
    seen.changes.clear();
    if breaks.generation != swap.generation {
        *breaks = Breaks {
            generation: swap.generation,
            ..Breaks::default()
        };
    }
    let mut sparked = Vec::new();
    for snapshot in &presented.1 {
        let instances = sim::Instances::in_snapshot(&swap.level, snapshot);
        let now = snapshot.time_ms as f64;
        seen.creatures = (snapshot.entities.iter())
            .filter(|entity| {
                matches!(
                    entity.kind,
                    sim::Kind::Avatar(_) | sim::Kind::Object(ObjectKind::Creature, _)
                )
            })
            .map(|entity| Vec3::from(entity.position))
            .collect();
        for entity in &snapshot.entities {
            let sim::Kind::Object(ObjectKind::Placeable, index) = entity.kind else {
                continue;
            };
            let Some(d) = instances.facts(index).and_then(|facts| facts.destructible) else {
                continue;
            };
            let Some(state) = entity.object else {
                continue;
            };
            let shown = (state.state, state.damage);
            let looks = looks(d.material);
            let middle = middle(entity.position, d);
            let was = breaks.seen.insert(index, shown);
            if was != Some(shown) {
                seen.changes.push(PropChange {
                    index,
                    d,
                    was,
                    now: shown,
                    position: entity.position,
                    own_dead: has_dead(&scene, index),
                });
            }
            if was.map(|was| change(was, shown)) == Some(Change::Broke) {
                sparked.push(at(looks.burst[d.size as usize], middle));
                let sound = format!("k1/{}", looks.destroyed);
                cues.0
                    .push(Cue::at(sound, middle / asset_ja::UNIT, now as f32));
            }
        }
    }
    if let Some(mut visuals) = visuals {
        visuals.0.extend(sparked);
    }
}

/// Where prop `d` standing at `position` (metres) has its middle.
fn middle(position: [f32; 3], d: Destructible) -> Vec3 {
    let up = MIDDLE_M[match d.size {
        Size::Small => 0,
        Size::Medium => 1,
        Size::Large => 2,
    }];
    Vec3::new(position[0], position[1], position[2] + up)
}

/// `visualeffects.2da` row `row` at `place` (metres).
fn at(row: u16, place: Vec3) -> HookVisual {
    HookVisual {
        row,
        place: Place::At(place),
    }
}

/// What a prop's changes show: a blow that tells, a break, and nothing for
/// a look that stays or lost hit points that do not grow.
#[cfg(feature = "probes")]
pub fn probes() -> sim::probe::Checks {
    let cases = [
        ((look::DEFAULT, 0), (look::DEFAULT, 3), Change::Hit),
        ((look::DEFAULT, 3), (look::DEAD, 20), Change::Broke),
        ((look::DEAD, 20), (look::DEAD, 20), Change::Nothing),
        ((look::DEFAULT, 3), (look::DEFAULT, 3), Change::Nothing),
    ];
    let wrong: Vec<String> = cases
        .iter()
        .filter(|(was, now, want)| change(*was, *now) != *want)
        .map(|(was, now, want)| {
            format!(
                "{was:?} to {now:?}: {:?}, want {want:?}",
                change(*was, *now)
            )
        })
        .collect();
    vec![(
        "prop_break_changes",
        sim::probe::check(
            wrong.is_empty(),
            format!("{} cases, wrong: {wrong:?}", cases.len()),
        ),
    )]
}
