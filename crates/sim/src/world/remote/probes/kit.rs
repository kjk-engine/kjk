//! The kit (`kit`): the shock bolt, the Sonic Blast and the stim dart, each
//! on whom the gate lets it touch, credited to the leader, with no dice.

use collision::UNIT;

use super::super::super::drivers::probes::poke;
use super::super::super::{Fighting, Id, Player, SimWorld, Streams};
use super::super::{Remotes, kit};
use crate::combat::Fighter;
use crate::combat::sheet::{Sheet, factions, flags};
use crate::input::{Command, Seat, TickInput, buttons};
use crate::math;
use crate::module::{Body, ObjectData, ObjectKind, Placement, Repute};
use crate::motion::MotionState;
use crate::probe::{Checks, check};
use crate::snapshot::{Kind, SimId, Snapshot};
use crate::{Guests, WorldConfig};

pub(super) fn probes() -> Checks {
    vec![
        ("bolt_hits_fair_only", bolt_fair()),
        ("bolt_kill_gives_xp", bolt_xp()),
        ("sonic_pushes_fair_only", sonic()),
        ("dart_revives_and_heals", dart()),
        ("dart_respects_duel", dart_duel()),
        ("dart_cannot_beat_the_loss", dart_lost()),
        ("two_darts_one_body", two_darts()),
        ("kit_draws_no_rng", no_dice()),
    ]
}

/// The creatures on the range, by tag.
const FOE: &str = "kit_foe";
const NEUTRAL: &str = "kit_neutral";
const DUELIST: &str = "kit_duelist";
const BIG: &str = "kit_big";
const SECOND: &str = "kit_second";
/// A companion following seat 0 but no guest's: the party's second.
const MATE: &str = "kit_mate";

/// The proving ground with its factions at war, five creatures parked in
/// a corner, seat 0 playing and `seats - 1` guests flying droids.
struct Range {
    world: SimWorld,
    /// Seat 0's body.
    leader: SimId,
    /// Where seat 0 stands (m), and the ground's axis from it (a unit).
    origin: [f32; 3],
    axis: [f32; 3],
}

fn body(size: u8) -> Body {
    Body {
        appearance: 0,
        size,
        movement: 0,
        walk_mps: 1.0,
        run_mps: 2.0,
        head_arc_h: 0.0,
        bag: 0,
        destroy_ms: 0,
        fade_ms: None,
    }
}

fn range(seats: u8) -> Result<Range, String> {
    let proving = crate::determinism::proving_ground();
    let placed = |tag: &str| {
        proving
            .placed()
            .iter()
            .find(|facts| facts.tag == tag)
            .map(|facts| facts.position)
            .ok_or(format!("the ground has no {tag}"))
    };
    let (gate, way_out) = (placed("gate")?, placed("way_out")?);
    let mut data = (**proving.module().ok_or("the ground has no module")?).clone();
    let creature = |tag: &str, faction: u8, flag: u8, size: u8| {
        let mut sheet = Sheet::commoner();
        sheet.faction = faction;
        sheet.flags |= flag;
        Placement::Own(ObjectData {
            tag: tag.into(),
            sheet: Some(sheet),
            body: Some(body(size)),
            ..ObjectData::unknown(tag, way_out, 0.0)
        })
    };
    data.repute = Repute::at_war([factions::HOSTILE_1, factions::PLAYER]);
    data.placements.extend([
        creature(FOE, factions::HOSTILE_1, 0, 3),
        creature(NEUTRAL, factions::NEUTRAL, 0, 3),
        creature(DUELIST, factions::HOSTILE_1, flags::MIN_ONE_HP, 3),
        creature(BIG, factions::HOSTILE_1, 0, 5),
        creature(SECOND, factions::HOSTILE_1, 0, 3),
        creature(MATE, factions::PLAYER, 0, 3),
    ]);
    let level = proving.with_module(data);
    let config = WorldConfig {
        seats,
        guests: Guests::Companions,
        ..WorldConfig::one_seat()
    };
    let mut world = SimWorld::new(config, level).map_err(|why| why.to_string())?;
    let first = step(&mut world, seats, &[]);
    let leader = first.body(Seat(0)).ok_or("seat 0 has no body")?;
    let origin = leader.position;
    let mut axis = math::sub(way_out, gate);
    axis[2] = 0.0;
    math::normalize(&mut axis);
    let mut range = Range {
        leader: leader.id,
        world,
        origin,
        axis,
    };
    let mate = range.id(MATE)?;
    let spot = range.spot(2.0, 1.5);
    range.put(mate, spot);
    range.edit(mate, |fighter| fighter.mind.leader = Some(Seat(0)));
    Ok(range)
}

