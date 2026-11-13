//! The Advanced rules shown (`docs/SABER-RULES.md` sections 6.1 and 12;
//! render side only, from the presented snapshot), while a session plays
//! them:
//!
//! - the Stamina bar (`bars`): a fighter's block points, coloured by how
//!   many are left and flashing when few are. The player's own stands on
//!   the HUD (`hud`), every other saber user's over its head with the
//!   "Overhead Stamina bars" option, faded with distance (`overhead`);
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

/// The Stamina bar of `points` left out of a maximum of `cap`, the most
/// the character's level lets them hold (`duel::standing::cap`). The bar is
/// never drawn past full, so a character whose Stamina stands over their
/// maximum for a step shows a full bar.
pub(crate) fn points(points: u8, cap: u8, now_ms: f64) -> Bar {
    let band = hud::GAUGE_BANDS
        .iter()
        .position(|&under| points < under)
        .unwrap_or(hud::GAUGE_BANDS.len());
    let share = (f32::from(points) / f32::from(cap.max(1))).min(1.0);
    let on = points >= hud::GAUGE_FLASH || lit(now_ms, hud::GAUGE_FLASH_MS);
    bar(share, POINTS[band], on)
}

/// `fighter`'s Stamina bar at `now_ms` (real time), out of the maximum its
/// `level` allows.
pub(crate) fn bars(fighter: &Fighter, level: u8, now_ms: f64) -> DuelBars {
    DuelBars {
        points: points(
            fighter.saber.duel.points,
            sim::combat::saber::duel::standing::cap(level),
            now_ms,
        ),
    }
}

/// Whether `entity` holds a blade that a block can be held with: a
/// lightsaber, or a melee blade such as a sword, a vibroblade or a staff.
/// Under the Advanced rules every such character has a Stamina bar.
pub(crate) fn blade_user(entity: &sim::EntityState, objects: &Instances) -> bool {
    matches!(
        crate::cast::weapon(entity, objects).delivery,
        Delivery::Saber(_) | Delivery::Melee
    )
}

/// Whether `sheet`'s character shows a Stamina bar: it holds a blade in
/// the hand it fights with (`hand`), which is a lightsaber, a sword, a
/// vibroblade or a staff. A character with a blaster, a stun baton or
/// nothing in that hand has no bar, as none of those can hold a block.
pub(crate) fn has_bar(sheet: &sim::combat::Sheet, hand: u8) -> bool {
    sim::combat::saber::Kit::of(sheet).blade(hand).is_some()
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
        .filter(|entity| blade_user(entity, &objects))
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
        .filter(|entity| Some(entity.id) != me && blade_user(entity, &objects))
        .filter_map(|entity| {
            let fighter = entity.fighter.as_ref().filter(|fighter| fighter.hp > 0)?;
            let level = crate::cast::sheet(entity, &objects).level;
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
                bars: bars(fighter, level, now_ms),
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
/// the block points by band, flashing only when few; the share the bar
/// fills to, out of the maximum the character's level allows; which
/// characters have a bar at all; and the facts a dump holds for one
/// fighter, which name one bar.
#[cfg(feature = "probes")]
pub fn probes() -> sim::probe::Checks {
    use sim::combat::Sheet;
    use sim::combat::saber::duel::standing;
    use sim::combat::sheet::feats;
    use sim::probe::check;
    let full = rules::duel::meters::POINTS_MAX;
    let (on, off) = (0.0, f64::from(hud::GAUGE_FLASH_MS) * 0.75);
    let rgb = |bar: Bar| [bar.color[0], bar.color[1], bar.color[2]];
    let bands = [100, 60, 30, 10].map(|left| rgb(points(left, full, on)));
    let low = [
        points(hud::GAUGE_FLASH - 1, full, off),
        points(hud::GAUGE_FLASH, full, off),
    ];
    // A level 1 character holds 62 Stamina at most: a bar of 31 is half
    // full, and one over the maximum is drawn full, never past it.
    let level_one = standing::cap(1);
    let shares = [
        points(level_one, level_one, on).share,
        points(full, level_one, on).share,
        points(31, level_one, on).share,
    ];
    let share_right = shares[0] == 1.0 && shares[1] == 1.0 && shares[2] == 0.5;
    // The made-up soldier holds a blaster pistol in its right hand and a
    // vibroblade in its left: the vibroblade has a bar, the blaster none.
    let soldier = Sheet::dev_soldier();
    let (blade_bar, gun_bar) = (has_bar(&soldier, 1), has_bar(&soldier, 0));
    // A stun baton (base item 1) has no bar, because it does not swing with
    // the blade's move tables and so cannot hold a block.
    let mut with_baton = soldier;
    with_baton.weapons[1] = Some(sim::combat::Weapon::plain(
        1,
        1,
        1,
        (1, 1),
        0,
        [feats::PROFICIENCY_MELEE; 3],
    ));
    let baton = with_baton.weapons[1].map(|weapon| weapon.delivery);
    let baton_bar = has_bar(&with_baton, 1);
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
        (
            "bar_share_by_cap",
            check(
                share_right,
                format!(
                    "a level 1 character holds {level_one} Stamina at most: {level_one} fills \
                     {}, {full} fills {}, and 31 fills {}",
                    shares[0], shares[1], shares[2]
                ),
            ),
        ),
        (
            "vibroblade_user_has_a_bar",
            check(
                blade_bar && !gun_bar,
                format!(
                    "the soldier's vibroblade hand has a Stamina bar ({blade_bar}), its blaster \
                     hand has none ({gun_bar})"
                ),
            ),
        ),
        (
            "stun_baton_has_no_bar_yet",
            check(
                !baton_bar,
                format!(
                    "a {baton:?} weapon in hand has a Stamina bar: {baton_bar}. A stun baton does \
                     not swing with the blade's move tables, so it holds no block and shows no \
                     bar; the spec asks for one and a later task builds it"
                ),
            ),
        ),
    ]
}
