//! The aim pose's probes (`kjk sim probes --only "aim pose"`): on JA's own
//! skeleton and animations, a body raising its gun to aim follows the
//! sim's raise without a jump, holds each wield's gun hand 15 to 30 cm
//! below its eyes, walks on with its legs untouched, and another character holds its
//! gun up after a shot for `aim_pose::OTHERS_HOLD_MS` and while it charges
//! a Power Blast, raising it with no jump either way. Presses and releases
//! during a pistol's or a rifle's shot move the gun hand with no jump, and
//! the fade hides no jump of a torso bone larger than 45°. A Sniper Shot's
//! sight shows while it is steadied, not while the gun is only aimed. The
//! camera's checks while aiming (`play_view::aim_camera_probes`) run in this
//! group too. The firing motion's checks on a posed body
//! (`recoil::probes`) pose their bodies with `Ja`, `run` and `standing`.

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
            ("raise_during_shot", raise_during_shot(&ja)),
        ],
        Err(why) => [
            "aim_pose_follows",
            "aim_pose_wields",
            "aim_pose_walks",
            "others_raise",
            "others_charge",
            "raise_during_shot",
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
pub(crate) const AIM_FROM: u64 = 1000;
/// How near (metres) the hand ends to where the full raise holds it.
const END_NEAR: f32 = 0.02;
/// The largest share of the whole raise one step may move the hand.
const STEP_MOST: f32 = 0.1;
/// The largest share of the whole way one step may move the hand while
/// the sim lowers the gun. The sim lowers it faster than it raises it
/// (`OUT_MS` against `IN_MS`), so a step may move it that much further.
pub(crate) const LOWER_MOST: f32 = STEP_MOST * IN_MS as f32 / OUT_MS as f32;
/// How far (metres) below the eyes a raised gun's hand stands, at least and
/// at most. A pistol's sights are then level with the cheek, and a rifle's
/// stock is at the shoulder.
const BELOW_EYES: (f32, f32) = (0.15, 0.30);
/// How far (metres) aiming may move a walking body's feet.
const FEET_NEAR: f32 = 0.001;
/// When another character fires (sim ms).
const SHOT_AT: u64 = 1000;
/// How near (metres) a gun let down comes back to the ready pose.
const DOWN_NEAR: f32 = 0.01;

/// JA's player skeleton, its animations and its hand and eye bolts.
pub(crate) struct Ja {
    pub(crate) rig: Rig,
    eyes: Option<asset_ja::Bolt>,
    body: EntityState,
}

impl Ja {
    pub(crate) fn load() -> Result<Ja, String> {
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
    pub(crate) fn part(&self, anim: Anim, start_ms: u64, looping: bool) -> Result<Shown, String> {
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
pub(crate) struct Step {
    pub(crate) legs: Shown,
    pub(crate) torso: Shown,
    pub(crate) shooting: Shooting,
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
pub(crate) fn run<T>(
    ja: &Ja,
    (wield, seat, steps): (Wield, bool, std::ops::RangeInclusive<u64>),
    step: impl Fn(u64) -> Result<Step, String>,
    mut measure: impl FnMut(&Figure, u64) -> T,
) -> Result<Vec<T>, String> {
    let (ready, _, number) = gun(wield);
    let mut figure = Figure::new(&ja.rig, 0.0, ready.name());
    figure.hold_gun(number);
    let mut out = Vec::new();
    for now in steps.step_by(STEP_MS as usize) {
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
pub(crate) const NEVER: u64 = u64::MAX;

/// The sim's raise at `now` after [Aim] was pressed at `from` and let go
/// of at `release`.
pub(crate) fn aimed(now: u64, from: u64, release: u64) -> Shooting {
    let mut shooting = Shooting::default();
    for at in (from..now).step_by(STEP_MS as usize) {
        shooting.aim_step(at < release, STEP_MS as u32);
    }
    shooting
}

/// Standing ready with `wield`'s gun, aiming from `AIM_FROM` until
/// `release`.
pub(crate) fn standing(
    ja: &Ja,
    wield: Wield,
    release: u64,
) -> impl Fn(u64) -> Result<Step, String> + '_ {
    let (ready, ..) = gun(wield);
    move |now| {
        Ok(Step {
            legs: ja.part(Anim::BOTH_STAND1, 0, true)?,
            torso: ja.part(ready, 0, true)?,
            shooting: aimed(now, AIM_FROM, release),
        })
    }
}

/// Where `wield`'s right hand holds its gun at full raise: a body that is
/// fully aimed from its first step, so that no raise is stepped through.
fn full_raise(ja: &Ja, wield: Wield) -> Result<Vec3, String> {
    let (ready, ..) = gun(wield);
    let full = aimed(AIM_FROM + 2 * u64::from(IN_MS), AIM_FROM, NEVER);
    let held = move |_| {
        Ok(Step {
            legs: ja.part(Anim::BOTH_STAND1, 0, true)?,
            torso: ja.part(ready, 0, true)?,
            shooting: full,
        })
    };
    run(ja, (wield, true, 0..=AIM_FROM), held, |figure, now| {
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
/// way up moves it more than `STEP_MOST` of the whole way, and no step on
/// the way down more than `LOWER_MOST`.
fn follows(ja: &Ja) -> Result<String, String> {
    let release = AIM_FROM + 2 * u64::from(IN_MS);
    let end = release + 2 * u64::from(OUT_MS);
    let points = run(
        ja,
        (Wield::Pistol, true, 0..=end),
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
            && down <= travel * LOWER_MOST,
        format!(
            "the hand travels {travel:.3} m, rising {:.3} m; {falls} steps lower it; it ends {ended:.3} m from the full raise and, let down, {back:.3} m from where it began; the largest step moves it {up:.3} m up ({:.0}% of the way) and {down:.3} m down ({:.0}%)",
            full.z - start.z,
            share(up),
            share(down)
        ),
    )
}

/// At full raise, each wield's gun hand (both of a pair) stands
/// `BELOW_EYES` under the eyes.
fn wields(ja: &Ja) -> Result<String, String> {
    let full = AIM_FROM + 2 * u64::from(IN_MS);
    let mut rows = Vec::new();
    let mut wrong = Vec::new();
    for wield in [Wield::Pistol, Wield::Pair, Wield::Rifle] {
        let hands: &[usize] = if wield == Wield::Pair { &[0, 1] } else { &[0] };
        let measured = run(
            ja,
            (wield, true, 0..=full),
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
            if !(BELOW_EYES.0..=BELOW_EYES.1).contains(&-off) {
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
                    aimed(now, AIM_FROM, NEVER)
                } else {
                    Shooting::default()
                },
            })
        }
    };
    let feet = |aim: bool| {
        run(
            ja,
            (Wield::Pistol, true, 0..=end),
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
    torso: impl Fn(u64) -> Shown,
    shooting: impl Fn(u64) -> Shooting,
    (from, held_at, down_at): (u64, u64, u64),
) -> Result<String, String> {
    let points = hand_path(ja, (false, 0..=down_at), torso, shooting)?;
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
    let held_at = step_at(SHOT_AT + 500);
    let down_at =
        step_at(SHOT_AT + aim_pose::OTHERS_HOLD_MS as u64 + u64::from(RAISE_MS) + STEP_MS);
    other_raises(
        ja,
        firing(ja, Wield::Pistol, &[SHOT_AT])?,
        |_| Shooting::default(),
        (SHOT_AT, held_at, down_at),
    )
}

/// Where a pistol's right hand is at each step to `end` (`seat`: a seat
/// acts through the body), its torso as `torso` says and its shooting as
/// `shooting` says.
fn hand_path(
    ja: &Ja,
    (seat, steps): (bool, std::ops::RangeInclusive<u64>),
    torso: impl Fn(u64) -> Shown,
    shooting: impl Fn(u64) -> Shooting,
) -> Result<Vec<(u64, Vec3)>, String> {
    Ok(gun_path(ja, (Wield::Pistol, seat, steps), torso, shooting)?.0)
}

/// The gun hand's place at each step (sim ms, metres).
type HandPath = Vec<(u64, Vec3)>;

/// Where `wield`'s right hand is at each step, as `hand_path` gives it for
/// a pistol, and the largest jump of a torso bone that died away over the
/// run, with that bone (`Figure::faded_jump`, radians).
fn gun_path(
    ja: &Ja,
    (wield, seat, steps): (Wield, bool, std::ops::RangeInclusive<u64>),
    torso: impl Fn(u64) -> Shown,
    shooting: impl Fn(u64) -> Shooting,
) -> Result<(HandPath, (f32, usize)), String> {
    let step = |now: u64| -> Result<Step, String> {
        Ok(Step {
            legs: ja.part(Anim::BOTH_STAND1, 0, true)?,
            torso: torso(now),
            shooting: shooting(now),
        })
    };
    let measured = run(ja, (wield, seat, steps), step, |figure, now| {
        let point = hand(ja, figure, now, 0).map(|point| (now, point));
        (point, figure.faded_jump())
    })?;
    let jump = (measured.iter()).fold(
        (0.0, 0),
        |most: (f32, usize), &(_, jump)| {
            if jump.0 > most.0 { jump } else { most }
        },
    );
    let points = (measured.into_iter())
        .map(|(point, _)| point)
        .collect::<Option<Vec<(u64, Vec3)>>>()
        .ok_or("no right hand bolt")?;
    Ok((points, jump))
}

/// `wield`'s torso that stands ready and fires at each of `shots` (sim ms,
/// in order), each part blending from the one before as the sim sets it.
pub(crate) fn firing(
    ja: &Ja,
    wield: Wield,
    shots: &[u64],
) -> Result<impl Fn(u64) -> Shown + use<>, String> {
    let (ready, shot_anim, _) = gun(wield);
    let shot_ms = ja.length_ms(shot_anim);
    let mut parts = vec![(0, ja.part(ready, 0, true)?)];
    for &at in shots {
        let (_, before) = parts[parts.len() - 1];
        let shot = blended(ja.part(shot_anim, at, false)?, &before, at);
        let after = ja.part(ready, at + shot_ms, true)?;
        parts.push((at, shot));
        parts.push((at + shot_ms, blended(after, &shot, at + shot_ms)));
    }
    Ok(move |now: u64| {
        (parts.iter().rev())
            .find(|&&(from, _)| from <= now)
            .map_or(parts[0].1, |&(_, part)| part)
    })
}

/// When the player's gun goes up early enough to be fully raised by
/// `SHOT_AT` (sim ms).
const RAISED_FROM: u64 = SHOT_AT - 400;

/// Something the player does to the gun.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Switch {
    /// The player presses [Aim].
    AimPressed,
    /// The player lets go of [Aim].
    AimLetGo,
    /// The player begins a Power Blast's charge.
    ChargeBegun,
    /// The player lets go of the charge without a shot.
    ChargeLetGo,
    /// The sim drops the aim at once and keeps the charge, as
    /// `Shooting::lower` does while a grenade is readied. The sim lowers
    /// the aim again at every step after that, so the probe counts [Aim]
    /// as let go from then on.
    AimDropped,
}

impl Switch {
    const ALL: [Switch; 5] = [
        Switch::AimPressed,
        Switch::AimLetGo,
        Switch::ChargeBegun,
        Switch::ChargeLetGo,
        Switch::AimDropped,
    ];

    fn name(self) -> &'static str {
        match self {
            Switch::AimPressed => "aim pressed",
            Switch::AimLetGo => "aim let go",
            Switch::ChargeBegun => "charge begun",
            Switch::ChargeLetGo => "charge let go",
            Switch::AimDropped => "aim dropped",
        }
    }

    /// Whether [Aim] and the charge must be held before it (None: either).
    fn needs(self) -> [Option<bool>; 2] {
        match self {
            Switch::AimPressed => [Some(false), None],
            Switch::AimLetGo => [Some(true), None],
            Switch::ChargeBegun => [None, Some(false)],
            Switch::ChargeLetGo => [None, Some(true)],
            Switch::AimDropped => [Some(true), Some(true)],
        }
    }

    /// Whether [Aim] and the charge are held after it (None: unchanged).
    fn leaves(self) -> [Option<bool>; 2] {
        match self {
            Switch::AimPressed => [Some(true), None],
            Switch::AimLetGo | Switch::AimDropped => [Some(false), None],
            Switch::ChargeBegun => [None, Some(true)],
            Switch::ChargeLetGo => [None, Some(false)],
        }
    }
}

/// Whether [Aim] and the charge are held from `RAISED_FROM` so that each of
/// `switches`, in order, finds what it needs. None if one of them cannot.
fn held_before(switches: &[Switch]) -> Option<[bool; 2]> {
    let mut first = [None; 2];
    let mut held: [Option<bool>; 2] = [None; 2];
    for switch in switches {
        for (i, (need, left)) in switch.needs().into_iter().zip(switch.leaves()).enumerate() {
            match (need, held[i]) {
                (Some(need), Some(was)) if need != was => return None,
                (Some(need), None) => (first[i], held[i]) = (Some(need), Some(need)),
                _ => {}
            }
            held[i] = left.or(held[i]);
        }
    }
    Some(first.map(|held| held.unwrap_or(false)))
}

/// The player's shooting at `now`: [Aim] and the charge held from
/// `RAISED_FROM` as `held` says, then each of `switches` at its moment
/// (sim ms, on a step).
fn switched(now: u64, held: [bool; 2], switches: &[(u64, Switch)]) -> Shooting {
    let mut shooting = Shooting::default();
    let [mut aim, charged] = held;
    let mut charge = charged.then_some(RAISED_FROM);
    for at in (RAISED_FROM..=now).step_by(STEP_MS as usize) {
        for &(_, switch) in switches.iter().filter(|&&(when, _)| when == at) {
            match switch {
                Switch::AimPressed => aim = true,
                Switch::AimLetGo => aim = false,
                Switch::ChargeBegun => charge = Some(at),
                Switch::ChargeLetGo => charge = None,
                Switch::AimDropped => {
                    aim = false;
                    shooting.lower();
                }
            }
        }
        if at < now {
            shooting.aim_step(aim, STEP_MS as u32);
        }
    }
    shooting.charging_from = charge.filter(|_| now >= RAISED_FROM);
    shooting
}

/// How long after each switch the next comes (ms).
const GAPS: [u64; 4] = [40, 120, 240, 400];
/// The most switches in one run.
const LONGEST: usize = 3;

/// The switches of a run, as the probe names them.
fn named(switches: &[Switch]) -> String {
    let names: Vec<&str> = switches.iter().map(|switch| switch.name()).collect();
    names.join(", then ")
}

/// The largest share of the way one step may move the gun hand in a run
/// of `switches`: more while the sim lowers the gun as [Aim] is let go of
/// (`LOWER_MOST`).
fn limit_of(switches: &[Switch]) -> f32 {
    if switches.contains(&Switch::AimLetGo) {
        LOWER_MOST
    } else {
        STEP_MOST
    }
}

/// Runs `job` on each of `jobs` on every core, the results in order.
fn on_every_core<J: Sync, T: Send>(jobs: &[J], job: impl Fn(&J) -> T + Sync) -> Vec<T> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let threads = std::thread::available_parallelism().map_or(1, |count| count.get());
    let taken = AtomicUsize::new(0);
    let mut done: Vec<(usize, T)> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut out = Vec::new();
                    loop {
                        let index = taken.fetch_add(1, Ordering::Relaxed);
                        let Some(one) = jobs.get(index) else {
                            return out;
                        };
                        out.push((index, job(one)));
                    }
                })
            })
            .collect();
        (workers.into_iter())
            .flat_map(|worker| {
                worker
                    .join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
            })
            .collect()
    });
    done.sort_by_key(|&(index, _)| index);
    done.into_iter().map(|(_, out)| out).collect()
}

