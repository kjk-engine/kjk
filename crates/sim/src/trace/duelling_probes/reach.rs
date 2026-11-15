//! The probes of a creature whose target stands off the walkable floor, on a
//! crate (`kjk sim probes --only duelling`): it strikes upward, jumps onto the
//! crate, pushes the target off, switches to its ranged weapon or hides, under
//! both rule sets. The room is a floor with a navigation mesh that has a hole
//! under the crate, whose box is a placeable's drawn shape
//! (`Contents::SHAPE | NONWALK`, owned by an entity), as
//! `motion::prop_step_probes` builds one.

use std::sync::Arc;

use collision::{Contents, LevelBuilder, Tag, UNIT};

use super::super::duels::{Missing, creature_object, duel_anims, duel_module, setup};
use crate::combat::Fighter;
use crate::combat::pace::{Fallback, Reach, Tier, fallback, power_allowed, reach};
use crate::combat::saber::SaberRules;
use crate::combat::sheet::Sheet;
use crate::input::{Command, buttons};
use crate::inventory::{Catalog, ItemFacts, slots};
use crate::math::Vec3;
use crate::module::ObjectKind;
use crate::motion::{Ground, MotionState, SOLE_Z};
use crate::probe::{Checks, check};
use crate::snapshot::{Kind, Snapshot};
use crate::{Level, Seat, SimWorld, StepReason, TickInput, TickRate, WorldConfig};
use rules::ai::duelling::reach as out_of;

/// The entity the crate belongs to: the module's first object, a placeable.
const PROP: u32 = 0;
/// The creature is the module's second object.
const CREATURE: u32 = 1;
/// How many ticks make a second.
const SECOND: usize = 125;
/// The room's floor reaches this far each way (metres), in cells this wide.
const ROOM: f32 = 12.0;
const CELL: f32 = 0.5;
/// The crate stands here (units) and is this wide, half a side.
const CRATE_AT: [f32; 2] = [200.0, 0.0];
/// How far (units) above its perch a target is put, to land on it.
const DROP: f32 = 30.0;
/// The seconds a fight settles for before the target is put on its crate.
const SETTLE: usize = 1;

/// A humanoid with no Force points and no Force jump.
const PLAIN: &str = "fp=0 jump=0";

/// A creature, its target and a room.
#[derive(Clone, Copy)]
struct Scene {
    /// The rules the blades play by (`ja` or `advanced`).
    rules: &'static str,
    /// The creature's fighter kind, and keys that follow it.
    kind: &'static str,
    keys: &'static str,
    /// It is a creature model that fights with its own strike.
    beast: bool,
    tier: Tier,
    /// Where the creature starts along x (units).
    from: f32,
    /// The crate's half side and height (units); the target stands on top.
    half: f32,
    high: f32,
    /// A wall segment stands in the room, and the creature has a blaster in
    /// its second weapon set.
    wall: bool,
    second: bool,
}

impl Scene {
    const BASE: Scene = Scene {
        rules: "advanced",
        kind: "vibroblade",
        keys: PLAIN,
        beast: false,
        tier: Tier::Ordinary,
        from: -300.0,
        half: 40.0,
        high: 120.0,
        wall: false,
        second: false,
    };
}

/// The wall segment's ends (units): just behind the creature's start, across
/// its line to the crate.
const WALL: [f32; 4] = [-380.0, -200.0, -370.0, 200.0];

