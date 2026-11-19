//! The grapple finishers lined up as they are drawn (`sim::world::grapples`
//! moves the pair, `asset_ja::grapple` measures it). Jedi Academy's pair,
//! played on JA's own skeleton 46 units apart, gives where the grabber's
//! hands touch the victim (`contacts`, found once a run). During each touch
//! the view pulls the grabber's hand onto the same place on the victim's
//! drawn KOTOR body (`touches`): the victim's node, and JA's offset from it
//! scaled to the bodies' size, eased in and out over
//! `grab::CONTACT_BLEND_MS`. Only the drawing moves: the sim's places and
//! hits stay as they are.
//!
//! The probes (`probes`) play each finisher between Carth and a Sith
//! trooper on the installs' data: JA's touches are found, the sim's gap
//! (`grab::GAP_M`) is the one that brings the KOTOR hands nearest them,
//! every touch meets within `MEET_M` once pulled, the victim stands where
//! JA's stands, and no clip is played twice.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use asset_ja::grapple::Contact;
#[cfg(feature = "probes")]
use asset_ja::grapple::Pair;
use asset_ja::rig::{Side, Touch};
use asset_odyssey::anim::AnimationIr;
use bevy::prelude::*;
use fmt_ja::animation_cfg::Sequence;
use fmt_ja::ghoul2::Gla;
use rules::duel::grab;
use sim::combat::grab::Grapple;

/// Each finisher's JA clips: the grabber's and the victim's.
pub const FINISHERS: [(&str, &str); 3] = [
    ("BOTH_KYLE_PA_1", "BOTH_PLAYER_PA_1"),
    ("BOTH_KYLE_PA_2", "BOTH_PLAYER_PA_2"),
    ("BOTH_KYLE_PA_3", "BOTH_PLAYER_PA_3"),
];

/// JA's own gap between its pair (metres): `idealDist`, 46 units.
pub fn ja_gap_m() -> f32 {
    grab::BREAK_DIST[0] * asset_ja::UNIT
}

/// The JA clip of sequence `name`, baked as the view's sequences are
/// (`viewer_ja::sequences`), alone and from its first frame.
pub fn clip(gla: &Gla, sequences: &BTreeMap<String, Sequence>, name: &str) -> Option<AnimationIr> {
    let sequence = sequences.get(name)?;
    let (cues, length) = asset_ja::chain(gla, &[(sequence, 0.0)])?;
    Some(asset_ja::g2::bake_with(
        asset_ja::g2::Player::new(gla),
        &cues,
        length,
        5.0,
        name,
    ))
}

/// Each finisher's JA clips, grabber's then victim's.
pub fn clips(
    gla: &Gla,
    sequences: &BTreeMap<String, Sequence>,
) -> Option<Vec<(AnimationIr, AnimationIr)>> {
    FINISHERS
        .iter()
        .map(|(grabber, victim)| {
            Some((
                clip(gla, sequences, grabber)?,
                clip(gla, sequences, victim)?,
            ))
        })
        .collect()
}

/// JA's touches for each finisher (`asset_ja::grapple::contacts`), within
/// `grab::CONTACT_M`.
pub fn contacts_of(gla: &Gla, clips: &[(AnimationIr, AnimationIr)]) -> Vec<Vec<Contact>> {
    clips
        .iter()
        .map(|(grabber, victim)| {
            // A hand let go for less than an ease out and back in is
            // held through.
            let bridge = 2.0 * grab::CONTACT_BLEND_MS / 1000.0;
            asset_ja::grapple::contacts(
                gla,
                (grabber, victim),
                ja_gap_m(),
                (grab::CONTACT_M, bridge),
            )
        })
        .collect()
}

/// The touches, found once a run from the view's JA skeleton.
pub fn contacts(gla: &Gla, sequences: &BTreeMap<String, Sequence>) -> &'static [Vec<Contact>] {
    static FOUND: OnceLock<Vec<Vec<Contact>>> = OnceLock::new();
    FOUND.get_or_init(|| {
        clips(gla, sequences).map_or_else(Vec::new, |clips| contacts_of(gla, &clips))
    })
}

/// How much (0 to 1) a touch from `from_s` to `to_s` holds the hand at
/// `t` seconds: whole within it, eased in over `grab::CONTACT_BLEND_MS`
/// before it and out over as long after it.
pub fn weight(contact: &Contact, t: f32) -> f32 {
    let blend = grab::CONTACT_BLEND_MS / 1000.0;
    if blend <= 0.0 {
        return f32::from(u8::from((contact.from_s..=contact.to_s).contains(&t)));
    }
    let rise = ((t - (contact.from_s - blend)) / blend).clamp(0.0, 1.0);
    let fall = (((contact.to_s + blend) - t) / blend).clamp(0.0, 1.0);
    let x = rise.min(fall);
    x * x * (3.0 - 2.0 * x)
}

