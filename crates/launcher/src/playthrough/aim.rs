//! Whether a player could aim the crosshair at what a goal uses.
//!
//! The walker clicks an object by its number (`sim::target::ask`), which the
//! game's player cannot: the player's E acts on what the crosshair picks
//! (`sim::target::pick`, in the windowed game only), and it sends no key
//! trace (`buttons::USE`, the harness's alone). So a use goal clicks, or
//! presses its key for, only what the crosshair picks, turned to it from its eyes or a camera
//! over its shoulder at any of a few heights up the object: from where the
//! player stands, or from a floor round the object a way leads to, within
//! the crosshair's range (the game walks the player over to what it
//! clicked). Peragus' consoles, which the crosshair could not pick, played
//! through every chain while no player could use them.

use super::*;

/// Heights over the player's feet (metres) the crosshair is aimed from:
/// its eyes (`targeting.pick.eyes`) and a camera over its shoulder.
const FROM: [f32; 2] = [sim::target::EYES, 2.2];

/// How far up the object's pick volume each look aims.
const SHARES: [f32; 4] = [0.25, 0.5, 0.75, 1.0];

/// The rings of floor round the object (metres, on the flat) a player may
/// aim from instead, within the crosshair's range, eight spots each.
const RINGS: [f32; 4] = [2.0, 4.0, 6.0, 8.0];
const _: () = assert!(RINGS[RINGS.len() - 1] <= sim::target::RANGE);

/// Whether the crosshair of the player moving `player` (body `me`) picks
/// object `index`, from where it stands or from a floor round the object
/// it can walk to; false for an object no crosshair can target at all.
pub(super) fn clickable(here: &Here, snapshot: &Snapshot, (me, player): (sim::SimId, &sim::motion::MotionState), index: u32) -> bool {
    let candidates = sim::target::candidates(here.level, snapshot, Some(me));
    let Some(target) = candidates.iter().find(|candidate| candidate.index == index) else {
        return false;
    };
    let states = here.level.states_in(snapshot);
    let aimed = |origin: [f32; 3]| aimable(here.level, states.as_deref(), &candidates, target, sim::target::feet_at(origin));
    aimed(player.origin)
        || RINGS
            .into_iter()
            .flat_map(|far| {
                (0..8).map(move |k| {
                    let angle = k as f32 * std::f32::consts::FRAC_PI_4;
                    [target.feet[0] + far * angle.cos(), target.feet[1] + far * angle.sin(), target.feet[2]]
                })
            })
            .map(|spot| walker_to(here.level, player, spot).goal)
            .any(|origin| aimed(origin) && here.leads(player.origin, origin))
}

/// Whether some look of a player with its feet at `feet` (metres), turned
/// to `target`, picks it.
fn aimable(
    level: &sim::Level,
    states: Option<&[u8]>,
    candidates: &[sim::target::Candidate],
    target: &sim::target::Candidate,
    feet: [f32; 3],
) -> bool {
    let (dx, dy) = (target.feet[0] - feet[0], target.feet[1] - feet[1]);
    let across = dx.hypot(dy);
    let looker = sim::target::Looker {
        feet,
        facing: if across > 1e-4 { [dx / across, dy / across] } else { [1.0, 0.0] },
    };
    let weighed = sim::target::weighed(candidates, &looker);
    FROM.into_iter().any(|height| {
        let from = [feet[0], feet[1], feet[2] + height];
        SHARES.into_iter().any(|share| {
            let mut forward = sim::math::sub(target.up(share), from);
            sim::math::normalize(&mut forward) > 0.0
                && sim::target::pick(level, states, &sim::target::Look { from, forward }, &looker, &weighed)
                    == Some(target.index)
        })
    })
}
