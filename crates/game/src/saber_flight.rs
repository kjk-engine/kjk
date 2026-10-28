//! The flight of a thrown saber as JA's game moves it, worked out for a
//! whole throw at once (`fly`) as straight legs, each from a moment on.
//!
//! The saber leaves the hand at 400 units a second along the view. The
//! server moves it every 25 ms frame and thinks for it every 100 ms: from
//! the second level up it re-aims at the point 4096 units along the view
//! from the eye (every think at level 3, every fifth at level 2), at 500
//! units a second. It turns back once the button is let go after half a
//! second, after six seconds, or when it is as far from its owner as the
//! level allows (700 units a level); coming back it thinks every frame,
//! heads for the hand at 700 units a second (900 at level 3), slower the
//! nearer the hand, re-aiming every frame at level 3 and every third
//! frame otherwise, and is caught within 32 units of the hand. With the
//! quirks the turn leaves it hanging where the server last placed it
//! until the next think (QUIRKS.md Q-0007).
//!
//! Flying out it spins about the vertical and tips over flat
//! (`turn_out`); coming back it is turned along the line from its
//! owner to it (`turn_back`).
use bevy::math::{Quat, Vec3};

const UNIT: f32 = asset_ja::UNIT;
const SERVER_FRAME_MS: f32 = 25.0;
const THINK_MS: f32 = 100.0;
/// The eye above the origin (the game's default view height), in units.
const EYE_HEIGHT: f32 = 36.0;

/// A throw of the saber in the right hand.
#[derive(Debug, Clone, Copy)]
pub struct Throw {
    /// When it leaves the hand, and when the button is let go (ms).
    pub start: f32,
    pub release: f32,
    /// The saber throw power's level, 1 to 3.
    pub level: u32,
}

/// One leg of the flight: from `from` (ms) the saber moves from `base` at
/// `velocity` (m/s), as a linear trajectory does.
#[derive(Debug, Clone, Copy)]
struct Leg {
    from: f32,
    base: Vec3,
    velocity: Vec3,
    returning: bool,
}

impl Leg {
    fn at(&self, ms: f32) -> Vec3 {
        self.base + self.velocity * (ms - self.from) / 1000.0
    }
}

#[derive(Debug, Clone)]
pub struct Flight {
    pub start: f32,
    /// When the hand has it again (ms).
    pub catch: f32,
    legs: Vec<Leg>,
    /// The view's yaw at the throw (radians, from +X).
    yaw: f32,
}

impl Flight {
    /// The leg under way at `ms`: the last one begun by then, else the
    /// first.
    fn leg_at(&self, ms: f32) -> &Leg {
        self.legs
            .iter()
            .rfind(|leg| leg.from <= ms)
            .unwrap_or(&self.legs[0])
    }

    /// Where the saber is at `ms` (metres).
    pub fn position(&self, ms: f32) -> Vec3 {
        self.leg_at(ms).at(ms)
    }

    /// How the saber is turned at `ms`: spinning on its way out, along the
    /// line from `owner` (metres) on its way back.
    pub fn rotation(&self, ms: f32, owner: Vec3) -> Quat {
        let leg = self.leg_at(ms);
        if leg.returning {
            turn_back(leg.at(ms) - owner)
        } else {
            turn_out(ms - self.start, self.yaw)
        }
    }
}

