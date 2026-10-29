//! The probes of `strikes` (`kjk sim probes`).

use super::*;
use crate::combat::Sheath;
use crate::combat::saber::Move as SaberMove;
use crate::combat::sheet::damage_types;
use crate::world::COMBAT_MS;

/// The probe's routines (`probes`).
const CATALOG: &str = r#"
// 0
void SendMessageToPC(object oPlayer, string szMessage);
// 1
object GetFirstPC();
// 2
object GetObjectByTag(string sTag, int nNth = 0);
// 3
void ActionAttack(object oAttackee, int bPassive = FALSE);
// 4
void SetLightsaberPowered(object oCreature, int bOverride, int bPowered = TRUE, int bShowTransition = FALSE);
"#;

/// The beast's spawn script attacks the dummy (`ActionAttack`, as KOTOR's
/// AI does); its end of round and the dummy's `OnAttacked` tell the player.
/// `probe_round_attack` tells of the round and attacks again, as
/// `k_ai_master`'s round does.
fn library() -> crate::script::Library {
    use fmt_odyssey::ncs::{Const, Op};
    use script_ncs::probes::Asm;
    let attack = |asm: &mut Asm| {
        asm.op(Op::Const(Const::Int(0)))
            .op(Op::Const(Const::Int(0)))
            .op(Op::Const(Const::String(b"dummy".to_vec())))
            .op(Op::Action {
                routine: 2,
                args: 2,
            })
            .op(Op::Action {
                routine: 3,
                args: 2,
            });
    };
    let tell = |asm: &mut Asm, words: &str| {
        asm.op(Op::Const(Const::String(words.as_bytes().to_vec())))
            .op(Op::Action {
                routine: 1,
                args: 0,
            })
            .op(Op::Action {
                routine: 0,
                args: 2,
            });
    };
    let script = |parts: &dyn Fn(&mut Asm)| {
        let mut asm = Asm::default();
        parts(&mut asm);
        asm.op(Op::Retn);
        asm.build()
    };
    crate::probe::library(
        CATALOG,
        [
            ("probe_attack", script(&|asm| attack(asm))),
            ("probe_round", script(&|asm| tell(asm, "round"))),
            ("probe_attacked", script(&|asm| tell(asm, "attacked"))),
            (
                "probe_round_attack",
                script(&|asm| {
                    tell(asm, "round");
                    attack(asm);
                }),
            ),
            // `SetLightsaberPowered(OBJECT_SELF, TRUE, TRUE)`.
            (
                "probe_saber_held",
                script(&|asm| {
                    asm.op(Op::Const(Const::Int(0)))
                        .op(Op::Const(Const::Int(1)))
                        .op(Op::Const(Const::Int(1)))
                        .op(Op::Const(Const::Object(0)))
                        .op(Op::Action {
                            routine: 4,
                            args: 4,
                        });
                }),
            ),
        ],
    )
}

/// The probe's beast: a katarn's gore (base item 83, 1d6 slashing and
/// piercing), its blow 0.5 s into its attack, a metre past its 1.3 m.
const GORE: Strike = Strike {
    weapon: Some(Weapon {
        base_item: 83,
        delivery: Delivery::Melee,
        dice: 1,
        die: 6,
        damage_types: damage_types::SLASHING | damage_types::PIERCING,
        proficiency: None,
        focus: None,
        specialization: None,
        ..Weapon::UNARMED
    }),
    hit_distance: 1.0,
    hit_ms: 500,
};

/// How long the probe runs: 1500 steps of 8 ms.
const STEPS: u64 = 1500;
const STEP_MS: u64 = 8;

/// How long the beast is held after its first blow in the held run (ms):
/// well past `COMBAT_MS` and four rounds together, and the steps that run
/// covers.
const HOLD_MS: u64 = 30_000;
const HELD_STEPS: u64 = 6000;

/// How much later than a round a blow may land when the beast's pacing
/// between blows took it out of its reach at the round, and it stepped in
/// again first (ms; the room's walls are off the axes, and a body's path
/// round its quarry along them is its own).
const PACED_LATE_MS: u64 = 375;

/// What a run saw: when the beast began its attacks and when the dummy
/// took its hits (sim ms), the hits' weapons, what was said, until when
/// the beast was held, the last snapshot.
struct Ran {
    begun: Vec<u64>,
    struck: Vec<u64>,
    weapons: Vec<Option<Delivery>>,
    said: Vec<String>,
    held_until: Option<u64>,
    last: crate::Snapshot,
}

/// A creature `tag` on the proving ground, `x` metres along it (3 m across),
/// facing `yaw`, with `sheet` and its event `hooks` (hook, script).
fn creature(
    tag: &str,
    x: f32,
    yaw: f32,
    sheet: crate::combat::Sheet,
    hooks: &[(&str, &str)],
) -> crate::module::ObjectData {
    use crate::module::{ObjectData, ObjectKind};
    ObjectData {
        kind: ObjectKind::Creature,
        tag: tag.into(),
        sheet: Some(sheet),
        scripts: hooks
            .iter()
            .map(|(hook, script)| (hook.to_string(), script.to_string()))
            .collect(),
        ..ObjectData::unknown(tag, crate::determinism::ground_point(x, 3.0, 0.0), yaw)
    }
}

