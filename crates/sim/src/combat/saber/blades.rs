//! Where a character's blades are, each tick, as JA's server finds them
//! (`docs/COMBAT.md`): its own copy of the player's
//! skeleton follows the legs' and torso's animations (`Posed`, JA's
//! `G_UpdateClientAnims`: a new animation or a restart is set at the tick's
//! time with the server's blend, `BLEND_MS`, the torso at its saber style's
//! speed), is
//! posed with the server's look (`facing`, `saber::look`), placed at the
//! origin pushed ahead along the velocity (`skeleton::Placement::ahead`)
//! and turned to the legs' yaw,
//! and each blade of the weapon in hand runs from its hilt's `*blade<n>`
//! tag (`sim::skeleton`). The last place of each blade is kept (`Trail`) for
//! the next tick's sweep.
//!
//! Which blades (`slots`): a lightsaber is JA's `single_1` in the right
//! hand; two sabers the same again in the left (the second while it is
//! lit); a double-bladed one is `dual_1`'s two blades on the right hand.
//! KOTOR's swords, vibroblades and staves (C9) have no JA hilt: they take
//! the same tags with a stand-in length (`MELEE_LENGTH`, a delegated
//! decision).
//!

use crate::combat::saber::look::{self, Doing};
use crate::combat::saber::{Kit, SaberState, Style};
use crate::math::{self, Vec3};
use crate::motion::{Anim, AnimInfo, AnimTable, MotionState};
use crate::skeleton::{
    self, Blade, BladeId, Blend, Frames, Hand, Placement, Playback, Pose, Sequence, SkeletonData,
    Under,
};
use crate::wire::{DecodeError, Reader, Writer, read_fixed, write_fixed};

/// The JA sabers the sim's skeleton data holds, in this order: the host
/// loads them by these names (`SkeletonData::sabers`).
pub const SABERS: [&str; 2] = ["single_1", "dual_1"];
/// `SABERS`' single saber and double-bladed one.
const SINGLE: usize = 0;
pub const STAFF: usize = 1;

/// The stand-in length of a sword's or vibroblade's blade, and of each end
/// of a staff (units past the grip).
pub const MELEE_LENGTH: f32 = rules::sabers::blades::MELEE_LENGTH;
pub const MELEE_STAFF_LENGTH: f32 = rules::sabers::blades::MELEE_STAFF_LENGTH;

/// How long JA's server blends a part into a new animation (ms).
const BLEND_MS: u32 = rules::sabers::blades::BLEND_MS;

/// A blade's last place older than this counts as none (ms; rulebook 5.1).
pub const TRAIL_MS: u64 = rules::sabers::blades::TRAIL_MS;

/// The server's push of the model ahead along the velocity, per unit of
/// speed: JA's server frames of travel (`rules::sabers::blades::PUSH_FRAMES`
/// / `sv_fps`), taken over the sim's tick (the push stands for the lag
/// between what the server has and what a client sees, which scales with
/// the step).
pub fn push_per_speed(msec: u32) -> f32 {
    rules::sabers::blades::PUSH_FRAMES * msec as f32 / 1000.0
}

/// One part as the server's skeleton plays it: the animation and restart
/// bit last set, and its playback.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shown {
    pub anim: Anim,
    pub flip: bool,
    pub playback: Playback,
}

/// What the server's copy of a character's skeleton plays (JA keeps it in
/// the entity's Ghoul2 instance; the sim in its fighter, so a snapshot
/// restores it).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Posed {
    pub legs: Option<Shown>,
    pub torso: Option<Shown>,
}

/// An animation as the skeleton's playback takes it.
pub fn sequence(info: &AnimInfo) -> Sequence {
    Sequence {
        first_frame: info.first_frame,
        frames: info.frames,
        frame_lerp: info.frame_lerp,
        looping: info.looping,
    }
}