/// The room's level: its floor, the crate and the wall, and the navigation
/// mesh with holes where they stand.
fn room(scene: &Scene, anims: Arc<crate::motion::AnimTable>) -> Level {
    let (half, high) = (scene.half * UNIT, scene.high * UNIT);
    let crate_box = [
        [CRATE_AT[0] * UNIT - half, CRATE_AT[1] * UNIT - half],
        [CRATE_AT[0] * UNIT + half, CRATE_AT[1] * UNIT + half],
    ];
    let wall_box = scene.wall.then(|| {
        [
            [WALL[0] * UNIT, WALL[1] * UNIT],
            [WALL[2] * UNIT, WALL[3] * UNIT],
        ]
    });
    let mut builder = LevelBuilder::new();
    builder.aabb(
        [-ROOM - 1.0, -ROOM - 1.0, -1.0],
        [ROOM + 1.0, ROOM + 1.0, 0.0],
        Tag::level(Contents::SOLID, 0),
    );
    builder.aabb(
        [crate_box[0][0], crate_box[0][1], 0.0],
        [crate_box[1][0], crate_box[1][1], high],
        Tag {
            entity: Some(PROP),
            ..Tag::level(Contents(Contents::SHAPE.0 | Contents::NONWALK.0), 0)
        },
    );
    if let Some([low, up]) = wall_box {
        builder.aabb(
            [low[0], low[1], 0.0],
            [up[0], up[1], 3.0],
            Tag::level(Contents::SOLID, 0),
        );
    }
    let covered = |x: f32, y: f32| {
        [Some(crate_box), wall_box]
            .into_iter()
            .flatten()
            .any(|[low, up]| x + CELL > low[0] && x < up[0] && y + CELL > low[1] && y < up[1])
    };
    let cells = (2.0 * ROOM / CELL) as usize;
    let triangles: Vec<[Vec3; 3]> = (0..cells)
        .flat_map(|i| (0..cells).map(move |j| (i, j)))
        .map(|(i, j)| (-ROOM + i as f32 * CELL, -ROOM + j as f32 * CELL))
        .filter(|&(x, y)| !covered(x, y))
        .flat_map(|(x, y)| {
            let [a, b, c, d] = [(x, y), (x + CELL, y), (x + CELL, y + CELL), (x, y + CELL)]
                .map(|(x, y)| [x, y, 0.0]);
            [[a, b, c], [a, c, d]]
        })
        .collect();
    let nav = collision::nav::NavMesh::build(&triangles, crate::walk::CLEARANCE * UNIT);
    Level::new("reach", Arc::new(builder.build()), anims).with_nav(nav)
}

/// A weapon item for a creature's weapon sets.
fn item(template: &str, weapon: crate::combat::Weapon, slot: u8) -> ItemFacts {
    ItemFacts {
        slots: 1 << slot,
        weapon: Some(weapon),
        bare: Some(weapon),
        ..ItemFacts::plain(template)
    }
}

/// A fight that runs: its world, the level and every tick's snapshot.
struct Run {
    world: SimWorld,
    level: Level,
    ticks: Vec<Snapshot>,
    scripted: bool,
}

impl Run {
    fn new(scene: &Scene) -> Result<Run, String> {
        let anims = Arc::new(duel_anims(&Missing::None)?);
        let mut level =
            room(scene, anims).with_friendly_fire(crate::combat::damage::FriendlyFire::Full);
        let keys: Vec<String> = scene.keys.split_whitespace().map(String::from).collect();
        let mut creature = setup(1, scene.kind, &{
            let mut all = vec![format!("x={}", scene.from), "y=0".into(), "yaw=0".into()];
            all.extend(keys.iter().cloned());
            all
        })?;
        creature.origin[2] = 0.125 - SOLE_Z;
        let mut module = duel_module(10)?;
        module.placements.push(crate::module::Placement::Own(
            crate::module::ObjectData::bare(
                ObjectKind::Placeable,
                "crate",
                [CRATE_AT[0] * UNIT, CRATE_AT[1] * UNIT, 0.0],
            ),
        ));
        module
            .placements
            .push(crate::module::Placement::Own(creature_object(
                &creature,
                crate::combat::sheet::factions::HOSTILE_1,
                "fencer",
                scene.beast,
            )));
        level = level.with_module(module);
        let gun = setup(1, "pistol", &[])?.sheet.weapons[0].ok_or("no pistol")?;
        if scene.second {
            let sword = creature.sheet.weapons[0].ok_or("no melee weapon")?;
            let catalog = Catalog::new([
                item("melee", sword, slots::RIGHT_WEAPON),
                item("gun", gun, slots::RIGHT_WEAPON_2),
            ]);
            level = level.with_scripts(crate::probe::library_of([], catalog));
        }
        let mut target = setup(0, "medium", &["hp=400".to_string(), "fp=0".to_string()])?;
        target.origin = [CRATE_AT[0], CRATE_AT[1], 0.125 - SOLE_Z];
        target.yaw = 180.0;
        let config = WorldConfig {
            seed: 7,
            ..WorldConfig::one_seat()
        };
        let mut world = SimWorld::new(config, level.clone()).map_err(|e| e.to_string())?;
        let mut start = world.snapshot();
        start.saber_rules = scene
            .rules
            .parse::<SaberRules>()
            .map_err(|e| e.to_string())?;
        let avatar = start
            .entities
            .iter_mut()
            .find(|e| e.kind == Kind::Avatar(Seat(0)))
            .ok_or("no seat 0")?;
        avatar.player = Some(MotionState::standing(
            [400.0, -300.0, target.origin[2]],
            0.0,
        ));
        avatar.sheet = Some(target.sheet);
        avatar.fighter = Some(Fighter::of(&target.sheet));
        let creature_entity = start
            .entities
            .iter_mut()
            .find(|e| e.kind == Kind::Object(ObjectKind::Creature, CREATURE))
            .ok_or("no creature")?;
        let creature_id = creature_entity.id.0;
        if let Some(fighter) = creature_entity.fighter.as_mut() {
            fighter.mind.pace.forced = Some(scene.tier);
        }
        crate::world::stances::restance_snapshot(&mut start, &level);
        world = SimWorld::from_snapshot(&start, level.clone()).map_err(|e| e.to_string())?;
        if scene.second {
            let id = creature_id;
            let mut state = world.script_state().clone();
            let held = [
                ("melee", slots::RIGHT_WEAPON),
                ("gun", slots::RIGHT_WEAPON_2),
            ];
            for (template, slot) in held {
                state.make_item(
                    level.scripts().ok_or("no scripts")?.items(),
                    template,
                    id,
                    1,
                    Some(slot),
                );
            }
            world.set_script_state(state);
        }
        Ok(Run {
            world,
            level,
            ticks: Vec::new(),
            scripted: scene.second,
        })
    }