/// The proving ground with `placed` on it (the first's index returned),
/// factions `war` at war with each other, the rest neutral to each other,
/// and the probe's scripts if `scripted` (else the minds pick their own
/// foes); a world on it, one seat playing `sheet`.
fn world_with(
    placed: Vec<crate::module::ObjectData>,
    war: [u8; 2],
    scripted: bool,
    sheet: crate::combat::Sheet,
) -> Result<(super::super::SimWorld, u32), String> {
    use crate::module::{Placement, Repute};
    use crate::{Guests, WorldConfig};

    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    let first = data.placements.len() as u32;
    data.placements
        .extend(placed.into_iter().map(Placement::Own));
    data.repute = Repute::at_war(war);
    let level = ground.with_module(data);
    let level = if scripted {
        level.with_scripts(library())
    } else {
        level
    };
    let world = super::super::SimWorld::new(
        WorldConfig {
            seed: 1,
            seats: 1,
            wanderers: 0,
            spawn: [0.0; 3],
            yaw: 0.0,
            sheet,
            guests: Guests::Avatars,
        },
        level,
    )
    .map_err(|error| error.to_string())?;
    Ok((world, first))
}

/// The fight of the object placed `index`th in `snapshot`.
fn object_fight(snapshot: &crate::Snapshot, index: u32) -> Result<crate::combat::Fighter, String> {
    snapshot
        .entities
        .iter()
        .find(|entity| matches!(entity.kind, crate::Kind::Object(_, i) if i == index))
        .and_then(|entity| entity.fighter)
        .ok_or_else(|| format!("object {index} has no fight"))
}

/// A beast (`strike`; none: a humanoid with empty hands) and a dummy on the
/// proving ground, of factions at war, the beast told to attack the dummy.
/// `held`: its rounds attack again, and its first blow is followed by a
/// hold of `HOLD_MS` (a stun, as Fear holds), the dummy in its reach.
fn scenario(strike: Option<Strike>, held: bool) -> Result<Ran, String> {
    use crate::combat::Sheet;
    use crate::combat::sheet::factions::{FRIENDLY_1, HOSTILE_1};
    use crate::module::ObjectData;
    use crate::notice::Notice;
    use crate::{StepReason, TickInput};

    let beast = ObjectData {
        personal_space: 1.3,
        strike,
        ..creature(
            "beast",
            -4.5,
            0.0,
            Sheet {
                abilities: [16, 10, 10, 4, 6, 5],
                faction: HOSTILE_1,
                ..Sheet::commoner()
            },
            &[
                ("ScriptSpawn", "probe_attack"),
                (
                    "ScriptEndRound",
                    if held {
                        "probe_round_attack"
                    } else {
                        "probe_round"
                    },
                ),
            ],
        )
    };
    let dummy = creature(
        "dummy",
        -2.5,
        0.0,
        Sheet {
            faction: FRIENDLY_1,
            max_hp: 500,
            ..Sheet::commoner()
        },
        &[("ScriptAttacked", "probe_attacked")],
    );
    let (mut world, beast_at) = world_with(
        vec![beast, dummy],
        [HOSTILE_1, FRIENDLY_1],
        true,
        Sheet::dev_soldier(),
    )?;
    let dummy_at = beast_at + 1;
    let mut ran = Ran {
        begun: Vec::new(),
        struck: Vec::new(),
        weapons: Vec::new(),
        said: Vec::new(),
        held_until: None,
        last: world.snapshot(),
    };
    let beast_id = ran
        .last
        .entities
        .iter()
        .find(|entity| matches!(entity.kind, crate::Kind::Object(_, i) if i == beast_at))
        .map(|entity| entity.id)
        .ok_or("no beast")?;
    let (mut waited, mut hp) = (0, object_fight(&ran.last, dummy_at)?.hp);
    for tick in 1..=if held { HELD_STEPS } else { STEPS } {
        let snapshot = crate::step(
            &mut world,
            tick,
            &TickInput::default(),
            STEP_MS as u32,
            StepReason::Authority,
        );
        let (beast, dummy) = (
            object_fight(&snapshot, beast_at)?,
            object_fight(&snapshot, dummy_at)?,
        );
        if beast.weapon_wait_ms > waited {
            ran.begun.push(snapshot.time_ms);
        }
        if dummy.hp < hp {
            ran.struck.push(snapshot.time_ms);
            ran.weapons
                .push(dummy.newest().next().and_then(|event| event.weapon));
        }
        (waited, hp) = (beast.weapon_wait_ms, dummy.hp);
        if held && ran.held_until.is_none() && !ran.struck.is_empty() {
            let until = snapshot.time_ms + HOLD_MS;
            let mut fighters = world.world.query::<(&Id, &mut Fighting)>();
            for (id, mut fighting) in fighters.iter_mut(&mut world.world) {
                if id.0 == beast_id {
                    fighting.0.force.stunned_until = until;
                }
            }
            ran.held_until = Some(until);
        }
        ran.said.extend(
            world
                .take_notices()
                .into_iter()
                .filter_map(|(_, notice)| match notice {
                    Notice::Said(words) => Some(words),
                    _ => None,
                }),
        );
        ran.last = snapshot;
    }
    Ok(ran)
}

