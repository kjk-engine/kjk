//! What changes about a character in a fight: its hit points and second
//! bar (Force or stamina points; hundredths, `docs/COMBAT.md`,
//! C7), whether it lives, its weapon's timer, the timers pain and
//! combat keep, who hurt it last, and a short ring of what happened to it,
//! which render and scripts read.
//! Carried whole in every snapshot.

use crate::combat::ai::Mind;
use crate::combat::effects::Effects;
use crate::combat::force::ForceState;
use crate::combat::idle::{Acting, Rest};
use crate::combat::items::Pack;
use crate::combat::saber::SaberState;
use crate::combat::saber::blades::{Posed, Trail};
use crate::combat::saber::grace::{self, Grace};
use crate::combat::saber::sweep::Swing;
use crate::combat::sheet::{Delivery, Sheet};
use crate::combat::specials::Shooting;
use crate::input::Command;
use crate::snapshot::SimId;
use crate::wire::{DecodeError, Reader, Writer, read_fixed, write_fixed};

/// Whether a character acts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Life {
    Alive,
    /// Its death animation plays; it takes no commands.
    Dead,
    /// A party member at 0 HP (I30 P11): it lies as the dead do and takes
    /// no commands, until the fight ends or a player revives it
    /// (`Fighter::fallen`).
    Downed,
}

/// A downed party member's fall: when, and who is reviving it (I30 P11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fallen {
    /// Sim ms it went down.
    pub since_ms: u64,
    pub revive: Option<Revive>,
}

/// A player holding use on a downed party member (`world::falls`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Revive {
    pub by: SimId,
    /// Sim ms the hold began, and how long it must last (ms).
    pub from_ms: u64,
    pub needs_ms: u32,
}

impl Revive {
    const BLANK: Revive = Revive {
        by: SimId(0),
        from_ms: 0,
        needs_ms: 0,
    };

    /// How far along it is at `now_ms`, 0 to 1: what views show.
    pub fn done(&self, now_ms: u64) -> f32 {
        let held = now_ms.saturating_sub(self.from_ms) as f32;
        (held / self.needs_ms.max(1) as f32).min(1.0)
    }

    fn encode(&self, out: &mut Writer) {
        out.u32(self.by.0);
        out.u64(self.from_ms);
        out.u32(self.needs_ms);
    }

    fn decode(input: &mut Reader) -> Result<Revive, DecodeError> {
        Ok(Revive {
            by: SimId(input.u32()?),
            from_ms: input.u64()?,
            needs_ms: input.u32()?,
        })
    }
}

impl Fallen {
    const BLANK: Fallen = Fallen {
        since_ms: 0,
        revive: None,
    };

    fn encode(&self, out: &mut Writer) {
        out.u64(self.since_ms);
        write_fixed(out, self.revive.as_ref(), &Revive::BLANK, Revive::encode);
    }

    fn decode(input: &mut Reader) -> Result<Fallen, DecodeError> {
        Ok(Fallen {
            since_ms: input.u64()?,
            revive: read_fixed(input, &Revive::BLANK, Revive::decode, "revive flag")?,
        })
    }
}

/// Where a hit landed (JA's hit locations; I23 3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Location {
    None = 0,
    Head,
    Chest,
    ChestLeft,
    ChestRight,
    Back,
    BackLeft,
    BackRight,
    Waist,
    ArmLeft,
    ArmRight,
    HandLeft,
    HandRight,
    LegLeft,
    LegRight,
    FootLeft,
    FootRight,
}

impl Location {
    /// Every location, in index order.
    pub const ALL: [Location; 17] = [
        Location::None,
        Location::Head,
        Location::Chest,
        Location::ChestLeft,
        Location::ChestRight,
        Location::Back,
        Location::BackLeft,
        Location::BackRight,
        Location::Waist,
        Location::ArmLeft,
        Location::ArmRight,
        Location::HandLeft,
        Location::HandRight,
        Location::LegLeft,
        Location::LegRight,
        Location::FootLeft,
        Location::FootRight,
    ];