/// The gun never jumps when the guns go up or down around a shot. The
/// player fires from the hip at `SHOT_AT`, with a pistol and with a rifle.
/// In each run the player does one to `LONGEST` of the `Switch`es, in every
/// order in which they can follow each other. With the pistol the first
/// happens every 40 ms from 200 ms before the shot until the sim's blend
/// out of the shot has ended, and on the shot's last two steps, and each
/// later one comes one of the `GAPS` after the one before. The rifle is
/// sampled more coarsely (`SWEEPS`). What the first ones need is held from
/// `RAISED_FROM`, early enough for the gun to be fully up by the shot:
/// [Aim] for letting go of it, the charge for letting go of it.
///
/// Another character also fires a pistol again while its guns are being let
/// down after its last shot. In every run no step from 200 ms before the
/// shot on may move the gun hand more than the run's share of the way from
/// the ready pose to the full raise: `STEP_MOST`, or `LOWER_MOST` while the
/// sim lowers the gun as [Aim] is let go of. The check names each single
/// switch's worst run, and for two and for three switches the worst run of
/// all their orders. It also names the largest jump of a torso bone that
/// the fade hid over all the runs, which may be no more than `FADED_MOST`,
/// so that a wrong starting pose cannot pass as a slide.
fn raise_during_shot(ja: &Ja) -> Result<String, String> {
    let orders = switch_orders();
    let (mut rows, mut wrong, mut counted) = (Vec::new(), Vec::new(), Vec::new());
    let mut faded: Vec<Jump> = Vec::new();
    let mut pistol_travel = None;
    for sampled in SWEEPS {
        let name = wield_name(sampled.0);
        let swept = sweep(ja, sampled, &orders)?;
        rows.extend(
            swept
                .rows(&orders)
                .into_iter()
                .map(|row| format!("{name} {row}")),
        );
        wrong.extend(swept.wrong().into_iter().map(|row| format!("{name} {row}")));
        counted.push(format!(
            "{} {name} runs, the first switch at {} moments across a {} ms shot",
            swept.runs, swept.moments, swept.shot_ms
        ));
        faded.push(Jump {
            run: format!("the {name} run {}", swept.faded.run),
            ..swept.faded
        });
        if sampled.0 == Wield::Pistol {
            pistol_travel = Some(swept.travel);
        }
    }
    let travel = pistol_travel.ok_or("no pistol sweep")?;
    let lowering = step_at(SHOT_AT + aim_pose::OTHERS_HOLD_MS as u64 + u64::from(RAISE_MS) / 2);
    let (refired, (angle, bone)) = gun_path(
        ja,
        (Wield::Pistol, false, 0..=SHOT_AT + 2000),
        firing(ja, Wield::Pistol, &[SHOT_AT, lowering])?,
        |_| Shooting::default(),
    )?;
    faded.push(Jump {
        angle,
        bone,
        run: "the pistol run where another character fires while lowering".into(),
    });
    let others = Swept::row(
        &(
            "another character fires while lowering".to_string(),
            (most_from(&refired), lowering as i64 - SHOT_AT as i64, 0),
            STEP_MOST,
        ),
        travel,
    );
    rows.push(format!("pistol {others}"));
    if most_from(&refired) > travel * STEP_MOST {
        wrong.push("another character fires while lowering".into());
    }
    // The largest jump of a torso bone that the fade hid, in each wield's
    // sweep and in the other character's run.
    for jump in &faded {
        let bone = (ja.rig.gla.bones.get(jump.bone))
            .filter(|_| jump.angle > 0.0)
            .map_or("no bone", |bone| bone.name.as_str());
        rows.push(format!(
            "the largest faded jump in {}: {:.1}° of {bone} (at most {:.1}°)",
            jump.run,
            jump.angle.to_degrees(),
            FADED_MOST.to_degrees()
        ));
        if jump.angle > FADED_MOST {
            wrong.push(format!("a faded jump in {}", jump.run));
        }
    }
    check(
        wrong.is_empty(),
        format!(
            "{} of {} orders of switches; the largest step of the gun hand: {}; wrong: {wrong:?}",
            counted.join(", and "),
            orders.len(),
            rows.join("; ")
        ),
    )
}