/// The checks of a beast's attack (see the module documentation), on the
/// proving ground: a creature with a strike its script sets on a dummy
/// lands blows of its creature weapon, each its hit moment into an attack,
/// a round apart; its rounds run (`ScriptEndRound`) and the dummy hears it
/// was attacked (`ScriptAttacked`); the same creature without a strike (a
/// humanoid with empty hands) lands its fists' blows instead; two runs end
/// the same.
/// For the approved test `beast_strikes`.
pub fn probes() -> Vec<(&'static str, Result<String, String>)> {
    let check = |ok: bool, what: String| if ok { Ok(what) } else { Err(what) };
    let ran = match scenario(Some(GORE), false) {
        Ok(ran) => ran,
        Err(why) => return vec![("strike_scenario", Err(why))],
    };
    let again = scenario(Some(GORE), false).map(|again| again.last.hash());
    let bare = scenario(None, false).map(|bare| bare.weapons);
    let apart: Vec<u64> = ran
        .struck
        .windows(2)
        .map(|pair| pair[1] - pair[0])
        .collect();
    // How long after its attack began each blow landed.
    let into: Vec<u64> = ran
        .struck
        .iter()
        .filter_map(|hit| {
            let begun = ran.begun.iter().rev().find(|begun| *begun < hit)?;
            Some(hit - begun)
        })
        .collect();
    let said = |words: &str| ran.said.iter().filter(|said| *said == words).count();
    let hit_ms = u64::from(GORE.hit_ms);
    vec![
        (
            "lands_blows",
            check(
                ran.struck.len() >= 3,
                format!("hits at {:?} ms, attacks at {:?}", ran.struck, ran.begun),
            ),
        ),
        (
            "a_round_apart",
            check(
                !apart.is_empty()
                    && apart.contains(&ROUND_MS)
                    && apart
                        .iter()
                        .all(|gap| (ROUND_MS..ROUND_MS + PACED_LATE_MS).contains(gap)),
                format!("{apart:?} ms apart"),
            ),
        ),
        (
            "at_the_hit_moment",
            check(
                into.len() == ran.struck.len()
                    && into
                        .iter()
                        .all(|ms| (hit_ms..hit_ms + STEP_MS).contains(ms)),
                format!("{into:?} ms into their attacks"),
            ),
        ),
        (
            "creature_weapon",
            check(
                ran.weapons
                    .iter()
                    .all(|weapon| *weapon == Some(Delivery::Melee)),
                format!("{:?}", ran.weapons),
            ),
        ),
        (
            "rounds_run",
            check(said("round") >= 2, format!("{:?}", ran.said)),
        ),
        (
            "target_hears",
            check(said("attacked") >= 1, format!("{:?}", ran.said)),
        ),
        (
            "humanoid_punches",
            check(
                bare.as_ref().is_ok_and(|weapons| {
                    !weapons.is_empty()
                        && weapons
                            .iter()
                            .all(|weapon| *weapon == Some(Delivery::Unarmed))
                }),
                format!("its hits' weapons {bare:?}"),
            ),
        ),
        (
            "same_every_run",
            check(
                again.as_ref().is_ok_and(|hash| *hash == ran.last.hash()),
                format!("{again:?}"),
            ),
        ),
    ]
}

/// The checks of a fight a hold interrupts (docs/EVENTS.md), on the
/// proving ground: a beast its script sets on a dummy, its rounds
/// attacking again as `k_ai_master`'s do, held for `HOLD_MS` after its
/// first blow with the dummy in its reach; its rounds run on through the
/// hold, as its attack's do (KOTOR's attack runs a round, blow or none),
/// and once the hold is over it lands blows again; two runs end the same.
/// For the approved test `fight_through_a_hold`.
pub fn hold_probes() -> Vec<(&'static str, Result<String, String>)> {
    let check = |ok: bool, what: String| if ok { Ok(what) } else { Err(what) };
    let ran = match scenario(Some(GORE), true) {
        Ok(ran) => ran,
        Err(why) => return vec![("hold_scenario", Err(why))],
    };
    let unseen = unseen_scenario();
    let again = scenario(Some(GORE), true).map(|again| again.last.hash());
    let Some(until) = ran.held_until else {
        return vec![("hold_scenario", Err("no first blow".into()))];
    };
    let after = ran.struck.iter().filter(|hit| **hit > until).count();
    // A round every `ROUND_MS` through the hold at least.
    let rounds = ran.said.iter().filter(|said| *said == "round").count();
    let through = (HOLD_MS / ROUND_MS) as usize;
    vec![
        (
            "rounds_through_the_hold",
            check(
                rounds >= through,
                format!("{rounds} rounds, {through} in the hold"),
            ),
        ),
        (
            "fights_on_after_the_hold",
            check(
                after >= 2,
                format!("held to {until} ms, hits at {:?}", ran.struck),
            ),
        ),
        (
            "attack_keeps_an_unseen_target",
            match unseen {
                Ok((lost, task)) => check(
                    lost == 0 && task == crate::combat::ai::Task::Engage,
                    format!("24 s behind a shut door: {lost} steps without it, task {task:?}"),
                ),
                Err(why) => Err(why),
            },
        ),
        (
            "same_every_run",
            check(
                again.as_ref().is_ok_and(|hash| *hash == ran.last.hash()),
                format!("{again:?}"),
            ),
        ),
    ]
}

/// How long the shut-door attack runs: 3000 steps of 8 ms, past four
/// rounds (the attack's old cap while it closed in).
const UNSEEN_STEPS: u64 = 3000;

/// A beast told to attack a dummy it cannot see, the ground's door shut
/// between them (the beast 3 m before it, the dummy 3 m past it): the
/// steps after its first 2 s its mind held another target than the dummy,
/// or none, and its task at the end. KOTOR's attack keeps its target, seen
/// or not, until it reaches it.
fn unseen_scenario() -> Result<(usize, crate::combat::ai::Task), String> {
    use crate::combat::Sheet;
    use crate::combat::sheet::factions::{FRIENDLY_1, HOSTILE_1};
    use crate::module::ObjectData;
    use crate::{StepReason, TickInput};

    let at = |x: f32| crate::determinism::ground_point(x, 0.0, 0.0);
    let beast = ObjectData {
        strike: Some(GORE),
        position: at(-3.0),
        ..creature(
            "beast",
            -3.0,
            0.0,
            Sheet {
                faction: HOSTILE_1,
                ..Sheet::commoner()
            },
            &[("ScriptSpawn", "probe_attack")],
        )
    };
    let dummy = ObjectData {
        position: at(3.0),
        ..creature(
            "dummy",
            3.0,
            0.0,
            Sheet {
                faction: FRIENDLY_1,
                ..Sheet::commoner()
            },
            &[],
        )
    };
    let (mut world, beast_at) = world_with(
        vec![beast, dummy],
        [HOSTILE_1, FRIENDLY_1],
        true,
        Sheet::dev_soldier(),
    )?;
    let mut last = world.snapshot();
    let dummy = last
        .entities
        .iter()
        .find(|entity| matches!(entity.kind, crate::Kind::Object(_, i) if i == beast_at + 1))
        .map(|entity| entity.id)
        .ok_or("no dummy")?;
    let mut lost = 0;
    for tick in 1..=UNSEEN_STEPS {
        last = crate::step(
            &mut world,
            tick,
            &TickInput::default(),
            STEP_MS as u32,
            StepReason::Authority,
        );
        if last.time_ms > 2000 && object_fight(&last, beast_at)?.target != Some(dummy) {
            lost += 1;
        }
    }
    Ok((lost, object_fight(&last, beast_at)?.mind.task))
}

