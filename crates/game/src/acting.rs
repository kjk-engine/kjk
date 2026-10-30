//! KOTOR's scripted animations on a retargeted body: what a script's
//! `ActionPlayAnimation` plays (`sim::combat::idle::Acting`), drawn as the
//! body's own KOTOR clip over its JA pose.
//!
//! - The clip for an `ANIMATION_…` constant has a name that the scripts' body
//!   models, other than a creature model, play it under, the
//!   table the sim times them by (`clips`); those left unnamed are
//!   not drawn. A droid's or a beast's own clips (`cast::Clips`) take a
//!   creature model's `c…` name first (`viewer::scripted_clips`).
//! - A looping one loops for the script's time, a fire-and-forget one
//!   plays once to its end; either blends in from the JA pose and back out
//!   over the clip's transition time.
//! - Moving, fighting or dying breaks it off (while a conversation holds
//!   the world only dying does: the sim's queue ends it, S7), and a
//!   script's stopping or replacing it ends it; it fades out from there.
//! - A death in place (`sim::combat::death::in_place`: in a scene, to a
//!   story's blow) plays the body's own dying clip (`die`, a creature
//!   model's `cdie`) where it stands and holds its last frame: the body lies
//!   as it ends, not as a ragdoll (`cast::dying_in_place`).
//! - A round's blow (`sim::script::effects::blows`) draws the
//!   `animations.2da` rows the sim chose (`sim::combat::rounds`, as K1's
//!   executable picks them): the attacker's swing (`c3a2`, `g1a1`), its
//!   target's answer (`c3d2`, `g2r1`), each its row's own clip
//!   (`stance_of`). Such a clip plays to its end whatever the body does,
//!   a fight included.
//! - Seats' bodies draw what the engine plays on them: KOTOR's animations
//!   for disarming, recovering and setting a mine (`sim::traps::animations`)
//!   and for Security on a door or a container (`sim::locks::animations`).
//! - A grenade's charge and throw draw KOTOR's `throwgren` over the upper
//!   body (`Throw`): the hand drawn back while the button is held, brought
//!   over the head as the grenade leaves, then the clip played out.
//! - While one `animations.2da` marks `hideequippeditems` plays (an
//!   injection, Security, a mine's), the weapons in hand are not drawn
//!   (`hides_items`, `weapons::armed`), as KOTOR has them.

use std::sync::Arc;

use asset_odyssey::anim::AnimationIr;
use bevy::prelude::*;
use sim::combat::death::DIE;
use sim::combat::idle::{Acting, FIRE_AND_FORGET};
use sim::script::effects::blows;

use crate::scene::Animator;

/// A wield's ready stance, by wield number added (`ready`); a creature
/// model's own one (`creadyr`) at `READY` itself.
pub const READY: u16 = 4000;
/// `ANIMATION_LOOPING_READY` (a line's `AnimList` 10148, the conversations'
/// `Ready`): no clip of its own, it plays its body's ready stance, as reone
/// names it (by the creature's wield type):
/// `g<wield>r1`, a creature model's `creadyr` (`stance_of`).
pub const LOOPING_READY: u16 = 19;
/// How long (ms) after an animation begins its body's motion does not
/// break it off: the stop of a run that brought it there.
const SETTLE_MS: f64 = 400.0;

/// What the engine plays on a body that scripts do not, and the clip's
/// name: a skill's use on a mine (`sim::traps::animations`) and on a lock
/// (`sim::locks::animations`), a grenade's throw, and the dying clip of a
/// death in place (`sim::combat::death::in_place`).
const ENGINE_CLIPS: &[(u16, &str)] = &[
    (sim::traps::animations::DISARM, "disablemine"),
    (sim::traps::animations::RECOVER, "disablemine"),
    (sim::traps::animations::SET, "setmine"),
    (sim::locks::animations::DOOR, "unlockdr"),
    (sim::locks::animations::PLACEABLE, "unlockcntr"),
    (THROW, "throwgren"),
    (DIE, "die"),
    (WHIRLWIND, "whirlwind"),
];

