//! The aim pose's probes (`kjk sim probes --only "aim pose"`): on JA's own
//! skeleton and animations, a body raising its gun to aim follows the
//! sim's raise without a jump, holds each wield's gun at the height of its
//! eyes, walks on with its legs untouched, and another character holds its
//! gun up after a shot for `aim_pose::OTHERS_HOLD_MS` and while it charges
//! a Power Blast, raising it with no jump either way. A Sniper Shot's sight
//! shows while it is steadied, not while the gun is only aimed. The camera's
//! checks while aiming (`play_view::aim_camera_probes`) run in this group too.

use std::collections::BTreeMap;
use std::sync::Arc;

use bevy::prelude::*;
use rules::weapons::aim::{IN_MS, OUT_MS};
use sim::combat::saber::blades::Shown;
use sim::combat::specials::Shooting;
use sim::motion::Anim;
use sim::skeleton::{Blend, Playback, Played};
use sim::{EntityState, Seat};

use crate::aim_pose::{self, Wield};
use crate::cast::{Figure, RAISE_MS, Rig};
use crate::probe_kit::{Checks, check};

pub fn probes() -> Checks {
    let mut checks = match Ja::load() {
        Ok(ja) => vec![
            ("aim_pose_follows", follows(&ja)),
            ("aim_pose_wields", wields(&ja)),
            ("aim_pose_walks", walks(&ja)),
            ("others_raise", others_raise(&ja)),
            ("others_charge", others_charge(&ja)),
        ],
        Err(why) => [
            "aim_pose_follows",
            "aim_pose_wields",
            "aim_pose_walks",
            "others_raise",
            "others_charge",
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

/// When [Aim] is let go of, for a body that never lets go of it.
const NEVER: u64 = u64::MAX;

/// The sim's raise at `now` after [Aim] was pressed at `AIM_FROM` and let
/// go of at `release`.
fn aimed(now: u64, release: u64) -> Shooting {
    let mut shooting = Shooting::default();
    for at in (AIM_FROM..now).step_by(STEP_MS as usize) {
        shooting.aim_step(at < release, STEP_MS as u32);
    }
    shooting
}

/// Standing ready with `wield`'s gun, aiming from `AIM_FROM` until
/// `release`.
fn standing(ja: &Ja, wield: Wield, release: u64) -> impl Fn(u64) -> Result<Step, String> + '_ {
    let (ready, ..) = gun(wield);
    move |now| {
        Ok(Step {
            legs: ja.part(Anim::BOTH_STAND1, 0, true)?,
            torso: ja.part(ready, 0, true)?,
            shooting: aimed(now, release),
        })
    }
}

/// Where `wield`'s right hand holds its gun at full raise: a body that is
/// fully aimed from its first step, so that no raise is stepped through.
fn full_raise(ja: &Ja, wield: Wield) -> Result<Vec3, String> {
    let (ready, ..) = gun(wield);
    let full = aimed(AIM_FROM + 2 * u64::from(IN_MS), NEVER);
    let held = move |_| {
        Ok(Step {
            legs: ja.part(Anim::BOTH_STAND1, 0, true)?,
            torso: ja.part(ready, 0, true)?,
            shooting: full,
        })
    };
    run(ja, (wield, true, AIM_FROM), held, |figure, now| {
        hand(ja, figure, now, 0)
    })?
    .into_iter()
    .last()
    .flatten()
    .ok_or("no right hand bolt".into())
}

/// The largest distance (metres) between two neighbouring points.
fn largest_step(points: &[Vec3]) -> f32 {
    (points.windows(2))
        .map(|pair| pair[1].distance(pair[0]))
        .fold(0.0, f32::max)
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

/// A drawn pistol, the sim's raise stepped from 0 to 1 (`IN_MS`) and,
/// after [Aim] is let go of, back to 0 (`OUT_MS`). On the way up the
/// right hand rises at every step and ends within `END_NEAR` of where a
/// body held at full raise from the start holds it (`full_raise`). On the
/// way down it ends within `DOWN_NEAR` of where it began. No step on the
/// way up moves it more than `STEP_MOST` of the whole way. The way down is
/// faster (`OUT_MS` against `IN_MS`), so a step down may move it that much
/// further in proportion.
fn follows(ja: &Ja) -> Result<String, String> {
    let release = AIM_FROM + 2 * u64::from(IN_MS);
    let end = release + 2 * u64::from(OUT_MS);
    let points = run(
        ja,
        (Wield::Pistol, true, end),
        standing(ja, Wield::Pistol, release),
        |figure, now| hand(ja, figure, now, 0).map(|point| (now, point)),
    )?
    .into_iter()
    .collect::<Option<Vec<(u64, Vec3)>>>()
    .ok_or("no right hand bolt")?;
    // The hand from the step before `from` to `to`.
    let span = |from: u64, to: u64| -> Vec<Vec3> {
        (points.iter())
            .filter(|&&(now, _)| now + STEP_MS >= from && now <= to)
            .map(|&(_, point)| point)
            .collect()
    };
    let (rise, lowering) = (
        span(AIM_FROM, AIM_FROM + u64::from(IN_MS)),
        span(release, end),
    );
    let (Some(&start), Some(&top), Some(&rest)) = (rise.first(), rise.last(), lowering.last())
    else {
        return Err("no steps measured".into());
    };
    let full = full_raise(ja, Wield::Pistol)?;
    let (travel, ended, back) = (
        start.distance(full),
        top.distance(full),
        rest.distance(start),
    );
    let (up, down) = (largest_step(&rise), largest_step(&lowering));
    let falls = (rise.windows(2)).filter(|w| w[1].z < w[0].z - 1e-4).count();
    let share = |step: f32| 100.0 * step / travel.max(1e-6);
    check(
        travel > 0.05
            && falls == 0
            && ended <= END_NEAR
            && back <= DOWN_NEAR
            && up <= travel * STEP_MOST
            && down <= travel * STEP_MOST * IN_MS as f32 / OUT_MS as f32,
        format!(
            "the hand travels {travel:.3} m, rising {:.3} m; {falls} steps lower it; it ends {ended:.3} m from the full raise and, let down, {back:.3} m from where it began; the largest step moves it {up:.3} m up ({:.0}% of the way) and {down:.3} m down ({:.0}%)",
            full.z - start.z,
            share(up),
            share(down)
        ),
    )
}

/// At full raise, each wield's gun hand (both of a pair) stands within
/// `EYE_NEAR` of the eyes' height.
fn wields(ja: &Ja) -> Result<String, String> {
    let full = AIM_FROM + 2 * u64::from(IN_MS);
    let mut rows = Vec::new();
    let mut wrong = Vec::new();
    for wield in [Wield::Pistol, Wield::Pair, Wield::Rifle] {
        let hands: &[usize] = if wield == Wield::Pair { &[0, 1] } else { &[0] };
        let measured = run(
            ja,
            (wield, true, full),
            standing(ja, wield, NEVER),
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
                shooting: if aim {
                    aimed(now, NEVER)
                } else {
                    Shooting::default()
                },
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

/// The first step at or after `ms`.
fn step_at(ms: u64) -> u64 {
    ms.div_ceil(STEP_MS) * STEP_MS
}

/// `part` set at `at_ms` blending from what `old` shows then, as the sim
/// sets a torso's animation (`rules::sabers::blades::BLEND_MS`).
fn blended(mut part: Shown, old: &Shown, at_ms: u64) -> Shown {
    let shown = old.playback.frames(at_ms as i64);
    part.playback.blend = Some(Blend {
        start_ms: at_ms as i64,
        length_ms: rules::sabers::blades::BLEND_MS,
        frame: shown.current as f32 + shown.lerp,
        next: shown.next,
        under: None,
    });
    part
}

/// Another character (no seat acts through it) with a pistol, its torso
/// as `torso` says and its shooting as `shooting` says, from its ready
/// pose until `from`. Its gun hand must be where the full raise holds it
/// at `held_at` and back where the ready pose holds it at `down_at`, and
/// no step from `from` on may move the hand more than `STEP_MOST` of the
/// way between the two.
fn other_raises(
    ja: &Ja,
    torso: impl Fn(u64) -> Result<Shown, String>,
    shooting: impl Fn(u64) -> Shooting,
    (from, held_at, down_at): (u64, u64, u64),
) -> Result<String, String> {
    let step = |now: u64| -> Result<Step, String> {
        Ok(Step {
            legs: ja.part(Anim::BOTH_STAND1, 0, true)?,
            torso: torso(now)?,
            shooting: shooting(now),
        })
    };
    let points = run(ja, (Wield::Pistol, false, down_at), step, |figure, now| {
        hand(ja, figure, now, 0).map(|point| (now, point))
    })?
    .into_iter()
    .collect::<Option<Vec<(u64, Vec3)>>>()
    .ok_or("no right hand bolt")?;
    let at = |ms: u64| {
        (points.iter())
            .find(|&&(now, _)| now == ms)
            .map(|&(_, point)| point)
    };
    let (Some(ready), Some(held), Some(down)) = (at(from - STEP_MS), at(held_at), at(down_at))
    else {
        return Err("a mark fell between steps".into());
    };
    let moving: Vec<Vec3> = (points.iter())
        .filter(|&&(now, _)| now + STEP_MS >= from)
        .map(|&(_, point)| point)
        .collect();
    let full = full_raise(ja, Wield::Pistol)?;
    let (up, back) = (held.distance(full), down.distance(ready));
    let (travel, most) = (full.distance(ready), largest_step(&moving));
    check(
        up <= END_NEAR && back <= DOWN_NEAR && most <= travel * STEP_MOST,
        format!(
            "{} ms in the hand is {up:.3} m from the full raise; at {} ms it is {back:.3} m from the ready pose; the largest step moves it {most:.3} m ({:.0}% of the {travel:.3} m way)",
            held_at - from,
            down_at - from,
            100.0 * most / travel.max(1e-6)
        ),
    )
}

/// Another character fires a pistol at `SHOT_AT`: 500 ms later its gun
/// hand is where the full raise holds it, and once `OTHERS_HOLD_MS` and
/// the let-down's blend (`cast::RAISE_MS`) have passed it is back where
/// the ready pose holds it. The first shot raises the gun with no jump.
fn others_raise(ja: &Ja) -> Result<String, String> {
    let shot_ms = ja.length_ms(Anim::BOTH_ATTACK2);
    let ready = ja.part(Anim::TORSO_WEAPONREADY2, 0, true)?;
    let shot = blended(
        ja.part(Anim::BOTH_ATTACK2, SHOT_AT, false)?,
        &ready,
        SHOT_AT,
    );
    let after = blended(
        ja.part(Anim::TORSO_WEAPONREADY2, SHOT_AT + shot_ms, true)?,
        &shot,
        SHOT_AT + shot_ms,
    );
    let torso = move |now: u64| {
        Ok(if now < SHOT_AT {
            ready
        } else if now < SHOT_AT + shot_ms {
            shot
        } else {
            after
        })
    };
    let held_at = step_at(SHOT_AT + 500);
    let down_at =
        step_at(SHOT_AT + aim_pose::OTHERS_HOLD_MS as u64 + u64::from(RAISE_MS) + STEP_MS);
    other_raises(
        ja,
        torso,
        |_| Shooting::default(),
        (SHOT_AT, held_at, down_at),
    )
}

/// Another character charges a Power Blast (or steadies a Sniper Shot)
/// from `SHOT_AT` for `CHARGE_MS` and lets go with no shot: its gun is up
/// once a raise's blend (`cast::RAISE_MS`) has passed, and down again a
/// blend after the charge ends.
fn others_charge(ja: &Ja) -> Result<String, String> {
    const CHARGE_MS: u64 = 500;
    let ready = ja.part(Anim::TORSO_WEAPONREADY2, 0, true)?;
    let charging = |now: u64| Shooting {
        charging_from: (SHOT_AT..SHOT_AT + CHARGE_MS)
            .contains(&now)
            .then_some(SHOT_AT),
        ..Shooting::default()
    };
    let blend = u64::from(RAISE_MS) + STEP_MS;
    other_raises(
        ja,
        move |_| Ok(ready),
        charging,
        (
            SHOT_AT,
            step_at(SHOT_AT + blend),
            step_at(SHOT_AT + CHARGE_MS + blend),
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