/// How each wield's sweep is sampled (`raise_during_shot`): the wield, how
/// far apart (ms) the moments of the first switch are, and the gaps between
/// later switches. The rifle is sampled more coarsely than the pistol, to
/// keep the group's time down.
const SWEEPS: [(Wield, u64, &[u64]); 2] =
    [(Wield::Pistol, 40, &GAPS), (Wield::Rifle, 120, &[40, 240])];

/// The largest jump (radians) of one torso bone that the fade may hide
/// (`Figure::faded_jump`) when the guns' state changes: 45°. A larger jump
/// would be a wrong starting pose shown as a slide over `cast::RAISE_MS`.
/// When the limit was set, the largest jump seen was 41.5° of the right
/// upper arm. It came in a pistol run where [Aim] is pressed 80 ms before
/// the shot, a charge is begun 40 ms later, and [Aim] is let go as the
/// shot begins.
const FADED_MOST: f32 = std::f32::consts::FRAC_PI_4;

/// When the watch for a jump of the gun hand begins (sim ms): 200 ms before
/// the shot.
const WATCH_FROM: u64 = SHOT_AT - 200;

/// The largest step of the gun hand among `points` from `WATCH_FROM` on.
fn most_from(points: &[(u64, Vec3)]) -> f32 {
    let moving: Vec<Vec3> = (points.iter())
        .filter(|&&(now, _)| now >= WATCH_FROM)
        .map(|&(_, point)| point)
        .collect();
    largest_step(&moving)
}

