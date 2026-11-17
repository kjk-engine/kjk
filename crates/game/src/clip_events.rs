//! What a KOTOR clip's own events (its MDL's) bring as a body plays it: a
//! script's animation or a round's swing and its answer (`acting`), as the
//! K1 executable answers each event it hooks on a creature's model:
//!
//! - `SwingShort`, `SwingLong`, `SwingTwirl`: the body's weapon row's
//!   `swingshort0–2`, `swinglong0–2`, `swingtwirl0–2` (`weaponsounds.2da`),
//!   one of three at random;
//! - `HitParry`: its `parry0`/`parry1`; `Clash`: its `clash0`/`clash1`;
//! - `Contact`: `visualeffects.2da` row 4011 (`VFX_COM_SPARKS_PARRY_METAL`,
//!   `v_parrspr_imp`) where the body's `LightSaberHook` node stands, left
//!   there (`script_fx::HookVisuals`).
//!
//! The row is the body's weapon in hand's `weaponmattype` (a lightsaber's
//! row 1, fists' 0: `WeaponSounds::in_hand`), each sound at the body,
//! following it. The executable's `hit` (the hit's sound, the struck's
//! pain, the blood) is the sim's landed blow here (`combat_fx`, `grunts`);
//! `snd_Footstep` a footstep (`ambience::ClipSteps`), where the sim's walk
//! steps nothing.

use bevy::prelude::*;
use sim::SimId;

use crate::sound::Cue;
use crate::weapon_sounds::{Swing, WeaponSounds};

/// The effect a `Contact` puts at the body's saber hook, and the hook.
pub const CONTACT_SPARKS: u16 = 4011;
pub const SPARKS_HOOK: &str = "lightsaberhook";

/// An event of a clip that brings something.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Heard {
    Swing(Swing),
    Parry,
    Clash,
    Contact,
    Footstep,
}

impl Heard {
    /// The event named `name` (without case), if it brings anything here.
    pub fn of(name: &str) -> Option<Heard> {
        let lower = name.to_ascii_lowercase();
        Some(match lower.as_str() {
            "swingshort" => Heard::Swing(Swing::Short),
            "swinglong" => Heard::Swing(Swing::Long),
            "swingtwirl" => Heard::Swing(Swing::Twirl),
            "hitparry" => Heard::Parry,
            "clash" => Heard::Clash,
            "contact" => Heard::Contact,
            "snd_footstep" => Heard::Footstep,
            _ => return None,
        })
    }
}

/// The sound `heard` of `entity`'s at `at` (units), the view's `ms`, of
/// weapon row `row`; none for a `Contact` or a `Footstep`, or a row
/// without it.
pub fn cue(
    heard: Heard,
    (entity, at, ms): (SimId, Vec3, f32),
    (kotor, row): (&WeaponSounds, usize),
) -> Option<Cue> {
    let mut rng = crate::view_rng::ja_seed(entity.0, ms.to_bits());
    let name = match heard {
        Heard::Swing(swing) => kotor.swing(row, swing, &mut rng),
        Heard::Parry => kotor.parry(row, &mut rng),
        Heard::Clash => kotor.clash(row, &mut rng),
        Heard::Contact | Heard::Footstep => None,
    }?;
    Some(Cue {
        entity: Some(entity),
        channel: audio::Channel::Weapon,
        follows: true,
        ..Cue::at(name, at, ms)
    })
}

