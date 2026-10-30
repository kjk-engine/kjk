//! The probes of `falls` (`kjk sim probes`).

use super::super::{Fighting, Hit, Hits, Id, Object, Player, PlayerSheet, SimWorld};
use super::*;
use crate::action::Action;
use crate::combat::Sheet;
use crate::combat::ai::Mind;
use crate::combat::damage::Source;
use crate::combat::fighter::{Fallen, Fighter, Life, Revive};
use crate::combat::sheet::factions;
use crate::input::{Command, Seat, TickInput};
use crate::module::{ObjectData, ObjectKind, Placement, Repute};
use crate::script::effects::{Applied, Effect};
use crate::snapshot::{Kind, SimId, Snapshot};
use crate::wire::{Reader, Writer};
use crate::{StepReason, WorldConfig};
use script_ncs::vm::ObjectId;

/// The falling probes (`kjk sim probes`; proposed approved test
/// `downed_and_revive`), each built on the proving ground.
pub fn probes() -> Vec<(&'static str, Result<String, String>)> {
    CASES.iter().map(|(name, case)| (*name, case())).collect()
}

type Case = fn() -> Result<String, String>;

const CASES: &[(&str, Case)] = &[
    ("downed_keeps_its_bytes", kept),
    ("party_member_downed", downed),
    ("death_on_the_downed", death),
    ("raised_from_death", raised_from_death),
    ("fall_is_no_death", fall_is_no_death),
    ("get_up_at_combat_end", get_up),
    ("whole_party_down", whole_party_down),
    ("lost_as_the_fight_ends", lost_as_the_fight_ends),
    ("arrives_standing", arrives_standing),
    ("left_party_gets_up", left_party_gets_up),
    ("revive_by_use", revive_by_use),
    ("use_alone_no_revive", use_alone_no_revive),
    ("revive_interrupted", revive_interrupted),
    ("two_revivers", two_revivers),
    ("revive_chosen_body", revive_chosen_body),
    ("revive_targets", revive_targets),
    ("revive_chosen", revive_chosen),
    ("revive_chosen_broken", revive_chosen_broken),
    ("free_play_poison_death", free_play_poison_death),
];

/// A downed fighter, and one being revived, read back as written.
fn kept() -> Result<String, String> {
    let mut down = Fighter::of(&Sheet::dev_soldier());
    down.fall(1234);
    let reviving = Fighter {
        fallen: Some(Fallen {
            since_ms: 1234,
            revive: Some(Revive {
                by: SimId(7),
                from_ms: 2000,
                needs_ms: REVIVE_MS,
            }),
        }),
        ..down
    };
    for (what, before) in [("down", down), ("being revived", reviving)] {
        let mut out = Writer::default();
        before.encode(&mut out);
        let after = Fighter::decode(&mut Reader::new("fighter", &out.0))
            .map_err(|error| format!("{what}: {error}"))?;
        if after != before {
            return Err(format!("{what}: read back as {after:?}"));
        }
    }
    Ok("a downed fighter and its revive read back as written".into())
}

const ALLY: &str = "probe_ally";
const FOE: &str = "probe_foe";
/// Long enough for any probe: the foe never acts.
const STUNNED_MS: u64 = 600_000;

/// Who a probe means.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Who {
    /// Seat 0's avatar.
    Host,
    /// Seat 1's body: its avatar, or the ally it drives.
    Guest,
    /// A companion of seat 0's.
    Ally,
    /// A hostile soldier, of hit points past what the ally's shots take, and stunned
    /// throughout (it never acts); a probe's script hit kills it.
    Foe,
}

/// A probe's world, stepped by hand. On the proving ground's module:
/// - the ally 2 m beside the entry, the foe 6 m off, aware of the host;
/// - two containers 0.8 m east and north of the entry (what a revive's use
///   must not open);
/// - `seats` players.
///
/// With `driven`, seat 1 has no avatar and drives the ally (`Mind::driver`).
struct Run {
    world: SimWorld,
    last: Snapshot,
    seats: u8,
    host: SimId,
    guest: Option<SimId>,
    ally: SimId,
    foe: SimId,
    /// The actions the next step sends.
    acts: Vec<(Seat, Action)>,
}

