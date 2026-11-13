//! The Advanced rules shown (`docs/SABER-RULES.md` sections 6.1 and 12;
//! render side only, from the presented snapshot), while a session plays
//! them:
//!
//! - the Stamina bar (`bars`): a fighter's block points, coloured by how
//!   many are left and flashing when few are. The player's own stands on
//!   the HUD (`hud`), every other saber user's over its head with the
//!   "Overhead duel bars" option, faded with distance (`overhead`);
//! - the unblockable warning (`warning`, the option off by default): a
//!   character whose swing playing cannot be blocked but by a perfect block
//!   (`Duel::warn`) wears a red shell (`shell`),
//!   never the player's own, nor one in stealth, nor during a conversation.

use std::collections::BTreeMap;

use bevy::prelude::*;
use rules::duel::hud;
use sim::combat::{Delivery, Fighter};
use sim::{Instances, SimId};
use ui::vm::{Bar, DuelBars, OverheadView};

use crate::cast::{BodyRoots, Hitboxes};
use crate::lifecycle_view::SceneCamera;
use crate::settings::DuelOptions;
use crate::shell::{Bodies, ShellMeshes};
use crate::shell::{PUSH_M, ShellMaterial, ShellParams};
use crate::sim_host::{OwnSeat, PresentedSnapshot, SceneSwap};
use crate::ui_view::Ui;

/// The block-point bar's colours, from few points to many (`hud::GAUGE_BANDS`
/// between them).
const POINTS: [[f32; 3]; 4] = [
    [0.86, 0.16, 0.12],
    [0.62, 0.22, 0.85],
    [0.42, 0.12, 0.58],
    [0.95, 0.0, 0.85],
];
/// What is left of a bar flashing off.
const FLASH_OFF: f32 = 0.3;
/// The warning's red, added over the body.
const WARNING: Vec4 = Vec4::new(1.0, 0.08, 0.04, 0.85);
/// Over-head bars: full to `NEAR_M` from the camera, gone at `FAR_M`.
const NEAR_M: f32 = 10.0;
const FAR_M: f32 = 25.0;

/// Whether a bar flashing on a cycle of `period_ms` shows at `now_ms`
/// (real time).
fn lit(now_ms: f64, period_ms: u32) -> bool {
    (now_ms / f64::from(period_ms)).fract() < 0.5
}

fn bar(share: f32, [r, g, b]: [f32; 3], on: bool) -> Bar {
    Bar {
        share,
        color: [r, g, b, if on { 1.0 } else { FLASH_OFF }],
    }
}

/// The block-point bar of `points` left (MDA's meter).
pub(crate) fn points(points: u8, now_ms: f64) -> Bar {
    let band = hud::GAUGE_BANDS
        .iter()
        .position(|&under| points < under)
        .unwrap_or(hud::GAUGE_BANDS.len());
    let share = f32::from(points) / f32::from(rules::duel::meters::POINTS_MAX);
    let on = points >= hud::GAUGE_FLASH || lit(now_ms, hud::GAUGE_FLASH_MS);
    bar(share, POINTS[band], on)
}

/// `fighter`'s Stamina bar at `now_ms` (real time).
pub(crate) fn bars(fighter: &Fighter, now_ms: f64) -> DuelBars {
    DuelBars {
        points: points(fighter.saber.duel.points, now_ms),
    }
}

/// Whether `entity` holds a blade that a block can be held with: a
/// lightsaber, or a melee blade such as a sword, a vibroblade or a staff.
pub(crate) fn blade_user(entity: &sim::EntityState, objects: &Instances) -> bool {
    matches!(
        crate::cast::weapon(entity, objects).delivery,
        Delivery::Saber(_) | Delivery::Melee
    )
}

/// Whether `entity` fights with a lightsaber in hand.
pub(crate) fn saber_user(entity: &sim::EntityState, objects: &Instances) -> bool {
    matches!(
        crate::cast::weapon(entity, objects).delivery,
        Delivery::Saber(_)
    )
}

