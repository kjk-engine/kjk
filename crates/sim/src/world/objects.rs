//! Doors and placeables hurt (KOTOR's Bash, `docs/COMBAT.md`): on the
//! authority, after `hurt`, each hit that found no character lands here if
//! it names a door or placeable with hit points
//! (`ObjectData::hit_points`, less `ObjectState::damage`).
//!
//! - Nothing hurts a `Plot` object, nor one already broken.
//! - A script's damage (`EffectDamage`, `EffectDeath`) hurts any other.
//! - A party member's weapon or grenade hurts only what may be bashed
//!   (`ObjectState::bashable`: locked); others' blows do not, so a fight in
//!   a corridor breaks none of its doors. A creature bashes only when a
//!   script asks (`DoDoorAction(…, DOOR_ACTION_BASH)`: `k_ai_master` for a
//!   door in its way): a KOTOR attack of what it has in hand a round
//!   (`script::State::bashes`), with a swing.
//! - A script's attack of a door or placeable (`ActionAttack` on an
//!   object, a player's too: TSL's Peragus shift console) strikes the same
//!   way, a blow a round within reach, and hurts it locked or not.
//! - Any grenade's blast reaches what may be bashed (`world::fly`), KOTOR's
//!   grenade script asking for creatures alone.
//! - A blow is worth what it would be against a body that does not dodge
//!   (KOTOR's best chance, 95%) whose every damage type resists the
//!   object's `Hardness` (`combat::damage::assess`), so a fast weapon's
//!   share of a round loses its share of the hardness too.
//! - A weapon's or grenade's blow then counts `BASH_TIMES` over: the
//!   owner's pace (2026-10-06), a door broken in 10 to 15 s of swinging.
//! - `Min1HP` holds it at 1 hit point; at none it **breaks**: unlocked, and
//!   a door shut opens away from the blow's maker, for good (`opens`
//!   refuses to close it again); a placeable that holds things opens, any
//!   other dies (its look, `snapshot::look`), as KOTOR's executable has it.
//!
//! - A weapon with TSL's Door Cutting (the plasma torch) takes all its hit
//!   points at a blow, whatever its hardness.
//!
//! What it did is the scripts' (`Battered`, raised in `world::scripts`):
//! `OnMeleeAttacked` with `GetLastAttacker` on every weapon's blow, one
//! its hardness turned whole too; `OnDamaged` with `GetLastDamager` on one
//! that told, and on breaking `OnDeath` with `GetLastKiller`.

use bevy_ecs::prelude::*;

use collision::{Body, Contents};

use super::{
    Avatar, BODY, Clock, Fighting, Hit, Id, Object, Player, PlayerSheet, Static, StepReason,
    Streams,
};
use crate::combat::damage::{self, Blow, Relation, Source};
use crate::combat::gear::weapon_traits;
use crate::combat::sheet::Delivery;
use crate::combat::sheet::{DAMAGE_TYPES, Guards};
use crate::combat::{Location, Sheet};
use crate::math;
use crate::module::{ObjectData, ObjectKind};
use crate::motion::Anim;
use crate::snapshot::{ObjectState, SimId, lock, look};

/// How many times over a weapon's or grenade's blow counts on an object:
/// the owner's pace for JA-style bashing, over KOTOR's own rule, which the
/// rest keeps: the object's `HP` and `Hardness` from its template, the
/// weapon's damage through the pipeline.
pub(crate) const BASH_TIMES: i32 = rules::interaction::bashing::TIMES;

/// What a blow on an object found: the object by its id (a hit that found
/// no character, `hurt`), or by its number among the level's entities (a
/// shot stopped on its brushes).
#[derive(Debug, Clone, Copy)]
pub(super) enum Struck {
    Id(SimId),
    Entity(u32),
}

/// One blow on an object, waiting for `objects`: what struck, or none for
/// a script's bash (`script::State::bashes`), a KOTOR attack of what its
/// maker has in hand, which may land though its maker is no party member.
#[derive(Debug, Clone, Copy)]
pub(super) struct Bash {
    struck: Struck,
    attacker: SimId,
    source: Option<Source>,
}