impl Run {
    fn new(seats: u8, driven: bool) -> Result<Run, String> {
        let ground = crate::determinism::proving_ground();
        let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
        let entry = data.entry.position;
        let at = |dx: f32, dy: f32| [entry[0] + dx, entry[1] + dy, entry[2]];
        let soldier = Sheet::dev_soldier();
        // Not plot: one that cannot fall is no fight (`party::in_combat`).
        let foe = Sheet {
            faction: factions::HOSTILE_1,
            max_hp: u16::MAX,
            ..soldier
        };
        let creature = |tag: &str, at: [f32; 3], sheet: Sheet| ObjectData {
            sheet: Some(sheet),
            ..ObjectData::bare(ObjectKind::Creature, tag, at)
        };
        let container = |tag: &str, at: [f32; 3]| super::super::containers::container(tag, at, &[]);
        data.placements.extend(
            [
                creature(ALLY, at(0.0, -2.0), soldier),
                creature(FOE, at(6.0, 0.0), foe),
                container("east", at(0.8, 0.0)),
                container("north", at(0.0, 0.8)),
            ]
            .map(Placement::Own),
        );
        // The player's faction (row 0) and the foe's (1) hostile.
        data.repute = Repute {
            rows: vec![vec![100, 0], vec![0, 100]],
        };
        let level = ground.with_module(data).with_scripts(library());
        let config = WorldConfig {
            seats,
            ..WorldConfig::one_seat()
        };
        let mut world = SimWorld::new(config, level.clone()).map_err(|e| e.to_string())?;
        let tagged = |world: &mut SimWorld, tag: &str| {
            let mut objects = world.world.query::<(&Id, &Object)>();
            objects
                .iter(&world.world)
                .find(|(_, object)| object.facts.as_ref().is_some_and(|facts| facts.tag == tag))
                .map(|(id, _)| id.0)
                .ok_or(format!("no {tag}"))
        };
        let (ally, foe) = (tagged(&mut world, ALLY)?, tagged(&mut world, FOE)?);
        let mut start = world.snapshot();
        let host = start.body(Seat(0)).ok_or("no host")?.id;
        // A driven ally is seat 1's body in place of its avatar.
        if driven {
            start
                .entities
                .retain(|entity| entity.kind != Kind::Avatar(Seat(1)));
        }
        let guest = start.body(Seat(1)).map(|body| body.id);
        for entity in &mut start.entities {
            let Some(fighter) = entity.fighter.as_mut() else {
                continue;
            };
            if entity.id == ally {
                fighter.mind = Mind {
                    driver: driven.then_some(Seat(1)),
                    ..Mind::companion(Seat(0))
                };
            } else if entity.id == foe {
                fighter.force.hold(0, STUNNED_MS);
                fighter.mind.aware_at = 0;
                fighter.target = Some(host);
            }
        }
        let world = SimWorld::from_snapshot(&start, level).map_err(|e| e.to_string())?;
        let guest = if driven { Some(ally) } else { guest };
        Ok(Run {
            world,
            last: start,
            seats,
            host,
            guest,
            ally,
            foe,
            acts: Vec::new(),
        })
    }

    fn id(&self, who: Who) -> SimId {
        match who {
            Who::Host => self.host,
            Who::Guest => self.guest.expect("a probe with a guest"),
            Who::Ally => self.ally,
            Who::Foe => self.foe,
        }
    }

    /// One 8 ms tick; each seat sends its command in `given`, else an idle
    /// one.
    fn step(&mut self, given: &[(Seat, Command)]) {
        let mut input = TickInput::default();
        for seat in (0..self.seats).map(Seat) {
            let command = given
                .iter()
                .find(|(s, _)| *s == seat)
                .map_or_else(Command::default, |(_, command)| *command);
            input.insert(seat, command);
        }
        for (seat, action) in std::mem::take(&mut self.acts) {
            input.act(seat, action);
        }
        let tick = self.last.tick + 1;
        self.last = crate::step(&mut self.world, tick, &input, 8, StepReason::Authority);
    }

    /// `ms` of steps with the same commands.
    fn hold(&mut self, ms: u64, given: &[(Seat, Command)]) {
        let end = self.last.time_ms + ms;
        while self.last.time_ms < end {
            self.step(given);
        }
    }

    /// A script's `damage` points on `who` from the foe, landing next step.
    fn hit(&mut self, who: Who, damage: u16) {
        self.blow(who, damage, false);
    }

    /// `hit`, a blow the story stages or not (`Source::Script`'s `staged`).
    fn blow(&mut self, who: Who, damage: u16, staged: bool) {
        let hit = Hit {
            attacker: self.foe,
            target: self.id(who),
            source: Source::Script {
                damage,
                damage_type: None,
                cast: false,
                shot: None,
                staged,
            },
            point: None,
            direction: None,
        };
        self.world.world.resource_mut::<Hits>().0.push(hit);
    }

    /// `who` stunned from now, as long as a stun lasts (`ja::STUN_MS`).
    fn stun(&mut self, who: Who) {
        let (id, now) = (self.id(who), self.last.time_ms);
        let mut fighters = self.world.world.query::<(&Id, &mut Fighting)>();
        for (own, mut fighting) in fighters.iter_mut(&mut self.world.world) {
            if own.0 == id {
                fighting.0.force.hold(now, u64::MAX);
            }
        }
    }

