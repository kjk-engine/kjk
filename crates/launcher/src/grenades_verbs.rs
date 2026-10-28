//! `kjk combat grenades`: grenades without a window (I22 section 7;
//! `sim::combat::grenades`). The rule's table and its pieces (the save, the
//! C11 hold, poison's course, the weakness, the ion grenade on droids); a
//! throw in the open arena (the charge, the arc, the bounces and the rest,
//! the fuse, the count carried); then in a module, a soldier 160 units from
//! a hostile creature (end_m01aa's Sith soldier by default, given HP to
//! spare) throwing each kind straight at it, so it bursts on the body:
//! KOTOR's damage, halved by a save, and the secondary effect; a grenade at
//! the thrower's own feet (friendly fire off spares it); and a creature
//! given grenades throwing them by itself. Every scene's recording replays.
//! Writes `facts.json`.

use std::process::ExitCode;

use sim::combat::damage::{self, Blow, FriendlyFire, Relation, Source};
use sim::combat::grenades::{self, Secondary};
use sim::combat::items::{self, Carried, KINDS, Pack, Poisoned};
use sim::combat::spells::*;
use sim::combat::sheet::{Ability, races};
use sim::combat::{Location, Sheet, mind};
use sim::{Command, SimRng, Snapshot, buttons};
use vfs::Discovery;

use crate::scenes::{Report, Run, Stage, fighter, player, seat0};

/// The dev soldier carrying `count` grenades of `row`, with HP to spare.
fn thrower(row: u16, count: u8) -> Sheet {
    let mut sheet = Sheet::dev_soldier();
    sheet.max_hp = 500;
    sheet.items = [None; KINDS];
    sheet.items[0] = Some(Carried { row, count });
    sheet
}

/// The grenades in flight in `snapshot`, by id.
fn lobs(snapshot: &Snapshot) -> Vec<(sim::SimId, sim::combat::Missile)> {
    snapshot
        .entities
        .iter()
        .filter_map(|entity| {
            let missile = entity.missile.filter(|missile| missile.lob.is_some())?;
            Some((entity.id, missile))
        })
        .collect()
}

/// The first grenade's flight: the script times (ms after the start) of
/// the first and the last step whose snapshot has it.
fn flight(run: &Run, start: &Snapshot) -> Option<(u64, u64)> {
    let first = run
        .snapshots
        .iter()
        .find_map(|snapshot| lobs(snapshot).first().map(|(id, _)| *id))?;
    let times: Vec<u64> = run
        .snapshots
        .iter()
        .filter(|snapshot| lobs(snapshot).iter().any(|(id, _)| *id == first))
        .map(|snapshot| snapshot.time_ms - start.time_ms - 8)
        .collect();
    Some((*times.first()?, *times.last()?))
}

/// A blast's assessment on `target` with the roll seeded by `seed`.
fn blast(row: u16, target: &Sheet, seed: u64) -> damage::Assessed {
    let blow = Blow {
        attacker: None,
        relation: Relation::Hostile,
        source: Source::Blast { row },
        location: Location::None,
        direction: None,
        sneak: 0,
    };
    damage::assess(
        &blow,
        None,
        target,
        FriendlyFire::Off,
        sim::combat::passive::Game::K1,
        &mut SimRng::new(seed, 7),
    )
}