    /// JA's damage multiplier for the location, in thousandths
    /// (`rules::combat::location`).
    pub fn per_mille(self) -> i64 {
        use rules::combat::location;
        match self {
            Location::Head => location::HEAD,
            Location::ArmLeft | Location::ArmRight => location::ARM,
            Location::LegLeft | Location::LegRight => location::LEG,
            Location::HandLeft | Location::HandRight => location::HAND,
            Location::FootLeft | Location::FootRight => location::FOOT,
            _ => location::BODY,
        }
    }

    fn from_index(index: u8) -> Option<Location> {
        Location::ALL.get(usize::from(index)).copied()
    }
}

/// How a hit ended (KOTOR's attack results, as `GetLastAttackResult`
/// reads them; I22 section 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Hit = 1,
    Critical = 2,
    /// `EffectAssuredHit`.
    Automatic = 3,
    /// Blocked by a saber.
    Parried = 4,
    /// A bolt deflected or reflected.
    Deflected = 5,
    /// It could not harm the target (friendly fire off, plot, neutral).
    Refused = 6,
    /// A script's visual effect on it (`Event::visual`): nothing struck.
    Visual = 7,
}

/// A script's visual effect (`EffectVisualEffect`, `EffectBeam`): its
/// `visualeffects.2da` row, how long it lasts (ms; 0 once), and for a beam
/// the body part it reaches from its source (`Event::by`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Visual {
    pub row: u16,
    pub ms: u32,
    pub beam: bool,
    pub part: u8,
}

impl Visual {
    const BLANK: Visual = Visual {
        row: 0,
        ms: 0,
        beam: false,
        part: 0,
    };

    pub(crate) fn encode(&self, out: &mut Writer) {
        out.u16(self.row);
        out.u32(self.ms);
        out.u8(u8::from(self.beam));
        out.u8(self.part);
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Visual, DecodeError> {
        Ok(Visual {
            row: input.u16()?,
            ms: input.u32()?,
            beam: input.u8()? != 0,
            part: input.u8()?,
        })
    }
}

impl Outcome {
    fn from_index(index: u8) -> Option<Outcome> {
        Some(match index {
            1 => Outcome::Hit,
            2 => Outcome::Critical,
            3 => Outcome::Automatic,
            4 => Outcome::Parried,
            5 => Outcome::Deflected,
            6 => Outcome::Refused,
            7 => Outcome::Visual,
            _ => return None,
        })
    }
}

/// One thing that happened to a character, for render (sparks, flinches,
/// numbers) and scripts (`GetLastDamager`, `GetTotalDamageDealt`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Event {
    /// Counts up by one per event (wrapping): a view sees which it drew.
    pub sequence: u16,
    pub tick: u64,
    /// Who did it; `None` for the world or a script.
    pub by: Option<SimId>,
    pub result: Outcome,
    pub location: Location,
    /// Hundredths of an HP taken.
    pub damage: i32,
    /// What struck: a weapon's delivery (a bolt's blaster, a blade's, a
    /// fist's), also what was turned (a parried blade, a deflected bolt);
    /// none for a power, a blast or a script. What a view plays (a
    /// blaster's impact, a blade's sparks, a block's flare).
    pub weapon: Option<Delivery>,
    /// Where a block happened, in whole units: where the blades met, or
    /// the bolt was turned (what a view plays its flare at); none for hits.
    /// A visual effect's place, if it was put at one.
    pub at: Option<[i32; 3]>,
    /// A script's visual effect (`Outcome::Visual`).
    pub visual: Option<Visual>,
}

/// How many events a fighter keeps: enough for a shotgun's shards and a
/// crit in one step.
pub const EVENTS: usize = 8;

impl Event {
    /// What an empty slot holds on the wire.
    const BLANK: Event = Event {
        sequence: 0,
        tick: 0,
        by: None,
        result: Outcome::Hit,
        location: Location::None,
        damage: 0,
        weapon: None,
        at: None,
        visual: None,
    };

    /// `point` (units) as an event keeps it.
    pub fn units(point: [f32; 3]) -> [i32; 3] {
        point.map(|v| v.round() as i32)
    }

