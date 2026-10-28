//! KOTOR's own attack, after the blades: for creatures JA's swings cannot
//! drive (a model of type S or L in `appearance.2da`: beasts, droids and
//! aliens with bones of their own; `module::Strike`), a katarn's gore, a
//! kath hound's bite, a rancor's claws; and for a humanoid with nothing in
//! its hands, its fists. On the authority and in replays, and in prediction
//! for the players' own (as their shots: `fire`).
//!
//! A creature with a strike and nothing in its hands (its weapon timer
//! runs in move_body's plain step, as fists' does) begins an attack when its
//! mind holds attack and its target stands within its reach: its personal
//! space (`perspace`) and its `hitdist` past it, to the target's body. The
//! attack takes a round (`damage::ROUND_MS`: KOTOR's one attack a round;
//! Knight and Master Speed's extra attacks scale the hit's budget in `hurt`,
//! as for every weapon hit, C2), and its blow lands at its model's hit
//! moment (`Strike::hit_ms`) on the target, if it is still within reach: a
//! hit of its creature weapon (bare, `Weapon::UNARMED`, without one)
//! through the pipeline every weapon hit takes (`combat::damage`). Both
//! sides count as in combat from then (`hurt`), and the target hears it was
//! attacked (its `last_attacker`, which raises its `OnAttacked`).
//!
//! A creature of these types holding a weapon fights with it as a humanoid
//! does (a Gamorrean's axe swings JA's way); humanoids (types B and F) never
//! strike so.
//!
//! A humanoid with nothing in its hands, a player or a creature, punches
//! (as JA's melee weapon does) with its fists, `Weapon::UNARMED`: `BOTH_MELEE1` and
//! `BOTH_MELEE2` in turn on the torso (by `SaberState::chain`'s parity, which
//! a change of weapons puts back to the first), held their length, so the
//! snapshot carries the punch to every view. A player's attack throws one
//! whoever stands there; a mind's, as a beast's, only with its target
//! within reach. A punch is a beast's attack in all else: its reach a
//! body's personal space (its appearance's; a player's a humanoid's,
//! `HUMAN_SPACE_M`) and `FIST_M` past it, about as far as JA's punch
//! reaches (20 units out, 8 on, a 6-unit box), its blow halfway through the
//! punch (`punch_hit_ms`), a round's cadence. Its blow lands on the foe in
//! front within reach (`FRONT`): its target if that is one, else the
//! nearest; with friendly fire off the party's fists pass through the party
//! as its shots do (`Targets::met_by`).
//!
//! A stun baton (`baseitems.2da` row 1, TSL's plasma torch among them: no
//! blade the weapon step swings, `combat::saber::Held::Other`) strikes as
//! the fists do, its blow a hit of the baton itself, so the party breaks a
//! door with it as with any blow (`objects`; the torch's Door Cutting).

use bevy_ecs::prelude::*;
use collision::UNIT;

use super::{
    Avatar, Clock, Fighting, Hit, Hits, Id, Object, Player, PlayerSheet, Static, StepReason, held,
    in_party, sheet_of,
};
use crate::combat::Weapon;
use crate::combat::damage::{FriendlyFire, ROUND_MS, Source};
use crate::combat::items;
use crate::combat::mind;
use crate::combat::saber::Change;
use crate::combat::sheet::Delivery;
use crate::input::buttons;
use crate::math::{self, Vec3};
use crate::module::Strike;
use crate::motion::{self, Anim, AnimTable};
use crate::snapshot::SimId;

/// One attack a round (ms).
const CADENCE_MS: i32 = ROUND_MS as i32;

/// How far past its personal space a punch lands (metres).
const FIST_M: f32 = rules::creatures::fists::REACH_M;
/// A humanoid's personal space where nothing gives one (a player's body,
/// whose appearance the sim does not hold).
const HUMAN_SPACE_M: f32 = rules::creatures::fists::HUMAN_SPACE_M;
/// A punch lands on a body this near where its thrower looks (the cosine).
const FRONT: f32 = rules::creatures::fists::FRONT;

/// How far `strike`'s blows reach from the striker's centre to the
/// target's (units): its personal space and `hitdist` (metres), and the
/// target's body.
fn reach(strike: &Strike, personal_space: f32) -> f32 {
    (personal_space + strike.hit_distance) / UNIT + motion::HALF_WIDTH
}

/// How far a humanoid's punch reaches (units, as `reach`), from its
/// personal space (metres; 0 for none known).
pub(super) fn punch_reach(personal_space: f32) -> f32 {
    reach(&fists(0), humanoid_space(personal_space))
}