/// The victim's KOTOR nodes a hand may touch, where each is drawn (world,
/// metres), with its body's scale against JA's.
#[derive(Debug, Clone, Copy)]
pub struct Held {
    pub nodes: [Vec3; 6],
    pub scale: f32,
}

/// The grabber's touches at `t` seconds into its finisher (`grapple`),
/// its view's yaw `yaw` (degrees), its own scale `scale`, against the
/// victim drawn as `held`: each hand onto its touch's place (world,
/// metres), the one weighing most where two overlap.
pub fn touches(
    contacts: &[Vec<Contact>],
    grapple: &Grapple,
    t: f32,
    (yaw, scale): (f32, f32),
    held: &Held,
) -> Vec<(Side, Vec3, f32)> {
    let Some(found) = contacts.get(usize::from(grapple.finisher.saturating_sub(1))) else {
        return Vec::new();
    };
    // The pair's space to the world's: its +Y along the grabber's view.
    let turn = Quat::from_rotation_z((yaw - 90.0).to_radians());
    let size = (scale + held.scale) / 2.0;
    let mut best: BTreeMap<u8, (Side, Vec3, f32)> = BTreeMap::new();
    for contact in found {
        let w = weight(contact, t);
        let node = asset_ja::grapple::VICTIM_BONES
            .iter()
            .position(|(_, kotor)| *kotor == contact.bone);
        let Some(node) = node.filter(|_| w > 0.0) else {
            continue;
        };
        let target = held.nodes[node] + turn * contact.offset_at(t) * size;
        let key = u8::from(contact.side == Side::Left);
        if best.get(&key).is_none_or(|(.., was)| *was < w) {
            best.insert(key, (contact.side, target, w));
        }
    }
    best.into_values().collect()
}

/// The touches as the pose takes them: each place in the model's space
/// (`to_model`, the model standing on `soles`).
pub fn in_model(touches: &[(Side, Vec3, f32)], to_model: Quat, soles: Vec3) -> Vec<Touch> {
    touches
        .iter()
        .map(|&(side, target, weight)| Touch {
            side,
            target: to_model * (target - soles),
            weight,
        })
        .collect()
}

/// How near (metres) every touch must meet once the hand is pulled on.
pub const MEET_M: f32 = 0.05;

/// How far either side of JA's gap, scaled, the fit searches (metres).
#[cfg(feature = "probes")]
const SEARCH_M: f32 = 0.2;

/// Where a fitted gap must lie (metres): JA's 46 units scaled by the
/// bodies' size give about 1.45 m, and the fit moves it at most `SEARCH_M`.
#[cfg(feature = "probes")]
const FITTED_M: (f32, f32) = (1.30, 1.60);

#[cfg(feature = "probes")]
/// The fitted gap may lie this far (metres) from the sim's (`grab::GAP_M`).
const GAP_KEPT_M: f32 = 0.03;

#[cfg(feature = "probes")]
/// The grabber's body (Carth) and the victim's (a Sith trooper).
const GRABBER: &str = "p_carthbb";
#[cfg(feature = "probes")]
const VICTIM: &str = "n_sithsoldier";

#[cfg(feature = "probes")]
pub fn probes() -> sim::probe::Checks {
    match measured() {
        Ok(checks) => checks,
        Err(why) => vec![("grapple contacts", Ok(format!("skipped: {why}")))],
    }
}

#[cfg(feature = "probes")]
/// How near (seconds) the sim's let-go must lie to the last touch: one
/// frame of Jedi Academy's 29 a second.
const LET_GO_S: f32 = 0.035;

#[cfg(feature = "probes")]
/// How high (metres) the victim's pelvis rises in Jedi Academy's second
/// finisher when the grabber swings it over by its foot. Standing, the
/// pelvis is 0.76 m up.
const SWUNG_M: f32 = 1.2;

#[cfg(feature = "probes")]
/// The second finisher swings the victim over by its foot and slams it
/// down, then swings it back over and slams it again before it gets up.
/// The second swing looks like a jump on the KOTOR body, so the sim lets go
/// (`grab::LET_GO_MS`) as the victim lies lowest between the two swings:
/// the first slam, when the grabber's left hand leaves the foot.
fn let_go(gla: &Gla, clips: &[(AnimationIr, AnimationIr)]) -> Result<String, String> {
    let (_, victim) = clips.get(1).ok_or("no second finisher")?;
    let pelvis: Vec<(f32, f32)> = asset_ja::rig::ja_points(gla, victim, &["pelvis"])
        .into_iter()
        .filter_map(|(time, points)| Some((time, points.first()?.z)))
        .collect();
    let swung: Vec<usize> = (1..pelvis.len().saturating_sub(1))
        .filter(|&at| {
            let z = pelvis[at].1;
            z > SWUNG_M && z >= pelvis[at - 1].1 && z > pelvis[at + 1].1
        })
        .collect();
    let [.., first, second] = swung[..] else {
        return Err(format!(
            "the victim is swung over {} times, not twice",
            swung.len()
        ));
    };
    let (lowest, z) = pelvis[first..=second]
        .iter()
        .copied()
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .ok_or("no keys between the swings")?;
    let sim = grab::LET_GO_MS as f32 / 1000.0;
    sim::probe::check(
        (lowest - sim).abs() <= LET_GO_S,
        format!(
            "swung over {:.2} s and {:.2} s in; the victim lies lowest between them {lowest:.3} s in (pelvis {z:.2} m up); the sim lets go {sim:.3} s in",
            pelvis[first].0, pelvis[second].0
        ),
    )
}