/// How long the brawl runs: 6000 steps of 8 ms.
const BRAWL_STEPS: u64 = 6000;

/// A blow taken: when, from whom, with what.
type Taken = (u64, Option<SimId>, Option<Delivery>);

/// What a brawl saw: the punches each side began (when, and what its torso
/// showed in the snapshot), the blows each took (when, from whom, with
/// what), when the foe died, the last snapshot.
struct Brawl {
    thrown: [Vec<(u64, Anim)>; 2],
    taken: [Vec<Taken>; 2],
    ids: [SimId; 2],
    died: Option<u64>,
    last: crate::Snapshot,
}

/// The player, bare-handed, 1.5 m from a bare-handed foe of a faction at
/// war with it on the proving ground (no scripts: the foe's mind picks the
/// player), facing it and holding attack.
fn brawl() -> Result<Brawl, String> {
    use crate::combat::Sheet;
    use crate::combat::sheet::factions::{HOSTILE_1, PLAYER};
    use crate::{StepReason, TickInput};

    let foe = creature(
        "brawler",
        -2.5,
        crate::determinism::TURN_DEGREES + 180.0,
        Sheet {
            faction: HOSTILE_1,
            max_hp: 20,
            ..Sheet::commoner()
        },
        &[],
    );
    let player_sheet = Sheet {
        max_hp: 200,
        weapons: [None; 2],
        ..Sheet::dev_soldier()
    };
    let (mut world, foe_at) = world_with(vec![foe], [PLAYER, HOSTILE_1], false, player_sheet)?;
    let first = world.snapshot();
    let foe_id = first
        .entities
        .iter()
        .find(|entity| matches!(entity.kind, crate::Kind::Object(_, i) if i == foe_at))
        .map(|entity| entity.id)
        .ok_or("no foe")?;
    let player_id = first
        .body(crate::Seat(0))
        .map(|body| body.id)
        .ok_or("no player")?;
    // The player on the foe's floor, 1.5 m back along the ground's x.
    let back = math::sub(
        crate::determinism::ground_point(0.0, 0.0, 0.0),
        crate::determinism::ground_point(1.5, 0.0, 0.0),
    );
    let mut players = world.world.query::<(&Id, &mut Player)>();
    let foe_origin = players
        .iter(&world.world)
        .find(|(id, _)| id.0 == foe_id)
        .map(|(_, player)| player.0.origin)
        .ok_or("the foe has no body")?;
    for (id, mut player) in players.iter_mut(&mut world.world) {
        if id.0 == player_id {
            player.0.origin = math::mad(foe_origin, 1.0 / UNIT, back);
            player.0.view_offset = [0; 3];
            player.0.view = [0.0, crate::determinism::TURN_DEGREES, 0.0];
        }
    }
    let ids = [player_id, foe_id];
    let mut seen = Brawl {
        thrown: [Vec::new(), Vec::new()],
        taken: [Vec::new(), Vec::new()],
        ids,
        died: None,
        last: world.snapshot(),
    };
    let fights = |snapshot: &crate::Snapshot| {
        ids.map(|id| {
            snapshot
                .get(id)
                .and_then(|entity| Some((entity.fighter?, entity.player?, entity.position)))
        })
    };
    let mut before = fights(&seen.last);
    for tick in 1..=BRAWL_STEPS {
        // Facing the foe wherever it stands, attack held.
        let [me, foe] = before.map(|fight| fight.map(|(.., at)| at));
        let yaw = match (me, foe) {
            (Some(me), Some(foe)) => math::atan2_deg(foe[1] - me[1], foe[0] - me[0]),
            _ => crate::determinism::TURN_DEGREES,
        };
        let mut input = TickInput::default();
        input.insert(
            crate::Seat(0),
            crate::Command {
                angles: crate::moves::view_angles(0.0, yaw),
                buttons: buttons::ATTACK,
                ..crate::Command::default()
            },
        );
        let snapshot = crate::step(
            &mut world,
            tick,
            &input,
            STEP_MS as u32,
            StepReason::Authority,
        );
        let now = fights(&snapshot);
        for side in 0..2 {
            let (Some((was, ..)), Some((fighter, ps, _))) = (before[side], now[side]) else {
                return Err(format!("{:?} gone at {}", ids[side], snapshot.time_ms));
            };
            if fighter.weapon_wait_ms > was.weapon_wait_ms {
                seen.thrown[side].push((snapshot.time_ms, ps.torso.anim));
            }
            if fighter.hp < was.hp {
                let event = fighter.newest().next();
                seen.taken[side].push((
                    snapshot.time_ms,
                    event.and_then(|event| event.by),
                    event.and_then(|event| event.weapon),
                ));
            }
            if side == 1 && was.alive() && !fighter.alive() {
                seen.died = Some(snapshot.time_ms);
            }
        }
        before = now;
        seen.last = snapshot;
    }
    Ok(seen)
}