    /// A visual effect shown at step `tick`, by `by`, at `at` (units) if
    /// put at a place (`Outcome::Visual`).
    pub fn visual(visual: Visual, by: Option<SimId>, at: Option<[i32; 3]>, tick: u64) -> Event {
        Event {
            tick,
            by,
            result: Outcome::Visual,
            at,
            visual: Some(visual),
            ..Event::BLANK
        }
    }

    /// Where it happened (units), if it says.
    pub fn point(&self) -> Option<[f32; 3]> {
        self.at.map(|at| at.map(|v| v as f32))
    }

    fn encode(&self, out: &mut Writer) {
        out.u16(self.sequence);
        out.u64(self.tick);
        write_fixed(out, self.by.as_ref(), &SimId(0), encode_id);
        out.u8(self.result as u8);
        out.u8(self.location as u8);
        out.u32(self.damage as u32);
        write_fixed(out, self.weapon.as_ref(), &Delivery::Unarmed, |d, out| {
            out.u8(d.index())
        });
        write_fixed(out, self.at.as_ref(), &[0; 3], |at, out| {
            for v in at {
                out.u32(*v as u32);
            }
        });
        write_fixed(out, self.visual.as_ref(), &Visual::BLANK, Visual::encode);
    }

    fn decode(input: &mut Reader) -> Result<Event, DecodeError> {
        let sequence = input.u16()?;
        let tick = input.u64()?;
        let by = read_fixed(input, &SimId(0), decode_id, "event source flag")?;
        let at = input.at();
        let result = Outcome::from_index(input.u8()?).ok_or(input.invalid("event result", at))?;
        let at = input.at();
        let location =
            Location::from_index(input.u8()?).ok_or(input.invalid("event location", at))?;
        Ok(Event {
            sequence,
            tick,
            by,
            result,
            location,
            damage: input.u32()? as i32,
            weapon: read_fixed(
                input,
                &Delivery::Unarmed,
                decode_delivery,
                "event weapon flag",
            )?,
            at: read_fixed(
                input,
                &[0; 3],
                |input| {
                    Ok([
                        input.u32()? as i32,
                        input.u32()? as i32,
                        input.u32()? as i32,
                    ])
                },
                "event point flag",
            )?,
            visual: read_fixed(input, &Visual::BLANK, Visual::decode, "event visual flag")?,
        })
    }
}

fn decode_delivery(input: &mut Reader) -> Result<Delivery, DecodeError> {
    let at = input.at();
    Delivery::from_index(input.u8()?).ok_or(input.invalid("event weapon", at))
}

