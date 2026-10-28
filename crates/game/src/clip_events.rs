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
    ]
}
