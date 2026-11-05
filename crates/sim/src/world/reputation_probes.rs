//! The probes of `AdjustReputation` (`kjk sim probes reputation`), on the
//! proving ground, no install. As K1's and TSL's executables apply it
//! (`module::adjust`), each change is held to the scale as it is kept:
//! Tatooine's Sand People, at war with the player, put at peace by one in
//! their robes (`k_ptat17_enter`'s +50), made mad (`k_ptat_tuskenmad`'s
//! −100), the robes taken off (−50) and put on again (+50) are at peace
//! again, both ways (the player's standing is one cell). Between two of the
//! table's factions the target's comes to regard the source's as the
//! source's regarded the target's, moved; the other way is untouched. A
//! dead source, and a source of the player's faction, change nothing (run
//! first, while the war stands at 0, so a change would show). The
//! standings survive a save, and an old save's pair reads as before.

use fmt_odyssey::ncs::{Const, Op};
use script_ncs::probes::Asm;

use super::{Fighting, Object, SimWorld};
use crate::combat::Sheet;
use crate::combat::fighter::Life;
use crate::combat::sheet::factions::{FRIENDLY_1, HOSTILE_1, HOSTILE_2, PLAYER};
use crate::input::{Command, Seat, TickInput};
use crate::module::{Adjustments, ObjectData, Placement, Repute};
use crate::probe::{Checks, check};
use crate::script::Pending;
use crate::{StepReason, WorldConfig};

const CATALOG: &str = r#"
// 0
void AdjustReputation(object oTarget, object oSourceFactionMember, int nAdjustment);
// 1
object GetObjectByTag(string sTag, int nNth=0);
// 2
object GetFirstPC();
"#;

/// The Sand People's member, a dead one, and two of factions standing
/// unevenly to each other (`UNEVEN`).
const TUSKEN: &str = "probe_tusken";
const DEAD_TUSKEN: &str = "probe_dead_tusken";
const FRIEND: &str = "probe_friend";
const OTHER: &str = "probe_other";

/// How `FRIENDLY_1` regards `HOSTILE_2`, and how `HOSTILE_2` regards it.
const UNEVEN: (u8, u8) = (80, 20);
/// The change between the two.
const NUDGE: i32 = 5;

/// What the dead source and the player's faction would make of the war.
const PEACE: i32 = 50;

/// Tatooine's changes, as its scripts make them: the robes on, the
/// chieftain made mad, the robes off, on again.
const ROBES: [(&str, i32); 4] = [
    ("robes on", 50),
    ("made mad", -100),
    ("robes off", -50),
    ("robes on again", 50),
];