    /// `who` at 1 HP, poisoned by the foe with a dose due now that kills it.
    fn poison(&mut self, who: Who) {
        let (id, now, by) = (self.id(who), self.last.time_ms, self.foe);
        let mut fighters = self.world.world.query::<(&Id, &mut Fighting)>();
        for (own, mut fighting) in fighters.iter_mut(&mut self.world.world) {
            if own.0 == id {
                fighting.0.hp = 100;
                fighting.0.pack.poison = Some(crate::combat::items::Poisoned {
                    by,
                    course: crate::combat::items::Course {
                        points: 10,
                        every_ms: 1000,
                        for_ms: 1000,
                        ..Default::default()
                    },
                    next: now,
                    until: now,
                    link: None,
                });
            }
        }
    }

    /// `effect` on `who` as a script of the host's applied it this step.
    fn effect(&mut self, who: Who, effect: Effect) {
        let applied = Applied {
            target: ObjectId(self.id(who).0),
            by: ObjectId(self.host.0),
            effect,
            until: None,
            spell: 0,
            id: 0,
            at: None,
        };
        let mut scripts = self
            .world
            .world
            .resource_mut::<super::super::scripts::Scripts>();
        scripts.state.applied.push(applied);
    }

    fn fighter(&self, who: Who) -> Fighter {
        let id = self.id(who);
        self.last
            .get(id)
            .and_then(|entity| entity.fighter)
            .expect("a character")
    }

    /// Every case's first look: its life and hit points.
    fn life(&self, who: Who) -> (Life, i32) {
        let fighter = self.fighter(who);
        (fighter.life, fighter.hp)
    }
}

/// A script library with nothing to run, so the scripts' events are raised
/// (a level without scripts raises none).
fn library() -> crate::script::Library {
    crate::probe::library("// 0\nint Random(int nMaxInteger);\n", [] as [(&str, _); 0])
}

/// Each party member at 0 HP is downed, a non-party creature dies.
fn downed() -> Result<String, String> {
    let cases = [
        ("the host", false, Who::Host, Life::Downed),
        ("a guest's avatar", false, Who::Guest, Life::Downed),
        ("a companion", false, Who::Ally, Life::Downed),
        ("a companion a guest drives", true, Who::Ally, Life::Downed),
        ("a hostile creature", false, Who::Foe, Life::Dead),
    ];
    for (what, driven, who, wanted) in cases {
        let mut run = Run::new(2, driven)?;
        run.step(&[]);
        run.hit(who, u16::MAX);
        run.step(&[]);
        let fighter = run.fighter(who);
        let since = fighter.fallen.map(|fallen| fallen.since_ms);
        let lying = run
            .last
            .get(run.id(who))
            .and_then(|entity| entity.player)
            .is_some_and(|ps| ps.flags & crate::motion::motion_flags::DEAD != 0);
        let downed_right =
            wanted != Life::Downed || (fighter.hp == 0 && since == Some(run.last.time_ms));
        if fighter.life != wanted || !lying || !downed_right {
            return Err(format!(
                "{what}: {:?} at {} HP/100, fell at {since:?}, lying {lying}",
                fighter.life, fighter.hp
            ));
        }
    }
    Ok("the host, a guest, a companion, driven or not: downed at 0 HP, lying; a hostile creature dead".into())
}

/// EffectDeath kills a standing companion and a downed one (P13: the
/// story's deaths); the downed one only by it.
fn death() -> Result<String, String> {
    for down_first in [false, true] {
        let mut run = Run::new(2, false)?;
        run.step(&[]);
        if down_first {
            run.hit(Who::Ally, 999);
            run.step(&[]);
            run.hit(Who::Ally, 999);
            run.step(&[]);
            if run.life(Who::Ally).0 != Life::Downed {
                return Err(format!(
                    "a downed companion hit again: {:?}",
                    run.life(Who::Ally)
                ));
            }
        }
        run.effect(Who::Ally, Effect::Death);
        run.step(&[]);
        if run.life(Who::Ally).0 != Life::Dead {
            return Err(format!(
                "down first {down_first}: {:?}",
                run.life(Who::Ally)
            ));
        }
    }
    Ok("EffectDeath kills a companion standing or downed; a hit does not kill the downed".into())
}

/// EffectResurrection raises the dead, a companion killed by EffectDeath
/// and a hostile creature killed by a blow: standing again, its hit points
/// `RISE_HP`, no longer lying (TSL's academy raises Bao-Dur, killed where
/// he lies in his cell, as the cells open).
fn raised_from_death() -> Result<String, String> {
    for who in [Who::Ally, Who::Foe] {
        let mut run = Run::new(2, false)?;
        run.step(&[]);
        match who {
            Who::Ally => run.effect(who, Effect::Death),
            _ => run.hit(who, u16::MAX),
        }
        run.step(&[]);
        let dead = run.life(who).0;
        run.effect(who, Effect::Resurrection);
        run.step(&[]);
        let (life, hp) = run.life(who);
        let lying = run
            .last
            .get(run.id(who))
            .and_then(|entity| entity.player)
            .is_some_and(|ps| ps.flags & crate::motion::motion_flags::DEAD != 0);
        if dead != Life::Dead || life != Life::Alive || hp < RISE_HP || lying {
            return Err(format!(
                "{who:?}: {dead:?}, then {life:?} at {hp} HP/100, lying {lying}"
            ));
        }
    }
    Ok(
        "a companion killed by EffectDeath and a creature killed by a blow stand again, RISE_HP"
            .into(),
    )
}