/// `kjk combat grenades`.
pub fn grenades(discovery: &Discovery, game: &str, module: &str, target_tag: &str) -> ExitCode {
    let mut report = Report::default();
    outln!("combat grenades {module}: KOTOR's grenades thrown as JA's thermal detonator");

    outln!("  the rule");
    let frag = grenades::effect(ITEM_ABILITY_GRENADE_FRAGMENTATION).expect("a grenade row");
    report.check(
        (frag.points, frag.damage_type, frag.dc) == (20, 1, Some(15))
            && grenades::effect(ITEM_ABILITY_GRENADE_LAUNCHED_FRAGMENTATION) == grenades::effect(ITEM_ABILITY_GRENADE_FRAGMENTATION)
            && grenades::effect(ITEM_ABILITY_GRENADE_ION).is_some_and(|ion| (ion.points, ion.droid_points) == (15, 45))
            && grenades::effect(ITEM_ABILITY_GRENADE_ADHESIVE).is_some_and(|adhesive| adhesive.dc.is_none())
            && grenades::effect(FORCE_POWER_FORCE_PUSH).is_none(),
        "the table: frag 20 piercing DC 15; TSL's launched rows as theirs; ion 15, 45 on droids; adhesive unsaved; other rows none".to_string(),
    );
    let soldier = Sheet::dev_soldier();
    report.check(
        grenades::reflex(&soldier, 15, 20)
            && !grenades::reflex(&soldier, 2, 1)
            && grenades::reflex(&soldier, 15, 13)
            && !grenades::reflex(&soldier, 15, 12),
        "Reflex: a natural 20 saves, a 1 fails, else d20 + 0 + DEX 2 against the DC (13 saves DC 15, 12 does not)".to_string(),
    );
    let mut held = sim::combat::Fighter::of(&soldier);
    let first = held.force.hold(1000, 9000);
    let again = held.force.hold(2000, 9000);
    report.check(
        first && !again && held.force.stunned_until == 4000 && held.force.immune_until == 5000,
        "C11: a 9 s stun holds 3 s, then a second of immunity".to_string(),
    );
    let mut poisoned = Pack {
        poison: Some(Poisoned::new(
            sim::SimId(0),
            items::Course {
                points: 4,
                every_ms: 3000,
                for_ms: 30_000,
                ..items::Course::default()
            },
            0,
            None,
        )),
        ..Pack::default()
    };
    let doses: usize = (1..=40).map(|s| poisoned.doses(s * 1000).len()).sum();
    report.check(
        doses == 10 && poisoned.poison.is_none(),
        format!("poison: {doses} doses of 4 in 30 s, then gone"),
    );
    let weakened = items::affected(
        soldier,
        &Pack {
            drained: (items::drain(Ability::Dexterity, 6), u64::MAX),
            ..Pack::default()
        },
        0,
    );
    report.check(
        weakened.defence() == soldier.defence() - 3,
        format!(
            "a sonic grenade's −6 DEX: defence {} → {}",
            soldier.defence(),
            weakened.defence()
        ),
    );
    let mut droid = Sheet::dev_soldier();
    droid.race = races::DROID;
    let ion: Vec<i32> = (0..40).map(|seed| blast(ITEM_ABILITY_GRENADE_ION, &droid, seed).damage).collect();
    let organic: Vec<i32> = (0..40)
        .map(|seed| blast(ITEM_ABILITY_GRENADE_ION, &soldier, seed).damage)
        .collect();
    report.check(
        ion.iter().all(|d| [4500, 2200].contains(d))
            && organic.iter().all(|d| [1500, 700].contains(d))
            && ion.contains(&2200)
            && ion.contains(&4500),
        "the ion grenade: 45 on a droid (22 saved), 15 on the living (7 saved)".to_string(),
    );

    outln!("  a throw in the arena (held 1 s, released; two frag grenades carried)");
    let arena = Stage::arena();
    let start = arena.with_sheet(thrower(ITEM_ABILITY_GRENADE_FRAGMENTATION, 2));
    let me = seat0(&start);
    let throw = |pitch: f32| {
        move |last: &Snapshot, since: u64| {
            let ps = player(last, me);
            let held = matches!(since, 0..1000 | 5000..6000 | 10_000..11_000);
            Command {
                buttons: if held { buttons::GRENADE } else { 0 },
                ..mind::looking(&ps, pitch, ps.view[1])
            }
        }
    };
    let run = arena.drive(&start, 15_000, &mut throw(-20.0));
    let thrown: Vec<u8> = run
        .snapshots
        .iter()
        .map(|snapshot| fighter(snapshot, me).pack.used[0])
        .collect();
    let (out, gone) = flight(&run, &start).unwrap_or((0, 0));
    let in_flight: Vec<sim::combat::Missile> = run
        .snapshots
        .iter()
        .flat_map(lobs)
        .map(|(_, missile)| missile)
        .collect();
    let rested = in_flight
        .iter()
        .any(|missile| missile.lob.is_some_and(|lob| lob.resting));
    let peak = in_flight
        .iter()
        .map(|missile| missile.base[2])
        .fold(f32::MIN, f32::max);
    report.check(
        out == 1000 && gone.abs_diff(1000 + grenades::FUSE_MS - 8) <= 16 && rested,
        format!(
            "thrown on the release at {out} ms, came to rest, burst at {gone} ms (the 3.5 s fuse)"
        ),
    );
    report.check(
        thrown.last() == Some(&2) && peak > player(&start, me).origin[2],
        format!(
            "two thrown, the third press throws nothing (count {:?}); the arc rose above the hand",
            thrown.last()
        ),
    );
    report.replayed("arena throw", run.replayed);

    outln!("  each kind thrown at {target_tag}, bursting on its body");
    let stage = match Stage::new(discovery, game, module, &[target_tag.to_string()]) {
        Ok(stage) => stage,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let Some(target) = stage.creature(target_tag) else {
        eprintln!("no creature tagged {target_tag} in {module}");
        return ExitCode::FAILURE;
    };
    let me = seat0(&stage.start);
    let scene = |row: u16, still: bool| {
        let mut start = stage
            .scene(target, 160.0, thrower(row, 1))
            .expect("a clear spot near the target");
        let now = start.time_ms;
        if let Some(theirs) = start.entities[target].fighter.as_mut() {
            theirs.hp = 10_000_000;
            if still {
                theirs.force.stunned_until = now + 600_000;
            }
        }
        start
    };
    // Held 0.4 s at the chest: a short, flat throw into the body.
    let at_them = |ms: u64| (if ms < 400 { buttons::GRENADE } else { 0 }, 0, 0);
    let mut results = Vec::new();
    for (row, name) in [
        (ITEM_ABILITY_GRENADE_FRAGMENTATION, "frag"),
        (ITEM_ABILITY_GRENADE_THERMAL_DETONATOR, "thermal detonator"),
        (ITEM_ABILITY_GRENADE_SONIC, "sonic"),
        (ITEM_ABILITY_GRENADE_POISON, "poison"),
        (ITEM_ABILITY_GRENADE_STUN, "concussion"),
    ] {
        let start = scene(row, row != ITEM_ABILITY_GRENADE_STUN);
        let scripted = stage
            .level
            .scripts()
            .is_some_and(|library| library.impact(row).is_some());
        let run = stage.run(&start, target, 4000, &mut |ms| at_them(ms));
        let before = fighter(&start, target);
        let hit = run.snapshots.iter().find_map(|snapshot| {
            let theirs = fighter(snapshot, target);
            (theirs.next_event != before.next_event)
                .then(|| (snapshot.time_ms, theirs.newest().next().copied()))
        });
        let effect = grenades::effect(row).expect("a grenade row");
        let end = run.snapshots.last().expect("stepped");
        let theirs = fighter(end, target);
        let (pass, what) = match hit {
            None => (false, "it never burst on the target".to_string()),
            Some((at, event)) => {
                let damage = event.map_or(0, |event| event.damage);
                let full = i32::from(effect.points) * 100;
                let saved = full > 0 && damage == full / 2;
                let whole = damage == full;
                let lingering = match effect.secondary {
                    Secondary::Knockdown => {
                        theirs.saber.floored()
                            || fighter(at_snapshot(&run, at), target).saber.floored()
                    }
                    Secondary::Weaken { points, .. } => theirs.pack.drained(end.time_ms)[Ability::Dexterity as usize] == points,
                    Secondary::Poison { .. } => theirs.pack.poison.is_some(),
                    // Held no longer than C11's round, from the burst.
                    Secondary::Held(_) => {
                        let force = fighter(at_snapshot(&run, at), target).force;
                        force.immune_until > before.force.immune_until
                            && force.stunned_until <= at + 3000
                    }
                    Secondary::None => false,
                };
                // A module whose library has the grenade's impact script
                // (`k_sup_grenade`): the script decides what it does (its
                // saves, its effects); it reached the target.
                let pass = if scripted {
                    damage > 0 || lingering || theirs.effects.lasting.iter().flatten().count() > 0
                } else {
                    match effect.secondary {
                        // No damage of its own: the effect, unless saved.
                        Secondary::Poison { .. } | Secondary::Held(_) => damage == 0,
                        _ => (whole && lingering) || (saved && !lingering),
                    }
                };
                (
                    pass,
                    format!(
                        "{name}{}: {:.2} HP ({}), secondary {}",
                        if scripted { " (k_sup_grenade)" } else { "" },
                        f64::from(damage) / 100.0,
                        if whole {
                            "whole"
                        } else if saved {
                            "saved, halved"
                        } else {
                            "none of its own"
                        },
                        if lingering { "took hold" } else { "spared" }
                    ),
                )
            }
        };
        report.check(pass, what.clone());
        results.push(serde_json::json!({"row": row, "result": what}));
        report.replayed(name, run.replayed);
    }

    outln!("  a frag grenade at the thrower's own feet");
    let start = scene(ITEM_ABILITY_GRENADE_FRAGMENTATION, true);
    let own = stage.drive(&start, 4500, &mut |last, since| {
        let ps = player(last, me);
        Command {
            buttons: if since < 100 { buttons::GRENADE } else { 0 },
            ..mind::looking(&ps, 89.0, ps.view[1])
        }
    });
    let mine = fighter(own.snapshots.last().expect("stepped"), me);
    report.check(
        mine.hp == fighter(&start, me).hp && mine.pack.used[0] == 1,
        format!(
            "friendly fire off: the thrower keeps {:.2} HP",
            f64::from(mine.hp) / 100.0
        ),
    );
    report.replayed("own feet", own.replayed);

    outln!("  a creature given a rifle and three frag grenades, its script using one at the player");
    // Carried in its inventory; its script queues the grenade's talent at
    // the player (`ActionUseTalentOnObject`, as `k_ai_master`'s grenade
    // branch does), and the creature throws it there, its burst cast as the
    // row's impact script. A gunner keeps its distance (a melee fighter
    // closes inside the 4 m a throw needs).
    let frags = [("g_w_fraggren01", 3)];
    let rifle = |theirs: &mut Sheet| {
        theirs.weapons[0] = Some(sim::combat::Weapon::plain(
            20,
            1,
            8,
            (1, 2),
            sim::combat::sheet::damage_types::ENERGY,
            [sim::combat::sheet::feats::PROFICIENCY_BLASTER_RIFLE, 0, 0],
        ));
    };
    match crate::items_verbs::given_items(&stage, target, &frags, rifle) {
        Ok((given, _)) => {
            let mut start = [160.0, 240.0, 320.0]
                .into_iter()
                .find_map(|distance| given.scene(target, distance, thrower(ITEM_ABILITY_GRENADE_FRAGMENTATION, 0)))
                .expect("a clear spot near the target");
            // Already fighting the player, as a creature whose AI round
            // chooses a talent is.
            let player_id = start.entities[seat0(&start)].id;
            let now = start.time_ms;
            if let Some(theirs) = start.entities[target].fighter.as_mut() {
                theirs.mind.cast_after = 0;
                theirs.target = Some(player_id);
                theirs.combat_until = now + sim::combat::COMBAT_MS;
            }
            let id = start.entities[target].id;
            let mut script = given.script.clone();
            script.queue(
                script_ncs::vm::ObjectId(id.0),
                sim::script::actions::Action::Talent {
                    talent: sim::script::actions::Use::Power(ITEM_ABILITY_GRENADE_FRAGMENTATION),
                    target: script_ncs::vm::ObjectId(player_id.0),
                },
            );
            let given = given.with_script(script);
            let run = given.run(&start, target, 20_000, &mut |_| (0, 0, 0));
            // Each of its lobs, as it left its hand.
            let mut thrown: Vec<(sim::SimId, sim::combat::Missile)> = run
                .snapshots
                .iter()
                .flat_map(lobs)
                .filter(|(_, lob)| lob.owner == id)
                .collect();
            thrown.sort_by_key(|(lob, missile)| (*lob, missile.from_ms));
            thrown.dedup_by_key(|(lob, _)| *lob);
            let me = seat0(&start);
            // Thrown toward the player: its first flight within 20° of the
            // way from the thrower to the player.
            let toward = thrown.first().is_some_and(|(_, missile)| {
                let way = mind::aim(missile.base, player(&start, me).origin).1;
                let flight = mind::aim([0.0; 3], missile.velocity).1;
                mind::turn(way, flight).abs() <= 20.0
            });
            let last = run.snapshots.last().expect("stepped");
            let left = crate::items_verbs::kinds_of(last, target);
            let frags = left
                .iter()
                .find(|(row, _)| *row == ITEM_ABILITY_GRENADE_FRAGMENTATION)
                .map_or(0, |(_, count)| *count);
            let hurt = fighter(&start, me).hp - fighter(last, me).hp;
            report.check(
                thrown.len() == 1 && toward && frags == 2,
                format!(
                    "it threw {} of its 3 toward the player, the frag its script chose (frags left {frags}); the player lost {:.2} HP to it",
                    thrown.len(),
                    f64::from(hurt) / 100.0
                ),
            );
            report.replayed("a creature's grenades", run.replayed);
        }
        Err(error) => report.check(false, error),
    }

    report.finish(serde_json::json!({
        "verb": "combat grenades", "game": game, "module": module, "target": target_tag,
        "results": results,
    }))
}

/// The snapshot of `run` at sim time `ms`.
fn at_snapshot(run: &Run, ms: u64) -> &Snapshot {
    run.snapshots
        .iter()
        .find(|snapshot| snapshot.time_ms >= ms)
        .unwrap_or_else(|| run.snapshots.last().expect("stepped"))
}