/// Who a script names: the first PC, or the creature tagged.
#[derive(Clone, Copy)]
enum Who {
    Pc,
    Tag(&'static str),
}

/// `AdjustReputation(target, source, by)`'s script.
fn adjusting(target: Who, source: Who, by: i32) -> fmt_odyssey::ncs::Ncs {
    let name = |asm: &mut Asm, who: Who| match who {
        Who::Pc => {
            asm.op(Op::Action {
                routine: 2,
                args: 0,
            });
        }
        Who::Tag(tag) => {
            asm.op(Op::Const(Const::Int(0)))
                .op(Op::Const(Const::String(tag.as_bytes().to_vec())))
                .op(Op::Action {
                    routine: 1,
                    args: 2,
                });
        }
    };
    let mut asm = Asm::default();
    asm.op(Op::Const(Const::Int(by)));
    name(&mut asm, source);
    name(&mut asm, target);
    asm.op(Op::Action {
        routine: 0,
        args: 3,
    })
    .op(Op::Retn);
    asm.build()
}

/// The probe's scripts: the robes' changes on the player by the Sand
/// People's member, then the uneven nudge, a dead source's and the
/// player's own faction's.
fn scripts() -> Vec<(String, fmt_odyssey::ncs::Ncs)> {
    let tusken = Who::Tag(TUSKEN);
    let mut scripts: Vec<_> = ROBES
        .iter()
        .map(|&(what, by)| (what.to_string(), adjusting(Who::Pc, tusken, by)))
        .collect();
    scripts.extend([
        (
            "nudge".to_string(),
            adjusting(Who::Tag(FRIEND), Who::Tag(OTHER), NUDGE),
        ),
        (
            "dead source".to_string(),
            adjusting(Who::Pc, Who::Tag(DEAD_TUSKEN), PEACE),
        ),
        (
            "player source".to_string(),
            adjusting(tusken, Who::Pc, PEACE),
        ),
    ]);
    scripts
}

/// `repute.2da` for the probe: the player and the Sand People (`HOSTILE_1`)
/// at war, `FRIENDLY_1` and `HOSTILE_2` regarding each other `UNEVEN`ly.
fn repute() -> Repute {
    let mut repute = Repute::at_war([PLAYER, HOSTILE_1]);
    let (friend, other) = (usize::from(FRIENDLY_1), usize::from(HOSTILE_2));
    repute.rows[friend][other] = UNEVEN.0;
    repute.rows[other][friend] = UNEVEN.1;
    repute
}

fn world() -> Result<SimWorld, String> {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    let [x, y, z] = data.entry.position;
    let dead = data.placements.len() as u32 + 1;
    let placed = [
        (TUSKEN, HOSTILE_1, 6.0),
        (DEAD_TUSKEN, HOSTILE_1, -6.0),
        (FRIEND, FRIENDLY_1, 8.0),
        (OTHER, HOSTILE_2, -8.0),
    ];
    data.placements
        .extend(placed.into_iter().map(|(tag, faction, dy)| {
            let sheet = Sheet {
                faction,
                ..Sheet::commoner()
            };
            Placement::Own(ObjectData::creature(tag, tag, [x, y + dy, z], sheet))
        }));
    data.repute = repute();
    let library = crate::probe::library(CATALOG, scripts());
    let level = ground.with_module(data).with_scripts(library);
    let mut world = SimWorld::new(WorldConfig::one_seat(), level).map_err(|e| e.to_string())?;
    step(&mut world);
    kill(&mut world, dead);
    step(&mut world);
    Ok(world)
}

fn step(world: &mut SimWorld) {
    let mut input = TickInput::default();
    input.insert(Seat(0), Command::default());
    let tick = world.tick() + 1;
    crate::step(world, tick, &input, 8, StepReason::Authority);
}

/// The object placed `index`th dies, as a killing blow leaves it.
fn kill(world: &mut SimWorld, index: u32) {
    let mut fighters = world.world.query::<(&Object, &mut Fighting)>();
    for (object, mut fighting) in fighters.iter_mut(&mut world.world) {
        if object.index == index {
            fighting.0.hp = 0;
            fighting.0.life = Life::Dead;
        }
    }
}

/// `script` run by the module, stepped.
fn run(world: &mut SimWorld, script: &str) {
    let mut state = world.script_state().clone();
    state
        .pending
        .push(Pending::new(script, crate::script::MODULE));
    world.set_script_state(state);
    step(world);
}

/// How `a` stands to `b` now.
fn standing(world: &SimWorld, a: u8, b: u8) -> u8 {
    world.level().module().map_or(0, |module| {
        module
            .repute
            .with(&world.script_state().reputation)
            .of(a, b)
    })
}

/// The scripts' state round-tripped through a save's encoding.
fn saved(state: &crate::script::State) -> Result<crate::script::State, String> {
    let mut out = crate::wire::Writer::default();
    state.encode(&mut out);
    let mut input = crate::wire::Reader::new("state", &out.0);
    crate::script::State::decode(&mut input).map_err(|error| error.to_string())
}

pub fn probes() -> Checks {
    let mut world = match world() {
        Ok(world) => world,
        Err(error) => return vec![("reputation", Err(error))],
    };
    let both = |world: &SimWorld| {
        (
            standing(world, HOSTILE_1, PLAYER),
            standing(world, PLAYER, HOSTILE_1),
        )
    };
    let start = both(&world);
    run(&mut world, "dead source");
    let after_dead = both(&world);
    run(&mut world, "player source");
    let after_player = both(&world);
    let robes: Vec<(&str, (u8, u8))> = ROBES
        .iter()
        .map(|&(what, _)| {
            run(&mut world, what);
            (what, both(&world))
        })
        .collect();
    let wanted = [50, 0, 0, 50];
    let robes_held = start == (0, 0)
        && robes
            .iter()
            .zip(wanted)
            .all(|((_, (them, us)), want)| *them == want && *us == want);
    run(&mut world, "nudge");
    let nudged = (
        standing(&world, FRIENDLY_1, HOSTILE_2),
        standing(&world, HOSTILE_2, FRIENDLY_1),
    );
    let nudge_wanted = ((i32::from(UNEVEN.1) + NUDGE) as u8, UNEVEN.1);
    let state = world.script_state().clone();
    let kept = saved(&state).map(|loaded| loaded.reputation == state.reputation);
    let old: Adjustments = [(PLAYER, HOSTILE_1, -50), (FRIENDLY_1, HOSTILE_2, 10)]
        .into_iter()
        .flat_map(|(a, b, by)| crate::module::adjustment_cells(a, b, by))
        .collect();
    let old_reads = {
        let table = repute();
        let standing = table.with(&old);
        [
            standing.of(PLAYER, HOSTILE_1),
            standing.of(HOSTILE_1, PLAYER),
            standing.of(FRIENDLY_1, HOSTILE_2),
            standing.of(HOSTILE_2, FRIENDLY_1),
        ]
    };
    vec![
        (
            "reputation_held_as_kept",
            check(
                robes_held,
                format!(
                    "the Sand People and the player from {start:?}: {robes:?} (wanted {wanted:?})"
                ),
            ),
        ),
        (
            "reputation_target_regards_source",
            check(
                nudged == nudge_wanted,
                format!(
                    "{FRIENDLY_1} to {HOSTILE_2} and back, from {UNEVEN:?} nudged by {NUDGE}: \
                     {nudged:?} (wanted {nudge_wanted:?})"
                ),
            ),
        ),
        (
            "reputation_dead_or_player_source_changes_nothing",
            check(
                after_dead == start && after_player == start,
                format!(
                    "{start:?}; after a dead source {after_dead:?}, the player's {after_player:?}"
                ),
            ),
        ),
        (
            "reputation_kept_in_a_save",
            check(
                kept == Ok(true),
                format!("{kept:?}: {:?}", state.reputation),
            ),
        ),
        (
            "reputation_old_pairs_read_as_before",
            check(
                old_reads == [0, 0, 90, 30] && old.len() == 3,
                format!("an old save's pairs as cells {old:?}: {old_reads:?}"),
            ),
        ),
    ]
}