/// Going down raises no death (no killer recorded); dying does.
fn fall_is_no_death() -> Result<String, String> {
    let mut run = Run::new(2, false)?;
    let killer = |run: &Run| {
        run.world
            .world
            .resource::<super::super::scripts::Scripts>()
            .state
            .last
            .get(&(run.ally.0, crate::script::last::KILLER))
            .copied()
    };
    run.step(&[]);
    run.hit(Who::Ally, 999);
    run.hold(100, &[]);
    let after_fall = killer(&run);
    run.effect(Who::Ally, Effect::Death);
    run.hold(100, &[]);
    let after_death = killer(&run);
    if after_fall.is_some() || after_death.is_none() {
        return Err(format!(
            "killer after the fall {after_fall:?}, after the death {after_death:?}"
        ));
    }
    Ok("a fall raises no death; EffectDeath then does".into())
}

/// Downed with the foe standing, the host gets up once the foe is dead and
/// the party out of combat, with `RISE_HP`, no sooner than `DOWN_MIN_MS`.
fn get_up() -> Result<String, String> {
    let mut run = Run::new(2, false)?;
    run.step(&[]);
    run.hit(Who::Host, 999);
    run.hit(Who::Foe, 999);
    run.step(&[]);
    let fell = run.last.time_ms;
    let mut rose = None;
    while run.last.time_ms < fell + 12_000 && rose.is_none() {
        run.step(&[]);
        if run.fighter(Who::Host).alive() {
            rose = Some(run.last.time_ms - fell);
        }
    }
    let hp = run.fighter(Who::Host).hp;
    match rose {
        Some(after) if after >= DOWN_MIN_MS && hp == RISE_HP => Ok(format!(
            "up {after} ms after the fight's end, at {} HP",
            hp / 100
        )),
        _ => Err(format!("rose {rose:?} ms after, at {hp}/100 HP")),
    }
}

/// The whole party down, one by one with the foe standing, is the loss; a
/// guest's use then revives nobody, and the snapshot says it through its
/// bytes.
fn whole_party_down() -> Result<String, String> {
    let mut run = Run::new(2, false)?;
    run.step(&[]);
    let mut lost_after = Vec::new();
    for who in [Who::Ally, Who::Guest, Who::Host] {
        run.hit(who, 999);
        run.step(&[]);
        lost_after.push(run.last.lost.is_some());
    }
    let using = Command {
        buttons: crate::moves::USE,
        ..Command::default()
    };
    run.hold(4000, &[(Seat(1), using)]);
    let standing = [Who::Host, Who::Guest, Who::Ally].map(|who| run.fighter(who).alive());
    let decoded = Snapshot::decode(&run.last.encode())
        .ok()
        .map(|again| again.lost);
    if lost_after != [false, false, true]
        || standing.contains(&true)
        || decoded != Some(run.last.lost)
    {
        return Err(format!(
            "lost after each fall {lost_after:?}, standing {standing:?}, decoded {decoded:?}"
        ));
    }
    Ok("lost only when the last stands no more; nobody gets up after; `lost` read back".into())
}

/// The party down in the step the foe dies: lost, and nobody gets up.
fn lost_as_the_fight_ends() -> Result<String, String> {
    let mut run = Run::new(2, false)?;
    run.step(&[]);
    for who in [Who::Host, Who::Guest, Who::Ally, Who::Foe] {
        run.hit(who, 999);
    }
    run.step(&[]);
    run.hold(12_000, &[]);
    let lives = [Who::Host, Who::Guest, Who::Ally].map(|who| run.life(who).0);
    if run.last.lost.is_none() || lives.contains(&Life::Alive) {
        return Err(format!("lost {:?}, lives {lives:?}", run.last.lost));
    }
    Ok("lost; out of combat nobody gets up".into())
}

/// A module installed while the host and the ally are down: both arrive
/// standing with `RISE_HP`.
fn arrives_standing() -> Result<String, String> {
    let mut run = Run::new(1, false)?;
    run.step(&[]);
    run.hit(Who::Host, 999);
    run.hit(Who::Ally, 999);
    run.step(&[]);
    let level = run.world.level().clone();
    run.last = run.world.install(level, None).map_err(|e| e.to_string())?;
    let lives = [Who::Host, Who::Ally].map(|who| run.life(who));
    if lives != [(Life::Alive, RISE_HP); 2] {
        return Err(format!("{lives:?}"));
    }
    Ok("arrived standing at 1 HP".into())
}

