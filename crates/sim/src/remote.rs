//! The remote droid (`docs/superpowers/specs/2026-10-29-remote-droid-design.md`):
//! a co-op guest with no party member to drive flies one, like Bao-Dur's
//! remote. It is no entity of the world's: no id, no body scripts see, no
//! fighter. The world makes it again each step from the seats with no body
//! (`world::remote`), so a save holds none; the snapshot carries it for the
//! views to draw and predict.

use crate::input::Seat;
use crate::wire::{DecodeError, Reader, Writer};

/// The kit's buttons, as indexes into `Remote::ready_ms`.
pub const KIT_BOLT: usize = 0;
pub const KIT_SONIC: usize = 1;
pub const KIT_DART: usize = 2;

/// A seat's remote droid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Remote {
    pub seat: Seat,
    /// Game space (Z up), metres.
    pub at: [f32; 3],
    /// Metres per second.
    pub velocity: [f32; 3],
    /// Degrees, as the seat's view turns it.
    pub yaw: f32,
    /// When each of the kit's buttons is ready again (sim ms).
    pub ready_ms: [u64; 3],
    /// Not drawn and its kit off: the world holds (a conversation, a scene,
    /// a movie, a fade).
    pub hidden: bool,
}

impl Remote {
    /// A droid for `seat` at `at`, still, its kit ready.
    pub fn new(seat: Seat, at: [f32; 3], yaw: f32) -> Remote {
        Remote {
            seat,
            at,
            velocity: [0.0; 3],
            yaw,
            ready_ms: [0; 3],
            hidden: false,
        }
    }
}

/// By seat, one each.
pub(crate) fn encode(remotes: &[Remote], out: &mut Writer) {
    out.u8(u8::try_from(remotes.len()).expect("fewer droids than seats"));
    for remote in remotes {
        out.u8(remote.seat.0);
        for value in remote.at.iter().chain(&remote.velocity) {
            out.f32(*value);
        }
        out.f32(remote.yaw);
        for ready in remote.ready_ms {
            out.u64(ready);
        }
        out.u8(u8::from(remote.hidden));
    }
}

pub(crate) fn decode(input: &mut Reader) -> Result<Vec<Remote>, DecodeError> {
    let mut remotes: Vec<Remote> = Vec::new();
    for _ in 0..input.u8()? {
        let at = input.at();
        let seat = Seat(input.u8()?);
        if seat.0 >= crate::MAX_SEATS || remotes.last().is_some_and(|last| last.seat >= seat) {
            return Err(input.invalid("remote seat", at));
        }
        let mut values = [0.0; 6];
        for value in &mut values {
            *value = input.f32()?;
        }
        let yaw = input.f32()?;
        let mut ready_ms = [0; 3];
        for ready in &mut ready_ms {
            *ready = input.u64()?;
        }
        let at = input.at();
        let hidden = match input.u8()? {
            0 => false,
            1 => true,
            _ => return Err(input.invalid("remote hidden flag", at)),
        };
        remotes.push(Remote {
            seat,
            at: [values[0], values[1], values[2]],
            velocity: [values[3], values[4], values[5]],
            yaw,
            ready_ms,
            hidden,
        });
    }
    Ok(remotes)
}
