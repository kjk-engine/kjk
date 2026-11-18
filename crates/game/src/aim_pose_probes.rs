//! The aim pose's probes (`kjk sim probes --only "aim pose"`): on JA's own
//! skeleton and animations, a body raising its gun to aim follows the
//! sim's raise without a jump, holds each wield's gun at the height of its
//! eyes, walks on with its legs untouched, and another character holds its
//! gun up after a shot for `aim_pose::OTHERS_HOLD_MS`. A Sniper Shot's sight
//! shows while it is steadied, not while the gun is only aimed. The camera's
//! checks while aiming (`play_view::aim_camera_probes`) run in this group too.

use std::collections::BTreeMap;
use std::sync::Arc;

use bevy::prelude::*;
use sim::combat::saber::blades::Shown;
use sim::combat::specials::Shooting;
use sim::motion::Anim;
use sim::skeleton::{Playback, Played};
use sim::{EntityState, Seat};

use crate::aim_pose::{self, Wield};
use crate::cast::{Figure, Rig};
use crate::probe_kit::{Checks, check};

pub fn probes() -> Checks {
    let mut checks = match Ja::load() {
        Ok(ja) => vec![
            ("aim_pose_follows", follows(&ja)),
            ("aim_pose_wields", wields(&ja)),
            ("aim_pose_walks", walks(&ja)),
            ("others_raise", others_raise(&ja)),
        ],
        Err(why) => [
            "aim_pose_follows",
            "aim_pose_wields",
            "aim_pose_walks",
            "others_raise",
        ]
        .map(|name| (name, Ok(format!("skipped: {why}"))))
        .into(),
    };
    checks.push(("steady_sight", steady_sight()));
    // The camera's checks need no install.
    checks.extend(crate::play_view::aim_camera_probes());
    checks
}

/// One sim step (ms): the sim advances at 125 steps a second.
const STEP_MS: u64 = 8;
/// When the body starts to aim (sim ms): it stands ready until then.
const AIM_FROM: u64 = 1000;
/// How near (metres) the hand ends to where the full raise holds it.
const END_NEAR: f32 = 0.02;
/// The largest share of the whole raise one step may move the hand.
const STEP_MOST: f32 = 0.1;
/// How near (metres) a raised gun's hand stands to the height of the eyes.
const EYE_NEAR: f32 = 0.06;
/// How far (metres) aiming may move a walking body's feet.
const FEET_NEAR: f32 = 0.001;
/// When another character fires (sim ms).
const SHOT_AT: u64 = 1000;
/// How near (metres) a gun let down comes back to the ready pose.
const DOWN_NEAR: f32 = 0.01;

/// JA's player skeleton, its animations and its hand and eye bolts.
struct Ja {
    rig: Rig,
    eyes: Option<asset_ja::Bolt>,
    body: EntityState,
}

impl Ja {
    fn load() -> Result<Ja, String> {
        let stack = crate::viewer_ja::stack(&crate::discover())?;
        let files = crate::viewer_ja::Files::base(&stack);
        let (glm, gla, gla_path) = crate::viewer_ja::glm_and_gla(&files, crate::stage::JA_BODY)?;
        let cfg = crate::viewer_ja::animation_cfg(&files, &gla_path)?;
        let sequences: BTreeMap<String, fmt_ja::animation_cfg::Sequence> = (cfg.sequences.iter())
            .map(|s| (s.name.to_ascii_uppercase(), s.clone()))
            .collect();
        let gla: &'static fmt_ja::ghoul2::Gla = Box::leak(Box::new(gla));
        let rig = Rig {
            gla,
            sequences,
            ja_sole: crate::viewer_rig::sole_height(&glm),
            quirks: false,
            hands: ["*r_hand", "*l_hand"].map(|tag| asset_ja::bolt(&glm, tag)),
            sabers: crate::weapons::Sabers::load(&files)?,
            events: Arc::new(asset_ja::events::Events::default()),
        };
        let mut world = sim::SimWorld::new(sim::WorldConfig::one_seat(), sim::Level::arena())
            .map_err(|error| error.to_string())?;
        let body = *world
            .snapshot()
            .body(Seat(0))
            .ok_or("the arena has no body")?;
        Ok(Ja {
            eyes: asset_ja::bolt(&glm, "*head_eyes"),
            rig,
            body,
        })
    }