/// A downed companion that leaves the party gets up at once, at
/// `RISE_HP`, the fight going on.
fn left_party_gets_up() -> Result<String, String> {
    let mut run = Run::new(1, false)?;
    run.step(&[]);
    run.hit(Who::Ally, 999);
    run.step(&[]);
    let down = run.life(Who::Ally);
    let ally = run.ally;
    let mut fighters = run.world.world.query::<(&Id, &mut Fighting)>();
    for (own, mut fighting) in fighters.iter_mut(&mut run.world.world) {
        if own.0 == ally {
            fighting.0.mind.leader = None;
        }
    }
    run.step(&[]);
    let after = run.life(Who::Ally);
    if down.0 != Life::Downed || after != (Life::Alive, RISE_HP) {
        return Err(format!("down {down:?}, out of the party {after:?}"));
    }
    Ok("out of the party, up at once at 1 HP".into())
}

impl Run {
    /// Puts character `id` at `at` (metres, at its height now), looking
    /// `yaw` as the commands that follow have it.
    fn place(&mut self, id: SimId, at: [f32; 2], yaw: f32) {
        let mut players = self.world.world.query::<(&Id, &mut Player)>();
        for (own, mut player) in players.iter_mut(&mut self.world.world) {
            if own.0 == id {
                player.0.origin[0] = at[0] / collision::UNIT;
                player.0.origin[1] = at[1] / collision::UNIT;
                player.0.view_offset = [0; 3];
                player.0.view = [0.0, yaw, 0.0];
            }
        }
    }

    /// The host's place across the floor (metres).
    fn host_at(&self) -> [f32; 2] {
        let entity = self.last.get(self.host).expect("the host");
        [entity.position[0], entity.position[1]]
    }

    /// The host downed, `who` placed `far` metres off it along `yaw`'s
    /// back, facing it, with `rank` in Treat Injury.
    fn reviving_setup(&mut self, who: Who, yaw: f32, rank: u8, far: f32) {
        self.step(&[]);
        self.hit(Who::Host, 999);
        self.step(&[]);
        let [x, y] = self.host_at();
        let (sin, cos) = crate::math::sin_cos_deg(yaw);
        let id = self.id(who);
        self.place(id, [x - far * cos, y - far * sin], yaw);
        let mut sheets = self.world.world.query::<(&Id, &mut PlayerSheet)>();
        for (own, mut sheet) in sheets.iter_mut(&mut self.world.world) {
            if own.0 == id {
                sheet.0.skills[crate::combat::sheet::skills::TREAT_INJURY] = rank;
            }
        }
    }
}

/// `--moves`' `use` held, looking `yaw` and a little down.
fn using(yaw: f32, also: u16) -> Command {
    Command {
        angles: crate::moves::view_angles(20.0, yaw),
        buttons: crate::moves::USE | also,
        ..Command::default()
    }
}

/// The guest revives the downed host in `revive_ms(rank)`, in combat (the
/// host's own fall keeps the party so): still down a tick before, standing
/// at `RISE_HP` after; by `--moves`' `use` from its avatar at three ranks
/// and from a companion it drives, and by `USE_HELD` alone (the target
/// block's hold).
fn revive_by_use() -> Result<String, String> {
    use crate::buttons::USE_HELD;
    use crate::moves::USE;
    let cases = [
        (false, 0u8, USE),
        (false, 10, USE),
        (false, 25, USE),
        (true, 0, USE),
        (false, 0, USE_HELD),
    ];
    let mut seen = Vec::new();
    for (driven, rank, held) in cases {
        let mut run = Run::new(2, driven)?;
        run.reviving_setup(Who::Guest, 0.0, rank, 1.5);
        let needs = u64::from(revive_ms(rank));
        let command = [(
            Seat(1),
            Command {
                buttons: held,
                ..using(0.0, 0)
            },
        )];
        run.hold(needs - 8, &command);
        let before = run.fighter(Who::Host);
        let by = before
            .fallen
            .and_then(|fallen| fallen.revive)
            .map(|revive| revive.by);
        run.hold(16, &command);
        let after = run.life(Who::Host);
        if before.life != Life::Downed
            || by != Some(run.id(Who::Guest))
            || after != (Life::Alive, RISE_HP)
        {
            return Err(format!(
                "driven {driven}, rank {rank}: before {:?} by {by:?}, after {after:?}",
                before.life
            ));
        }
        let driven = if driven { " (driven)" } else { "" };
        let hold = if held == USE_HELD {
            " (the block's hold)"
        } else {
            ""
        };
        seen.push(format!("rank {rank}{driven}{hold}: {needs} ms"));
    }
    Ok(seen.join(", "))
}