    /// `n` ticks with seat 0 standing still. A creature the scripts run is
    /// told to attack seat 0.
    fn step(&mut self, n: usize) {
        for _ in 0..n {
            let mut input = TickInput::default();
            input.insert(Seat(0), Command::default());
            if self.scripted {
                let mut state = self.world.script_state().clone();
                let (player, foe) = (self.seat().map(|e| e.0), self.creature_id());
                if let (Some(player), Some(foe)) = (player, foe) {
                    state.intents.insert(
                        foe,
                        crate::script::actions::Intent {
                            target: script_ncs::vm::ObjectId(player),
                            passive: false,
                            talent: None,
                        },
                    );
                    self.world.set_script_state(state);
                }
            }
            let tick = self.ticks.last().map_or(self.world.tick(), |s| s.tick) + 1;
            let msec = TickRate::default().msec(tick);
            let snapshot = crate::step(&mut self.world, tick, &input, msec, StepReason::Authority);
            self.ticks.push(snapshot);
        }
    }

    fn seat(&self) -> Option<(u32, MotionState)> {
        let last = self.ticks.last()?;
        let entity = last.own(Seat(0))?;
        Some((entity.id.0, entity.player?))
    }

    fn creature_id(&self) -> Option<u32> {
        self.ticks
            .last()?
            .entities
            .iter()
            .find(|e| e.kind == Kind::Object(ObjectKind::Creature, CREATURE))
            .map(|e| e.id.0)
    }

    /// Seat 0 stood at `at` (units, its origin) on the next tick: the world
    /// is made again from the snapshot.
    fn put_target(&mut self, at: Vec3) -> Result<(), String> {
        let mut start = self.ticks.last().ok_or("no ticks")?.clone();
        let avatar = start
            .entities
            .iter_mut()
            .find(|e| e.kind == Kind::Avatar(Seat(0)))
            .ok_or("no seat 0")?;
        let yaw = avatar.player.map_or(180.0, |ps| ps.view[1]);
        let mut ps = MotionState::standing(at, yaw);
        ps.ground = None;
        avatar.player = Some(ps);
        let state = self.world.script_state().clone();
        self.world =
            SimWorld::from_snapshot(&start, self.level.clone()).map_err(|e| e.to_string())?;
        self.world.set_script_state(state);
        Ok(())
    }

    /// The target put on top of its crate, and settled there.
    fn perch_target(&mut self, high: f32) -> Result<(), String> {
        self.step(SETTLE * SECOND);
        self.put_target([CRATE_AT[0], CRATE_AT[1], high + DROP - SOLE_Z])
    }

    /// Where the target is put back on the floor, beside the crate.
    fn floor_target(&mut self) -> Result<(), String> {
        self.put_target([CRATE_AT[0] - 200.0, 0.0, DROP - SOLE_Z])
    }

