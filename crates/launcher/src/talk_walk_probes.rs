//! `kjk sim probes "talk walks in the games"`: the player's click to talk
//! (the target block's walk, `sim::target::Walk`) on a creature of one of
//! the games' own levels, from all round it, where this machine has the
//! install (each check passes, saying so, where it has not). Mika Dorin,
//! the bartender of Dreshdae's cantina on Korriban (`korr_m33aa`), stands
//! deep inside a long counter. The walk straight at her ends against the
//! counter's far end, more than a conversation's reach from her.

use sim::module::ObjectKind;
use sim::probe::Checks;
use sim::target::Choice;
use sim::uses::{Choosing, walk_and_choose_from};

/// A module of `game` as a plan reads it; `None` where this machine lacks
/// the install.
fn planned(game: &str, module: &str) -> Option<Result<sim::Level, String>> {
    let installer = crate::scenes::installer(&crate::discover(), game).ok()?;
    let request = installer.request(module, None, 1);
    Some(installer.plan_without_encounters(&request).map(|plan| plan.level))
}

/// How far from the speaker (metres, across the floor) the walks start, and
/// in how many directions round it.
const START_M: f32 = 7.0;
const DIRECTIONS: usize = 16;

/// The click to talk to the creature tagged `tag` in `module`, from every
/// start round it on the floor's walkmesh `START_M` off: whether each walk
/// asked for the conversation within its reach.
fn talks_from_all_round(module: &str, tag: &str) -> Result<String, String> {
    let level = match planned("k1", module) {
        None => return Ok("no k1 install: not checked".into()),
        Some(level) => level?,
    };
    let (index, speaker) = sim::Instances::of(&level, &[])
        .placements()
        .find(|(_, facts)| facts.kind == ObjectKind::Creature && facts.tag.eq_ignore_ascii_case(tag))
        .map(|(index, facts)| (index, facts.position))
        .ok_or(format!("no {tag} in {module}"))?;
    let nav = level.nav().ok_or("no walkmesh")?;
    let reach = sim::target::reach(Choice::Talk, ObjectKind::Creature);
    let (mut talked, mut refused) = (Vec::new(), Vec::new());
    for k in 0..DIRECTIONS {
        let degrees = 360.0 * k as f32 / DIRECTIONS as f32;
        let (sin, cos) = sim::math::sin_cos_deg(degrees);
        let at = [
            speaker[0] + START_M * cos,
            speaker[1] + START_M * sin,
            speaker[2],
        ];
        let Some(z) = nav.height(at, 1.0, 1.0) else {
            continue;
        };
        let from = [at[0], at[1], z];
        let mut world = sim::SimWorld::new(sim::WorldConfig::one_seat(), level.clone())
            .map_err(|error| error.to_string())?;
        let how = Choosing {
            stand: None,
            run_in: 0,
            object: index,
            kind: ObjectKind::Creature,
            choice: Choice::Talk,
            cancel_at: None,
        };
        let mut off = None;
        walk_and_choose_from(&level, &mut world, Some(from), &how, |asked, last, _| {
            if asked {
                off = last
                    .entities
                    .iter()
                    .find(|entity| entity.kind == sim::Kind::Avatar(sim::Seat(0)))
                    .map(|entity| sim::math::flat_distance(entity.position, speaker));
            }
            asked
        })?;
        let line = format!("{degrees:.0}°");
        match off {
            Some(off) if off <= reach => talked.push(format!("{line} {off:.1} m")),
            Some(off) => refused.push(format!("{line} asked {off:.1} m off")),
            None => refused.push(format!("{line} never asked")),
        }
    }
    let what = format!("talked from {talked:?}; not from {refused:?}");
    if refused.is_empty() && !talked.is_empty() {
        Ok(what)
    } else {
        Err(what)
    }
}

/// The checks.
pub fn probes() -> Checks {
    vec![(
        "talk_to_mika_dorin_behind_dreshdae_bar",
        talks_from_all_round("korr_m33aa", "kor33_bartender"),
    )]
}