pub struct DuelViewPlugin;

impl Plugin for DuelViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Warned>()
            .add_systems(
                Update,
                facts
                    .run_if(resource_exists::<PresentedSnapshot>)
                    .run_if(resource_exists::<SceneSwap>),
            )
            .add_systems(
                PostUpdate,
                // With the frame's camera, as the floating words are placed
                // (`floaty_view`): the bars are drawn in their layer.
                overhead
                    .run_if(resource_exists::<PresentedSnapshot>)
                    .run_if(resource_exists::<DuelOptions>)
                    .after(bevy::transform::TransformSystems::Propagate)
                    .before(crate::ui_view::Floated),
            )
            .add_systems(
                PostUpdate,
                warning
                    .run_if(resource_exists::<PresentedSnapshot>)
                    .run_if(resource_exists::<DuelOptions>)
                    .run_if(resource_exists::<BodyRoots>)
                    .after(crate::play_view::Drawn),
            );
    }
}

/// One standing saber user's duel meters, for `dump`: its Stamina and its
/// guard.
fn fact(id: u32, fighter: &Fighter) -> serde_json::Value {
    let duel = &fighter.saber.duel;
    serde_json::json!({
        "id": id, "hp": fighter.hp, "fp": fighter.fp,
        "points": duel.points,
        "block_held": duel.block_held, "aimed": duel.aimed,
        "warn": duel.warn, "broken": duel.broken,
        "perfect_at": duel.perfect_at, "wear": duel.wear,
    })
}

/// The duel meters of every saber user standing, for `dump`: the rules,
/// and each one's Stamina and guard.
fn facts(
    presented: Res<PresentedSnapshot>,
    swap: Res<SceneSwap>,
    mut facts: ResMut<console::DumpFacts>,
) {
    let snapshot = presented.0.current();
    let objects = Instances::in_snapshot(&swap.level, snapshot);
    let fighters: Vec<_> = snapshot
        .entities
        .iter()
        .filter(|entity| saber_user(entity, &objects))
        .filter_map(|entity| {
            let fighter = entity.fighter.as_ref().filter(|fighter| fighter.hp > 0)?;
            Some(fact(entity.id.0, fighter))
        })
        .collect();
    facts.0.insert(
        "duel".into(),
        serde_json::json!({
            "rules": snapshot.saber_rules.name(),
            "now_ms": snapshot.time_ms,
            "fighters": fighters,
        }),
    );
}

/// Every other saber user's duel bars over its head, with the option on in
/// a duel mode.
#[allow(clippy::too_many_arguments)]
fn overhead(
    presented: Res<PresentedSnapshot>,
    options: Res<DuelOptions>,
    (swap, seat): (Option<Res<SceneSwap>>, Option<Res<OwnSeat>>),
    hitboxes: Option<Res<Hitboxes>>,
    places: Query<&GlobalTransform>,
    cameras: Query<(&GlobalTransform, &Camera), SceneCamera>,
    real: Res<Time<Real>>,
    mut ui: ResMut<Ui>,
) {
    let snapshot = presented.0.current();
    let rules = snapshot.saber_rules;
    let (true, Some(swap), Ok(camera)) = (
        options.bars && rules.duel(),
        swap.as_ref(),
        cameras.single(),
    ) else {
        ui.vm.overhead.clear();
        return;
    };
    let me = crate::cast::me(snapshot, seat.map_or(sim::Seat(0), |seat| seat.0));
    let objects = Instances::in_snapshot(&swap.level, snapshot);
    let eye = camera.0.translation();
    let now_ms = real.elapsed_secs_f64() * 1000.0;
    ui.vm.overhead = snapshot
        .entities
        .iter()
        .filter(|entity| Some(entity.id) != me && saber_user(entity, &objects))
        .filter_map(|entity| {
            let fighter = entity.fighter.as_ref().filter(|fighter| fighter.hp > 0)?;
            let at = crate::scene::game_to_bevy() * Vec3::from_array(entity.position);
            let alpha = 1.0 - ((eye.distance(at) - NEAR_M) / (FAR_M - NEAR_M)).clamp(0.0, 1.0);
            if alpha <= 0.0 {
                return None;
            }
            let over = crate::bark_view::over(
                snapshot,
                entity.id,
                0.0,
                hitboxes.as_deref(),
                &places,
                camera,
            )?;
            Some(OverheadView {
                over,
                alpha,
                bars: bars(fighter, now_ms),
            })
        })
        .collect();
}