/// The constants a body has a clip for, and the clip's name: the scripts'
/// `ANIMATION_…` constants and the conversations' own clips
/// (`asset_odyssey::creature::scripted_clips`, the table the sim's lengths
/// are read by) and the engine's own.
pub fn clips() -> impl Iterator<Item = (u16, &'static str)> {
    asset_odyssey::creature::scripted_clips().chain(ENGINE_CLIPS.iter().copied())
}

/// Whether `animation` plays its clip once to its end, whatever its time:
/// a fire-and-forget constant or an engine clip below
/// `creature::LOOPED`; the rest loop for their time.
fn plays_once(animation: u16) -> bool {
    (FIRE_AND_FORGET..asset_odyssey::creature::LOOPED).contains(&animation)
}

/// A whirlwind's victim spinning (`EffectWhirlWind`), `animations.2da`
/// row 75 `whirlwind`, which the table marks looping.
pub const WHIRLWIND: u16 = asset_odyssey::creature::LOOPED + 75;

/// KOTOR's grenade throw (`spells.2da` rows 87–95: `conjanim` and
/// `castanim` `throw`), `animations.2da` row 57 `throwgren`, 1.5 s: drawn
/// over the upper body as a grenade is charged and thrown (`Throw`).
pub const THROW: u16 = 10_057;
/// The node the throw is drawn under: the arms, the head and the upper
/// spine; the legs and the torso's aim stay JA's.
const THROW_ROOT: &str = "torsoupr_g";
/// In `throwgren` (s): the hand drawn back, held while the charge lasts,
/// and the hand coming over the head, where the grenade leaves it.
const THROW_HELD: f32 = 0.55;
const THROW_RELEASE: f32 = 0.82;
/// How long the charge takes to draw the hand back, and the release to
/// bring it over (ms; the clip's own pace is 550 and 270): the sim throws
/// on the button's release, so the arm comes over as the grenade leaves.
const WIND_UP_MS: f64 = 450.0;
const WHIP_MS: f64 = 90.0;
/// How quickly the throw takes over the pose (ms).
const THROW_FADE_IN_MS: f64 = 100.0;

/// The ready stances (`READY` + wield number, `g<wield>r1`): reone's
/// in-combat pause of each wield; with `creature` (a creature model's own
/// names, `creature::creature_model`), its own `creadyr` at `READY`.
pub fn ready_names(creature: bool) -> impl Iterator<Item = (u16, String)> {
    let own = creature.then(|| (READY, "creadyr".to_string()));
    own.into_iter()
        .chain((1..=9).map(|wield| (READY + wield, format!("g{wield}r1"))))
}

/// A wield's draw (`DRAW` + wield number, `g<wield>w1`, the exe's
/// animation 10246 by its table of wield types: rows 91, 132, 173, 214,
/// 222, 236, 244).
pub const DRAW: u16 = 4100;

/// The draws (`DRAW` + wield number, `g<wield>w1`).
pub fn draw_names() -> impl Iterator<Item = (u16, String)> {
    (1..=9).map(|wield| (DRAW + wield, format!("g{wield}w1")))
}

/// A body's flinch from damage that comes with no weapon (a script's
/// `EffectDamage`, a power's, a blast's): the exe's damage handler plays
/// the damage clip of the body's wield (`FLINCH` + wield number,
/// `g<wield>d1`), a creature model's own `cdamages` (`FLINCH` itself),
/// as a weapon's attack plays its own answer instead (its round's). Its
/// spasm for electrical and dark-side damage is not drawn: a hit's event
/// does not say its damage's type.
pub const FLINCH: u16 = 4200;

