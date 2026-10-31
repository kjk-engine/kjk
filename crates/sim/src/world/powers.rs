//! The step's Force (I26), between the minds and the moves: each
//! character in the move order casts what its command asks (`combat::force`
//! has the rules, the state and JA's numbers), then every character's
//! upkeep: regeneration, its speed and gravity under speed and grip, and
//! the hand it holds out.
//!
//! Who is caught is decided on everyone as they stood before the powers,
//! so the order of casting changes only who acts on what was already
//! changed (a stun lands before a later caster's turn). What the powers do
//! to bodies (shoves, the grip's lift and carry, speed) and what they cost
//! the caster run everywhere, prediction included; what they do to hit
//! points, to another's pool, and what needs a roll (saving throws, a
//! creature's confusion) run on the authority and in replays only, as
//! `hurt` does (I22 C20). Damage goes through `hurt` as hits whose source
//! is a power.
//!
//! Not yet: pushing bolts back and pull's disarm (with I24's bolt
//! deflection), rage and sight (KOTOR has no rows for them).

use collision::{Bodies, CollisionWorld, Extents, UNIT};

use super::{
    Avatar, BODY, Clock, Fighting, Hit, Hits, Id, Inputs, Object, Player, PlayerSheet, SHOT,
    Static, Streams, Targets, command_for, door_states, faction_of, sheet_of,
};
use crate::combat::damage::{Relation, Source};
use crate::combat::effects::Modifies;
use crate::combat::force::{self, Channel, Gesture, Power, crowd, ja};
use crate::combat::saber::Getup;
use crate::combat::sheet::{flags, type_rows};
use crate::combat::{Fighter, Sheet, effects, items, mind, specials};
use crate::input::{Seat, buttons};
use crate::math::{self, Vec3, scale};
use crate::motion::{self, Anim, MotionState};
use crate::script::effects::Hold;
use crate::snapshot::SimId;
use crate::world::StepReason;
use bevy_ecs::prelude::*;

/// The buttons the Force reads.
const FORCE_BUTTONS: u16 =
    buttons::FORCE_POWER | buttons::FORCE_GRIP | buttons::FORCE_LIGHTNING | buttons::FORCE_DRAIN;

/// The powers that run while held, each with a button of its own.
const HELD: [Power; 3] = [Power::Grip, Power::Lightning, Power::Drain];

/// A held power's own button (none for the rest).
fn held_button(power: Power) -> u16 {
    match power {
        Power::Grip => buttons::FORCE_GRIP,
        Power::Lightning => buttons::FORCE_LIGHTNING,
        Power::Drain => buttons::FORCE_DRAIN,
        _ => 0,
    }
}

/// A character as the powers see it, unchanging through the step.
struct Person {
    id: SimId,
    seat: Option<Seat>,
    /// A player or a companion of one.
    party: bool,
    /// Its commands its player's own (a seat's, not steered by a script),
    /// so the powers it uses are its player's orders.
    ordered: bool,
    /// Its sheet as what runs on it changes it (`effects::sheet_now`).
    sheet: Sheet,
    /// Its bar's size: its own sheet's (`Sheet::pool`), whatever runs on it.
    pool: u16,
    faction: u8,
    /// Where it stood before the powers.
    ps: MotionState,
    /// Alive before the powers: an area power takes no corpse.
    alive: bool,
    /// Where its model is drawn, if JA does not pose it (`world::body`).
    bulk: Option<crate::module::Bulk>,
}

impl Person {
    fn eye(&self) -> Vec3 {
        mind::eye(&self.ps)
    }

    /// Its box (units).
    fn bounds(&self) -> (Vec3, Vec3) {
        let [x, y, z] = self.ps.origin;
        let half = motion::HALF_WIDTH;
        (
            [x - half, y - half, z + motion::SOLE_Z],
            [x + half, y + half, z + self.ps.maxs_z],
        )
    }

    fn centre(&self) -> Vec3 {
        let (min, max) = self.bounds();
        std::array::from_fn(|axis| (min[axis] + max[axis]) * 0.5)
    }

    /// The view's full direction.
    fn forward(&self) -> Vec3 {
        math::angle_vectors(self.ps.view).0
    }
}

/// What the casters look through: the level, the doors, everyone.
struct Scene<'a> {
    level: &'a crate::Level,
    /// How the factions stand (none without a module).
    standing: Option<crate::module::Standing<'a>>,
    states: Option<&'a [u8]>,
    people: &'a [Person],
    targets: Targets,
    /// The bodies each player saw, when it saw the world behind the
    /// authority (I25, C18): its aimed powers find their target there.
    views: Vec<Option<Targets>>,
}

impl Scene<'_> {
    fn find(&self, id: SimId) -> Option<usize> {
        self.people.iter().position(|person| person.id == id)
    }

    /// How `a` stands to `b` (`module::relation`).
    fn relation(&self, a: usize, b: usize) -> Relation {
        let side = |person: &Person| crate::module::Side {
            id: person.id.0,
            faction: person.faction,
            party: person.party,
        };
        crate::module::relation(self.standing, side(&self.people[a]), side(&self.people[b]))
    }

    /// One side: the same character, the party, or friends by faction.
    fn side(&self, a: usize, b: usize) -> bool {
        matches!(self.relation(a, b), Relation::Itself | Relation::Party)
    }

    /// Whether `a` and `b` are hostile by faction.
    fn hostile(&self, a: usize, b: usize) -> bool {
        self.relation(a, b) == Relation::Hostile
    }

    /// Whether a power that friendly fire governs may take `b` from `a`.
    fn harms(&self, a: usize, b: usize) -> bool {
        !self.level.friendly_fire().spares(self.relation(a, b))
    }

    /// Nothing of the level between two points (units).
    fn clear(&self, from: Vec3, to: Vec3) -> bool {
        self.level.sees(self.states, from, to)
    }

    /// The first body a line from `from` along `caster`'s view meets
    /// within `range`, with where (units): among the bodies as the caster
    /// saw them, if it is a player behind the authority.
    fn aimed(&self, caster: usize, from: Vec3, range: f32) -> Option<(usize, Vec3)> {
        let me = &self.people[caster];
        let end = math::mad(from, range, me.forward());
        let targets = self.views[caster].as_ref().unwrap_or(&self.targets);
        let trace = self.level.with(self.states, |world| {
            Bodies::new(world, targets.met_by(me.id))
                .skipping(me.id.0 | BODY)
                .trace(scale(from, UNIT), scale(end, UNIT), &Extents::POINT, SHOT)
        });
        let id = trace.entity.filter(|entity| entity & BODY != 0)?;
        let at = self.find(SimId(id & !BODY))?;
        Some((at, scale(trace.end, 1.0 / UNIT)))
    }

    /// Everyone alive around `caster` that an area power reaches: nearer
    /// than `range` (to the nearest point of its box), within `field`
    /// degrees of the view in yaw and pitch from the eye (if given), its
    /// box's centre in front (a dot of at least `front` with the view,
    /// from the origin) and in clear sight from the origin or the eye.
    fn around(&self, caster: usize, range: f32, field: Option<f32>, front: f32) -> Vec<usize> {
        let origin = self.people[caster].ps.origin;
        self.around_from(caster, origin, range, field, front)
    }

    /// `around`, its reach and its front measured from `from`.
    fn around_from(
        &self,
        caster: usize,
        from: Vec3,
        range: f32,
        field: Option<f32>,
        front: f32,
    ) -> Vec<usize> {
        let me = &self.people[caster];
        let forward = me.forward();
        (0..self.people.len())
            .filter(|&at| at != caster && self.people[at].alive)
            .filter(|&at| {
                let them = &self.people[at];
                if let Some(field) = field {
                    let (pitch, yaw) = mind::aim(me.eye(), them.ps.origin);
                    if mind::turn(me.ps.view[1], yaw).abs() > field
                        || mind::turn(me.ps.view[0], pitch).abs() > field
                    {
                        return false;
                    }
                }
                let centre = them.centre();
                let mut toward = math::sub(centre, from);
                math::normalize(&mut toward);
                if math::dot(toward, forward) < front {
                    return false;
                }
                let (min, max) = them.bounds();
                let nearest: Vec3 =
                    std::array::from_fn(|axis| from[axis].clamp(min[axis], max[axis]));
                math::length(math::sub(nearest, from)) < range
                    && (self.clear(me.ps.origin, centre) || self.clear(me.eye(), centre))
            })
            .collect()
    }
}

/// A JA point of `max` (whole points) in hundredths: 1% of it.
fn points(count: i32, max: u16) -> i32 {
    count * i32::from(max)
}

/// How long a toggled power runs at `level`.
fn toggle_length(power: Power, level: u8) -> u64 {
    let index = usize::from(level.clamp(1, 3) - 1);
    match power {
        Power::Speed => ja::SPEED_MS[index],
        Power::MindTrick => ja::TRICK_MS[index],
        Power::Rage => ja::RAGE_MS[index],
        Power::Sight => ja::SIGHT_MS[index],
        _ => ja::GUARD_MS,
    }
}