/// The step's blows on objects; empty between steps.
#[derive(Resource, Default)]
pub(super) struct Battering(Vec<Bash>);

impl Battering {
    /// A hit that found no character.
    pub(super) fn hit(&mut self, hit: &Hit) {
        self.0.push(Bash {
            struck: Struck::Id(hit.target),
            attacker: hit.attacker,
            source: Some(hit.source),
        });
    }

    /// A shot of `attacker`'s stopped on the level's `entity` (none, or a
    /// body: nothing here).
    pub(super) fn shot(&mut self, entity: Option<u32>, attacker: SimId, source: Source) {
        if let Some(entity) = entity.filter(|entity| entity & BODY == 0) {
            self.0.push(Bash {
                struck: Struck::Entity(entity),
                attacker,
                source: Some(source),
            });
        }
    }
}

/// Each object that may be bashed among `objects`: its number among the
/// level's entities (what a blade's trace meets of it) and its id.
/// In id order, as every list the sim's dice follow.
pub(super) fn bashable<'a>(
    objects: impl Iterator<Item = (&'a Id, &'a Object)>,
) -> Vec<(u32, SimId)> {
    let mut all: Vec<(u32, SimId)> = objects
        .filter(|(_, object)| {
            (object.facts.as_deref()).is_some_and(|facts| object.state.bashable(facts))
        })
        .map(|(id, object)| (object.index, id.0))
        .collect();
    all.sort_by_key(|(_, id)| *id);
    all
}

/// A door's and a placeable's half width and height (metres): what a fist
/// or a blast meets of it, `sim::target`'s pick volumes.
const DOOR_BODY: (f32, f32) = (crate::target::DOOR_RADIUS, crate::target::DOOR_HEIGHT);
const PLACEABLE_BODY: (f32, f32) = (
    crate::target::PLACEABLE_RADIUS,
    crate::target::PLACEABLE_HEIGHT,
);

/// The bodies of what may be bashed among `objects`: what a party member's
/// fist or a blast meets (by its id, as a character's), in metres, in id
/// order.
pub(super) fn bodies<'a>(objects: impl Iterator<Item = (&'a Id, &'a Object)>) -> Vec<Body> {
    bodies_where(objects, |object, facts| object.state.bashable(facts))
}

/// The bodies of every one of `objects` there is (the 107per turret's bolts
/// meet any placeable: `world::minigame`), in metres, in id order.
pub(super) fn placeable_bodies<'a>(
    objects: impl Iterator<Item = (&'a Id, &'a Object)>,
) -> Vec<Body> {
    bodies_where(objects, |_, _| true)
}

/// The bodies of those of `objects` that `keep` keeps, in id order.
fn bodies_where<'a>(
    objects: impl Iterator<Item = (&'a Id, &'a Object)>,
    keep: impl Fn(&Object, &crate::module::ObjectData) -> bool,
) -> Vec<Body> {
    let mut all: Vec<Body> = objects
        .filter_map(|(id, object)| {
            let facts = object.facts.as_deref()?;
            keep(object, facts).then_some(())?;
            let (half, height) = if object.kind == ObjectKind::Door {
                DOOR_BODY
            } else {
                PLACEABLE_BODY
            };
            let [x, y, z] = facts.position;
            Some(Body {
                entity: id.0.0 | BODY,
                min: [x - half, y - half, z],
                max: [x + half, y + half, z + height],
                contents: Contents::BODY,
            })
        })
        .collect();
    all.sort_by_key(|body| body.entity);
    all
}

/// Where a body is in the middle (units, as characters' places are).
pub(super) fn middle(body: &Body) -> [f32; 3] {
    std::array::from_fn(|i| (body.min[i] + body.max[i]) * 0.5 / collision::UNIT)
}

/// What blows did to doors and placeables this step, for the scripts: the
/// object, who struck it, the whole hit points it lost (none for a
/// weapon's blow its hardness turned), and whether it broke.
#[derive(Resource, Default)]
pub(crate) struct Battered(pub Vec<(SimId, SimId, u16, bool)>);