/// The flinches (`FLINCH` + wield number, `g<wield>d1`); with `creature`,
/// a creature model's own `cdamages` at `FLINCH`.
pub fn flinch_names(creature: bool) -> impl Iterator<Item = (u16, String)> {
    let own = creature.then(|| (FLINCH, "cdamages".to_string()));
    own.into_iter()
        .chain((1..=9).map(|wield| (FLINCH + wield, format!("g{wield}d1"))))
}

/// A clip the engine plays once over a body's pose as something befalls
/// it, the sim not waiting for it:
///
/// - **a draw**, as its weapons change in its hands: the exe's equip
///   handler, as a weapon slot's item changes (a script's
///   `ActionEquipItem`, the AI's pick), sets the slot (the weapon hangs in
///   the hand at once) and plays the new wield's draw where the body has
///   one (`DRAW`); an unarmed body has none;
/// - **a flinch** from damage with no weapon (`FLINCH`, `Flinch`).
#[derive(Default, Clone, Copy)]
pub struct Once {
    /// Begun, not yet shown; and when (sim ms) it was first shown.
    pending: bool,
    from: Option<f64>,
}

impl Once {
    /// The clip plays from the next frame shown.
    pub fn begin(&mut self) {
        self.pending = true;
    }

    /// Begun and not yet played out.
    fn busy(&self) -> bool {
        self.pending || self.from.is_some()
    }

    /// The clip at `ms` (sim ms), the first of `clips` the body has: how
    /// far in and how much over the pose; none once it has played.
    pub fn shown<'a>(
        &mut self,
        bound: &'a Bound,
        ms: f64,
        clips: impl IntoIterator<Item = u16>,
    ) -> Option<(&'a Animator, f32, f32)> {
        if std::mem::take(&mut self.pending) {
            self.from = Some(ms);
        }
        let from = self.from?;
        let found = clips
            .into_iter()
            .find_map(|wanted| bound.iter().find(|(a, _)| *a == wanted));
        let Some((_, clip)) = found else {
            self.from = None;
            return None;
        };
        let at = ((ms - from) / 1000.0).max(0.0) as f32;
        let length = clip.length();
        if at >= length {
            self.from = None;
            return None;
        }
        let fade = clip.transition().max(f32::EPSILON);
        let weight = (at / fade).min((length - at) / fade).clamp(0.0, 1.0);
        (weight > 0.0).then_some((clip, at, weight))
    }
}

/// The clips a body of wield number `wield` draws (`Once::shown`), and
/// flinches with (the wield's, else a creature model's own).
pub fn draw_of(wield: u8) -> [u16; 1] {
    [DRAW + u16::from(wield)]
}

fn flinch_of(wield: u8) -> [u16; 2] {
    [FLINCH + u16::from(wield), FLINCH]
}

/// A body's flinches (`FLINCH`): the hits it took last seen (its fighter's
/// next event), and the clip.
#[derive(Default, Clone, Copy)]
pub struct Flinch {
    seen: Option<u16>,
    once: Once,
    /// How many it has begun, for dumps.
    pub count: u16,
}

impl Flinch {
    /// Follows `fighter` (as its later snapshot has it): a hit new since it
    /// was last followed (none the first time) with no weapon that it takes
    /// damage from, standing, flinches it, unless JA's own pain plays on
    /// it already (`pained`: the sim's flinch, drawn on a JA pose). With
    /// `weapons` (a creature model, which has no JA pain) a weapon's hit
    /// flinches it too: its damage clip, as reone's attack plays on its target.
    pub fn follow(&mut self, fighter: Option<&sim::combat::Fighter>, pained: bool, weapons: bool) {
        use sim::combat::Outcome;
        let Some(fighter) = fighter else {
            self.seen = None;
            return;
        };
        let Some(last) = self.seen.replace(fighter.next_event) else {
            return;
        };
        let fresh = fighter.next_event.wrapping_sub(last);
        let hit = fresh <= u16::MAX / 2
            && fighter.alive()
            && fighter.hp > 0
            && fighter.newest().take(usize::from(fresh)).any(|event| {
                (weapons || event.weapon.is_none())
                    && event.damage > 0
                    && matches!(
                        event.result,
                        Outcome::Hit | Outcome::Critical | Outcome::Automatic
                    )
            });
        if hit && !pained {
            self.count = self.count.wrapping_add(1);
            self.once.begin();
        }
    }