/// The Force system (see the module documentation).
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(super) fn force(
    clock: Res<Clock>,
    inputs: Res<Inputs>,
    level: Res<Static>,
    mut streams: ResMut<Streams>,
    mut hits: ResMut<Hits>,
    mut spells: ResMut<super::scripts::SpellsAt>,
    mut casts: ResMut<super::scripts::Casts>,
    mut ordered: ResMut<super::scripts::Ordered>,
    mut let_go: ResMut<super::scripts::LetGo>,
    scripts: Res<super::scripts::Scripts>,
    history: Res<super::history::History>,
    objects: Query<&Object>,
    mut actors: Query<(
        &Id,
        Option<&Avatar>,
        Option<&Object>,
        Option<&PlayerSheet>,
        &mut Player,
        &mut Fighting,
    )>,
) {
    spells.0.clear();
    casts.0.clear();
    ordered.0.clear();
    let_go.0.clear();
    let mut actors: Vec<_> = actors.iter_mut().collect();
    actors.sort_by_key(|(id, avatar, ..)| (avatar.is_none(), avatar.map(|a| a.0), id.0));
    let people: Vec<Person> = actors
        .iter()
        .map(|(id, avatar, object, sheet, player, fighting)| {
            // What runs on it changes its sheet (saves, defence, abilities).
            let base = sheet_of(*sheet, *object);
            let sheet = effects::sheet_now(base, &fighting.0, clock.time_ms);
            Person {
                pool: base.pool(),
                id: id.0,
                seat: avatar.map(|a| a.0),
                party: super::in_party(*avatar, &fighting.0),
                ordered: avatar.is_some() && !fighting.0.mind.steered,
                faction: faction_of(&fighting.0, &sheet),
                sheet,
                ps: player.0,
                alive: fighting.0.alive(),
                bulk: super::object_bulk(*object).copied(),
            }
        })
        .collect();
    let mut bodies: Vec<MotionState> = people.iter().map(|person| person.ps).collect();
    let mut fighters: Vec<Fighter> = actors.iter().map(|row| row.5.0).collect();
    let states = door_states(&level.0, &objects);
    let scene = Scene {
        level: &level.0,
        standing: (level.0.module.as_ref())
            .map(|module| module.repute.with(&scripts.state.reputation)),
        states: states.as_deref(),
        people: &people,
        targets: Targets::of(
            level.0.friendly_fire(),
            people.iter().zip(&fighters).map(|(person, fighter)| {
                let bulk = person.bulk.as_ref();
                (person.id, &person.ps, bulk, fighter.alive(), person.party)
            }),
        ),
        views: people
            .iter()
            .map(|person| {
                let seat = person
                    .seat
                    .filter(|_| clock.reason != StepReason::Predict)?;
                let view = history.view_tick(clock.tick, inputs.0.view_lag(seat), clock.msec)?;
                Some(history.targets_at(view, level.0.friendly_fire()))
            })
            .collect(),
    };
    let mut cast = Cast {
        scene: &scene,
        bodies: &mut bodies,
        fighters: &mut fighters,
        hits: &mut hits.0,
        spells: &mut spells.0,
        casts: &mut casts.0,
        ordered: &mut ordered.0,
        let_go: &mut let_go.0,
        deferred: &scripts.state.delayed,
        library: level.0.scripts().map(|library| &**library),
        regeneration: level
            .0
            .scripts()
            .and_then(|library| library.regeneration())
            .filter(|_| !scripts.state.party.no_regen),
        fp_regeneration: level
            .0
            .scripts()
            .and_then(|library| library.regeneration())
            .map(|regeneration| regeneration.fp)
            .filter(|fp| fp.time > 0.0),
        health_regeneration: level
            .0
            .scripts()
            .and_then(|library| library.health_regeneration()),
        row: 0,
        rng: &mut streams.authority,
        now: clock.time_ms,
        tick: clock.tick,
        msec: clock.msec,
        authority: clock.reason != StepReason::Predict,
    };
    // The dead let go first, so no victim of theirs stays held a step.
    for at in 0..people.len() {
        if !cast.fighters[at].alive()
            && let Some(channel) = cast.fighters[at].force.channel
        {
            cast.end(at, channel);
        }
    }
    for at in 0..people.len() {
        let command = command_for(
            &inputs.0,
            actors[at].1,
            &people[at].ps,
            &cast.fighters[at],
            clock.time_ms,
        );
        cast.turn(at, command.buttons, command.force);
    }
    for at in 0..people.len() {
        cast.upkeep(at);
    }
    for (at, row) in actors.iter_mut().enumerate() {
        row.4.0 = bodies[at];
        row.5.0 = fighters[at];
    }
}

/// TSL's feat that adds to its healing (`feat.2da` 196,
/// `REGENERATE_VITALITY_POINTS`), and how much.
const REGENERATE_FEAT: u16 = 196;
const REGENERATE_FEAT_RATE: f32 = rules::force::tsl_regeneration::HEALING_FEAT_RATE;

/// TSL's natural healing of a living party member short of its maximum
/// (`CSWSCreature`'s regeneration in its executable): a rate of the
/// table's base out of combat or in it, plus its Constitution modifier and
/// its rank in Treat Injury (Repair for a droid) times their bonuses, plus
/// `REGENERATE_FEAT_RATE` with `REGENERATE_FEAT`, doubled for each
/// vitality regeneration modifier running on it whatever its percentage
/// (QUIRKS.md Q-0242: Battle Meditation's 50 % doubles); that many hit
/// points every `time_per_hp` seconds. The original
/// adds `rate / time_per_hp` a second through an accumulator every frame;
/// given at each `time_per_hp` here, so no fraction needs keeping. [verify] that a stealthed body counts as out of
/// combat as the executable's second test suggests.
fn regenerate(
    fighter: &mut Fighter,
    sheet: &Sheet,
    table: crate::script::Regeneration,
    (now, msec): (u64, u32),
) {
    use crate::combat::sheet::{Ability, races, skills};
    let max = i32::from(sheet.max_hp) * 100;
    if !fighter.alive() || fighter.hp <= 0 || fighter.hp >= max {
        return;
    }
    let every = (table.time_per_hp.max(0.0001) * 1000.0) as u64;
    if every == 0 || now / every == now.saturating_sub(u64::from(msec)) / every {
        return;
    }
    let base = if fighter.combat_until > now {
        table.in_combat
    } else {
        table.out_of_combat
    };
    let skill = if sheet.race == races::DROID {
        skills::REPAIR
    } else {
        skills::TREAT_INJURY
    };
    let mut rate = base
        + sheet.modifier(Ability::Constitution) as f32 * table.con_bonus
        + f32::from(sheet.skills[skill]) * table.skill_bonus;
    if sheet.feats.has(REGENERATE_FEAT) {
        rate += REGENERATE_FEAT_RATE;
    }
    for _ in 0..fighter.effects.healing_doubled(now) {
        rate += rate;
    }
    if rate <= 0.0 {
        return;
    }
    fighter.hp = (fighter.hp + (rate * 100.0).round() as i32).min(max);
}

/// K1's natural healing of a living party member short of its maximum
/// (`HealthRegeneration`: the creature's update in its executable, which
/// adds the rate a frame into a carry and gives its whole points): a whole
/// hit point each `every_ms`, fighting or not as `regenerate` tells them
/// apart, so no fraction needs keeping.
fn regenerate_k1(
    fighter: &mut Fighter,
    sheet: &Sheet,
    rates: crate::script::HealthRegeneration,
    (now, msec): (u64, u32),
) {
    let max = i32::from(sheet.max_hp) * 100;
    if !fighter.alive() || fighter.hp <= 0 || fighter.hp >= max {
        return;
    }
    let every = rates.every_ms(sheet.max_hp, fighter.combat_until > now);
    let points = now / every - now.saturating_sub(u64::from(msec)) / every;
    let gained = i32::try_from(points)
        .unwrap_or(i32::MAX)
        .saturating_mul(100);
    fighter.hp = fighter.hp.saturating_add(gained).min(max);
}

/// One step's casting over copies of everyone's movement and fight.
struct Cast<'a, 'b> {
    scene: &'a Scene<'b>,
    bodies: &'a mut [MotionState],
    fighters: &'a mut [Fighter],
    hits: &'a mut Vec<Hit>,
    /// Whom powers reached (`scripts::SpellsAt`), and the `spells.2da` row
    /// the one acting now uses.
    spells: &'a mut Vec<super::scripts::SpellAt>,
    /// The casts whose impact scripts decide them (C10), and the library
    /// that has those scripts.
    casts: &'a mut Vec<crate::script::Cast>,
    /// The party members whose players ordered a power
    /// (`scripts::Ordered`).
    ordered: &'a mut Vec<SimId>,
    /// The scripted grips let go (`scripts::LetGo`), and the scripts'
    /// deferred code, whose ticks a scripted grip holds for.
    let_go: &'a mut Vec<(script_ncs::vm::ObjectId, u16, script_ncs::vm::ObjectId)>,
    deferred: &'a [crate::script::Delayed],
    library: Option<&'a crate::script::Library>,
    /// TSL's natural healing, unless a script turned it off
    /// (`DisableHealthRegen`), and its Force point regeneration
    /// (`force::regenerate_tsl`; none in K1, which keeps C13).
    regeneration: Option<crate::script::Regeneration>,
    fp_regeneration: Option<crate::script::FpRegeneration>,
    /// K1's natural healing (`regenerate_k1`; no script turns it off).
    health_regeneration: Option<crate::script::HealthRegeneration>,
    row: u16,
    rng: &'a mut crate::rng::SimRng,
    now: u64,
    tick: u64,
    msec: u32,
    authority: bool,
}

impl Cast<'_, '_> {
    fn person(&self, at: usize) -> &Person {
        &self.scene.people[at]
    }

    /// What `row` costs `at` (hundredths; `ModuleData::power_cost`, TSL's
    /// adjustments); `None` where the module lists none.
    fn cost(&self, at: usize, row: u16) -> Option<i32> {
        let module = self.scene.level.module.as_ref()?;
        let person = self.person(at);
        let place = scale(person.ps.origin, collision::UNIT);
        module
            .power_cost(row, &person.sheet, person.party, place)
            .map(|cost| cost * 100)
    }