/// A player's held use alone (`USE`, no `USE_HELD`) beside the downed host,
/// past when a revive would have ended: no revive begins, the host stays
/// down.
fn use_alone_no_revive() -> Result<String, String> {
    let mut run = Run::new(2, false)?;
    run.reviving_setup(Who::Guest, 0.0, 0, 1.5);
    let held = Command {
        buttons: crate::buttons::USE,
        ..using(0.0, 0)
    };
    let mut begun = false;
    let end = run.last.time_ms + u64::from(revive_ms(0)) + 500;
    while run.last.time_ms < end {
        run.step(&[(Seat(1), held)]);
        begun |= run
            .fighter(Who::Host)
            .fallen
            .and_then(|f| f.revive)
            .is_some();
    }
    let life = run.life(Who::Host).0;
    if begun || life != Life::Downed {
        return Err(format!("revive begun {begun}, the host {life:?}"));
    }
    Ok("the use bit held alone revives nobody".into())
}

/// What else befalls the reviver as it breaks off.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Befalls {
    Nothing,
    /// A hit: the guest still holding starts over.
    Hit,
    /// A stun (`ja::STUN_MS`, longer than the probe waits): it holds
    /// nothing meanwhile.
    Stun,
}

/// What breaks a revive off 1 s in: moving, attacking, a hit, a stun,
/// letting go. 2 s later (past when the unbroken revive would have ended)
/// the host is still down; after a hit, the guest still holding has
/// started over.
fn revive_interrupted() -> Result<String, String> {
    let moving = Command {
        forward: 127,
        ..using(0.0, 0)
    };
    let breaks: [(&str, Command, Befalls); 5] = [
        ("moving", moving, Befalls::Nothing),
        (
            "attacking",
            using(0.0, crate::buttons::ATTACK),
            Befalls::Nothing,
        ),
        ("hurt", using(0.0, 0), Befalls::Hit),
        ("stunned", using(0.0, 0), Befalls::Stun),
        ("letting go", Command::default(), Befalls::Nothing),
    ];
    for (what, then, befalls) in breaks {
        let mut run = Run::new(2, false)?;
        run.reviving_setup(Who::Guest, 0.0, 0, 1.5);
        run.hold(1000, &[(Seat(1), using(0.0, 0))]);
        match befalls {
            Befalls::Hit => run.hit(Who::Guest, 1),
            Befalls::Stun => run.stun(Who::Guest),
            Befalls::Nothing => {}
        }
        let broke_at = run.last.time_ms;
        run.hold(2008, &[(Seat(1), then)]);
        let fighter = run.fighter(Who::Host);
        let from = fighter.fallen.and_then(|f| f.revive).map(|r| r.from_ms);
        let right = if befalls == Befalls::Hit {
            from.is_some_and(|from| from > broke_at)
        } else {
            from.is_none()
        };
        if !fighter.downed() || !right {
            return Err(format!("{what}: {:?}, revive from {from:?}", fighter.life));
        }
    }
    Ok("moving, attacking, a hit, a stun and letting go each break a revive off".into())
}

/// The downed host and, nearer the guest, a downed companion, both ahead:
/// the host chosen in the target block (`Action::Revive`) is revived and
/// the companion left down; with nothing chosen, the nearer is revived.
fn revive_chosen_body() -> Result<String, String> {
    for choose in [true, false] {
        let mut run = Run::new(2, false)?;
        run.reviving_setup(Who::Guest, 0.0, 0, 1.5);
        run.hit(Who::Ally, 999);
        run.step(&[]);
        let [x, y] = run.host_at();
        run.place(run.ally, [x - 0.75, y + 0.3], 0.0);
        if choose {
            run.acts.push((Seat(1), Action::Revive(run.host)));
        }
        let held = Command {
            buttons: crate::buttons::USE_HELD,
            ..using(0.0, 0)
        };
        run.hold(u64::from(revive_ms(0)) + 8, &[(Seat(1), held)]);
        let lives = (run.life(Who::Host).0, run.life(Who::Ally).0);
        let wanted = if choose {
            (Life::Alive, Life::Downed)
        } else {
            (Life::Downed, Life::Alive)
        };
        if lives != wanted {
            return Err(format!("chosen {choose}: host and companion {lives:?}"));
        }
    }
    Ok("the body chosen is revived past a nearer one; with none chosen, the nearest".into())
}

/// Two seats' use on the downed host: seat 1 revives, seat 2 waits; neither
/// opens the container behind it.
fn two_revivers() -> Result<String, String> {
    let mut run = Run::new(3, false)?;
    let third = run.last.body(Seat(2)).ok_or("no seat 2")?.id;
    run.reviving_setup(Who::Guest, 0.0, 0, 1.5);
    let [x, y] = run.host_at();
    // Seat 2 south of the host, looking north (the "north" container).
    run.place(third, [x, y - 1.5], 90.0);
    let both = [(Seat(1), using(0.0, 0)), (Seat(2), using(90.0, 0))];
    run.hold(1000, &both);
    let by = run
        .fighter(Who::Host)
        .fallen
        .and_then(|f| f.revive)
        .map(|r| r.by);
    let opened: u64 = run
        .last
        .entities
        .iter()
        .filter(|e| matches!(e.kind, Kind::Object(ObjectKind::Placeable, _)))
        .filter_map(|e| e.object.map(|object| object.tenants))
        .fold(0, |all, tenants| all | tenants);
    if by != Some(run.id(Who::Guest)) || opened != 0 {
        return Err(format!(
            "revived by {by:?}, containers held open by seats {opened:#b}"
        ));
    }
    Ok("the lower seat revives, the other waits; no container opens".into())
}

