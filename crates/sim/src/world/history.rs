//! Lag compensation's pose history (`docs/COMBAT.md`,
//! C18): the authority keeps every character for the last `HISTORY` ticks,
//! and tests a player's shots and blades against the others as they were
//! when that player saw them (its view lag, `TickInput::view_lag`), at most
//! `REWIND_MS` back. Creatures' attacks, which the authority decides with
//! no latency, are never rewound.
//!
//! A tick's pose is a character's body (from `remember`, the step's last
//! system) and its fence (from the blades step, `Fence`): its blades' base
//! and tip, whether they stop others', and its guard. A blade's sweep meets
//! the others' bodies, blades and guards as its holder saw them.
//!
//! The history is not snapshot state: a world keeps it from its own
//! steps, seeded with the snapshot it was made from (bodies only), and a
//! rewind never reaches before that snapshot's tick. A replay starts where
//! its world started, so it rewinds exactly as the authority did; a world
//! restored from a save rewinds only as far as the load. Prediction never
//! rewinds. A tick without a fence (the seed's) is met as it stands now.

use std::collections::BTreeMap;

use bevy_ecs::prelude::*;
use collision::Body;

use super::{Avatar, Clock, Fighting, Id, Player, Targets, body, in_party};
use crate::combat::damage::FriendlyFire;
use crate::combat::saber::blades::Sweep;
use crate::math::Vec3;
use crate::snapshot::{SimId, Snapshot};

/// Ticks of poses kept (512 ms at 125 Hz), and the most a player's view is
/// rewound (ms; the book says what it covers).
pub(super) const HISTORY: usize = 64;
pub const REWIND_MS: u64 = rules::sabers::lag::REWIND_MS;

/// A character's blades and guard at the end of a tick's blades step.
#[derive(Debug, Clone, Copy)]
pub(super) struct Fence {
    /// Each blade's place that tick, with its place a tick before.
    pub blades: [Option<Sweep>; 2],
    /// Whether its blades stopped others' (`Step::solid`).
    pub solid: bool,
    /// Its block by state against blades (C6) but for the arc, if it could
    /// block then.
    pub guard: Option<Stand>,
}

/// A guard ready to block: its defence level against blades, and where it
/// stood and looked (the arc's centre).
#[derive(Debug, Clone, Copy)]
pub(super) struct Stand {
    pub level: u8,
    pub origin: Vec3,
    pub yaw: f32,
}

/// A character at a tick: its body if it was alive, whether it was in the
/// party (for shots that pass through the party), and its fence.
#[derive(Debug, Clone, Copy)]
struct Pose {
    tick: u64,
    body: Option<Body>,
    party: bool,
    fence: Option<Fence>,
}

/// See the module documentation.
#[derive(Resource, Default)]
pub(super) struct History {
    /// The earliest tick a rewind may reach.
    first_tick: u64,
    /// Each character's poses, in slots by tick modulo `HISTORY`.
    poses: BTreeMap<SimId, [Option<Pose>; HISTORY]>,
}

impl History {
    /// A history starting at `snapshot`: its characters as it has them.
    pub(super) fn starting(snapshot: &Snapshot) -> History {
        let mut history = History {
            first_tick: snapshot.tick,
            poses: BTreeMap::new(),
        };
        for entity in &snapshot.entities {
            if let (Some(player), Some(fighter)) = (entity.player, entity.fighter) {
                history.record(
                    snapshot.tick,
                    entity.id,
                    fighter.alive().then(|| body(entity.id, &player)),
                    snapshot.in_party(entity),
                );
            }
        }
        history
    }

    /// `id`'s slot for `tick`, holding that tick's pose (a blank one when
    /// it held an older tick's).
    fn slot(&mut self, tick: u64, id: SimId) -> &mut Pose {
        let slots = self.poses.entry(id).or_insert([None; HISTORY]);
        let slot = &mut slots[(tick % HISTORY as u64) as usize];
        if slot.is_none_or(|pose| pose.tick != tick) {
            *slot = Some(Pose {
                tick,
                body: None,
                party: false,
                fence: None,
            });
        }
        slot.as_mut().expect("just filled")
    }

    fn record(&mut self, tick: u64, id: SimId, body: Option<Body>, party: bool) {
        let pose = self.slot(tick, id);
        pose.body = body;
        pose.party = party;
    }

    /// Keeps `id`'s fence at `tick`.
    pub(super) fn fence(&mut self, tick: u64, id: SimId, fence: Fence) {
        self.slot(tick, id).fence = Some(fence);
    }

    fn at(&self, tick: u64, id: SimId) -> Option<&Pose> {
        self.poses.get(&id)?[(tick % HISTORY as u64) as usize]
            .as_ref()
            .filter(|pose| pose.tick == tick)
    }

    /// `id`'s fence at `tick`, if the blades step kept one then.
    pub(super) fn fence_at(&self, tick: u64, id: SimId) -> Option<&Fence> {
        self.at(tick, id)?.fence.as_ref()
    }

    /// The tick a view `lag_ms` behind tick `now` saw, in ticks of `msec`,
    /// at most `REWIND_MS` back and never before the history's start;
    /// `None` when there is nothing to rewind.
    pub(super) fn view_tick(&self, now: u64, lag_ms: u16, msec: u32) -> Option<u64> {
        let back = u64::from(lag_ms).min(REWIND_MS) / u64::from(msec.max(1));
        let view = now.saturating_sub(back).max(self.first_tick);
        (view < now).then_some(view)
    }

    /// Everyone's bodies as they stood at `tick` (those without a pose
    /// then, or dead then, are not there).
    pub(super) fn targets_at(&self, tick: u64, friendly_fire: FriendlyFire) -> Targets {
        Targets::of_bodies(
            friendly_fire,
            self.poses
                .keys()
                .filter_map(|id| self.at(tick, *id).map(|pose| (*id, pose.body, pose.party))),
        )
    }
}

/// Keeps this step's bodies (the last system of an authority step).
pub(super) fn remember(
    clock: Res<Clock>,
    mut history: ResMut<History>,
    actors: Query<(&Id, Option<&Avatar>, &Player, &Fighting)>,
) {
    for (id, avatar, player, fighting) in &actors {
        history.record(
            clock.tick,
            id.0,
            fighting.0.alive().then(|| body(id.0, &player.0)),
            in_party(avatar, &fighting.0),
        );
    }
}