/// The wield as the probe names it.
fn wield_name(wield: Wield) -> &'static str {
    match wield {
        Wield::Pistol => "pistol",
        Wield::Pair => "pair",
        Wield::Rifle => "rifle",
    }
}

/// One wield's sweep (`sweep`).
struct Swept {
    /// The worst run of each order of switches: its name, its largest step
    /// (m), when its first switch happened (ms from the shot) and the gap
    /// between its switches, and its limit as a share of `travel`.
    worst: Vec<(String, (f32, i64, u64), f32)>,
    /// The way (m) from the ready pose to the full raise.
    travel: f32,
    runs: usize,
    moments: usize,
    shot_ms: u64,
    /// The largest jump of a torso bone that faded away.
    faded: Jump,
}

/// A jump of a torso bone that faded away (`Figure::faded_jump`): its
/// angle (radians), the bone's number in the skeleton, and the run it came
/// in.
#[derive(Debug, Clone, Default)]
struct Jump {
    angle: f32,
    bone: usize,
    run: String,
}

impl Swept {
    fn row((name, (most, at, gap), _): &(String, (f32, i64, u64), f32), travel: f32) -> String {
        let then = if *gap > 0 {
            format!(", each next {gap} ms later")
        } else {
            String::new()
        };
        format!(
            "{name}: {most:.3} m ({:.0}% of {travel:.3} m) at {at:+} ms{then}",
            100.0 * most / travel.max(1e-6)
        )
    }