    /// The creature on a tick: its body and its fighter.
    fn creature(tick: &Snapshot) -> Option<(MotionState, Fighter)> {
        let entity = tick
            .entities
            .iter()
            .find(|e| e.kind == Kind::Object(ObjectKind::Creature, CREATURE))?;
        Some((entity.player?, entity.fighter?))
    }

    fn target(tick: &Snapshot) -> Option<(MotionState, Fighter)> {
        let entity = tick.own(Seat(0))?;
        Some((entity.player?, entity.fighter?))
    }

    /// Whether the target sees the creature on a tick.
    fn seen(&self, tick: &Snapshot) -> bool {
        match (Run::target(tick), Run::creature(tick)) {
            (Some((target, _)), Some((creature, _))) => self.level.sees(
                None,
                crate::combat::mind::eye(&target),
                crate::combat::mind::chest(&creature),
            ),
            _ => true,
        }
    }
}

/// Whether a body at `ps` stands inside the crate's box (units).
fn inside_crate(ps: &MotionState, half: f32) -> bool {
    (ps.origin[0] - CRATE_AT[0]).abs() < half + crate::motion::HALF_WIDTH * 0.5
        && (ps.origin[1] - CRATE_AT[1]).abs() < half + crate::motion::HALF_WIDTH * 0.5
}

/// Which tick (from the one after `from`) the creature first stands on the
/// crate.
fn stands_on_crate(ticks: &[Snapshot], from: usize) -> Option<usize> {
    ticks
        .iter()
        .enumerate()
        .skip(from)
        .find(|(_, tick)| {
            Run::creature(tick).is_some_and(|(ps, _)| ps.ground == Some(Ground::Entity(PROP)))
        })
        .map(|(at, _)| at)
}