impl Posed {
    /// Follows the player's parts (`G_UpdateClientAnims`): a part whose
    /// animation or restart bit changed is set at `now` with the server's
    /// blend, the legs at their own speed, the torso at `torso_speed`.
    pub fn follow(&mut self, ps: &MotionState, torso_speed: f32, anims: &AnimTable, now: u64) {
        let set = |shown: &mut Option<Shown>, anim: Anim, flip: bool, speed: f32| {
            if shown.is_some_and(|s| s.anim == anim && s.flip == flip) {
                return;
            }
            let old = shown.map(|s| s.playback);
            let playback = Playback::set(
                old.as_ref(),
                &sequence(anims.get(anim)),
                speed,
                now as i64,
                BLEND_MS,
                None,
            );
            *shown = Some(Shown {
                anim,
                flip,
                playback,
            });
        };
        set(&mut self.legs, ps.legs.anim, ps.legs.flip, 1.0);
        set(&mut self.torso, ps.torso.anim, ps.torso.flip, torso_speed);
    }

    /// Both parts frozen on `frame` of `anim` (a saber lock's frame, set on
    /// the whole body with the server's blend), when it moved.
    pub fn freeze(&mut self, anim: Anim, frame: i32, anims: &AnimTable, now: u64) {
        let held = |shown: &Option<Shown>| {
            shown.is_some_and(|s| s.anim == anim && s.playback.start_frame == frame)
        };
        if held(&self.legs) && held(&self.torso) {
            return;
        }
        let info = anims.get(anim);
        let one = Sequence {
            first_frame: frame,
            frames: 1,
            frame_lerp: info.frame_lerp.abs().max(1),
            looping: false,
        };
        for part in [&mut self.legs, &mut self.torso] {
            let old = part.map(|s| s.playback);
            let playback = Playback::set(old.as_ref(), &one, 1.0, now as i64, BLEND_MS, None);
            // The restart bit is left as it was: a lock's parts go back to
            // what the player plays when it ends.
            let flip = part.is_some_and(|s| s.flip);
            *part = Some(Shown {
                anim,
                flip,
                playback,
            });
        }
    }

    /// The pose of a character standing as `ps` with `saber` at `now`, once
    /// both parts have been set: the server's look on it
    /// (`saber::look::facing`, the torso animation's turn read off this
    /// pose's `Motion` bolt), and the legs' yaw the model faces.
    pub fn pose(
        &self,
        data: &SkeletonData,
        ps: &MotionState,
        (saber, alive): (&SaberState, bool),
        now: u64,
    ) -> Option<(Pose, f32)> {
        let (legs, torso) = (self.legs?, self.torso?);
        let bare = Pose {
            legs: legs.playback,
            torso: torso.playback,
            look: None,
        };
        let doing = Doing {
            anims: [legs.anim, torso.anim],
            special_move: saber.in_special(),
            locked: saber.locked(now),
            alive,
        };
        let facing = look::facing(ps, doing, || skeleton::motion(data, &bare, now as i64));
        Some((
            Pose {
                look: Some(facing.look),
                ..bare
            },
            facing.legs_yaw,
        ))
    }

    pub(crate) fn encode(&self, out: &mut Writer) {
        for part in [&self.legs, &self.torso] {
            write_fixed(out, part.as_ref(), &BLANK, encode_shown);
        }
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Posed, DecodeError> {
        Ok(Posed {
            legs: read_fixed(input, &BLANK, decode_shown, "posed legs flag")?,
            torso: read_fixed(input, &BLANK, decode_shown, "posed torso flag")?,
        })
    }
}

const BLANK_PLAYBACK: Playback = Playback {
    start_frame: 0,
    end_frame: 0,
    speed: 0.0,
    start_ms: 0,
    looping: false,
    blend: None,
};

const BLANK: Shown = Shown {
    anim: Anim::BOTH_STAND1,
    flip: false,
    playback: BLANK_PLAYBACK,
};

const BLANK_BLEND: Blend = Blend {
    start_ms: 0,
    length_ms: 0,
    frame: 0.0,
    next: 0,
    under: None,
};

const BLANK_UNDER: Under = Under {
    frame: 0.0,
    next: 0,
    start: Frames {
        current: 0,
        next: 0,
        lerp: 0.0,
    },
    amount: 0.0,
};

fn encode_shown(shown: &Shown, out: &mut Writer) {
    out.u16(shown.anim.index());
    out.u8(u8::from(shown.flip));
    let p = &shown.playback;
    out.u32(p.start_frame as u32);
    out.u32(p.end_frame as u32);
    out.f32(p.speed);
    out.u64(p.start_ms as u64);
    out.u8(u8::from(p.looping));
    write_fixed(out, p.blend.as_ref(), &BLANK_BLEND, |blend, out| {
        out.u64(blend.start_ms as u64);
        out.u32(blend.length_ms);
        out.f32(blend.frame);
        out.u32(blend.next as u32);
        write_fixed(out, blend.under.as_ref(), &BLANK_UNDER, |under, out| {
            out.f32(under.frame);
            out.u32(under.next as u32);
            out.u32(under.start.current as u32);
            out.u32(under.start.next as u32);
            out.f32(under.start.lerp);
            out.f32(under.amount);
        });
    });
}

fn decode_flag(input: &mut Reader, what: &'static str) -> Result<bool, DecodeError> {
    let at = input.at();
    match input.u8()? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(input.invalid(what, at)),
    }
}

