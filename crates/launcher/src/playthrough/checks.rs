//! The harness's own checks (`kjk sim probes`): the chase, shut doors,
//! walls, quiet talks, stragglers, the fallen.

use super::*;

/// What a probe's chase is set up with before its step (`probes`): the
/// chase, its walk, the foe's id and place, the player.
pub(super) type Prepare<'a> =
    &'a dyn Fn(&mut Chase, &mut Option<Walker>, (sim::SimId, [f32; 3]), &sim::motion::MotionState);

/// A probe's chase step: the command, whether it walks, and its walk after.
pub(super) type Chased = Option<(Command, bool, Option<Walker>)>;

/// The checks of a fight's chase (`Chase`), the rule every fight of the
/// harness goes by, a `fight`'s and any goal's fight back, for `kjk sim
/// probes` and the approved test `harness_chase`: on the arena (no
/// navigation mesh, so no way leads anywhere), the player and one foe due
/// west of it. A blade swings at a foe within the sim's swing
/// (`mind::STRIKE`), stepping in while beyond its reach (`mind::REACH`), and
/// stands to swing within it; a foe out of its swing that no way leads to is
/// gone at straight, and so is one the walk to it arrived short of (in
/// Dantooine's ruins the fight back stood where its walk had ended, short of
/// a strafing guard droid, and swung at the air out of reach while the droid
/// held the party in combat). A gun fires at a foe it sees within `SHOOT_M`,
/// and from where it stands at one seen that no way leads to (a far shot).
pub fn probes() -> sim::probe::Checks {
    let mut world = match sim::SimWorld::new(sim::WorldConfig::one_seat(), sim::Level::arena()) {
        Ok(world) => world,
        Err(error) => return vec![("chase arena", Err(error.to_string()))],
    };
    let base = world.snapshot();
    let level = world.level().clone();
    // A chase with `sheet` against a foe `d` metres away, its state first
    // set by `prepare` (the chase, its walk, the foe's id and place, the
    // player): the command it gives, whether it walks, and its walk after.
    let chase_once = |sheet: sim::combat::Sheet, d: f32, prepare: Prepare<'_>| -> Chased {
        let mut snapshot = base.clone();
        let mine = snapshot
            .entities
            .iter()
            .position(|e| e.kind == sim::Kind::Avatar(Seat(0)))?;
        snapshot.entities[mine].sheet = Some(sheet);
        let me = snapshot.entities[mine];
        let player = me.player?;
        let foe = sim::EntityState {
            id: sim::SimId(u32::MAX - 1),
            kind: sim::Kind::Object(ObjectKind::Creature, 0),
            position: [me.position[0] - d, me.position[1], me.position[2]],
            player: Some(sim::motion::MotionState {
                origin: [player.origin[0] - d / UNIT, player.origin[1], player.origin[2]],
                ..player
            }),
            ..me
        };
        snapshot.entities.push(foe);
        let refused = BTreeSet::new();
        let here = Here::new(&level, &snapshot, &refused);
        let mut chase = Chase::default();
        let mut walker = None;
        prepare(&mut chase, &mut walker, (foe.id, foe.position), &player);
        let mut acts = Vec::new();
        let (command, stepping) = chase.strike(
            &mut walker,
            &here,
            &world,
            &mut Reach::default(),
            &player,
            &me,
            (foe.id, foe.position),
            0,
            &mut acts,
        );
        Some((command, stepping, walker))
    };
    // The command a chase gives with `sheet` against a foe `d` metres away,
    // its walk planned and arrived where the player stands when `arrived`.
    let strike = |sheet: sim::combat::Sheet, d: f32, arrived: bool| -> Option<(Command, bool)> {
        chase_once(sheet, d, &|chase, walker, (id, at), player| {
            if arrived {
                *walker = Some(Walker::new(player.origin));
                chase.planned = Some((id, at));
            }
        })
        .map(|(command, stepping, _)| (command, stepping))
    };
    let (close, swing) = melee_m();
    let blade = sim::combat::Sheet::dev_jedi();
    let gun = sim::combat::Sheet::dev_soldier();
    // Fists reach shorter than a blade: the chase steps in to the sim's own
    // punch (Telos's Entertainment Module, two mercenaries at the bar: the
    // Exile stood 1.3 m off, inside a blade's reach and out of his punch's,
    // pressing attack at the air).
    let fists = sim::combat::Sheet::commoner();
    let punch = sim::player_punch_reach_m();
    let cases: [(&'static str, sim::combat::Sheet, f32, bool, bool, i8); 8] = [
        ("chase_punches_stepping_in", fists, punch * 0.9, false, true, 127),
        ("chase_out_of_a_punch_walks_in", fists, punch + 0.1, false, false, 127),
        ("chase_swings_stepping_in", blade, (close + swing) / 2.0, false, true, 127),
        ("chase_swings_standing_within_reach", blade, close / 2.0, false, true, 0),
        ("chase_goes_straight_at_one_no_way_leads_to", blade, swing + 0.6, false, false, 127),
        ("chase_arrived_short_goes_straight_on", blade, swing + 0.6, true, false, 127),
        ("chase_shoots_one_seen_within_reach", gun, SHOOT_M - 1.0, false, true, 0),
        ("chase_shoots_far_at_one_no_way_leads_to", gun, SHOOT_M + 3.0, false, true, 0),
    ];
    cases
        .into_iter()
        .map(|(name, sheet, d, arrived, attacks, forward)| {
            let delivery = sheet.main_weapon().delivery;
            let result = match strike(sheet, d, arrived) {
                None => Err("no player on the arena".to_string()),
                Some((command, stepping)) => {
                    let attacked = command.buttons & buttons::ATTACK != 0;
                    let detail = format!(
                        "{delivery:?} at {d:.2} m: attack {attacked}, forward {}, walking {stepping}",
                        command.forward
                    );
                    if attacked == attacks && command.forward == forward {
                        Ok(detail)
                    } else {
                        Err(detail)
                    }
                }
            };
            (name, result)
        })
        .chain(replans(&chase_once))
        .chain(hovering(&world, &base))
        .chain(loots())
        .chain(picks_remains())
        .chain(picks())
        .chain(walks_for())
        .chain(shut_doors(&base, &level))
        .collect()
}