    /// The flinch's clip at `ms` (sim ms) for wield number `wield`.
    pub fn shown<'a>(
        &mut self,
        bound: &'a Bound,
        ms: f64,
        wield: u8,
    ) -> Option<(&'a Animator, f32, f32)> {
        self.once.shown(bound, ms, flinch_of(wield))
    }
}

/// A creature model's attacks, as reone names them for one
/// (`Creature::getAnimationName`, outside a scene's round): its own swing,
/// bite or gore (`ATTACK` + 1 and + 2, `g0a1` and `g0a2`), and its gun's
/// shot (`ATTACK` + 3, `b0a1`).
pub const ATTACK: u16 = 4300;

/// The attacks (`ATTACK` + 1 to + 3).
pub fn attack_names() -> impl Iterator<Item = (u16, String)> {
    ["g0a1", "g0a2", "b0a1"]
        .into_iter()
        .zip(1..)
        .map(|(name, n)| (ATTACK + n, name.to_string()))
}

/// The attack clips a creature model of wield number `wield` plays for its
/// `count`th attack, the first bound one shown: a gun's shot, else its own
/// two in turn [verify: reone always swings `g0a1`; the exe's choice
/// between them], else the first.
fn attack_of(wield: u8, count: u8) -> [u16; 2] {
    match wield {
        5 | 6 | 7 | 9 => [ATTACK + 3, ATTACK + 1],
        _ => [ATTACK + 1 + u16::from(count.wrapping_add(1) % 2), ATTACK + 1],
    }
}

/// A creature model's attack (droids, beasts: no JA swing draws it): a rise
/// of its weapon timer (`Fighter::weapon_wait_ms`) is an attack the sim
/// began (a beast's strike, `sim::world::strikes`, a gun's shot, a blade's
/// move), and plays its attack clip (`attack_of`) once over its pose; one
/// begun while the last still plays waits for the next.
#[derive(Default, Clone, Copy)]
pub struct Attack {
    timer: Option<i32>,
    /// How many it has begun (its clip's turn; for dumps).
    pub count: u8,
    once: Once,
}

impl Attack {
    /// Follows `fighter` (as its later snapshot has it).
    pub fn follow(&mut self, fighter: Option<&sim::combat::Fighter>) {
        let Some(fighter) = fighter else {
            self.timer = None;
            return;
        };
        let before = self.timer.replace(fighter.weapon_wait_ms);
        let begun = before.is_some_and(|before| fighter.weapon_wait_ms > before);
        if begun && fighter.alive() && !self.once.busy() {
            self.count = self.count.wrapping_add(1);
            self.once.begin();
        }
    }

    /// The attack's clip at `ms` (sim ms) for wield number `wield`.
    pub fn shown<'a>(
        &mut self,
        bound: &'a Bound,
        ms: f64,
        wield: u8,
    ) -> Option<(&'a Animator, f32, f32)> {
        self.once.shown(bound, ms, attack_of(wield, self.count))
    }
}

