//! What a held body plays, as the games' executables have it
//! (`acting::of_state`), how a scripted clip takes over
//! (`acting::blended_in`), and that clips handed one to the next, as
//! conversation lines hand them (TSL's `101awake`: the Exile afloat line
//! after line, then sinking), never leave the body under them showing.

use std::collections::BTreeMap;

use bevy::prelude::*;
use sim::combat::effects::state;
use sim::combat::idle::Acting;

use crate::probe_kit::{Checks, check, not_installed};
use crate::{acting, viewer};

pub fn probes() -> Checks {
    vec![
        ("each state's clip", states_play_their_clips()),
        ("a clip's blend", clips_blend_as_the_engine_does()),
        ("clips handed on cover the body", clips_handed_on_cover()),
    ]
}

/// K1's body (Deadeye Duncan's) and two of its loops: `ANIMATION_LOOPING_DEAD`
/// and `ANIMATION_LOOPING_PAUSE`.
const BODY: &str = "pmbcm";
const DEAD: u16 = 26;
const PAUSE: u16 = 0;
const FRAME_MS: u64 = 16;
/// How much of the body under the clips may show once the first has
/// blended in.
const UNDER_MOST: f32 = 0.01;

/// Line by line as a conversation plays them: a loop for 2 s, the same loop
/// again for 2 s (carried on), another loop for 2 s (blended over the
/// first), then a clip played once for longer than it lasts (held on its
/// end). From the first clip's blend on, the share of the body left under
/// the clips (the product of `1 − weight`) stays at most `UNDER_MOST`.
fn clips_handed_on_cover() -> Result<String, String> {
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
    let once = (clips.iter())
        .map(|&(animation, _)| animation)
        .find(|&animation| {
            (sim::combat::idle::FIRE_AND_FORGET..asset_odyssey::creature::LOOPED)
                .contains(&animation)
        })
        .ok_or(format!("{BODY}: no clip played once"))?;
    let rest = crate::cast::rest_of(&ir);
    let mut world = World::new();
    let nodes: Vec<Entity> = rest
        .iter()
        .map(|_| world.spawn(Transform::IDENTITY).id())
        .collect();
    let bound = acting::bind(&viewer::Placed::at(ir, [0.0; 3]), &clips, &nodes);
    let lines = [
        (DEAD, 0, 2000),
        (DEAD, 2000, 2000),
        (PAUSE, 4000, 2000),
        (once, 6000, 8000),
    ];
    let mut shows = acting::Shows::default();
    let mut worst = (0.0_f32, 0);
    for now in (0..14_000).step_by(FRAME_MS as usize) {
        let acting = lines.iter().rev().find(|&&(_, since, _)| now >= since).map(
            |&(animation, since, ms)| Acting {
                animation,
                since,
                ms,
            },
        );
        shows.follow(acting, (true, false, true, true), now as f64);
        let under: f32 = (shows.shown(&bound, now as f64, (0, None)))
            .map(|(_, _, weight)| 1.0 - weight)
            .product();
        if now >= 500 && under > worst.0 {
            worst = (under, now);
        }
    }
    let (under, at) = worst;
    check(
        under <= UNDER_MOST,
        format!(
            "at most {under:.3} of the body under the clips (at {at} ms), clip {once} played once"
        ),
    )
}

fn states_play_their_clips() -> Result<String, String> {
    let held = [
        (state::CONFUSED, None),
        (state::FRIGHTENED, None),
        (state::DROID_STUN, Some(acting::DISABLED)),
        (state::STUNNED, Some(acting::DAZED)),
        (state::PARALYZED, Some(acting::PARALYZED)),
        (state::SLEEP, Some(acting::ASLEEP)),
        (state::CHOKE, Some(acting::CHOKED)),
        (state::WHIRLWIND, Some(acting::WHIRLWIND)),
        (state::CRUSH, Some(acting::CRUSHED)),
        (state::MIND_TRICK, Some(acting::DAZED)),
        (state::DROID_SCRAMBLE, Some(acting::DAZED)),
        (state::DROID_CONFUSED, None),
    ];
    for (state, clip) in held {
        if acting::of_state(state) != clip {
            return Err(format!("state {state} plays {:?}", acting::of_state(state)));
        }
    }
    Ok(format!("{} states", held.len()))
}

fn clips_blend_as_the_engine_does() -> Result<String, String> {
    let at = acting::blended_in;
    if at(0.0, 1.0) != 0.0 {
        return Err("shows before it begins".to_string());
    }
    if at(1.0, 1.0) != 1.0 || at(0.3, 0.2) != 1.0 {
        return Err("not whole by its transition time".to_string());
    }
    if at(0.0, 0.0) != 1.0 {
        return Err("a clip of no transition does not cut straight in".to_string());
    }
    let quarter = at(0.25, 1.0);
    if !(0.8..1.0).contains(&quarter) {
        return Err(format!("{quarter} shown a quarter second in"));
    }
    Ok(format!("{quarter:.2} shown a quarter second into 1 s"))
}
