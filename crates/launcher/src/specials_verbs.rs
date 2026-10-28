//! `kjk combat specials`: the second bar and the feat specials without a
//! window (I22 C7 and section 4, "Everyone else"; `sim::combat::specials`).
//! A gunner (`Sheet::dev_soldier` with a weapon and feats per scene, 500
//! HP so the fight cannot end it) stands 160 units from a hostile creature
//! of a module (end_m01aa's Sith soldier by default, given HP to spare);
//! each scene steps the authority from the same start, recorded and
//! replayed:
//! - stamina: its size by CON, and an empty bar refilling in 20 s;
//! - Power Blast: the charge, its cost, one shot on the release, heavier
//!   than the pistol's plain shots;
//! - Rapid Shot and Multi Shot: the burst's shots and spacing, the gun's
//!   recovery, the cost and the round's penalties;
//! - Sniper Shot: aiming halves movement, each aimed shot pays and leaves
//!   defence −5;
//! - sneak attack: the rule's cases, and a scoundrel's disruptor on a held
//!   target against one that faces it, both shot once it is aware;
//! - a creature given Rapid Shot fires a burst for the talent its script
//!   chose.
//!
//! In the shooting scenes the target is stunned (it stands still; held,
//! which matters only to sneak attacks). Writes `facts.json`.

use std::process::ExitCode;

use sim::combat::fighter::EVENTS;
use sim::combat::sheet::{Blaster, Weapon, damage_types, feats};
use sim::combat::specials::{self, Exposure, Special};
use sim::combat::{Event, Sheet, force, mind};
use sim::{Command, Snapshot, buttons};
use vfs::Discovery;

use crate::scenes::{Report, Run, Stage, fighter, hold, player, seat0};

/// A blaster of base item `row` with `dice`d6 and its line's feats.
fn blaster(row: u16, dice: u8, line: [u16; 3]) -> Weapon {
    Weapon::plain(row, dice, 6, (1, 2), damage_types::ENERGY, line)
}

fn pistol() -> Weapon {
    blaster(
        12,
        1,
        [
            feats::PROFICIENCY_BLASTER,
            feats::WEAPON_FOCUS_BLASTER,
            feats::WEAPON_SPEC_BLASTER,
        ],
    )
}

fn rifle() -> Weapon {
    blaster(
        20,
        1,
        [
            feats::PROFICIENCY_BLASTER_RIFLE,
            feats::WEAPON_FOCUS_BLASTER_RIFLE,
            feats::WEAPON_SPEC_BLASTER_RIFLE,
        ],
    )
}

fn disruptor() -> Weapon {
    blaster(16, 2, [feats::PROFICIENCY_BLASTER_RIFLE, 0, 0])
}

/// The dev soldier with `weapon` and `extra` feats, and HP to spare.
fn gunner(weapon: Weapon, extra: &[u16]) -> Sheet {
    let mut sheet = Sheet::dev_soldier();
    sheet.max_hp = 500;
    sheet.weapons = [Some(weapon), None];
    for feat in extra {
        sheet.feats.insert(*feat);
    }
    sheet
}

/// A scene's start with seat 0 as `sheet` `distance` units from the
/// target, which is given HP to spare and, when `still`, stunned for the
/// scene.
fn start_with(
    stage: &Stage,
    target: usize,
    distance: f32,
    sheet: Sheet,
    still: bool,
) -> Option<Snapshot> {
    let mut start = stage.scene(target, distance, sheet)?;
    let time = start.time_ms;
    // The gunner's weapon is up, as in a fight: a lowered gun is raised
    // first (`motion::weapon`), its 250 ms before every scene's first shot.
    let me = seat0(&start);
    if let Some(mine) = start.entities[me].fighter.as_mut() {
        mine.saber.out_until = time + sim::combat::COMBAT_MS;
    }
    // HP to spare, its sheet's maximum too: HP over the maximum is cut to
    // it every step (`combat::effects::worn`).
    let mut sheet = start.entities[target].sheet.or_else(|| {
        sim::Instances::in_snapshot(&stage.level, &start)
            .of_entity(&start.entities[target])
            .and_then(|data| data.sheet)
    })?;
    sheet.max_hp = u16::MAX;
    start.entities[target].sheet = Some(sheet);
    let theirs = start.entities[target].fighter.as_mut()?;
    theirs.hp = i32::from(u16::MAX) * 100;
    if still {
        theirs.force.stunned_until = time + 600_000;
    }
    Some(start)
}