/// The attack's probes (`kjk sim probes --only creature_attack`): a rise of the
/// weapon timer begins one, the first look and a fall do not; a gun shoots
/// `b0a1`, bare hands alternate `g0a1` and `g0a2`.
#[cfg(feature = "probes")]
pub fn attack_probes() -> sim::probe::Checks {
    use sim::combat::Fighter;
    use sim::probe::check;
    let mut attack = Attack::default();
    let mut fighter = Fighter::fresh(10, 0);
    // The pending begun after each look at a timer.
    let mut look = |timer: i32| {
        fighter.weapon_wait_ms = timer;
        attack.follow(Some(&fighter));
        std::mem::take(&mut attack.once.pending)
    };
    let looks = [look(1500), look(900), look(2400)];
    let names: Vec<(u16, String)> = attack_names().collect();
    vec![
        (
            "a rising weapon timer begins an attack, not the first look or a fall",
            check(looks == [false, false, true], format!("{looks:?}")),
        ),
        (
            "a creature model's attacks: g0a1 and g0a2 in turn, a gun's b0a1",
            check(
                names[2] == (ATTACK + 3, "b0a1".to_string())
                    && attack_of(8, 1) == [ATTACK + 1, ATTACK + 1]
                    && attack_of(8, 2) == [ATTACK + 2, ATTACK + 1]
                    && attack_of(7, 1) == [ATTACK + 3, ATTACK + 1],
                format!("{names:?}"),
            ),
        ),
    ]
}

/// The flinch's probes (`kjk sim probes --only flinch`): a hit with no weapon
/// flinches a body that stands, once, and a weapon's a creature model; a
/// weapon's on a humanoid, a refused one, the first look, JA's own pain and a
/// death do not.
#[cfg(feature = "probes")]
pub fn flinch_probes() -> sim::probe::Checks {
    use sim::combat::{Delivery, Event, Fighter, Location, Outcome};
    use sim::probe::check;
    let event = |weapon, result, damage| Event {
        sequence: 0,
        tick: 0,
        by: None,
        result,
        location: Location::None,
        damage,
        weapon,
        at: None,
        visual: None,
    };
    // Whether the hit `event` brings flinches, after a first look; on a
    // creature model with `weapons`.
    let flinches = |event: Event, pained: bool, hp: i32, weapons: bool| {
        let mut fighter = Fighter::fresh(10, 0);
        let mut flinch = Flinch::default();
        flinch.follow(Some(&fighter), false, weapons);
        fighter.record(event);
        fighter.hp = hp;
        flinch.follow(Some(&fighter), pained, weapons);
        let begun = flinch.once.pending;
        flinch.follow(Some(&fighter), false, weapons);
        (begun, flinch.once.pending)
    };
    let script = event(None, Outcome::Hit, 300);
    let blade = event(Some(Delivery::Melee), Outcome::Hit, 300);
    let cases = [
        ("script", flinches(script, false, 700, false), (true, true)),
        ("blade", flinches(blade, false, 700, false), (false, false)),
        ("blade on a beast", flinches(blade, false, 700, true), (true, true)),
        (
            "refused",
            flinches(event(None, Outcome::Refused, 0), false, 1000, true),
            (false, false),
        ),
        ("pained", flinches(script, true, 700, false), (false, false)),
        ("killed", flinches(script, false, 0, true), (false, false)),
    ];
    let mut first = Flinch::default();
    let mut fighter = Fighter::fresh(10, 0);
    fighter.record(script);
    first.follow(Some(&fighter), false, false);
    let names: Vec<(u16, String)> = flinch_names(true).take(3).collect();
    vec![
        (
            "a hit with no weapon flinches a standing body, once",
            check(
                cases.iter().all(|(_, got, want)| got == want) && !first.once.pending,
                format!(
                    "{:?}; first look {}",
                    cases.map(|(name, got, _)| (name, got)),
                    first.once.pending
                ),
            ),
        ),
        (
            "the wield's damage clip, a creature model's own",
            check(
                names
                    == [
                        (FLINCH, "cdamages".to_string()),
                        (FLINCH + 1, "g1d1".to_string()),
                        (FLINCH + 2, "g2d1".to_string()),
                    ]
                    && flinch_of(4) == [FLINCH + 4, FLINCH],
                format!("{names:?}"),
            ),
        ),
    ]
}

