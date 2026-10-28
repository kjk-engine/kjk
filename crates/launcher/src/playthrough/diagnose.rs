//! What holds the player, for a blocker's report.

use super::*;

/// What holds the player, for a blocker's report.
pub(super) fn diagnose(
    here: &Here,
    world: &sim::SimWorld,
    me: &sim::EntityState,
    goal: &Goal,
    walker: Option<&Walker>,
) -> Vec<String> {
    let (level, snapshot) = (here.level, here.snapshot);
    let mut held = vec![format!(
        "the player at ({:.2}, {:.2}, {:.2}), HP {}, level {}, wielding {:?}",
        me.position[0],
        me.position[1],
        me.position[2],
        me.fighter.map_or(0, |f| f.hp / 100),
        me.sheet.map_or(0, |sheet| sheet.level),
        me.sheet.map(|sheet| sheet.main_weapon().delivery)
    )];
    // What may keep it from moving: off the floor, using an item, held.
    let now = snapshot.time_ms;
    let mut state = Vec::new();
    if let Some(player) = me.player.filter(|player| player.ground.is_none()) {
        state.push(format!("off the floor (flags {:#x})", player.flags));
    }
    if let Some(fighter) = me.fighter {
        if let Some((_, until)) = fighter.pack.using {
            state.push(format!("using an item until {until} ms"));
        }
        if fighter.force.stunned(now) {
            state.push("stunned or held".into());
        }
        if !fighter.alive() {
            state.push("down".into());
        }
    }
    if !state.is_empty() {
        held.push(format!("the player is {}", state.join(", ")));
    }
    // Bodies and placeables close enough to stand in the player's way.
    let beside: Vec<String> = snapshot
        .entities
        .iter()
        .filter(|entity| {
            entity.id != me.id
                && (entity.fighter.is_some()
                    || matches!(entity.kind, sim::Kind::Object(ObjectKind::Placeable, _)))
                && flat(entity.position, me.position) < 2.0
        })
        .map(|entity| {
            let name = match entity.kind {
                sim::Kind::Object(_, index) => here.facts(index)
                    .map_or_else(|| format!("#{index}"), |data| data.tag.clone()),
                kind => format!("{kind:?}"),
            };
            format!(
                "{name} at ({:.2}, {:.2}, {:.2}){}",
                entity.position[0],
                entity.position[1],
                entity.position[2],
                match entity.fighter {
                    Some(f) if !f.alive() => ", dead",
                    Some(_) => "",
                    None => ", no fighter",
                }
            )
        })
        .collect();
    if !beside.is_empty() {
        held.push(format!("beside the player: {}", beside.join("; ")));
    }
    if let Some(talk) = snapshot.talk.holding(ObjectId(me.id.0)) {
        held.push(format!(
            "in {} at E{}{}{}",
            talk.dialog,
            talk.entry,
            if talk.paused { ", paused" } else { "" },
            if talk.until_ms == u64::MAX && talk.replies.is_empty() {
                ", waiting with no reply offered"
            } else {
                ""
            }
        ));
        // A paused conversation waits on its owner's queue.
        if let Some(owner) = snapshot.get(sim::SimId(talk.owner.0)) {
            let queue = world.script_state().actions.get(&talk.owner.0);
            held.push(format!(
                "its owner #{} at ({:.2}, {:.2}), {:.1} m away; queued: {}",
                talk.owner.0,
                owner.position[0],
                owner.position[1],
                flat(owner.position, me.position),
                queue.map_or_else(
                    || "nothing".to_string(),
                    |queue| queue
                        .iter()
                        .map(|queued| format!("{:?}", queued.action))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            ));
        }
    }
    // A paused conversation waits on whoever resumes it: every queue
    // with work in it.
    if snapshot.talk.holding(ObjectId(me.id.0)).is_some() {
        for (id, queue) in &world.script_state().actions {
            let Some(head) = queue.first() else {
                continue;
            };
            let at = snapshot
                .get(sim::SimId(*id))
                .map_or([f32::NAN; 3], |entity| entity.position);
            // A move: where to, and whether a way leads there.
            let to = match head.action {
                sim::script::actions::Action::Move { to, .. } => match to {
                    sim::script::actions::Target::Object(object) => {
                        snapshot.get(sim::SimId(object.0)).map(|e| e.position)
                    }
                    sim::script::actions::Target::Point(point, _) => Some(point),
                },
                _ => None,
            };
            let way = to.map_or_else(String::new, |to| {
                let from = snapshot
                    .get(sim::SimId(*id))
                    .and_then(|entity| entity.player)
                    .map(|ps| ps.origin);
                // Where a body stands there (its origin, units).
                let goal = [
                    to[0] / UNIT,
                    to[1] / UNIT,
                    to[2] / UNIT - sim::motion::SOLE_Z,
                ];
                let corners = from.and_then(|from| here.route(from, goal, false));
                let leads = corners.is_some();
                let corners = corners.map_or_else(String::new, |corners| {
                    corners
                        .iter()
                        .take(4)
                        .map(|c| format!("({:.2}, {:.2})", c[0] * UNIT, c[1] * UNIT))
                        .collect::<Vec<_>>()
                        .join(" ")
                });
                let between = if leads {
                    String::new()
                } else {
                    on_the_way(here, at, to).join(", ")
                };
                format!(
                    " to ({:.2}, {:.2}, {:.2}), {}{}",
                    to[0],
                    to[1],
                    to[2],
                    if leads {
                        format!("a way leads there by {corners}")
                    } else {
                        "no way there".to_string()
                    },
                    if between.is_empty() {
                        String::new()
                    } else {
                        format!(" (on the way: {between})")
                    }
                )
            });
            // What its mind does with it.
            let mind = snapshot
                .get(sim::SimId(*id))
                .and_then(|entity| entity.fighter)
                .map_or_else(String::new, |fighter| {
                    format!(
                        "; HP {}, its mind {:?}, walker {:?}, forward {}, moving {:?}",
                        fighter.hp / 100,
                        fighter.mind.task,
                        fighter
                            .mind
                            .walker
                            .map(|w| (w.goal.map(|v| v * UNIT), w.stalls)),
                        fighter.last_command.forward,
                        snapshot
                            .get(sim::SimId(*id))
                            .and_then(|e| e.player)
                            .map(|ps| (
                                ps.velocity,
                                ps.legs.anim,
                                ps.flags,
                                ps.hold_ms,
                                ps.run_speed
                            ))
                    )
                });
            // Deferred code by its script alone (its stack says nothing).
            let action = match &head.action {
                sim::script::actions::Action::Code { script, .. } => format!("code of {script}"),
                action => format!("{action:?}"),
            };
            let tag = snapshot
                .get(sim::SimId(*id))
                .and_then(|e| match e.kind {
                    sim::Kind::Object(_, index) => here.facts(index),
                    _ => None,
                })
                .map_or_else(String::new, |data| format!("{} ", data.tag));
            held.push(format!(
                "{tag}#{id} at ({:.2}, {:.2}) is on {action}{way}, {} queued{mind}",
                at[0],
                at[1],
                queue.len()
            ));
        }
    }
    // Security without the spike asked for: the party's spikes.
    if let Goal::Unlock(_, Some(_)) = goal {
        let spikes: Vec<&str> = snapshot
            .goods
            .carried(sim::inventory::PARTY)
            .map(|(_, item)| item.template.as_str())
            .filter(|template| template.contains("spike"))
            .collect();
        held.push(format!("the party's spikes: {spikes:?}"));
    }
    let target = match goal {
        Goal::To(target)
        | Goal::Use(target)
        | Goal::Exit(target)
        | Goal::Unlock(target, _)
        | Goal::Bash(target)
        | Goal::Mine(target)
        | Goal::Put(_, target) => Some(target),
        _ => None,
    };
    if let Some(Target::Tag(tag, nth)) = target {
        match find(here, tag, *nth) {
            Some((index, at)) => {
                // A creature: its life, and whether the player sees it.
                let entity = snapshot
                    .entities
                    .iter()
                    .find(|e| matches!(e.kind, sim::Kind::Object(_, i) if i == index));
                let seen = entity
                    .and_then(|e| e.player)
                    .zip(me.player)
                    .map(|(it, eye)| {
                        level.sees(here.sight(), mind::eye(&eye), mind::chest(&it))
                    });
                let foe = entity.is_some_and(|e| {
                    hostiles(here, world, at, 1.0)
                        .iter()
                        .any(|(id, _)| *id == e.id)
                });
                let life = entity.and_then(|e| e.fighter).map_or(String::new(), |f| {
                    format!(
                        ", {:?} hp {}, faction {:?}, foe {foe}, z {:.2}, seen {seen:?}, in a fight to {} ms, target {:?}",
                        f.life, f.hp, f.faction, at[2], f.combat_until, f.target
                    )
                });
                held.push(format!(
                    "{tag} at ({:.2}, {:.2}), {:.1} m away, state {}{life}",
                    at[0],
                    at[1],
                    flat(at, me.position),
                    object_of(here, index).state
                ))
            }
            None => held.push(format!(
                "no object tagged {tag}{} in the module",
                nth.map_or_else(String::new, |nth| format!(" (#{nth})"))
            )),
        }
    }
    // Any other goal held in combat (an exit refused, a ramp's script
    // waiting it out): what keeps the party in it, as the sim reads it
    // (`SimWorld::combat_keepers`), plot ones too.
    if snapshot.in_combat && !matches!(goal, Goal::Fight(_)) {
        held.push("the party is in combat (no leaving)".into());
        for keeper in keepers(here, world, me.position) {
            held.push(format!(
                "keeping it: {} #{} at ({:.2}, {:.2}, {:.2}), {:.1} m from {}, its target {:?}{}",
                keeper.tag,
                keeper.id.0,
                keeper.at[0],
                keeper.at[1],
                keeper.at[2],
                keeper.near.1,
                keeper.near.0,
                keeper.target.map(|t| t.0),
                if keeper.plot { ", plot" } else { "" }
            ));
        }
    }
    if let Goal::Fight(radius) = goal {
        for (id, at) in hostiles(here, world, me.position, *radius) {
            let foe = snapshot.get(id);
            let tag = here.tag_of(foe);
            let fighter = foe.and_then(|foe| foe.fighter);
            held.push(format!(
                "foe {tag} #{} at ({:.2}, {:.2}, {:.2}), {:.1} m away, HP {}{}, its target {:?}, struck last by {:?}",
                id.0,
                at[0],
                at[1],
                at[2],
                flat(at, me.position),
                fighter.map_or(0, |f| f.hp / 100),
                fighter
                    .and_then(|f| f.pack.shield)
                    .map_or(String::new(), |shield| format!(" (a shield of {} left)", shield.left)),
                fighter.and_then(|f| f.target).map(|t| t.0),
                fighter.and_then(|f| f.last_attacker).map(|(by, ms)| (by.0, ms))
            ));
        }
    }
    if let Some(walker) = walker {
        let doors = |index| here.door(index);
        let player = me.player.expect("the player moves");
        let metres = |p: [f32; 3]| {
            format!(
                "({:.2}, {:.2}, {:.2})",
                p[0] * UNIT,
                p[1] * UNIT,
                p[2] * UNIT
            )
        };
        match here.route(player.origin, walker.goal, false) {
            None => {
                held.push(format!(
                    "no way there over the navigation mesh ({}); on the straight way: {}",
                    sim::walk::why_none(level, player.origin, walker.goal),
                    on_the_way(here, me.position, walker.goal.map(|v| v * UNIT))
                        .join(", ")
                ));
                // What shuts in where the player can get to.
                let placed = level.placed();
                let shut: Vec<String> = sim::walk::frontier(level, player.origin, &doors)
                    .iter()
                    .take(12)
                    .map(|(at, entity)| {
                        let what = entity
                            .and_then(|index| placed.get(index as usize))
                            .map_or("furniture".to_string(), |object| {
                                format!("{:?} {}", object.kind, object.tag)
                            });
                        format!("{what} at ({:.2}, {:.2}, {:.2})", at[0], at[1], at[2])
                    })
                    .collect();
                held.push(format!("shut in by: {}", shut.join(", ")));
                // The way with every door open, and the shut ones on it.
                let open = |_| sim::ObjectState {
                    state: 1,
                    ..sim::ObjectState::default()
                };
                if let Some(corners) = sim::walk::route(level, player.origin, walker.goal, &open) {
                    let legs: Vec<([f32; 3], [f32; 3])> = std::iter::once(player.origin)
                        .chain(corners.iter().copied())
                        .zip(corners.iter().copied())
                        .map(|(a, b)| (a.map(|v| v * UNIT), b.map(|v| v * UNIT)))
                        .collect();
                    let shut: Vec<String> = here
                        .objects()
                        .iter()
                        .filter(|(index, data)| {
                            data.kind == ObjectKind::Door
                                && !sim::walk::passable(level, *index, doors(*index))
                                && legs
                                    .iter()
                                    .any(|(a, b)| off_the_way(*a, *b, data.position) <= 4.0)
                        })
                        .map(|(index, data)| {
                            format!(
                                "{} #{index} at ({:.2}, {:.2}), {:?}",
                                data.tag, data.position[0], data.position[1], data.opens
                            )
                        })
                        .collect();
                    held.push(format!(
                        "shut on the way with every door open: {}",
                        shut.join(", ")
                    ));
                }
            }
            Some(corners) => held.push(format!(
                "walking to {} by {} corners, the first {}, making for {}; stalled {} times",
                metres(walker.goal),
                corners.len(),
                corners.first().map_or_else(String::new, |c| metres(*c)),
                walker.waypoint.map_or_else(|| "nothing".into(), metres),
                walker.stalls
            )),
        }
        // What stands in the way of a stalled walk.
        if walker.stalls > 0 {
            for entity in &snapshot.entities {
                let d = flat(entity.position, me.position);
                if entity.id == me.id || d > 2.0 {
                    continue;
                }
                let tag = here.tag_of(Some(entity));
                held.push(format!(
                    "near: {:?} {tag} #{} at ({:.2}, {:.2}, {:.2}), {d:.1} m, {}",
                    entity.kind,
                    entity.id.0,
                    entity.position[0],
                    entity.position[1],
                    entity.position[2],
                    match entity.fighter {
                        Some(f) if f.downed() => "down",
                        Some(f) if !f.alive() => "dead",
                        Some(_) => "alive",
                        None => "",
                    }
                ));
            }
        }
    }
    let queued = world
        .script_state()
        .actions
        .get(&me.id.0)
        .map_or(0, |actions| actions.len());
    if queued > 0 {
        held.push(format!("{queued} script actions queued on the player"));
    }
    held
}