    /// The part `anim` played from `start_ms`, looping or held at its end.
    fn part(&self, anim: Anim, start_ms: u64, looping: bool) -> Result<Shown, String> {
        let sequence =
            (self.rig.sequences.get(anim.name())).ok_or(format!("no {}", anim.name()))?;
        Ok(Shown {
            anim,
            flip: false,
            playback: Playback {
                start_frame: sequence.first_frame,
                end_frame: sequence.first_frame + sequence.frame_count,
                speed: sequence.frames_per_second.max(1) as f32 / 20.0,
                start_ms: start_ms as i64,
                looping,
                blend: None,
            },
            plays: Played::OWN,
        })
    }

    /// How long `anim` plays (ms).
    fn length_ms(&self, anim: Anim) -> u64 {
        self.rig.sequences.get(anim.name()).map_or(0, |sequence| {
            u64::from(sequence.frame_count.max(0) as u32) * 1000
                / u64::from(sequence.frames_per_second.max(1) as u32)
        })
    }
}

/// What the body plays and how it shoots at one step.
struct Step {
    legs: Shown,
    torso: Shown,
    shooting: Shooting,
}

/// A wield's ready pose and shot, and the stance wield number.
fn gun(wield: Wield) -> (Anim, Anim, u8) {
    match wield {
        Wield::Pistol => (Anim::TORSO_WEAPONREADY2, Anim::BOTH_ATTACK2, 5),
        Wield::Pair => (Anim::TORSO_WEAPONREADY2, Anim::BOTH_ATTACK2, 6),
        Wield::Rifle => (Anim::TORSO_WEAPONREADY3, Anim::BOTH_ATTACK3, 7),
    }
}

/// A figure holding `wield`'s gun, followed each step to `end_ms` as
/// `step` says (`seat`: a seat acts through it), and measured by
/// `measure` after each step.
fn run<T>(
    ja: &Ja,
    (wield, seat, end_ms): (Wield, bool, u64),
    step: impl Fn(u64) -> Result<Step, String>,
    mut measure: impl FnMut(&Figure, u64) -> T,
) -> Result<Vec<T>, String> {
    let (ready, _, number) = gun(wield);
    let mut figure = Figure::new(&ja.rig, 0.0, ready.name());
    figure.hold_gun(number);
    let mut out = Vec::new();
    for now in (0..=end_ms).step_by(STEP_MS as usize) {
        let Step {
            legs,
            torso,
            shooting,
        } = step(now)?;
        let mut entity = ja.body;
        let (Some(player), Some(fighter)) = (entity.player.as_mut(), entity.fighter.as_mut())
        else {
            return Err("the arena's body is no fighter".into());
        };
        player.legs.anim = legs.anim;
        player.torso.anim = torso.anim;
        fighter.pose.legs = Some(legs);
        fighter.pose.torso = Some(torso);
        fighter.shooting = shooting;
        figure.follow(
            &ja.rig,
            (now, Default::default(), None),
            &entity,
            (true, seat),
        );
        out.push(measure(&figure, now));
    }
    Ok(out)
}

/// The sim's raise after holding [Aim] from `AIM_FROM` to `now`.
fn aimed(now: u64) -> Shooting {
    let mut shooting = Shooting::default();
    for _ in (AIM_FROM..now).step_by(STEP_MS as usize) {
        shooting.aim_step(true, STEP_MS as u32);
    }
    shooting
}

/// Standing ready with `wield`'s gun, aiming from `AIM_FROM`.
fn standing(ja: &Ja, wield: Wield) -> impl Fn(u64) -> Result<Step, String> + '_ {
    let (ready, ..) = gun(wield);
    move |now| {
        Ok(Step {
            legs: ja.part(Anim::BOTH_STAND1, 0, true)?,
            torso: ja.part(ready, 0, true)?,
            shooting: aimed(now),
        })
    }
}

/// Where hand `hand` (0 right, 1 left) holds its gun at `ms` (metres,
/// JA's model space).
fn hand(ja: &Ja, figure: &Figure, ms: u64, hand: usize) -> Option<Vec3> {
    let bolt = ja.rig.hands[hand].as_ref()?;
    Some(figure.bolt(ms as f64, bolt).w_axis.truncate())
}

/// The height of the eyes at `ms` (metres).
fn eyes(ja: &Ja, figure: &Figure, ms: u64) -> Option<f32> {
    match &ja.eyes {
        Some(bolt) => Some(figure.bolt(ms as f64, bolt).w_axis.z),
        None => (figure.ja_points(ms as f64, &["cranium"]).first()).map(|(_, at)| at.z),
    }
}