/// A probe that runs under the rules it is given.
type Case = fn(&'static str) -> Result<String, String>;

/// The rule sets the probes that hold under both run under.
const RULES: [&str; 2] = ["ja", "advanced"];

fn strikes_up(rules: &'static str) -> Result<String, String> {
    let scene = Scene {
        rules,
        high: 20.0,
        half: 24.0,
        ..Scene::BASE
    };
    let mut run = Run::new(&scene)?;
    run.perch_target(scene.high)?;
    let hp = Run::target(run.ticks.last().ok_or("no ticks")?).map(|(_, f)| f.hp);
    run.step(8 * SECOND);
    let last = run.ticks.last().ok_or("no ticks")?;
    let after = Run::target(last).map(|(_, f)| f.hp);
    let in_box = run
        .ticks
        .iter()
        .filter_map(Run::creature)
        .any(|(ps, _)| inside_crate(&ps, scene.half));
    check(
        after < hp && !in_box,
        format!(
            "under the {rules} rules the target's health went from {hp:?} to {after:?}; the creature stood in the crate's box: {in_box}"
        ),
    )
}

fn jumps_onto(rules: &'static str) -> Result<String, String> {
    let scene = Scene {
        rules,
        high: out_of::JUMP_RISE.floor() - 1.0,
        half: 80.0,
        ..Scene::BASE
    };
    let mut run = Run::new(&scene)?;
    run.perch_target(scene.high)?;
    let from = run.ticks.len();
    run.step(6 * SECOND);
    let on = stands_on_crate(&run.ticks, from);
    check(
        on.is_some(),
        format!(
            "under the {rules} rules the creature stood on the crate at tick {on:?} of {from} + {}",
            6 * SECOND
        ),
    )
}

fn hides_from(rules: &'static str) -> Result<String, String> {
    let scene = Scene {
        rules,
        wall: true,
        ..Scene::BASE
    };
    let mut run = Run::new(&scene)?;
    run.perch_target(scene.high)?;
    run.step(8 * SECOND);
    let last = run.ticks.last().ok_or("no ticks")?.clone();
    let hidden = !run.seen(&last);
    run.floor_target()?;
    let from = run.ticks.len();
    run.step(8 * SECOND);
    let swung = run.ticks[from..]
        .iter()
        .filter_map(Run::creature)
        .any(|(_, f)| {
            f.saber.current_move.attack() || f.last_command.buttons & buttons::ATTACK != 0
        });
    check(
        hidden && swung,
        format!(
            "under the {rules} rules the target could not see the creature after 8 seconds: {hidden}; the creature attacked once the target was back on the floor: {swung}"
        ),
    )
}

/// The creature jumps, as one that presses jump (the edges of `up`).
fn presses_jump(ticks: &[Snapshot]) -> usize {
    let ups: Vec<bool> = ticks
        .iter()
        .filter_map(Run::creature)
        .map(|(_, f)| f.last_command.up > 0)
        .collect();
    ups.windows(2).filter(|pair| !pair[0] && pair[1]).count()
}

pub(super) fn probes() -> Checks {
    vec![
        ("reach_table", reach_table()),
        ("fallback_table", fallback_table()),
        ("powers_are_free_out_of_reach", powers_are_free()),
        ("strikes_a_target_on_a_low_prop", strikes_up("advanced")),
        ("jumps_onto_a_prop", jumps_onto("advanced")),
        ("hides_from_a_target_out_of_reach", hides_from("advanced")),
        ("force_user_jumps_higher", force_user_jumps_higher()),
        ("pushes_a_target_off", pushes_a_target_off()),
        ("switches_to_its_blaster", switches_to_its_blaster()),
        ("beast_does_not_swing_at_air", beast_does_not_swing_at_air()),
        ("beast_never_jumps", beast_never_jumps()),
        ("floor_fights_unchanged_under_ja", floor_fights_unchanged()),
        ("both_rule_sets_reach", both_rule_sets()),
    ]
}

fn both_rule_sets() -> Result<String, String> {
    let cases: [(&str, Case); 3] = [
        ("strikes up", strikes_up),
        ("jumps", jumps_onto),
        ("hides", hides_from),
    ];
    let mut told = Vec::new();
    for (name, case) in cases {
        for rules in RULES {
            case(rules).map_err(|e| format!("{name} under {rules}: {e}"))?;
            told.push(format!("{name} under {rules}"));
        }
    }
    Ok(told.join(", "))
}

fn reach_table() -> Result<String, String> {
    let rows = [
        (reach(true, 200.0, 0.0, true, true, 0), Reach::Floor),
        (reach(false, -5.0, 30.0, true, true, 0), Reach::Blade),
        (
            reach(false, 40.0, out_of::JUMP_RISE, true, false, 0),
            Reach::Jump,
        ),
        (reach(false, 40.0, 90.0, true, true, 0), Reach::ForceJump),
        (reach(false, 40.0, 90.0, true, false, 0), Reach::Not),
        (reach(false, 40.0, 20.0, false, true, 0), Reach::Not),
        (reach(false, 40.0, 20.0, true, true, 2), Reach::Not),
        (reach(false, 40.0, 100.0, true, true, 0), Reach::Not),
    ];
    let wrong: Vec<_> = rows.iter().filter(|(got, want)| got != want).collect();
    check(
        wrong.is_empty(),
        format!("wrong answers (got, wanted): {wrong:?}"),
    )
}

fn fallback_table() -> Result<String, String> {
    let rows = [
        (fallback(Tier::Skilled, true, false), Fallback::Push),
        (fallback(Tier::Boss, true, true), Fallback::Push),
        (fallback(Tier::Ordinary, true, true), Fallback::Ranged),
        (fallback(Tier::Ordinary, false, false), Fallback::Hide),
        (fallback(Tier::Boss, false, false), Fallback::Hide),
    ];
    let wrong: Vec<_> = rows.iter().filter(|(got, want)| got != want).collect();
    check(
        wrong.is_empty(),
        format!("wrong answers (got, wanted): {wrong:?}"),
    )
}

fn powers_are_free() -> Result<String, String> {
    let (held, free) = (
        power_allowed(false, true, true, 0, 99_000),
        power_allowed(true, true, true, 0, 99_000),
    );
    check(
        !held && free,
        format!(
            "a power mid-swing, in reach, before its wait is over: {held}; out of reach: {free}"
        ),
    )
}

fn force_user_jumps_higher() -> Result<String, String> {
    let high = 90.0;
    let scene = Scene {
        half: 80.0,
        high,
        ..Scene::BASE
    };
    let reached = |keys: &'static str| -> Result<(bool, usize), String> {
        let mut run = Run::new(&Scene { keys, ..scene })?;
        run.perch_target(high)?;
        let from = run.ticks.len();
        run.step(10 * SECOND);
        Ok((
            stands_on_crate(&run.ticks, from).is_some(),
            presses_jump(&run.ticks[from..]),
        ))
    };
    let (with_force, _) = reached("fp=100")?;
    let (without, presses) = reached(PLAIN)?;
    check(
        with_force && !without && presses <= 2,
        format!(
            "with Force points it stood on the crate: {with_force}; without, it did: {without} and pressed jump {presses} times"
        ),
    )
}