/// A gun's chase at a body met where its model is drawn: TSL's sensor ball
/// (`c_senball`, 1.33 m to 1.66 m over its soles, `kjk combat bodies tsl`),
/// 5 m off. The crosshair line the chase aims meets its body (aimed 0.3 m
/// over its origin, under it, the prologue's droid was never hit). And a
/// mine placed and gone (set off) is `sprung`; one still there is not.
pub(super) fn hovering(world: &sim::SimWorld, base: &Snapshot) -> sim::probe::Checks {
    use collision::{BoxWorld, CollisionWorld, Contents, Extents};
    let ball = sim::module::Bulk {
        min: [-0.15, -0.17, 1.33],
        max: [0.15, 0.13, 1.66],
    };
    let Some(module) = sim::determinism::proving_ground().module().map(|m| (**m).clone()) else {
        return vec![("chase_shoots_a_hovering_droid", Err("the proving ground has no module".into()))];
    };
    let droid = ObjectData {
        kind: ObjectKind::Creature,
        body: Some(sim::module::Body {
            appearance: 571,
            size: 0,
            movement: 0,
            walk_mps: 0.0,
            run_mps: 0.0,
            head_arc_h: 0.0,
            bag: 0,
            destroy_ms: 0,
            fade_ms: None,
            bulk: Some(ball),
        }),
        ..ObjectData::unknown("droid", [0.0; 3], 0.0)
    };
    let mine = ObjectData {
        kind: ObjectKind::Trigger,
        trap: Some(sim::traps::Trap {
            one_shot: true,
            ..Default::default()
        }),
        tag: "mine".into(),
        ..ObjectData::unknown("mine", [0.0; 3], 0.0)
    };
    let level = world.level().clone().with_module(sim::module::ModuleData {
        placements: vec![sim::module::Placement::Own(droid), sim::module::Placement::Own(mine)],
        ..module
    });
    let mut snapshot = base.clone();
    let Some(me) = snapshot
        .entities
        .iter()
        .find(|e| e.kind == sim::Kind::Avatar(Seat(0)))
        .copied()
    else {
        return vec![("chase_shoots_a_hovering_droid", Err("no player on the arena".into()))];
    };
    let me = sim::EntityState {
        sheet: Some(sim::combat::Sheet::dev_soldier()),
        ..me
    };
    let (Some(player), d) = (me.player, 5.0) else {
        return vec![("chase_shoots_a_hovering_droid", Err("no player on the arena".into()))];
    };
    let foe_ps = sim::motion::MotionState {
        origin: [player.origin[0] - d / UNIT, player.origin[1], player.origin[2]],
        ..player
    };
    let foe = sim::EntityState {
        id: sim::SimId(u32::MAX - 1),
        kind: sim::Kind::Object(ObjectKind::Creature, 0),
        position: [me.position[0] - d, me.position[1], me.position[2]],
        player: Some(foe_ps),
        ..me
    };
    snapshot.entities.push(foe);
    let refused = BTreeSet::new();
    let shot = {
        let here = Here::new(&level, &snapshot, &refused);
        let (command, _) = Chase::default().strike(
            &mut None,
            &here,
            world,
            &mut Reach::default(),
            &player,
            &me,
            (foe.id, foe.position),
            0,
            &mut Vec::new(),
        );
        let angle = |axis: usize| {
            sim::math::short_to_degrees(command.angles[axis].wrapping_add(player.view_offset[axis]))
        };
        let (forward, ..) = sim::math::angle_vectors([angle(0), angle(1), 0.0]);
        // Bodies stand in metres.
        let from = mind::crosshair(&player).map(|v| v * UNIT);
        let to = sim::math::mad(from, 20.0, forward);
        let bodies = [sim::body(foe.id, &foe_ps, Some(&ball))];
        let met = collision::Bodies::new(&BoxWorld::new(), &bodies)
            .trace(from, to, &Extents::POINT, Contents::BODY)
            .fraction
            < 1.0;
        let detail = format!(
            "attack {}, the crosshair line pitched {:.1} degrees meets the ball {met}",
            command.buttons & buttons::ATTACK != 0,
            angle(0)
        );
        sim::probe::check(met && command.buttons & buttons::ATTACK != 0, detail)
    };
    // The mine (number 1) gone, then standing.
    let sprung_when = |standing: bool| {
        let mut snapshot = base.clone();
        if standing {
            snapshot.entities.push(sim::EntityState {
                id: sim::SimId(u32::MAX - 2),
                kind: sim::Kind::Object(ObjectKind::Trigger, 1),
                player: None,
                fighter: None,
                ..me
            });
        }
        sprung(&Here::new(&level, &snapshot, &refused), "mine", None)
    };
    let (gone, there) = (sprung_when(false), sprung_when(true));
    vec![
        ("chase_shoots_a_hovering_droid_where_it_is_drawn", shot),
        (
            "to_a_mine_set_off_is_met",
            sim::probe::check(gone && !there, format!("gone: sprung {gone}; standing: sprung {there}")),
        ),
    ]
}