/// One step: seat 0 standing, the guests with `given` commands (else
/// still).
fn step(world: &mut SimWorld, seats: u8, given: &[(u8, Command)]) -> Snapshot {
    let mut input = TickInput::default();
    for seat in 0..seats {
        let command = given
            .iter()
            .find(|(of, _)| *of == seat)
            .map_or(Command::default(), |(_, command)| *command);
        input.insert(Seat(seat), command);
    }
    let tick = world.tick() + 1;
    crate::step(world, tick, &input, 8, crate::StepReason::Authority)
}

impl Range {
    fn now(&self) -> u64 {
        self.world
            .world
            .resource::<super::super::super::Clock>()
            .time_ms
    }

    fn id(&mut self, tag: &str) -> Result<SimId, String> {
        let level = self
            .world
            .world
            .resource::<super::super::super::Static>()
            .0
            .clone();
        self.world
            .snapshot()
            .entities
            .iter()
            .find(|entity| match entity.kind {
                Kind::Object(ObjectKind::Creature, index) => level
                    .placed()
                    .get(index as usize)
                    .is_some_and(|facts| facts.tag == tag),
                _ => false,
            })
            .map(|entity| entity.id)
            .ok_or(format!("no {tag}"))
    }

    /// A place `ahead` metres along the axis from seat 0 and `side` across.
    fn spot(&self, ahead: f32, side: f32) -> [f32; 3] {
        let across = [-self.axis[1], self.axis[0], 0.0];
        math::add(
            self.origin,
            math::add(math::scale(self.axis, ahead), math::scale(across, side)),
        )
    }

    /// `id` standing still at `at` (m).
    fn put(&mut self, id: SimId, at: [f32; 3]) {
        let units = math::scale(at, 1.0 / UNIT);
        poke(
            &mut self.world,
            id,
            move |_: &mut Fighter, motion: &mut MotionState| {
                motion.origin = units;
                motion.velocity = [0.0; 3];
            },
        );
    }

    /// `id` fighting seat 0 (struck a moment ago, for a minute).
    fn fights(&mut self, id: SimId) {
        let (leader, now) = (self.leader, self.now());
        poke(
            &mut self.world,
            id,
            move |fighter: &mut Fighter, _: &mut MotionState| {
                fighter.target = Some(leader);
                fighter.combat_until = now + 60_000;
            },
        );
    }

    fn edit(&mut self, id: SimId, edit: impl Fn(&mut Fighter) + 'static) {
        poke(
            &mut self.world,
            id,
            move |fighter: &mut Fighter, _: &mut MotionState| edit(fighter),
        );
    }

    fn fighter(&mut self, id: SimId) -> Option<Fighter> {
        let mut query = self.world.world.query::<(&Id, &Fighting)>();
        query
            .iter(&self.world.world)
            .find(|(own, _)| own.0 == id)
            .map(|(_, fighting)| fighting.0)
    }

    fn motion(&mut self, id: SimId) -> Option<MotionState> {
        let mut query = self.world.world.query::<(&Id, &Player)>();
        query
            .iter(&self.world.world)
            .find(|(own, _)| own.0 == id)
            .map(|(_, player)| player.0)
    }

    fn hp(&mut self, id: SimId) -> i32 {
        self.fighter(id).map_or(i32::MIN, |fighter| fighter.hp)
    }

    /// Seat `seat`'s droid at `at` (m), still, its kit ready.
    fn droid(&mut self, seat: u8, at: [f32; 3]) -> Result<(), String> {
        let mut remotes = self.world.world.resource_mut::<Remotes>();
        let remote = (remotes.0.iter_mut())
            .find(|remote| remote.seat == Seat(seat))
            .ok_or(format!("seat {seat} has no droid"))?;
        remote.at = at;
        remote.velocity = [0.0; 3];
        remote.ready_ms = [0; 3];
        Ok(())
    }