fn pushes_a_target_off() -> Result<String, String> {
    let scene = Scene {
        keys: "fp=100 powers=23",
        tier: Tier::Skilled,
        from: -60.0,
        half: 24.0,
        high: 120.0,
        ..Scene::BASE
    };
    let mut run = Run::new(&scene)?;
    run.perch_target(scene.high)?;
    let from = run.ticks.len();
    run.step(5 * SECOND);
    let down = run.ticks[from..]
        .iter()
        .position(|tick| Run::target(tick).is_some_and(|(ps, _)| ps.ground == Some(Ground::World)));
    check(
        down.is_some(),
        format!("the target stood on the floor again {down:?} ticks after it was put on the crate"),
    )
}

fn switches_to_its_blaster() -> Result<String, String> {
    let scene = Scene {
        second: true,
        ..Scene::BASE
    };
    let mut run = Run::new(&scene)?;
    run.perch_target(scene.high)?;
    run.step(5 * SECOND);
    let shooting = run
        .ticks
        .last()
        .and_then(Run::creature)
        .is_some_and(|(_, f)| f.shooting.fired_at > 0);
    run.floor_target()?;
    run.step(3 * SECOND + SECOND / 2);
    let melee = run
        .ticks
        .last()
        .and_then(|tick| {
            tick.entities
                .iter()
                .find(|e| e.kind == Kind::Object(ObjectKind::Creature, CREATURE))
        })
        .and_then(|e| e.sheet)
        .is_some_and(|sheet: Sheet| !sheet.main_weapon().delivery.ranged());
    check(
        shooting && melee,
        format!(
            "the creature fired at its target on the crate: {shooting}; held its melee weapon again 3.5 seconds after the target was back on the floor: {melee}"
        ),
    )
}

fn beast_does_not_swing_at_air() -> Result<String, String> {
    let scene = Scene {
        beast: true,
        kind: "fists",
        wall: true,
        ..Scene::BASE
    };
    let mut run = Run::new(&scene)?;
    run.perch_target(scene.high)?;
    let from = run.ticks.len();
    run.step(8 * SECOND);
    let after = from + (out_of::AFTER_MS as usize * SECOND / 1000) + SECOND / 2;
    let swung = run.ticks[after..]
        .iter()
        .filter_map(Run::creature)
        .any(|(_, f)| f.last_command.buttons & buttons::ATTACK != 0);
    let last = run.ticks.last().ok_or("no ticks")?.clone();
    let hidden = !run.seen(&last);
    check(
        !swung && hidden,
        format!(
            "the beast attacked after the target had stood on the crate for {} ms: {swung}; it was out of the target's sight after 8 seconds: {hidden}",
            out_of::AFTER_MS
        ),
    )
}

fn beast_never_jumps() -> Result<String, String> {
    let scene = Scene {
        beast: true,
        kind: "fists",
        high: 30.0,
        half: 60.0,
        ..Scene::BASE
    };
    let mut run = Run::new(&scene)?;
    run.perch_target(scene.high)?;
    run.step(8 * SECOND);
    let jumps = presses_jump(&run.ticks);
    check(jumps == 0, format!("the beast pressed jump {jumps} times"))
}

fn floor_fights_unchanged() -> Result<String, String> {
    let scene = Scene {
        rules: "ja",
        wall: true,
        ..Scene::BASE
    };
    let mut run = Run::new(&scene)?;
    run.step(SETTLE * SECOND);
    run.floor_target()?;
    run.step(8 * SECOND);
    let marked = run.ticks.iter().filter_map(Run::creature).any(|(_, f)| {
        f.mind.pace.off_floor_since != 0 || f.mind.pace.hide_at.is_some() || f.last_command.up > 0
    });
    check(
        !marked,
        format!(
            "a creature fighting a target on the floor under Jedi Academy's rules noted a perch, hid or jumped: {marked}"
        ),
    )
}