/// A container's use (`loot_step`): opened and emptied first; closed once
/// it holds nothing after an open (not at once: an open the player's action
/// has yet to carry out refuses the take all sent with it); opened again a
/// press's time on while it still holds something, and closed holding it
/// after `LOOT_OPENS` opens.
pub(super) fn loots() -> sim::probe::Checks {
    let press = 4 * PRESS_MS;
    let cases: [(&'static str, bool, u8, u64, Loot); 6] = [
        ("loot_opens_first", true, 0, 0, Loot::Open),
        ("loot_opens_an_empty_one_first", false, 0, 0, Loot::Open),
        ("loot_waits_for_the_take_all", true, 1, press / 2, Loot::Wait),
        ("loot_opens_again_while_it_holds", true, 1, press, Loot::Open),
        ("loot_closes_once_empty", false, 1, press / 2, Loot::Close),
        ("loot_closes_holding_after_its_opens", true, LOOT_OPENS, press, Loot::Close),
    ];
    cases
        .into_iter()
        .map(|(name, holds, opens, since, want)| {
            let (pressed_at, now) = if opens == 0 { (0, 1000) } else { (1000, 1000 + since) };
            let got = loot_step(holds, opens, now, pressed_at);
            let detail = format!("holds {holds}, {opens} opens, {since} ms since the last: {got:?}");
            (name, if got == want { Ok(detail) } else { Err(detail) })
        })
        .collect()
}

/// The remains a use of one dead loots (`pick_remains`), its own (1) lying
/// where it fell and a neighbour's (2) a metre off, as Tatooine's Dark Jedi
/// fell: its own while they hold something; once emptied by its take all,
/// still its own (to be closed), not the neighbour's; not yet opened and
/// emptied (another use's), the neighbour's; none past `REMAINS_M`.
pub(super) fn picks_remains() -> sim::probe::Checks {
    let remains = |index: u32, d: f32, holds: bool| Standing {
        index,
        at: [d, 0.0, 0.0],
        d,
        holds,
    };
    let far = REMAINS_M + 1.0;
    // Name, whether its own hold anything, the remains the use opened, how
    // far off the neighbour's stand, and the remains looted.
    type Case = (&'static str, bool, Option<u32>, f32, Option<u32>);
    let cases: [Case; 4] = [
        ("remains_own_first", true, None, 1.0, Some(1)),
        ("remains_opened_kept_once_emptied", false, Some(1), 1.0, Some(1)),
        ("remains_neighbours_when_own_emptied_before", false, None, 1.0, Some(2)),
        ("remains_none_past_their_reach", false, None, far, None),
    ];
    cases
        .into_iter()
        .map(|(name, own_holds, opened, neighbour_d, want)| {
            let got = pick_remains([remains(1, 0.0, own_holds), remains(2, neighbour_d, true)], opened)
                .map(|picked| picked.index);
            let detail = format!("own holds {own_holds}, opened {opened:?}, neighbour {neighbour_d} m off: {got:?}");
            (name, if got == want { Ok(detail) } else { Err(detail) })
        })
        .collect()
}

/// The walk to a target (`walk_for`): one made for another spot farther
/// than `REPLAN_M` off (a chase's, for its foe) is planned again; one made
/// for the target is kept.
pub(super) fn walks_for() -> sim::probe::Checks {
    let target = [0.0, 0.0, 0.0];
    let cases: [(&'static str, f32, bool); 2] = [
        ("walk_for_another_spot_planned_again", REPLAN_M + 2.0, true),
        ("walk_for_its_target_kept", REPLAN_M / 2.0, false),
    ];
    cases
        .into_iter()
        .map(|(name, off, replanned)| {
            let mut walker = Some(Walker::new([off / UNIT, 0.0, 0.0]));
            let planned = walk_for(&mut walker, target, || Walker::new(target)).goal != [off / UNIT, 0.0, 0.0];
            let detail = format!("a walk {off:.1} m off its target: planned again {planned}");
            (name, if planned == replanned { Ok(detail) } else { Err(detail) })
        })
        .collect()
}

/// The foe a fight takes (`Chase::pick`): the one it fights while that one
/// still fits, though another stands as near or nearer; else the nearest
/// that fits (none fought, the one fought left out or gone).
pub(super) fn picks() -> sim::probe::Checks {
    let (near, far, gone) = (sim::SimId(1), sim::SimId(2), sim::SimId(3));
    let foes = vec![(near, [17.55, 0.0, 0.0]), (far, [17.6, 0.0, 0.0])];
    let cases: [(&'static str, Option<sim::SimId>, Option<sim::SimId>, sim::SimId); 4] = [
        ("pick_keeps_the_foe_fought", Some(far), None, far),
        ("pick_takes_the_nearest_first", None, None, near),
        ("pick_leaves_the_foe_fought_once_it_no_longer_fits", Some(far), Some(far), near),
        ("pick_takes_the_nearest_once_the_foe_fought_is_gone", Some(gone), None, near),
    ];
    cases
        .into_iter()
        .map(|(name, fought, unfit, expected)| {
            let chase = Chase {
                foe: fought,
                ..Chase::default()
            };
            let picked = chase.pick(foes.clone(), |(id, _)| Some(*id) != unfit).map(|(id, _)| id);
            let detail = format!("fought {fought:?}, left out {unfit:?}: picked {picked:?}");
            (name, if picked == Some(expected) { Ok(detail) } else { Err(detail) })
        })
        .collect()
}

/// A door pressed this goal that stayed shut is not pressed again in
/// passing (`closed_door_near`), but the goal's own is; a use or an exit
/// whose way one shut
/// fights the foes near first (`fights_first`); a walk, or a use with no
/// door refused, does not.
pub(super) fn shut_doors(base: &Snapshot, level: &sim::Level) -> sim::probe::Checks {
    let mut snapshot = base.clone();
    let Some(me) = snapshot
        .entities
        .iter()
        .find(|e| e.kind == sim::Kind::Avatar(Seat(0)))
        .copied()
    else {
        return vec![("door_refused_not_pressed", Err("no player on the arena".into()))];
    };
    let door = sim::EntityState {
        id: sim::SimId(u32::MAX - 2),
        kind: sim::Kind::Object(ObjectKind::Door, 0),
        position: [me.position[0] + 1.0, me.position[1], me.position[2]],
        player: None,
        object: Some(sim::ObjectState::default()),
        ..me
    };
    snapshot.entities.push(door);
    let (none, refused) = (BTreeSet::new(), BTreeSet::from([0]));
    let near = |refused: &BTreeSet<u32>, own: Option<u32>| {
        closed_door_near(&Here::new(level, &snapshot, refused), me.position, false, own)
            .map(|(i, _)| i)
    };
    let (pressed, passed, goal) = (near(&none, None), near(&refused, None), near(&refused, Some(0)));
    let at = || Target::Place([0.0, 0.0]);
    let decided = [
        fights_first(&Goal::Use(at()), false, true, true),
        fights_first(&Goal::Use(at()), false, true, false),
        fights_first(&Goal::Use(at()), true, true, false),
        fights_first(&Goal::Exit(at()), false, true, false),
        fights_first(&Goal::To(at()), false, true, true),
    ];
    vec![
        (
            "door_refused_not_pressed",
            if pressed == Some(0) && passed.is_none() {
                Ok(format!("pressed {pressed:?}, once refused {passed:?}"))
            } else {
                Err(format!("pressed {pressed:?}, once refused {passed:?}"))
            },
        ),
        (
            "goal_door_pressed_again",
            if goal == Some(0) {
                Ok(format!("the goal's own door, refused: pressed {goal:?}"))
            } else {
                Err(format!("the goal's own door, refused: pressed {goal:?}"))
            },
        ),
        (
            "use_fights_past_a_shut_door",
            if decided == [true, true, false, true, false] {
                Ok(format!(
                    "use shut, use in combat, fought, exit in combat, walk shut: {decided:?}"
                ))
            } else {
                Err(format!(
                    "use shut, use in combat, fought, exit in combat, walk shut: {decided:?}"
                ))
            },
        ),
    ]
}

/// The checks of the walls a walk plans round (`TurnedBack`), for `kjk sim
/// probes --only "harness walls"`: on a 40 m by 20 m floor, a walker
/// standing a little into it going round a body; a trigger 2 m
/// wide across the way from the player to a goal 16 m east. Turned back by
/// it, the walk goes round it by its open end, no leg in it; with no open
/// end, through it. A trigger the player stood in while talking and was
/// jumped out of turned it back; one it walked out of did not. A walk
/// approached off the mesh makes for the floor it would from the mesh
/// (`Here::approach`).
pub fn wall_probes() -> sim::probe::Checks {
    let mut world = match sim::SimWorld::new(sim::WorldConfig::one_seat(), sim::Level::arena()) {
        Ok(world) => world,
        Err(error) => return vec![("walls arena", Err(error.to_string()))],
    };
    let snapshot = world.snapshot();
    let floor: Vec<[[f32; 3]; 3]> = (-10..10)
        .flat_map(|i| (-5..5).map(move |j| (i as f32 * 2.0, j as f32 * 2.0)))
        .flat_map(|(x, y)| {
            let [a, b, c, d] =
                [[x, y], [x + 2.0, y], [x + 2.0, y + 2.0], [x, y + 2.0]].map(|[x, y]| [x, y, 0.0]);
            [[a, b, c], [a, c, d]]
        })
        .collect();
    let nav = collision::nav::NavMesh::build(&floor, sim::walk::CLEARANCE * UNIT);
    let level = world.level().clone().with_nav(nav);
    let origin = |x: f32, y: f32| [x / UNIT, y / UNIT, -sim::motion::SOLE_Z];
    let (from, to) = (origin(-8.0, 0.0), origin(8.0, 0.0));
    // The trigger across the way, from y -10 m to `reach`.
    let band = |reach: f32| ObjectData {
        kind: ObjectKind::Trigger,
        geometry: vec![
            [-1.0, -10.0, 0.0],
            [1.0, -10.0, 0.0],
            [1.0, reach, 0.0],
            [-1.0, reach, 0.0],
        ],
        ..ObjectData::unknown("band", [0.0; 3], 0.0)
    };
    // The way from `from` to `to` round `trigger`: whether it goes round
    // it, and whether any leg (sampled every 10 cm) enters it.
    let walk = |trigger: &ObjectData| {
        let walled: Arc<[bool]> = sim::walk::faces_under(&level, trigger).into();
        let refused = BTreeSet::new();
        let here = Here::new(&level, &snapshot, &refused).walled(&walled);
        let corners = here.route(from, to, true)?;
        let enters = std::iter::once(from)
            .chain(corners)
            .collect::<Vec<_>>()
            .windows(2)
            .any(|leg| {
                let (a, b) = (leg[0].map(|v| v * UNIT), leg[1].map(|v| v * UNIT));
                let steps = (flat(a, b) / 0.1).ceil().max(1.0) as usize;
                (0..=steps).any(|k| {
                    let t = k as f32 / steps as f32;
                    let at = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
                    sim::module::inside_outline(&trigger.geometry, at)
                })
            });
        Some((here.goes_round(from, to), enters))
    };
    let routed = |name: &'static str, reach: f32, round: bool| {
        let result = match walk(&band(reach)) {
            None => Err("no way to the goal".to_string()),
            Some((went_round, enters)) => {
                let detail = format!(
                    "a trigger to y {reach} m: round it {went_round}, a leg in it {enters}"
                );
                if went_round == round && enters != round {
                    Ok(detail)
                } else {
                    Err(detail)
                }
            }
        };
        (name, result)
    };
    // A conversation held at (0, 0) in trigger 7, then the player at
    // `after` and out of it: whether it turned the player back.
    let left = |name: &'static str, after: [f32; 3], turns: bool| {
        let mut turned = TurnedBack::default();
        let inside = BTreeSet::from([7]);
        turned.note("walls", [0.0; 3], true, || inside.clone());
        let back = turned.note("walls", after, false, BTreeSet::new);
        let detail = format!(
            "out of it {:.2} m on: turned back {back}, walls {:?}",
            flat([0.0; 3], after),
            turned.triggers
        );
        (
            name,
            if back == turns {
                Ok(detail)
            } else {
                Err(detail)
            },
        )
    };
    // A target a metre past the floor's east edge, which no way leads to,
    // approached by a player standing on the floor at (-8, 0) and by one
    // 2 m over it there (off the mesh: on furniture's rim): both make for
    // the same floor before it, one a way leads to from where the second
    // steps back on.
    let approached = {
        let refused = BTreeSet::new();
        let here = Here::new(&level, &snapshot, &refused);
        let target = [21.0, 0.0, 0.0];
        let player = |rise: f32| {
            let [x, y, z] = from;
            sim::motion::MotionState::standing([x, y, z + rise / UNIT], 0.0)
        };
        let (floor, rim) = (player(0.0), player(2.0));
        let goal = |player: &sim::motion::MotionState| here.approach(player, target).goal;
        let back_on = sim::walk::back_on(&level, rim.origin);
        let leads = back_on.is_some_and(|spot| here.leads(spot, goal(&rim)));
        let detail = format!(
            "from the floor to {:?}, from 2 m over it to {:?} (back on at {:?}): a way leads there {leads}",
            goal(&floor).map(|v| v * UNIT),
            goal(&rim).map(|v| v * UNIT),
            back_on.map(|spot| spot.map(|v| v * UNIT)),
        );
        (
            "approach_off_the_mesh_from_where_it_steps_back_on",
            if leads && goal(&rim) == goal(&floor) {
                Ok(detail)
            } else {
                Err(detail)
            },
        )
    };
    // A walker standing a few millimetres into the floor (Chuundar's
    // wooden throne room is uneven by that much), a body 1.5 m ahead on its
    // way: it finds a side to go round by, each side stood on the floor
    // there, not tested at the walker's own height (in the floor, every
    // side had failed and the player stayed pressed into Freyyr).
    let rounds = {
        let [x, y, z] = origin(-4.0, 6.0);
        let low = [x, y, z - 0.003 / UNIT];
        let player = sim::motion::MotionState::standing(low, 0.0);
        let body = origin(-2.5, 6.0);
        let goal = origin(2.0, 6.0);
        let mut walker = sim::walk::Walker::new(goal);
        walker.waypoint = Some(goal);
        walker.stalls = 1;
        walker.around(&level, &player, &[body]);
        let detail = format!(
            "making for {:?} round a body at {:?}",
            walker.waypoint.map(|v| v.map(|c| c * UNIT)),
            body.map(|c| c * UNIT)
        );
        (
            "walker_low_in_the_floor_goes_round_a_body",
            if walker.rounding { Ok(detail) } else { Err(detail) },
        )
    };
    // A placeable at (0, 4) with action points 0.7 m south and north of
    // it, used by the player at (-8, 0): its walk makes for the south one,
    // the one nearest the player (`Here::use_point`), not for the floor
    // before the placeable on the line from the player. The same placeable
    // standing at (0, -4), its facts still placed at (0, 4) (one made in
    // play, its template's facts): its north point, round where it stands.
    let used = {
        let refused = BTreeSet::new();
        let here = Here::new(&level, &snapshot, &refused);
        let bin = ObjectData {
            kind: ObjectKind::Placeable,
            use_points: vec![[0.0, -0.7, 0.0], [0.0, 0.7, 0.0]],
            ..ObjectData::unknown("bin", [0.0, 4.0, 0.0], 0.0)
        };
        let player = sim::motion::MotionState::standing(from, 0.0);
        let goal = |at: [f32; 3]| {
            let point = Here::use_point(from.map(|v| v * UNIT), at, Some(&bin));
            here.approach(&player, point).goal.map(|v| v * UNIT)
        };
        let (placed, made) = (goal([0.0, 4.0, 0.0]), goal([0.0, -4.0, 0.0]));
        let detail = format!("making for {placed:?}; made elsewhere, {made:?}");
        (
            "use_walks_to_the_nearest_action_point",
            if flat(placed, [0.0, 3.3, 0.0]) < 0.05 && flat(made, [0.0, -3.3, 0.0]) < 0.05 {
                Ok(detail)
            } else {
                Err(detail)
            },
        )
    };
    vec![
        rounds,
        used,
        routed("walk_goes_round_a_trigger_that_turned_it_back", 5.0, true),
        routed("walk_goes_through_one_with_no_way_round", 10.0, false),
        left(
            "jumped_out_of_a_talking_trigger_turns_back",
            [0.0, 5.9, 0.0],
            true,
        ),
        left("walked_out_of_one_does_not", [0.0, 0.05, 0.0], false),
        approached,
    ]
}

/// The checks of where a `use` looks for one fallen (`note_fallen`), for
/// `kjk sim probes --only "harness fallen"`: a creature lying dead, then
/// taken with its remains to stand where it was (`Bag::at`), then its body
/// moved 7 m as it fades and its corpse's entry gone (the remains made: a
/// Sith thug on Korriban, shoved by the player, its medallion's remains left
/// 5.7 m from where the harness last saw the body, the loot goal met as one
/// that left nothing). It is looked for where its remains stand, and a use
/// finding none holding anything is done only once it emptied them; one
/// taken without remains to come is done (Tatooine's dune Dark Jedi).
/// Whether a conversation with one of `party` waits in a creature's
/// `actions`, behind what it does first (Bastila's surrender on the Unknown
/// World: her fake cast, then her talk).
pub(super) fn talk_coming(actions: &BTreeMap<u32, Vec<sim::script::actions::Queued>>, party: &[u32]) -> bool {
    actions.values().flatten().any(|queued| {
        matches!(
            queued.action,
            sim::script::actions::Action::Converse { with, .. } if party.contains(&with.0)
        )
    })
}

/// A fight's quiet waits on a conversation with the party queued behind a
/// creature's other actions, not on one with someone else.
pub fn quiet_probes() -> sim::probe::Checks {
    use sim::script::actions::{Action, Queued};
    let queued = |with: u32| {
        BTreeMap::from([(
            7,
            vec![
                Queued {
                    action: Action::Hold { talk: None },
                    started_ms: Some(0),
                },
                Queued {
                    action: Action::Converse {
                        with: ObjectId(with),
                        dialog: String::new(),
                        asker: None,
                    },
                    started_ms: None,
                },
            ],
        )])
    };
    [
        ("fight_quiet_waits_on_a_queued_talk", queued(1), true),
        ("fight_quiet_ignores_talk_with_others", queued(9), false),
        ("fight_quiet_without_talk", BTreeMap::new(), false),
    ]
    .into_iter()
    .map(|(name, actions, want)| {
        let coming = talk_coming(&actions, &[0, 1]);
        let detail = format!("a conversation coming {coming}");
        (name, if coming == want { Ok(detail) } else { Err(detail) })
    })
    .collect()
}

/// A use waits on a foe fighting a companion more than `STRAY_M` from the
/// player (the Leviathan's trooper at Carth 27 m from the hangar lift),
/// reaching as far as the farthest such foe; not on one fighting the
/// player, nor one at a companion beside it.
pub fn straggle_probes() -> sim::probe::Checks {
    let (player, carth, bastila) = (sim::SimId(1), sim::SimId(2), sim::SimId(3));
    let mates = [(carth, [27.0, 0.0, 0.0]), (bastila, [4.0, 0.0, 0.0])];
    let keeper = |id: u32, x: f32, target: sim::SimId| Keeper {
        id: sim::SimId(id),
        at: [x, 0.0, 0.0],
        tag: String::new(),
        target: Some(target),
        plot: false,
        near: (String::new(), 0.0),
    };
    let cases: [(&'static str, Vec<Keeper>, Option<f32>); 4] = [
        ("use_waits_on_a_companions_fight_far_off", vec![keeper(9, 29.0, carth)], Some(30.0)),
        (
            "use_waits_reaching_the_farthest",
            vec![keeper(9, 29.0, carth), keeper(10, 33.0, carth)],
            Some(34.0),
        ),
        ("use_not_held_by_a_fight_at_hand", vec![keeper(9, 5.0, bastila)], None),
        ("use_not_held_by_a_foe_at_the_player", vec![keeper(9, 12.0, player)], None),
    ];
    cases
        .into_iter()
        .map(|(name, keepers, want)| {
            let got = straggling(&keepers, &mates, [0.0; 3]);
            let detail = format!("waits to {got:?} m");
            (name, if got == want { Ok(detail) } else { Err(detail) })
        })
        .collect()
}

pub fn fallen_probes() -> sim::probe::Checks {
    use sim::remains::{Bag, Corpse};
    let mut world = match sim::SimWorld::new(sim::WorldConfig::one_seat(), sim::Level::arena()) {
        Ok(world) => world,
        Err(error) => return vec![("fallen arena", Err(error.to_string()))],
    };
    let base = world.snapshot();
    let Some(me) = base
        .entities
        .iter()
        .find(|e| e.kind == sim::Kind::Avatar(Seat(0)))
        .copied()
    else {
        return vec![("fallen arena", Err("no player on the arena".into()))];
    };
    let id = sim::SimId(u32::MAX - 1);
    // The snapshot with the dead creature lying at `at`.
    let lying = |at: [f32; 3]| {
        let mut snapshot = base.clone();
        let fighter = me.fighter.map(|fighter| sim::combat::Fighter {
            life: sim::combat::Life::Dead,
            ..fighter
        });
        snapshot.entities.push(sim::EntityState {
            id,
            kind: sim::Kind::Object(ObjectKind::Creature, 0),
            position: at,
            fighter,
            ..me
        });
        snapshot
    };
    let corpse = |bag: Option<[f32; 3]>| {
        BTreeMap::from([(
            id.0,
            Corpse {
                due_ms: 0,
                bag: bag.map(|at| Bag { row: 0, at, yaw: 0.0 }),
            },
        )])
    };
    let (fell_at, taken_at, shoved_to) = ([2.0, 0.0, 0.0], [2.0, 0.0, -0.6], [-5.0, 0.0, 0.0]);
    // The steps it is seen in: lying, taken (its remains to come, or none),
    // moved on as it fades and its entry gone.
    let seen = |bag: Option<[f32; 3]>| {
        let mut fallen = BTreeMap::new();
        note_fallen(&mut fallen, &lying(fell_at), &BTreeMap::new());
        note_fallen(&mut fallen, &lying(fell_at), &corpse(bag));
        note_fallen(&mut fallen, &lying(shoved_to), &BTreeMap::new());
        fallen.get(&0).copied()
    };
    // Where it is looked for, and whether a use not finding its remains
    // holding anything is done, having emptied them (`taken`) or not.
    let case = |name: &'static str, bag: Option<[f32; 3]>, taken: bool, at: [f32; 3], done: bool| {
        let result = match seen(bag) {
            None => Err("not seen fallen".to_string()),
            Some(fell) => {
                let used = fell.used(taken, &BTreeMap::new());
                let detail = format!(
                    "looked for at {:?}, emptied by the use {taken}: done {used}",
                    fell.at
                );
                if fell.at == at && used == done {
                    Ok(detail)
                } else {
                    Err(detail)
                }
            }
        };
        (name, result)
    };
    vec![
        case("fallen_looked_for_where_its_remains_stand", Some(taken_at), false, taken_at, false),
        case("fallen_remains_emptied_by_the_use", Some(taken_at), true, taken_at, true),
        case("fallen_without_remains_left_nothing", None, false, shoved_to, true),
    ]
}

/// The probes of the harness's weapon draw (`gear::strongest`, as `bash`
/// and the swap back from a gun draw): of the weapons carried, the
/// strongest of the kind asked for that the wielder is trained for; a
/// vibroblade passed over by one without melee proficiency (Mira), who then
/// draws her blaster, the gun asked for when no blade is trained for; a
/// weapon for the left hand alone never drawn into the right; a tie to the
/// first carried.
pub fn draw_probes() -> sim::probe::Checks {
    use sim::combat::sheet::{Blaster, Delivery, Rows, Weapon, feats};
    use sim::inventory::slots;
    let made = |item: u32, delivery: Delivery, dice: u8, die: u8, need: u16, slot: u8| {
        let weapon = Weapon {
            delivery,
            dice,
            die,
            ..Weapon::UNARMED
        };
        (item, weapon, vec![need], 1 << slot)
    };
    let carried = [
        made(1, Delivery::Melee, 1, 6, feats::PROFICIENCY_MELEE, slots::RIGHT_WEAPON),
        made(2, Delivery::Melee, 2, 6, feats::PROFICIENCY_MELEE, slots::RIGHT_WEAPON),
        made(3, Delivery::Melee, 3, 8, feats::PROFICIENCY_MELEE, slots::LEFT_WEAPON),
        made(4, Delivery::Blaster(Blaster::Pistol), 1, 6, feats::PROFICIENCY_BLASTER, slots::RIGHT_WEAPON),
        made(5, Delivery::Blaster(Blaster::Pistol), 1, 6, feats::PROFICIENCY_BLASTER, slots::RIGHT_WEAPON),
    ];
    let trained = |rows: &[u16]| rows.iter().fold(Rows::<4>::default(), |all, row| all.with(*row));
    let soldier = trained(&[feats::PROFICIENCY_MELEE, feats::PROFICIENCY_BLASTER]);
    let scout = trained(&[feats::PROFICIENCY_BLASTER]);
    let all = trained(&[feats::PROFICIENCY_ALL]);
    let draws = |feats: Option<&Rows<4>>| {
        let blade = super::gear::strongest(&carried, feats, true);
        (blade, blade.or_else(|| super::gear::strongest(&carried, feats, false)))
    };
    let (soldier_blade, _) = draws(Some(&soldier));
    let (scout_blade, scout_draw) = draws(Some(&scout));
    let (all_blade, _) = draws(Some(&all));
    let (sheetless_blade, _) = draws(None);
    vec![
        (
            "draw_strongest_trained_blade",
            sim::probe::check(
                soldier_blade == Some(2),
                format!("one trained in melee draws the 2d6 vibroblade, not the 1d6 or the left hand's 3d8: {soldier_blade:?}"),
            ),
        ),
        (
            "draw_passes_over_untrained_blade",
            sim::probe::check(
                scout_blade.is_none() && scout_draw == Some(4),
                format!("one without melee proficiency draws no blade ({scout_blade:?}) and her first blaster instead ({scout_draw:?})"),
            ),
        ),
        (
            "draw_proficiency_all",
            sim::probe::check(
                all_blade == Some(2) && sheetless_blade == Some(2),
                format!("Proficiency All trains every weapon ({all_blade:?}), and with no sheet nothing is passed over ({sheetless_blade:?})"),
            ),
        ),
    ]
}