/// The whole flight of `throw`: `hand(ms)` is where the hand holds the
/// hilt and `origin(ms)` the owner's origin (metres), `facing` the view.
/// The saber is moved on the server's 25 ms frames and thinks every
/// 100 ms flying out, every frame coming back; each change of course
/// starts a new leg. Without a catch within 20 s it is caught then.
pub fn fly(
    throw: &Throw,
    hand: &dyn Fn(f32) -> Vec3,
    origin: &dyn Fn(f32) -> Vec3,
    facing: Vec3,
    quirks: bool,
) -> Flight {
    let facing = facing.normalize_or_zero();
    let level = throw.level.clamp(1, 3) as f32;
    let start = throw.start;
    let mut legs = vec![Leg {
        from: start,
        base: hand(start),
        velocity: facing * 400.0 * UNIT,
        returning: false,
    }];
    // Where the server last worked the saber out to be.
    let mut known = hand(start);
    // Where the newest leg has it at `ms`.
    let now = |legs: &[Leg], ms: f32| legs[legs.len() - 1].at(ms);
    let mut next_think = start + THINK_MS;
    let mut reaim = 0.0;
    let mut catch = start + 20000.0;
    let mut returning = false;

    let mut ms = start;
    while ms < start + 20000.0 {
        ms += SERVER_FRAME_MS;
        if ms < next_think {
            continue;
        }
        if !returning {
            let out = ms - start;
            let let_go = out > 500.0 && (ms >= throw.release || out > 6000.0);
            let far = origin(ms).distance(known) >= 700.0 * level * UNIT;
            if let_go || far {
                reaim = 0.0;
                returning = true;
                if quirks {
                    // It hangs where it was last worked out until the next
                    // think (QUIRKS.md Q-0007).
                    legs.push(Leg {
                        from: ms,
                        base: known,
                        velocity: Vec3::ZERO,
                        returning: true,
                    });
                    next_think = ms + THINK_MS;
                    continue;
                }
                known = now(&legs, ms);
            } else {
                if level >= 2.0 && reaim < ms {
                    known = now(&legs, ms);
                    let eye = origin(ms) + Vec3::Z * EYE_HEIGHT * UNIT;
                    let target = eye + facing * 4096.0 * UNIT;
                    legs.push(Leg {
                        from: ms,
                        base: known,
                        velocity: (target - known).normalize_or_zero() * 500.0 * UNIT,
                        returning: false,
                    });
                    reaim = if level >= 3.0 { ms + 100.0 } else { ms + 400.0 };
                }
                known = now(&legs, ms);
                next_think = ms + THINK_MS;
                continue;
            }
        }

        // Coming back: it thinks every frame from here on.
        next_think = ms;
        let to_hand = hand(ms) - known;
        let away = to_hand.length() / UNIT;
        if reaim < ms {
            known = now(&legs, ms);
            let speed = if level >= 3.0 { 900.0 } else { 700.0 };
            let slower = if away < 64.0 {
                200.0
            } else if away < 128.0 {
                150.0
            } else if away < 256.0 {
                100.0
            } else {
                0.0
            };
            reaim = if level >= 3.0 { ms } else { ms + 50.0 };
            legs.push(Leg {
                from: ms,
                base: known,
                velocity: to_hand.normalize_or_zero() * (speed - slower) * UNIT,
                returning: true,
            });
        }
        if away <= 32.0 {
            catch = ms;
            break;
        }
        known = now(&legs, ms);
    }

    Flight {
        start,
        catch,
        legs,
        yaw: facing.y.atan2(facing.x),
    }
}

/// The saber flying out, `out_ms` after the throw, the view at `yaw`
/// (radians): it spins 800° a second about the vertical and tips over at
/// half a degree a millisecond until it lies flat.
pub fn turn_out(out_ms: f32, yaw: f32) -> Quat {
    let out = out_ms.max(0.0);
    let pitch = (out * 0.5).min(90.0).to_radians();
    Quat::from_rotation_z(yaw + (800.0 * out / 1000.0).to_radians()) * Quat::from_rotation_y(pitch)
}

/// The saber coming back, `away` from its owner: turned to lie along that
/// line.
pub fn turn_back(away: Vec3) -> Quat {
    let away = away.normalize_or_zero();
    let rise = away.z.clamp(-1.0, 1.0).asin();
    Quat::from_rotation_z(away.y.atan2(away.x))
        * Quat::from_rotation_y(std::f32::consts::FRAC_PI_2 - rise)
}