/// A downed party member is a target offering Revive: the host's avatar, a
/// companion, a companion a guest drives; standing, none of them is a
/// target, nor is the guest's standing avatar.
fn revive_targets() -> Result<String, String> {
    use crate::target::{Choice, body_choices, candidates, number};
    for driven in [false, true] {
        let mut run = Run::new(2, driven)?;
        run.step(&[]);
        let level = run.world.level().clone();
        let number_of = |run: &Run, id: SimId| run.last.get(id).and_then(number);
        let party = [Who::Host, Who::Guest, Who::Ally].map(|who| number_of(&run, run.id(who)));
        let targets = |run: &Run| -> Vec<(Option<u32>, Vec<Choice>)> {
            candidates(&level, &run.last)
                .iter()
                .filter(|candidate| party.contains(&Some(candidate.index)))
                .map(|candidate| {
                    let body = crate::target::entity(&run.last, candidate.index);
                    (Some(candidate.index), body.map_or(Vec::new(), body_choices))
                })
                .collect()
        };
        let standing = targets(&run);
        run.hit(Who::Host, 999);
        run.hit(Who::Ally, 999);
        run.step(&[]);
        let downed = targets(&run);
        let wanted =
            [Who::Host, Who::Ally].map(|who| (number_of(&run, run.id(who)), vec![Choice::Revive]));
        if !standing.is_empty() || downed != wanted {
            return Err(format!(
                "driven {driven}: standing targets {standing:?}, downed {downed:?}, wanted {wanted:?}"
            ));
        }
    }
    Ok("the downed host and companion (driven or not) offer Revive; the standing party is no target".into())
}

/// How a chosen revive is broken off, `BREAK_MS` after the host shows it
/// under way.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Breaker {
    Moving,
    Hurt,
}

const BREAK_MS: u64 = 1000;
/// How long a chosen revive probe runs (sim ms).
const CHOSEN_MS: u64 = 6000;

/// What a chosen revive came to (sim ms): when the walk arrived, the host's
/// revive began, the hold ended, the host stood.
#[derive(Debug, Default)]
struct Chosen {
    arrived: Option<u64>,
    began: Option<u64>,
    ended: Option<u64>,
    rose: Option<u64>,
}

/// The guest, `far` metres off the downed host (along the proving ground,
/// inside its west half) with `rank` in Treat Injury, chooses Revive on it
/// as the target block does: walks there (`target::Walk`, steered in place
/// of its keys), then asks for the body (`Action::Revive`) and holds
/// (`target::Reviving`); `breaker` breaks it off. Run `CHOSEN_MS` at most:
/// the fight (the foe struck the host) ends some 8 s in, and the host gets
/// up.
fn chosen_revive(rank: u8, far: f32, breaker: Option<Breaker>) -> Result<Chosen, String> {
    use crate::target::{Choice, HOLD_BREAKS, Reviving, Stride, WALK_BREAKS, Walk};
    let mut run = Run::new(2, false)?;
    run.reviving_setup(Who::Guest, crate::determinism::TURN_DEGREES, rank, far);
    // The snapshot the player sees has the guest where it was put.
    run.step(&[]);
    let level = run.world.level().clone();
    let me = run.id(Who::Guest);
    let body = crate::target::AVATARS;
    let start = run.last.time_ms;
    let (mut walk, mut hold, mut chosen) = (None::<Walk>, None::<Reviving>, Chosen::default());
    let mut started = false;
    while run.last.time_ms < start + CHOSEN_MS {
        let now = run.last.time_ms;
        if run.fighter(Who::Host).alive() {
            chosen.rose = Some(now);
            break;
        }
        let revive = run.fighter(Who::Host).fallen.and_then(|f| f.revive);
        if chosen.began.is_none() {
            chosen.began = revive.filter(|r| r.by == me).map(|r| r.from_ms);
        }
        let mine = run.last.get(me).cloned().ok_or("no guest")?;
        let ps = mine.player.ok_or("the guest has no body")?;
        let to = crate::target::candidates(&level, &run.last)
            .iter()
            .find(|candidate| candidate.index == body)
            .map(|candidate| candidate.feet)
            .ok_or("the downed host is no target")?;
        let spot = crate::target::Spot::plain(to, Choice::Revive, ObjectKind::Creature);
        let state = |_: u32| crate::ObjectState::default();
        if !started {
            started = true;
            if Walk::near(&ps, spot) {
                hold = Some(Reviving::new(body, now));
                run.acts.push((Seat(1), Action::Revive(run.host)));
            } else {
                walk = Some(Walk::new(
                    &level,
                    &state,
                    &ps,
                    body,
                    spot,
                    Choice::Revive,
                    now,
                ));
            }
        }
        let mut piloted = None;
        if let Some(going) = walk.as_mut() {
            match going.stride(&level, &state, &ps, spot, now) {
                Stride::Steer(command) => piloted = Some(command),
                Stride::Arrived => {
                    chosen.arrived = Some(now);
                    walk = None;
                    hold = Some(Reviving::new(body, now));
                    run.acts.push((Seat(1), Action::Revive(run.host)));
                }
                Stride::Unreachable => return Err(format!("unreachable at {now}")),
            }
        }
        if let Some(holding) = hold.as_mut() {
            piloted = holding.keep(crate::target::entity(&run.last, body), &mine, now);
            if piloted.is_none() {
                hold = None;
                chosen.ended = Some(now);
            }
        }
        let due = chosen.began.is_some_and(|began| now == began + BREAK_MS);
        let base = Command {
            forward: if due && breaker == Some(Breaker::Moving) {
                127
            } else {
                0
            },
            ..Command::default()
        };
        if due && breaker == Some(Breaker::Hurt) {
            run.hit(Who::Guest, 1);
        }
        let breaks = if hold.is_some() {
            HOLD_BREAKS
        } else {
            WALK_BREAKS
        };
        let (command, cancelled) = crate::target::steer(piloted, base, breaks);
        if cancelled {
            (walk, hold) = (None, None);
            chosen.ended = Some(now);
        }
        run.step(&[(Seat(1), command)]);
    }
    Ok(chosen)
}