/// The bound clips `animation` plays on a body of wield number `wield`
/// (`stance::wield`), the first bound one shown: `LOOPING_READY` the
/// wield's ready stance, else a creature model's own; any other its own.
pub fn stance_of(animation: u16, wield: u8) -> [Option<u16>; 2] {
    use asset_odyssey::creature::row_animation;
    match (animation, blows::of(animation)) {
        (LOOPING_READY, _) => [Some(READY + u16::from(wield)), Some(READY)],
        (_, Some((_, row))) => [
            Some(row_animation(row, false)),
            Some(row_animation(row, true)),
        ],
        _ => [Some(animation), None],
    }
}

/// The clips among `clips` a character's equipped items are hidden for
/// while it plays them: those `animations.2da` marks `hideequippeditems`
/// (the same in K1 and TSL; `kjk script security` checks the table).
pub const HIDES_ITEMS: &[&str] = &["inject", "unlockdr", "unlockcntr", "disablemine", "setmine"];

/// Whether `acting` hides its character's equipped items at `ms` (sim ms):
/// it plays one of `HIDES_ITEMS`.
pub fn hides_items(acting: &Acting, ms: f64) -> bool {
    acting.playing(ms as u64)
        && clips()
            .any(|(animation, name)| animation == acting.animation && HIDES_ITEMS.contains(&name))
}

/// A body's scripted clips, bound to its nodes, by constant.
pub type Bound = Vec<(u16, Animator)>;

/// The loaded clips bound to a body's nodes (`scene::animator`); the throw
/// to its upper body only.
pub fn bind(
    placed: &crate::viewer::Placed,
    clips: &[(u16, Arc<AnimationIr>)],
    nodes: &[Entity],
) -> Bound {
    clips
        .iter()
        .filter_map(|(animation, clip)| {
            let upper = (*animation == THROW).then(|| {
                Arc::new(AnimationIr {
                    root: Some(THROW_ROOT.to_string()),
                    ..(**clip).clone()
                })
            });
            let clip = upper.as_ref().unwrap_or(clip);
            Some((*animation, crate::scene::animator(placed, clip, nodes)?))
        })
        .collect()
}

/// A character's grenade throw as its body shows it (`THROW`), from its
/// snapshots: the charge (`Pack::charging_from`) draws the hand back and
/// holds it there; the throw (`Pack::last`, a grenade's row and when)
/// brings it over the head as the grenade leaves, and the clip plays out.
#[derive(Default, Clone, Copy)]
pub struct Throw {
    /// The charge last seen begin (sim ms), and whether it still runs.
    from: Option<u64>,
    charging: bool,
    /// The throw last seen (sim ms).
    thrown: Option<u64>,
}

impl Throw {
    /// Follows `fighter` (as its later snapshot has it); a fallen one
    /// throws nothing.
    pub fn follow(&mut self, fighter: Option<&sim::combat::Fighter>) {
        let Some(fighter) = fighter.filter(|f| f.alive()) else {
            *self = Throw::default();
            return;
        };
        let pack = &fighter.pack;
        self.charging = pack.charging_from.is_some();
        if pack.charging_from.is_some() {
            self.from = pack.charging_from;
        }
        let (row, at) = pack.last;
        if at > 0 && sim::combat::grenades::effect(row).is_some() {
            self.thrown = Some(at);
        }
    }

    /// The throw's clip (`bound`'s `THROW`) at `ms` (sim ms): how far in and
    /// how much over the pose.
    pub fn shown<'a>(&self, bound: &'a Bound, ms: f64) -> Option<(&'a Animator, f32, f32)> {
        let (_, clip) = bound.iter().find(|(a, _)| *a == THROW)?;
        let held = |charged: f64| THROW_HELD * (charged / WIND_UP_MS).clamp(0.0, 1.0) as f32;
        let thrown = self
            .thrown
            .filter(|&at| self.from.is_none_or(|from| at >= from) && ms >= at as f64);
        let (from, at) = match (thrown, self.from) {
            (Some(at), from) => {
                let from = from.unwrap_or(at) as f64;
                let since = ms - at as f64;
                let reached = held(at as f64 - from);
                let at = if since < WHIP_MS {
                    reached + (THROW_RELEASE - reached) * (since / WHIP_MS) as f32
                } else {
                    THROW_RELEASE + ((since - WHIP_MS) / 1000.0) as f32
                };
                (from, at)
            }
            (None, Some(from)) if self.charging && ms >= from as f64 => {
                (from as f64, held(ms - from as f64))
            }
            _ => return None,
        };
        let length = clip.length();
        let fade_in = ((ms - from) / THROW_FADE_IN_MS).clamp(0.0, 1.0) as f32;
        let fade_out = ((length - at) / clip.transition()).clamp(0.0, 1.0);
        let weight = fade_in.min(fade_out);
        (weight > 0.0).then_some((clip, at.min(length), weight))
    }
}

