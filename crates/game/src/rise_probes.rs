//! Bodies up again after lying (`cast::at_rest`): on K1's own body and
//! clips, a script's looping dead animation, then the standing one a script
//! plays for a moment before jumping the body away (Taris's duel ring:
//! `k_ptar_dueler_ud`'s loser), then none. Each frame posed as the cast
//! poses one (`acting::Shows` over the body), with and without the body put
//! back at rest first. TSL's Handmaiden throwing her hood back (`offhood`)
//! and standing on: the hood stays back (`cast::held_nodes`). And JA's
//! nineteen deaths: each get-up's first frame,
//! turned as the view turns it (`cast::getup_turn`), lies on the side the
//! death left the body on, its head where the death's lies, its pelvis
//! near.

use std::collections::BTreeMap;

use bevy::ecs::system::SystemState;
use bevy::prelude::*;
use sim::combat::idle::Acting;

use crate::cast;
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
        ("a hood thrown back stays back", hood_stays_off()),
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

/// A body model, its scripted clips, the index of its node `node` and its
/// rest pose, from `resolver`.
fn body(
    resolver: &vfs::resolver::Resolver,
    game: vfs::Game,
    (model, node): (&str, &str),
) -> Result<Body, String> {
    let ir = viewer::model(resolver, model)?.ok_or(format!("{model}: no model"))?;
    let clips = viewer::scripted_clips(&mut BTreeMap::new(), resolver, (game, false), &ir)?;
    let node = (ir.nodes.iter())
        .position(|n| n.name.eq_ignore_ascii_case(node))
        .ok_or(format!("{model}: no {node}"))?;
    let pause = clips
        .iter()
        .find(|(animation, _)| *animation == acting::LOOPING_PAUSE)
        .map(|(_, clip)| clip.clone());
    let held = cast::held_nodes(&ir, pause.as_deref());
    Ok(Body {
        rest: cast::rest_of(&ir),
        held,
        placed: viewer::Placed::at(ir, [0.0; 3]),
        clips,
        node,
    })
}

struct Body {
    placed: viewer::Placed,
    clips: Vec<(u16, std::sync::Arc<asset_odyssey::anim::AnimationIr>)>,
    rest: Vec<(Option<usize>, Vec3, Quat)>,
    /// `cast::held_nodes` by its standing clip.
    held: Vec<bool>,
    node: usize,
}

/// How each frame starts: posed over what the last left, from rest (all
/// nodes, as before `cast::held_nodes`), or from rest but the held nodes.
#[derive(Clone, Copy)]
enum Start {
    Stale,
    Rest,
    Held,
}

fn lying_clip_ends() -> Result<String, String> {
    let Some(index) = crate::k1_index() else {
        return Ok(not_installed(vfs::Game::Kotor1));
    };
    let resolver = crate::k1_open(&index, None)?;
    let body = body(&resolver, vfs::Game::Kotor1, (BODY, ROOT_DUMMY))?;
    let scene = |from, ms| {
        let tilt = |world: &World, nodes: &[Entity]| leans(&body, world, nodes);
        played(&body, (DEAD, 360_000, 100), from, ms, tilt)
    };
    let lying = scene(Start::Held, UP_MS - 500);
    let [stale, afresh, held] =
        [Start::Stale, Start::Rest, Start::Held].map(|from| scene(from, END_MS));
    check(
        lying > 30.0 && stale > 20.0 && afresh < 1.0 && held < 1.0,
        format!(
            "{ROOT_DUMMY} tilted {lying:.0}° lying; once up, {stale:.0}° posed over what the clips left, {afresh:.1}° posed from rest, {held:.1}° from rest but the nodes its standing clip does not key"
        ),
    )
}

/// The Handmaiden's robe (TSL's `P_HandmaidenBB`, Atris's academy), its
/// hood's middle and the dialog clip `Hood_Off` throws it back by
/// (`creature::DIALOG_CLIPS`), its length.
const ROBE: &str = "p_handmaidenbb";
const HOOD: &str = "mhood_g";
const HOOD_OFF: u16 = 10_565;
const HOOD_OFF_MS: u32 = 1_533;

