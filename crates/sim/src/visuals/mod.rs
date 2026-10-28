//! Scripts' visual effects on a door or placeable (`EffectVisualEffect`,
//! `EffectBeam` applied to one: the Star Forge's drain beams onto its
//! captive Jedi and the invisible placeables between them). A character
//! carries its visuals as events on its fighter; an object has none, so
//! the module keeps them here from the step they were applied until they
//! end (one fleeting for `SEEN_MS`, so every view sees it), and the
//! snapshot carries them for the views (`launcher::script_fx`). A module
//! left takes its own with it: a new module begins with none.

use crate::combat::fighter::Visual;
use crate::snapshot::SimId;
use crate::wire::{DecodeError, Reader, Writer};

/// How long a fleeting visual on an object is kept after its step (ms):
/// long enough for every view to have presented it.
pub const SEEN_MS: u64 = 1000;

/// A visual on an object: the effect it came in (`script::effects::Applied::id`:
/// the views tell them apart by it), the object, who applied it (a beam's
/// source), its row, length and body part, and when it began (sim ms).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OnObject {
    pub id: u32,
    pub on: SimId,
    pub by: SimId,
    pub visual: Visual,
    pub since: u64,
}

impl OnObject {
    /// When it ends (sim ms): never for a permanent one (`Visual::ms`
    /// `u32::MAX`).
    pub fn until(&self) -> Option<u64> {
        (self.visual.ms != u32::MAX).then(|| self.since + u64::from(self.visual.ms))
    }

    /// Whether it is still kept at `now` (ms).
    pub fn kept(&self, now: u64) -> bool {
        self.until()
            .is_none_or(|until| now < until.max(self.since + SEEN_MS))
    }
}

pub(crate) fn encode(visuals: &[OnObject], out: &mut Writer) {
    out.u16(u16::try_from(visuals.len()).expect("fewer visuals on objects than 65536"));
    for visual in visuals {
        out.u32(visual.id);
        out.u32(visual.on.0);
        out.u32(visual.by.0);
        visual.visual.encode(out);
        out.u64(visual.since);
    }
}

/// The visuals `encode` wrote, in the order they were applied (by effect,
/// each once: one state, one encoding).
pub(crate) fn decode(input: &mut Reader) -> Result<Vec<OnObject>, DecodeError> {
    let mut visuals: Vec<OnObject> = Vec::new();
    for _ in 0..input.u16()? {
        let at = input.at();
        let visual = OnObject {
            id: input.u32()?,
            on: SimId(input.u32()?),
            by: SimId(input.u32()?),
            visual: Visual::decode(input)?,
            since: input.u64()?,
        };
        if visuals
            .last()
            .is_some_and(|last| (last.since, last.id) >= (visual.since, visual.id))
        {
            return Err(input.invalid("visual on an object", at));
        }
        visuals.push(visual);
    }
    Ok(visuals)
}

#[cfg(feature = "probes")]
mod probes;
#[cfg(feature = "probes")]
pub use probes::probes;