    /// Pays `cost` from `at`'s pool if it holds what it takes from it (TSL's
    /// Force Body: a share, and as much from its health; `effects::pay`).
    fn pay(&mut self, at: usize, cost: i32) -> bool {
        effects::pay(&mut self.fighters[at], cost, self.now)
    }

    /// Whether the power `at` uses now (`row`) has an impact script that
    /// decides what it does (C10: "JA delivers, KOTOR decides"): its
    /// delivery here only finds whom it catches (and shoves, lifts or
    /// carries them); the script, run in the scripts' step, does the rest.
    fn scripted(&self) -> bool {
        self.library
            .is_some_and(|library| library.impact(self.row).is_some())
    }

    /// `at`'s cast of the power it uses now, at `target` (the one aimed
    /// at, if any) catching `caught`, for its impact script (authority
    /// only).
    fn cast_at(&mut self, at: usize, target: Option<usize>, caught: &[usize]) {
        if !self.authority {
            return;
        }
        let id = |them: usize| script_ncs::vm::ObjectId(self.person(them).id.0);
        let where_at = target.map_or(self.person(at).ps.origin, |them| {
            self.person(them).ps.origin
        });
        self.casts.push(crate::script::Cast {
            caster: id(at),
            row: self.row,
            target: target.map_or(script_ncs::vm::ObjectId::INVALID, id),
            at: scale(where_at, collision::UNIT),
            caught: caught.iter().map(|&them| id(them)).collect(),
        });
    }

    /// That `at`'s power (`row`) reached `them` (authority only); a
    /// scripted power's impact script signals this itself.
    fn reached(&mut self, at: usize, them: usize, harmful: bool) {
        if self.authority && at != them && !self.scripted() {
            self.spells.push(super::scripts::SpellAt {
                caster: self.person(at).id,
                target: self.person(them).id,
                row: self.row,
                harmful,
            });
        }
    }

    /// A power's hit from `from` on `to` of `points` JA points (and up to
    /// `extra` more by the roll); `aimed` is where it struck and the way it
    /// pushes (a grip's squeeze has neither).
    fn hit(
        &mut self,
        from: usize,
        to: usize,
        (points, extra, damage_type): (i32, u8, Option<u8>),
        aimed: Option<(Vec3, Vec3, bool)>,
    ) {
        self.reached(from, to, true);
        self.hits.push(Hit {
            attacker: self.person(from).id,
            target: self.person(to).id,
            source: Source::Power {
                points,
                extra,
                area: aimed.is_some_and(|(_, _, area)| area),
                pushes: aimed.is_some(),
                damage_type,
            },
            point: aimed.map(|(point, ..)| point),
            direction: aimed.map(|(_, push, _)| push),
        });
    }

    /// `at`'s turn: its channel runs or ends, a held channel power starts,
    /// the power button's press casts.
    fn turn(&mut self, at: usize, held: u16, selected: u8) {
        if !self.fighters[at].alive() {
            return;
        }
        let force = self.fighters[at].force;
        let pressed = held & !force.held;
        self.fighters[at].force.held = held & FORCE_BUTTONS;
        let person = self.person(at);
        let chosen = force::selected(&person.sheet.powers, selected, person.ordered);
        let holding = |power: Power| {
            held & held_button(power) != 0
                || (held & buttons::FORCE_POWER != 0 && chosen.is_some_and(|(p, ..)| p == power))
        };
        let powers = self.person(at).sheet.powers;
        // The powers pressed now: the chosen one, and each held power whose
        // own button went down.
        let orders = chosen
            .filter(|_| pressed & buttons::FORCE_POWER != 0)
            .map(|(.., row)| row)
            .into_iter()
            .chain(
                HELD.into_iter()
                    .filter(|power| pressed & held_button(*power) != 0)
                    .filter_map(|power| force::best(&powers, power).map(|(row, _)| row)),
            );
        self.order(at, orders);
        if let Some(channel) = force.channel {
            self.row = force::row_at(&powers, channel.power, channel.level).unwrap_or(0);
            self.run(at, channel, holding(channel.power));
        } else {
            for power in HELD {
                // The power button holds the row chosen; the power's own
                // button its best.
                let (row, level) = match chosen {
                    Some((p, level, row)) if p == power && held & held_button(power) == 0 => {
                        (row, level)
                    }
                    _ => force::best(&powers, power).unwrap_or((0, 1)),
                };
                self.row = row;
                if holding(power) && self.start(at, power, row, level) {
                    break;
                }
            }
        }
        if pressed & buttons::FORCE_POWER != 0
            && let Some((power, level, row)) = chosen
        {
            self.row = row;
            self.press(at, power, level, row);
        }
    }

    /// TSL's forfeit rule on `at`'s player's order of a power, of `rows`
    /// pressed now (authority only): a party member whose player orders one
    /// it knows and can pay for now (the executable's `GetSpellUsesLeft`,
    /// its order's test), once a step, whether or not the power then goes
    /// out (`scripts::Ordered`).
    fn order(&mut self, at: usize, mut rows: impl Iterator<Item = u16>) {
        let &Person {
            id, ordered, party, ..
        } = self.person(at);
        if !self.authority || !ordered || !party {
            return;
        }
        let fighter = &self.fighters[at];
        let payable = |cost: i32| {
            effects::can_pay(
                &fighter.effects.lasting,
                (fighter.fp, fighter.hp),
                cost,
                self.now,
            )
        };
        if rows.any(|row| self.cost(at, row).is_some_and(payable)) {
            self.ordered.push(id);
        }
    }

    /// Whether `at` may put a hand out now: no hand out, no weapon timer,
    /// not knocked down (JA's knockdown takes the hand; a push or pull may
    /// still come while getting up, `throwing`).
    fn free_hand(&self, at: usize, throwing: bool) -> bool {
        let fighter = &self.fighters[at];
        let saber = &fighter.saber;
        let down = saber.floored() && !(throwing && saber.getup != Getup::Down);
        !down && !fighter.force.hand_out(self.now) && fighter.weapon_wait_ms <= 0
    }

    /// Starts a held power if it can: known, its wait over, a free hand,
    /// the cost in the pool (C12: paid now for its first `ja::CHANNEL_MS`); a
    /// grip also needs someone to latch onto.
    fn start(&mut self, at: usize, power: Power, row: u16, level: u8) -> bool {
        if !self.person(at).sheet.powers.has(row) {
            return false;
        }
        let ready = match power {
            Power::Grip => self.fighters[at].force.grip_ready,
            Power::Lightning => self.fighters[at].force.lightning_ready,
            _ => self.fighters[at].force.drain_ready,
        };
        let Some(cost) = self.cost(at, row) else {
            return false;
        };
        let fighter = &self.fighters[at];
        let payable = effects::can_pay(
            &fighter.effects.lasting,
            (fighter.fp, fighter.hp),
            cost,
            self.now,
        );
        if ready > self.now || !self.free_hand(at, false) || !payable {
            return false;
        }
        let mut target = None;
        let mut until = self.now + ja::CHANNEL_MS;
        if power == Power::Grip {
            let eye = self.person(at).eye();
            let Some((victim, _)) = self.scene.aimed(at, eye, ja::GRIP_RANGE) else {
                return false;
            };
            let them = &self.fighters[victim];
            if !them.alive()
                || them.force.gripped_by.is_some()
                || them.force.absorbing(self.now) > 0
                || !self.scene.harms(at, victim)
            {
                return false;
            }
            self.fighters[victim].force.gripped_by = Some(self.person(at).id);
            target = Some(self.person(victim).id);
            // JA's grip lasts its level's time; a scripted one while its
            // impact's ticks still wait (`squeeze`).
            until = if self.scripted() {
                self.cast_at(at, Some(victim), &[victim]);
                u64::MAX
            } else {
                self.now + ja::GRIP_HOLD_MS[usize::from(level - 1)]
            };
        }
        self.pay(at, cost);
        let force = &mut self.fighters[at].force;
        force.tricked = 0;
        force.channel = Some(Channel {
            power,
            level,
            until,
            next_tick: self.now,
            from: self.now,
            target,
        });
        true
    }

    /// A channel's step: held on (every channel ends on release, JA's; a
    /// level-1 lightning or drain runs at most `ja::CHANNEL_MS`, above that
    /// on while held), paid for (C12), its ticks, or its end. A caster
    /// knocked down lets go.
    fn run(&mut self, at: usize, mut channel: Channel, held: bool) {
        let row = force::row_at(&self.person(at).sheet.powers, channel.power, channel.level);
        let cost = row.and_then(|row| self.cost(at, row)).unwrap_or(0);
        if channel.power != Power::Grip && channel.level > 1 && held {
            channel.until = self.now + ja::CHANNEL_MS;
        }
        let owed = force::channel_cost(cost, channel.from, self.now, self.msec);
        let mut going = self.now < channel.until
            && held
            && !self.fighters[at].saber.floored()
            && self.pay(at, owed);
        if going && channel.power == Power::Grip {
            going = self.squeeze(at, &mut channel);
        } else if going {
            while going && channel.next_tick <= self.now {
                // A scripted impact every `ja::IMPACT_EVERY_MS` held (C12's
                // cost's pace), the first at the start.
                let impact = channel
                    .next_tick
                    .saturating_sub(channel.from)
                    .is_multiple_of(ja::IMPACT_EVERY_MS);
                match channel.power {
                    Power::Lightning => self.lightning(at, channel.level, impact),
                    _ => self.drain(at, channel.level, impact),
                }
                channel.next_tick += ja::TICK_MS;
            }
        }
        if going {
            self.fighters[at].force.channel = Some(channel);
        } else {
            self.end(at, channel);
        }
    }