/// See the module documentation. Its encoding has one width whatever it
/// holds (absent values are written blank behind their flag), so a
/// snapshot's bytes stay in place from one tick to the next and the
/// network's XOR deltas of them stay small (`net::codec`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fighter {
    /// Hundredths of an HP; the sheet has the maximum.
    pub hp: i32,
    /// Hundredths of a point of the second bar (`Sheet::pool`): FP for a
    /// Force user, stamina for anyone else.
    pub fp: i32,
    pub life: Life,
    /// Milliseconds before the weapon may act again (`weaponTime`).
    pub weapon_wait_ms: i32,
    /// Which hand's weapon is out: 0 right, 1 left (or a second saber).
    pub hand: u8,
    /// Sim ms until which it counts as in combat (`COMBAT_MS` after the last
    /// hit or attack, reone's rule).
    pub combat_until: u64,
    /// Sim ms until which a pain animation plays: a creature does nothing
    /// new meanwhile (JA's pain debounce).
    pub pain_until: u64,
    /// The last to hurt it, and when (sim ms).
    pub last_attacker: Option<(SimId, u64)>,
    /// A faction set at run time (`ChangeToStandardFaction`), else the
    /// sheet's.
    pub faction: Option<u8>,
    /// Who it is fighting (`GetAttackTarget`); a creature's mind keeps it.
    pub target: Option<SimId>,
    /// The last command it acted on: what prediction repeats for a
    /// character whose mind runs on the authority only.
    pub last_command: Command,
    /// A ring: event `n` in slot `n % EVENTS`; a view compares sequence
    /// numbers to see which it has not drawn.
    pub events: [Option<Event>; EVENTS],
    /// The next event's sequence number.
    pub next_event: u16,
    /// Its saber or melee weapon (`combat::saber`).
    pub saber: SaberState,
    /// Its Force (`combat::force`).
    pub force: ForceState,
    /// A creature's mind (`combat::ai`); a player's stays idle.
    pub mind: Mind,
    /// Its blaster's feat special under way, and the penalties of the last
    /// (`combat::specials`).
    pub shooting: Shooting,
    /// What it used of what it carries, and what still runs on it
    /// (`combat::items`: a throw's charge, a heal, a shield, a stim, and
    /// poison and weakness from grenades).
    pub pack: Pack,
    /// What the server's copy of its skeleton plays, where its blades were
    /// last tick, and what its attack under way owes (I24,
    /// `combat::saber::blades`, `sweep`).
    pub pose: Posed,
    pub trails: [Option<Trail>; 2],
    pub swing: Swing,
    /// KOTOR's effects on it (`combat::effects`: valor, energy
    /// resistance, Slow's chain).
    pub effects: Effects,
    /// Blade hits held for it while its late view catches up (I25, C19,
    /// `combat::saber::grace`): a remote player's only.
    pub grace: Grace,
    /// Standing at rest, toward an idle animation (`combat::idle`).
    pub rest: Rest,
    /// What a script has it play (`combat::idle::Acting`).
    pub acting: Option<Acting>,
    /// Down, if it is (`Life::Downed`, I30 P11): since when, and who
    /// revives it.
    pub fallen: Option<Fallen>,
    /// Stealth and the skill buttons (`Covert`).
    pub covert: Covert,
    /// Who says whether its weapon is out (`Sheath`).
    pub sheath: Sheath,
    /// TSL's fake combat (the executable's combat reason 2): sim ms until
    /// which it stands in combat without a fight, weapon out, no combat
    /// round (`HELD`: until a script ends it, `SetFakeCombatState`; 8 s
    /// after a flourish, `CreatureFlourishWeapon`). A real fight ends it.
    pub fake_until: u64,
    /// When its weapon may flourish again (sim ms; the executable's 1 s).
    pub flourish_ready: u64,
    /// What scripts made of its body for good (`Look`).
    pub look: Look,
}

/// What scripts made of a body for good (I35): TSL's `ChangeObjectAppearance`
/// puts an `appearance.2da` row in place of its template's (its stats'
/// appearance, kept with it); `DuplicateHeadAppearance` gives it another
/// creature's head (a `heads.2da` row, kept as KOTOR keeps it, a byte).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Look {
    pub appearance: Option<u16>,
    pub head: Option<u8>,
}

impl Look {
    fn encode(&self, out: &mut Writer) {
        write_fixed(out, self.appearance.as_ref(), &0, |row, out| out.u16(*row));
        write_fixed(out, self.head.as_ref(), &0, |row, out| out.u8(*row));
    }

    fn decode(input: &mut Reader) -> Result<Look, DecodeError> {
        Ok(Look {
            appearance: read_fixed(input, &0, |input| input.u16(), "appearance flag")?,
            head: read_fixed(input, &0, |input| input.u8(), "head flag")?,
        })
    }
}

/// `Fighter::fake_until` held until a script lets go.
pub const HELD: u64 = u64::MAX;

/// Who says whether a fighter's weapon is out: KOTOR's own rule, its
/// player, or a script.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Sheath {
    /// The game: out in a fight, put away `COMBAT_MS` after it, as
    /// KOTOR shows weapons only in a fight.
    #[default]
    Fight,
    /// Its player put it away (the holster key, `Action::Holster`): away
    /// even in a fight, until it attacks or changes weapons.
    Holstered,
    /// A script holds its lightsaber lit (`SetLightsaberPowered` with its
    /// override) until it lets go.
    Lit,
    /// Its player asked to put it away (the holster key), its weapon step
    /// not yet done with it (a swing finishing): the put-away plays (a
    /// blade's flourish, a gun lowered), then `Holstered`.
    Holstering,
}

