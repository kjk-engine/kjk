//! What a held body plays, as the games' executables have it
//! (`acting::of_state`), and how a scripted clip takes over
//! (`acting::blended_in`).

use sim::combat::effects::state;

use crate::acting;
use crate::probe_kit::Checks;

pub fn probes() -> Checks {
    vec![
        ("each state's clip", states_play_their_clips()),
        ("a clip's blend", clips_blend_as_the_engine_does()),
    ]
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
