//! The duel modes shown (`docs/SABER-RULES.md` sections 6.1 and 12; render
//! side only, from the presented snapshot), while a session plays one:
//!
//! - the duel bars (`bars`): a saber user's block points, MDA's own meter
//!   (over a head under MD, its Force), coloured by how many are left and
//!   flashing when few are; and its stamina, the mishap count turned upside
//!   down, flashing in a warning's colour as the count climbs. The player's
//!   own stand on the HUD (`hud`), every other saber user's over its head
//!   with the "Overhead duel bars" option, faded with distance
//!   (`overhead`);
//! - the unblockable warning (`warning`, the option off by default): a
//!   character whose swing playing cannot be blocked but by a perfect block
//!   (`Duel::warn`) wears a red shell (`shell`, as the stealth field's),
//!   never the player's own, nor one in stealth, nor during a conversation.

use std::collections::BTreeMap;

use bevy::prelude::*;
use rules::duel::hud;
use sim::combat::saber::SaberRules;
use sim::combat::{Delivery, Fighter};
use sim::{Instances, SimId};
use ui::vm::{Bar, DuelBars, OverheadView};

use crate::cast::{BodyRoots, Hitboxes};
use crate::lifecycle_view::SceneCamera;
use crate::settings::DuelOptions;
use crate::shell::{PUSH_M, ShellMaterial, ShellParams};
use crate::sim_host::{OwnSeat, PresentedSnapshot, SceneSwap};
use crate::stealth_view::{Bodies, ShellMeshes};
use crate::ui_view::Ui;

/// The block-point bar's colours, from few points to many (`hud::GAUGE_BANDS`
/// between them), and MD's Force over a head.
const POINTS: [[f32; 3]; 4] = [
    [0.86, 0.16, 0.12],
    [0.62, 0.22, 0.85],
    [0.42, 0.12, 0.58],
    [0.95, 0.0, 0.85],
];
const FORCE: [f32; 3] = [0.25, 0.45, 1.0];
/// The stamina bar's grey, and its warnings' colours (`hud::LAMPS`).
const STAMINA: [f32; 3] = [0.72, 0.72, 0.72];
const LAMPS: [[f32; 3]; 4] = [
    [0.3, 0.9, 0.35],
    [0.25, 0.45, 1.0],
    [0.95, 0.0, 0.85],
    [0.86, 0.16, 0.12],
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

/// The stamina bar of a `mishap` count: grey, flashing in the colour of
/// the highest warning it passed.
pub(crate) fn stamina(mishap: u8, now_ms: f64) -> Bar {
    let share = 1.0 - f32::from(mishap) / f32::from(hud::STAMINA_STEPS);
    match hud::LAMPS.iter().rposition(|&above| mishap > above) {
        Some(lamp) => bar(share, LAMPS[lamp], lit(now_ms, hud::LAMP_MS)),
        None => bar(share, STAMINA, true),
    }
}

/// `fighter`'s duel bars under `rules` at `now_ms` (real time): its block
/// points under MDA, else its Force (`force`, a share of its pool) where
/// one is given (over a head; the HUD has the Force bar).
pub(crate) fn bars(
    rules: SaberRules,
    fighter: &Fighter,
    force: Option<f32>,
    now_ms: f64,
) -> DuelBars {
    let duel = &fighter.saber.duel;
    let points = if rules.own_meter() {
        Some(points(duel.points, now_ms))
    } else {
        force.map(|share| bar(share, FORCE, true))
    };
    DuelBars {
        points,
        stamina: stamina(duel.mishap, now_ms),
    }
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
                Update,
                overhead
                    .run_if(resource_exists::<PresentedSnapshot>)
                    .run_if(resource_exists::<DuelOptions>)
                    .before(crate::ui_view::Stepped),
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

/// The duel meters of every saber user standing, for `dump`: the rules,
/// and each one's points, mishaps and guard.
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
            let duel = &fighter.saber.duel;
            Some(serde_json::json!({
                "id": entity.id.0, "hp": fighter.hp, "fp": fighter.fp,
                "points": duel.points, "mishap": duel.mishap,
                "fatigued": duel.fatigued, "drained": duel.drained,
                "block_held": duel.block_held, "aimed": duel.aimed,
                "warn": duel.warn, "broken": duel.broken,
                "perfect_at": duel.perfect_at, "wear": duel.wear,
            }))
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
            let pool = crate::cast::sheet(entity, &objects).pool();
            let force = (pool > 0).then(|| fighter.fp as f32 / (f32::from(pool) * 100.0));
            Some(OverheadView {
                over,
                alpha,
                bars: bars(rules, fighter, force, now_ms),
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
    crate::stealth_view::follow(&mut warned.by, &swinging, &material, &bodies, &mut commands);
}

/// The bars' colours and flashes (`kjk sim probes --only "duel bars"`):
/// block points by band, flashing only when few; stamina grey until the
/// first warning, then its colour, flashing.
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
    let lamp_off = f64::from(hud::LAMP_MS) * 0.75;
    let calm = stamina(hud::LAMPS[0], lamp_off);
    let warned = hud::LAMPS.map(|above| stamina(above + 1, on));
    let warned_off = stamina(hud::LAMPS[3] + 1, lamp_off);
    vec![
        (
            "block points by band, flashing when few",
            check(
                bands == [POINTS[3], POINTS[2], POINTS[1], POINTS[0]]
                    && flashes == [FLASH_OFF, 1.0],
                format!("{bands:?}, flashing {flashes:?}"),
            ),
        ),
        (
            "stamina grey, then each warning's colour, flashing",
            check(
                rgb(calm) == STAMINA
                    && calm.color[3] == 1.0
                    && calm.share == 1.0 - f32::from(hud::LAMPS[0]) / f32::from(hud::STAMINA_STEPS)
                    && warned.map(rgb) == LAMPS
                    && warned_off.color[3] == FLASH_OFF,
                format!("{calm:?}, {warned:?}, off {warned_off:?}"),
            ),
        ),
    ]
}
