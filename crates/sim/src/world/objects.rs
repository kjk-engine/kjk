//! Doors and placeables hurt (KOTOR's Bash, `docs/COMBAT.md`): on the
//! authority, after `hurt`, each hit that found no character lands here if
//! it names a door or placeable with hit points
//! (`ObjectData::hit_points`, less `ObjectState::damage`).
//!
//! - Nothing hurts a `Plot` object, nor one already broken.
//! - A script's damage (`EffectDamage`, `EffectDeath`) hurts any other.
//! - A party member's weapon or grenade hurts only what may be bashed
//!   (`ObjectState::bashable`: locked); others' blows do not, so a fight in
//!   a corridor breaks none of its doors.
//! - A party member's weapon on a placeable the party attacks
//!   (`attackable`: its `placeables.2da` row is `hostile`) hurts it locked
//!   or not, unless it is `Plot`; either way it is an attack, and its
//!   `OnMeleeAttacked` runs (the Star Forge's stasis tanks, `Plot`).
//! - A creature bashes only when a
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

/// Whether `facts` is a placeable of a `hostile` appearance
/// (`placeables.2da`), not broken: one the party attacks while its faction
/// is hostile to the player (`attackable`). KOTOR's own rows mark one each:
/// K1's Star Forge stasis tanks (`Dead_Malak`) and TSL's power conductors.
fn hostile_placeable(facts: &ObjectData, state: &ObjectState) -> bool {
    facts.kind == ObjectKind::Placeable && facts.toughness.hostile && !state.broken(facts)
}

/// Whether the party attacks `facts`, locked or not: a placeable of a
/// `hostile` appearance whose faction now (`faction`, as the scripts leave
/// it) the player regards as hostile in `standing`, as the executable asks
/// before it offers an attack on one. A party member's weapon on one is an
/// attack: its `OnMeleeAttacked` runs, and it is hurt unless it is `Plot`.
/// The stasis tanks are `Plot`, and their attacked script kills the captive
/// Jedi inside, so that Malak cannot drain him; it turns the tank neutral
/// then, and the party attacks it no more.
pub(crate) fn attackable(
    facts: &ObjectData,
    state: &ObjectState,
    faction: u8,
    standing: Option<crate::module::Standing>,
) -> bool {
    hostile_placeable(facts, state)
        && standing.is_some_and(|standing| {
            standing.hostile(crate::combat::sheet::factions::PLAYER, faction)
        })
}

/// How the party regards the area's objects now: the standings as the
/// scripts left them, and the factions scripts gave doors and placeables
/// (`script::State::factions`).
#[derive(Clone, Copy)]
pub(super) struct Regard<'a> {
    standing: Option<crate::module::Standing<'a>>,
    factions: &'a std::collections::BTreeMap<u32, u8>,
}

impl<'a> Regard<'a> {
    pub(super) fn of(level: &'a super::Level, state: &'a crate::script::State) -> Regard<'a> {
        Regard {
            standing: (level.module.as_ref()).map(|module| module.repute.with(&state.reputation)),
            factions: &state.factions,
        }
    }

    /// Whether the party attacks `id` (`attackable`), its faction as the
    /// scripts leave it.
    fn attacks(&self, id: SimId, facts: &ObjectData, state: &ObjectState) -> bool {
        let faction = self.factions.get(&id.0).copied().unwrap_or(facts.faction);
        attackable(facts, state, faction, self.standing)
    }

    /// Whether a party member's blow meets `object`: it may be bashed, or
    /// the party attacks it now (a stasis tank turned neutral stops no
    /// more of the party's bolts and blades).
    fn meets(&self, id: SimId, object: &Object) -> bool {
        (object.facts.as_deref()).is_some_and(|facts| {
            object.state.bashable(facts) || self.attacks(id, facts, &object.state)
        })
    }
}

/// Whether `ps` aims at `facts`: the line from its eyes along its view, as
/// far as a target is picked (`target::RANGE`), passes through its pick
/// volume. A blow is an attack on a placeable the party attacks only when
/// its maker aims at it, not when a sweep or a shot at another crosses it.
fn aims_at(ps: &crate::motion::MotionState, facts: &ObjectData) -> bool {
    let eyes = math::scale(
        math::add(ps.origin, [0.0, 0.0, ps.eye_height]),
        collision::UNIT,
    );
    let forward = math::angle_vectors(ps.view).0;
    let (half, height) = PLACEABLE_BODY;
    let [x, y, z] = facts.position;
    let (min, max) = ([x - half, y - half, z], [x + half, y + half, z + height]);
    // The line's entry into and exit from the box, axis by axis.
    let (mut enter, mut leave) = (0.0_f32, crate::target::RANGE);
    for axis in 0..3 {
        let (from, way) = (eyes[axis], forward[axis]);
        if way.abs() < f32::EPSILON {
            if from < min[axis] || from > max[axis] {
                return false;
            }
            continue;
        }
        let (a, b) = ((min[axis] - from) / way, (max[axis] - from) / way);
        enter = enter.max(a.min(b));
        leave = leave.min(a.max(b));
    }
    enter <= leave
}