/// E on Revive from 3.5 m: the guest walks to the downed host, holds without
/// a key, and the host stands `revive_ms(rank)` after the revive began, at
/// rank 0 and 10; from 1.5 m it holds at once.
fn revive_chosen() -> Result<String, String> {
    let mut seen = Vec::new();
    for (rank, far) in [(0u8, 3.5), (10, 3.5), (0, 1.5)] {
        let chosen = chosen_revive(rank, far, None)?;
        let needs = u64::from(revive_ms(rank));
        let took = chosen
            .rose
            .zip(chosen.began)
            .map(|(rose, began)| rose - began);
        let walked = chosen.arrived.is_some() == (far > 2.0);
        if took.is_none_or(|took| !(needs..=needs + 8).contains(&took))
            || !walked
            || chosen.ended.is_some()
        {
            return Err(format!(
                "rank {rank} from {far} m: {chosen:?}, needs {needs} ms"
            ));
        }
        seen.push(format!(
            "rank {rank} from {far} m: arrived {:?}, up {} ms after it began",
            chosen.arrived,
            took.unwrap_or(0)
        ));
    }
    Ok(seen.join("; "))
}

/// A chosen revive 1 s in: the guest's own forward key ends the hold, as
/// does a hit (the host begins it again; the hold lets go); either way the
/// host is still down when the unbroken revive would have ended, with no
/// revive under way.
fn revive_chosen_broken() -> Result<String, String> {
    for breaker in [Breaker::Moving, Breaker::Hurt] {
        let chosen = chosen_revive(0, 1.5, Some(breaker))?;
        let ended_right = chosen
            .began
            .zip(chosen.ended)
            .is_some_and(|(began, ended)| {
                (began + BREAK_MS..=began + BREAK_MS + 16).contains(&ended)
            });
        if chosen.rose.is_some() || !ended_right {
            return Err(format!("{breaker:?}: {chosen:?}"));
        }
    }
    Ok("the player's own move and a hit each end a chosen revive; the host stays down".into())
}

/// A free-play death by the sim's own damage in a script's shape (a
/// poison's dose) is JA's: no staged dying clip (`death::in_place`); a
/// story's blow (a script's own `EffectDamage`) plays KOTOR's.
fn free_play_poison_death() -> Result<String, String> {
    for staged in [false, true] {
        let mut run = Run::new(1, false)?;
        run.step(&[]);
        if staged {
            run.blow(Who::Foe, u16::MAX, true);
        } else {
            run.poison(Who::Foe);
        }
        run.step(&[]);
        let fighter = run.fighter(Who::Foe);
        let dying = fighter
            .acting
            .is_some_and(|acting| acting.animation == crate::combat::death::DIE);
        if fighter.life != Life::Dead || dying != staged {
            return Err(format!(
                "staged {staged}: {:?}, the dying clip {dying}",
                fighter.life
            ));
        }
    }
    Ok("a poison's dose kills by JA's death; a story's blow by KOTOR's dying clip".into())
}
