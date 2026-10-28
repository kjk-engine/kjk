//! Drawing and holstering pistols, as drawn (view only; the sim keeps its
//! raise and drop, `motion::weapon`): a body that carries its pistols on its
//! thighs (`weapons::Hands::carries`, the players' characters) reaches
//! down to each holster, takes the gun and brings it up to aim; holstering
//! is the same backwards. One track runs it, `along`:
//!
//! - 0: the hand where the pose has it, the gun in its holster;
//! - 1: the hand on the holstered gun's grip (`asset_ja::rig::Stowed`);
//! - 2: the gun in hand and aimed (`asset_ja::rig::Aim`).
//!
//! The gun goes from the holster to the hand as the track passes 1 (and
//! back). A draw runs the track up over `DRAW_MS`, the sim's raise; a
//! holster down over `HOLSTER_MS`. Turned round midway, it runs back from
//! where it is, so nothing jumps.

/// How long a draw takes (ms): the sim's raise, which the first shot waits
/// on (`rules::weapons_in_hand::change::RAISE_MS`).
pub const DRAW_MS: f64 = rules::weapons_in_hand::change::RAISE_MS as f64;
/// How long a holster takes (ms): longer than the sim's drop, the gun no
/// longer firing (view only).
pub const HOLSTER_MS: f64 = 600.0;

/// A body's draw: where it is on the track, as of when (ms).
#[derive(Debug, Clone, Copy, Default)]
pub struct Drawing {
    along: f64,
    ms: Option<f64>,
}

/// What the track gives the pose at a moment: how far the hand is on the
/// holstered gun (0 to 1, `Stowed::share`), how far the gun is aimed
/// (`Aim::weight` at most), and whether the gun is in hand.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawPose {
    pub stow: f32,
    pub aim: f32,
    pub in_hand: bool,
}

impl Drawing {
    /// Already drawn (a body that comes into view armed) or not.
    pub fn at(drawn: bool) -> Drawing {
        Drawing {
            along: if drawn { 2.0 } else { 0.0 },
            ms: None,
        }
    }

    /// The track moved on to `ms`, toward the gun out (`armed`) or away.
    pub fn step(&mut self, armed: bool, ms: f64) -> DrawPose {
        let since = self.ms.map_or(0.0, |was| (ms - was).max(0.0));
        self.ms = Some(ms);
        self.along = if armed {
            (self.along + since * 2.0 / DRAW_MS).min(2.0)
        } else {
            (self.along - since * 2.0 / HOLSTER_MS).max(0.0)
        };
        self.pose()
    }

    /// Where the track is, as the pose takes it.
    pub fn pose(&self) -> DrawPose {
        let along = self.along as f32;
        let smooth = |x: f32| x * x * (3.0 - 2.0 * x);
        if along < 1.0 {
            DrawPose {
                stow: smooth(along),
                aim: 0.0,
                in_hand: false,
            }
        } else {
            DrawPose {
                stow: smooth(2.0 - along),
                aim: 1.0,
                in_hand: true,
            }
        }
    }

    /// Neither drawing nor holstering: all out or all away.
    pub fn settled(&self) -> bool {
        self.along <= 0.0 || self.along >= 2.0
    }
}