    /// Whether code the impact of `at`'s power now (`row`) on `victim`
    /// deferred still waits to run.
    fn ticking(&self, at: usize, victim: usize) -> bool {
        let id = |them: usize| script_ncs::vm::ObjectId(self.person(them).id.0);
        let cast = (id(at), self.row, id(victim));
        self.deferred.iter().any(|delayed| {
            delayed
                .spell
                .as_ref()
                .is_some_and(|by| (by.caster, by.row, by.target) == cast)
        })
    }

    /// Ends `at`'s channel: its wait starts, a grip's victim goes free (a
    /// scripted grip's, of its impact's ticks still to come and its hold).
    fn end(&mut self, at: usize, channel: Channel) {
        if channel.power == Power::Grip
            && let Some(victim) = channel.target.and_then(|id| self.scene.find(id))
            && let Some(row) =
                force::row_at(&self.person(at).sheet.powers, channel.power, channel.level)
            && self
                .library
                .is_some_and(|library| library.impact(row).is_some())
            && self.fighters[victim].force.gripped_by == Some(self.person(at).id)
        {
            if self.authority {
                let id = |them: usize| script_ncs::vm::ObjectId(self.person(them).id.0);
                self.let_go.push((id(at), row, id(victim)));
            }
            let theirs = &mut self.fighters[victim].force;
            theirs.stunned_until = theirs.stunned_until.min(self.now);
        }
        let delay = ja::DELAY_MS[usize::from(channel.level > 1)];
        let force = &mut self.fighters[at].force;
        force.channel = None;
        match channel.power {
            Power::Grip => force.grip_ready = self.now + ja::GRIP_COOLDOWN_MS,
            Power::Lightning => force.lightning_ready = self.now + delay,
            _ => force.drain_ready = self.now + delay,
        }
        if let Some(victim) = channel.target.and_then(|id| self.scene.find(id))
            && self.fighters[victim].force.gripped_by == Some(self.person(at).id)
        {
            self.fighters[victim].force.gripped_by = None;
        }
    }

    /// A running grip's tick (I23 4.6.4): whether it holds on, and if so
    /// its squeezes, at levels 2 and 3 its one crush, and its lift (level 2)
    /// or carry (level 3). False lets go (the caller ends the channel).
    fn squeeze(&mut self, at: usize, channel: &mut Channel) -> bool {
        let Some(victim) = channel.target.and_then(|id| self.scene.find(id)) else {
            return false;
        };
        if self.lets_go(at, victim, channel.level) {
            return false;
        }
        let scripted = self.scripted();
        // A scripted grip holds through its first run, then for as long as
        // its impact's deferred code still waits.
        if scripted && self.now >= channel.from + ja::CHANNEL_MS && !self.ticking(at, victim) {
            return false;
        }
        // Every squeeze due by now; a scripted grip's script deals its own.
        while channel.next_tick <= self.now {
            if !scripted {
                self.hit(at, victim, (ja::GRIP_POINTS, 0, None), None);
            }
            channel.next_tick += ja::GRIP_SQUEEZE_MS;
        }
        let crush_at = channel.from + ja::GRIP_CRUSH_MS;
        if !scripted
            && channel.level > 1
            && crush_at <= self.now
            && self.now < crush_at + u64::from(self.msec)
        {
            self.crush(at, victim, channel.level);
        }
        let held_for = self.now.saturating_sub(channel.from);
        if held_for % ja::GRIP_LIFT_EVERY_MS < u64::from(self.msec) {
            match channel.level {
                2 => self.bodies[victim].velocity[2] = ja::GRIP_LIFT,
                3 => self.carry(at, victim),
                _ => {}
            }
        }
        true
    }

    /// Whether `at`'s grip of `level` on `victim` slips, judged where both
    /// stood before the powers: the victim dead, its absorb taking all the
    /// level, out of reach, out of the front (below level 3), or out of
    /// sight.
    fn lets_go(&self, at: usize, victim: usize, level: u8) -> bool {
        let me = self.person(at);
        let them = self.person(victim);
        let fighter = &self.fighters[victim];
        let effective = level.saturating_sub(fighter.force.absorbing(self.now));
        let mut toward = math::sub(them.ps.origin, me.ps.origin);
        let distance = math::length(toward);
        let forward = me.forward();
        let mut facing = [forward[0], forward[1], 0.0];
        math::normalize(&mut facing);
        toward[2] = 0.0;
        math::normalize(&mut toward);
        !fighter.alive()
            || effective == 0
            || distance > ja::GRIP_RANGE
            || (effective < 3 && math::dot(facing, toward) <= ja::GRIP_FRONT_DOT)
            || !self.scene.clear(me.ps.origin, them.ps.origin)
    }

    /// The crush of a grip of `level` (2 or 3): its points, the victim's
    /// hand to its throat, and any grip of the victim's own let go.
    fn crush(&mut self, at: usize, victim: usize, level: u8) {
        let points = ja::GRIP_CRUSH[usize::from(level - 1)];
        self.hit(at, victim, (points, 0, None), None);
        let theirs = &mut self.fighters[victim].force;
        theirs.hand = Some((Gesture::Choke, self.now + ja::GRIP_CRUSH_HAND_MS));
        if let Some(own) = theirs.channel.filter(|own| own.power == Power::Grip) {
            self.end(victim, own);
        }
    }

    /// A level-3 grip carries its victim's body toward a point ahead of
    /// and above the caster, faster the further it has to go.
    fn carry(&mut self, at: usize, victim: usize) {
        let me = self.person(at);
        let ahead = math::mad(me.ps.origin, ja::GRIP_CARRY_AHEAD, me.forward());
        let point = math::add(ahead, [0.0, 0.0, ja::GRIP_CARRY_UP]);
        let body = &mut self.bodies[victim];
        let mut way = math::sub(point, body.origin);
        let away = math::normalize(&mut way);
        let speed = ja::GRIP_CARRY_WITHIN
            .iter()
            .position(|&band| away < band)
            .map_or(ja::GRIP_CARRY_BEYOND, |band| ja::GRIP_CARRY_SPEEDS[band]);
        body.velocity = scale(way, speed);
    }

    /// Who a line or level-3 cone power reaches this tick, with where: both
    /// from the eye, where the caster aims (JA's from the origin, QUIRKS.md
    /// Q-0032, fixed).
    fn caught(&self, at: usize, level: u8, radius: f32) -> Vec<(usize, Vec3)> {
        let eye = self.person(at).eye();
        if level < 3 {
            self.scene
                .aimed(at, eye, ja::LINE_RANGE)
                .into_iter()
                .collect()
        } else {
            self.scene
                .around_from(at, eye, radius, None, ja::CONE_DOT)
                .into_iter()
                .filter(|&them| self.scene.harms(at, them))
                .map(|them| (them, self.person(them).centre()))
                .collect()
        }
    }

    /// A lightning tick (I23 4.6.5): `ja::LIGHTNING_POINTS` and up to
    /// `ja::LIGHTNING_EXTRA` more JA points to each caught, pushing along
    /// the view; absorb takes off the roll or all of it and feeds the
    /// absorber's pool.
    fn lightning(&mut self, at: usize, level: u8, impact: bool) {
        let forward = self.person(at).forward();
        let caught = self.caught(at, level, ja::LIGHTNING_RADIUS);
        if self.scripted() {
            // Its impact script's damage, every `ja::IMPACT_EVERY_MS` (`impact`).
            if impact && !caught.is_empty() {
                let them: Vec<usize> = caught.iter().map(|(them, _)| *them).collect();
                self.cast_at(at, Some(them[0]), &them);
            }
            return;
        }
        for (them, point) in caught {
            let absorb = self.fighters[them].force.absorbing(self.now);
            let (points, extra) = match (absorb, level.saturating_sub(absorb)) {
                (0, _) => (ja::LIGHTNING_POINTS, ja::LIGHTNING_EXTRA),
                (_, 0) => (0, 0),
                _ => (ja::LIGHTNING_POINTS, 0),
            };
            if absorb > 0 {
                self.feed(them, ja::ABSORB_TICK);
            }
            if points > 0 {
                // KOTOR's lightning is electrical: energy shields take it.
                self.hit(
                    at,
                    them,
                    (points, extra, Some(type_rows::ELECTRICAL)),
                    Some((point, forward, level == 3)),
                );
            }
        }
    }

