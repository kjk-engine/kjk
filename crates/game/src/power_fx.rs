//! The Force powers' and the items' sounds, from the presented snapshots:
//! JA's for the powers JA's rules drive, KOTOR's for KOTOR's own powers,
//! items, shields and grenades (`docs/COMBAT.md`, "Sounds"). Each is read
//! off what changed in a character between two snapshots:
//!
//! - Push and pull: a throw's wait starting with its hand out: JA's `push`
//!   or `pull` in the body slot.
//! - Toggles (speed, protect, absorb, rage, sight, the mind trick) starting:
//!   JA's `speed`, `protect`, `absorb`, `rage`, `see`, `distract`; while one
//!   runs its loop is held at the character (`speedloop`, `protectloop`,
//!   `absorbloop`, `rageloop`, `seeloop`; the mind trick has none).
//! - Channels starting: lightning's `lightning`, drain's `drain` (body
//!   slot). Each hit lightning lands sounds one of `lightninghit1–3` one
//!   time in three on the struck; each drain, `drained` there. A gripped
//!   character sounds `grip` where it is every second it is held.
//! - A hit on a protected character: `protecthit`. A Force jump's start:
//!   `jump`. A heal (the power button pressed with Cure or Heal selected,
//!   paid): `heal` in the item slot.
//! - KOTOR's powers JA has no shape for (stun, Valor, Energy Resistance,
//!   Slow, Supress / Breach), cast with their hand out: KOTOR's cast sound
//!   (`spells.2da` `castsound`, `v_useforce`). A hold's own visual and
//!   its sound (a stun's `VFX_DUR_STUN`) are the script effects' view's
//!   (`script_fx`), as the state it stands for shows it.
//! - Items used: medpacs, antidotes and stims KOTOR's `gui_inject`, repair
//!   kits `gui_upgrade` (`castsound`); a shield its visual effect's on
//!   sound, its loop while it lasts, its off sound when it ends (red for
//!   the Sith and Mandalorian shields, blue for the rest).
//! - Grenades, thrown as JA's thermal detonator: JA's sounds for what JA's
//!   physics does and KOTOR has none for (`spells.2da` names no cast or
//!   projectile sound for them): the charge's start `charge` (the arming
//!   click, `EV_WEAPON_CHARGE`) and the throw `fire` in the thrower's
//!   weapon slot; while it lives `thermloop` held where it is, and
//!   `warning` half a second before it bursts. Each bounce and its landing:
//!   KOTOR's `grenadesnd.2da` sound for the surface met
//!   (`weapon_sounds::grenade_land`). Its burst: its visual effect's sound
//!   (`cb_gr_fragment`, …).
//!
//! KOTOR's sounds are named `k1/<resref>` (`sound::load_kotor`).

use std::collections::BTreeMap;

use audio::Channel;
use bevy::prelude::*;
use collision::{Contents, Extents};
use sim::buttons;
use sim::combat::force::{ForceState, Gesture, Power, of_row};
use sim::combat::{Fighter, Missile, Outcome, grenades};
use sim::{EntityState, SimId, Snapshot};

use crate::combat_fx::FightDrawn;
use crate::grenade_view;
use crate::sim_host::{PresentedSnapshot, SceneSwap};
use crate::sound::{Cue, Cues, Hold, Loops};
use crate::weapon_sounds::WeaponSounds;

const UNIT: f32 = asset_ja::UNIT;
const FORCE: &str = "sound/weapons/force/";
/// JA's thermal detonator's sounds.
const CHARGE: &str = "sound/weapons/thermal/charge.wav";
const THROW: &str = "sound/weapons/thermal/fire.wav";
const FUSE_LOOP: &str = "sound/weapons/thermal/thermloop.wav";
const WARNING: &str = "sound/weapons/thermal/warning.wav";
/// How often a gripped character sounds the grip (ms).
const GRIP_EVERY_MS: u64 = 1000;