    /// Each single switch's worst run, then the worst order of each longer
    /// length (largest against its limit).
    fn rows(&self, orders: &[Vec<Switch>]) -> Vec<String> {
        let mut rows = Vec::new();
        for length in 1..=LONGEST {
            let of_length =
                (orders.iter().zip(&self.worst)).filter(|(switches, _)| switches.len() == length);
            if length == 1 {
                rows.extend(of_length.map(|(_, worst)| Swept::row(worst, self.travel)));
            } else if let Some((_, most)) =
                of_length.max_by(|(_, a), (_, b)| (a.1.0 / a.2).total_cmp(&(b.1.0 / b.2)))
            {
                rows.push(format!(
                    "worst of {length} switches: {}",
                    Swept::row(most, self.travel)
                ));
            }
        }
        rows
    }

    /// The orders whose worst run moved the gun hand more than their limit.
    fn wrong(&self) -> Vec<String> {
        (self.worst.iter())
            .filter(|(_, (most, ..), limit)| *most > self.travel * limit)
            .map(|(name, ..)| name.clone())
            .collect()
    }
}

/// One wield's runs of `orders` (`raise_during_shot`), sampled as `every`
/// and `gaps` say (`SWEEPS`).
fn sweep(
    ja: &Ja,
    (wield, every, gaps): (Wield, u64, &[u64]),
    orders: &[Vec<Switch>],
) -> Result<Swept, String> {
    let (_, shot_anim, _) = gun(wield);
    let shot_ms = ja.length_ms(shot_anim);
    // Every `every` ms from 200 ms before the shot until the sim's blend
    // out of it is over, and the shot's last two steps.
    let after = u64::from(rules::sabers::blades::BLEND_MS);
    let mut moments: Vec<u64> = (WATCH_FROM..=SHOT_AT + shot_ms + after)
        .step_by(every as usize)
        .collect();
    moments.extend([2, 1].map(|steps| step_at(SHOT_AT + shot_ms) - steps * STEP_MS));
    // One run for each order, moment of its first switch and gap between
    // its switches. Each starts with the body standing ready shortly before
    // anything is held, and ends once every raise and let-down is over.
    let runs: Vec<(usize, u64, u64)> = (orders.iter().enumerate())
        .flat_map(|(index, switches)| {
            let gaps: &[u64] = if switches.len() > 1 { gaps } else { &[0] };
            (moments.iter()).flat_map(move |&at| gaps.iter().map(move |&gap| (index, at, gap)))
        })
        .collect();
    let steps = on_every_core(&runs, |&(index, at, gap)| {
        let switches = &orders[index];
        let held = held_before(switches).unwrap_or([false; 2]);
        let timed: Vec<(u64, Switch)> = (switches.iter().enumerate())
            .map(|(nth, &switch)| (at + nth as u64 * gap, switch))
            .collect();
        let last = timed.last().map_or(at, |&(when, _)| when);
        let run = RAISED_FROM - 40..=(SHOT_AT + shot_ms + 600).max(last + 400);
        let (points, faded) = gun_path(
            ja,
            (wield, true, run),
            firing(ja, wield, &[SHOT_AT])?,
            |now| switched(now, held, &timed),
        )?;
        Ok::<_, String>((most_from(&points), faded))
    });
    // The worst run of each order: its largest step, when its first switch
    // happened and the gap between its switches.
    let mut worst: Vec<(String, (f32, i64, u64), f32)> = (orders.iter())
        .map(|switches| (named(switches), (0.0, 0, 0), limit_of(switches)))
        .collect();
    let mut faded = Jump::default();
    for (&(index, at, gap), step) in runs.iter().zip(steps) {
        let (step, (angle, bone)) = step?;
        if angle > faded.angle {
            let run = format!(
                "\"{}\" with the first switch at {:+} ms and each next {gap} ms later",
                named(&orders[index]),
                at as i64 - SHOT_AT as i64
            );
            faded = Jump { angle, bone, run };
        }
        let most = &mut worst[index].1;
        if step > most.0 {
            *most = (step, at as i64 - SHOT_AT as i64, gap);
        }
    }
    // The ready pose: the hand on the step before the shot, nothing held.
    let (before, _) = gun_path(
        ja,
        (wield, true, 0..=SHOT_AT - STEP_MS),
        firing(ja, wield, &[])?,
        |_| Shooting::default(),
    )?;
    let ready = before
        .last()
        .map(|&(_, point)| point)
        .ok_or("no step before the shot")?;
    Ok(Swept {
        worst,
        travel: full_raise(ja, wield)?.distance(ready),
        runs: runs.len(),
        moments: moments.len(),
        shot_ms,
        faded,
    })
}

/// Every order of one to `LONGEST` switches that can follow each other.
fn switch_orders() -> Vec<Vec<Switch>> {
    let mut orders: Vec<Vec<Switch>> = Vec::new();
    let mut longer: Vec<Vec<Switch>> = vec![Vec::new()];
    for _ in 0..LONGEST {
        longer = (longer.iter())
            .flat_map(|before| {
                Switch::ALL
                    .into_iter()
                    .map(|next| [before.as_slice(), &[next]].concat())
            })
            .filter(|switches| held_before(switches).is_some())
            .collect();
        orders.extend(longer.iter().cloned());
    }
    orders
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
        move |_| ready,
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