    /// A drain tick (I23 4.6.6): the caster's own regeneration waits; each
    /// caught one with a pool it may harm loses `ja::DRAIN` JA points of it
    /// by level and the caster heals as many of its health, both on the
    /// authority. Absorb feeds the drained, and what gets through is the
    /// levels it does not cover, taken as the points themselves (as the
    /// game plays it).
    fn drain(&mut self, at: usize, level: u8, impact: bool) {
        let rested = self.now + ja::DRAIN_CASTER_WAIT_MS;
        let own = &mut self.fighters[at].force;
        own.regen_at = own.regen_at.max(rested);
        let caught = self.caught(at, level, ja::DRAIN_RADIUS);
        if self.scripted() {
            // Its impact script's work, every `ja::IMPACT_EVERY_MS` (`impact`).
            let harmed: Vec<usize> = caught
                .iter()
                .map(|&(them, _)| them)
                .filter(|&them| self.scene.harms(at, them))
                .collect();
            if impact && let Some(&first) = harmed.first() {
                self.cast_at(at, Some(first), &harmed);
            }
            return;
        }
        for (them, _) in caught {
            let pool = self.person(them).sheet.max_fp;
            if pool == 0 || self.fighters[them].fp <= 0 || !self.scene.harms(at, them) {
                continue;
            }
            self.reached(at, them, true);
            let absorb = self.fighters[them].force.absorbing(self.now);
            let amount = if absorb > 0 {
                self.feed(them, ja::ABSORB_TICK);
                i32::from(level.saturating_sub(absorb))
            } else {
                ja::DRAIN[usize::from(level.clamp(1, 3) - 1)]
            };
            if amount == 0 || !self.authority {
                continue;
            }
            let victim = &mut self.fighters[them];
            victim.fp -= points(amount, pool).min(victim.fp);
            victim.force.regen_at = victim
                .force
                .regen_at
                .max(self.now + ja::DRAIN_VICTIM_WAIT_MS);
            let max_hp = self.person(at).sheet.max_hp;
            let caster = &mut self.fighters[at];
            caster.hp = (caster.hp + points(amount, max_hp)).min(i32::from(max_hp) * 100);
        }
    }

    /// An absorber gains `count` JA points of its pool (authority only: it
    /// changes another's pool): a throw's `ja::ABSORB_THROW` per absorb
    /// level, a lightning or drain tick's `ja::ABSORB_TICK` (JA's cost ÷ 3,
    /// at least 1).
    fn feed(&mut self, at: usize, count: i32) {
        if !self.authority {
            return;
        }
        let max_fp = self.person(at).sheet.max_fp;
        let fighter = &mut self.fighters[at];
        fighter.fp = (fighter.fp + points(count, max_fp)).min(i32::from(max_fp) * 100);
    }

    /// The power button pressed with `power` selected.
    fn press(&mut self, at: usize, power: Power, level: u8, row: u16) {
        let Some(cost) = self.cost(at, row) else {
            return;
        };
        use crate::combat::spells::{FORCE_POWER_CONFUSION, FORCE_POWER_MIND_TRICK};
        match power {
            // TSL's Mind Trick and Confusion and its Fury are their scripts'
            // where they run (TSL's rules over JA's toggles, the owner's
            // call): the one aimed at frozen or turned (`EffectMindTrick`,
            // `EffectFactionModifier`), the caster's Fury.
            Power::MindTrick
                if self.scripted()
                    && matches!(row, FORCE_POWER_MIND_TRICK | FORCE_POWER_CONFUSION) =>
            {
                if let Some(them) = self.aim_effect(at, cost)
                    && self.authority
                {
                    self.cast_at(at, Some(them), &[them]);
                }
            }
            Power::Rage if self.scripted() => {
                if self.free_hand(at, false) && self.pay(at, cost) {
                    self.fighters[at].force.hand =
                        Some((Gesture::Push, self.now + ja::TRICK_HAND_MS));
                    self.cast_at(at, Some(at), &[at]);
                }
            }
            Power::Speed
            | Power::Protect
            | Power::Absorb
            | Power::MindTrick
            | Power::Rage
            | Power::Sight => {
                self.toggle(at, power, level, cost);
            }
            Power::Push | Power::Pull => self.push_pull(at, power, level, cost),
            Power::Heal => self.heal(at, level, cost),
            Power::Stun => self.crowd(at, row, cost),
            Power::Valor | Power::Resist => self.bless(at, power, level, cost),
            Power::Slow => self.slow(at, level, cost),
            Power::Dispel => self.dispel(at, level, cost),
            // Held powers start while held (`turn`).
            Power::Grip | Power::Lightning | Power::Drain => {}
        }
    }

    /// A toggled power (speed, protect, absorb, mind trick): pressed while
    /// it runs, it stops if it has run `ja::TOGGLE_MIN_MS`; else it starts,
    /// paid now.
    /// Protect and absorb exclude each other.
    fn toggle(&mut self, at: usize, power: Power, level: u8, cost: i32) {
        let force = self.fighters[at].force;
        let (until, running_level) = match power {
            Power::Speed => (force.speed_until, force.speed_level),
            Power::Protect => (force.protect_until, force.protect_level),
            Power::Absorb => (force.absorb_until, force.absorb_level),
            Power::Rage => (force.rage_until, force.rage_level),
            Power::Sight => (force.sight_until, 1),
            _ => (force.trick_until, force.trick_level),
        };
        // Cast again at its running level it turns off (after its least
        // time); another rank replaces it, as K1's scripts replace a weaker
        // speed or shield with the one cast.
        if until > self.now && level == running_level {
            if self.now + toggle_length(power, running_level) >= until + ja::TOGGLE_MIN_MS {
                self.set_toggle(at, power, 0, 0);
            }
            return;
        }
        if power == Power::MindTrick && !self.free_hand(at, false) {
            return;
        }
        // Rage needs `ja::RAGE_LEAST` JA points of health left and its
        // recovery over.
        let max_hp = self.person(at).sheet.max_hp;
        if power == Power::Rage
            && (force.recover_until > self.now
                || self.fighters[at].hp < points(ja::RAGE_LEAST, max_hp))
        {
            return;
        }
        if !self.pay(at, cost) {
            return;
        }
        self.set_toggle(at, power, self.now + toggle_length(power, level), level);
        // Its script adds KOTOR's side (defence, saves, resistance; the
        // native stand-ins give way to it, `effects::sheet_now`). Mind
        // trick's KOTOR rows aim at another (Affect Mind), not JA's shape.
        if power != Power::MindTrick && self.scripted() {
            self.cast_at(at, Some(at), &[at]);
        }
        // Protect, absorb and rage exclude each other.
        let excluded: &[Power] = match power {
            Power::Protect => &[Power::Absorb, Power::Rage],
            Power::Absorb => &[Power::Protect, Power::Rage],
            Power::Rage => &[Power::Protect, Power::Absorb],
            _ => &[],
        };
        for &other in excluded {
            self.set_toggle(at, other, 0, 0);
        }
        match power {
            Power::Rage => {
                self.fighters[at].force.rage_paid_at =
                    self.now + ja::RAGE_HP_EVERY_MS[usize::from(level.clamp(1, 3) - 1)];
            }
            Power::MindTrick => self.trick(at, level),
            _ => {}
        }
    }

    fn set_toggle(&mut self, at: usize, power: Power, until: u64, level: u8) {
        let now = self.now;
        if until == 0 {
            effects::end_power(&mut self.fighters[at], power, now);
        }
        let force = &mut self.fighters[at].force;
        match power {
            Power::Speed => (force.speed_until, force.speed_level) = (until, level),
            Power::Protect => (force.protect_until, force.protect_level) = (until, level),
            Power::Absorb => (force.absorb_until, force.absorb_level) = (until, level),
            Power::Rage => {
                // Turned off, it ends now and its recovery starts.
                if until == 0 {
                    force.rage_until = force.rage_until.min(now);
                } else {
                    (force.rage_until, force.rage_level) = (until, level);
                }
            }
            Power::Sight => force.sight_until = until,
            _ => {
                (force.trick_until, force.trick_level) = (until, level);
                if until == 0 {
                    force.tricked = 0;
                }
            }
        }
    }

    /// Mind trick's cast (I23 4.6.11): the hand (`ja::TRICK_HAND_MS`);
    /// level 1 the one aimed at within `ja::TRICK_AIMED`, level 2 everyone
    /// not on its side within `ja::TRICK_AREA` and `ja::TRICK_FIELD` of the
    /// view. Players stop seeing the caster (the view hides it); a creature
    /// that is no Force user loses its enemy for `ja::CONFUSE_MS` by level
    /// (authority only).
    fn trick(&mut self, at: usize, level: u8) {
        self.fighters[at].force.hand = Some((Gesture::Push, self.now + ja::TRICK_HAND_MS));
        let caught: Vec<usize> = if level < 2 {
            let eye = self.person(at).eye();
            self.scene
                .aimed(at, eye, ja::TRICK_AIMED)
                .map(|(them, _)| them)
                .into_iter()
                .collect()
        } else {
            self.scene
                .around(at, ja::TRICK_AREA, Some(ja::TRICK_FIELD), -1.0)
        };
        for them in caught
            .into_iter()
            .filter(|&them| !self.scene.side(at, them))
        {
            self.reached(at, them, true);
            match self.person(them).seat {
                Some(seat) => self.fighters[at].force.tricked |= super::party::bit(seat),
                None if self.authority && self.person(them).sheet.max_fp == 0 => {
                    let theirs = &mut self.fighters[them];
                    theirs.force.confused_until =
                        self.now + ja::CONFUSE_MS[usize::from(level.clamp(1, 3) - 1)];
                    theirs.target = None;
                }
                None => {}
            }
        }
    }

