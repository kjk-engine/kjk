//! `kjk combat lag`: lag compensation's evidence (I25; I22 C18). A player
//! shoots at a creature that runs across its view (a neutral one, placed
//! ahead on the navigation mesh: its mind chases a spot to the side it has
//! a straight way to), aiming each step at where the creature was
//! `L` ms before, as a client that far behind sees it. Each `L` (0, 80,
//! 150 ms) runs twice: with the view lag in the input (the authority
//! tests the shot against the world the player saw) and without it (the
//! present world): the first lands its shots, the second misses once the
//! creature has moved past a body's width. With a hitscan (a disruptor)
//! and with bolts (a pistol: reported only, as a bolt's flight asks the
//! shooter to lead anyway); and a level-1 lightning's line (an aimed
//! power), on the level without its scripts so each 50 ms strike is JA's
//! own hit (a module's impact script marks one a second, C10). Every run's
//! recording replays. Writes
//! `facts.json`.

use std::collections::VecDeque;
use std::process::ExitCode;

use sim::combat::ai::Task;
use sim::combat::sheet::{Weapon, damage_types, feats};
use sim::combat::{Sheet, mind};
use sim::{Recorder, Seat, SimWorld, Snapshot, StepReason, TickInput, TickRate, step};
use vfs::Discovery;

/// What the player does at the runner: fire its weapon from half a
/// second on, or hold a level-1 lightning (Shock, row 43: a line, struck
/// every 50 ms) from a second to a second and a half.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Fire,
    Lightning,
}

/// One run's result.
struct Shots {
    fired: usize,
    landed: usize,
    replayed: bool,
}

/// The runner's hit events so far (sequence numbers counted).
fn events(snapshot: &Snapshot, runner: usize) -> u16 {
    snapshot.entities[runner]
        .fighter
        .map_or(0, |fighter| fighter.next_event)
}

#[allow(clippy::too_many_arguments)]
fn run(
    level: &sim::Level,
    start: &Snapshot,
    me: usize,
    runner: usize,
    lag_ms: u16,
    compensated: bool,
    seconds: u64,
    action: Action,
) -> Shots {
    let rate = TickRate::default();
    let mut world = SimWorld::from_snapshot(start, level.clone()).expect("the run starts");
    let mut recorder = Recorder::new(rate.hz(), start.clone(), level.id(), 100);
    let mut last = start.clone();
    // Where the runner was, newest last, to aim at the past.
    let mut seen: VecDeque<[f32; 3]> = VecDeque::new();
    let back = usize::from(lag_ms) / usize::from(rate.msec(1) as u16).max(1);
    let (mut fired, first_events) = (0usize, events(start, runner));
    while last.time_ms < start.time_ms + seconds * 1000 {
        let tick = last.tick + 1;
        let runner_at = last.entities[runner]
            .player
            .map_or([0.0; 3], |p| mind::chest(&p));
        seen.push_back(runner_at);
        if seen.len() > back + 1 {
            seen.pop_front();
        }
        let aim = seen[0];
        let player = last.entities[me].player.expect("a player");
        let (pitch, yaw) = mind::aim(mind::crosshair(&player), aim);
        let elapsed = last.time_ms - start.time_ms;
        let fire = action == Action::Fire && elapsed >= 500;
        let lightning = action == Action::Lightning && (1000..1500).contains(&elapsed);
        let mut command = mind::looking(&player, pitch, yaw);
        if fire {
            command.buttons |= sim::buttons::ATTACK;
        }
        if lightning {
            command.buttons |= sim::buttons::FORCE_LIGHTNING;
        }
        let mut input = TickInput::default();
        input.insert(Seat(0), command);
        if compensated {
            input.set_view_lag(Seat(0), lag_ms);
        }
        let msec = rate.msec(tick);
        let weapon_before = last.entities[me].fighter.map_or(0, |f| f.weapon_wait_ms);
        last = step(&mut world, tick, &input, msec, StepReason::Authority);
        recorder.record(msec, &input, &last);
        let weapon_after = last.entities[me].fighter.map_or(0, |f| f.weapon_wait_ms);
        if fire && weapon_after > weapon_before {
            fired += 1;
        }
        if lightning && elapsed == 1000 {
            // Half a second held: a strike due every 50 ms.
            fired += 10;
        }
    }
    let landed = usize::from(events(&last, runner).wrapping_sub(first_events));
    let recording = recorder.finish(&last);
    let replayed = recording
        .play(level)
        .is_ok_and(|end| end.hash() == recording.end);
    Shots {
        fired,
        landed,
        replayed,
    }
}