/// What a body shows of scripts' animations: the one its character plays,
/// and the one that played before, fading out; each with when (sim ms)
/// it was broken off, if it was.
#[derive(Default)]
pub struct Shows {
    playing: Option<(Acting, Option<f64>)>,
    fading: Option<(Acting, f64)>,
    /// The one playing when its events were last read, and how far in
    /// (seconds; `passed`).
    heard: Option<(Acting, f32)>,
    /// When (sim ms) the body was first followed: a clip begun before it
    /// (a save loaded mid-clip) is heard from there on, not from its start.
    first: Option<f64>,
}

impl Shows {
    /// Follows the character at `ms`: what it plays now, and whether it
    /// stands still out of a fight, alive (else what plays breaks off; a
    /// scene's melee blow is broken off only by its character dying,
    /// `alive`, and so is anything while a conversation holds the world,
    /// `held`, where the sim's queue ends it, S7). One that is replaced or
    /// stopped fades out from then.
    pub fn follow(
        &mut self,
        acting: Option<Acting>,
        (still, alive, held): (bool, bool, bool),
        ms: f64,
    ) {
        self.first.get_or_insert(ms);
        if self.playing.map(|(was, _)| was) != acting {
            if let Some((was, broke)) = self.playing.take() {
                self.fading = Some((was, broke.map_or(ms, |broke| broke.min(ms))));
            }
            self.playing = acting.map(|acting| (acting, None));
        }
        // A body that arrives from a run as it begins (a walk to a mine
        // ends in its skill's use) settles first.
        if let Some((acting, broke @ None)) = &mut self.playing
            && acting.animation != DIE
            && !(if held || blows::of(acting.animation).is_some() {
                alive
            } else {
                still
            })
            && (held || ms > acting.since as f64 + SETTLE_MS)
        {
            *broke = Some(ms);
        }
    }

    /// The events (its MDL's, by name) the clip of the animation it plays
    /// (by the character's wield number `wield`) passed since they were
    /// last read, at `ms`: from its start for one begun since (`heard_from`),
    /// none once it is no longer shown.
    pub fn passed<'a>(&mut self, bound: &'a Bound, ms: f64, wield: u8) -> Vec<&'a str> {
        let shown = self.playing.and_then(|(acting, broke)| {
            let (animator, at, _) = shown(bound, (&acting, wield), ms, broke)?;
            Some((acting, animator, at))
        });
        let Some((acting, animator, at)) = shown else {
            self.heard = None;
            return Vec::new();
        };
        let from = heard_from(self.heard.replace((acting, at)), (acting, at), self.first);
        if from == at {
            return Vec::new();
        }
        crate::model_fx::passed(animator.clip(), from, at).collect()
    }

    /// The clips shown at `ms` (`shown`, by the character's wield number
    /// `wield`), the one fading out first; in a fight (`combat`: its own
    /// melee wield number) a character whose last clip was a round's holds
    /// its wield's ready stance once that clip ends (`ready`), under it as
    /// it fades.
    pub fn shown<'a>(
        &self,
        bound: &'a Bound,
        ms: f64,
        (wield, combat): (u8, Option<u8>),
    ) -> impl Iterator<Item = (&'a Animator, f32, f32)> {
        let ready = combat.and_then(|wield| {
            let (acting, broke) = self.playing?;
            ready(bound, (&acting, broke), wield, ms)
        });
        let fading = self
            .fading
            .and_then(|(acting, broke)| shown(bound, (&acting, wield), ms, Some(broke)));
        let playing = self
            .playing
            .and_then(|(acting, broke)| shown(bound, (&acting, wield), ms, broke));
        ready.into_iter().chain(fading).chain(playing)
    }
}