fn decode_shown(input: &mut Reader) -> Result<Shown, DecodeError> {
    let at = input.at();
    let anim = Anim::from_index(input.u16()?).ok_or(input.invalid("posed animation", at))?;
    let flip = decode_flag(input, "posed flip")?;
    let start_frame = input.u32()? as i32;
    let end_frame = input.u32()? as i32;
    let speed = input.f32()?;
    let start_ms = input.u64()? as i64;
    let looping = decode_flag(input, "posed loop")?;
    let blend = read_fixed(
        input,
        &BLANK_BLEND,
        |input| {
            Ok(Blend {
                start_ms: input.u64()? as i64,
                length_ms: input.u32()?,
                frame: input.f32()?,
                next: input.u32()? as i32,
                under: read_fixed(
                    input,
                    &BLANK_UNDER,
                    |input| {
                        Ok(Under {
                            frame: input.f32()?,
                            next: input.u32()? as i32,
                            start: Frames {
                                current: input.u32()? as i32,
                                next: input.u32()? as i32,
                                lerp: input.f32()?,
                            },
                            amount: input.f32()?,
                        })
                    },
                    "posed blend's blend flag",
                )?,
            })
        },
        "posed blend flag",
    )?;
    Ok(Shown {
        anim,
        flip,
        playback: Playback {
            start_frame,
            end_frame,
            speed,
            start_ms,
            looping,
            blend,
        },
    })
}

/// A blade's place at a tick: where it leaves the hilt and which way it
/// runs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Trail {
    pub muzzle: Vec3,
    pub direction: Vec3,
    /// When it was there (sim ms).
    pub at_ms: u64,
}

const BLANK_TRAIL: Trail = Trail {
    muzzle: [0.0; 3],
    direction: [0.0; 3],
    at_ms: 0,
};

impl Trail {
    pub(crate) fn encode_pair(trails: &[Option<Trail>; 2], out: &mut Writer) {
        for trail in trails {
            write_fixed(out, trail.as_ref(), &BLANK_TRAIL, |trail, out| {
                for value in trail.muzzle.iter().chain(&trail.direction) {
                    out.f32(*value);
                }
                out.u64(trail.at_ms);
            });
        }
    }

    pub(crate) fn decode_pair(input: &mut Reader) -> Result<[Option<Trail>; 2], DecodeError> {
        let read = |input: &mut Reader| {
            let mut values = [0.0f32; 6];
            for value in &mut values {
                *value = input.f32()?;
            }
            Ok(Trail {
                muzzle: [values[0], values[1], values[2]],
                direction: [values[3], values[4], values[5]],
                at_ms: input.u64()?,
            })
        };
        Ok([
            read_fixed(input, &BLANK_TRAIL, read, "trail flag")?,
            read_fixed(input, &BLANK_TRAIL, read, "trail flag")?,
        ])
    }
}

/// One blade a character holds: which of the data's blades on which hand,
/// and a stand-in length for a melee weapon's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Slot {
    pub id: BladeId,
    pub length: Option<f32>,
}

