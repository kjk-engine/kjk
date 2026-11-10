//! The probes of `target` (`kjk sim probes`).

use super::*;
use crate::math::{self, Vec3};
use crate::module::{ObjectKind, Opens};
use collision::UNIT;

/// The targeting's checks on the proving ground (`determinism`), for the
/// approved test `use_actions`: the crosshair on the door picks it, a
/// creature before the door wins, soft aim picks what the ray misses, a
/// wall hides from it, what is at the feet is picked looking up, past the
/// range nothing is, a target holds and lets go, each kind's choices, a
/// placeable hung up a wall picked from the floor beneath it (`hung`), a
/// console on a bench or in its own solid body picked (`seen`)
/// (`candidates` is checked
/// with a world, `world::uses::probes`).
pub fn probes() -> Vec<(&'static str, Result<String, String>)> {
    use crate::determinism::{ground_point, proving_ground};
    let check = |ok: bool, what: String| if ok { Ok(what) } else { Err(what) };
    let level = proving_ground();
    let eye = |x: f32, y: f32| ground_point(x, y, 1.6);
    let feet = |x: f32, y: f32| ground_point(x, y, 0.0);
    // The ground's axes in the world.
    let (o, ex, ey) = (feet(0.0, 0.0), feet(1.0, 0.0), feet(0.0, 1.0));
    let axis = |to: Vec3| math::sub(to, o);
    let along = |x: f32, y: f32, z: f32| {
        let v = math::add(math::scale(axis(ex), x), math::scale(axis(ey), y));
        let v = [v[0], v[1], z];
        math::scale(v, 1.0 / math::length(v))
    };
    let looker = |x: f32, y: f32, fx: f32, fy: f32| {
        let f = along(fx, fy, 0.0);
        Looker {
            feet: feet(x, y),
            facing: [f[0], f[1]],
        }
    };
    let thing = |index, kind, x: f32, y: f32, radius, height| Candidate {
        index,
        kind,
        feet: feet(x, y),
        radius,
        height,
        ahead: false,
        points: [None; 2],
    };
    let door = thing(0, ObjectKind::Door, 0.0, 0.0, DOOR_RADIUS, DOOR_HEIGHT);
    let pick_at = |from: Vec3, forward: Vec3, who: Looker, candidates: &[Candidate]| {
        pick(&level, None, &Look { from, forward }, &who, candidates)
    };
    let creature = thing(
        5,
        ObjectKind::Creature,
        -3.0,
        0.0,
        CREATURE_RADIUS,
        CREATURE_HEIGHT,
    );
    let sign = thing(6, ObjectKind::Placeable, -5.75, 1.0, 0.2, 3.2);
    let hidden = thing(7, ObjectKind::Placeable, 2.0, 3.15, 0.2, 3.2);
    let chest = thing(8, ObjectKind::Placeable, -5.0, 0.0, 0.4, 0.6);
    let far = thing(9, ObjectKind::Placeable, -6.0, 5.5, 0.4, 1.5);
    let forward_x = along(1.0, 0.0, 0.0);
    let forward_y = along(0.0, 1.0, 0.0);
    let mut hold = Hold::default();
    let held = [
        hold.update(Some(3), 0),
        hold.update(None, 100),
        hold.update(None, 300),
    ];
    let mut checks = vec![
        (
            "target_door",
            pick_at(
                eye(-6.0, 0.0),
                forward_x,
                looker(-5.0, 0.0, 1.0, 0.0),
                &[door],
            ),
        ),
        (
            "target_nearer_wins",
            pick_at(
                eye(-6.0, 0.0),
                forward_x,
                looker(-5.0, 0.0, 1.0, 0.0),
                &[door, creature],
            ),
        ),
        (
            "target_soft_aim",
            pick_at(
                eye(-6.0, -3.0),
                forward_y,
                looker(-6.0, -3.0, 0.0, 1.0),
                &[sign],
            ),
        ),
        (
            "target_wall_hides",
            pick_at(
                eye(-1.0, 3.0),
                forward_x,
                looker(-1.0, 3.0, 1.0, 0.0),
                &[hidden],
            ),
        ),
        (
            "target_at_feet",
            pick_at(
                eye(-6.5, 0.0),
                along(0.0, 0.0, 1.0),
                looker(-6.0, 0.0, 1.0, 0.0),
                &[chest],
            ),
        ),
        (
            "target_out_of_range",
            pick_at(
                eye(-6.0, -5.8),
                forward_y,
                looker(-6.0, -5.0, 0.0, 1.0),
                &[far],
            ),
        ),
    ]
    .into_iter()
    .zip([Some(0), Some(5), Some(6), None, Some(8), None])
    .map(|((name, got), want)| {
        (
            name,
            check(got == want, format!("{got:?}, wanted {want:?}")),
        )
    })
    .collect::<Vec<_>>();
    // A companion is picked in front of the player; at its heels, between
    // it and the camera, or inside its body, the look passes it.
    let companion = Candidate {
        ahead: true,
        ..creature
    };
    let companion_picks = [(-5.0, Some(5)), (-2.0, None), (-3.1, None)].map(|(x, want)| {
        let got = pick_at(
            eye(-6.0, 0.0),
            forward_x,
            looker(x, 0.0, 1.0, 0.0),
            &[companion],
        );
        (got == want, got)
    });
    checks.push((
        "target_companion_ahead",
        check(
            companion_picks.iter().all(|(ok, _)| *ok),
            format!("{companion_picks:?}"),
        ),
    ));
    checks.push((
        "target_holds",
        check(held == [Some(3), Some(3), None], format!("{held:?}")),
    ));
    let made = |kind, opens, container, conversation: &str| crate::module::ObjectData {
        kind,
        opens,
        container,
        useable: true,
        conversation: conversation.into(),
        ..crate::module::ObjectData::unknown("probe", [0.0; 3], 0.0)
    };
    let shut = crate::snapshot::ObjectState {
        lock: crate::snapshot::lock::PLACED,
        ..crate::snapshot::ObjectState::default()
    };
    let open = crate::snapshot::ObjectState { state: 1, ..shut };
    let of = |kind, opens, container, talk: &str, state, hostile| {
        choices(
            kind,
            &made(kind, opens, container, talk),
            state,
            hostile,
            Some(Security { spike: Some(9) }),
            None,
        )
    };
    use Choice::{Close, Mine, Open, Talk, Unlock, Use};
    let (door, place, creature) = (
        ObjectKind::Door,
        ObjectKind::Placeable,
        ObjectKind::Creature,
    );
    let got = [
        of(door, Opens::Yes, false, "", shut, false),
        of(door, Opens::Locked, false, "", shut, false),
        of(door, Opens::Yes, false, "", open, false),
        of(place, Opens::Yes, true, "", shut, false),
        of(place, Opens::Locked, true, "", shut, false),
        of(place, Opens::Never, false, "", shut, false),
        of(creature, Opens::Never, false, "talk", shut, false),
        of(creature, Opens::Never, false, "talk", shut, true),
        of(creature, Opens::Never, false, "", shut, false),
    ];
    let locked = vec![Open, Unlock { spike: None }, Unlock { spike: Some(9) }];
    let want = [
        vec![Open],
        locked.clone(),
        vec![Close, Use],
        vec![Open],
        locked,
        vec![Use],
        vec![Talk],
        Vec::new(),
        Vec::new(),
    ];
    checks.push(("target_choices", check(got == want, format!("{got:?}"))));
    // Security only for a character with ranks, and not on a lock only its
    // key opens: Open alone.
    let locked_door = made(door, Opens::Locked, false, "");
    let key_only = crate::module::ObjectData {
        lock: Some(crate::module::Lock {
            key_required: true,
            ..crate::module::Lock::default()
        }),
        ..locked_door.clone()
    };
    let opened_only = [
        choices(door, &locked_door, shut, false, None, None),
        choices(
            door,
            &key_only,
            shut,
            false,
            Some(Security { spike: Some(9) }),
            None,
        ),
    ];
    checks.push((
        "target_security_offered",
        check(
            opened_only == [vec![Open], vec![Open]],
            format!("{opened_only:?}"),
        ),
    ));
    // TSL's mine on a lock: offered on a locked door (a key-only one too)
    // and a locked placeable, container or not, where the party carries
    // one; not on an unlocked door or container, an open door, nor a lock
    // its data marks `NotBlastable`.
    let not_blastable = crate::module::ObjectData {
        toughness: crate::module::Toughness {
            not_blastable: true,
            ..crate::module::Toughness::default()
        },
        ..locked_door.clone()
    };
    let (locked_box, locked_thing, unlocked_box, unlocked_door) = (
        made(place, Opens::Locked, true, ""),
        made(place, Opens::Locked, false, ""),
        made(place, Opens::Yes, true, ""),
        made(door, Opens::Yes, false, ""),
    );
    let mined = [
        (door, &key_only, shut),
        (door, &locked_door, open),
        (door, &unlocked_door, shut),
        (door, &not_blastable, shut),
        (place, &locked_box, shut),
        (place, &locked_thing, shut),
        (place, &unlocked_box, shut),
    ]
    .map(|(kind, facts, state)| {
        let demolitions = crate::target::Demolitions {
            recover: false,
            lock_mine: true,
        };
        choices(kind, facts, state, false, None, Some(demolitions))
    });
    checks.push((
        "target_mine_offered",
        check(
            mined
                == [
                    vec![Open, Mine],
                    vec![Use],
                    vec![Open],
                    vec![Open],
                    vec![Open, Mine],
                    vec![Use, Mine],
                    vec![Open],
                ]
                && ask(Mine, 7, door) == Some(crate::action::Action::Mine(7))
                && ask(Mine, 7, place) == Some(crate::action::Action::Mine(7)),
            format!("{mined:?}"),
        ),
    ));
    // Close (QUIRKS.md Q-0330) only on an open door the player could open
    // again: not one only scripts open, not one leading out, not one
    // broken open.
    let scripted = made(door, Opens::Never, false, "");
    let leads_out = crate::module::ObjectData {
        link: Some(crate::module::Link {
            module: "elsewhere".into(),
            tag: String::new(),
            destination: None,
        }),
        ..made(door, Opens::Yes, false, "")
    };
    let sturdy = crate::module::ObjectData {
        hit_points: Some((10, 10)),
        ..made(door, Opens::Yes, false, "")
    };
    let broken = crate::snapshot::ObjectState { damage: 10, ..open };
    let closes = [
        (&scripted, open),
        (&leads_out, open),
        (&sturdy, broken),
        (&sturdy, open),
    ]
    .map(|(facts, state)| choices(door, facts, state, false, None, None));
    checks.push((
        "target_close_offered",
        check(
            closes == [vec![Use], vec![Use], vec![Use], vec![Close, Use]]
                && ask(Close, 7, door)
                    == Some(crate::action::Action::Use {
                        object: 7,
                        verb: crate::action::Verb::Close,
                    }),
            format!("{closes:?}"),
        ),
    ));
    // A walk keeps the player's hand and power; its own key cancels it.
    let base = crate::Command {
        weapon: 1,
        force: 5,
        ..crate::Command::default()
    };
    let walking = crate::Command {
        forward: 127,
        ..crate::Command::default()
    };
    let (taken, cancelled) = steer(Some(walking), base, WALK_BREAKS);
    checks.push((
        "steer_keeps_hands",
        check(
            (taken.forward, taken.weapon, taken.force, cancelled) == (127, 1, 5, false),
            format!("{taken:?} cancelled {cancelled}"),
        ),
    ));
    // A walk whose choice the target no longer offers ends.
    let player =
        crate::motion::MotionState::standing(math::scale(feet(-6.0, 0.0), 1.0 / UNIT), 0.0);
    let walk = Walk::new(
        &level,
        &|_| crate::ObjectState::default(),
        &player,
        5,
        Spot::plain(feet(-2.0, 0.0), Choice::Talk, ObjectKind::Creature),
        Choice::Talk,
        0,
    );
    checks.push((
        "walk_drops_a_stale_choice",
        check(
            walk.wanted(&[Choice::Talk]) && !walk.wanted(&[]),
            "a walk to talk, then nothing to talk to".into(),
        ),
    ));
    // A walk to talk stops before a way out across its path (#255: C8-42
    // knocked back behind the Crystal Cave's exit): it talks from there
    // when the creature is in reach, is out of reach when not, and walks on
    // where no way out lies ahead.
    checks.push((
        "walk_keeps_out_of_exits",
        walk_keeps_out(&level, &feet, &axis),
    ));
    // What stands just behind a wall: hidden from the crosshair and from
    // sight.
    let behind = thing(10, ObjectKind::Placeable, 0.15, 3.0, PLACEABLE_RADIUS, 3.2);
    let picked = pick_at(
        eye(-1.0, 3.0),
        forward_x,
        looker(-1.0, 3.0, 1.0, 0.0),
        &[behind],
    );
    let seen = in_sight(
        &level,
        None,
        eye(-1.0, 3.0),
        ground_point(0.15, 3.0, 1.0),
        10,
    );
    // A panel hung 1.2 m up a wall (Yavin Station's door consoles) over a
    // floor the navigation mesh covers, from the ground's middle to its
    // far side, reaches down to that floor; one on the floor keeps its
    // volume, one over no floor (the ground's near half) hangs about its
    // middle; a level without a mesh keeps them as placed.
    let corner = |x: f32, y: f32| ground_point(x, y, 0.0);
    let nav = collision::nav::NavMesh::build(
        &[
            [corner(0.5, -5.5), corner(7.5, -5.5), corner(7.5, 5.5)],
            [corner(0.5, -5.5), corner(7.5, 5.5), corner(0.5, 5.5)],
        ],
        crate::walk::CLEARANCE * UNIT,
    );
    let meshed = proving_ground().with_nav(nav);
    let up = |x: f32, y: f32, z: f32| ground_point(x, y, z)[2];
    let tall = PLACEABLE_HEIGHT;
    let cases = [
        (
            &meshed,
            ground_point(4.0, 0.0, 1.2),
            (up(4.0, 0.0, 0.0), tall + 1.2),
        ),
        (
            &meshed,
            ground_point(4.0, 0.0, 0.0),
            (up(4.0, 0.0, 0.0), tall),
        ),
        (
            &meshed,
            ground_point(-4.0, 0.0, 1.2),
            (up(-4.0, 0.0, 1.2) - tall * 0.5, tall),
        ),
        (
            &level,
            ground_point(4.0, 0.0, 1.2),
            (up(4.0, 0.0, 1.2), tall),
        ),
    ];
    let hangs: Vec<(f32, f32)> = cases
        .iter()
        .map(|(level, at, _)| hung(level, *at, tall))
        .collect();
    let near = |a: (f32, f32), b: (f32, f32)| (a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3;
    checks.push((
        "target_hung_panel_reaches_the_floor",
        check(
            cases
                .iter()
                .zip(&hangs)
                .all(|((.., wanted), got)| near(*wanted, *got)),
            format!("{hangs:?}"),
        ),
    ));
    // A console standing on a bench (the 17° ramp's high end), its volume
    // hanging over no floor, so that its middle lies 1 cm inside the
    // bench (Peragus' administration console); and one in the 56° wedge,
    // its middle deep inside and its top 1 cm under the surface, seen from
    // above (the central core's round console, whose own body is solid).
    // Each is picked aimed at: the first by the ray, the second (the look
    // steeper than its volume is wide) by soft aim.
    let on = |x: f32, y: f32, top: f32, share: f32| Candidate {
        feet: ground_point(x, y, top - 0.01 - tall * share),
        ..thing(11, ObjectKind::Placeable, x, y, PLACEABLE_RADIUS, tall)
    };
    let benches = [
        (
            on(-3.3, -4.5, 1.2 * 3.7 / 4.0, 0.5),
            ground_point(-1.0, -4.5, 2.2),
        ),
        (
            on(-6.3, 4.0, 2.2 * 1.2 / 1.5, 1.0),
            ground_point(-5.5, 4.0, 5.0),
        ),
    ];
    let picks: Vec<Option<u32>> = benches
        .iter()
        .map(|(console, from)| {
            let mut forward = math::sub(console.middle(), *from);
            math::normalize(&mut forward);
            let who = Looker {
                feet: [from[0], from[1], console.feet[2]],
                facing: [forward[0], forward[1]],
            };
            pick_at(*from, forward, who, &[*console])
        })
        .collect();
    checks.push((
        "target_console_on_a_bench",
        check(
            picks.iter().all(|picked| *picked == Some(11)),
            format!("{picks:?}"),
        ),
    ));
    checks.push((
        "wall_hides_what_is_close_behind",
        check(
            picked.is_none() && !seen,
            format!("picked {picked:?}, seen {seen}"),
        ),
    ));
    checks
}

/// The crosshair on a placeable used from its action point, for the
/// approved test `crosshair_action_points`. A container stands 1.5 m behind
/// the proving ground's wall, and the player stands 0.6 m before the wall
/// and looks at it. The crosshair picks the container when its action point
/// stands on the player's side of the wall, by the ray and by soft aim, as
/// a use from there goes through (`uses::may_use`). This is Korriban's force
/// cage round Lonna Vash's body. The crosshair picks nothing when the
/// container has no action point, or when its action point stands behind
/// the wall too.
pub fn action_point_probes() -> Vec<(&'static str, Result<String, String>)> {
    use crate::determinism::{ground_point, proving_ground};
    let level = proving_ground();
    let chest = |point: Option<[f32; 2]>| Candidate {
        index: 12,
        kind: ObjectKind::Placeable,
        feet: ground_point(1.5, 3.0, 0.0),
        radius: PLACEABLE_RADIUS,
        height: PLACEABLE_HEIGHT,
        ahead: false,
        points: [point.map(|[x, y]| ground_point(x, y, 0.0)), None],
    };
    let feet = ground_point(-0.6, 3.0, 0.0);
    let from = ground_point(-1.5, 3.0, 1.6);
    let mut ahead = math::sub(feet, from);
    ahead[2] = 0.0;
    math::normalize(&mut ahead);
    let looker = Looker {
        feet,
        facing: [ahead[0], ahead[1]],
    };
    // Straight at the container's middle, and 2 degrees to its side (inside
    // the soft aim's cone, the ray passing it by).
    let at_middle = |candidate: &Candidate| {
        let mut forward = math::sub(candidate.middle(), from);
        math::normalize(&mut forward);
        forward
    };
    let aside = |forward: Vec3| {
        let (sin, cos) = math::sin_cos_deg(2.0);
        [
            forward[0] * cos - forward[1] * sin,
            forward[0] * sin + forward[1] * cos,
            forward[2],
        ]
    };
    let picks = |candidate: Candidate| {
        let straight = at_middle(&candidate);
        [straight, aside(straight)]
            .map(|forward| pick(&level, None, &Look { from, forward }, &looker, &[candidate]))
    };
    [
        (
            "crosshair_through_a_cage_by_its_action_point",
            Some([-0.3, 3.0]),
            Some(12),
        ),
        ("crosshair_no_action_point_hidden", None, None),
        (
            "crosshair_action_point_behind_the_wall_hidden",
            Some([0.8, 3.0]),
            None,
        ),
    ]
    .into_iter()
    .map(|(name, point, want)| {
        let got = picks(chest(point));
        let what = format!("{got:?}, wanted {want:?} both ways");
        (
            name,
            if got == [want; 2] {
                Ok(what)
            } else {
                Err(what)
            },
        )
    })
    .collect()
}

/// A way out of the proving ground: the strip of the ground from `from` to
/// `to` along its x axis (`ground_x`), across its whole width.
struct Strip<'a> {
    ground_x: &'a dyn Fn(Vec3) -> f32,
    from: f32,
    to: f32,
}