/// A hood thrown back stays back (`cast::held_nodes`): `offhood` keys the
/// robe's hood, its standing clip (the supermodel's `pause1`) does not, so
/// KOTOR leaves the hood as `offhood` ended while she stands on; the body
/// posed from rest each frame put it up again the moment the clip ended.
fn hood_stays_off() -> Result<String, String> {
    let installs = crate::discover();
    let Some(install) = installs.get(vfs::Game::Kotor2) else {
        return Ok(not_installed(vfs::Game::Kotor2));
    };
    let index = vfs::DirIndex::build(&install.data_root).map_err(|e| format!("{e:?}"))?;
    let resolver = vfs::resolver::Resolver::open(
        vfs::Game::Kotor2,
        &index,
        None,
        vfs::resolver::TextureQuality::High,
    )
    .map_err(|e| format!("{e:?}"))?;
    let body = body(&resolver, vfs::Game::Kotor2, (ROBE, HOOD))?;
    let thrown = |from| {
        let turn = |world: &World, nodes: &[Entity]| {
            world.get::<Transform>(nodes[body.node]).map_or(0.0, |t| {
                t.rotation
                    .angle_between(body.rest[body.node].2)
                    .to_degrees()
            })
        };
        played(&body, (HOOD_OFF, HOOD_OFF_MS, 360_000), from, END_MS, turn)
    };
    let [old, held] = [Start::Rest, Start::Held].map(thrown);
    check(
        body.held[body.node] && old < 1.0 && held > 20.0,
        format!(
            "{HOOD} turned {held:.0}° from its rest standing on after offhood, {old:.1}° when every node is posed from rest"
        ),
    )
}

/// The measure `measure` takes of the body `ms` into a scene where its
/// scripts play `first` (an animation for its ms) from the start, then the
/// standing clip from `UP_MS` for its ms, each frame posed by the clips
/// over it as `from` starts it.
fn played(
    body: &Body,
    (first, first_ms, standing_ms): (u16, u32, u32),
    from: Start,
    ms: u64,
    measure: impl Fn(&World, &[Entity]) -> f32,
) -> f32 {
    let rest = &body.rest[..];
    let mut world = World::new();
    let nodes: Vec<Entity> = rest
        .iter()
        .map(|&(_, t, r)| {
            world
                .spawn(Transform::from_translation(t).with_rotation(r))
                .id()
        })
        .collect();
    let bound = acting::bind(&body.placed, &body.clips, &nodes);
    let mut shows = acting::Shows::default();
    let mut state: SystemState<Query<&mut Transform>> = SystemState::new(&mut world);
    for now in (0..=ms).step_by(FRAME_MS as usize) {
        let acting = if now < UP_MS {
            Acting {
                animation: first,
                since: 0,
                ms: first_ms,
            }
        } else {
            Acting {
                animation: PAUSE,
                since: UP_MS,
                ms: standing_ms,
            }
        };
        let now = now as f64;
        shows.follow(Some(acting), (true, false, true, false), now);
        let Ok(mut transforms) = state.get_mut(&mut world) else {
            return f32::NAN;
        };
        match from {
            Start::Stale => {}
            Start::Rest => cast::at_rest((&nodes, rest, &[]), &mut transforms),
            Start::Held => cast::at_rest((&nodes, rest, &body.held), &mut transforms),
        }
        for (clip, at, weight) in shows.shown(&bound, now, (0, None)) {
            clip.pose_over(at, weight, &mut transforms);
        }
    }
    measure(&world, &nodes)
}

/// How far (degrees) the body's node leans from upright.
fn leans(body: &Body, world: &World, nodes: &[Entity]) -> f32 {
    let rest = &body.rest;
    // The node's up in the model's space, its ancestors turned by `turn`.
    let up = |turn: &dyn Fn(usize) -> Quat| {
        let (mut model, mut at) = (Quat::IDENTITY, Some(body.node));
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