/// Where (seconds in) the events of `acting`'s clip, now `at` in, are heard
/// from, the clip `heard` last as it was: from there for the same one, from
/// its start for one begun since the body was `first` followed (sim ms),
/// else from `at`: a clip already playing when the body was first seen (a
/// save loaded in the middle of it) cues only what comes after.
pub fn heard_from(
    heard: Option<(Acting, f32)>,
    (acting, at): (Acting, f32),
    first: Option<f64>,
) -> f32 {
    match heard {
        Some((was, from)) if was == acting => from,
        _ if first.is_some_and(|first| (acting.since as f64) < first) => at,
        _ => 0.0,
    }
}

/// What a body of wield number `wield` shows of a scripted animation at
/// `ms` (sim ms): its clip (`stance_of`), how far in (seconds), and how
/// much of it over the JA pose (0 to 1). `broke`: when (sim ms) it was
/// broken off, if it was: it fades out from then.
fn shown<'a>(
    bound: &'a Bound,
    (acting, wield): (&Acting, u8),
    ms: f64,
    broke: Option<f64>,
) -> Option<(&'a Animator, f32, f32)> {
    let animator = stance_of(acting.animation, wield)
        .into_iter()
        .flatten()
        .find_map(|wanted| bound.iter().find(|(a, _)| *a == wanted))
        .map(|(_, animator)| animator)?;
    let since = acting.since as f64;
    let seconds = ((ms - since) / 1000.0).max(0.0) as f32;
    let length = animator.length();
    let end = if plays_once(acting.animation) {
        since + f64::from(length) * 1000.0
    } else {
        since + f64::from(acting.ms)
    };
    let fade = f64::from(animator.transition()) * 1000.0;
    // A dying clip is held at its end.
    let end = if acting.animation == DIE {
        f64::INFINITY
    } else {
        end
    };
    let end = broke.map_or(end, |broke| end.min(broke + fade));
    let weight = ((ms - since) / fade).min((end - ms) / fade).clamp(0.0, 1.0) as f32;
    let at = if plays_once(acting.animation) {
        seconds.min(length)
    } else {
        seconds % length
    };
    (weight > 0.0).then_some((animator, at, weight))
}

/// The ready stance (`g<wield>r1`, the pause pose reone holds in combat)
/// a character holds after a round's clip (`blows`): its own wield's. It
/// fades in over its
/// transition time before the clip ends and loops from there; none once
/// the clip is broken off (the character died). Its clips are `READY` plus
/// the wield number.
fn ready<'a>(
    bound: &'a Bound,
    (acting, broke): (&Acting, Option<f64>),
    wield: u8,
    ms: f64,
) -> Option<(&'a Animator, f32, f32)> {
    if broke.is_some() {
        return None;
    }
    blows::of(acting.animation)?;
    let clip = stance_of(acting.animation, wield)
        .into_iter()
        .flatten()
        .find_map(|wanted| bound.iter().find(|(a, _)| *a == wanted))
        .map(|(_, clip)| clip)?;
    let (_, stance) = bound.iter().find(|(a, _)| *a == READY + u16::from(wield))?;
    let end = acting.since as f64 + f64::from(clip.length()) * 1000.0;
    let fade = f64::from(stance.transition()) * 1000.0;
    let weight = ((ms - (end - fade)) / fade).clamp(0.0, 1.0) as f32;
    let at = (((ms - end).max(0.0) / 1000.0) as f32) % stance.length();
    (weight > 0.0).then_some((stance, at, weight))
}