impl Sheath {
    fn code(self) -> u8 {
        self as u8
    }

    fn decode(input: &mut Reader) -> Result<Sheath, DecodeError> {
        let at = input.at();
        Ok(match input.u8()? {
            0 => Sheath::Fight,
            1 => Sheath::Holstered,
            2 => Sheath::Lit,
            3 => Sheath::Holstering,
            _ => return Err(input.invalid("sheath", at)),
        })
    }
}

/// Stealth and the skill buttons: whether in stealth mode, the skill
/// buttons held on the last step (a use on a press), and until when an
/// attack that ended stealth still lands as its opener (sim ms,
/// `stealth::OPENER_MS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Covert {
    pub stealth: bool,
    pub held: u16,
    pub opener_until: u64,
}

impl Covert {
    fn encode(&self, out: &mut Writer) {
        out.u8(u8::from(self.stealth));
        out.u16(self.held);
        out.u64(self.opener_until);
    }

    fn decode(input: &mut Reader) -> Result<Covert, DecodeError> {
        let at = input.at();
        let stealth = match input.u8()? {
            0 => false,
            1 => true,
            _ => return Err(input.invalid("stealth", at)),
        };
        Ok(Covert {
            stealth,
            held: input.u16()?,
            opener_until: input.u64()?,
        })
    }
}

impl Fighter {
    /// At full health with `max_hp` and `max_fp` (whole points).
    pub fn fresh(max_hp: u16, max_fp: u16) -> Fighter {
        Fighter {
            hp: i32::from(max_hp) * 100,
            fp: i32::from(max_fp) * 100,
            life: Life::Alive,
            weapon_wait_ms: 0,
            hand: 0,
            combat_until: 0,
            pain_until: 0,
            last_attacker: None,
            faction: None,
            target: None,
            last_command: Command::default(),
            events: [None; EVENTS],
            next_event: 0,
            saber: SaberState::default(),
            force: ForceState::default(),
            mind: Mind::default(),
            shooting: Shooting::default(),
            pack: Pack::default(),
            pose: Posed::default(),
            trails: [None; 2],
            swing: Swing::default(),
            effects: Effects::default(),
            grace: [None; grace::HELD],
            rest: Rest::default(),
            acting: None,
            fallen: None,
            covert: Covert::default(),
            sheath: Sheath::Fight,
            fake_until: 0,
            flourish_ready: 0,
            look: Look::default(),
        }
    }

    /// At full health and a full bar by `sheet`.
    pub fn of(sheet: &Sheet) -> Fighter {
        Fighter::fresh(sheet.max_hp, sheet.pool())
    }

    pub fn alive(&self) -> bool {
        self.life == Life::Alive
    }

    /// Short of full by what a template's current hit points are under its
    /// most (`module::ObjectData::hit_points`: a creature placed or made
    /// hurt), never below 1.
    pub fn hurt_as(mut self, hit_points: Option<(i32, i32)>) -> Fighter {
        if let Some((now, most)) = hit_points {
            let short = most.saturating_sub(now).max(0).saturating_mul(100);
            self.hp = self.hp.saturating_sub(short).max(100);
        }
        self
    }

    /// Its hit points whole, as KOTOR counts them (its scripts and
    /// screens): rounded up while it stands, so one standing with part of a
    /// point left (JA's shares of damage leave hundredths) has 1, never the
    /// 0 a script reads as dead; as they are once it does not stand.
    pub fn whole_hp(&self) -> i32 {
        if self.alive() {
            (self.hp + 99).div_euclid(100).max(1)
        } else {
            self.hp / 100
        }
    }

    /// Whether its weapon is put away at `now` (sim ms) by who says so
    /// (`Sheath`): its player's holster, or the game's rule (no one it
    /// fights, `COMBAT_MS` past its last blow and its own last attack); a script's
    /// lit saber is out.
    pub fn weapon_away(&self, now: u64) -> bool {
        match self.sheath {
            Sheath::Holstered | Sheath::Holstering => true,
            Sheath::Lit => false,
            Sheath::Fight => {
                self.target.is_none()
                    && self.combat_until <= now
                    && self.saber.out_until <= now
                    && !self.faking(now)
            }
        }
    }

