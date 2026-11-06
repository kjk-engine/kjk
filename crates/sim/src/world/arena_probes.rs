//! The probes of `arena` (`kjk sim probes`): on the proving ground as an
//! arena, a foe called in stands ahead hostile and is taken away by a
//! clear, god mode turns on and off, a player in the pit goes down and
//! comes back whole at the respawn point, and the arena's props stand where
//! its rules put them, once.

use std::sync::Arc;

use super::SimWorld;
use super::arena::{Prop, Rules, SPAWN_AHEAD, prop_template};
use crate::action::{Action, ArenaOrder};
use crate::combat::Sheet;
use crate::combat::sheet::{factions, flags};
use crate::input::{Command, Seat, TickInput};
use crate::module::{ObjectData, ObjectKind};
use crate::probe::check;
use crate::snapshot::{Kind, Snapshot};
use crate::{StepReason, WorldConfig};

/// The foe's template, friendly as written: the arena turns it.
const FOE: &str = "probe_foe";
/// How long a downed player lies in the probes' arena (ms).
const RESPAWN_MS: u64 = 500;

/// The proving ground as an arena laying out `props`, the foe's template in
/// its library; the world and its rules.
pub(super) fn arena_with(props: Vec<Prop>) -> Result<(SimWorld, Rules), String> {
    arena_playing(props, WorldConfig::one_seat(), |_| {})
}

/// `arena_with` with its seat set up by `config` and its module changed by
/// `edit`.
pub(super) fn arena_playing(
    props: Vec<Prop>,
    config: WorldConfig,
    edit: impl FnOnce(&mut crate::module::ModuleData),
) -> Result<(SimWorld, Rules), String> {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    edit(&mut data);
    let ground = ground.with_module(data);
    let entry = ground.module().ok_or("the ground has no module")?.entry;
    let rules = Rules {
        respawn: entry.position,
        yaw: entry.yaw,
        pit: -3.0,
        respawn_ms: RESPAWN_MS,
        props: props.into(),
    };
    let mut foe = ObjectData::creature(FOE, FOE, [0.0; 3], Sheet::dev_soldier());
    if let Some(sheet) = foe.sheet.as_mut() {
        sheet.faction = factions::PLAYER;
        sheet.flags |= flags::PLOT;
    }
    let library = crate::probe::library("", std::iter::empty::<(String, _)>())
        .with_creatures([(FOE.to_string(), Arc::new(foe))].into());
    let level = ground.with_scripts(library).with_arena(rules.clone());
    let world = SimWorld::new(config, level).map_err(|error| error.to_string())?;
    Ok((world, rules))
}

/// The proving ground as an arena with no props.
fn arena() -> Result<(SimWorld, Rules), String> {
    arena_with(Vec::new())
}

/// `steps` steps of seat 0 standing, `orders` given on the first.
fn run(world: &mut SimWorld, orders: &[ArenaOrder], steps: u32) -> Snapshot {
    let mut last = world.snapshot();
    for step in 0..steps {
        let mut input = TickInput::default();
        input.insert(Seat(0), Command::default());
        if step == 0 {
            for order in orders {
                input.act(Seat(0), Action::Arena(order.clone()));
            }
        }
        let tick = world.tick() + 1;
        last = crate::step(world, tick, &input, 8, StepReason::Authority);
    }
    last
}

/// The creatures standing in `snapshot` that no seat plays.
fn foes(snapshot: &Snapshot) -> Vec<&crate::snapshot::EntityState> {
    snapshot
        .entities
        .iter()
        .filter(|entity| matches!(entity.kind, Kind::Object(ObjectKind::Creature, _)))
        .filter(|entity| entity.fighter.is_some_and(|fighter| fighter.alive()))
        .collect()
}

pub fn probes() -> crate::probe::Checks {
    vec![
        ("arena_spawn_and_clear", spawn_and_clear()),
        ("arena_god", god()),
        ("arena_pit_and_respawn", pit_and_respawn()),
        ("arena_down_gets_up_by_its_side", down_gets_up()),
        ("arena_props_laid_out_once", props_laid_out()),
    ]
}

/// The placeables standing in `snapshot`: where, and their template.
fn placeables(world: &SimWorld, snapshot: &Snapshot) -> Vec<([f32; 3], String)> {
    let instances = crate::instance::Instances::of(world.level(), &snapshot.spawned);
    snapshot
        .entities
        .iter()
        .filter_map(|entity| {
            let Kind::Object(ObjectKind::Placeable, index) = entity.kind else {
                return None;
            };
            let template = instances.made_from(index)?.template.clone();
            Some((entity.position, template))
        })
        .collect()
}

/// Two props in the rules stand where they say, made from their rows'
/// templates, whose facts the level knows (a joiner rebuilds them so), and
/// are not laid out again while they stand.
fn props_laid_out() -> Result<String, String> {
    let props: Vec<Prop> = [(39, [3.0, 4.0, 0.0]), (179, [-3.0, 4.0, 0.0])]
        .into_iter()
        .map(|(appearance, at)| Prop {
            appearance,
            name: format!("prop {appearance}"),
            at,
            yaw: 270.0,
            footprint: vec![[-0.5, -0.5, 0.0], [0.5, 0.5, 1.0]],
            destructible: None,
        })
        .collect();
    let (mut world, rules) = arena_with(props.clone())?;
    let first = run(&mut world, &[], 4);
    let later = run(&mut world, &[], 30);
    let standing = placeables(&world, &later);
    let placed = props.iter().all(|prop| {
        standing.iter().any(|(at, template)| {
            *template == prop_template(prop.appearance)
                && (at[0] - prop.at[0]).hypot(at[1] - prop.at[1]) < 0.01
        })
    });
    let known = props.iter().all(|prop| {
        world
            .level()
            .template(ObjectKind::Placeable, &prop_template(prop.appearance))
            .is_some_and(|facts| facts.geometry == prop.footprint)
    });
    check(
        placeables(&world, &first).len() == 2 && standing.len() == 2 && placed && known,
        format!(
            "{} standing after 4 steps, {} after 34, where the rules put them: {placed}; facts known: {known} ({} props in the rules)",
            placeables(&world, &first).len(),
            standing.len(),
            rules.props.len()
        ),
    )
}