/// A humanoid's personal space: its own, else `HUMAN_SPACE_M`.
pub(crate) fn humanoid_space(personal_space: f32) -> f32 {
    if personal_space > 0.0 {
        personal_space
    } else {
        HUMAN_SPACE_M
    }
}

/// The fists as a strike, its blow `hit_ms` into the punch.
fn fists(hit_ms: u16) -> Strike {
    Strike {
        weapon: None,
        hit_distance: FIST_M,
        hit_ms,
    }
}

/// The punch thrown after `chain` punches: the left, then the right (JA's
/// weapon step alternates them).
fn punch(chain: u8) -> Anim {
    if chain.is_multiple_of(2) {
        Anim::BOTH_MELEE1
    } else {
        Anim::BOTH_MELEE2
    }
}

/// When `anim`'s blow lands (ms into it): halfway, the fist out; half a
/// round where the table lacks it.
fn punch_hit_ms(anims: &AnimTable, anim: Anim) -> u16 {
    let length = anims.get(anim).length_ms();
    let length = if length > 0 { length } else { CADENCE_MS };
    (length / 2).clamp(0, i32::from(u16::MAX)) as u16
}

/// A character as a blow may meet it: its place, its chest, whether it
/// lives, whether it is of the party.
#[derive(Clone, Copy)]
struct Standing {
    id: SimId,
    origin: Vec3,
    chest: Vec3,
    alive: bool,
    party: bool,
}

/// The strikes system (see the module documentation).
#[allow(clippy::type_complexity)]
pub(super) fn strikes(
    clock: Res<Clock>,
    level: Res<Static>,
    mut hits: ResMut<Hits>,
    scripts: Res<super::scripts::Scripts>,
    mut actors: Query<(
        &Id,
        Option<&Avatar>,
        Option<&Object>,
        Option<&PlayerSheet>,
        &mut Player,
        &mut Fighting,
    )>,
    placed: Query<(&Id, &Object), Without<Player>>,
) {
    let now = clock.time_ms;
    let msec = clock.msec as i32;
    let anims = &level.0.anims;
    // Everyone as they stand after the moves, by id.
    let mut bodies: Vec<Standing> = actors
        .iter()
        .map(|(id, avatar, _, _, player, fighting)| {
            let ps = &player.0;
            Standing {
                id: id.0,
                origin: ps.origin,
                chest: mind::chest(ps),
                alive: fighting.0.alive(),
                party: in_party(avatar, &fighting.0),
            }
        })
        .collect();
    bodies.sort_by_key(|body| body.id);
    let bashable = super::objects::bodies(placed.iter());
    // The companion leading the party (`lent`) is its player's hands: it
    // throws a punch at their press, and punches what may be bashed, as
    // their own body does (TSL's Bao-Dur led to strike a force field open:
    // `a_ffield_baodur`).
    let lent = super::lent(&scripts.state.party);
    let mut rows: Vec<_> = actors.iter_mut().collect();
    rows.sort_by_key(|(id, ..)| id.0);
    for (id, avatar, object, sheet, mut player, mut fighting) in rows {
        if clock.reason == StepReason::Predict && avatar.is_none() {
            continue;
        }
        let facts = object.and_then(|object| object.facts.as_ref());
        let fighter = &mut fighting.0;
        let sheet = sheet_of(sheet, object);
        let in_hand = held(&sheet, fighter);
        if !fighter.alive() || !matches!(in_hand.delivery, Delivery::Unarmed | Delivery::Baton) {
            continue;
        }
        let personal_space = facts.map_or(0.0, |facts| facts.personal_space);
        let beast = facts.and_then(|facts| facts.strike);
        // A punch under way is the one before the chain's next; a stun
        // baton's (or TSL's plasma torch's) is a blow of what is in hand.
        let (strike, space) = match beast {
            Some(strike) => (strike, personal_space),
            None => (
                Strike {
                    weapon: (in_hand.delivery == Delivery::Baton).then_some(in_hand),
                    ..fists(punch_hit_ms(
                        anims,
                        punch(fighter.saber.chain.wrapping_add(1)),
                    ))
                },
                humanoid_space(personal_space),
            ),
        };
        let ps = &player.0;
        let reach = reach(&strike, space);
        // Across the floor, as a mind closes in; not a floor away.
        let within = |body: &Standing| {
            let [x, y, z] = math::sub(body.origin, ps.origin);
            body.alive && (x * x + y * y).sqrt() <= reach && z.abs() <= reach
        };
        let target = fighter
            .target
            .and_then(|target| bodies.binary_search_by_key(&target, |body| body.id).ok())
            .map(|at| bodies[at])
            .filter(within);
        let held_up = fighter.force.stunned(now) || fighter.pain_until > now;
        let wait = fighter.weapon_wait_ms;
        if wait > 0 {
            // The blow, the step the attack passes its hit moment.
            let mark = (CADENCE_MS - i32::from(strike.hit_ms)).max(1);
            let landing = wait <= mark && wait + msec > mark;
            let foe = match beast {
                _ if !landing || held_up => None,
                Some(_) => target,
                None => {
                    let party = in_party(avatar, fighter) || lent == Some(id.0);
                    let apart = level.0.friendly_fire == FriendlyFire::Off && party;
                    // The party punches what may be bashed too (`objects`).
                    let near: Vec<Standing> = match party {
                        true => (bodies.iter().copied())
                            .chain(bashable.iter().map(|body| nearest(body, ps.origin)))
                            .collect(),
                        false => bodies.clone(),
                    };
                    in_front(ps, &near, id.0, target.map(|body| body.id), |body| {
                        within(body) && !(apart && body.party)
                    })
                }
            };
            if let Some(foe) = foe {
                let mut weapon = strike.weapon.unwrap_or(Weapon::UNARMED);
                weapon.damage_bonus = weapon
                    .damage_bonus
                    .saturating_add(items::stim_damage(&fighter.pack));
                let mut direction = math::sub(foe.origin, ps.origin);
                direction[2] = 0.0;
                math::normalize(&mut direction);
                hits.0.push(Hit {
                    attacker: id.0,
                    target: foe.id,
                    source: Source::Weapon {
                        weapon,
                        cadence_ms: CADENCE_MS as u32,
                        share: 1000,
                        area: false,
                    },
                    // Its chest, on the striker's side.
                    point: Some(math::mad(foe.chest, -motion::HALF_WIDTH, direction)),
                    direction: Some(direction),
                });
            }
            continue;
        }
        if fighter.last_command.buttons & buttons::ATTACK == 0 || held_up {
            continue;
        }
        let force = &fighter.force;
        let saber = &fighter.saber;
        let starts = match beast {
            Some(_) => target.is_some(),
            // Not gripped, its hand out, changing weapons or down (as a
            // shot waits: `fire`).
            None => {
                (avatar.is_some() || lent == Some(id.0) || target.is_some())
                    && force.gripped_by.is_none()
                    && !force.hand_out(now)
                    && saber.change == Change::None
                    && !saber.floored()
            }
        };
        if !starts {
            continue;
        }
        // A new attack; the timer's overrun carries into it, so the rounds
        // keep their length.
        fighter.weapon_wait_ms = wait.max(-msec) + CADENCE_MS;
        fighter.combat_until = now + super::COMBAT_MS;
        if beast.is_none() {
            let chain = fighter.saber.chain;
            fighter.saber.chain = (chain % 2) ^ 1;
            // Attacking shows a mind-tricking character again (JA).
            fighter.force.tricked = 0;
            motion::react(&mut player.0, anims, punch(chain), true);
        }
    }
}