fn ja(name: &str) -> String {
    format!("{FORCE}{name}")
}

fn kotor(resref: &str) -> String {
    format!("k1/{resref}")
}

/// JA's Force sounds this plays.
pub fn sounds() -> Vec<String> {
    let mut names: Vec<String> = [
        "push.wav",
        "pull.wav",
        "speed.wav",
        "protect.mp3",
        "absorb.mp3",
        "rage.wav",
        "see.wav",
        "distract.wav",
        "speedloop.wav",
        "protectloop.wav",
        "absorbloop.wav",
        "rageloop.wav",
        "seeloop.wav",
        "lightning.wav",
        "drain.wav",
        "drained.mp3",
        "grip.mp3",
        "protecthit.mp3",
        "jump.mp3",
        "heal.wav",
    ]
    .map(ja)
    .to_vec();
    names.extend((1..=3).map(|n| ja(&format!("lightninghit{n}.wav"))));
    names.extend([CHARGE, THROW, FUSE_LOOP, WARNING].map(String::from));
    names
}

/// KOTOR's sounds this plays (resrefs).
pub fn kotor_sounds() -> Vec<&'static str> {
    let mut resrefs = vec![
        "v_useforce",
        "gui_inject",
        "gui_upgrade",
        "gen_shieldbluon",
        "v_dur_shldblue",
        "gen_shieldbluoff",
        "gen_shieldredon",
        "v_dur_shldred",
        "gen_shieldredoff",
    ];
    resrefs.extend(GRENADES.iter().map(|&(_, sound)| sound));
    resrefs
}

/// Each grenade's burst (`spells.2da` row → its visual effect's sound).
const GRENADES: [(u16, &str); 9] = [
    (87, "cb_gr_fragment"),
    (88, "cb_gr_stun"),
    (89, "cb_gr_thermal"),
    (90, "cb_gr_poison"),
    (91, "cb_gr_sonic"),
    (92, "cb_gr_sticky"),
    (93, "cb_gr_cryoban"),
    (94, "cb_gr_plasma"),
    (95, "cb_gr_ion"),
];