    /// In TSL's fake combat at `now` (`fake_until`).
    pub fn faking(&self, now: u64) -> bool {
        self.fake_until > now
    }

    /// In combat at `now`: a real fight (`COMBAT_MS` past its last blow) or a fake
    /// one.
    pub fn in_combat(&self, now: u64) -> bool {
        self.combat_until > now || self.faking(now)
    }

    /// A party member down (I30 P11).
    pub fn downed(&self) -> bool {
        self.life == Life::Downed
    }

    /// Debilitated at `now`, as KOTOR's `CSWSCreature::IsDebilitated` has
    /// it (its portrait's `LBL_DEBILATATED`): not standing, held (C11's
    /// hold), or under a KOTOR state but confusion (a creature's or a
    /// droid's, which leave it acting).
    pub fn debilitated(&self, now: u64) -> bool {
        use super::effects::state;
        let force = &self.force;
        !self.alive()
            || force.stunned_until > now
            || (force.state_until > now
                && !matches!(force.state, 0 | state::CONFUSED | state::DROID_CONFUSED))
    }

    /// Dead: no getting up.
    pub fn dead(&self) -> bool {
        self.life == Life::Dead
    }

    /// Goes down at `now` (I30 P11): no hit points left, the blade out, no
    /// one it fights.
    pub(crate) fn fall(&mut self, now: u64) {
        self.life = Life::Downed;
        self.hp = 0;
        self.fallen = Some(Fallen {
            since_ms: now,
            revive: None,
        });
        self.saber.on = false;
        self.target = None;
    }

    /// Up again with at least `hp` hundredths (I30 P11).
    pub(crate) fn rise(&mut self, hp: i32) {
        self.life = Life::Alive;
        self.hp = self.hp.max(hp);
        self.fallen = None;
    }

    /// Records `event` (its sequence number is set here) over the oldest.
    pub fn record(&mut self, mut event: Event) {
        event.sequence = self.next_event;
        self.next_event = self.next_event.wrapping_add(1);
        self.events[usize::from(event.sequence) % EVENTS] = Some(event);
    }

    /// The events newest first.
    pub fn newest(&self) -> impl Iterator<Item = &Event> {
        let next = usize::from(self.next_event);
        (1..=EVENTS)
            .filter_map(move |back| self.events[(next + EVENTS * 2 - back) % EVENTS].as_ref())
    }