/// How a blow on an object is resolved: a body of defence
/// `bashing::OBJECT_DEFENCE` over the base, `hardness` resisted of every
/// damage type, `most` hit points (what a push is weighed against).
fn stand_in(hardness: u8, most: i32) -> Sheet {
    Sheet {
        defence: rules::interaction::bashing::OBJECT_DEFENCE,
        max_hp: most.clamp(1, i32::from(u16::MAX)) as u16,
        guards: Guards {
            immunity: [0; DAMAGE_TYPES],
            resist: [hardness; DAMAGE_TYPES],
        },
        ..Sheet::commoner()
    }
}

/// What `source` may do to `facts` as `state` leaves it, by a party member
/// (`party`) or not, a script's blow (`ordered`: a bash, an attack of the
/// object) or not: whether it is hurt at all.
pub(crate) fn hurts(
    source: &Source,
    facts: &ObjectData,
    state: &ObjectState,
    party: bool,
    ordered: bool,
) -> bool {
    // A door that is scenery never breaks: its collision has no way open.
    let scenery = facts.kind == ObjectKind::Door && facts.opens == crate::module::Opens::Never;
    if facts.toughness.plot || scenery || state.broken(facts) || state.hit_points(facts).is_none() {
        return false;
    }
    match source {
        Source::Script { .. } | Source::Death => true,
        // A script's blow lands locked or not: a bash is asked only of
        // what is locked, an attack of an object of anything that breaks
        // (`script::Thing::breakable`).
        Source::Weapon { .. } | Source::Blast { .. } => ordered || party && state.bashable(facts),
        Source::Power { .. } => false,
    }
}

/// Hundredths of a hit point `lost` (whole points at most `now`) as
/// `facts` lets them land: held at 1 for `Min1HP`. Returns the whole
/// points lost and whether it broke.
pub(crate) fn land(facts: &ObjectData, state: &mut ObjectState, lost: i32) -> (u16, bool) {
    let Some((now, _)) = state.hit_points(facts) else {
        return (0, false);
    };
    let floor = if facts.toughness.min_one_hp { 1 } else { 0 };
    let whole = ((lost + 50) / 100).clamp(0, (now - floor).max(0));
    let whole = u16::try_from(whole).unwrap_or(u16::MAX);
    state.damage = state.damage.saturating_add(whole);
    (whole, state.broken(facts))
}