    /// Push or pull (I23 4.6.2, 4.6.3) at `level`, costing `cost`: the
    /// caster's hand and wait, then everyone caught in turn, each
    /// countering what it can, the push breaking a grip on the caster,
    /// knocking down, whirling (push 2) and shoving; a scripted push hands
    /// whom it shoved to its impact script.
    fn push_pull(&mut self, at: usize, power: Power, level: u8, cost: i32) {
        if self.fighters[at].force.push_pull_ready > self.now
            || !self.free_hand(at, true)
            || !self.pay(at, cost)
        {
            return;
        }
        let pull = power == Power::Pull;
        let (gesture, hand, wait) = if pull {
            (Gesture::Pull, ja::PULL_HAND_MS, ja::PULL_COOLDOWN_MS)
        } else {
            (Gesture::Push, ja::PUSH_HAND_MS, ja::PUSH_COOLDOWN_MS)
        };
        let force = &mut self.fighters[at].force;
        force.hand = Some((gesture, self.now + hand));
        force.push_pull_ready = self.now + wait;
        force.tricked = 0;
        let caught: Vec<usize> = if level < 2 {
            let eye = self.person(at).eye();
            self.scene
                .aimed(at, eye, ja::PUSH_PULL_AIMED)
                .map(|(them, _)| them)
                .into_iter()
                .collect()
        } else {
            let field = ja::PUSH_PULL_FIELD[usize::from(level >= 3)];
            self.scene
                .around(at, ja::PUSH_PULL_RANGE, Some(field), ja::PUSH_PULL_FRONT)
        };
        let scripted = !pull && self.scripted();
        let mut shoved = Vec::new();
        for them in caught {
            if self.scene.side(at, them)
                || !self.fighters[them].alive()
                || self.fighters[them].saber.floored()
            {
                continue;
            }
            shoved.push(them);
            self.reached(at, them, true);
            let absorb = self.fighters[them].force.absorbing(self.now);
            if absorb > 0 {
                self.feed(them, ja::ABSORB_THROW * i32::from(absorb));
            }
            let effective = level.saturating_sub(absorb);
            if effective == 0 {
                continue;
            }
            let share = self.countered(them, power, effective, gesture, hand);
            self.break_grip(at, them, effective);
            self.knock(at, them, power, effective);
            if !pull && level == 2 && share > 0.0 && !scripted {
                self.whirlwind(at, them);
            }
            let strength = (ja::PUSH_PULL_STRENGTH * f32::from(effective)) * share;
            if strength > 0.0 {
                self.shove(at, them, strength, pull);
            }
        }
        if scripted && !shoved.is_empty() {
            let target = if level < 3 {
                self.throw_target(at, &shoved)
            } else {
                None
            };
            self.cast_at(at, target, &shoved);
        }
    }

    /// A gripped caster's throw at its gripper of no higher a grip level
    /// than `effective` breaks the grip, and the gripper waits
    /// `ja::GRIP_BROKEN_MS` for its next.
    fn break_grip(&mut self, at: usize, them: usize, effective: u8) {
        if self.fighters[at].force.gripped_by != Some(self.person(them).id) {
            return;
        }
        let Some(grip) = self.fighters[them]
            .force
            .channel
            .filter(|grip| grip.power == Power::Grip && grip.level <= effective)
        else {
            return;
        };
        self.end(them, grip);
        self.fighters[them].force.grip_ready = self.now + ja::GRIP_BROKEN_MS;
    }

    /// A full-strength throw (`effective` 3) knocks down one that is not
    /// level 3 against it, within `ja::KNOCKDOWN_REACH` for each level it
    /// is short of 2, and it gets up the quick way.
    fn knock(&mut self, at: usize, them: usize, power: Power, effective: u8) {
        let own = self.push_pull_level(them, power);
        let reach = ja::KNOCKDOWN_REACH * f32::from(2u8.saturating_sub(own));
        let caster = self.person(at).ps.origin;
        let distance = math::length(math::sub(self.bodies[them].origin, caster));
        let saber = &mut self.fighters[them].saber;
        if effective == 3 && own < 3 && distance <= reach && !saber.floored() {
            saber.quicker = true;
            saber.knock_down(self.now, ja::KNOCKDOWN_MS);
        }
    }

    /// Sends `them`'s body away from the caster (toward it on a pull) at
    /// `strength`, less with distance, at least `ja::SHOVE_MIN`; one
    /// standing still vertically is lifted at least `ja::SHOVE_LIFT`.
    fn shove(&mut self, at: usize, them: usize, strength: f32, pull: bool) {
        let caster = self.person(at).ps.origin;
        let body = &mut self.bodies[them];
        let mut direction = math::sub(body.origin, caster);
        let distance = math::normalize(&mut direction);
        if pull {
            direction = scale(direction, -1.0);
        }
        let speed = (strength - ja::SHOVE_FALLOFF * distance).max(ja::SHOVE_MIN);
        let up = direction[2] * speed;
        let rise = if body.velocity[2] == 0.0 {
            up.max(ja::SHOVE_LIFT)
        } else {
            up
        };
        body.velocity = [direction[0] * speed, direction[1] * speed, rise];
    }

    /// A scripted push's target below level 3: the one aimed at within
    /// `ja::PUSH_PULL_RANGE` if it was shoved, else the shoved one nearest the
    /// caster (the first of equals).
    fn throw_target(&self, at: usize, shoved: &[usize]) -> Option<usize> {
        let eye = self.person(at).eye();
        let caster = self.person(at).ps.origin;
        self.scene
            .aimed(at, eye, ja::PUSH_PULL_RANGE)
            .map(|(them, _)| them)
            .filter(|them| shoved.contains(them))
            .or_else(|| {
                let away = |them: usize| math::length(math::sub(self.bodies[them].origin, caster));
                shoved
                    .iter()
                    .copied()
                    .min_by(|&a, &b| away(a).total_cmp(&away(b)))
            })
    }

    /// `them`'s level against a push or pull: its push chain's best, one
    /// less while it moves (a forward or side key held).
    fn push_pull_level(&self, them: usize, power: Power) -> u8 {
        let known =
            force::best(&self.person(them).sheet.powers, power).map_or(0, |(_, level)| level);
        let command = &self.fighters[them].last_command;
        let moving = command.forward != 0 || command.right != 0;
        known.saturating_sub(u8::from(moving))
    }

    /// How much of a throw at `effective` level reaches `them`: all of it
    /// unless it can counter (it knows the power, stands on the ground with
    /// a free hand, its own throw wait over, not held). Countering, it
    /// throws the caster's `gesture` back for `hand` ms and stops all of it
    /// at its level or above, else lets through `ja::COUNTERED` by how far
    /// short it falls.
    fn countered(
        &mut self,
        them: usize,
        power: Power,
        effective: u8,
        gesture: Gesture,
        hand: u64,
    ) -> f32 {
        let level = self.push_pull_level(them, power);
        let force = &self.fighters[them].force;
        let counters = level > 0
            && self.bodies[them].ground.is_some()
            && self.free_hand(them, false)
            && force.push_pull_ready <= self.now
            && !force.stunned(self.now);
        if !counters {
            return 1.0;
        }
        self.fighters[them].force.hand = Some((gesture, self.now + hand));
        if level >= effective {
            0.0
        } else {
            ja::COUNTERED[usize::from((effective - level).min(2))]
        }
    }

    /// KOTOR's Cure and Heal as JA's team heal (a stand-in):
    /// everyone on the caster's side within `ja::HEAL_RANGE` by level who is
    /// hurt, the caster too, gains `ja::HEAL` JA points by how many are
    /// healed; paid only if someone is hurt.
    fn heal(&mut self, at: usize, level: u8, cost: i32) {
        let reach = ja::HEAL_RANGE[usize::from(level.clamp(1, 3) - 1)];
        let origin = self.person(at).ps.origin;
        let hurt: Vec<usize> = (0..self.fighters.len())
            .filter(|&them| self.scene.side(at, them) && self.fighters[them].alive())
            .filter(|&them| {
                self.fighters[them].hp < i32::from(self.person(them).sheet.max_hp) * 100
            })
            .filter(|&them| math::length(math::sub(self.person(them).ps.origin, origin)) <= reach)
            .collect();
        if hurt.is_empty() || !self.pay(at, cost) || !self.authority {
            return;
        }
        if self.scripted() {
            // Its script heals (KOTOR's amounts, its party in range).
            self.cast_at(at, Some(at), &hurt);
            return;
        }
        let each = ja::HEAL[hurt.len().min(3) - 1];
        for them in hurt {
            self.reached(at, them, false);
            let max_hp = self.person(them).sheet.max_hp;
            let fighter = &mut self.fighters[them];
            fighter.hp = (fighter.hp + points(each, max_hp)).min(i32::from(max_hp) * 100);
        }
    }

    /// KOTOR's crowd control (C11): the hand (`ja::TRICK_HAND_MS`); the one
    /// aimed at within `ja::STUN_RANGE`, or for the area rows everyone not
    /// on the caster's side within it, `ja::STUN_FIELD` of the view and in
    /// front; each one not immune (C11's second, or the row's Force Immunity
    /// feat) that does not resist the Force (`resists`) and fails its save
    /// (`crowd::save`, against `power_dc`) is held for `ja::STUN_MS`, then
    /// immune for `ja::IMMUNE_MS`; fear ends on damage. The droid powers only take droids, and go out only at one
    /// aimed at (KOTOR offers them at no other target); the rest only the
    /// living (authority only: the rolls).
    fn crowd(&mut self, at: usize, row: u16, cost: i32) {
        if !self.free_hand(at, false) {
            return;
        }
        let droid = |cast: &Self, them: usize| cast.person(them).sheet.flags & flags::DROID != 0;
        let caught = self.crowd_caught(at, row);
        if crowd::droids(row) && caught.first().is_none_or(|&them| !droid(self, them)) {
            return;
        }
        if !self.pay(at, cost) {
            return;
        }
        let force = &mut self.fighters[at].force;
        force.hand = Some((Gesture::Push, self.now + ja::TRICK_HAND_MS));
        force.tricked = 0;
        if !self.authority {
            return;
        }
        let alive: Vec<usize> = caught
            .into_iter()
            .filter(|&them| !self.scene.side(at, them) && self.fighters[them].alive())
            .filter(|&them| !crowd::droids(row) || droid(self, them))
            .collect();
        if self.scripted() {
            // Its impact script decides each one (droids or the living,
            // saves, holds).
            let aimed = (!crowd::area(row))
                .then(|| alive.first().copied())
                .flatten();
            self.cast_at(at, aimed, &alive);
            return;
        }
        let dc = self.power_dc(at);
        let save = crowd::save(row);
        for them in alive {
            let sheet = self.person(them).sheet;
            if droid(self, them) != crowd::droids(row) {
                continue;
            }
            // Signalled before its resistance and save, as KOTOR's is.
            self.reached(at, them, true);
            if self.fighters[them].force.immune_until > self.now
                || crowd::immunity(row).is_some_and(|feat| sheet.feats.has(feat))
                || [crowd::immunity_type(row), crowd::immunity::MIND_SPELLS]
                    .iter()
                    .any(|kind| self.fighters[them].effects.immune_to(*kind, self.now))
                || self.resists(at, them)
                || self.saves(them, save, dc)
            {
                continue;
            }
            let theirs = &mut self.fighters[them].force;
            theirs.hold(self.now, ja::STUN_MS);
            theirs.hold = if crowd::fear(row) {
                Hold::Fear
            } else {
                Hold::Stun
            };
        }
    }

