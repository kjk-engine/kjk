//! Combat: KOTOR's character sheet decides how much, JA decides whether
//! and when (`docs/COMBAT.md`).
//!
//! - `sheet`: the sheet combat reads (static for a fight);
//! - `passive`: what the feats a sheet holds do where their rules apply;
//! - `gear`: what worn items give a sheet beyond weapons and guards;
//! - `fighter`: what changes in a fight, carried in snapshots;
//! - `damage`: the pipeline every hit goes through;
//! - `weapons`: JA's blasters and their cadence;
//! - `specials`: KOTOR's feat attacks as blaster specials, sneak attacks;
//! - `missiles`: bolts in flight;
//! - `mind`: aiming, swinging and looking, as a mind's hands;
//! - `ai`: creatures' minds (I27): tasks, routes, powers;
//! - `balance`: TSL's creature autobalance (`autobalance.2da`);
//! - `saber`: lightsabers and melee (I24);
//! - `force`: the Force (I26);
//! - `forms`: TSL's Forms, lightsaber and Force;
//! - `effects`: KOTOR's effects on JA's powers, and its own effect powers;
//! - `grenades`: thrown with JA's physics, bursting with KOTOR's effects;
//! - `items`: KOTOR's consumables: medpacs, shields, stims, and the pack;
//! - `spells`: the `spells.2da` rows the native rules name, by label.

pub mod ai;
pub mod balance;
pub mod damage;
pub mod death;
pub mod effects;
pub mod fighter;
pub mod force;
pub mod forms;
pub mod gear;
pub mod grenades;
pub mod idle;
pub mod items;
pub mod location;
pub mod mind;
pub mod missiles;
pub mod pace;
pub mod pair;
pub mod passive;
pub mod rounds;
pub mod saber;
pub mod sheet;
pub mod specials;
pub mod spells;
pub mod troop;
pub mod weapons;

/// KOTOR's combat round (ms): a creature's attacks, its AI's turn
/// (`OnEndCombatRound`) and a scene's blow (`CutsceneAttack`, landing at its
/// half) all keep it.
pub const ROUND_MS: u64 = rules::combat::timing::ROUND_MS;

/// How long a character counts as in combat after it attacks or is hit
/// (ms): reone's deactivation delay after its last round.
pub const COMBAT_MS: u64 = rules::combat::timing::COMBAT_MS;

/// What a player's weapon swap takes of a fight (ms): TSL makes it a combat
/// round action (`CSWSCombatRound::AddSwitchWeaponsAction`), the swap at its
/// start; here the weapon waits that long after what it was doing
/// (`Fighter::weapon_wait_ms`). Out of a fight, and for a script's swap, it is
/// instant.
pub const SWAP_MS: i32 = rules::combat::timing::SWAP_MS;

/// A d20 roll against a DC as KOTOR's d20 rules make it (an attack roll
/// against defence, a saving throw against its DC): a natural 20 always
/// succeeds, a natural 1 always fails, else `d20` + `bonus` ≥ `dc`
/// (reone's attack roll; Aurora's saves) [verify: the K1 exe's saves].
pub fn check(d20: i32, bonus: i32, dc: i32) -> bool {
    match d20 {
        20 => true,
        1 => false,
        _ => d20 + bonus >= dc,
    }
}

pub use damage::{Blow, FriendlyFire, Relation, Resolved, Source};
pub use fighter::{Event, Fighter, Life, Location, Look, Outcome, Sheath};
pub use missiles::Missile;
pub use sheet::{Delivery, Sheet, Weapon};