/// Breaks `state`'s object open: unlocked, a shut door opened away from
/// `from` (metres), a container open, any other placeable dead.
fn break_open(facts: &ObjectData, state: &mut ObjectState, from: Option<[f32; 3]>) {
    state.lock = lock::UNLOCKED;
    match facts.kind {
        ObjectKind::Door if state.state == 0 => state.state = facts.opening(from),
        ObjectKind::Placeable if facts.container => state.state = look::OPEN,
        ObjectKind::Placeable => state.state = look::DEAD,
        _ => {}
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(super) fn objects(
    clock: Res<Clock>,
    level: Res<Static>,
    mut streams: ResMut<Streams>,
    mut battering: ResMut<Battering>,
    mut battered: ResMut<Battered>,
    mut scripts: ResMut<super::scripts::Scripts>,
    mut actors: Query<(
        &Id,
        Option<&Avatar>,
        Option<&Object>,
        Option<&PlayerSheet>,
        &mut Player,
        &Fighting,
    )>,
    mut objects: Query<(&Id, &mut Object), Without<Player>>,
) {
    battered.0.clear();
    let mut blows = std::mem::take(&mut battering.0);
    // Prediction shows no damage (I22 C20).
    if clock.reason == StepReason::Predict {
        return;
    }
    // The scripts' bashes of the last step (kept in their state, so a
    // snapshot between carries them).
    blows.extend(
        std::mem::take(&mut scripts.state.bashes)
            .into_iter()
            .map(|(door, by)| Bash {
                struck: Struck::Id(SimId(door)),
                attacker: SimId(by),
                source: None,
            }),
    );
    if blows.is_empty() {
        return;
    }
    // Who struck a script's bash, with what: they swing.
    let mut swings = Vec::new();
    for Bash {
        struck,
        attacker: by,
        source,
    } in blows
    {
        let found = objects.iter_mut().find(|(id, object)| match struck {
            Struck::Id(struck) => id.0 == struck,
            Struck::Entity(index) => object.index == index,
        });
        let Some((id, mut object)) = found else {
            continue;
        };
        let target = id.0;
        let Some(facts) = object.facts.clone() else {
            continue;
        };
        let attacker = actors.iter().find(|(id, ..)| id.0 == by);
        let party = attacker
            .is_some_and(|(_, avatar, _, _, _, fighting)| super::in_party(avatar, &fighting.0));
        let sheet = attacker.map(|(_, _, object, sheet, _, fighting)| {
            crate::combat::effects::sheet_now(
                super::sheet_of(sheet, object),
                &fighting.0,
                clock.time_ms,
            )
        });
        // A script's bash: one KOTOR attack of what is in hand (a round's,
        // its pace 1).
        let ordered = source.is_none();
        let weapon = (attacker.zip(sheet.as_ref()))
            .map_or(crate::combat::Weapon::UNARMED, |((.., fighting), sheet)| {
                super::held(sheet, &fighting.0)
            });
        let source = source.unwrap_or(Source::Weapon {
            weapon,
            cadence_ms: damage::WHOLE_CADENCE_MS,
            share: 1000,
            area: false,
        });
        if !hurts(&source, &facts, &object.state, party, ordered) {
            continue;
        }
        if ordered {
            swings.push((by, weapon.delivery));
        }
        let most = facts.hit_points.map_or(1, |(_, most)| most);
        let blow = Blow {
            attacker: Some(by),
            relation: Relation::Hostile,
            source,
            location: Location::None,
            direction: None,
            sneak: 0,
            dice: attacker.map_or(1, |(.., fighting)| {
                crate::combat::balance::dice(&fighting.0)
            }),
        };
        let assessed = damage::assess(
            &blow,
            sheet.as_ref(),
            &stand_in(facts.toughness.hardness, most),
            level.0.friendly_fire,
            level.0.game(),
            &mut streams.authority,
        );
        let worth = match source {
            // TSL's Door Cutting (the plasma torch): a hit takes all its
            // hit points, whatever its hardness.
            Source::Weapon { weapon, .. } if weapon.traits & weapon_traits::DOOR_CUTTING != 0 => {
                object
                    .state
                    .hit_points(&facts)
                    .map_or(0, |(now, _)| now.max(0))
                    * 100
            }
            Source::Weapon { .. } | Source::Blast { .. } => {
                assessed.damage.saturating_mul(BASH_TIMES)
            }
            _ => assessed.damage,
        };
        let (lost, broke) = land(&facts, &mut object.state, worth);
        // A weapon's blow that its hardness turned is an attack still: its
        // `OnMeleeAttacked` runs (TSL's Peragus med bay door barks for a
        // plasma torch), its `OnDamaged` not.
        if lost == 0 && !matches!(source, Source::Weapon { .. }) {
            continue;
        }
        if broke {
            let from =
                attacker.map(|(.., player, _)| math::scale(player.0.origin, collision::UNIT));
            break_open(&facts, &mut object.state, from);
        }
        battered.0.push((target, by, lost, broke));
    }
    for (by, delivery) in swings {
        if let Some((.., mut player, _)) = actors.iter_mut().find(|(id, ..)| id.0 == by) {
            crate::motion::react(&mut player.0, &level.0.anims, swing(delivery), true);
        }
    }
}

/// The move a script's bash plays with `delivery` in hand: a punch, a
/// blade's stroke from above, a gun's fire.
fn swing(delivery: Delivery) -> Anim {
    match delivery {
        Delivery::Unarmed => Anim::BOTH_MELEE1,
        Delivery::Blaster(_) => Anim::BOTH_ATTACK3,
        Delivery::Baton | Delivery::Melee | Delivery::Saber(_) => Anim::BOTH_A1_T__B_,
    }
}

#[cfg(feature = "probes")]
mod probes;
#[cfg(feature = "probes")]
pub use probes::probes;