    /// Whom crowd control row `row` from `at` catches: the one aimed at
    /// within `ja::STUN_RANGE` (first) and, for a splash, everyone round it;
    /// for the area rows everyone within it, `ja::STUN_FIELD` of the view
    /// and in front.
    fn crowd_caught(&self, at: usize, row: u16) -> Vec<usize> {
        if crowd::area(row) {
            self.scene.around(
                at,
                ja::STUN_RANGE,
                Some(ja::STUN_FIELD),
                ja::PUSH_PULL_FRONT,
            )
        } else {
            let eye = self.person(at).eye();
            let aimed = self
                .scene
                .aimed(at, eye, ja::STUN_RANGE)
                .map(|(them, _)| them);
            match (aimed, crowd::splash(row)) {
                // The one aimed at first, then everyone round it.
                (Some(them), Some(metres)) => std::iter::once(them)
                    .chain(
                        self.scene
                            .around_from(at, self.person(them).ps.origin, metres / UNIT, None, -1.0)
                            .into_iter()
                            .filter(|&near| near != them),
                    )
                    .collect(),
                _ => aimed.into_iter().collect(),
            }
        }
    }

    /// Valor or Energy Resistance (`combat::effects`): on the caster, and
    /// its side within `effects::PARTY_REACH` for Valor and the improved
    /// resistance; the hand (`ja::TRICK_HAND_MS`).
    fn bless(&mut self, at: usize, power: Power, level: u8, cost: i32) {
        if !self.free_hand(at, false) || !self.pay(at, cost) {
            return;
        }
        self.fighters[at].force.hand = Some((Gesture::Push, self.now + ja::TRICK_HAND_MS));
        let party = power == Power::Valor || level >= 2;
        let origin = self.person(at).ps.origin;
        let blessed: Vec<usize> = (0..self.fighters.len())
            .filter(|&them| {
                them == at
                    || (party
                        && self.scene.side(at, them)
                        && self.fighters[them].alive()
                        && math::length(math::sub(self.person(them).ps.origin, origin))
                            <= effects::PARTY_REACH)
            })
            .collect();
        if self.scripted() {
            // Its script grants the bonuses (KOTOR's, for their time).
            self.cast_at(at, Some(at), &blessed);
            return;
        }
        for them in blessed {
            self.reached(at, them, false);
            let theirs = &mut self.fighters[them].effects;
            match power {
                // A stronger one running stays.
                Power::Valor if theirs.valor(self.now) <= level => {
                    theirs.valor = (level, self.now + effects::VALOR_MS)
                }
                Power::Resist if theirs.resisting(self.now) <= level => {
                    theirs.resist = (level, self.now + effects::RESIST_MS)
                }
                _ => {}
            }
        }
    }

    /// The one a KOTOR effect power aims at (the crowd control's reach),
    /// if it is not on the caster's side and lives; the hand
    /// (`ja::TRICK_HAND_MS`), the cost paid either way.
    fn aim_effect(&mut self, at: usize, cost: i32) -> Option<usize> {
        if !self.free_hand(at, false) || !self.pay(at, cost) {
            return None;
        }
        let force = &mut self.fighters[at].force;
        force.hand = Some((Gesture::Push, self.now + ja::TRICK_HAND_MS));
        force.tricked = 0;
        let eye = self.person(at).eye();
        let (them, _) = self.scene.aimed(at, eye, ja::STUN_RANGE)?;
        let reached = !self.scene.side(at, them) && self.fighters[them].alive();
        if reached {
            self.reached(at, them, true);
        }
        reached.then_some(them)
    }

    /// The caster's DC for its powers, KOTOR's engine rule (`GetSpellSaveDC`;
    /// `k_sp1_generic` asks it: `force::save_dc`).
    fn power_dc(&self, at: usize) -> i32 {
        force::save_dc(&self.person(at).sheet)
    }

    /// KOTOR's Force Resistance (`ResistForce`, before any save): one under
    /// Resist Force or Force Immunity (absorb at 1 or 2) has a resistance
    /// (`force::resistance`), a script's `EffectForceResistanceIncrease` its
    /// amount (the higher counts), and resists unless the caster's d20 +
    /// level reaches it (authority only: a roll).
    fn resists(&mut self, at: usize, them: usize) -> bool {
        let tier = self.fighters[them].force.absorbing(self.now);
        // Effects' and worn items' (Improved Force Resistance), the higher.
        let granted = self.fighters[them]
            .effects
            .sum(Modifies::ForceResistance, self.now)
            .max(i32::from(self.person(them).sheet.gear.force_resist));
        let resistance = force::resistance(self.person(them).sheet.level, tier, granted);
        if resistance <= 0 {
            return false;
        }
        let d20 = i32::try_from(self.rng.below(rules::interaction::checks::DIE)).unwrap_or(0) + 1;
        d20 + i32::from(self.person(at).sheet.level) < resistance
    }

    /// A d20 save of `them`'s (`Sheet::save`'s `index`) against `dc`
    /// (`combat::check`; authority only: a roll).
    fn saves(&mut self, them: usize, index: usize, dc: i32) -> bool {
        let total = self.person(them).sheet.save(index);
        let d20 = i32::try_from(self.rng.below(rules::interaction::checks::DIE)).unwrap_or(0) + 1;
        crate::combat::check(d20, total, dc)
    }

    /// Slow, Affliction or Plague at the one aimed at (not droids;
    /// `combat::effects`): spared by resisting the Force (`resists`), Slow
    /// also by a Will save, Affliction by a Fortitude save at
    /// `effects::AFFLICTION_DC`, Plague by none (authority only).
    fn slow(&mut self, at: usize, level: u8, cost: i32) {
        let Some(them) = self.aim_effect(at, cost) else {
            return;
        };
        if self.scripted() {
            self.cast_at(at, Some(them), &[them]);
            return;
        }
        if !self.authority || self.person(them).sheet.flags & flags::DROID != 0 {
            return;
        }
        let spared = self.resists(at, them)
            || match level {
                1 => {
                    let dc = self.power_dc(at);
                    self.saves(them, 2, dc)
                }
                // Affliction and Plague are poisons: Knight Valor and up and
                // the improved resistance keep them off.
                _ if self.fighters[them].effects.poison_immune(self.now) => true,
                2 => self.saves(them, 0, effects::AFFLICTION_DC),
                _ => false,
            };
        if spared {
            return;
        }
        let length = match level {
            1 => effects::SLOW_MS,
            _ => {
                let sapping = usize::from(level.min(3) - 2);
                u64::from(effects::SAP_MOST[sapping]) * u64::from(effects::SAP_EVERY_MS[sapping])
            }
        };
        let theirs = &mut self.fighters[them].effects;
        theirs.slowed = (level.clamp(1, 3), self.now + length);
        // The first point goes at once: all `most` within its time.
        theirs.sapped = (0, self.now);
    }

    /// Supress Force (level 1) or Force Breach (2) at the one aimed at:
    /// its first- and second-tier powers end, or all of them.
    fn dispel(&mut self, at: usize, level: u8, cost: i32) {
        // The cost and the hand are predicted; another's powers end on the
        // authority (I22 section 10).
        if let Some(them) = self.aim_effect(at, cost)
            && self.authority
        {
            if self.scripted() {
                self.cast_at(at, Some(them), &[them]);
            } else {
                effects::dispel(&mut self.fighters[them], level >= 2, self.now);
            }
        }
    }

    /// Whirlwind (push's second level) on one it caught: resisting the
    /// Force (`resists`) or a Reflex save against the caster's DC spares
    /// it, else held (C11) and the caster's level ÷ `ja::WHIRLWIND_DIVISOR`
    /// in damage, at least `ja::WHIRLWIND_LEAST` (authority only).
    fn whirlwind(&mut self, at: usize, them: usize) {
        // Only a hostile that is no plot creature (C17: neutrals and plot
        // creatures take nothing from the party's powers).
        let plot = self.person(them).sheet.flags & flags::PLOT != 0;
        if !self.authority
            || plot
            || !self.scene.hostile(at, them)
            || self.fighters[them].force.immune_until > self.now
        {
            return;
        }
        let dc = self.power_dc(at);
        if self.resists(at, them) || self.saves(them, 1, dc) {
            return;
        }
        if self.fighters[them].force.hold(self.now, ja::STUN_MS) {
            self.fighters[them].force.hold = Hold::Whirlwind;
        }
        let damage = (u16::from(self.person(at).sheet.level) / ja::WHIRLWIND_DIVISOR)
            .max(ja::WHIRLWIND_LEAST);
        self.hits.push(Hit {
            attacker: self.person(at).id,
            target: self.person(them).id,
            source: Source::Script {
                damage,
                damage_type: None,
                cast: false,
                shot: None,
                staged: false,
            },
            point: None,
            direction: None,
        });
    }

