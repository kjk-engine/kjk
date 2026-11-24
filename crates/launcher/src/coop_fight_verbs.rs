//! `kjk combat coop`: a guest's companion through a fight in a game's own
//! module, its scripts running. The module is installed for the host and
//! one guest (`Guests::Companions`). The creatures tagged `follow` join the
//! party, and the guest's seat takes the first of them (`sim::drivers`).
//! A module name starting with `@` is a chain checkpoint (its path or its
//! name), with its own party. The host walks to the nearest hostile
//! creature and shoots back at whoever targets it. The guest's seat pushes
//! its mouse fully down and fully up in turn and attacks every other
//! second. Its look turns as the guest's screen turns it (`Look::turn`,
//! with `held_pitch`), starting `START_PITCH` down, so the companion it
//! takes keeps a pitch delta (`switch::look_on`).
//!
//! Each tick it reads whether the seat still drives its companion and
//! whether a script steers the companion (`Mind::steered`, its mind playing
//! it). Each half swing in which the guest played its companion throughout,
//! it checks that the view reached the pitch limit in the way pushed. Each
//! span in which the companion was out of its player's hands is listed,
//! with what its action queue held as the span began.

use std::process::ExitCode;

use sim::module::ObjectKind;
use sim::{Kind, Seat, SimWorld, Snapshot, StepReason, TickInput, TickRate, step};
use vfs::Discovery;

/// The guest's seat.
const GUEST: Seat = Seat(1);
/// How far the guest's mouse pushes the view each half swing, and the time
/// of one swing, down then up (degrees, ms): twice the view's whole range,
/// so each half reaches its end.
const PITCH_PUSH: f32 = 360.0;
const SWING_MS: u64 = 3000;
/// How fast the guest's view turns round (degrees a ms).
const TURN_PER_MS: f32 = 0.02;
/// Where the guest's look starts (degrees down): it joins looking down, so
/// the body it takes keeps a pitch delta (`switch::look_on`).
const START_PITCH: f32 = 40.0;
/// How near the view's limit a half swing must bring the view (degrees).
const REACH_SLACK: f32 = 3.0;

/// A stretch of ticks in which the guest's companion was not its own.
struct Span {
    from_ms: u64,
    to_ms: u64,
    why: &'static str,
    told: String,
}

/// Half a swing of the guest's mouse: which way it pushes, since when,
/// whether the guest played its companion throughout (driven, standing, not
/// stunned, not steered), and the view's least and greatest pitch.
struct Half {
    down: bool,
    from_ms: u64,
    own: bool,
    lowest: f32,
    highest: f32,
}

impl Half {
    fn new(down: bool, from_ms: u64) -> Half {
        Half {
            down,
            from_ms,
            own: true,
            lowest: f32::MAX,
            highest: f32::MIN,
        }
    }
}

/// The guest's mouse at `ms`, one tick's motion (degrees, x right, y down,
/// as `Look::turn` takes it): pushed down for half of each swing and up for
/// the other half, each half far past the view's limit; a slow turn round.
fn guest_mouse(ms: u64, tick_ms: u64) -> bevy::math::Vec2 {
    let per_ms = PITCH_PUSH / (SWING_MS / 2) as f32;
    let down = ms % SWING_MS < SWING_MS / 2;
    let y = if down { per_ms } else { -per_ms } * tick_ms as f32;
    bevy::math::Vec2::new(TURN_PER_MS * tick_ms as f32, y)
}

/// The guest's command from its look: attack held every other second.
fn guest_command(ms: u64, look: &kjk_game::sim_host::Look) -> sim::Command {
    let attack = (ms / 1000).is_multiple_of(2);
    sim::Command {
        angles: sim::moves::view_angles(look.pitch, look.yaw),
        buttons: if attack { sim::buttons::ATTACK } else { 0 },
        ..sim::Command::default()
    }
}

/// The host's walk toward the nearest living hostile creature, planned
/// again every `REPLAN_MS`; none while one is within `CLOSE_M`.
const REPLAN_MS: u64 = 2000;
const CLOSE_M: f32 = 6.0;