/// A drawn pistol, the sim's raise stepped from 0 to 1 over 200 ms: the
/// right hand rises at every step, ends within `END_NEAR` of where the
/// full raise holds it, and no step moves it more than `STEP_MOST` of the
/// whole way.
fn follows(ja: &Ja) -> Result<String, String> {
    let end = AIM_FROM + 2 * u64::from(rules_in_ms());
    let at = run(
        ja,
        (Wield::Pistol, true, end),
        standing(ja, Wield::Pistol),
        |figure, now| (now, hand(ja, figure, now, 0)),
    )?;
    let points: Vec<(u64, Vec3)> = (at.into_iter())
        .filter_map(|(now, point)| Some((now, point?)))
        .filter(|&(now, _)| now + STEP_MS >= AIM_FROM)
        .collect();
    let (Some(&(_, start)), Some(&(_, held))) = (points.first(), points.last()) else {
        return Err("no right hand bolt".into());
    };
    let top = AIM_FROM + u64::from(rules_in_ms());
    let ended = (points.iter())
        .find(|&&(now, _)| now >= top)
        .map_or(f32::MAX, |&(_, point)| point.distance(held));
    let travel = start.distance(held);
    let steps: Vec<f32> = points.windows(2).map(|w| w[1].1.distance(w[0].1)).collect();
    let most = steps.iter().copied().fold(0.0, f32::max);
    let falls = (points.windows(2))
        .filter(|w| w[1].1.z < w[0].1.z - 1e-4)
        .count();
    check(
        travel > 0.05 && falls == 0 && ended <= END_NEAR && most <= travel * STEP_MOST,
        format!(
            "the hand travels {travel:.3} m, rising {:.3} m; {falls} steps lower it; it ends {ended:.3} m from the full raise; the largest step moves it {most:.3} m ({:.0}% of the way)",
            held.z - start.z,
            100.0 * most / travel.max(1e-6)
        ),
    )
}

fn rules_in_ms() -> u32 {
    rules::weapons::aim::IN_MS
}

/// At full raise, each wield's gun hand (both of a pair) stands within
/// `EYE_NEAR` of the eyes' height.
fn wields(ja: &Ja) -> Result<String, String> {
    let full = AIM_FROM + 2 * u64::from(rules_in_ms());
    let mut rows = Vec::new();
    let mut wrong = Vec::new();
    for wield in [Wield::Pistol, Wield::Pair, Wield::Rifle] {
        let hands: &[usize] = if wield == Wield::Pair { &[0, 1] } else { &[0] };
        let measured = run(
            ja,
            (wield, true, full),
            standing(ja, wield),
            |figure, now| {
                (now == full).then(|| {
                    let eye = eyes(ja, figure, now);
                    let ready = (hands.iter())
                        .map(|&at| hand(ja, figure, now, at).map(|point| point.z))
                        .collect::<Vec<_>>();
                    (eye, ready)
                })
            },
        )?;
        let Some((Some(eye), heights)) = measured.into_iter().flatten().last() else {
            return Err(format!("{wield:?}: no eyes"));
        };
        for (at, height) in hands.iter().zip(heights) {
            let height = height.ok_or(format!("{wield:?}: no hand bolt {at}"))?;
            let off = height - eye;
            rows.push(format!("{wield:?} hand {at}: {off:+.3} m"));
            if off.abs() > EYE_NEAR {
                wrong.push(format!("{wield:?} hand {at}"));
            }
        }
    }
    check(
        wrong.is_empty(),
        format!(
            "gun hands from the eyes' height: {}; wrong: {wrong:?}",
            rows.join(", ")
        ),
    )
}

