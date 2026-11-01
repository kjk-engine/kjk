//! KOTOR's gun feat visuals held while the alternate button is. The
//! executable plays each once on the gun's hook as the attack begins
//! (`blaster_view::FEAT_ROWS`); a charge or an aim here lasts as long as the
//! button, so their clips stand at a moment of them (`blaster_view::Held`),
//! each gun of a pair its own:
//!
//! - A Power Blast's charge (`Shooting::charging_from`): the tier's charge-up
//!   (`v_pb_p_gen`, `v_pb_m_gen`; the first tier, which has none of its own,
//!   the Improved one's), a fountain of `fx_flare01`s quickening to
//!   `CHARGE_HOLD_MS` into its clip and staying there until the shot leaves;
//!   then its clip runs on, its births over, for its flares to die out
//!   (`DIE_OUT_MS`).
//! - A Sniper Shot's aim (`Shooting::aiming`): the laser sight (`v_sn_r_gen`'s
//!   `LazerSight`, a thin red ribbon to its reference node), from just under
//!   the muzzle to where the shot would go (the first wall or body along the
//!   view; KOTOR's stops at its reference node, a metre or so out), once
//!   the gun is raised on it:
//!   drawn out of hand or swung away, its barrel off that way by more than
//!   `RAISED`, none.

use std::collections::BTreeMap;

use bevy::prelude::*;
use collision::{Bodies, CollisionWorld as _, Extents};
use sim::combat::Delivery;
use sim::combat::specials::Special;
use sim::{Instances, SimId, Snapshot};

use crate::blaster_view::{BlasterShots, Held};
use crate::cast::{self, Guns};
use crate::combat_fx::{FightDrawn, gun_muzzle};
use crate::scene::game_to_bevy;
use crate::shots::OwnFire;
use crate::sim_host::{PresentedSnapshot, SceneSwap, view_ms};

const UNIT: f32 = asset_ja::UNIT;
/// How far under the muzzle the laser leaves (metres).
const UNDER: f32 = 0.025;
/// How far a gun's barrel (its root to its muzzle, a few degrees off the
/// bore) may point off its aim and still count as raised on it (degrees):
/// a pair's raised guns stand 12 to 14 off, one being drawn far more.
const RAISED: f32 = 18.0;
/// How far behind its model's origin the sight's emitter stands (metres,
/// `v_sn_r_gen`): the model is placed that far on, for the line to leave
/// from under the muzzle.
const SIGHT_BACK: f32 = 0.1865;
/// How far into its clip the sight stands: its births stop at 1.6 s.
const SIGHT_HOLD_MS: f64 = 1000.0;
/// How far into its clip a charge stands: its births reach their fastest at
/// 0.733 s and stop by 0.8 s.
const CHARGE_HOLD_MS: f64 = 733.0;
/// How long a charge let go stays on the gun: its flares' life.
const DIE_OUT_MS: f64 = 500.0;

pub struct AimFxPlugin;

impl Plugin for AimFxPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            draw.in_set(FightDrawn)
                .after(bevy::transform::TransformSystems::Propagate),
        );
    }
}

/// A charge's or a raised aim's gun: which, and what it shows.
type Gun = (SimId, u8, u16);

/// When each held gun began (the view's ms); each charge let go, when; and
/// the scene they are in.
#[derive(Default)]
struct Since {
    generation: u32,
    held: BTreeMap<Gun, f64>,
    dying: BTreeMap<Gun, f64>,
}