/// Where the nearest living hostile creature stands from seat 0 (metres),
/// and how far it is.
fn nearest_hostile(level: &sim::Level, snapshot: &Snapshot) -> Option<([f32; 3], f32)> {
    use sim::combat::sheet::factions;
    let host = snapshot.body(Seat(0))?;
    let objects = sim::Instances::in_snapshot(level, snapshot);
    snapshot
        .entities
        .iter()
        .filter(|entity| matches!(entity.kind, Kind::Object(ObjectKind::Creature, _)))
        .filter_map(|entity| {
            let fighter = entity.fighter?;
            let sheet = entity
                .sheet
                .or_else(|| objects.of_entity(entity).and_then(|object| object.sheet));
            let faction = fighter.faction.or(sheet.map(|sheet| sheet.faction))?;
            let hostile = [factions::HOSTILE_1, factions::HOSTILE_2].contains(&faction);
            (hostile && fighter.alive()).then_some(entity.position)
        })
        .map(|at| (at, sim::math::length(sim::math::sub(at, host.position))))
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

/// The body the guest's seat drives in `snapshot`, if any.
fn guest_body(snapshot: &Snapshot) -> Option<&sim::EntityState> {
    snapshot.entities.iter().find(|entity| {
        matches!(entity.kind, Kind::Object(ObjectKind::Creature, _))
            && entity
                .fighter
                .is_some_and(|fighter| fighter.mind.driver == Some(GUEST))
    })
}

/// What `id`'s action queue holds, by kind.
fn queue_of(world: &SimWorld, id: u32) -> String {
    let state = world.script_state();
    let queued: Vec<&str> = state
        .actions
        .get(&id)
        .map_or_else(Vec::new, |queue| queue.iter().map(|queued| queued.action.kind()).collect());
    format!(
        "queue [{}], intent {}, goal {}",
        queued.join(", "),
        state.intents.contains_key(&id),
        state.goals.contains_key(&id)
    )
}

/// The game's module installed for the host and one guest, the creatures
/// tagged `follow` in the party.
fn coop_world(
    discovery: &Discovery,
    game: &str,
    module: &str,
    arrive: Option<&str>,
    (hostile, follow): (&[String], &[String]),
) -> Result<SimWorld, String> {
    let installer = crate::scenes::installer(discovery, game)?;
    // A chain checkpoint (`@name`): its party and its companions' own
    // scripts, the module's data taken as it stands.
    if let Some(name) = module.strip_prefix('@') {
        let path = crate::playthrough::checkpoint(game, name);
        let mut world = crate::playthrough::resume(&installer, &TickRate::default(), &path, true)?;
        world.set_seats_held(2);
        return Ok(world);
    }
    let prepared = installer.prepare_one(module, arrive)?;
    let level = crate::stage::with_factions(&prepared.plan.level, hostile, &[]);
    let config = sim::WorldConfig {
        seats: 2,
        guests: sim::Guests::Companions,
        ..sim::WorldConfig::one_seat()
    };
    let mut world =
        SimWorld::new(config, sim::Level::arena()).map_err(|error| error.to_string())?;
    world
        .install(level, arrive)
        .map_err(|error| format!("install refused: {error}"))?;
    let mut world = crate::stage::with_followers(world, follow, Some(&installer))?;
    world.set_seats_held(2);
    Ok(world)
}

/// See the module's notes. Fails when the guest's companion left its
/// player's hands in the fight.
pub fn coop(
    discovery: &Discovery,
    game: &str,
    module: &str,
    arrive: Option<&str>,
    seconds: u64,
    (hostile, follow): (&[String], &[String]),
) -> ExitCode {
    let mut world = match coop_world(discovery, game, module, arrive, (hostile, follow)) {
        Ok(world) => world,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let rate = TickRate::default();
    let mut last = world.snapshot();
    let mut spans: Vec<Span> = Vec::new();
    let (mut driven, mut steered, mut held_halves, mut fighting) = (0u64, 0u64, 0u64, 0u64);
    let mut ticks = 0u64;
    let mut walker: Option<sim::walk::Walker> = None;
    let mut look = kjk_game::sim_host::Look {
        pitch: START_PITCH,
        yaw: 0.0,
    };
    let limit = kjk_game::sim_host::pitch_limit();
    let mut half = Half::new(true, rate.time_ms(last.tick));
    let end = last.tick + seconds * u64::from(sim::TICK_HZ);
    while last.tick < end {
        let tick = last.tick + 1;
        let ms = rate.time_ms(tick);
        let down = ms % SWING_MS < SWING_MS / 2;
        if down != half.down {
            // The half swing over: the guest pushed the view to its end.
            let reached = if half.down {
                half.highest >= limit - REACH_SLACK
            } else {
                half.lowest <= -limit + REACH_SLACK
            };
            if half.own && !reached {
                held_halves += 1;
                let way = if half.down { "down" } else { "up" };
                spans.push(Span {
                    from_ms: half.from_ms,
                    to_ms: ms,
                    why: "pitch held",
                    told: format!(
                        "pushed {way}, the view reached {:.1} to {:.1} of ±{limit:.1}",
                        half.lowest, half.highest
                    ),
                });
            }
            half = Half::new(down, ms);
        }
        let mut input = TickInput::default();
        let host = last.body(Seat(0)).and_then(|entity| entity.player);
        let far = nearest_hostile(world.level(), &last).filter(|(_, apart)| *apart > CLOSE_M);
        if ms % REPLAN_MS < rate.time_ms(1) {
            walker = far
                .zip(host)
                .and_then(|((at, _), host)| {
                    crate::script_verbs::walk_to(world.level(), &host, at[0], at[1]).ok()
                });
        }
        let walking = match (walker.as_mut(), host, far) {
            (Some(walker), Some(host), Some(_)) => {
                let doors = crate::scenes::door_states(&last);
                Some(walker.command(world.level(), &host, last.time_ms, &doors))
            }
            _ => None,
        };
        let host_command =
            walking.unwrap_or_else(|| crate::combat_verbs::player_command(&last));
        input.insert(Seat(0), host_command);
        // The guest's screen turns its look as `net_join` does.
        let held = kjk_game::sim_host::held_pitch(&last, GUEST);
        look.turn(guest_mouse(ms, rate.time_ms(1)), held);
        let command = guest_command(ms, &look);
        input.insert(GUEST, command);
        last = step(
            &mut world,
            tick,
            &input,
            rate.msec(tick),
            StepReason::Authority,
        );
        ticks += 1;
        let body = guest_body(&last);
        let (lost, why) = match body {
            None => {
                half.own = false;
                (true, "no body")
            }
            Some(entity) => {
                driven += 1;
                let Some(fighter) = entity.fighter else {
                    continue;
                };
                // A foe fighting it, not only its own swings at the air.
                let fought = last.entities.iter().any(|other| {
                    other
                        .fighter
                        .is_some_and(|f| f.target == Some(entity.id) && f.alive())
                });
                if fought {
                    fighting += 1;
                }
                let own = fighter.alive() && !fighter.force.stunned(last.time_ms);
                half.own &= own && !fighter.mind.steered;
                if let Some(player) = entity.player {
                    half.lowest = half.lowest.min(player.view[0]);
                    half.highest = half.highest.max(player.view[0]);
                }
                if fighter.mind.steered {
                    steered += 1;
                    (true, "steered")
                } else {
                    (false, "")
                }
            }
        };
        let open = spans
            .last_mut()
            .filter(|span| span.to_ms + rate.time_ms(1) >= ms && span.why == why);
        match (lost, open) {
            (true, Some(span)) => span.to_ms = ms,
            (true, None) => spans.push(Span {
                from_ms: ms,
                to_ms: ms,
                why,
                told: body.map_or_else(String::new, |entity| queue_of(&world, entity.id.0)),
            }),
            _ => {}
        }
    }
    outln!(
        "combat coop {module}: {seconds} s ({ticks} ticks); the guest drove its companion {driven}, in combat {fighting}, steered {steered}, half swings short of the limit {held_halves}"
    );
    outln!(
        "  the guest drives {:?}; the host ends at {:?}, the nearest hostile {:?}",
        guest_body(&last).map(|entity| entity.id),
        last.body(Seat(0)).map(|entity| entity.position),
        nearest_hostile(world.level(), &last)
    );
    for span in spans.iter().take(40) {
        outln!(
            "  {} to {} ms: {}; {}",
            span.from_ms,
            span.to_ms,
            span.why,
            span.told
        );
    }
    if fighting == 0 {
        outln!("  no foe fought the guest's companion");
    }
    if spans.is_empty() {
        outln!("  the guest kept its companion and its view throughout");
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