/// The rules' probes (`kjk sim probes --only "clip events"`).
#[cfg(feature = "probes")]
pub fn probes() -> sim::probe::Checks {
    use asset_odyssey::anim::AnimationIr;
    use sim::probe::check;
    let clip = |events: &[(f32, &str)]| AnimationIr {
        name: "c2p1".into(),
        length: 1.5,
        transition: 0.25,
        root: None,
        tracks: Vec::new(),
        settings: Vec::new(),
        events: events
            .iter()
            .map(|&(at, name)| (at, name.to_string()))
            .collect(),
    };
    let named = |events: Vec<&str>| events.join(",");
    let c2p1 = clip(&[
        (0.27, "swingshort"),
        (0.3, "contact"),
        (0.3, "hitparry"),
        (1.2, "Clash"),
    ]);
    vec![
        (
            "events named as the executable hooks them",
            check(
                [
                    ("SwingShort", Some(Heard::Swing(Swing::Short))),
                    ("swinglong", Some(Heard::Swing(Swing::Long))),
                    ("SwingTwirl", Some(Heard::Swing(Swing::Twirl))),
                    ("HitParry", Some(Heard::Parry)),
                    ("clash", Some(Heard::Clash)),
                    ("Contact", Some(Heard::Contact)),
                    ("snd_Footstep", Some(Heard::Footstep)),
                    ("hit", None),
                    ("detonate", None),
                ]
                .iter()
                .all(|(name, want)| Heard::of(name) == *want),
                "swings, parry, clash, contact, footsteps; the hit elsewhere".into(),
            ),
        ),
        ("events passed in a frame, from its start", {
            let early = named(crate::model_fx::passed(&c2p1, 0.0, 0.3).collect());
            let both = named(crate::model_fx::passed(&c2p1, 0.28, 0.31).collect());
            check(
                early == "swingshort" && both == "contact,hitparry",
                format!("[0, 0.3): {early}; [0.28, 0.31): {both}"),
            )
        }),
        ("a clip resumed mid-way cues only what comes after", {
            use crate::acting::heard_from;
            let acting = |since| sim::combat::idle::Acting {
                animation: 10,
                since,
                ms: 1500,
            };
            let (loaded, begun) = (acting(4000), acting(5000));
            // A save loaded at 5.1 s, 1.1 s into a clip begun at 4.0 s.
            let resumed = heard_from(None, (loaded, 1.1), Some(5100.0));
            let fresh = heard_from(None, (begun, 0.05), Some(5000.0));
            let next = heard_from(Some((loaded, 1.1)), (loaded, 1.25), Some(5100.0));
            let on_load = named(crate::model_fx::passed(&c2p1, resumed, 1.1).collect());
            let after = named(crate::model_fx::passed(&c2p1, next, 1.25).collect());
            check(
                on_load.is_empty() && fresh == 0.0 && after == "Clash",
                format!(
                    "loaded 1.1 s in: from {resumed} ({on_load:?}), then {after:?}; \
                     begun since: from {fresh}"
                ),
            )
        }),
        ("a loop's wrap passes its end and its start", {
            let wrapped = named(crate::model_fx::passed(&c2p1, 1.1, 0.28).collect());
            check(
                wrapped == "swingshort,Clash",
                format!("[1.1, end] + [0, 0.28): {wrapped}"),
            )
        }),
        (
            "a loop's time that steps back plays nothing again (issue 415)",
            {
                use crate::model_fx::passed_since;
                // 0.5 s into the loop, the view's time goes back by 1 ms.
                let back = named(passed_since(&c2p1, (0.5, 0.499), true));
                // From 1.1 s on to 0.28 s into its second round.
                let wrapped = named(passed_since(&c2p1, (1.1, 1.78), true));
                // A step longer than the whole loop passes each event once.
                let whole = named(passed_since(&c2p1, (0.2, 1.9), true));
                check(
                    back.is_empty()
                        && wrapped == "swingshort,Clash"
                        && whole.split(',').count() == 4,
                    format!(
                        "back 1 ms: {back:?}; over the end: {wrapped:?}; a whole loop: {whole:?}"
                    ),
                )
            },
        ),
        (
            "a JA keyed sound plays once while its part flips on and off its frame",
            {
                use asset_ja::events::{Passage, Sounded};
                use fmt_ja::animation_cfg::Sequence;
                let sequence = |first_frame| Sequence {
                    name: "BOTH_WALK1".into(),
                    first_frame,
                    frame_count: 20,
                    loop_frame: 0,
                    frames_per_second: 20,
                };
                let (walk, other) = (sequence(100), sequence(500));
                // How often a sound keyed on frame 105 plays over the frames a
                // part steps through, each step in `walk` or not.
                let sounded = |steps: &[(i32, bool)]| {
                    let mut sounded = Sounded::default();
                    steps
                        .windows(2)
                        .filter(|step| {
                            let ((before, _), (now, same)) = (step[0], step[1]);
                            let passage = Passage {
                                before,
                                now,
                                same: same.then_some(&walk),
                            };
                            sounded.step(&passage);
                            sounded.sounds(&passage, 105)
                        })
                        .count()
                };
                // On the frame and one past it, 30 steps in turn.
                let flips: Vec<_> = (0..31).map(|n| (105 + n % 2, true)).collect();
                // Between the frame and another sequence's frame.
                let swaps: Vec<_> = (0..31)
                    .map(|n| {
                        if n % 2 == 0 {
                            (105, false)
                        } else {
                            (other.first_frame, false)
                        }
                    })
                    .collect();
                // Two rounds of the loop, a frame a step.
                let rounds: Vec<_> = (0..40).map(|n| (100 + n % 20, true)).collect();
                let counts = [sounded(&flips), sounded(&swaps), sounded(&rounds)];
                check(
                    counts == [1, 1, 2],
                    format!(
                        "flipping frames: {}, swapping sequences: {}, two loops: {}",
                        counts[0], counts[1], counts[2]
                    ),
                )
            },
        ),
        (
            "one character starts the same sound at most once in its gap",
            {
                let mut started = crate::sound::Started::default();
                // 30 footsteps asked for by one character in half a second.
                let burst = (0..30)
                    .filter(|n| started.first(7, "metal_step", *n as f32 * 1000.0 / 60.0))
                    .count();
                let other = started.first(8, "metal_step", 0.0);
                let unlike = started.first(7, "saberhup", 490.0);
                let later = started.first(7, "metal_step", 900.0);
                let gap = rules::engine::sounds::SAME_GAP_MS as usize;
                check(
                    (2..=500usize.div_ceil(gap)).contains(&burst) && other && unlike && later,
                    format!(
                        "30 asked in 500 ms: {burst} started; another character: {other}; \
                     another sound: {unlike}; the same at 900 ms: {later}"
                    ),
                )
            },
        ),
    ]
}
