//! Getting unstuck from another character (issue 280). Characters are
//! solid to each other. A character that keeps running into another one
//! for `walking.jam.stuck_ms` walks through it until the two have parted
//! (`walking.jam.parted_m` between their boxes); then they are solid to
//! each other again. What each body has run into, and whom it walks
//! through, is its `MotionState::jam`, which the snapshot carries, so a
//! client predicting a body steps it as the authority does. Time is the
//! steps' own milliseconds: no wall clock.

use std::borrow::Cow;

use collision::Body;

use crate::motion::Jam;

/// How long a character keeps running into another before it walks
/// through it (sim milliseconds).
const STUCK_MS: u32 = rules::walking::jam::STUCK_MS;

/// The speed (m/s) under which a character trying to move that touches
/// another is running into it.
const CRAWL_MPS: f32 = rules::walking::jam::CRAWL_MPS;

/// The gap (metres) between two characters' boxes once they have parted.
const PARTED_M: f32 = rules::walking::jam::PARTED_M;

/// Who walks through whom this step: each pair as (the body that ran into
/// the other, the other), by their bodies' entities. A pair goes both
/// ways: neither body meets the other.
pub(super) struct Jams(Vec<(u32, u32)>);

impl Jams {
    /// The pairs the bodies' jams hold, each body by its entity.
    pub(super) fn of<'a>(jams: impl Iterator<Item = (u32, &'a Jam)>) -> Jams {
        Jams(
            jams.flat_map(|(own, jam)| {
                jam.passing
                    .into_iter()
                    .filter(|other| *other != 0)
                    .map(move |other| (own, other))
            })
            .collect(),
        )
    }

    /// Whether `a` and `b` walk through each other.
    pub(super) fn passes(&self, a: u32, b: u32) -> bool {
        self.0.iter().any(|pair| *pair == (a, b) || *pair == (b, a))
    }

    /// `bodies` as the body `own` meets them: less those it walks through.
    pub(super) fn meets<'b>(&self, own: u32, bodies: &'b [Body]) -> Cow<'b, [Body]> {
        if !self.0.iter().any(|(a, b)| *a == own || *b == own) {
            return Cow::Borrowed(bodies);
        }
        Cow::Owned(
            bodies
                .iter()
                .filter(|body| !self.passes(own, body.entity))
                .copied()
                .collect(),
        )
    }

    /// The characters body `own` (its box `mine`) walks through that it has
    /// parted from, or that are gone (dead or out of the world) from the
    /// living `bodies`, are solid to it again.
    pub(super) fn release(&mut self, own: u32, jam: &mut Jam, mine: &Body, bodies: &[Body]) {
        for slot in &mut jam.passing {
            let other = *slot;
            if other == 0 {
                continue;
            }
            let near = bodies
                .iter()
                .find(|body| body.entity == other)
                .is_some_and(|body| !parted(mine, body));
            if !near {
                *slot = 0;
                self.0.retain(|pair| *pair != (own, other));
            }
        }
        if jam.passing[0] == 0 {
            jam.passing = [jam.passing[1], 0];
        }
    }

    /// One step of `msec` in which body `own` ran into `by` (none: it
    /// moved freely). Running into the same character for `STUCK_MS`
    /// without a break, it walks through that one from the next move on.
    pub(super) fn ran_into(&mut self, own: u32, jam: &mut Jam, by: Option<u32>, msec: u32) {
        let Some(by) = by else {
            (jam.by, jam.ms) = (0, 0);
            return;
        };
        jam.ms = if jam.by == by {
            jam.ms.saturating_add(msec)
        } else {
            msec
        };
        jam.by = by;
        if jam.ms < STUCK_MS {
            return;
        }
        (jam.by, jam.ms) = (0, 0);
        if self.passes(own, by) {
            return;
        }
        // The older of two it already walks through is solid again.
        if jam.passing[1] != 0 {
            let older = jam.passing[0];
            self.0.retain(|pair| *pair != (own, older));
            jam.passing[0] = jam.passing[1];
        }
        let free = usize::from(jam.passing[0] != 0);
        jam.passing[free] = by;
        self.0.push((own, by));
    }
}

/// Whether two boxes (metres) stand at least `PARTED_M` apart, across or
/// up and down.
fn parted(a: &Body, b: &Body) -> bool {
    (0..3)
        .any(|axis| a.min[axis] - b.max[axis] >= PARTED_M || b.min[axis] - a.max[axis] >= PARTED_M)
}

/// The character a body ran into in a step of `msec`: one of the bodies
/// it `touched` (entities) that `may` pass, while its command moved it
/// (`trying`) by only `moved` (units) across, slower than `CRAWL_MPS`. The
/// one it ran into last (`last`) before any other.
pub(super) fn blocked_by(
    trying: bool,
    moved: f32,
    msec: u32,
    touched: &[u32],
    last: u32,
    may: impl Fn(u32) -> bool,
) -> Option<u32> {
    let crawl = CRAWL_MPS / collision::UNIT * msec as f32 / 1000.0;
    if !trying || moved >= crawl {
        return None;
    }
    let mut bodies = touched
        .iter()
        .copied()
        .filter(|entity| *entity & super::BODY != 0 && may(*entity));
    let first = bodies.clone().next();
    bodies.find(|entity| *entity == last).or(first)
}