/// `kjk combat lag`.
pub fn lag(
    discovery: &Discovery,
    game: &str,
    module: &str,
    arrive: Option<&str>,
    runner_tag: &str,
) -> ExitCode {
    let mut world = match crate::scenes::installed(discovery, game, module, arrive, &[], &[]) {
        Ok(world) => world,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let level = world.level().clone();
    let mut start = world.snapshot();
    let me = crate::scenes::seat0(&start);
    let Some(&runner) = crate::scenes::tagged(&level, &start, runner_tag).first() else {
        eprintln!("no creature tagged {runner_tag}");
        return ExitCode::FAILURE;
    };
    // The runner: ahead of the player on the navigation mesh, running
    // across its view to a spot to the side it has a way to (its mind
    // chases it); everyone else dead.
    let player = start.entities[me].player.expect("a player");
    let [x, y, z] = player.origin;
    let doors = |_: u32| sim::ObjectState::default();
    let placed = (0..12u8)
        .flat_map(|turn| {
            [480.0f32, 400.0, 320.0, 560.0, 640.0].map(move |reach| (f32::from(turn) * 30.0, reach))
        })
        .flat_map(|(turn, reach)| {
            [900.0f32, -900.0, 600.0, -600.0, 400.0, -400.0]
                .map(move |across| (turn, reach, across))
        })
        .find_map(|(turn, reach, across)| {
            let (sin, cos) = sim::math::sin_cos_deg(player.view[1] + turn);
            let ahead = [x + reach * cos, y + reach * sin, z];
            let side = [ahead[0] - across * sin, ahead[1] + across * cos, z];
            let way = sim::walk::route(&level, ahead, side, &doors)?;
            // Nearly straight across, in the shooter's sight.
            (way.len() <= 2 && sim::walk::route(&level, player.origin, ahead, &doors).is_some())
                .then_some((ahead, side))
        });
    let Some((ahead, side)) = placed else {
        eprintln!("no open ground ahead of the player to run across");
        return ExitCode::FAILURE;
    };
    for (at, entity) in start.entities.iter_mut().enumerate() {
        let (Some(fighter), Some(ps)) = (entity.fighter.as_mut(), entity.player.as_mut()) else {
            continue;
        };
        if at == runner {
            *ps = sim::motion::MotionState::standing(ahead, player.view[1] + 90.0);
            fighter.mind.task = Task::Chase;
            fighter.mind.last_seen = Some((side, start.time_ms + 60_000));
            fighter.mind.cast_after = u64::MAX;
        } else if at != me {
            fighter.life = sim::combat::Life::Dead;
            ps.flags |= sim::motion::motion_flags::DEAD;
        }
    }
    outln!(
        "combat lag {module}: a player shoots at {runner_tag} running across its view 480 units ahead, aiming where it was L ms before"
    );
    let mut results = Vec::new();
    let mut pass = true;
    let mut present = 0.0;
    for (name, hitscan, weapon) in [
        (
            "disruptor (hitscan)",
            true,
            Weapon::plain(
                16,
                2,
                6,
                (1, 2),
                damage_types::ENERGY,
                [feats::PROFICIENCY_BLASTER_RIFLE, 0, 0],
            ),
        ),
        (
            "pistol (bolts)",
            false,
            Weapon::plain(
                12,
                1,
                6,
                (1, 2),
                damage_types::ENERGY,
                [feats::PROFICIENCY_BLASTER, 0, 0],
            ),
        ),
    ] {
        let mut armed = start.clone();
        let mut sheet = Sheet::dev_soldier();
        sheet.weapons = [Some(weapon), None];
        armed.entities[me].sheet = Some(sheet);
        for lag_ms in [0u16, 80, 150] {
            let with = run(&level, &armed, me, runner, lag_ms, true, 4, Action::Fire);
            let without = run(&level, &armed, me, runner, lag_ms, false, 4, Action::Fire);
            let share = |shots: &Shots| shots.landed as f64 / shots.fired.max(1) as f64;
            outln!(
                "  {name}, {lag_ms:>3} ms behind: compensated {}/{} landed ({:.0}%), uncompensated {}/{} ({:.0}%); replays {} / {}",
                with.landed,
                with.fired,
                share(&with) * 100.0,
                without.landed,
                without.fired,
                share(&without) * 100.0,
                if with.replayed { "same" } else { "DIFFERENT" },
                if without.replayed {
                    "same"
                } else {
                    "DIFFERENT"
                }
            );
            // A hitscan's compensated shots land as a present-view
            // shooter's do, the uncompensated fall away with the lag. Bolts
            // fly long enough that a shooter must lead them anyway (this
            // aim does not): reported, not judged.
            pass &= with.replayed && without.replayed;
            if hitscan {
                if lag_ms == 0 {
                    present = share(&with);
                }
                pass &= share(&with) + 0.2 >= present;
                if lag_ms >= 150 {
                    pass &= share(&without) < share(&with);
                }
            }
            results.push(serde_json::json!({
                "weapon": name, "lag_ms": lag_ms,
                "compensated": {"fired": with.fired, "landed": with.landed},
                "uncompensated": {"fired": without.fired, "landed": without.landed},
                "replayed": with.replayed && without.replayed,
            }));
        }
    }
    // A line power (a level-1 lightning) finds its target as the caster
    // saw it. On the level without its scripts, so every 50 ms strike is
    // JA's own hit: with them, only the impact script's once a second
    // leaves a mark (C10), one in the half second.
    let native = level.clone().without_scripts();
    let mut casting = start.clone();
    casting.place = native.place();
    let mut jedi = Sheet::dev_jedi();
    jedi.powers = sim::combat::sheet::Rows::default().with(43);
    casting.entities[me].sheet = Some(jedi);
    if let Some(fighter) = casting.entities[me].fighter.as_mut() {
        *fighter = sim::combat::Fighter::of(&jedi);
    }
    for lag_ms in [0u16, 80, 150] {
        let with = run(
            &native,
            &casting,
            me,
            runner,
            lag_ms,
            true,
            2,
            Action::Lightning,
        );
        let without = run(
            &native,
            &casting,
            me,
            runner,
            lag_ms,
            false,
            2,
            Action::Lightning,
        );
        outln!(
            "  lightning (Shock, a line), {lag_ms:>3} ms behind: compensated {}/{} strikes landed, uncompensated {}/{}",
            with.landed,
            with.fired,
            without.landed,
            without.fired
        );
        pass &= with.replayed && without.replayed && with.landed * 10 >= with.fired * 8;
        if lag_ms >= 150 {
            pass &= without.landed < with.landed;
        }
        results.push(serde_json::json!({
            "weapon": "lightning", "lag_ms": lag_ms,
            "compensated": {"fired": with.fired, "landed": with.landed},
            "uncompensated": {"fired": without.fired, "landed": without.landed},
        }));
    }
    outln!("  {}", if pass { "passed" } else { "FAILED" });
    crate::runtime::write_facts(serde_json::json!({
        "verb": "combat lag", "game": game, "module": module, "runner": runner_tag,
        "results": results, "pass": pass,
    }));
    if pass {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
