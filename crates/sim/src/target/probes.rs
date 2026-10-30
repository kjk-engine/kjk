//! The probes of `target` (`kjk sim probes`).

use super::*;
use crate::math::{self, Vec3};
use crate::module::{ObjectKind, Opens};
use collision::UNIT;

/// The targeting's checks on the proving ground (`determinism`), for the
/// approved test `use_actions`: the crosshair on the door picks it, a
/// creature before the door wins, soft aim picks what the ray misses, a
/// wall hides from it, what is at the feet is picked looking up, past the
/// range nothing is, a target holds and lets go, each kind's choices
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
            (false, false),
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
        choices(door, &locked_door, shut, false, None, (false, false)),
        choices(
            door,
            &key_only,
            shut,
            false,
            Some(Security { spike: Some(9) }),
            (false, false),
        ),
    ];
    checks.push((
        "target_security_offered",
        check(
            opened_only == [vec![Open], vec![Open]],
            format!("{opened_only:?}"),
        ),
    ));
    // TSL's mine on a door: offered on a shut door (a key-only one too)
    // where the party carries one, not on an open door.
    let mined = [(&key_only, shut), (&locked_door, open)]
        .map(|(facts, state)| choices(door, facts, state, false, None, (false, true)));
    checks.push((
        "target_mine_offered",
        check(
            mined == [vec![Open, Mine], vec![Use]]
                && ask(Mine, 7, door) == Some(crate::action::Action::Mine(7)),
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
    .map(|(facts, state)| choices(door, facts, state, false, None, (false, false)));
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
    checks.push((
        "wall_hides_what_is_close_behind",
        check(
            picked.is_none() && !seen,
            format!("picked {picked:?}, seen {seen}"),
        ),
    ));
    checks
}