#[cfg(feature = "probes")]
/// The checks on the installs' data (see the module documentation).
fn measured() -> Result<sim::probe::Checks, String> {
    use sim::probe::check;
    let discovery = crate::discover();
    let stack = crate::viewer_ja::stack(&discovery)?;
    let files = crate::viewer_ja::Files::base(&stack);
    let (glm, gla, gla_path) = crate::viewer_ja::glm_and_gla(&files, crate::stage::JA_BODY)?;
    let cfg = crate::viewer_ja::animation_cfg(&files, &gla_path)?;
    let sequences: BTreeMap<String, Sequence> = cfg
        .sequences
        .iter()
        .map(|s| (s.name.to_ascii_uppercase(), s.clone()))
        .collect();
    let resolver = crate::viewer::resolver(&discovery, vfs::Game::Kotor1, None)?;
    let mut cache = BTreeMap::new();
    let model = |name: &str, cache: &mut BTreeMap<_, _>| {
        crate::viewer::cached(cache, &resolver, name)?.ok_or(format!("{name}.mdl: not found"))
    };
    let (grabber, victim) = (model(GRABBER, &mut cache)?, model(VICTIM, &mut cache)?);
    let ja_sole = crate::viewer_rig::sole_height(&glm);
    let clips = clips(&gla, &sequences).ok_or("a finisher's clip is not in animation.cfg")?;
    let contacts = contacts_of(&gla, &clips);
    let mut found = Vec::new();
    let mut fitted = Vec::new();
    let mut met = Vec::new();
    let mut facing = Vec::new();
    let (mut found_ok, mut fitted_ok, mut met_ok, mut facing_ok) = (true, true, true, true);
    for (n, ((grabber_clip, victim_clip), touches)) in clips.iter().zip(&contacts).enumerate() {
        let finisher = n + 1;
        let foot = touches.iter().any(|c| c.bone.ends_with("foot_g"));
        found_ok &= !touches.is_empty() && (finisher != 2 || foot);
        let listed: Vec<String> = touches
            .iter()
            .map(|c| {
                format!(
                    "{:?} hand on {} {:.2}-{:.2} s",
                    c.side, c.bone, c.from_s, c.to_s
                )
            })
            .collect();
        found.push(format!("finisher {finisher}: {}", listed.join(", ")));
        let pair = Pair::new(
            (&grabber, &victim),
            &gla,
            (grabber_clip, victim_clip),
            ja_sole,
        );
        let gap = pair.fitted_gap(touches, ja_gap_m(), SEARCH_M);
        let sim_gap = grab::GAP_M[n];
        fitted_ok &=
            (FITTED_M.0..=FITTED_M.1).contains(&gap) && (gap - sim_gap).abs() <= GAP_KEPT_M;
        fitted.push(format!(
            "finisher {finisher}: {gap:.2} m (the sim stands them {sim_gap:.2} m apart; scale {:.2})",
            pair.scale()
        ));
        let before = pair.errors(touches, sim_gap, false);
        let after = pair.errors(touches, sim_gap, true);
        met_ok &= after.iter().all(|e| *e < MEET_M);
        let cm = |errors: &[f32]| {
            errors
                .iter()
                .map(|e| format!("{:.0}", e * 100.0))
                .collect::<Vec<_>>()
                .join("/")
        };
        met.push(format!(
            "finisher {finisher}: {} cm before, {} cm after",
            cm(&before),
            cm(&after)
        ));
        let (ja, kotor) = pair.sides();
        facing_ok &= ja.signum() == kotor.signum() || (ja.abs() < 0.05 && kotor.abs() < 0.08);
        facing.push(format!(
            "finisher {finisher}: the victim's pelvis at worst {ja:+.2} m to the grabber's right in JA, {kotor:+.2} m on the KOTOR bodies"
        ));
    }
    Ok(vec![
        ("ja_contacts_found", check(found_ok, found.join("; "))),
        ("gap_fitted", check(fitted_ok, fitted.join("; "))),
        ("contacts_meet_after_ik", check(met_ok, met.join("; "))),
        ("victim_stands_as_ja", check(facing_ok, facing.join("; "))),
        ("let_go_before_second_lift", let_go(&gla, &clips)),
    ])
}
