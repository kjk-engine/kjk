//! Probes of `ActionMoveAwayFromObject` (`sim::script::engine::Engine::away`),
//! ensuring fleeing creatures clear their escape range across flat, sloped,
//! and narrow walkways so heartbeat triggers do not stall inside range and
//! repeat sounds such as `kas_m24aa`'s Tach cries.

use std::sync::Arc;

use collision::{CollisionWorld, Contents, LevelBuilder, Tag, UNIT};
use fmt_odyssey::ncs::{Const, Op, Pair};
use script_ncs::asm::Asm;
use script_ncs::catalog::Catalog;

use crate::combat::Sheet;
use crate::combat::sheet::factions;
use crate::input::TickInput;
use crate::module::{ObjectData, Placement};
use crate::motion::AnimTable;
use crate::notice::Notice;
use crate::probe::{Checks, check};
use crate::world::{Level, SimWorld, StepReason, WorldConfig, step};

fn floor_level(quads: &[([f32; 3], [f32; 3], [f32; 3], [f32; 3])]) -> Level {
    let mut builder = LevelBuilder::new();
    let mut tris = Vec::new();
    let tag = Tag::level(Contents::SOLID, 0);
    for &(a, b, c, d) in quads {
        let [pa, pb, pc, pd] = [a, b, c, d].map(|p| p.map(|v| v / UNIT));
        builder.triangle([pa, pb, pc], tag);
        builder.triangle([pa, pc, pd], tag);
        tris.push([a, b, c]);
        tris.push([a, c, d]);
    }
    let mut nav = collision::nav::NavMesh::build(&tris, crate::walk::CLEARANCE * UNIT);
    let built = Arc::new(builder.build());
    if let Some(level) = built.level() {
        nav.mark(level);
    }
    Level::new("flee_test", built, Arc::new(AnimTable::stand_in())).with_nav(nav)
}

fn flee_library() -> super::Library {
    let catalog = r#"
int TRUE = 1;
int FALSE = 0;
// 0
object GetFirstPC();
// 1
float GetDistanceToObject(object oObject);
// 2
void ClearAllActions(int nClearCombatState = FALSE);
// 3
void ActionMoveAwayFromObject(object oFleeFrom, int bRun = FALSE, float fMoveAwayRange = 40.0f);
// 4
void PlaySound(string sSoundName);
"#;
    // `if (GetDistanceToObject(GetFirstPC()) < 40.0) { ClearAllActions(); ActionMoveAwayFromObject(GetFirstPC(), TRUE, 40.0); PlaySound("c_tach_bat1"); }`
    let mut asm = Asm::default();
    asm.op(Op::Action {
        routine: 0,
        args: 0,
    })
    .op(Op::Action {
        routine: 1,
        args: 1,
    })
    .op(Op::Const(Const::Float(40.0)))
    .op(Op::Lt(Pair::FloatFloat))
    .to(Op::Jz, "done")
    .op(Op::Const(Const::Int(0)))
    .op(Op::Action {
        routine: 2,
        args: 1,
    })
    .op(Op::Const(Const::Float(40.0)))
    .op(Op::Const(Const::Int(1)))
    .op(Op::Action {
        routine: 0,
        args: 0,
    })
    .op(Op::Action {
        routine: 3,
        args: 3,
    })
    .op(Op::Const(Const::String(b"c_tach_bat1".to_vec())))
    .op(Op::Action {
        routine: 4,
        args: 1,
    })
    .label("done")
    .op(Op::Retn);
    super::Library::new(
        Catalog::parse(catalog).expect("catalog parses"),
        [("flee_beat".to_string(), asm.build())].into(),
        super::Talents::default(),
    )
}

fn run_flee(
    quads: &[([f32; 3], [f32; 3], [f32; 3], [f32; 3])],
    creature_at: [f32; 3],
) -> Result<(usize, [f32; 3], f32), String> {
    let mut data = (**crate::determinism::proving_ground()
        .module()
        .ok_or("proving ground module")?)
    .clone();
    data.entry.position = [0.0, 0.0, 0.0];
    data.placements.clear();
    let mut creature = ObjectData::creature(
        "tach",
        "tach",
        creature_at,
        Sheet {
            faction: factions::NEUTRAL,
            ..Sheet::dev_soldier()
        },
    );
    creature.scripts = vec![("ScriptHeartbeat".to_string(), "flee_beat".to_string())];
    data.placements.push(Placement::Own(creature));
    let level = floor_level(quads)
        .with_module(data)
        .with_scripts(flee_library());
    let config = WorldConfig {
        spawn: [0.0, 0.0, 0.0],
        ..WorldConfig::one_seat()
    };
    let mut world = SimWorld::new(config, level).map_err(|e| e.to_string())?;
    let input = TickInput::default();
    let mut sounds = 0usize;
    let mut last = world.snapshot();
    for tick in 1..=1562 {
        last = step(&mut world, tick, &input, 16, StepReason::Authority);
        for (_, notice) in world.take_notices() {
            if matches!(notice, Notice::Sound(sound) if sound.resref() == "c_tach_bat1") {
                sounds += 1;
            }
        }
    }
    let creature = last
        .entities
        .iter()
        .find(|entity| {
            matches!(
                entity.kind,
                crate::snapshot::Kind::Object(crate::module::ObjectKind::Creature, _)
            )
        })
        .ok_or("creature alive")?;
    let player = last
        .entities
        .iter()
        .find(|entity| matches!(entity.kind, crate::snapshot::Kind::Avatar(_)))
        .ok_or("player alive")?;
    let dist = super::flat(creature.position, player.position);
    Ok((sounds, creature.position, dist))
}

pub fn probes() -> Checks {
    let flat = [(
        [-10.0, -10.0, 0.0],
        [60.0, -10.0, 0.0],
        [60.0, 10.0, 0.0],
        [-10.0, 10.0, 0.0],
    )];
    let near = match run_flee(&flat, [39.95, 0.0, 0.0]) {
        Ok((sounds, _, dist)) => check(
            sounds == 1 && dist >= 40.0,
            format!("sounds={sounds} dist={dist:.3} m"),
        ),
        Err(err) => Err(err),
    };
    let l_walk = [
        (
            [-5.0, -5.0, 0.0],
            [28.0, -5.0, 0.0],
            [28.0, 5.0, 0.0],
            [-5.0, 5.0, 0.0],
        ),
        (
            [20.0, 5.0, 0.0],
            [28.0, 5.0, 0.0],
            [28.0, 48.0, 2.5],
            [20.0, 48.0, 2.5],
        ),
    ];
    let sloped = match run_flee(&l_walk, [24.0, 0.0, 0.0]) {
        Ok((sounds, pos, dist)) => check(
            sounds == 1 && pos[1] >= 30.0 && dist >= 40.0,
            format!("sounds={sounds} pos={pos:?} dist={dist:.3} m"),
        ),
        Err(err) => Err(err),
    };
    vec![
        ("fleeing clears range so heartbeat sound stops", near),
        ("fleeing follows sloped and turned walkway", sloped),
    ]
}