/// `down N` for a death that leaves the body on its back (1) and one on its
/// front (4): it plays that death, lies past the respawn, and gets up where
/// it lies in the get-up for that side (`death::getup`).
fn down_gets_up() -> Result<String, String> {
    use crate::motion::Anim;
    let mut seen = Vec::new();
    for (n, death, getup) in [
        (1, Anim::BOTH_DEATH1, Anim::BOTH_GETUP4),
        (4, Anim::BOTH_DEATH4, Anim::BOTH_GETUP3),
    ] {
        let (mut world, _) = arena()?;
        let start = run(&mut world, &[], 2)
            .own(Seat(0))
            .ok_or("no player")?
            .position;
        let fell = run(&mut world, &[ArenaOrder::Down(n)], 2);
        let me = fell.own(Seat(0)).ok_or("no player")?;
        let lies = me.fighter.is_some_and(|f| f.downed())
            && me.player.is_some_and(|p| p.legs.anim == death);
        let steps = super::falls::ARENA_REVIVE_MS / 8 + 2;
        let up = run(&mut world, &[], steps);
        let me = up.own(Seat(0)).ok_or("no player")?;
        let rose = me.fighter.is_some_and(|f| f.alive())
            && me.player.is_some_and(|p| p.legs.anim == getup);
        let moved = (me.position[0] - start[0]).hypot(me.position[1] - start[1]);
        seen.push(format!(
            "down {n}: lay in {death:?} {lies}, up in {getup:?} {rose}, {moved:.2} m from where it fell"
        ));
        if !lies || !rose || moved > 0.1 {
            return Err(seen.join("; "));
        }
    }
    Ok(seen.join("; "))
}

/// Two foes called in stand `SPAWN_AHEAD` ahead, hostile and killable
/// though their template is a friendly plot creature; a clear takes both.
fn spawn_and_clear() -> Result<String, String> {
    let (mut world, _) = arena()?;
    let start = world.snapshot();
    let me = start.own(Seat(0)).ok_or("no player")?.position;
    let spawn = ArenaOrder::Spawn(FOE.to_string());
    let called = run(&mut world, &[spawn.clone(), spawn], 30);
    let standing = foes(&called);
    let apart: Vec<f32> = standing
        .iter()
        .map(|foe| (foe.position[0] - me[0]).hypot(foe.position[1] - me[1]))
        .collect();
    let hostile = standing.iter().all(|foe| {
        foe.sheet.is_some_and(|sheet| {
            sheet.faction == factions::HOSTILE_1 && sheet.flags & flags::PLOT == 0
        })
    });
    let cleared = run(&mut world, &[ArenaOrder::Clear], 30);
    let left = foes(&cleared).len();
    check(
        standing.len() == 2
            && apart.iter().all(|m| (m - SPAWN_AHEAD).abs() < 0.5)
            && hostile
            && left == 0,
        format!(
            "{} called in at {apart:?} m (hostile, no plot: {hostile}); {left} left after a clear",
            standing.len()
        ),
    )
}

/// God mode on, then off again.
fn god() -> Result<String, String> {
    let (mut world, _) = arena()?;
    let immortal = |snapshot: &Snapshot| {
        snapshot
            .own(Seat(0))
            .and_then(|me| me.sheet)
            .is_some_and(|sheet| sheet.flags & flags::IMMORTAL != 0)
    };
    let on = immortal(&run(&mut world, &[ArenaOrder::God], 3));
    let off = immortal(&run(&mut world, &[ArenaOrder::God], 3));
    check(on && !off, format!("on: {on}; off again: {}", !off))
}

/// The player under the pit's floor goes down, then after `RESPAWN_MS`
/// stands at the respawn point whole, and the arena is not lost.
fn pit_and_respawn() -> Result<String, String> {
    let (mut world, rules) = arena()?;
    let mut start = world.snapshot();
    let at = start
        .entities
        .iter()
        .position(|entity| entity.kind == Kind::Avatar(Seat(0)))
        .ok_or("no player")?;
    if let Some(player) = start.entities[at].player.as_mut() {
        player.origin[2] = (rules.pit - 1.0) / collision::UNIT;
    }
    if let Some(fighter) = start.entities[at].fighter.as_mut() {
        fighter.hp /= 2;
    }
    let mut world = SimWorld::from_snapshot(&start, world.level().clone())
        .map_err(|error| error.to_string())?;
    let fell = run(&mut world, &[], 2);
    let down = fell.entities[at].fighter.is_some_and(|f| f.downed());
    let back = run(&mut world, &[], (RESPAWN_MS / 8) as u32 + 10);
    let me = &back.entities[at];
    let whole = me.fighter.is_some_and(|f| f.alive())
        && me.sheet.is_some_and(|sheet| {
            me.fighter
                .is_some_and(|f| f.hp == i32::from(sheet.max_hp) * 100)
        });
    let [x, y, _] = me.position;
    let home = (x - rules.respawn[0]).hypot(y - rules.respawn[1]);
    check(
        down && whole && home < 0.1 && back.lost.is_none(),
        format!(
            "down after the fall: {down}; back whole: {whole}, {home:.2} m from the respawn point; lost: {:?}",
            back.lost
        ),
    )
}
