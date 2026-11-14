//! The Advanced rules shown (`docs/SABER-RULES.md` sections 6.1 and 12;
//! render side only, from the presented snapshot), while a session plays
//! them:
//!
//! - the Stamina bar (`bars`): a fighter's block points, coloured by how
//!   many are left and flashing when few are. The player's own stands on
//!   the HUD (`hud`). With the "Overhead Stamina bars" option, an enemy in a
//!   fight with the player or the party has it over its head, under a red
//!   health bar, faded with distance (`overhead`). Never a companion, a
//!   neutral or a civilian has one over its head. Every character has a
//!   Stamina bar, whatever it holds in its hands;
//! - the unblockable warning (`warning`, the option off by default): a
//!   character whose swing playing cannot be blocked but by a perfect block
//!   (`Duel::warn`) wears a red shell (`shell`),
//!   never the player's own, nor one in stealth, nor during a conversation.

use std::collections::BTreeMap;

use bevy::prelude::*;
use rules::duel::hud;
use sim::combat::{Delivery, Fighter};
use sim::{Instances, SimId, SimWorld, Snapshot};
use ui::vm::{Bar, DuelBars, OverheadView};

use crate::battle_music::Body;
use crate::cast::{BodyHeights, BodyRoots};
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
/// Only such a character can block. Every character has Stamina.
pub(crate) fn blade_user(entity: &sim::EntityState, objects: &Instances) -> bool {
    matches!(
        crate::cast::weapon(entity, objects).delivery,
        Delivery::Saber(_) | Delivery::Melee
    )
}

pub struct DuelViewPlugin;

impl Plugin for DuelViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Warned>()
            .add_systems(Update, facts.run_if(resource_exists::<PresentedSnapshot>))
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

/// One standing fighter's duel meters, for `dump`: its Stamina and its
/// guard.
fn fact(id: u32, fighter: &Fighter) -> serde_json::Value {
    let duel = &fighter.saber.duel;
    serde_json::json!({
        "id": id, "hp": fighter.hp, "fp": fighter.fp,
        "points": duel.points,
        "target": fighter.target.map(|target| target.0),
        "combat_until": fighter.combat_until,
        "block_held": duel.block_held, "aimed": duel.aimed,
        "warn": duel.warn, "broken": duel.broken,
        "perfect_at": duel.perfect_at, "wear": duel.wear,
    })
}

/// The duel meters of every fighter standing, for `dump`: the rules, and
/// each one's Stamina and guard.
fn facts(presented: Res<PresentedSnapshot>, mut facts: ResMut<console::DumpFacts>) {
    let snapshot = presented.0.current();
    let fighters: Vec<_> = snapshot
        .entities
        .iter()
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
            "party": snapshot
                .entities
                .iter()
                .filter(|entity| snapshot.in_party(entity))
                .map(|entity| entity.id.0)
                .collect::<Vec<_>>(),
            "fighters": fighters,
        }),
    );
}

/// How far over the top of a model its bars stand (metres): the slight
/// offset above the head. `floaty::BARS_OVER` is only the pixels between
/// the Stamina bar and the point this gives.
const OVER_M: f32 = 0.12;
/// The health bar's red.
const HEALTH: [f32; 3] = [0.86, 0.16, 0.12];

/// What the bars' decision knows of the fight, as plain data.
struct Fight {
    now_ms: u64,
    me: Option<SimId>,
    bodies: BTreeMap<u32, Body>,
    party: Vec<SimId>,
    /// The player's target by the crosshair.
    aimed: Option<SimId>,
    /// Whom the player fights, while the player is in combat.
    fought: Option<SimId>,
}

impl Fight {
    fn new(
        (now_ms, me, bodies): (u64, Option<SimId>, &[Body]),
        party: Vec<SimId>,
        aimed: Option<SimId>,
        me_fighter: Option<&Fighter>,
    ) -> Fight {
        Fight {
            now_ms,
            me,
            bodies: bodies.iter().map(|body| (body.id, *body)).collect(),
            party,
            aimed,
            fought: me_fighter
                .filter(|fighter| fighter.in_combat(now_ms))
                .and_then(|fighter| fighter.target),
        }
    }