    fn ready(&mut self, seat: u8) -> [u64; 3] {
        (self.world.world.resource::<Remotes>().0.iter())
            .find(|remote| remote.seat == Seat(seat))
            .map_or([0; 3], |remote| remote.ready_ms)
    }

    /// Seat `seat` pressing `button` with its droid at `from` looking at
    /// `id`'s chest: one step, `seats` seats.
    fn fire(
        &mut self,
        seats: u8,
        seat: u8,
        from: [f32; 3],
        button: u16,
        at: SimId,
    ) -> Result<Snapshot, String> {
        self.droid(seat, from)?;
        let chest = self
            .motion(at)
            .map(|motion| math::scale(crate::combat::mind::chest(&motion), UNIT));
        let (pitch, yaw) = crate::combat::mind::aim(from, chest.ok_or("no target")?);
        let command = Command {
            angles: crate::moves::view_angles(pitch, yaw),
            buttons: button,
            ..Command::default()
        };
        Ok(step(&mut self.world, seats, &[(seat, command)]))
    }

    fn xp(&self) -> i32 {
        (self.world.script_state().xp)
            .get(&crate::inventory::PARTY)
            .copied()
            .unwrap_or(0)
    }
}

/// The droid hovering 1.5 m over seat 0, a step behind it.
fn over(range: &Range) -> [f32; 3] {
    math::add(range.spot(-1.0, 0.0), [0.0, 0.0, 1.5])
}

/// A bolt at each of a fair foe, a neutral, a Min1HP duelist and the
/// ground's door: only the foe is hurt, and its last attacker is seat 0's
/// body.
fn bolt_fair() -> Result<String, String> {
    let mut seen = Vec::new();
    for (tag, fighting, wanted) in [
        (FOE, true, true),
        (NEUTRAL, true, false),
        (DUELIST, true, false),
        (FOE, false, false),
    ] {
        let mut range = range(2)?;
        let id = range.id(tag)?;
        let spot = range.spot(3.0, 0.0);
        range.put(id, spot);
        if fighting {
            range.fights(id);
        }
        let before = range.hp(id);
        let from = over(&range);
        range.fire(2, 1, from, buttons::ATTACK, id)?;
        let after = range.hp(id);
        let by = range
            .fighter(id)
            .and_then(|fighter| fighter.last_attacker)
            .map(|(by, _)| by);
        let hurt = after < before;
        let ok = hurt == wanted && (!wanted || by == Some(range.leader));
        seen.push((tag, fighting, before, after, by, ok));
    }
    let wrong: Vec<_> = seen.iter().filter(|row| !row.5).collect();
    check(
        wrong.is_empty(),
        format!("(target, fighting, hp before, after, last attacker, ok): {seen:?}"),
    )
}

/// A droid's killing bolt gives the party the XP a leader's kill of the
/// same creature does (a death's worth, `scripts::kill_xp`, whoever
/// killed: none on the bare ground, which has no XP table).
fn bolt_xp() -> Result<String, String> {
    let killed = |by_droid: bool| -> Result<(i32, bool), String> {
        let mut range = range(2)?;
        let id = range.id(FOE)?;
        let spot = range.spot(3.0, 0.0);
        range.put(id, spot);
        range.fights(id);
        range.edit(id, |fighter| fighter.hp = 100);
        let before = range.xp();
        if by_droid {
            let from = over(&range);
            range.fire(2, 1, from, buttons::ATTACK, id)?;
        } else {
            let leader = range.leader;
            range
                .world
                .world
                .resource_mut::<super::super::super::Hits>()
                .0
                .push(super::super::super::Hit {
                    attacker: leader,
                    target: id,
                    source: super::super::kit::bolt_source(),
                    point: None,
                    direction: None,
                });
            step(&mut range.world, 2, &[]);
        }
        for _ in 0..20 {
            step(&mut range.world, 2, &[]);
        }
        let dead = range.fighter(id).is_none_or(|fighter| !fighter.alive());
        Ok((range.xp() - before, dead))
    };
    let (droid, leader) = (killed(true)?, killed(false)?);
    check(
        droid.1 && leader.1 && droid.0 == leader.0,
        format!("XP (gained, killed): by the droid {droid:?}, by the leader {leader:?}"),
    )
}