/// The red shells over the characters swinging what cannot be blocked, and
/// the one shell material.
#[derive(Resource, Default)]
pub struct Warned {
    by: BTreeMap<SimId, Vec<Entity>>,
    material: Option<Handle<ShellMaterial>>,
}

/// Each frame: the warning's shells over those newly swinging an
/// unblockable attack, none over those done, with the option on in a duel
/// mode.
#[allow(clippy::too_many_arguments)]
fn warning(
    presented: Res<PresentedSnapshot>,
    options: Res<DuelOptions>,
    seat: Option<Res<OwnSeat>>,
    roots: Res<BodyRoots>,
    children: Query<&'static Children>,
    meshes: ShellMeshes,
    (mut warned, mut materials): (ResMut<Warned>, ResMut<Assets<ShellMaterial>>),
    mut commands: Commands,
) {
    let snapshot = presented.0.current();
    let seat = seat.map_or(sim::Seat(0), |seat| seat.0);
    let me = crate::cast::me(snapshot, seat);
    let shown = options.warning && snapshot.saber_rules.duel() && snapshot.talk_for(seat).is_none();
    let swinging: Vec<SimId> = snapshot
        .entities
        .iter()
        .filter(|entity| shown && Some(entity.id) != me)
        .filter(|entity| {
            entity
                .fighter
                .is_some_and(|fighter| fighter.saber.duel.warn && !fighter.covert.stealth)
        })
        .map(|entity| entity.id)
        .collect();
    let material = warned
        .material
        .get_or_insert_with(|| {
            materials.add(ShellMaterial {
                params: ShellParams {
                    tint: WARNING,
                    push: Vec4::new(PUSH_M, 0.0, 0.0, 0.0),
                },
                texture: None,
            })
        })
        .clone();
    let bodies = Bodies {
        roots: &roots,
        children: &children,
        meshes: &meshes,
    };
    crate::shell::follow(&mut warned.by, &swinging, &material, &bodies, &mut commands);
}

/// The bar's colours and flashes (`kjk sim probes --only "duel bars"`):
/// the block points by band, flashing only when few; and the facts a dump
/// holds for one fighter, which name one bar.
#[cfg(feature = "probes")]
pub fn probes() -> sim::probe::Checks {
    use sim::probe::check;
    let (on, off) = (0.0, f64::from(hud::GAUGE_FLASH_MS) * 0.75);
    let rgb = |bar: Bar| [bar.color[0], bar.color[1], bar.color[2]];
    let bands = [100, 60, 30, 10].map(|left| rgb(points(left, on)));
    let low = [
        points(hud::GAUGE_FLASH - 1, off),
        points(hud::GAUGE_FLASH, off),
    ];
    let flashes = low.map(|bar| bar.color[3]);
    let shown = fact(1, &Fighter::fresh(10_000, 10));
    let keys: Vec<String> = match shown.as_object() {
        Some(fact) => fact.keys().cloned().collect(),
        None => Vec::new(),
    };
    let one_bar = keys.iter().any(|key| key == "points")
        && !keys.iter().any(|key| key == "mishap" || key == "stamina");
    vec![
        (
            "one_bar",
            check(one_bar, format!("the facts of a saber user hold {keys:?}")),
        ),
        (
            "block points by band, flashing when few",
            check(
                bands == [POINTS[3], POINTS[2], POINTS[1], POINTS[0]]
                    && flashes == [FLASH_OFF, 1.0],
                format!("{bands:?}, flashing {flashes:?}"),
            ),
        ),
    ]
}
