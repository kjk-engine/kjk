//! What a part of the body plays, and which frames it shows at a moment:
//! Ghoul2's timing as `docs/G2-SPEC.md` specifies it (without the
//! reference's playback quirks), in whole milliseconds.

/// An `animation.cfg` sequence as a part is handed it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sequence {
    pub first_frame: i32,
    pub frames: i32,
    /// Milliseconds per frame, negative for a sequence played backward
    /// (`ceil(1000 / fps)`, `floor` backward); 0 holds the first frame.
    pub frame_lerp: i32,
    /// The sequence has a loop frame.
    pub looping: bool,
}

/// An animation on a part (Ghoul2's bone animation): from `start_frame`
/// toward `end_frame` (one past the last frame it shows that way) at
/// `speed` frames per 50 ms, set at `start_ms`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Playback {
    pub start_frame: i32,
    pub end_frame: i32,
    /// Frames per 50 ms; negative runs backward.
    pub speed: f32,
    pub start_ms: i64,
    pub looping: bool,
    pub blend: Option<Blend>,
}

/// The pose the part blends from over `length_ms` from `start_ms`: frame
/// `frame` (whole part) mixed toward `next` by `frame`'s fraction, or, set
/// while the last blend was under way (`under`), the pose that blend showed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Blend {
    pub start_ms: i64,
    pub length_ms: u32,
    pub frame: f32,
    pub next: i32,
    pub under: Option<Under>,
}

/// The blend a blend began in, partway (QUIRKS.md Q-0081): what it blended
/// from (`frame` toward `next`), where its animation stood when it began,
/// and how far through it was. One level deep: the pose that one began
/// from is its frames.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Under {
    pub frame: f32,
    pub next: i32,
    pub start: Frames,
    pub amount: f32,
}

/// The frames a part shows: `current` mixed toward `next` by `lerp`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frames {
    pub current: i32,
    pub next: i32,
    pub lerp: f32,
}

impl Playback {
    /// `sequence` set at `now_ms` at `speed_scale` times its rate, blending
    /// over `blend_ms` from what `old` shows then (docs/G2-SPEC.md, "The
    /// ranges" and "The pose a blend starts from"): a backward sequence
    /// runs over its own frames, last to first; a blend set at the very
    /// moment the last one was keeps that one's pose and takes the new
    /// length. `begin`, a frame of the range, starts the sequence there, as
    /// if set early enough to reach it at `now_ms`, on a whole millisecond
    /// (the caller's pick-up rule).
    ///
    /// A change while `old` is still blending blends from the pose `old`
    /// shows then (`Under`), not from its own frames as Ghoul2 does
    /// (QUIRKS.md Q-0017, Q-0081).
    pub fn set(
        old: Option<&Playback>,
        sequence: &Sequence,
        speed_scale: f32,
        now_ms: i64,
        blend_ms: u32,
        begin: Option<f32>,
    ) -> Playback {
        let speed = if sequence.frame_lerp == 0 {
            0.0
        } else {
            50.0 / sequence.frame_lerp as f32 * speed_scale
        };
        let first = sequence.first_frame;
        let (start_frame, end_frame) = if speed < 0.0 {
            (first + sequence.frames - 1, first - 1)
        } else {
            (first, first + sequence.frames)
        };
        let blend = match (blend_ms > 0, old) {
            (true, Some(old)) => Some(match old.blend {
                Some(pending) if pending.start_ms == now_ms => Blend {
                    length_ms: blend_ms,
                    ..pending
                },
                _ => old.captured(now_ms, blend_ms),
            }),
            _ => None,
        };
        let start_ms = match begin {
            Some(frame) if speed != 0.0 => {
                (now_ms as f32 - (frame - start_frame as f32) * 50.0 / speed).trunc() as i64
            }
            _ => now_ms,
        };
        Playback {
            start_frame,
            end_frame,
            speed,
            start_ms,
            looping: sequence.looping,
            blend,
        }
    }

    /// The blend a new animation set at `now_ms` starts from: the frames
    /// shown then, a forward one's recomputed from its place (the next
    /// frame, or the first again or the last at the end), and the blend
    /// they were shown in, if one was under way.
    fn captured(&self, now_ms: i64, length_ms: u32) -> Blend {
        let shown = self.frames(now_ms);
        let place = shown.current as f32 + shown.lerp;
        let (frame, next) = if self.speed < 0.0 {
            (place, shown.next)
        } else {
            let end = self.end_frame as f32;
            let within = |f: f32| {
                if f < end {
                    f
                } else if self.looping {
                    self.start_frame as f32
                } else {
                    end - 1.0
                }
            };
            (within(place), within(place + 1.0) as i32)
        };
        // (A blend set at its own start was kept by `set`.)
        let under = self.blending(now_ms).map(|(blend, amount)| Under {
            frame: blend.frame,
            next: blend.next,
            start: self.frames(blend.start_ms),
            amount,
        });
        Blend {
            start_ms: now_ms,
            length_ms,
            frame,
            next,
            under,
        }
    }

    /// The frame shown at `now_ms` with the fraction toward the next
    /// (what the pick-up rule reads off the other part).
    pub fn frame_at(&self, now_ms: i64) -> f32 {
        let shown = self.frames(now_ms);
        shown.current as f32 + shown.lerp
    }

    /// The frames shown at `now_ms` (docs/G2-SPEC.md, "The frames at a
    /// moment"): the place along the range, held on the last frame when
    /// frozen, wrapped by whole lengths once a loop reaches the end; the
    /// frame at or behind it, the one ahead (the first again at a loop's
    /// last), and how far between.
    pub fn frames(&self, now_ms: i64) -> Frames {
        let forward = self.speed >= 0.0;
        let step = if forward { 1 } else { -1 };
        let last = self.end_frame - step;
        let hold = |frame: i32| Frames {
            current: frame,
            next: frame,
            lerp: 0.0,
        };
        if self.end_frame == self.start_frame {
            return hold(last.max(0));
        }
        let (start, bound) = (self.start_frame as f32, self.end_frame as f32);
        let elapsed = now_ms.saturating_sub(self.start_ms) as f32 / 50.0;
        let place = start + elapsed.max(0.0) * self.speed;
        let length = bound - start;
        let wrapped = |place: f32| bound + (place - bound) % length - length;
        let past_end = if forward {
            place >= bound
        } else {
            place <= bound
        };
        let beyond_last = if forward {
            place > last as f32
        } else {
            place < last as f32
        };
        let place = match (self.looping, past_end) {
            (true, true) => wrapped(place),
            (false, _) if beyond_last => return hold(last),
            _ => place,
        };
        let behind = if forward { place.trunc() } else { place.ceil() };
        let at_last = if forward {
            place >= last as f32
        } else {
            behind <= last as f32
        };
        let current = behind as i32;
        let next = match (at_last, self.looping) {
            (false, _) => current + step,
            (true, true) => self.start_frame,
            (true, false) => last,
        };
        Frames {
            current,
            next,
            lerp: (place - behind).abs(),
        }
    }

    /// The blend under way at `now_ms` and how far through it is.
    pub fn blending(&self, now_ms: i64) -> Option<(Blend, f32)> {
        let blend = self.blend?;
        let elapsed = now_ms.saturating_sub(blend.start_ms) as f32;
        let length = blend.length_ms as f32;
        (elapsed >= 0.0 && elapsed < length).then(|| (blend, elapsed / length))
    }
}