/// Each object a party member's blow meets among `objects` (`Regard::meets`):
/// its number among the level's entities (what a blade's trace meets of
/// it) and its id. In id order, as every list the sim's dice follow.
pub(super) fn bashable<'a>(
    objects: impl Iterator<Item = (&'a Id, &'a Object)>,
    regard: Regard,
) -> Vec<(u32, SimId)> {
    let mut all: Vec<(u32, SimId)> = objects
        .filter(|(id, object)| regard.meets(id.0, object))
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

/// The bodies of what a party member's blow meets among `objects`
/// (`Regard::meets`): what its fist, its bolt or a blast meets (by its id,
/// as a character's), in metres, in id order.
pub(super) fn bodies<'a>(
    objects: impl Iterator<Item = (&'a Id, &'a Object)>,
    regard: Regard,
) -> Vec<Body> {
    bodies_where(objects, |id, object, _| regard.meets(id, object))
}

/// The bodies of every one of `objects` there is (the 107per turret's bolts
/// meet any placeable: `world::minigame`), in metres, in id order.
pub(super) fn placeable_bodies<'a>(
    objects: impl Iterator<Item = (&'a Id, &'a Object)>,
) -> Vec<Body> {
    bodies_where(objects, |_, _, _| true)
}

/// The bodies of those of `objects` that `keep` keeps, in id order.
fn bodies_where<'a>(
    objects: impl Iterator<Item = (&'a Id, &'a Object)>,
    keep: impl Fn(SimId, &Object, &crate::module::ObjectData) -> bool,
) -> Vec<Body> {
    let mut all: Vec<Body> = objects
        .filter_map(|(id, object)| {
            let facts = object.facts.as_deref()?;
            keep(id.0, object, facts).then_some(())?;
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

/// What blows did to doors and placeables this step, for the scripts.
#[derive(Resource, Default)]
pub(crate) struct Battered(pub Vec<BlowTaken>);

/// A blow a door or placeable took.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BlowTaken {
    pub object: SimId,
    pub by: SimId,
    /// A weapon's attack (`OnMeleeAttacked`): a script's or a blast's
    /// damage only damages, as the executables raise the attacked event
    /// from an attack's resolution and never from a damage effect (TSL's
    /// 103per airlock blows only when attacked, not when a floating mine
    /// goes off beside it).
    pub attacked: bool,
    /// Whole hit points lost (none for a weapon's blow its hardness turned).
    pub lost: u16,
    pub broke: bool,
}

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
/// object) or not, on what the party attacks (`attacked`, `attackable`) or
/// not: whether it is hurt at all.
pub(crate) fn hurts(
    source: &Source,
    facts: &ObjectData,
    state: &ObjectState,
    party: bool,
    (ordered, attacked): (bool, bool),
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
        Source::Weapon { .. } => ordered || party && (state.bashable(facts) || attacked),
        // A blast attacks nothing: it breaks only what may be bashed.
        Source::Blast { .. } => ordered || party && state.bashable(facts),
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
            second: None,
        });
        // An attack on a placeable the party attacks is a blow of a body a
        // seat drives that aims at it: a companion's sweep or a stray shot
        // at Malak that crosses a stasis tank does not kill its Jedi.
        let aimed = attacker.is_some_and(|(_, avatar, _, _, player, fighting)| {
            (avatar.is_some() || fighting.0.mind.driver.is_some()) && aims_at(&player.0, &facts)
        });
        let attacked =
            aimed && Regard::of(&level.0, &scripts.state).attacks(target, &facts, &object.state);
        if !hurts(&source, &facts, &object.state, party, (ordered, attacked)) {
            // A party member's weapon on a `Plot` placeable it attacks is an
            // attack still: its script runs though it is not hurt.
            if party && attacked && matches!(source, Source::Weapon { .. }) {
                battered.0.push(BlowTaken {
                    object: target,
                    by,
                    attacked: true,
                    lost: 0,
                    broke: false,
                });
            }
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
        battered.0.push(BlowTaken {
            object: target,
            by,
            attacked: matches!(source, Source::Weapon { .. }),
            lost,
            broke,
        });
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