/// Walking (`BOTH_WALK1`) with a pistol, aimed and not: the legs play the
/// walk, and aiming moves the feet no more than `FEET_NEAR`.
fn walks(ja: &Ja) -> Result<String, String> {
    const FEET: [&str; 2] = ["rtalus", "ltalus"];
    let end = AIM_FROM + 1000;
    let walking = |aim: bool| {
        move |now| -> Result<Step, String> {
            Ok(Step {
                legs: ja.part(Anim::BOTH_WALK1, 0, true)?,
                torso: ja.part(Anim::TORSO_WEAPONREADY2, 0, true)?,
                shooting: if aim { aimed(now) } else { Shooting::default() },
            })
        }
    };
    let feet = |aim: bool| {
        run(
            ja,
            (Wield::Pistol, true, end),
            walking(aim),
            |figure, now| {
                let legs = figure.shown_parts().map(|[legs, _]| legs.to_string());
                (legs, figure.ja_points(now as f64, &FEET))
            },
        )
    };
    let (aimed, plain) = (feet(true)?, feet(false)?);
    let mut most = 0.0_f32;
    let mut legs = Vec::new();
    for ((shown, points), (_, plain)) in aimed.iter().zip(&plain) {
        if points.len() != FEET.len() {
            return Err(format!("no feet: {points:?}"));
        }
        for ((_, a), (_, b)) in points.iter().zip(plain) {
            most = most.max(a.distance(*b));
        }
        legs.extend(shown.clone().filter(|name| !legs.contains(name)));
    }
    check(
        most <= FEET_NEAR && legs == ["BOTH_WALK1"],
        format!("the legs play {legs:?}; aiming moves the feet at most {most:.4} m"),
    )
}

/// Another character (no seat acts through it) fires a pistol at
/// `SHOT_AT`: 500 ms later its gun hand is where the full raise holds it,
/// and once `OTHERS_HOLD_MS` and a raise's blend have passed it is back
/// where the ready pose holds it.
fn others_raise(ja: &Ja) -> Result<String, String> {
    let shot_ms = ja.length_ms(Anim::BOTH_ATTACK2);
    // The first steps at or after 500 ms, and after the hold and a blend.
    let step = |ms: u64| ms.div_ceil(STEP_MS) * STEP_MS;
    let held_at = step(SHOT_AT + 500);
    let down_at = step(SHOT_AT + aim_pose::OTHERS_HOLD_MS as u64 + 150 + STEP_MS);
    let firing = |now: u64| -> Result<Step, String> {
        let torso = if (SHOT_AT..SHOT_AT + shot_ms).contains(&now) {
            ja.part(Anim::BOTH_ATTACK2, SHOT_AT, false)?
        } else if now >= SHOT_AT {
            ja.part(Anim::TORSO_WEAPONREADY2, SHOT_AT + shot_ms, true)?
        } else {
            ja.part(Anim::TORSO_WEAPONREADY2, 0, true)?
        };
        Ok(Step {
            legs: ja.part(Anim::BOTH_STAND1, 0, true)?,
            torso,
            shooting: Shooting::default(),
        })
    };
    let marks = [SHOT_AT - STEP_MS, held_at, down_at];
    let measured = run(
        ja,
        (Wield::Pistol, false, down_at),
        firing,
        |figure, now| {
            marks
                .contains(&now)
                .then(|| hand(ja, figure, now, 0))
                .flatten()
        },
    )?;
    let [ready, held, down] =
        <[Vec3; 3]>::try_from(measured.into_iter().flatten().collect::<Vec<_>>())
            .map_err(|found| format!("measured {found:?}"))?;
    let full = AIM_FROM + 2 * u64::from(rules_in_ms());
    let raised = run(
        ja,
        (Wield::Pistol, true, full),
        standing(ja, Wield::Pistol),
        |figure, now| hand(ja, figure, now, 0),
    )?
    .into_iter()
    .last()
    .flatten()
    .ok_or("no right hand bolt")?;
    let (up, back) = (held.distance(raised), down.distance(ready));
    check(
        up <= END_NEAR && back <= DOWN_NEAR,
        format!(
            "{} ms after the shot the hand is {up:.3} m from the full raise; at {} ms it is {back:.3} m from the ready pose",
            held_at - SHOT_AT,
            down_at - SHOT_AT
        ),
    )
}

/// A Sniper Shot steadied shows its sight line; a gun only aimed shows
/// none.
fn steady_sight() -> Result<String, String> {
    let mut aiming = Shooting::default();
    for _ in 0..50 {
        aiming.aim_step(true, STEP_MS as u32);
    }
    let steadying = Shooting {
        charging_from: Some(1000),
        steadying: true,
        ..aiming
    };
    let [aimed, steadied] =
        [aiming, steadying].map(|shooting| crate::aim_fx::holds_feat(&shooting));
    check(
        aiming.aiming && steadied && !aimed,
        format!(
            "aimed (raised {}): sight {aimed}; steadied: sight {steadied}",
            aiming.aiming
        ),
    )
}