/// An item's use (`spells.2da` row): its cast sound, or a shield's on,
/// loop and off sounds.
enum ItemSound {
    Cast(&'static str),
    Shield([&'static str; 3]),
    None,
}

fn item_sound(row: u16) -> ItemSound {
    const BLUE: [&str; 3] = ["gen_shieldbluon", "v_dur_shldblue", "gen_shieldbluoff"];
    const RED: [&str; 3] = ["gen_shieldredon", "v_dur_shldred", "gen_shieldredoff"];
    match row {
        64..=67 | 69..=77 => ItemSound::Cast("gui_inject"),
        84 | 127 | 128 => ItemSound::Cast("gui_upgrade"),
        // The Sith and Mandalorian shields (100, 104) are red.
        100 | 104 => ItemSound::Shield(RED),
        99..=107 => ItemSound::Shield(BLUE),
        _ => ItemSound::None,
    }
}

pub struct PowerFxPlugin;

/// What a character's Force and pack were at its last snapshot.
#[derive(Clone, Copy)]
struct Seen {
    force: ForceState,
    fp: i32,
    last_use: (u16, u64),
    charging_from: Option<u64>,
    shield: Option<u16>,
    next_event: u16,
    /// When a grip on it last sounded.
    grip_sounded: u64,
}

#[derive(Resource, Default)]
struct Powers {
    seen: BTreeMap<SimId, Seen>,
    /// Grenades in flight as last seen, and when.
    lobs: BTreeMap<SimId, Missile>,
    lobs_ms: u64,
    generation: u32,
}

impl Plugin for PowerFxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Powers>().add_systems(
            PostUpdate,
            sound
                // Only in a session (the menu waits without one).
                .run_if(resource_exists::<crate::sim_host::PresentedSnapshot>)
                .after(bevy::transform::TransformSystems::Propagate)
                .before(FightDrawn),
        );
    }
}

fn sound(
    presented: Res<PresentedSnapshot>,
    swap: Res<SceneSwap>,
    mut powers: ResMut<Powers>,
    (mut cues, mut loops): (ResMut<Cues>, ResMut<Loops>),
    tables: Option<Res<WeaponSounds>>,
) {
    let powers = &mut *powers;
    if swap.generation != powers.generation {
        *powers = Powers {
            generation: swap.generation,
            ..Powers::default()
        };
    }
    for snapshot in &presented.1 {
        let channels: BTreeMap<SimId, Power> = snapshot
            .entities
            .iter()
            .filter_map(|e| Some((e.id, e.fighter?.force.channel?.power)))
            .collect();
        for entity in &snapshot.entities {
            changes(powers, entity, snapshot, &channels, &mut cues.0);
        }
        grenades(
            powers,
            snapshot,
            (&swap.level, tables.as_deref()),
            &mut cues.0,
        );
    }
    let current = presented.0.current();
    for entity in &current.entities {
        if let Some(missile) = entity.missile.filter(|m| m.lob.is_some()) {
            let at = Vec3::from_array(missile.at(current.time_ms));
            loops.0.push(Hold::at(FUSE_LOOP.to_string(), at));
        }
    }
    for entity in &current.entities {
        let Some(f) = entity.fighter.filter(Fighter::alive) else {
            continue;
        };
        let at = place(entity);
        let now = current.time_ms;
        let force = f.force;
        let running = [
            (force.speed_until, "speedloop.wav"),
            (force.protect_until, "protectloop.wav"),
            (force.absorb_until, "absorbloop.wav"),
            (force.rage_until, "rageloop.wav"),
            (force.sight_until, "seeloop.wav"),
        ];
        for (until, name) in running {
            if until > now {
                loops.0.push(Hold::at(ja(name), at));
            }
        }
        if let Some(shield) = f.pack.shield.filter(|s| s.until > now)
            && let ItemSound::Shield([_, hum, _]) = item_sound(shield.row)
        {
            loops.0.push(Hold::at(kotor(hum), at));
        }
    }
}

fn place(entity: &EntityState) -> Vec3 {
    Vec3::from_array(entity.position) / UNIT
}

/// A sound of `entity`'s in `channel`, going with it.
fn on(entity: &EntityState, name: String, channel: Channel, ms: f32) -> Cue {
    Cue {
        entity: Some(entity.id),
        channel,
        follows: true,
        ..Cue::at(name, place(entity), ms)
    }
}

/// A sound where `entity` is now, as the game's predefined sounds (`G_PreDefSound`).
fn at(entity: &EntityState, name: String, ms: f32) -> Cue {
    Cue {
        entity: Some(entity.id),
        ..Cue::at(name, place(entity), ms)
    }
}

/// Whether a push (`false`) or a pull (`true`) was cast between `was` and
/// `now`: the throws' wait starting, with its hand out.
pub fn thrown(was: &ForceState, now: &ForceState) -> Option<bool> {
    match now.hand {
        Some((gesture @ (Gesture::Push | Gesture::Pull), _))
            if now.push_pull_ready > was.push_pull_ready =>
        {
            Some(gesture == Gesture::Pull)
        }
        _ => None,
    }
}

fn started(until: u64, before: u64, now: u64) -> bool {
    until > now && before <= now && until != before
}

/// What changed for one character since its last snapshot.
fn changes(
    powers: &mut Powers,
    entity: &EntityState,
    snapshot: &Snapshot,
    channels: &BTreeMap<SimId, Power>,
    cues: &mut Vec<Cue>,
) {
    let Some(f) = entity.fighter else { return };
    let now = snapshot.time_ms;
    let ms = now as f32;
    let force = f.force;
    let mut seen = Seen {
        force,
        fp: f.fp,
        last_use: f.pack.last,
        charging_from: f.pack.charging_from,
        shield: f.pack.shield.filter(|s| s.until > now).map(|s| s.row),
        next_event: f.next_event,
        grip_sounded: 0,
    };
    let Some(before) = powers.seen.get(&entity.id).copied() else {
        powers.seen.insert(entity.id, seen);
        return;
    };
    seen.grip_sounded = before.grip_sounded;
    let was = before.force;
    let mut rng = crate::view_rng::ja_seed(entity.id.0, snapshot.tick as u32);
    let mut cast = false;

    // Push and pull.
    if let Some(pull) = thrown(&was, &force) {
        let name = if pull { "pull.wav" } else { "push.wav" };
        cues.push(on(entity, ja(name), Channel::Body, ms));
        cast = true;
    }
    // Toggles.
    let toggles = [
        (
            force.speed_until,
            was.speed_until,
            "speed.wav",
            Channel::Body,
        ),
        (force.sight_until, was.sight_until, "see.wav", Channel::Auto),
        (
            force.trick_until,
            was.trick_until,
            "distract.wav",
            Channel::Auto,
        ),
        (force.rage_until, was.rage_until, "rage.wav", Channel::Auto),
    ];
    for (until, before_until, name, channel) in toggles {
        if started(until, before_until, now) {
            cues.push(on(entity, ja(name), channel, ms));
            cast = true;
        }
    }
    for (until, before_until, name) in [
        (force.protect_until, was.protect_until, "protect.mp3"),
        (force.absorb_until, was.absorb_until, "absorb.mp3"),
    ] {
        if started(until, before_until, now) {
            cues.push(at(entity, ja(name), ms));
            cast = true;
        }
    }
    // Channels.
    let channel = force.channel.map(|c| (c.power, c.from));
    if let Some((power, from)) = channel
        && was.channel.map(|c| (c.power, c.from)) != Some((power, from))
    {
        let name = match power {
            Power::Lightning => Some("lightning.wav"),
            Power::Drain => Some("drain.wav"),
            _ => None,
        };
        if let Some(name) = name {
            cues.push(on(entity, ja(name), Channel::Body, ms));
        }
        cast = true;
    }
    // Gripped.
    if force.gripped_by.is_some() && now >= seen.grip_sounded + GRIP_EVERY_MS {
        cues.push(at(entity, ja("grip.mp3"), ms));
        seen.grip_sounded = now;
    }
    // A Force jump.
    if force.jumping && !was.jumping {
        cues.push(at(entity, ja("jump.mp3"), ms));
    }
    // KOTOR's own powers: a hand out, paid for, with nothing else to show
    // for it (a counter's hand is not paid).
    let hand_new = force.hand.map(|(_, until)| until) != was.hand.map(|(_, until)| until)
        && force.hand.is_some_and(|(_, until)| until > now);
    if hand_new && !cast && f.fp < before.fp {
        cues.push(on(entity, kotor("v_useforce"), Channel::Auto, ms));
        cast = true;
    }
    // A heal: the power button pressed with a heal selected, paid. The
    // bar pays for more than powers (a saber's or a blaster's special, a
    // throw, protect's absorbing), so its fall alone is no heal.
    let pressed = force.held & !was.held & buttons::FORCE_POWER != 0;
    let healing =
        of_row(u16::from(f.last_command.force)).is_some_and(|(power, _)| power == Power::Heal);
    if f.fp < before.fp && pressed && healing && !cast {
        cues.push(on(entity, ja("heal.wav"), Channel::Item, ms));
    }
    // Hits on it: lightning, drain, protect.
    let fresh = f.next_event.wrapping_sub(before.next_event);
    for event in f.events.iter().flatten() {
        let struck = matches!(
            event.result,
            Outcome::Hit | Outcome::Critical | Outcome::Automatic
        );
        if event.sequence.wrapping_sub(before.next_event) >= fresh || !struck {
            continue;
        }
        match event.by.and_then(|by| channels.get(&by)) {
            Some(Power::Lightning) if event.weapon.is_none() => {
                if rng.roll_between(0, 2) == 0 {
                    let name = format!("lightninghit{}.wav", rng.roll_between(1, 3));
                    cues.push(on(entity, ja(&name), Channel::Body, ms));
                }
            }
            Some(Power::Drain) if event.weapon.is_none() => {
                cues.push(at(entity, ja("drained.mp3"), ms));
            }
            _ => {}
        }
        if force.protect_until > now {
            cues.push(at(entity, ja("protecthit.mp3"), ms));
        }
    }
    // A grenade's charge and throw.
    if seen.charging_from.is_some() && seen.charging_from != before.charging_from {
        cues.push(on(entity, CHARGE.to_string(), Channel::Weapon, ms));
    }
    // Items.
    if seen.last_use != before.last_use && seen.last_use.1 != 0 {
        if grenades::effect(seen.last_use.0).is_some() {
            cues.push(on(entity, THROW.to_string(), Channel::Weapon, ms));
        }
        match item_sound(seen.last_use.0) {
            ItemSound::Cast(resref) => cues.push(on(entity, kotor(resref), Channel::Item, ms)),
            ItemSound::Shield([on_sound, ..]) => {
                cues.push(on(entity, kotor(on_sound), Channel::Item, ms));
            }
            ItemSound::None => {}
        }
    }
    if let Some(row) = before.shield
        && seen.shield.is_none()
        && let ItemSound::Shield([.., off]) = item_sound(row)
    {
        cues.push(on(entity, kotor(off), Channel::Item, ms));
    }
    powers.seen.insert(entity.id, seen);
}

/// The grenades in `snapshot`: one that bounced or came to rest since
/// the last lands there, one whose warning came sounds it, and one gone
/// burst where it was last seen.
fn grenades(
    powers: &mut Powers,
    snapshot: &Snapshot,
    (level, tables): (&sim::Level, Option<&WeaponSounds>),
    cues: &mut Vec<Cue>,
) {
    let now = snapshot.time_ms;
    let ms = now as f32;
    let flying: BTreeMap<SimId, Missile> = snapshot
        .entities
        .iter()
        .filter_map(|e| Some((e.id, e.missile.filter(|m| m.lob.is_some())?)))
        .collect();
    for (id, missile) in &flying {
        let at = Vec3::from_array(missile.at(now));
        let warning = missile.until_ms.saturating_sub(grenade_view::WARNING_MS);
        if (powers.lobs_ms..now).contains(&warning) {
            cues.push(Cue::at(WARNING.to_string(), at, ms));
        }
        let Some(was) = powers.lobs.get(id) else {
            continue;
        };
        if was.from_ms == missile.from_ms {
            continue;
        }
        // The surface met: along the way it came in, through where it
        // bounced.
        let base = Vec3::from_array(missile.base);
        let way = Vec3::from_array(was.velocity_at(missile.from_ms)).normalize_or(Vec3::NEG_Z);
        let states = level.states_in(snapshot);
        let met = level.with(states.as_deref(), |world| {
            world.trace(
                ((base - way * 2.0) * UNIT).to_array(),
                ((base + way * 2.0 * grenades::HALF_SIZE) * UNIT).to_array(),
                &Extents::POINT,
                Contents::SOLID,
            )
        });
        let material = if met.fraction < 1.0 {
            met.surface.material
        } else {
            0
        };
        if let Some(name) = tables.and_then(|tables| tables.grenade_land(material)) {
            cues.push(Cue::at(name, base, ms));
        }
    }
    for (id, missile) in &powers.lobs {
        if flying.contains_key(id) {
            continue;
        }
        let row = missile.lob.map_or(0, |lob| lob.row);
        if let Some(&(_, sound)) = GRENADES.iter().find(|(r, _)| *r == row) {
            let at = Vec3::from_array(missile.at(powers.lobs_ms));
            cues.push(Cue::at(kotor(sound), at, ms));
        }
    }
    powers.lobs = flying;
    powers.lobs_ms = now;
}