/// A blast at a medium foe, a big foe and the duelist side by side 3 m
/// ahead, with a second foe behind the droid: the two foes in the cone are
/// thrown, the medium one knocked down and the big one not; the duelist
/// and the one behind stay put.
fn sonic() -> Result<String, String> {
    let mut range = range(2)?;
    let [foe, big, duelist, behind] = [FOE, BIG, DUELIST, SECOND].map(|tag| range.id(tag));
    let (foe, big, duelist, behind) = (foe?, big?, duelist?, behind?);
    for (id, at) in [
        (foe, range.spot(3.0, 0.0)),
        (big, range.spot(3.0, 1.0)),
        (duelist, range.spot(3.0, -1.0)),
        (behind, range.spot(-4.0, 0.0)),
    ] {
        range.put(id, at);
        range.fights(id);
    }
    let from = over(&range);
    range.fire(2, 1, from, buttons::ALT_ATTACK, foe)?;
    let moving = |range: &mut Range, id| {
        range
            .motion(id)
            .map_or(0.0, |motion| math::length(motion.velocity) * UNIT)
    };
    let floored = |range: &mut Range, id| {
        range
            .fighter(id)
            .is_some_and(|fighter| fighter.saber.floored())
    };
    let seen =
        [foe, big, duelist, behind].map(|id| (moving(&mut range, id), floored(&mut range, id)));
    let thrown = |(speed, _): (f32, bool)| speed > 1.0;
    check(
        thrown(seen[0])
            && seen[0].1
            && thrown(seen[1])
            && !seen[1].1
            && !thrown(seen[2])
            && !seen[2].1
            && !thrown(seen[3]),
        format!(
            "(speed m/s, down) foe {:?}, big {:?}, duelist {:?}, behind {:?}",
            seen[0], seen[1], seen[2], seen[3]
        ),
    )
}

/// The dart at the party's second: downed, it gets up with its share of
/// its most HP and the revive's wait is set; hurt, it gains its share and
/// the heal's wait is set.
fn dart() -> Result<String, String> {
    let mut range = range(2)?;
    let mate = range.id(MATE)?;
    let max = i32::from(Sheet::commoner().max_hp) * 100;
    let now = range.now();
    range.edit(mate, move |fighter| fighter.fall(now));
    let from = over(&range);
    range.fire(2, 1, from, buttons::MEDPAC, mate)?;
    let revived = range
        .fighter(mate)
        .map(|fighter| (fighter.alive(), fighter.hp));
    let after_revive = range.ready(1);
    let half = max / 2;
    let mut healer = self::range(2)?;
    healer.edit(mate, move |fighter| fighter.hp = half);
    let from = over(&healer);
    healer.fire(2, 1, from, buttons::MEDPAC, mate)?;
    let healed = healer.hp(mate);
    let after_heal = healer.ready(1);
    let rise = max * super::super::kit::REVIVE_PERMILLE / 1000;
    let gain = max * super::super::kit::HEAL_PERMILLE / 1000;
    check(
        revived == Some((true, rise))
            && after_revive[crate::remote::KIT_DART] > now
            && healed == half + gain
            && after_heal[crate::remote::KIT_DART] > now,
        format!(
            "revived (up, hp) {revived:?} (wanted {rise}), ready {after_revive:?}; healed {half} to {healed} (wanted {}), ready {after_heal:?}",
            half + gain
        ),
    )
}

/// Seat 0 fighting the Min1HP duelist: the dart does nothing and its wait
/// is not spent.
fn dart_duel() -> Result<String, String> {
    let mut range = range(2)?;
    let (leader, duelist) = (range.leader, range.id(DUELIST)?);
    let spot = range.spot(2.0, 0.0);
    range.put(duelist, spot);
    range.fights(duelist);
    let hurt = range.hp(leader) / 2;
    range.edit(leader, move |fighter| fighter.hp = hurt);
    let from = over(&range);
    range.fire(2, 1, from, buttons::MEDPAC, leader)?;
    let (hp, ready) = (range.hp(leader), range.ready(1));
    check(
        hp <= hurt && ready[crate::remote::KIT_DART] == 0,
        format!("hp {hurt} then {hp}, ready {ready:?}"),
    )
}