fn draw(
    presented: Option<Res<PresentedSnapshot>>,
    swap: Option<Res<SceneSwap>>,
    (guns, fire, eyes): (Res<Guns>, Res<OwnFire>, Res<crate::first_person::Eyes>),
    places: Query<&GlobalTransform>,
    mut since: Local<Since>,
    mut shots: ResMut<BlasterShots>,
) {
    let (Some(presented), Some(swap)) = (presented, swap) else {
        return;
    };
    if since.generation != swap.generation {
        *since = Since {
            generation: swap.generation,
            ..Since::default()
        };
    }
    let current = presented.0.current();
    let now = view_ms(&presented.0);
    let objects = Instances::in_snapshot(&swap.level, current);
    let states = swap.level.states_in(current);
    let bodies: Vec<collision::Body> = current
        .entities
        .iter()
        .filter(|e| e.fighter.is_some_and(|f| f.alive()))
        .filter_map(|e| {
            let ps = e.player?;
            Some(sim::body(e.id, &ps, sim::bulk_of(objects.of_entity(e))))
        })
        .collect();
    let was = std::mem::take(&mut since.held);
    // A charge before the eyes the camera looks through plays as small as
    // the own muzzle flashes there; the sight keeps its length.
    let charge_scale = |by: SimId| {
        if eyes.0 == Some(by) {
            crate::first_person::FLASH_SCALE
        } else {
            1.0
        }
    };
    for gunner in gunners(current, &objects, &fire) {
        // Where the shot would go (metres, game space), as far as a
        // disruptor's reaches.
        let point = || {
            let to = gunner.eye + gunner.forward * sim::combat::weapons::HITSCAN_RANGE * UNIT;
            let met = swap.level.with(states.as_deref(), |world| {
                Bodies::new(world, &bodies)
                    .skipping(gunner.id.0 | sim::BODY)
                    .trace(
                        gunner.eye.to_array(),
                        to.to_array(),
                        &Extents::POINT,
                        sim::SHOT,
                    )
            });
            Vec3::from_array(met.end)
        };
        let aimed = matches!(gunner.special, Special::SniperShot(_)).then(point);
        for &hand in &gunner.hands {
            let Some(muzzle) = gun_muzzle(&guns, &places, (gunner.id, hand)).map(|at| at * UNIT)
            else {
                continue;
            };
            let row = match aimed {
                Some(_) => sight_row(),
                None => charge_row(gunner.special),
            };
            let Some(row) = row else { continue };
            let (at, way, hold, reach, scale) = match aimed {
                Some(point) => {
                    let from = muzzle - Vec3::Z * UNDER;
                    let way = (point - from).normalize_or(gunner.forward);
                    let raised = barrel(&guns, &places, (gunner.id, hand))
                        .is_some_and(|barrel| barrel.angle_between(way) <= RAISED.to_radians());
                    if !raised {
                        continue;
                    }
                    let reach = from.distance(point);
                    (
                        from + way * SIGHT_BACK,
                        way,
                        SIGHT_HOLD_MS,
                        Some(reach),
                        1.0,
                    )
                }
                None => (
                    muzzle,
                    gunner.forward,
                    CHARGE_HOLD_MS,
                    None,
                    charge_scale(gunner.id),
                ),
            };
            let gun = (gunner.id, hand, row);
            let began = was.get(&gun).copied().unwrap_or(now);
            since.held.insert(gun, began);
            since.dying.remove(&gun);
            shots.held.push(Held {
                by: gunner.id,
                hand,
                row,
                at,
                way,
                since: began,
                hold_ms: Some(hold),
                reach,
                scale,
            });
        }
    }
    // A charge let go runs on where its gun is, for its flares to die out.
    for gun in was.keys().filter(|gun| Some(gun.2) != sight_row()) {
        if !since.held.contains_key(gun) {
            since.dying.insert(*gun, now);
        }
    }
    since.dying.retain(|_, from| now - *from < DIE_OUT_MS);
    for &(by, hand, row) in since.dying.keys() {
        let Some(at) = gun_muzzle(&guns, &places, (by, hand)).map(|at| at * UNIT) else {
            continue;
        };
        let way = barrel(&guns, &places, (by, hand)).unwrap_or(Vec3::Y);
        shots.held.push(Held {
            by,
            hand,
            row,
            at,
            way,
            since: now,
            hold_ms: None,
            reach: None,
            scale: charge_scale(by),
        });
    }
}

/// The charge-up a Power Blast holds: its tier's, the first tier's the
/// Improved one's.
fn charge_row(special: Special) -> Option<u16> {
    let tier = match special {
        Special::PowerBlast(tier) => tier.max(2),
        _ => 2,
    };
    crate::blaster_view::feat_row(Special::PowerBlast(tier))
}

/// The laser sight: KOTOR's first Sniper Shot's row, the sight alone.
fn sight_row() -> Option<u16> {
    crate::blaster_view::feat_row(Special::SniperShot(1))
}

/// Which way `who`'s gun in `hand` points as drawn (game space): from its
/// root to its muzzle.
fn barrel(guns: &Guns, places: &Query<&GlobalTransform>, (who, hand): (SimId, u8)) -> Option<Vec3> {
    let (root, muzzle) = guns.0.get(&who)?[usize::from(hand.min(1))]?;
    let place = places.get(root).ok()?;
    let way = place.transform_point(muzzle) - place.translation();
    Some((game_to_bevy().inverse() * way).normalize_or_zero())
}

/// A gunner charging a Power Blast or aiming a Sniper Shot this frame.
struct Gunner {
    id: SimId,
    special: Special,
    /// The hands holding its guns (both of a pair).
    hands: Vec<u8>,
    /// The view's eye and forward (metres, game space).
    eye: Vec3,
    forward: Vec3,
}

/// The gunners charging or aiming in `current`, the own one as it is at
/// own time (`OwnFire::own`).
fn gunners(current: &Snapshot, objects: &Instances, fire: &OwnFire) -> Vec<Gunner> {
    current
        .entities
        .iter()
        .filter_map(|entity| {
            let entity = match &fire.own {
                Some(own) if Some(entity.id) == fire.me => own,
                _ => entity,
            };
            let ps = entity.player?;
            let shooting = entity.fighter.filter(|f| f.alive())?.shooting;
            if shooting.charging_from.is_none() && !shooting.aiming {
                return None;
            }
            let sheet = cast::sheet(entity, objects);
            let Delivery::Blaster(blaster) = cast::weapon(entity, objects).delivery else {
                return None;
            };
            Some(Gunner {
                id: entity.id,
                special: Special::of(&sheet, blaster)?,
                hands: if sheet.pistol_pair() {
                    vec![0, 1]
                } else {
                    vec![0]
                },
                eye: Vec3::from_array(sim::muzzle(&ps, [0.0; 3])) * UNIT,
                forward: Vec3::from_array(sim::math::angle_vectors(ps.view).0),
            })
        })
        .collect()
}
