//! The sim funnel: every change to the game world goes through one function,
//!
//! ```text
//! step(world, tick, input, msec, reason) -> Snapshot
//! ```
//!
//! and one more for a module change, `SimWorld::install` (the commit of an
//! install transaction), and nothing else reads or writes the world. Time
//! is an argument (whole milliseconds), inputs arrive in a canonical order,
//! randomness comes from seeded streams saved in the state, and the result
//! is a plain-value snapshot. The same inputs therefore give the same
//! snapshot, bit for bit, which is what saves, replay, client prediction
//! and the network stand on.
//!
//! Players move as JA's players do (`move_body`, I13) against the level's
//! collision; a KOTOR module's objects (`module`, I14) are entities, whose
//! doors and triggers players open and walk through. Wanderers are the
//! placeholder world's. See `docs/SIM.md`, `docs/MOVEMENT.md` and
//! `docs/LIFECYCLE.md`.

pub mod action;
pub mod combat;
pub mod creation;
pub mod determinism;
pub mod fixtures;
pub mod fog;
mod input;
mod install;
pub mod instance;
pub mod inventory;
pub mod levels;
pub mod math;
#[cfg(feature = "probes")]
pub mod measure;
mod migrate;
pub mod minigame;
pub mod module;
pub mod moves;
pub mod notice;
pub mod pack;
pub mod pazaak;
/// The bumps pending on the branch, per format (`build.rs`).
mod pending {
    include!(concat!(env!("OUT_DIR"), "/pending.rs"));
}
pub mod flight;
pub mod motion;
pub mod persona;
#[cfg(feature = "probes")]
pub mod probe;
mod rate;
mod replay;
pub mod rng;
mod save;
#[cfg(feature = "probes")]
pub mod scaling;
pub mod script;
pub mod since;
pub mod skeleton;
mod snapshot;
pub mod spawner;
pub mod stealth;
pub mod target;
#[cfg(feature = "probes")]
pub mod trace;
pub mod traps;
pub mod visuals;
pub mod walk;
mod wire;
mod world;

pub use input::{Command, Seat, TickInput, buttons};
pub use install::{Announcement, InstallError, Mismatch, Restamp, WALKER_HALF_WIDTH_M, counts};
pub use instance::Instances;
pub use rate::{Accumulator, MAX_CATCH_UP, TICK_HZ, TickRate};
pub use replay::{Divergence, RecordedTick, Recorder, Recording};
pub use rng::SimRng;
pub use save::{FORMAT as SAVE_FORMAT, Save, ScriptVmState};
pub use snapshot::FORMAT as SNAPSHOT_FORMAT;
pub use snapshot::{
    AreaSound, Busy, Census, Departing, Departure, EntityState, Fade, Feedback, Going, Kill, Kind,
    ObjectState, Parts, Place, Presented, PresentedEntity, Progress, ReadyCheck, RequestWait,
    RoomAnim, Screens, SimId, Snapshot, SnapshotHash, Spawned, Stash, Stashed, Status, StatusKind,
    Track, Transit, Transition, cannot_equip, lock, look, pin, room_key, sound, trap,
};
pub use wire::{DecodeError, Since, Stored};
#[cfg(feature = "probes")]
pub use world::aim_probes;
pub use world::arena;
#[cfg(feature = "probes")]
pub use world::arena_probes;
pub use world::charting;
pub use world::containers;
#[cfg(feature = "probes")]
pub use world::coop_items;
pub use world::coop_talk;
#[cfg(feature = "probes")]
pub use world::destroy_probes as destroys;
pub use world::drivers;
#[cfg(feature = "probes")]
pub use world::effects_probes;
pub use world::falls;
#[cfg(feature = "probes")]
pub use world::feats;
#[cfg(feature = "probes")]
pub use world::floor_probes;
#[cfg(feature = "probes")]
pub use world::force_probes;
#[cfg(feature = "probes")]
pub use world::galaxy;
pub use world::item_uses;
pub use world::locks;
#[cfg(feature = "probes")]
pub use world::minds_probes;
pub use world::objects;
#[cfg(feature = "probes")]
pub use world::pace_probes;
#[cfg(feature = "probes")]
pub use world::pair_probes;
pub use world::party;
#[cfg(feature = "probes")]
pub use world::player_probes;
#[cfg(feature = "probes")]
pub use world::presentation_probes;
#[cfg(feature = "probes")]
pub use world::regen_probes;
pub use world::remains;
#[cfg(feature = "probes")]
pub use world::scene_probes;
#[cfg(feature = "probes")]
pub use world::spawner_probes;
#[cfg(feature = "probes")]
pub use world::speaker_probes;
pub use world::strikes;
pub use world::strikes::player_punch_reach_m;
pub use world::switch;
#[cfg(feature = "probes")]
pub use world::talk_end_probes;
#[cfg(feature = "probes")]
pub use world::tsl_probes;
pub use world::uses;
#[cfg(feature = "probes")]
pub use world::weapon_set_probes;
#[cfg(feature = "probes")]
pub use world::workshop_probes;
pub use world::{
    BODY, Digests, Guests, HEARTBEAT_MS, KeyUse, Level, LevelId, MAX_SEATS, REWIND_MS, SHOT,
    SimWorld, StepReason, UseCame, UseHeard, WorldConfig, bodies_pass, body, hand_muzzle, lent,
    muzzle, party_member, seen_cuts, step,
};