/// The whole party down is lost the same step; a dart after it revives no
/// one.
fn dart_lost() -> Result<String, String> {
    let mut range = range(2)?;
    let (leader, mate) = (range.leader, range.id(MATE)?);
    let now = range.now();
    for id in [leader, mate] {
        range.edit(id, move |fighter| fighter.fall(now));
    }
    let lost = step(&mut range.world, 2, &[]).lost.is_some();
    let from = over(&range);
    range.fire(2, 1, from, buttons::MEDPAC, mate)?;
    let up = [leader, mate].map(|id| range.fighter(id).is_some_and(|fighter| fighter.alive()));
    check(
        lost && up == [false, false],
        format!("lost {lost}, up again after a dart (leader, second) {up:?}"),
    )
}

/// Seats 1 and 2 dart the downed second in one step: one revive, seat
/// 1's, and only seat 1's wait spent.
fn two_darts() -> Result<String, String> {
    let mut range = range(3)?;
    let leader = range.id(MATE)?;
    let now = range.now();
    range.edit(leader, move |fighter| fighter.fall(now));
    let (one, two) = (over(&range), math::add(over(&range), [0.0, 0.0, 0.5]));
    range.droid(1, one)?;
    range.droid(2, two)?;
    let chest = range
        .motion(leader)
        .map(|motion| math::scale(crate::combat::mind::chest(&motion), UNIT))
        .ok_or("no leader")?;
    let aimed = |from| {
        let (pitch, yaw) = crate::combat::mind::aim(from, chest);
        Command {
            angles: crate::moves::view_angles(pitch, yaw),
            buttons: buttons::MEDPAC,
            ..Command::default()
        }
    };
    step(&mut range.world, 3, &[(1, aimed(one)), (2, aimed(two))]);
    let hp = range
        .fighter(leader)
        .map(|fighter| (fighter.alive(), fighter.hp));
    let (first, second) = (range.ready(1), range.ready(2));
    let rise = i32::from(Sheet::commoner().max_hp) * super::super::kit::REVIVE_PERMILLE / 10;
    check(
        hp == Some((true, rise))
            && first[crate::remote::KIT_DART] > now
            && second[crate::remote::KIT_DART] == 0,
        format!("the second (up, hp) {hp:?}, seat 1 ready {first:?}, seat 2 ready {second:?}"),
    )
}

/// Each of the kit's buttons fired at a fair foe and a hurt leader: the
/// kit leaves both random streams as it found them.
fn no_dice() -> Result<String, String> {
    let mut range = range(2)?;
    let (leader, foe) = (range.leader, range.id(FOE)?);
    let spot = range.spot(3.0, 0.0);
    range.put(foe, spot);
    range.fights(foe);
    range.edit(leader, |fighter| fighter.hp /= 2);
    let from = over(&range);
    range.droid(1, from)?;
    let mut seen = Vec::new();
    for button in [buttons::ATTACK, buttons::ALT_ATTACK, buttons::MEDPAC] {
        let mut input = TickInput::default();
        input.insert(Seat(0), Command::default());
        input.insert(
            Seat(1),
            Command {
                buttons: button,
                ..Command::default()
            },
        );
        range
            .world
            .world
            .resource_mut::<super::super::super::Inputs>()
            .0 = input;
        let streams = |world: &SimWorld| {
            let streams = world.world.resource::<Streams>();
            (streams.shared, streams.authority)
        };
        let before = streams(&range.world);
        let ready = range.ready(1);
        kit::kit(&mut range.world.world);
        let fired = range.ready(1) != ready;
        seen.push((button, fired, streams(&range.world) == before));
    }
    check(
        seen.iter().all(|(_, fired, same)| *fired && *same),
        format!("(button, fired, streams unchanged): {seen:?}"),
    )
}