    pub(crate) fn encode(&self, out: &mut Writer) {
        out.u32(self.hp as u32);
        out.u32(self.fp as u32);
        out.u8(match self.life {
            Life::Alive => 0,
            Life::Dead => 1,
            Life::Downed => 2,
        });
        out.u32(self.weapon_wait_ms as u32);
        out.u8(self.hand);
        out.u64(self.combat_until);
        out.u64(self.pain_until);
        write_fixed(
            out,
            self.last_attacker.as_ref(),
            &(SimId(0), 0),
            |(id, at), out| {
                out.u32(id.0);
                out.u64(*at);
            },
        );
        write_fixed(out, self.faction.as_ref(), &0, |faction, out| {
            out.u8(*faction)
        });
        write_fixed(out, self.target.as_ref(), &SimId(0), encode_id);
        encode_command(&self.last_command, out);
        for event in &self.events {
            write_fixed(out, event.as_ref(), &Event::BLANK, Event::encode);
        }
        out.u16(self.next_event);
        self.saber.encode(out);
        self.force.encode(out);
        self.mind.encode(out);
        self.shooting.encode(out);
        self.pack.encode(out);
        self.pose.encode(out);
        Trail::encode_pair(&self.trails, out);
        self.swing.encode(out);
        self.effects.encode(out);
        grace::encode(&self.grace, out);
        self.rest.encode(out);
        write_fixed(
            out,
            self.acting.as_ref(),
            &Acting::default(),
            Acting::encode,
        );
        write_fixed(out, self.fallen.as_ref(), &Fallen::BLANK, Fallen::encode);
        self.covert.encode(out);
        out.u8(self.sheath.code());
        out.u64(self.fake_until);
        out.u64(self.flourish_ready);
        self.look.encode(out);
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Fighter, DecodeError> {
        let hp = input.u32()? as i32;
        let fp = input.u32()? as i32;
        let at = input.at();
        let life = match input.u8()? {
            0 => Life::Alive,
            1 => Life::Dead,
            2 => Life::Downed,
            _ => return Err(input.invalid("life", at)),
        };
        let weapon_wait_ms = input.u32()? as i32;
        let hand = input.u8()?;
        let combat_until = input.u64()?;
        let pain_until = input.u64()?;
        let last_attacker = read_fixed(
            input,
            &(SimId(0), 0),
            |input| Ok((SimId(input.u32()?), input.u64()?)),
            "last attacker flag",
        )?;
        let faction = read_fixed(input, &0, |input| input.u8(), "faction flag")?;
        let target = read_fixed(input, &SimId(0), decode_id, "target flag")?;
        let last_command = decode_command(input)?;
        let mut events = [None; EVENTS];
        for slot in &mut events {
            *slot = read_fixed(input, &Event::BLANK, Event::decode, "event flag")?;
        }
        let next_event = input.u16()?;
        let saber = SaberState::decode(input)?;
        let force = ForceState::decode(input)?;
        let mind = Mind::decode(input)?;
        let shooting = Shooting::decode(input)?;
        let pack = Pack::decode(input)?;
        let pose = Posed::decode(input)?;
        let trails = Trail::decode_pair(input)?;
        let swing = Swing::decode(input)?;
        let effects = Effects::decode(input)?;
        let grace = grace::decode(input)?;
        let rest = Rest::decode(input)?;
        let acting = read_fixed(input, &Acting::default(), Acting::decode, "acting flag")?;
        let at = input.at();
        let fallen = read_fixed(input, &Fallen::BLANK, Fallen::decode, "fallen flag")?;
        if (life == Life::Downed) != fallen.is_some() {
            return Err(input.invalid("fallen", at));
        }
        let covert = Covert::decode(input)?;
        let sheath = Sheath::decode(input)?;
        let fake_until = input.u64()?;
        let flourish_ready = input.u64()?;
        let look = Look::decode(input)?;
        Ok(Fighter {
            hp,
            fp,
            life,
            weapon_wait_ms,
            hand,
            combat_until,
            pain_until,
            last_attacker,
            faction,
            target,
            last_command,
            events,
            next_event,
            saber,
            force,
            mind,
            shooting,
            pack,
            pose,
            trails,
            swing,
            effects,
            grace,
            rest,
            acting,
            fallen,
            covert,
            sheath,
            fake_until,
            flourish_ready,
            look,
        })
    }
}

pub(crate) fn encode_id(id: &SimId, out: &mut Writer) {
    out.u32(id.0);
}

pub(crate) fn decode_id(input: &mut Reader) -> Result<SimId, DecodeError> {
    Ok(SimId(input.u32()?))
}

pub(crate) fn encode_command(command: &Command, out: &mut Writer) {
    for angle in command.angles {
        out.u16(angle as u16);
    }
    out.i8(command.forward);
    out.i8(command.right);
    out.i8(command.up);
    out.u16(command.buttons);
    out.u8(command.weapon);
    out.u8(command.force);
    out.u8(command.reply);
    for step in command.aim {
        out.i8(step);
    }
}

pub(crate) fn decode_command(input: &mut Reader) -> Result<Command, DecodeError> {
    let mut angles = [0i16; 3];
    for angle in &mut angles {
        *angle = input.u16()? as i16;
    }
    Ok(Command {
        angles,
        forward: input.i8()?,
        right: input.i8()?,
        up: input.i8()?,
        buttons: input.u16()?,
        weapon: input.u8()?,
        force: input.u8()?,
        reply: input.u8()?,
        aim: [input.i8()?, input.i8()?, input.i8()?],
    })
}
