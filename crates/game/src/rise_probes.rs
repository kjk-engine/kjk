//! Bodies up again after lying (`cast::at_rest`): on K1's own body and
//! clips, a script's looping dead animation, then the standing one a script
//! plays for a moment before jumping the body away (Taris's duel ring:
//! `k_ptar_dueler_ud`'s loser), then none. Each frame posed as the cast
//! poses one (`acting::Shows` over the body), with and without the body put
//! back at rest first. And JA's nineteen deaths: each get-up's first frame,
//! turned as the view turns it (`cast::getup_turn`), lies on the side the
//! death left the body on, its head where the death's lies, its pelvis
//! near.

use std::collections::BTreeMap;

use bevy::ecs::system::SystemState;
use bevy::prelude::*;
use sim::combat::idle::Acting;

use crate::cast::{self, Rest};
use crate::probe_kit::{Checks, check, not_installed};
use crate::{acting, viewer};

/// The body: Deadeye Duncan's (a type B body, its head on `headhook`).
const BODY: &str = "pmbcm";
/// `ANIMATION_LOOPING_DEAD`, then `ANIMATION_LOOPING_PAUSE` for 0.1 s from
/// `UP_MS`, as the duel ring's loser plays them.
const DEAD: u16 = 26;
const PAUSE: u16 = 0;
const UP_MS: u64 = 3000;
const END_MS: u64 = 6000;
const FRAME_MS: u64 = 16;
/// The node a lying clip lays down, which the JA pose never turns.
const ROOT_DUMMY: &str = "rootdummy";

pub fn probes() -> Checks {
    vec![
        ("up from a lying clip", lying_clip_ends()),
        ("get-ups start where deaths lie", getups_start_lying()),
    ]
}

/// Furthest (metres) a get-up's turned first frame may set the pelvis from
/// where its death's last frame left it.
const GETUP_SLIDE_M: f32 = 0.6;

fn getups_start_lying() -> Result<String, String> {
    let (gla, cfg) = match crate::stage::ja_skeleton(&crate::discover()) {
        Ok(found) => found,
        Err(why) => return Ok(format!("skipped: {why}")),
    };
    let sequences: BTreeMap<String, fmt_ja::animation_cfg::Sequence> = cfg
        .sequences
        .iter()
        .map(|s| (s.name.to_ascii_uppercase(), s.clone()))
        .collect();
    let mut rows = Vec::new();
    let mut wrong = Vec::new();
    for n in 1..=19 {
        let name = format!("BOTH_DEATH{n}");
        let death = sim::motion::Anim::from_name(&name).ok_or(format!("no {name}"))?;
        let getup = sim::combat::death::getup(death);
        let pose = |anim: sim::motion::Anim, last| {
            let sequence = sequences.get(anim.name())?;
            cast::lying(&gla, sequence, last)
        };
        let (Some(lain), Some(up), Some(turn)) = (
            pose(death, true),
            pose(getup.anim, false),
            cast::getup_turn(&gla, &sequences, death, getup.anim),
        ) else {
            return Err(format!(
                "{name} or {}: not in the skeleton",
                getup.anim.name()
            ));
        };
        let turned = Quat::from_rotation_z(turn.to_radians()) * up.1;
        let slide = (turned - lain.1).truncate().length();
        let heading = (up.0 + turn - lain.0 + 540.0).rem_euclid(360.0) - 180.0;
        let side = lain.2.signum() == up.2.signum();
        rows.push(format!(
            "{n}: {} turn {turn:.0}° (table {:.0}°), pelvis {slide:.2} m",
            getup.anim.name(),
            -getup.turn
        ));
        if !side || heading.abs() > 1.0 || slide > GETUP_SLIDE_M {
            wrong.push(format!(
                "{n} (side {side}, head {heading:.0}°, pelvis {slide:.2} m)"
            ));
        }
    }
    check(
        wrong.is_empty(),
        format!("{}; wrong: {wrong:?}", rows.join("; ")),
    )
}

fn lying_clip_ends() -> Result<String, String> {
    let Some(index) = crate::k1_index() else {
        return Ok(not_installed(vfs::Game::Kotor1));
    };
    let resolver = crate::k1_open(&index, None)?;
    let ir = viewer::model(&resolver, BODY)?.ok_or(format!("{BODY}: no model"))?;
    let clips = viewer::scripted_clips(
        &mut BTreeMap::new(),
        &resolver,
        (vfs::Game::Kotor1, false),
        &ir,
    )?;
    let node = (ir.nodes.iter())
        .position(|n| n.name.eq_ignore_ascii_case(ROOT_DUMMY))
        .ok_or(format!("{BODY}: no {ROOT_DUMMY}"))?;
    let rest = cast::rest_of(&ir);
    let placed = viewer::Placed::at(ir, [0.0; 3]);
    let lying = played(&placed, &clips, &rest, node, true, UP_MS - 500);
    let [stale, afresh] =
        [false, true].map(|at_rest| played(&placed, &clips, &rest, node, at_rest, END_MS));
    check(
        lying > 30.0 && stale > 20.0 && afresh < 1.0,
        format!(
            "{ROOT_DUMMY} tilted {lying:.0}° lying; once up, {stale:.0}° posed over what the clips left, {afresh:.1}° posed from rest"
        ),
    )
}

/// How far (degrees) the body's `node` leans from upright `ms` into the
/// scene, posed each frame by the clips its scripts play over it (from
/// rest first with `at_rest`).
fn played(
    placed: &viewer::Placed,
    clips: &[(u16, std::sync::Arc<asset_odyssey::anim::AnimationIr>)],
    rest: &Rest,
    node: usize,
    at_rest: bool,
    ms: u64,
) -> f32 {
    let mut world = World::new();
    let nodes: Vec<Entity> = rest
        .iter()
        .map(|&(_, t, r)| {
            world
                .spawn(Transform::from_translation(t).with_rotation(r))
                .id()
        })
        .collect();
    let bound = acting::bind(placed, clips, &nodes);
    let mut shows = acting::Shows::default();
    let mut state: SystemState<Query<&mut Transform>> = SystemState::new(&mut world);
    for now in (0..=ms).step_by(FRAME_MS as usize) {
        let acting = if now < UP_MS {
            Acting {
                animation: DEAD,
                since: 0,
                ms: 360_000,
            }
        } else {
            Acting {
                animation: PAUSE,
                since: UP_MS,
                ms: 100,
            }
        };
        let now = now as f64;
        shows.follow(Some(acting), (true, true, false), now);
        let Ok(mut transforms) = state.get_mut(&mut world) else {
            return f32::NAN;
        };
        if at_rest {
            cast::at_rest((&nodes, rest), &mut transforms);
        }
        for (clip, at, weight) in shows.shown(&bound, now, (0, None)) {
            clip.pose_over(at, weight, &mut transforms);
        }
    }
    // The node's up in the model's space, its ancestors turned by `turn`.
    let up = |turn: &dyn Fn(usize) -> Quat| {
        let (mut model, mut at) = (Quat::IDENTITY, Some(node));
        while let Some(index) = at {
            model = turn(index) * model;
            at = rest[index].0;
        }
        model * Vec3::Z
    };
    let posed = up(&|index| {
        world
            .get::<Transform>(nodes[index])
            .map_or(rest[index].2, |t| t.rotation)
    });
    posed.angle_between(up(&|index| rest[index].2)).to_degrees()
}