/// The blades of the weapon in `hand` (see the module documentation):
/// none unlit, none of a saber in flight but the pair's left one.
pub fn slots(kit: &Kit, saber: &SaberState, hand: u8) -> [Option<Slot>; 2] {
    let Some(blade) = kit.blade(hand) else {
        return [None; 2];
    };
    if !saber.on {
        return [None; 2];
    }
    let length = |staff: bool| {
        (!blade.saber).then_some(if staff {
            MELEE_STAFF_LENGTH
        } else {
            MELEE_LENGTH
        })
    };
    let slot = |hand, saber, blade, staff| {
        Some(Slot {
            id: BladeId { hand, saber, blade },
            length: length(staff),
        })
    };
    let second = !saber.second_off;
    let away = saber.thrown.is_some();
    if blade.double {
        if away {
            return [None; 2];
        }
        [
            slot(Hand::Right, STAFF, 0, true),
            second.then(|| slot(Hand::Right, STAFF, 1, true)).flatten(),
        ]
    } else if kit.forced_style(hand) == Some(Style::Dual) {
        [
            (!away)
                .then(|| slot(Hand::Right, SINGLE, 0, false))
                .flatten(),
            second.then(|| slot(Hand::Left, SINGLE, 0, false)).flatten(),
        ]
    } else if away {
        [None; 2]
    } else {
        [slot(Hand::Right, SINGLE, 0, false), None]
    }
}

/// An angle in degrees brought to (−180, 180] by whole turns.
pub fn normalize_180(angle: f32) -> f32 {
    let mut angle = angle;
    while angle > 180.0 {
        angle -= 360.0;
    }
    while angle <= -180.0 {
        angle += 360.0;
    }
    angle
}

/// Where a character standing as `ps` stands for its blades: its origin
/// pushed ahead, facing its legs' yaw (`Posed::pose`).
pub fn placement(ps: &MotionState, msec: u32, legs_yaw: f32) -> Placement {
    Placement::ahead(ps.origin, ps.velocity, push_per_speed(msec), legs_yaw)
}

/// `slot`'s blade in the world at `now` (units), its length the data's or
/// the stand-in's.
pub fn place(
    data: &SkeletonData,
    pose: &Pose,
    now: u64,
    placement: &Placement,
    slot: &Slot,
) -> Option<Blade> {
    let length = slot
        .length
        .or(data.saber_blade(slot.id).map(|b| b.length))?;
    let bolt = skeleton::bolt(data, pose, now as i64, skeleton::Bolt::Blade(slot.id))?;
    Some(Blade::of(&skeleton::world(&bolt, placement), length))
}

/// A blade now and where it was a tick ago, if it was placed then.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sweep {
    pub before: Option<Trail>,
    pub now: Blade,
    pub length: f32,
}

impl Sweep {
    /// The trail it leaves for the next tick.
    pub fn trail(&self, now_ms: u64) -> Trail {
        Trail {
            muzzle: self.now.muzzle,
            direction: self.now.direction,
            at_ms: now_ms,
        }
    }

    /// Where it was, or where it is when it has no fresh trail.
    pub fn start(&self) -> (Vec3, Vec3) {
        self.before
            .map_or((self.now.muzzle, self.now.direction), |t| {
                (t.muzzle, t.direction)
            })
    }
}

/// The trail of a blade last placed at `trail`, if it is still fresh at
/// `now_ms`.
pub fn fresh(trail: Option<Trail>, now_ms: u64) -> Option<Trail> {
    trail.filter(|t| now_ms.saturating_sub(t.at_ms) <= TRAIL_MS)
}

/// The box around blades (units): every muzzle and tip, the saber entity
/// bolts and blades clip against (rulebook 5.6).
pub fn bounds(blades: impl Iterator<Item = Blade>) -> Option<(Vec3, Vec3)> {
    blades.fold(None, |acc, blade| {
        let (mut min, mut max) = acc.unwrap_or((blade.muzzle, blade.muzzle));
        for p in [blade.muzzle, blade.tip] {
            for axis in 0..3 {
                min[axis] = min[axis].min(p[axis]);
                max[axis] = max[axis].max(p[axis]);
            }
        }
        Some((min, max))
    })
}

/// The unit direction from `a` to `b`, or none where they meet.
pub fn toward(a: Vec3, b: Vec3) -> Option<Vec3> {
    let mut d = math::sub(b, a);
    (math::normalize(&mut d) > 0.0).then_some(d)
}