    /// The fight `snapshot` shows, as `objects` has its sheets: `aimed` is
    /// the number of the object the player's crosshair holds
    /// (`sim::target::number`).
    fn of(
        snapshot: &Snapshot,
        objects: &Instances,
        me: Option<SimId>,
        aimed: Option<u32>,
    ) -> Fight {
        let (bodies, _) = crate::battle_music::bodies_and_blows(snapshot, objects);
        Fight::new(
            (snapshot.time_ms, me, &bodies),
            snapshot
                .entities
                .iter()
                .filter(|entity| snapshot.in_party(entity))
                .map(|entity| entity.id)
                .collect(),
            aimed.and_then(|number| Some(sim::target::entity(snapshot, number)?.id)),
            me.and_then(|me| snapshot.entities.iter().find(|entity| entity.id == me))
                .and_then(|entity| entity.fighter.as_ref()),
        )
    }

    /// Whether bars show over `id`: a living enemy, hostile to the player by
    /// `hostile`, that the player has targeted or fights, or that is in
    /// combat with one of the party. Never a companion, a neutral or a
    /// civilian, nor one whose fight is over.
    fn shows_bars(
        &self,
        id: SimId,
        fighter: &Fighter,
        hostile: &impl Fn(&Body, &Body) -> bool,
    ) -> bool {
        let (Some(foe), Some(player)) = (
            self.bodies.get(&id.0),
            self.me.and_then(|me| self.bodies.get(&me.0)),
        ) else {
            return false;
        };
        let engaged = self.aimed == Some(id)
            || self.fought == Some(id)
            || (fighter.in_combat(self.now_ms)
                && fighter
                    .target
                    .is_some_and(|target| self.party.contains(&target)));
        fighter.hp > 0
            && foe.id != player.id
            && !foe.party
            && foe.alive
            && hostile(player, foe)
            && engaged
    }
}

/// The point the bars stand over: `height` (metres, at the drawn `scale`)
/// over `feet`, plus `OVER_M`, in world space. It does not depend on the
/// camera.
fn anchor(feet: Vec3, height: f32, scale: f32) -> Vec3 {
    feet + Vec3::Y * (height * scale + OVER_M)
}

/// Where `entity` stands in the world as drawn, and the scale it is drawn
/// at: its body root's, else where the snapshot has it, unscaled. A knocked
/// down body keeps its standing height.
fn drawn(
    entity: &sim::EntityState,
    roots: &BodyRoots,
    places: &Query<&GlobalTransform>,
) -> (Vec3, f32) {
    roots
        .0
        .get(&entity.id)
        .and_then(|root| places.get(*root).ok())
        .map_or_else(
            || {
                (
                    crate::scene::game_to_bevy() * Vec3::from_array(entity.position),
                    1.0,
                )
            },
            |root| (root.translation(), root.to_scale_rotation_translation().0.y),
        )
}

/// The point of the window, as shares across and down, that world point `at`
/// is drawn at, while it is on the screen.
fn on_screen(at: Vec3, (place, camera): (&GlobalTransform, &Camera)) -> Option<[f32; 2]> {
    let share = camera.world_to_viewport(place, at).ok()? / camera.logical_viewport_size()?;
    let on = |v: f32| (0.0..=1.0).contains(&v);
    (on(share.x) && on(share.y)).then_some([share.x, share.y])
}