/// The checks of a bare-handed brawl (see the module documentation), on
/// the proving ground: the player, its hands empty and attack held, throws
/// the left punch, then the right, in turn, a round apart, each shown on
/// its torso in the snapshot; its blows land halfway through its punches
/// and punch the foe to death, every one its fists'; the foe, its hands
/// empty, closes in and punches the player, its fists' blows from its own
/// punches, which its snapshot shows; two runs end the same. For the
/// approved test `fists_brawl`.
pub fn fist_probes() -> Vec<(&'static str, Result<String, String>)> {
    let check = |ok: bool, what: String| if ok { Ok(what) } else { Err(what) };
    let seen = match brawl() {
        Ok(seen) => seen,
        Err(why) => return vec![("brawl_scenario", Err(why))],
    };
    let again = brawl().map(|again| again.last.hash());
    let [player, foe] = &seen.thrown;
    let [player_took, foe_took] = &seen.taken;
    let [player_id, foe_id] = seen.ids;
    let alternate = |thrown: &[(u64, Anim)]| {
        thrown.len() >= 2
            && thrown
                .iter()
                .enumerate()
                .all(|(n, (_, anim))| *anim == punch(n as u8))
    };
    let apart: Vec<u64> = player
        .windows(2)
        .map(|pair| pair[1].0 - pair[0].0)
        .collect();
    // How long after the player's punch began each of its blows landed.
    let into: Vec<u64> = foe_took
        .iter()
        .filter_map(|(hit, ..)| {
            let (begun, anim) = player.iter().rev().find(|(begun, _)| begun < hit)?;
            (hit - begun).checked_sub(u64::from(punch_hit_ms(&AnimTable::stand_in(), *anim)))
        })
        .collect();
    let fists_of = |took: &[Taken], by: SimId| {
        !took.is_empty()
            && took
                .iter()
                .all(|(_, from, weapon)| *from == Some(by) && *weapon == Some(Delivery::Unarmed))
    };
    vec![
        (
            "player_punches_in_turn",
            check(
                alternate(player),
                format!("the player's punches {player:?}"),
            ),
        ),
        (
            "a_round_apart",
            check(
                !apart.is_empty() && apart.iter().all(|gap| *gap == ROUND_MS),
                format!("{apart:?} ms apart"),
            ),
        ),
        (
            "halfway_through_the_punch",
            check(
                !into.is_empty()
                    && into.len() == foe_took.len()
                    && into.iter().all(|ms| *ms < STEP_MS),
                format!("{into:?} ms past the punches' middles"),
            ),
        ),
        (
            "punched_to_death",
            check(
                seen.died.is_some() && fists_of(foe_took, player_id),
                format!("dead at {:?}, blows {foe_took:?}", seen.died),
            ),
        ),
        (
            "foe_punches_back",
            check(
                fists_of(player_took, foe_id) && alternate(foe),
                format!("blows {player_took:?}, punches {foe:?}"),
            ),
        ),
        (
            "same_every_run",
            check(
                again.as_ref().is_ok_and(|hash| *hash == seen.last.hash()),
                format!("{again:?}"),
            ),
        ),
        ("punch_lands_from_a_doorway", doorway_punch()),
    ]
}

/// A thrower standing inside a bashable body's box (a force field's
/// doorway) touches it, so whichever way it faces the body is in front of
/// its punch (`nearest`, `in_front`).
fn doorway_punch() -> Result<String, String> {
    let origin = [1422.7007, 7747.086, 295.0653];
    let door = collision::Body {
        entity: 7 | super::super::BODY,
        min: [35.2, 196.2, 6.9],
        max: [36.5, 197.4, 9.9],
        contents: collision::Contents::default(),
    };
    let faced: Vec<bool> = [0.0f32, 90.0, 175.3, 270.0]
        .into_iter()
        .map(|yaw| {
            let ps = motion::MotionState::standing(origin, yaw);
            let body = nearest(&door, ps.origin);
            in_front(&ps, &[body], SimId(1), None, |_| true).is_some_and(|met| met.id == SimId(7))
        })
        .collect();
    let detail = format!("met facing 0, 90, 175.3, 270 degrees: {faced:?}");
    if faced.iter().all(|met| *met) {
        Ok(detail)
    } else {
        Err(detail)
    }
}

/// How long the sheathing run lasts: 2700 steps of 8 ms.
const SHEATH_STEPS: u64 = 2700;

/// The sheathing run's presses of attack (sim ms, from and to): a click
/// draws (the first during the 8 s rule's put-away), the last, held, draws
/// and swings.
const CLICKS: [(u64, u64); 3] = [(1000, 1100), (10_000, 10_100), (15_000, 16_000)];

/// When the sheathing run's player holsters: idle, its 8 s still running,
/// then mid-swing; and when it takes its left hand's weapon (a blaster)
/// out (sim ms).
const HOLSTER_MS: [u64; 2] = [14_000, 16_400];
const HAND_MS: u64 = 20_000;

/// What the player presses in the sheathing run at `ms`.
fn sheath_input(ms: u64, yaw: f32) -> crate::TickInput {
    let attack = CLICKS.iter().any(|(from, to)| (*from..*to).contains(&ms));
    let mut input = crate::TickInput::default();
    input.insert(
        crate::Seat(0),
        crate::Command {
            angles: crate::moves::view_angles(0.0, yaw),
            buttons: if attack { buttons::ATTACK } else { 0 },
            weapon: u8::from(ms >= HAND_MS),
            ..crate::Command::default()
        },
    );
    if HOLSTER_MS.contains(&ms) {
        input.act(crate::Seat(0), crate::action::Action::Holster);
    }
    input
}