impl crate::walk::Ways for Strip<'_> {
    fn door(&self, _index: u32) -> crate::ObjectState {
        crate::ObjectState::default()
    }

    fn leads_out(&self, body: Vec3) -> bool {
        (self.from..=self.to).contains(&(self.ground_x)(body))
    }
}

/// `walk_keeps_out_of_exits`: a player at ground x `at` (running toward
/// +x at `speed` units a second), sent to talk with a creature at x -2
/// (reach `CONVERSATION_RANGE`, 4 m), a way out across the ground at
/// `strip` (none: nowhere), its first stride.
fn walk_keeps_out(
    level: &Level,
    feet: &dyn Fn(f32, f32) -> Vec3,
    axis: &dyn Fn(Vec3) -> Vec3,
) -> Result<String, String> {
    let ex = axis(feet(1.0, 0.0));
    let ground_x = |at: Vec3| {
        let from = axis(at);
        (from[0] * ex[0] + from[1] * ex[1]) / (ex[0] * ex[0] + ex[1] * ex[1])
    };
    let stride = |at: f32, speed: f32, strip: Option<(f32, f32)>| {
        let ways = Strip {
            ground_x: &ground_x,
            from: strip.map_or(f32::MAX, |(from, _)| from),
            to: strip.map_or(f32::MIN, |(_, to)| to),
        };
        let mut player =
            crate::motion::MotionState::standing(math::scale(feet(at, 0.0), 1.0 / UNIT), 0.0);
        let length = math::length([ex[0], ex[1], 0.0]);
        player.velocity = [ex[0] / length * speed, ex[1] / length * speed, 0.0];
        let spot = Spot::plain(feet(-2.0, 0.0), Choice::Talk, ObjectKind::Creature);
        let mut walk = Walk::new(level, &ways, &player, 5, spot, Choice::Talk, 0);
        walk.stride(level, &ways, &player, spot, 0)
    };
    let run = crate::motion::BASE_SPEED;
    let cases = [
        // In reach (3.8 m) with a way out 0.3 m ahead: talked to from here.
        (-5.8, 0.0, Some((-5.5, -5.0)), "arrived"),
        // Out of reach (5 m), the way out 0.4 m ahead: no walk through it.
        (-7.0, 0.0, Some((-6.6, -6.4)), "unreachable"),
        // The same walk with no way out ahead walks on.
        (-7.0, 0.0, None, "steer"),
        // A way out it stands in already (where it arrived) is not one it
        // keeps out of: it walks on.
        (-7.0, 0.0, Some((-7.5, -6.0)), "steer"),
        // At a run, the way out 1.2 m ahead, within its slide: it lets the
        // run slide to a stop before deciding, not out of reach yet.
        (-7.0, run, Some((-5.8, -5.5)), "brake"),
    ];
    let got: Vec<&str> = cases
        .iter()
        .map(|&(at, speed, strip, _)| match stride(at, speed, strip) {
            Stride::Arrived => "arrived",
            Stride::Unreachable => "unreachable",
            Stride::Steer(command) if command.forward == 0 && command.right == 0 => "brake",
            Stride::Steer(_) => "steer",
        })
        .collect();
    let want: Vec<&str> = cases.iter().map(|case| case.3).collect();
    let what = format!("{got:?}");
    if got == want {
        Ok(what)
    } else {
        Err(format!("{what}, wanted {want:?}"))
    }
}