/// A door or placeable that may be bashed (`body`, metres) as a punch
/// meets it: at its point nearest a striker standing at `origin` (units),
/// as though a body stood there.
fn nearest(body: &collision::Body, origin: Vec3) -> Standing {
    let at: Vec3 = std::array::from_fn(|i| (origin[i] * UNIT).clamp(body.min[i], body.max[i]));
    let point = math::scale(at, 1.0 / UNIT);
    Standing {
        id: SimId(body.entity & !super::BODY),
        origin: point,
        chest: point,
        alive: true,
        party: false,
    }
}

/// The body in front of `ps` (`FRONT`) that `meets` lets a punch land on:
/// `target` if it is one, else the nearest (the lowest id of the nearest),
/// never the thrower `own`.
fn in_front(
    ps: &motion::MotionState,
    bodies: &[Standing],
    own: SimId,
    target: Option<SimId>,
    meets: impl Fn(&Standing) -> bool,
) -> Option<Standing> {
    let (forward, ..) = math::angle_vectors([0.0, ps.view[1], 0.0]);
    let flat = |body: &Standing| {
        let mut way = math::sub(body.origin, ps.origin);
        way[2] = 0.0;
        way
    };
    let mut met = bodies.iter().filter(|body| {
        let mut way = flat(body);
        let apart = math::normalize(&mut way);
        body.id != own && meets(body) && (apart == 0.0 || math::dot(way, forward) >= FRONT)
    });
    let nearest = met
        .clone()
        .min_by(|a, b| math::length(flat(a)).total_cmp(&math::length(flat(b))));
    met.find(|body| Some(body.id) == target)
        .or(nearest)
        .copied()
}

#[cfg(feature = "probes")]
mod probes;
#[cfg(feature = "probes")]
pub use probes::{blade_taken_probes, fist_probes, hold_probes, probes, sheath_probes};