    /// Rage's running (I23 4.6.9): `ja::RAGE_HP` JA points of health every
    /// `ja::RAGE_HP_EVERY_MS` by level (authority only), stopping at
    /// `ja::RAGE_FLOOR_HP`; once it has ended (run out, turned off, or out
    /// of health) its recovery (`ja::RECOVER_MS`).
    fn rage(&mut self, at: usize) {
        let now = self.now;
        let max_hp = self.person(at).sheet.max_hp;
        let authority = self.authority;
        let fighter = &mut self.fighters[at];
        let force = &mut fighter.force;
        if force.rage_level > 0 && force.rage_until <= now {
            force.recover_until = force.rage_until + ja::RECOVER_MS;
            force.rage_level = 0;
            return;
        }
        if force.rage_until <= now {
            return;
        }
        let every = ja::RAGE_HP_EVERY_MS[usize::from(force.rage_level.clamp(1, 3) - 1)];
        while force.rage_paid_at <= now {
            force.rage_paid_at += every;
            if !authority {
                continue;
            }
            let cost = points(ja::RAGE_HP, max_hp);
            if fighter.hp - cost < ja::RAGE_FLOOR_HP * 100 {
                fighter.hp = ja::RAGE_FLOOR_HP * 100;
                force.rage_until = now;
                break;
            }
            fighter.hp -= cost;
        }
    }

    /// `at`'s upkeep after everyone's turn: the dead let go of everything;
    /// a hand or trick that ran out is put away; a victim whose gripper
    /// let go is free; regeneration; its speed (`ja::SPEED_SCALE` speeding,
    /// `ja::GRIPPING_SCALE` gripping, `ja::GRIPPED_SCALE` gripped) and
    /// gravity (none while lifted or carried); the hand it holds out.
    fn upkeep(&mut self, at: usize) {
        let now = self.now;
        if !self.fighters[at].alive() {
            if let Some(channel) = self.fighters[at].force.channel {
                self.end(at, channel);
            }
            self.fighters[at].force.stop();
            // The dead fall and lie as anyone does (a lifted body would
            // float on).
            let body = &mut self.bodies[at];
            body.gravity = motion::GRAVITY;
            body.run_speed = motion::BASE_SPEED;
            return;
        }
        let me = self.person(at).id;
        let force = &mut self.fighters[at].force;
        if force.hand.is_some_and(|(_, until)| until <= now) {
            force.hand = None;
        }
        if force.trick_until <= now {
            force.tricked = 0;
        } else if force.tricked == 0 {
            // No one left tricked: the power stops (JA).
            force.trick_until = 0;
        }
        let gripped_by = force.gripped_by;
        let gripper = gripped_by
            .and_then(|id| self.scene.find(id))
            .and_then(|by| {
                self.fighters[by]
                    .force
                    .channel
                    .filter(|grip| grip.power == Power::Grip && grip.target == Some(me))
            });
        if gripper.is_none() {
            self.fighters[at].force.gripped_by = None;
        }
        self.rage(at);
        // A player using sight sees through every mind trick on it.
        if let Some(seat) = self.person(at).seat
            && self.fighters[at].force.sight_until > now
        {
            for fighter in self.fighters.iter_mut() {
                fighter.force.tricked &= !super::party::bit(seat);
            }
        }
        let sheet = self.person(at).sheet;
        let pool = self.person(at).pool;
        let origin = self.person(at).ps.origin;
        let fighter = &mut self.fighters[at];
        // Items run out: a shield or a stim ends (a use lands in the
        // firing step, `items::press`); Affliction and Plague sap.
        items::settle(fighter, now);
        fighter.effects.sap(now);
        effects::worn(fighter, &sheet, pool, (now, self.tick));
        if self.authority {
            effects::regenerate(fighter, (sheet.max_hp, pool), (now, self.msec));
        }
        if self.person(at).party
            && let Some(regeneration) = self.regeneration
        {
            regenerate(
                &mut self.fighters[at],
                &sheet,
                regeneration,
                (now, self.msec),
            );
        }
        if self.person(at).party
            && let Some(rates) = self.health_regeneration
        {
            regenerate_k1(&mut self.fighters[at], &sheet, rates, (now, self.msec));
        }
        let party = self.person(at).party;
        let module = self.scene.level.module.as_ref();
        let fighter = &mut self.fighters[at];
        // A real fight ends a fake one (TSL's combat reason).
        if fighter.combat_until > now {
            fighter.fake_until = 0;
        }
        let over = fighter.effects.sum(Modifies::ForcePoints, now).max(0);
        match self.fp_regeneration.filter(|_| sheet.max_fp > 0) {
            // TSL's: its party's Force users alone regenerate, by its rule
            // (its other creatures never do).
            Some(table) => {
                if party {
                    let fighting = fighter.combat_until > now;
                    let percent = fighter.effects.sum(Modifies::FpRegen, now);
                    let rating = module.map_or(0, |module| {
                        module.force.rating(scale(origin, collision::UNIT))
                    });
                    let (force, fp) = (&mut fighter.force, &mut fighter.fp);
                    let step = (fighting, percent, rating, over);
                    force::regenerate_tsl(force, fp, &sheet, table, step, self.msec);
                }
            }
            None => {
                // Nor while a saber special plays (rulebook I24 2.12).
                let paused = fighter.force.pausing(now)
                    || fighter.saber.in_special()
                    || fighter.saber.flying();
                force::regenerate(&mut fighter.force, &mut fighter.fp, pool, paused, now);
            }
        }
        // Temporary Force points past the pool go when their effect ends;
        // temporary hit points too (a battle stim's own, the native rule's,
        // go with the stim).
        fighter.fp = fighter.fp.min(i32::from(pool) * 100 + over * 100);
        if fighter.pack.stim.is_none() {
            let vitality = fighter.effects.sum(Modifies::Vitality, now).max(0) * 100;
            fighter.pack.vitality = fighter.pack.vitality.min(vitality);
        }
        let force = fighter.force;
        let aiming = fighter.shooting.aiming;
        let slowed = fighter.effects.slowed_down(now);
        let speeding = force.speed_until > now;
        let mut speed = if speeding { ja::SPEED_SCALE } else { 1.0 };
        // Aiming a Sniper Shot halves movement (JA's disruptor zoom), as
        // Affliction and Plague do.
        if aiming {
            speed *= specials::AIM_SPEED;
        }
        if slowed {
            speed *= effects::SLOWED_SPEED;
        }
        // Scripts' speed effects: percent more or less. The speed powers'
        // own (K1's +99 %) are JA's speed's while it runs: it moves the
        // caster (C10).
        let speed_rows = [
            crate::combat::spells::FORCE_POWER_SPEED_BURST,
            crate::combat::spells::FORCE_POWER_KNIGHT_SPEED,
            crate::combat::spells::FORCE_POWER_SPEED_MASTERY,
        ];
        let percent: i32 = fighter
            .effects
            .counted(now)
            .filter(|l| l.modifies == Modifies::Speed)
            .filter(|l| !(speeding && speed_rows.contains(&l.spell)))
            .map(|l| i32::from(l.amount))
            .sum();
        speed *= (100 + percent).clamp(effects::SPEED_LEAST, effects::SPEED_MOST) as f32 / 100.0;
        if force.raging(now) > 0 {
            speed *= ja::RAGE_SPEED;
        } else if force.recover_until > now {
            speed *= ja::RECOVER_SPEED;
        }
        if force
            .channel
            .is_some_and(|channel| channel.power == Power::Grip)
        {
            speed *= ja::GRIPPING_SCALE;
        }
        if gripper.is_some() {
            speed *= ja::GRIPPED_SCALE[usize::from(speeding)];
        }
        let body = &mut self.bodies[at];
        body.run_speed = motion::BASE_SPEED * speed;
        body.gravity = if gripper.is_some_and(|grip| grip.level > 1) {
            0.0
        } else {
            motion::GRAVITY
        };
        // A grip's victim clutches its throat while held (JA's choke, the
        // whole body once it is off the ground), as KOTOR's choked do.
        let choke = Some((Anim::BOTH_CHOKE3, body.ground.is_none()));
        let gesture = match (force.hand, force.channel) {
            _ if gripper.is_some() => choke,
            (Some((Gesture::Choke, _)), _) => choke,
            (Some((Gesture::Push, _)), _) => Some((Anim::BOTH_FORCEPUSH, false)),
            (Some((Gesture::Pull, _)), _) => Some((Anim::BOTH_FORCEPULL, false)),
            (None, Some(channel)) if channel.power == Power::Lightning => {
                Some((Anim::BOTH_FORCELIGHTNING_HOLD, false))
            }
            (None, Some(_)) => Some((Anim::BOTH_FORCEGRIP_HOLD, false)),
            (None, None) => None,
        };
        if let Some((anim, whole)) = gesture {
            motion::hold(body, &self.scene.level.anims, anim, whole, self.msec);
        }
    }
}