/// Each enemy in a fight with the player or the party has a health bar and
/// a Stamina bar over its head, with the option on in a duel mode.
#[allow(clippy::too_many_arguments)]
fn overhead(
    presented: Res<PresentedSnapshot>,
    options: Res<DuelOptions>,
    (swap, seat): (Option<Res<SceneSwap>>, Option<Res<OwnSeat>>),
    targeted: Option<Res<crate::target_view::Targeted>>,
    (roots, heights): (Res<BodyRoots>, Res<BodyHeights>),
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
    let standing = swap.level.module().map(|module| snapshot.standings(module));
    let hostile = crate::battle_music::hostility(standing);
    let fight = Fight::of(
        snapshot,
        &objects,
        me,
        targeted.as_ref().and_then(|targeted| targeted.object),
    );
    let eye = camera.0.translation();
    let now_ms = real.elapsed_secs_f64() * 1000.0;
    ui.vm.overhead = snapshot
        .entities
        .iter()
        .filter_map(|entity| {
            let fighter = entity.fighter.as_ref()?;
            if !fight.shows_bars(entity.id, fighter, &hostile) {
                return None;
            }
            let sheet = crate::cast::sheet(entity, &objects);
            let (feet, scale) = drawn(entity, &roots, &places);
            let height = heights
                .0
                .get(&entity.id)
                .copied()
                .unwrap_or(crate::cast::HEAD_M);
            let alpha = 1.0 - ((eye.distance(feet) - NEAR_M) / (FAR_M - NEAR_M)).clamp(0.0, 1.0);
            if alpha <= 0.0 {
                return None;
            }
            let over = on_screen(anchor(feet, height, scale), camera)?;
            let health = fighter.hp as f32 / (f32::from(sheet.max_hp).max(1.0) * 100.0);
            Some(OverheadView {
                over,
                alpha,
                bars: bars(fighter, sheet.level, now_ms),
                health: bar(health.clamp(0.0, 1.0), HEALTH, true),
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
/// fills to, out of the maximum the character's level allows; that every
/// character has the Stamina bar and that no older second bar is drawn
/// besides it for a character without the Force; and the facts a dump holds
/// for one fighter, which name one bar.
#[cfg(feature = "probes")]
pub fn probes() -> sim::probe::Checks {
    use sim::combat::Sheet;
    use sim::combat::saber::SaberRules;
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
    // Whatever a character holds, and nothing at all, it has the Stamina bar
    // and, unless it uses the Force, no older second bar.
    let soldier = Sheet::dev_soldier();
    let held = |weapon: Option<sim::combat::Weapon>| {
        let mut sheet = soldier;
        sheet.weapons = [None, weapon];
        sheet
    };
    let plain = |item: u16| {
        sim::combat::Weapon::plain(item, 1, 1, (1, 1), 0, [feats::PROFICIENCY_MELEE; 3])
    };
    // Base items 1 (a stun baton), 78 (a warblade), 79 (a gaffi stick) and 81
    // to 83 (a creature's claws, bite and gore).
    let mut holders = vec![("the made-up soldier", soldier), ("fists", held(None))];
    holders.extend([1, 78, 79, 81, 82, 83].map(|item| ("a melee weapon", held(Some(plain(item))))));
    let mut jedi = Sheet::dev_jedi();
    jedi.weapons = [None, None];
    let wrong: Vec<&str> = holders
        .iter()
        .filter(|(_, sheet)| {
            crate::hud::gauges(SaberRules::DuelAdvanced, sheet)
                != crate::hud::Gauges {
                    second: false,
                    stamina: true,
                }
        })
        .map(|(name, _)| *name)
        .collect();
    let rows = [
        (SaberRules::DuelAdvanced, soldier, (false, true)),
        (SaberRules::DuelAdvanced, jedi, (true, true)),
        (SaberRules::Ja, soldier, (true, false)),
        (SaberRules::Ja, jedi, (true, false)),
    ];
    let bars: Vec<_> = rows
        .iter()
        .map(|(rules, sheet, _)| {
            let gauges = crate::hud::gauges(*rules, sheet);
            (gauges.second, gauges.stamina)
        })
        .collect();
    let rows_right = rows.iter().map(|row| row.2).eq(bars.iter().copied());
    let flashes = low.map(|bar| bar.color[3]);
    let shown = fact(1, &Fighter::fresh(10_000, 10));
    let keys: Vec<String> = match shown.as_object() {
        Some(fact) => fact.keys().cloned().collect(),
        None => Vec::new(),
    };
    let one_bar = keys.iter().any(|key| key == "points")
        && !keys.iter().any(|key| key == "mishap" || key == "stamina");
    // The fight: the player (1), a companion (2), enemies (3 to 6) and a
    // neutral who becomes hostile (7), with the player in combat.
    let body = |id, party, faction| Body {
        id,
        at: [0.0; 3],
        seat: id == 1,
        party,
        alive: true,
        faction: Some(faction),
    };
    let bodies = [
        body(1, true, 1),
        body(2, true, 1),
        body(3, false, 2),
        body(4, false, 2),
        body(5, false, 3),
        body(6, false, 2),
        body(7, false, 3),
    ];
    // Hostile: faction 2 to the party; faction 3 only once a standing change
    // (here `turned`) makes it so.
    let turned = std::cell::Cell::new(false);
    let hostile = |_: &Body, other: &Body| {
        other.faction == Some(2) || (turned.get() && other.faction == Some(3))
    };
    let now: u64 = 10_000;
    let fighter = |target: Option<u32>, combat: bool| {
        let mut fighter = Fighter::fresh(10_000, 10);
        fighter.target = target.map(SimId);
        fighter.combat_until = if combat { now + 5_000 } else { 0 };
        fighter
    };
    let fight = |aimed: Option<SimId>, player: &Fighter| {
        Fight::new(
            (now, Some(SimId(1)), &bodies),
            vec![SimId(1), SimId(2)],
            aimed,
            Some(player),
        )
    };
    let idle = fighter(None, false);
    let rows: Vec<(&str, bool, bool)> = {
        let calm = fight(None, &idle);
        let ranged = fight(Some(SimId(3)), &idle);
        let fighting = fight(None, &fighter(Some(6), true));
        let over = fight(None, &fighter(Some(6), false));
        let mut rows = vec![
            (
                "a companion in combat",
                calm.shows_bars(SimId(2), &fighter(Some(3), true), &hostile),
                false,
            ),
            (
                "a civilian in combat with another civilian",
                calm.shows_bars(SimId(5), &fighter(Some(7), true), &hostile),
                false,
            ),
            (
                "an idle enemy",
                calm.shows_bars(SimId(3), &idle, &hostile),
                false,
            ),
            (
                "an enemy targeted at range",
                ranged.shows_bars(SimId(3), &idle, &hostile),
                true,
            ),
            (
                "an enemy hitting a companion",
                calm.shows_bars(SimId(4), &fighter(Some(2), true), &hostile),
                true,
            ),
            (
                "an enemy that stopped hitting a companion",
                calm.shows_bars(SimId(4), &fighter(Some(2), false), &hostile),
                false,
            ),
            (
                "an enemy the player fights",
                fighting.shows_bars(SimId(6), &idle, &hostile),
                true,
            ),
            (
                "an enemy the player fought, the fight over",
                over.shows_bars(SimId(6), &idle, &hostile),
                false,
            ),
            (
                "the player",
                ranged.shows_bars(SimId(1), &fighter(Some(3), true), &hostile),
                false,
            ),
        ];
        rows.push((
            "a neutral, still neutral, hitting a companion",
            calm.shows_bars(SimId(7), &fighter(Some(2), true), &hostile),
            false,
        ));
        turned.set(true);
        rows.push((
            "a neutral made hostile, hitting a companion",
            calm.shows_bars(SimId(7), &fighter(Some(2), true), &hostile),
            true,
        ));
        rows
    };
    let wrong_who: Vec<&str> = rows
        .iter()
        .filter(|(_, got, want)| got != want)
        .map(|(name, _, _)| *name)
        .collect();
    // A droid, a human and a tall creature, standing on the same floor, and
    // a creature drawn at twice its size.
    let [droid, human, tall] = [0.9_f32, 1.8, 3.2].map(|height| anchor(Vec3::ZERO, height, 1.0));
    let big = anchor(Vec3::ZERO, 1.8, 2.0);
    let heads = [(droid, 0.9), (human, 1.8), (tall, 3.2), (big, 3.6)];
    let over_heads = heads
        .iter()
        .all(|(at, height)| (at.y - height - OVER_M).abs() < 1e-5 && at.x == 0.0 && at.z == 0.0);
    let ordered = droid.y < human.y && human.y < tall.y;
    let real = real_fight();
    vec![
        (
            "bars_over_foes_of_a_real_arena_fight",
            check(real.is_ok(), real.unwrap_or_else(|error| error)),
        ),
        (
            "bars_only_over_engaged_enemies",
            check(
                wrong_who.is_empty(),
                format!("overhead bars are wrong for {wrong_who:?}"),
            ),
        ),
        (
            "bars_anchor_over_the_head",
            check(
                over_heads && ordered && droid.y < 1.8,
                format!(
                    "the anchor of a 0.9 m droid, a 1.8 m human, a 3.2 m creature and a 1.8 m \
                     model drawn twice as large stands {OVER_M} m over each head: {droid:?}, \
                     {human:?}, {tall:?}, {big:?}"
                ),
            ),
        ),
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
            "every_character_has_a_bar",
            check(
                wrong.is_empty(),
                format!(
                    "under the Advanced rules a soldier with a blaster, a stun baton, a melee \
                     weapon or bare fists has the Stamina bar and no second bar; not so for \
                     {wrong:?}"
                ),
            ),
        ),
        (
            "one_bar_without_the_force",
            check(
                rows_right,
                format!(
                    "(second bar, Stamina) drawn for a soldier and a Jedi under the Advanced \
                     rules, then under Jedi Academy's: {bars:?}"
                ),
            ),
        ),
    ]
}

/// Two foes called into the proving ground's arena, in the snapshot of its
/// world. Each stands hostile to the player. With no fight yet neither has
/// bars. With the crosshair on one by its number (not its `SimId`) only that
/// one has them. Once the foe is in combat with the player it has them, and
/// it has none once its combat is over. The player's own number picks no one.
#[cfg(feature = "probes")]
fn real_fight() -> Result<String, String> {
    use sim::combat::Sheet;
    use sim::combat::sheet::{factions, flags};
    use sim::module::ObjectData;
    use sim::{Command, Seat, StepReason, TickInput, WorldConfig};
    const FOE: &str = "bars_foe";
    let ground = sim::determinism::proving_ground();
    let entry = ground.module().ok_or("the ground has no module")?.entry;
    let rules = sim::arena::Rules {
        respawn: entry.position,
        yaw: entry.yaw,
        pit: -3.0,
        respawn_ms: 500,
    };
    let mut foe = ObjectData::creature(FOE, FOE, [0.0; 3], Sheet::dev_soldier());
    if let Some(sheet) = foe.sheet.as_mut() {
        sheet.faction = factions::PLAYER;
        sheet.flags |= flags::PLOT;
    }
    let library = sim::probe::library_of([foe], sim::inventory::Catalog::default());
    let level = ground.with_scripts(library).with_arena(rules);
    let mut world =
        SimWorld::new(WorldConfig::one_seat(), level.clone()).map_err(|error| error.to_string())?;
    let mut snapshot = world.snapshot();
    for step in 0..4 {
        let mut input = TickInput::default();
        input.insert(Seat(0), Command::default());
        if step == 0 {
            for _ in 0..2 {
                input.act(
                    Seat(0),
                    sim::action::Action::Arena(sim::action::ArenaOrder::Spawn(FOE.into())),
                );
            }
        }
        let tick = world.tick() + 1;
        snapshot = sim::step(&mut world, tick, &input, 8, StepReason::Authority);
    }
    let me = crate::cast::me(&snapshot, Seat(0)).ok_or("no player")?;
    let objects = Instances::in_snapshot(&level, &snapshot);
    // The proving ground has no reputation table, so sides stand hostile
    // when their factions differ.
    let hostile = |a: &Body, b: &Body| a.faction != b.faction;
    // The foes: the creatures called in, with the number the crosshair
    // names them by.
    let foes: Vec<_> = snapshot
        .entities
        .iter()
        .filter(|entity| {
            entity.id != me
                && entity
                    .sheet
                    .is_some_and(|sheet| sheet.faction == factions::HOSTILE_1)
        })
        .filter_map(|entity| Some((entity, sim::target::number(entity)?)))
        .collect();
    // Which foes show bars with the crosshair on `aimed`, and each foe in
    // combat with the player (`fighting`) or not.
    let bars_in = |aimed: Option<u32>, fighting: bool| -> Vec<bool> {
        let fight = Fight::of(&snapshot, &objects, Some(me), aimed);
        foes.iter()
            .filter_map(|(foe, _)| {
                let mut fighter = foe.fighter?;
                if fighting {
                    fighter.target = Some(me);
                    fighter.combat_until = snapshot.time_ms + 1_000;
                }
                Some(fight.shows_bars(foe.id, &fighter, &hostile))
            })
            .collect()
    };
    let calm = bars_in(None, false);
    let picked: Vec<Vec<bool>> = foes
        .iter()
        .map(|(_, number)| bars_in(Some(*number), false))
        .collect();
    let fighting = bars_in(None, true);
    let own = bars_in(Some(sim::target::AVATARS), false);
    let distinct = foes.iter().any(|(foe, number)| *number != foe.id.0);
    let only_the_picked = picked.iter().enumerate().all(|(nth, shown)| {
        shown
            .iter()
            .enumerate()
            .all(|(other, shows)| *shows == (other == nth))
    });
    let ids: Vec<_> = foes
        .iter()
        .map(|(foe, number)| (foe.id.0, *number))
        .collect();
    let detail = format!(
        "foes (id, crosshair number) {ids:?}: bars with no fight {calm:?}; with the crosshair on \
         each {picked:?}; in combat with the player {fighting:?}; on the player's own number {own:?}"
    );
    let ok = foes.len() == 2
        && distinct
        && calm.iter().all(|shows| !shows)
        && only_the_picked
        && fighting.iter().all(|shows| *shows)
        && own.iter().all(|shows| !shows);
    if ok { Ok(detail) } else { Err(detail) }
}