/// A step of the sheathing run: its time, a fighter's sheath, its saber lit,
/// and its saber move.
type SheathStep = (u64, Sheath, bool, SaberMove);

/// A move that swings: not ready, nor a draw or put-away flourish.
fn swings(saber_move: SaberMove) -> bool {
    !matches!(
        saber_move,
        SaberMove::READY | SaberMove::NONE | SaberMove::DRAW | SaberMove::PUTAWAY
    )
}

/// What the sheathing run saw, the player's steps and the scripted Jedi's,
/// and the last snapshot.
struct Sheathing {
    player: Vec<SheathStep>,
    held: Vec<SheathStep>,
    last: crate::Snapshot,
}

/// The player, a Jedi with its saber in hand, alone on the proving ground
/// beside a neutral Jedi whose spawn script holds its saber lit
/// (`SetLightsaberPowered` with the override), pressing `sheath_input`.
fn sheathing() -> Result<Sheathing, String> {
    use crate::combat::Sheet;
    use crate::combat::sheet::factions::{HOSTILE_1, NEUTRAL, PLAYER};

    let jedi = creature(
        "held",
        4.0,
        crate::determinism::TURN_DEGREES,
        Sheet {
            faction: NEUTRAL,
            ..Sheet::dev_jedi()
        },
        &[("ScriptSpawn", "probe_saber_held")],
    );
    let (mut world, jedi_at) =
        world_with(vec![jedi], [PLAYER, HOSTILE_1], true, Sheet::dev_jedi())?;
    let first = world.snapshot();
    let jedi_id = first
        .entities
        .iter()
        .find(|entity| matches!(entity.kind, crate::Kind::Object(_, i) if i == jedi_at))
        .map(|entity| entity.id)
        .ok_or("no Jedi")?;
    let player_id = first
        .body(crate::Seat(0))
        .map(|body| body.id)
        .ok_or("no player")?;
    let mut seen = Sheathing {
        player: Vec::new(),
        held: Vec::new(),
        last: first,
    };
    for tick in 1..=SHEATH_STEPS {
        let input = sheath_input(seen.last.time_ms, crate::determinism::TURN_DEGREES);
        seen.last = crate::step(
            &mut world,
            tick,
            &input,
            STEP_MS as u32,
            crate::StepReason::Authority,
        );
        for (id, into) in [(player_id, &mut seen.player), (jedi_id, &mut seen.held)] {
            let fighter = seen
                .last
                .get(id)
                .and_then(|entity| entity.fighter)
                .ok_or(format!("{id:?} gone at {}", seen.last.time_ms))?;
            into.push((
                seen.last.time_ms,
                fighter.sheath,
                fighter.saber.on,
                fighter.saber.current_move,
            ));
        }
    }
    Ok(seen)
}

/// A hostile Jedi's charge: when its saber lit, how far from the player
/// against where it started, and whether it swung then.
struct Charge {
    lit_at: Option<(f32, bool)>,
    start: f32,
}

/// A Jedi of a faction at war with the player 3 m from it on the proving
/// ground, its saber unlit (no scripts: its mind picks the player and
/// closes in), the player standing still with a blaster.
fn charge() -> Result<Charge, String> {
    use crate::combat::Sheet;
    use crate::combat::sheet::factions::{HOSTILE_1, PLAYER};

    let foe = creature(
        "charger",
        -1.0,
        crate::determinism::TURN_DEGREES + 180.0,
        Sheet {
            faction: HOSTILE_1,
            ..Sheet::dev_jedi()
        },
        &[],
    );
    let (mut world, foe_at) = world_with(
        vec![foe],
        [PLAYER, HOSTILE_1],
        false,
        crate::combat::Sheet::dev_soldier(),
    )?;
    let first = world.snapshot();
    let foe_id = first
        .entities
        .iter()
        .find(|entity| matches!(entity.kind, crate::Kind::Object(_, i) if i == foe_at))
        .map(|entity| entity.id)
        .ok_or("no foe")?;
    let player_id = first
        .body(crate::Seat(0))
        .map(|body| body.id)
        .ok_or("no player")?;
    // The player on the foe's floor, 3 m back along the ground's x, where
    // the brawl's player stands (the ground hides it from farther along).
    let back = math::sub(
        crate::determinism::ground_point(0.0, 0.0, 0.0),
        crate::determinism::ground_point(3.0, 0.0, 0.0),
    );
    let mut players = world.world.query::<(&Id, &mut Player)>();
    let foe_origin = players
        .iter(&world.world)
        .find(|(id, _)| id.0 == foe_id)
        .map(|(_, player)| player.0.origin)
        .ok_or("the foe has no body")?;
    for (id, mut player) in players.iter_mut(&mut world.world) {
        if id.0 == player_id {
            player.0.origin = math::mad(foe_origin, 1.0 / UNIT, back);
        }
    }
    let mut last = world.snapshot();
    let apart = |snapshot: &crate::Snapshot| -> Option<f32> {
        let at = |id| snapshot.get(id).map(|entity| entity.position);
        Some(math::length(math::sub(at(foe_id)?, at(player_id)?)))
    };
    let start = apart(&last).ok_or("no bodies")?;
    for tick in 1..=STEPS {
        let mut input = crate::TickInput::default();
        input.insert(crate::Seat(0), crate::Command::default());
        last = crate::step(
            &mut world,
            tick,
            &input,
            STEP_MS as u32,
            crate::StepReason::Authority,
        );
        let fighter = last
            .get(foe_id)
            .and_then(|entity| entity.fighter)
            .ok_or("the foe is gone")?;
        if fighter.saber.on {
            return Ok(Charge {
                lit_at: apart(&last).map(|apart| (apart, swings(fighter.saber.current_move))),
                start,
            });
        }
    }
    Ok(Charge {
        lit_at: None,
        start,
    })
}