/// The events the entity at `at` gained over `run`.
fn events(run: &Run, start: &Snapshot, at: usize) -> Vec<Event> {
    let mut next = fighter(start, at).next_event;
    let mut gained = Vec::new();
    for snapshot in &run.snapshots {
        let now = fighter(snapshot, at);
        while next != now.next_event {
            gained.extend(now.events[usize::from(next) % EVENTS]);
            next = next.wrapping_add(1);
        }
    }
    gained
}

/// When seat 0's gun fired: the script times (ms after the start) of the
/// steps its timer rose in (a step's command is the script's at its
/// start, a step before the snapshot it makes).
fn shots(run: &Run, start: &Snapshot) -> Vec<u64> {
    let me = seat0(start);
    let mut timer = fighter(start, me).weapon_wait_ms;
    let mut fired = Vec::new();
    for snapshot in &run.snapshots {
        let now = fighter(snapshot, me).weapon_wait_ms;
        if now > timer {
            fired.push(snapshot.time_ms - start.time_ms - 8);
        }
        timer = now;
    }
    fired
}

/// Whether `times` start with `expected`, each within a step.
fn on_time(times: &[u64], expected: &[u64]) -> bool {
    times.len() >= expected.len()
        && times
            .iter()
            .zip(expected)
            .all(|(time, expected)| time.abs_diff(*expected) <= 8)
}

/// The mean damage (HP) of `events`.
fn mean(events: &[Event]) -> f64 {
    let total: i64 = events.iter().map(|event| i64::from(event.damage)).sum();
    total as f64 / events.len().max(1) as f64 / 100.0
}

/// The snapshot `ms` after the start (a step is 8 ms).
fn at(run: &Run, ms: u64) -> &Snapshot {
    &run.snapshots[(ms / 8).saturating_sub(1) as usize]
}

