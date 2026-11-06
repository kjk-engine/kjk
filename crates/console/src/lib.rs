//! The command queue that scripts drive the runtime with (`kjk --cmds '…'`).
//!
//! Commands run one at a time, and each one finishes only when what it
//! promises is true: a wait has elapsed, a screenshot file exists, a dump is
//! written. A command that cannot keep its promise ends the run with an
//! error instead of letting the next one start.
//!
//! A screenshot also waits until the renderer has no pipelines left to
//! compile: shaders compile in the background, and a frame captured before
//! they finish shows nothing of the scene.

//! The command language and the camera moves need no Bevy (`runtime` off):
//! the approved tests parse and move without building it.

mod motion;
mod script;

#[cfg(feature = "runtime")]
mod runtime;
#[cfg(feature = "runtime")]
mod video;
#[cfg(feature = "runtime")]
mod vitals;

pub use motion::{CHEST_M, CameraMotion, Follow, Move, Path};
#[cfg(feature = "runtime")]
pub use runtime::*;
pub use script::{CameraPose, Command, Frames, ParseError, Video, WaitFor, parse};
#[cfg(feature = "runtime")]
pub use video::AUDIO;