/// When a gun's first bolt left (sim ms): the player alone on the proving
/// ground with a blaster (base item 12), away as the game has it out of a
/// fight, holding attack from `GUN_PRESS_MS`.
fn first_bolt() -> Result<Option<u64>, String> {
    use crate::combat::sheet::factions::{HOSTILE_1, PLAYER};
    let (mut world, _) = world_with(
        Vec::new(),
        [PLAYER, HOSTILE_1],
        false,
        crate::combat::Sheet::dev_soldier(),
    )?;
    for tick in 1..=GUN_STEPS {
        let now = tick * STEP_MS;
        let mut input = crate::TickInput::default();
        input.insert(
            crate::Seat(0),
            crate::Command {
                buttons: if now >= GUN_PRESS_MS {
                    buttons::ATTACK
                } else {
                    0
                },
                ..crate::Command::default()
            },
        );
        let snapshot = crate::step(
            &mut world,
            tick,
            &input,
            STEP_MS as u32,
            crate::StepReason::Authority,
        );
        if snapshot
            .entities
            .iter()
            .any(|entity| entity.kind == crate::Kind::Missile)
        {
            return Ok(Some(snapshot.time_ms));
        }
    }
    Ok(None)
}

/// The blade-taken runs: the click that lights the saber, when the holster
/// key starts its put-away flourish, how long they run (steps).
const TAKEN_CLICK: (u64, u64) = (1000, 1100);
const TAKEN_HOLSTER_MS: u64 = 3000;
const TAKEN_STEPS: u64 = 1500;

/// The player, a Jedi alone on the proving ground, lights its saber with a
/// click; when `holstered`, the holster key starts the put-away flourish
/// at `TAKEN_HOLSTER_MS`. A step later its blades are taken from its hands
/// as a script's `ActionUnequipItem` takes them (its sheet replaced, as
/// `Change::Sheet` does). Each step after: its time, sheath, saber lit,
/// saber move; and whether its weapon is away at the end.
fn blade_taken(holstered: bool) -> Result<(Vec<SheathStep>, bool), String> {
    use crate::combat::Sheet;
    use crate::combat::sheet::factions::{HOSTILE_1, PLAYER};

    let (mut world, _) = world_with(Vec::new(), [PLAYER, HOSTILE_1], false, Sheet::dev_jedi())?;
    let player_id = world
        .snapshot()
        .body(crate::Seat(0))
        .map(|body| body.id)
        .ok_or("no player")?;
    let take_ms = TAKEN_HOLSTER_MS + STEP_MS;
    let mut steps = Vec::new();
    let mut away = false;
    for tick in 1..=TAKEN_STEPS {
        let now = (tick - 1) * STEP_MS;
        if now == take_ms {
            let mut sheets = world.world.query::<(&Id, &mut PlayerSheet)>();
            for (id, mut sheet) in sheets.iter_mut(&mut world.world) {
                if id.0 == player_id {
                    sheet.0.weapons = [None, None];
                }
            }
        }
        let mut input = crate::TickInput::default();
        input.insert(
            crate::Seat(0),
            crate::Command {
                buttons: if (TAKEN_CLICK.0..TAKEN_CLICK.1).contains(&now) {
                    buttons::ATTACK
                } else {
                    0
                },
                ..crate::Command::default()
            },
        );
        if holstered && now == TAKEN_HOLSTER_MS {
            input.act(crate::Seat(0), crate::action::Action::Holster);
        }
        let snapshot = crate::step(
            &mut world,
            tick,
            &input,
            STEP_MS as u32,
            crate::StepReason::Authority,
        );
        let fighter = snapshot
            .get(player_id)
            .and_then(|entity| entity.fighter)
            .ok_or("the player has no fight")?;
        if snapshot.time_ms > take_ms {
            steps.push((
                snapshot.time_ms,
                fighter.sheath,
                fighter.saber.on,
                fighter.saber.current_move,
            ));
        }
        away = fighter.weapon_away(snapshot.time_ms);
    }
    Ok((steps, away))
}

/// A blade taken from the hand some other way than a change of hands (a
/// script's `ActionUnequipItem`: K1's Duros after the Taris raid): taken
/// in its put-away flourish, the flourish ends with it; taken lit, it goes
/// out; either way, a second on, nothing is lit or under way, and once its
/// 8 s are out its weapon is away (`weapons::armed`: not held).
pub fn blade_taken_probes() -> Vec<(&'static str, Result<String, String>)> {
    let check = |ok: bool, what: String| if ok { Ok(what) } else { Err(what) };
    let settled = |steps: &[SheathStep]| {
        let after = steps.first().map_or(0, |step| step.0) + 1000;
        steps
            .iter()
            .filter(|step| step.0 >= after)
            .all(|&(_, _, on, saber_move)| {
                !on && matches!(saber_move, SaberMove::READY | SaberMove::NONE)
            })
    };
    [("taken_in_its_put_away", true), ("taken_lit", false)]
        .into_iter()
        .map(|(name, holstered)| {
            let result = match blade_taken(holstered) {
                Ok((steps, away)) => check(
                    settled(&steps) && away && !steps.is_empty(),
                    format!(
                        "first {:?}, last {:?}, away at the end {away}",
                        steps.first(),
                        steps.last()
                    ),
                ),
                Err(why) => Err(why),
            };
            (name, result)
        })
        .collect()
}

/// When the gun run's attack is pressed, and how long it runs (steps).
const GUN_PRESS_MS: u64 = 1000;
const GUN_STEPS: u64 = 250;