/// `kjk combat specials`.
pub fn specials(discovery: &Discovery, game: &str, module: &str, target_tag: &str) -> ExitCode {
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
    outln!("combat specials {module}: a gunner 160 units from {target_tag}");
    let mut report = Report::default();
    let scene = |sheet: Sheet, still: bool| {
        start_with(&stage, target, 160.0, sheet, still).expect("a clear spot near the target")
    };

    // The bar (C7): stamina by CON for anyone without the Force.
    outln!("  stamina");
    let soldier = gunner(pistol(), &[]);
    let start = scene(soldier, true);
    let full = i32::from(soldier.pool()) * 100;
    report.check(
        soldier.pool() == 120 && fighter(&start, me).fp == full,
        format!(
            "the soldier (CON 14) has {} stamina, full at the start; the Jedi's bar is its {} FP",
            soldier.pool(),
            Sheet::dev_jedi().pool()
        ),
    );
    let mut empty = start.clone();
    if let Some(fighter) = empty.entities[me].fighter.as_mut() {
        fighter.fp = 0;
    }
    let run = stage.run(&empty, target, 21_000, &mut |_| (0, 0, 0));
    let refilled = run
        .snapshots
        .iter()
        .find(|snapshot| fighter(snapshot, me).fp >= full)
        .map(|snapshot| snapshot.time_ms - start.time_ms);
    let every = force::regen_every(soldier.pool());
    report.check(
        refilled.is_some_and(|ms| ms.abs_diff(20_000) <= every + 8),
        format!("0 to 120 stamina in {refilled:?} ms (C13's rule: the bar in 20 s)"),
    );
    report.replayed("stamina", run.replayed);

    // The specials' shapes, as the rule gives them.
    outln!("  the specials' shots and choice");
    let shaped = |special: Special| special.shot(pistol());
    let plain = pistol();
    report.check(
        [(1, 5), (2, 8), (3, 10)].iter().all(|&(tier, bonus)| {
            let shot = shaped(Special::PowerBlast(tier));
            shot.damage_bonus == plain.damage_bonus + bonus
                && shot.attack_bonus == plain.attack_bonus - 3
        }),
        "Power Blast tiers: +5 / +8 / +10 damage at −3 to hit".to_string(),
    );
    report.check(
        (1..=3).all(|tier| shaped(Special::SniperShot(tier)).crit_threat == tier + 1)
            && shaped(Special::RapidShot(1)) == plain,
        "Sniper Shot tiers: the threat range ×2 / ×3 / ×4; a Rapid Shot shot is plain".to_string(),
    );
    let knows = |rows: &[u16]| gunner(pistol(), rows);
    let all = [
        feats::POWER_BLAST[0],
        feats::RAPID_SHOT[0],
        feats::SNIPER_SHOT[0],
    ];
    report.check(
        Special::of(&knows(&all), Blaster::Pistol) == Some(Special::PowerBlast(1))
            && Special::of(&knows(&all), Blaster::Rifle) == Some(Special::RapidShot(1))
            && Special::of(&knows(&all), Blaster::Disruptor) == Some(Special::SniperShot(1))
            && Special::of(&knows(&all[2..]), Blaster::Pistol) == Some(Special::SniperShot(1))
            && Special::of(&knows(&feats::RAPID_SHOT), Blaster::Rifle)
                == Some(Special::RapidShot(3))
            && Special::of(&knows(&[]), Blaster::Rifle).is_none(),
        "each gun takes its own line first (pistol Power Blast, rifle Rapid Shot, disruptor Sniper Shot), else another known; Multi Shot is tier 3"
            .to_string(),
    );

    // Power Blast: the pistol's charge.
    outln!("  Power Blast (pistol): a full charge, released at 1 s, every 2.5 s");
    let cost = specials::cost(&soldier);
    let blasting = gunner(pistol(), &[feats::POWER_BLAST[0]]);
    let start = scene(blasting, true);
    let run = stage.run(&start, target, 10_000, &mut |ms| {
        let alt = ms % 2500 < 1000;
        (if alt { buttons::ALT_ATTACK } else { 0 }, 0, 0)
    });
    let charging = fighter(at(&run, 500), me);
    report.check(
        charging.shooting.charging_from.is_some()
            // Paid at the start, a half second's regeneration back since.
            && (full - cost..=full - cost + full / 40).contains(&charging.fp)
            && on_time(&shots(&run, &start), &[1000]),
        format!(
            "charging at 0.5 s with {:.0} stamina left (paid {}), the first shot on the release at {:?} ms",
            f64::from(charging.fp) / 100.0,
            cost / 100,
            shots(&run, &start).first()
        ),
    );
    let blasts = events(&run, &start, target);
    let plain_start = scene(soldier, true);
    let plain_run = stage.run(
        &plain_start,
        target,
        10_000,
        &mut hold(buttons::ATTACK, 10_000),
    );
    let plain_hits = events(&plain_run, &plain_start, target);
    report.check(
        shots(&run, &start).len() == 4
            && blasts.len() == 4
            && mean(&blasts) >= 2.0 * mean(&plain_hits),
        format!(
            "4 blasts in 10 s, {} landed, {:.2} HP each against {:.2} for {} plain shots (+5 damage at −3, paced by 1.8 s)",
            blasts.len(),
            mean(&blasts),
            mean(&plain_hits),
            plain_hits.len()
        ),
    );
    report.replayed("Power Blast", run.replayed);
    report.replayed("plain pistol", plain_run.replayed);

    // Rapid Shot and Multi Shot: the burst.
    for (name, feat, count, penalty) in [
        ("Rapid Shot", feats::RAPID_SHOT[0], 2usize, -4i8),
        ("Multi Shot", feats::RAPID_SHOT[2], 3, -1),
    ] {
        outln!("  {name} (rifle, 350 ms): a press at 0, the trigger held from 100 ms");
        let bursting = gunner(rifle(), &[feat]);
        let start = scene(bursting, true);
        let run = stage.run(&start, target, 2000, &mut |ms| match ms {
            0 => (buttons::ALT_ATTACK, 0, 0),
            ms if ms >= 100 => (buttons::ATTACK, 0, 0),
            _ => (0, 0, 0),
        });
        let fired = shots(&run, &start);
        // The burst 150 ms apart, then the next plain shot one refire short
        // of the burst's shots.
        let ready = (count as u64 - 1) * 350;
        let mut expected: Vec<u64> = (0..count as u64).map(|shot| shot * 150).collect();
        expected.push(ready);
        let after = fighter(at(&run, 8), me);
        let (attack, defence) = after.shooting.exposed(start.time_ms + 8);
        report.check(
            on_time(&fired, &expected)
                && after.fp == full - cost
                && (attack, defence) == (penalty, penalty),
            format!(
                "shots at {:?} ms (expected {expected:?}, within a step); paid {}; {attack} to hit, {defence} to defence for the round",
                &fired[..fired.len().min(count + 2)],
                cost / 100
            ),
        );
        let later = fighter(at(&run, 1990), me)
            .shooting
            .exposed(start.time_ms + 1990);
        report.check(
            later == (penalty, penalty) && after.shooting.exposed(start.time_ms + 3100) == (0, 0),
            "the penalties last the round (3 s)".to_string(),
        );
        report.replayed(name, run.replayed);
    }

    // Sniper Shot: aiming.
    outln!("  Sniper Shot (disruptor): aiming while running 1 s in the arena, then aimed shots");
    let sniping = gunner(disruptor(), &[feats::SNIPER_SHOT[0]]);
    let arena = Stage::arena();
    let open = arena.with_sheet(sniping);
    let mine = seat0(&open);
    let ran = |aim: bool| {
        let run = arena.drive(&open, 1000, &mut |last, _| {
            let ps = player(last, mine);
            Command {
                forward: 127,
                buttons: if aim { buttons::ALT_ATTACK } else { 0 },
                ..mind::looking(&ps, 0.0, ps.view[1])
            }
        });
        let from = player(&open, mine).origin;
        let to = player(run.snapshots.last().expect("stepped"), mine).origin;
        let flat = ((to[0] - from[0]).powi(2) + (to[1] - from[1]).powi(2)).sqrt();
        (flat, run.replayed)
    };
    let ((aimed, aimed_same), (free, free_same)) = (ran(true), ran(false));
    report.check(
        (aimed / free - specials::AIM_SPEED).abs() < 0.1,
        format!(
            "aiming runs {aimed:.0} units in 1 s, {free:.0} without (× {:.2})",
            aimed / free
        ),
    );
    let start = scene(sniping, true);
    let run = stage.run(&start, target, 2000, &mut |ms| {
        let fire = if ms >= 300 { buttons::ATTACK } else { 0 };
        (buttons::ALT_ATTACK | fire, 0, 0)
    });
    let fired = shots(&run, &start);
    let after = fighter(run.snapshots.last().expect("stepped"), me);
    let paid = full - after.fp;
    let regained = full / 20 * 2;
    report.check(
        fighter(at(&run, 200), me).shooting.aiming
            && fired.len() == 3
            && (paid - 3 * cost).abs() <= regained
            && after.shooting.exposed(run.snapshots.last().expect("stepped").time_ms) == (0, -5),
        format!(
            "aiming; {} aimed shots (600 ms apart) paid {:.0} of the bar (3 × {}, less what came back); defence −5",
            fired.len(),
            f64::from(paid) / 100.0,
            cost / 100
        ),
    );
    report.replayed("aiming", aimed_same && free_same);
    report.replayed("Sniper Shot", run.replayed);

    // Sneak attack: the rule, then in the world.
    outln!("  sneak attack");
    let scoundrel = gunner(disruptor(), &feats::SNEAK_ATTACK[..3]);
    let dodger = gunner(disruptor(), &feats::UNCANNY_DODGE[..1]);
    let open = Exposure {
        behind: false,
        held: false,
        unaware: false,
        distance: 200.0,
    };
    let dice =
        |target: &Sheet, exposure: Exposure| specials::sneak_dice(&scoundrel, target, exposure);
    let soldier_sheet = Sheet::dev_soldier();
    report.check(
        dice(&soldier_sheet, open) == 0
            && dice(&soldier_sheet, Exposure { behind: true, ..open }) == 3
            && dice(&dodger, Exposure { behind: true, ..open }) == 0
            && dice(&dodger, Exposure { held: true, ..open }) == 3
            && dice(&soldier_sheet, Exposure { unaware: true, ..open }) == 3
            && dice(&soldier_sheet, Exposure { unaware: true, distance: 400.0, ..open }) == 0
            && specials::sneak_dice(&soldier_sheet, &soldier_sheet, Exposure { held: true, ..open }) == 0,
        "3d6 (Sneak Attack III) from behind, on the held or the unaware, within 10 m; not on one facing it; Uncanny Dodge spares the back only".to_string(),
    );
    report.check(
        specials::behind([0.0; 3], 0.0, [-100.0, 0.0, 0.0])
            && specials::behind([0.0; 3], 0.0, [-100.0, 90.0, 0.0])
            && !specials::behind([0.0; 3], 0.0, [-100.0, 110.0, 0.0])
            && !specials::behind([0.0; 3], 90.0, [-100.0, 0.0, 0.0]),
        "behind is the rear 90°".to_string(),
    );
    // Fire only once the creature has taken the scoundrel in (its slowest
    // reaction and a margin): before, it is unaware, sneak-attackable
    // whether it faces the gun or not.
    let aware = sim::combat::ai::reaction_ms(0) + 500;
    let fight = |still: bool| {
        let start = scene(scoundrel, still);
        let mut trigger = |ms: u64| {
            let firing = (aware..aware + 6000).contains(&ms);
            (if firing { buttons::ATTACK } else { 0 }, 0, 0)
        };
        let run = stage.run(&start, target, aware + 6000, &mut trigger);
        (events(&run, &start, target), run.replayed)
    };
    let ((held_hits, held_same), (facing_hits, facing_same)) = (fight(true), fight(false));
    report.check(
        held_hits.len() >= 5
            && facing_hits.len() >= 5
            && mean(&held_hits) >= 1.8 * mean(&facing_hits),
        format!(
            "a scoundrel's disruptor: {:.2} HP a hit on the held target ({} hits), {:.2} on it fighting and facing ({} hits)",
            mean(&held_hits),
            held_hits.len(),
            mean(&facing_hits),
            facing_hits.len()
        ),
    );
    report.replayed("sneak attack", held_same && facing_same);

    // Awareness: the creature takes in the soldier before it acts.
    outln!("  awareness: {target_tag} spots the soldier standing 160 units off");
    let start = scene(gunner(pistol(), &[]), false);
    let run = stage.run(&start, target, 3000, &mut |_| (0, 0, 0));
    let theirs = |snapshot: &Snapshot| fighter(snapshot, target);
    let spotted = run
        .snapshots
        .iter()
        .find(|snapshot| theirs(snapshot).target.is_some())
        .map(|snapshot| snapshot.time_ms);
    let acted = run
        .snapshots
        .iter()
        .find(|snapshot| {
            let command = theirs(snapshot).last_command;
            command.buttons != 0 || command.forward != 0 || command.right != 0
        })
        .map(|snapshot| snapshot.time_ms);
    let awareness = sim::Instances::in_snapshot(&stage.level, &stage.start)
        .of_entity(&stage.start.entities[target])
        .and_then(|data| data.sheet)
        .map_or(0, |sheet| {
            sheet.skills[sim::combat::sheet::skills::AWARENESS]
        });
    let reaction = sim::combat::ai::reaction_ms(awareness);
    let waited = spotted
        .zip(acted)
        .map(|(seen, acted)| acted.saturating_sub(seen));
    report.check(
        waited.is_some_and(|waited| waited.abs_diff(reaction) <= 16),
        format!(
            "Awareness {awareness}: spotted at {:?} ms, first acted {waited:?} ms later (1.5 s less 150 ms a rank: {reaction} ms)",
            spotted.map(|at| at - start.time_ms)
        ),
    );
    report.replayed("awareness", run.replayed);

    // A creature's script uses its special: its talent queued at the player
    // (`ActionUseTalentOnObject`, as `k_ai_master`'s combat feats are), its
    // mind fires the burst (`ai::feat_use`). Where scripts run no mind
    // chooses a special itself.
    outln!("  a creature given a rifle and Rapid Shot, its script using the feat at the gunner");
    let given = crate::items_verbs::given_items(&stage, target, &[], |theirs| {
        theirs.feats.insert(feats::RAPID_SHOT[0]);
        theirs.feats.insert(feats::PROFICIENCY_BLASTER_RIFLE);
        theirs.weapons[0] = Some(rifle());
    });
    match given {
        Ok((given, theirs)) => {
            let blaster_kind = match theirs.main_weapon().delivery {
                sim::combat::Delivery::Blaster(kind) => Some(kind),
                _ => None,
            };
            let mut start = start_with(&given, target, 160.0, gunner(pistol(), &[]), false)
                .expect("a clear spot near the target");
            // Already fighting the gunner, as a creature whose AI round
            // chooses a talent is.
            let gunner_id = start.entities[seat0(&start)].id;
            let now = start.time_ms;
            if let Some(fighting) = start.entities[target].fighter.as_mut() {
                fighting.mind.cast_after = 0;
                fighting.target = Some(gunner_id);
                fighting.combat_until = now + sim::combat::COMBAT_MS;
            }
            let mut script = given.script.clone();
            script.queue(
                script_ncs::vm::ObjectId(start.entities[target].id.0),
                sim::script::actions::Action::Talent {
                    talent: sim::script::actions::Use::Feat(feats::RAPID_SHOT[0]),
                    target: script_ncs::vm::ObjectId(gunner_id.0),
                },
            );
            let given = given.with_script(script);
            let run = given.run(&start, target, 6_000, &mut |_| (0, 0, 0));
            let bursts = run
                .snapshots
                .windows(2)
                .filter(|pair| {
                    fighter(&pair[0], target).shooting.burst == 0
                        && fighter(&pair[1], target).shooting.burst > 0
                })
                .count();
            report.check(
                blaster_kind.is_some()
                    && Special::of(&theirs, blaster_kind.unwrap_or(Blaster::Pistol)).is_some()
                    && bursts == 1,
                format!(
                    "its {blaster_kind:?} fired {bursts} burst for the Rapid Shot its script chose"
                ),
            );
            report.replayed("a creature's bursts", run.replayed);
        }
        Err(error) => report.check(false, error),
    }

    report.finish(serde_json::json!({
        "verb": "combat specials", "game": game, "module": module, "target": target_tag,
    }))
}