/// The checks of putting weapons away (docs/COMBAT.md, "Weapons out and
/// away"), on the proving ground: a saber a click lit stays lit 8 s after
/// the click (`COMBAT_MS`) and goes out; the holster key puts it out at
/// once, though its 8 s still run, and mid-swing as the swing ends; an
/// attack lights it again; a weapon taken out is out whatever held the
/// last; a saber a script holds lit stays lit long past 8 s; a foe's
/// saber is lit once it spots the player, out of a swing's reach; a gun
/// away is raised before its first bolt; the action's
/// bytes read back; two runs end the same. For the approved test
/// `holster_weapon`.
pub fn sheath_probes() -> Vec<(&'static str, Result<String, String>)> {
    use crate::action::Action;
    let check = |ok: bool, what: String| if ok { Ok(what) } else { Err(what) };
    let seen = match sheathing() {
        Ok(seen) => seen,
        Err(why) => return vec![("sheath_scenario", Err(why))],
    };
    let again = sheathing().map(|again| again.last.hash());
    let player = &seen.player;
    // When the saber lit and went out, from step to step.
    let turned = |on: bool| -> Vec<u64> {
        player
            .windows(2)
            .filter(|pair| pair[0].2 != on && pair[1].2 == on)
            .map(|pair| pair[1].0)
            .collect()
    };
    let (lit, out) = (turned(true), turned(false));
    let lit_in = |(from, to): (u64, u64)| lit.iter().any(|ms| (from..=to).contains(ms));
    let out_in = |from: u64, to: u64| out.iter().copied().find(|ms| (from..=to).contains(ms));
    let at = |ms: u64| player.iter().find(|(time, ..)| *time >= ms).copied();
    let [idle, mid_swing] = HOLSTER_MS;
    let first_off = CLICKS[0].1 + COMBAT_MS;
    let swing_ends = player
        .iter()
        .find(|(ms, .., saber_move)| *ms > mid_swing && !swings(*saber_move))
        .map(|(ms, ..)| *ms);
    let held_long = seen
        .held
        .iter()
        .filter(|(ms, ..)| *ms > COMBAT_MS + 2000)
        .all(|(_, sheath, on, _)| *on && *sheath == Sheath::Lit);
    let bytes = Action::from_bytes(&Action::Holster.to_bytes());
    let charge = charge();
    let bolt = first_bolt();
    vec![
        (
            "lit_by_a_click",
            check(
                CLICKS.iter().all(|click| lit_in(*click)),
                format!("lit at {lit:?}"),
            ),
        ),
        (
            "out_8_s_after_the_click",
            check(
                out_in(CLICKS[0].0, first_off + 2 * STEP_MS).is_some_and(|ms| ms >= first_off),
                format!("out at {out:?}, 8 s after the click at {first_off}"),
            ),
        ),
        (
            "holstered_at_once",
            check(
                out_in(idle, idle + STEP_MS).is_some()
                    && at(idle + STEP_MS)
                        .is_some_and(|(_, sheath, on, _)| sheath == Sheath::Holstered && !on),
                format!("out at {out:?}, then {:?}", at(idle + STEP_MS)),
            ),
        ),
        (
            "flourishes",
            check(
                at(idle + STEP_MS).is_some_and(|step| step.3 == SaberMove::PUTAWAY)
                    && at(CLICKS[1].0 + STEP_MS).is_some_and(|step| step.3 == SaberMove::DRAW),
                format!(
                    "holstered {:?}, clicked {:?}",
                    at(idle + STEP_MS),
                    at(CLICKS[1].0 + STEP_MS)
                ),
            ),
        ),
        (
            "an_attack_draws_it",
            check(
                at(CLICKS[2].1).is_some_and(|(_, sheath, on, _)| sheath == Sheath::Fight && on),
                format!("{:?}", at(CLICKS[2].1)),
            ),
        ),
        (
            "mid_swing_as_the_swing_ends",
            check(
                at(mid_swing + STEP_MS).is_some_and(|(_, sheath, on, saber_move)| {
                    sheath == Sheath::Holstering && on && swings(saber_move)
                }) && swing_ends.is_some_and(|ends| out_in(ends, ends + STEP_MS).is_some()),
                format!(
                    "{:?}, the swing over at {swing_ends:?}, out at {out:?}",
                    at(mid_swing + STEP_MS)
                ),
            ),
        ),
        (
            "a_weapon_taken_out_is_out",
            check(
                at(HAND_MS + 1000).is_some_and(|(_, sheath, ..)| sheath == Sheath::Fight),
                format!("the left hand out {:?}", at(HAND_MS + 1000)),
            ),
        ),
        (
            "a_script_holds_it_lit",
            check(
                held_long && seen.held.last().is_some_and(|(ms, ..)| *ms > COMBAT_MS * 2),
                format!("the held Jedi's last {:?}", seen.held.last()),
            ),
        ),
        (
            "a_foe_comes_lit",
            match charge {
                Ok(Charge {
                    lit_at: Some((apart, false)),
                    start,
                }) if apart > crate::combat::mind::STRIKE * UNIT => Ok(format!(
                    "lit {apart:.2} m from the player of {start:.2}, out of a swing's reach"
                )),
                other => Err(format!(
                    "{:?}",
                    other.map(|charge| (charge.lit_at, charge.start))
                )),
            },
        ),
        (
            "a_gun_rises_before_it_fires",
            check(
                bolt.as_ref()
                    .is_ok_and(|bolt| bolt.is_some_and(|ms| ms + STEP_MS >= GUN_PRESS_MS + 250)),
                format!("the first bolt at {bolt:?}, attack from {GUN_PRESS_MS}"),
            ),
        ),
        (
            "action_bytes",
            check(bytes == Ok(Action::Holster), format!("{bytes:?}")),
        ),
        (
            "same_every_run",
            check(
                again.as_ref().is_ok_and(|hash| *hash == seen.last.hash()),
                format!("{again:?}"),
            ),
        ),
    ]
}
