//! Creatures' minds (`docs/COMBAT.md`: a mind turns
//! an intent into commands, as a player's hands would, and submits them
//! like a player). The layering of iw4L's bots: **observe** (on its look
//! ticks, what it can see: `world::minds`), **choose a task**, **route**
//! (`sim::walk` over the level's navigation mesh) and **act** (aim, fire,
//! swing, cast):
//!
//! - **Engage** a visible hostile (the one it had, else the nearest):
//!   gunners stand and fire, moving to another spot only when their target
//!   goes out of sight (`combat::troop`); saber and
//!   melee fighters route to it and swing within reach, a blade wielder
//!   paced as JA's Jedi (`combat::pace`); Force users cast what their
//!   scripts choose (`k_ai_master`'s talents), or on a stage without
//!   scripts what fits (`choose_power`).
//! - **Chase** to where it last saw its target, for `CHASE_MS`.
//! - **Follow** its leader (a companion of a seat) when nothing else calls,
//!   and come back to it before straying `LEASH` away.
//! - **Idle** otherwise: stand, looking where it looks.
//!
//! The state is fixed-size and rides in the snapshot (`Fighter::mind`);
//! everything a mind draws comes from a hash of the time and its id, not a
//! stream. Minds run on the authority only; prediction repeats each
//! creature's last command.

use crate::combat::Fighter;
use crate::combat::force::{self, Power};
use crate::combat::pace::Pace;
use crate::combat::saber::Style;
use crate::combat::sheet::{Sheet, feats};
use crate::combat::specials::{self, Special};
use crate::combat::troop::Troop;
use crate::combat::{grenades, items};
use crate::input::{Seat, buttons};
use crate::math::Vec3;
use crate::walk::Walker;
use crate::wire::{DecodeError, Reader, Writer, read_fixed, write_fixed};
use rules::minds::powers;
use rules::minds::tasks;

/// How long a mind chases the last place it saw its target (ms): as long
/// as a fight lasts past its last blow (`COMBAT_MS`) [verify].
pub const CHASE_MS: u64 = crate::combat::COMBAT_MS;
/// How far a companion strays from its leader before it goes back (units),
/// and how near it keeps when following.
pub const LEASH: f32 = tasks::LEASH;
pub const FOLLOW_NEAR: f32 = tasks::FOLLOW_NEAR;
/// A melee fighter routes (rather than walking straight) beyond this.
pub const ROUTE_BEYOND: f32 = tasks::ROUTE_BEYOND;
/// A melee fighter closing in farther than this (units) bears off to a
/// flank (`world::minds`).
pub const FLANK_FROM: f32 = tasks::FLANK_FROM;

/// What a mind is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Task {
    #[default]
    Idle,
    Engage,
    Chase,
    Follow,
}

const TASKS: [Task; 4] = [Task::Idle, Task::Engage, Task::Chase, Task::Follow];

/// A mind's state (see the module documentation).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Mind {
    pub task: Task,
    /// The seat it follows and fights for (a companion), if any.
    pub leader: Option<Seat>,
    /// The seat whose player drives it (a companion a guest plays, I30 P2):
    /// it moves by that seat's command instead of its own choices.
    pub driver: Option<Seat>,
    /// Driven, moved this step by its script or a conversation it is in, not
    /// its driver (`world::minds::think`, on the authority): it acts on its
    /// mind's command, and a client predicts it so, as a creature.
    pub steered: bool,
    /// Its script queue holds something (an action, a move, an attack), as
    /// the authority's scripts left it this step (`world::minds`): what a
    /// client, which runs no scripts, knows of it. A body with nothing
    /// queued may be pushed aside (`walk::Met`).
    pub queued: bool,
    /// A script's move is under way for it (`script::actions::Goal`), as
    /// the authority's scripts left it: it walks at KOTOR's pace.
    pub moving: bool,
    /// Where it is walking to, if anywhere.
    pub walker: Option<Walker>,
    /// Where it last saw its target, and when (sim ms).
    pub last_seen: Option<(Vec3, u64)>,
    /// No new power before this (sim ms).
    pub cast_after: u64,
    /// Force buttons it holds for a channel, until when, and the power
    /// selected meanwhile.
    pub hold: u16,
    pub hold_until: u64,
    pub selected: u8,
    /// When it is aware of the foe it last spotted (sim ms): until then it
    /// is caught unaware (sneak-attackable) and has not begun to fight
    /// (I22 section 5's Awareness, `reaction_ms`).
    pub aware_at: u64,
    /// A blade wielder's pacing in a fight (`combat::pace`).
    pub pace: Pace,
    /// A gunner's (`combat::troop`).
    pub troop: Troop,
}

impl Mind {
    /// A companion of `seat`.
    pub fn companion(seat: Seat) -> Mind {
        Mind {
            leader: Some(seat),
            ..Mind::default()
        }
    }

    /// Walks to `goal`: a new walk if it had none or the goal moved more
    /// than `moved` units (seen from above) from the one it had.
    pub fn walk_to(&mut self, goal: Vec3, moved: f32) -> &mut Walker {
        let stale = self.walker.is_none_or(|walker| {
            let d = [walker.goal[0] - goal[0], walker.goal[1] - goal[1]];
            d[0] * d[0] + d[1] * d[1] > moved * moved
        });
        if stale {
            self.walker = Some(Walker::new(goal));
        }
        self.walker.as_mut().expect("set above")
    }

    pub(crate) fn encode(&self, out: &mut Writer) {
        out.u8(self.task as u8);
        for seat in [self.leader, self.driver] {
            write_fixed(out, seat.as_ref(), &Seat(0), |seat, out| out.u8(seat.0));
        }
        // Its flags in one byte: steered, queued, moving.
        out.u8(u8::from(self.steered) | u8::from(self.queued) << 1 | u8::from(self.moving) << 2);
        write_fixed(out, self.walker.as_ref(), &Walker::BLANK, Walker::encode);
        write_fixed(
            out,
            self.last_seen.as_ref(),
            &([0.0; 3], 0),
            |(at, when), out| {
                for value in at {
                    out.f32(*value);
                }
                out.u64(*when);
            },
        );
        out.u64(self.cast_after);
        out.u16(self.hold);
        out.u64(self.hold_until);
        out.u8(self.selected);
        out.u64(self.aware_at);
        self.pace.encode(out);
        self.troop.encode(out);
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Mind, DecodeError> {
        let at = input.at();
        let task = TASKS
            .get(usize::from(input.u8()?))
            .copied()
            .ok_or(input.invalid("mind task", at))?;
        let seat = |input: &mut Reader, what| {
            read_fixed(input, &Seat(0), |input| Ok(Seat(input.u8()?)), what)
        };
        let (leader, driver) = (seat(input, "leader flag")?, seat(input, "driver flag")?);
        let at = input.at();
        let flags = input.u8()?;
        if flags > 7 {
            return Err(input.invalid("mind flags", at));
        }
        let (steered, queued, moving) = (flags & 1 != 0, flags & 2 != 0, flags & 4 != 0);
        Ok(Mind {
            task,
            leader,
            driver,
            steered,
            queued,
            moving,
            walker: read_fixed(input, &Walker::BLANK, Walker::decode, "walker flag")?,
            last_seen: read_fixed(
                input,
                &([0.0; 3], 0),
                |input| Ok(([input.f32()?, input.f32()?, input.f32()?], input.u64()?)),
                "last seen flag",
            )?,
            cast_after: input.u64()?,
            hold: input.u16()?,
            hold_until: input.u64()?,
            selected: input.u8()?,
            aware_at: input.u64()?,
            pace: Pace::decode(input)?,
            troop: Troop::decode(input)?,
        })
    }
}

/// How long a creature that has just spotted a foe takes to act (ms), less
/// a little a rank of Awareness (I22 section 5's Awareness;
/// `minds.looking.reaction_ms`) [verify: KOTOR's AI starts on its
/// perception event, with no delay of its own the references show].
pub fn reaction_ms(awareness: u8) -> u64 {
    use rules::minds::looking;
    looking::REACTION_MS.saturating_sub(looking::REACTION_PER_RANK_MS * u64::from(awareness))
}

/// A power a mind uses: pressed once with its row selected, or a channel's
/// button held for a while.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cast {
    Press(u8),
    Hold(u16, u64),
}

/// How near a mind casts (units): grip's reach; lightning's line and drain
/// within drain's area's reach (well inside the line's); the level-3
/// lightning's cone; the crowd-control rows' aimed reach for the aimed.
const GRIP_REACH: f32 = force::ja::GRIP_RANGE;
const CHANNEL_REACH: f32 = force::ja::DRAIN_RADIUS;
const STORM_REACH: f32 = force::ja::LIGHTNING_RADIUS;
const AIMED_REACH: f32 = force::ja::STUN_RANGE;

/// How near its target a mind casts `row` (units): as `choose_power`
/// does; a push or pull where its shove still tells (its strength × level
/// over the shove's fall-off a unit); one cast on itself or its side
/// anywhere.
pub fn power_reach(row: u16) -> f32 {
    match force::of_row(row) {
        Some((Power::Grip, _)) => GRIP_REACH,
        Some((Power::Lightning, 3)) => STORM_REACH,
        Some((Power::Lightning | Power::Drain, _)) => CHANNEL_REACH,
        Some((Power::Push | Power::Pull, level)) => {
            force::ja::PUSH_PULL_STRENGTH * f32::from(level) / force::ja::SHOVE_FALLOFF
        }
        Some((
            Power::Heal
            | Power::Speed
            | Power::Protect
            | Power::Absorb
            | Power::Valor
            | Power::Resist
            | Power::Rage
            | Power::Sight,
            _,
        )) => f32::MAX,
        _ => AIMED_REACH,
    }
}

/// How long a mind holds grip, and lightning or drain (ms); a grip its
/// script chose, for as long as K1's choke ticks (the grip lets go itself
/// once the ticks are done, `powers::Cast::squeeze`).
const GRIP_HOLD_MS: u64 = powers::GRIP_HOLD_MS;
const SCRIPTED_GRIP_HOLD_MS: u64 = powers::SCRIPTED_GRIP_HOLD_MS;
const CHANNEL_HOLD_MS: u64 = powers::CHANNEL_HOLD_MS;

/// The power button with `row` selected; `None` for a row a command
/// cannot select (`Command::force` is a byte, and its top value is pull's).
fn press(row: u16) -> Option<Cast> {
    u8::try_from(row)
        .ok()
        .filter(|row| *row != force::PULL)
        .map(Cast::Press)
}

/// A power its script chose (C14, `ActionUseTalentOnObject`), if it knows
/// it and can pay `cost` of its row: grip, lightning and drain held as
/// `choose_power` holds them, the rest pressed.
pub fn power_cast(
    row: u16,
    sheet: &Sheet,
    fighter: &Fighter,
    cost: &dyn Fn(u16) -> Option<i32>,
) -> Option<Cast> {
    let affords = cost(row).is_some_and(|cost| fighter.fp >= cost);
    if !sheet.powers.has(row) || !affords {
        return None;
    }
    match force::of_row(row)?.0 {
        Power::Grip => Some(Cast::Hold(buttons::FORCE_GRIP, SCRIPTED_GRIP_HOLD_MS)),
        Power::Lightning => Some(Cast::Hold(buttons::FORCE_LIGHTNING, CHANNEL_HOLD_MS)),
        Power::Drain => Some(Cast::Hold(buttons::FORCE_DRAIN, CHANNEL_HOLD_MS)),
        _ => press(row),
    }
}

/// What a feat its script chose comes to (C14): a melee feat's saber
/// style (Power Attack strong, Critical Strike medium, Flurry fast: C8);
/// a ranged feat's alternate, the special the gun suits (`special`,
/// `Special::of`) when the bar can pay.
pub enum FeatUse {
    Style(Style),
    Cast(Cast),
}

pub fn feat_use(
    row: u16,
    sheet: &Sheet,
    fighter: &Fighter,
    special: Option<Special>,
) -> Option<FeatUse> {
    let styles: [(&[u16], Style); 3] = [
        (&feats::POWER_ATTACK, Style::Strong),
        (&feats::CRITICAL_STRIKE, Style::Medium),
        (&feats::FLURRY, Style::Fast),
    ];
    if let Some((_, style)) = styles.iter().find(|(rows, _)| rows.contains(&row)) {
        return Some(FeatUse::Style(*style));
    }
    let ranged: [&[u16]; 3] = [&feats::POWER_BLAST, &feats::RAPID_SHOT, &feats::SNIPER_SHOT];
    if !ranged.iter().any(|rows| rows.contains(&row)) {
        return None;
    }
    choose_special(special, fighter, specials::cost(sheet)).map(FeatUse::Cast)
}

/// A gunner's feat special (`combat::specials`) when its bar can pay
/// `cost`: the alternate button held for a full Power Blast charge or a
/// Sniper Shot's aim, or pressed once for a Rapid Shot's burst.
pub fn choose_special(special: Option<Special>, fighter: &Fighter, cost: i32) -> Option<Cast> {
    let special = special.filter(|_| fighter.fp >= cost)?;
    let ms = match special {
        // A tick past the last charge step.
        Special::PowerBlast(_) => {
            specials::CHARGE_STEP_MS * specials::CHARGE_STEPS + rules::minds::items::CHARGE_PAST_MS
        }
        Special::RapidShot(_) => 1,
        Special::SniperShot(_) => rules::minds::items::SNIPER_AIM_MS,
    };
    Some(Cast::Hold(buttons::ALT_ATTACK, ms))
}

/// A consumable a fighter in a fight uses now (`combat::items`), if it
/// carries it: a heal when hurt (`minds.items.heal_below`), a cure while
/// poisoned or weakened,
/// else a stim it has none of running. Never a shield: KOTOR's AI
/// (`k_ai_master`) raises one once a fight through its item's talent (a
/// droid's worn one, anyone else's carried), and never a party member's.
pub fn choose_item(sheet: &Sheet, fighter: &Fighter, now: u64) -> Option<Cast> {
    use items::Use;
    let pack = &fighter.pack;
    // Below its share (hundredths of an HP against the maximum's).
    let hurt = fighter.hp * 100 < i32::from(sheet.max_hp) * 100 * rules::minds::items::HEAL_BELOW;
    let afflicted = pack.afflicted(now);
    let droid = sheet.race == crate::combat::sheet::races::DROID;
    let wanted = |u: Use| match u {
        Use::Heal { droid: kit, .. } => hurt && kit == droid && pack.using.is_none(),
        Use::Squad { .. } => hurt && pack.using.is_none(),
        Use::Antidote => afflicted,
        Use::Stim(_) => pack.stim.is_none(),
        Use::Shield { .. } | Use::Grenade => false,
    };
    let (_, carried) = pack.next(&sheet.items, wanted)?;
    Some(Cast::Hold(items::use_of(carried.row)?.button(), 1))
}

/// A grenade thrown at a target `distance` units away, now and then
/// (`pick` in 0..1 below `minds.items.grenade_chance`): within its throwing
/// band, if it carries one, the button held for a charge that grows with
/// the distance.
pub fn choose_grenade(sheet: &Sheet, fighter: &Fighter, distance: f32, pick: f32) -> Option<Cast> {
    let near = rules::minds::items::GRENADE_NEAREST_M / collision::UNIT;
    let far = rules::minds::items::GRENADE_FARTHEST_M / collision::UNIT;
    fighter
        .pack
        .next(&sheet.items, |u| u == items::Use::Grenade)?;
    if pick >= rules::minds::items::GRENADE_CHANCE || !(near..=far).contains(&distance) {
        return None;
    }
    Some(throw(distance))
}

/// How far off its view a target may stand for a mind to begin a throw at
/// it (degrees) [verify: the engine's own; KOTOR's thrower turns to face].
pub const THROW_FACING: f32 = powers::THROW_FACING;

/// A grenade thrown at a target `distance` units away: the button held for
/// a charge that grows with the distance, full at the farthest a mind
/// throws.
pub fn throw(distance: f32) -> Cast {
    let far = rules::minds::items::GRENADE_FARTHEST_M / collision::UNIT;
    let charge = (distance / far * grenades::CHARGE_MS as f32) as u64;
    Cast::Hold(buttons::GRENADE, charge.clamp(1, grenades::CHARGE_MS))
}

/// What a mind knows of the foe it fights, for its powers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Foe {
    /// How far (units).
    pub distance: f32,
    pub stunned: bool,
    pub droid: bool,
    /// Slow's chain on it.
    pub slowed: bool,
    /// The lowest level of the powers it could be dispelled of (speed,
    /// protect, absorb, valor, energy resistance), if any run.
    pub buff: Option<u8>,
}

/// What a Force user in a fight casts now, if anything, with `pick` (a
/// number in 0..1 from its hash) to vary it (`minds.powers`): healing
/// itself when hurt; a grip held within its reach; a lightning held within
/// its reach (a line's here; level 3's cone) or a drain within drain's; a
/// push when crowded; a crowd-control row on a foe not already
/// stunned (within the aimed reach; droid stun on droids, the rest on the living);
/// now and then Slow's chain on a living foe not slowed, Supress or Breach
/// on one with a power it can end, and a buff it lacks (speed, protect, absorb, valor,
/// energy resistance, or rage above half health). Only what it knows, can
/// pay (`cost` of the row) and whose wait is over, and only rows a command
/// can select.
pub fn choose_power(
    sheet: &Sheet,
    fighter: &Fighter,
    foe: Foe,
    cost: &dyn Fn(u16) -> Option<i32>,
    pick: f32,
    now: u64,
) -> Option<Cast> {
    let Foe {
        distance,
        stunned: target_stunned,
        droid: target_droid,
        ..
    } = foe;
    let force = &fighter.force;
    let affords = |row: u16| cost(row).is_some_and(|cost| fighter.fp >= cost);
    let known = |power: Power| force::best(&sheet.powers, power).filter(|(row, _)| affords(*row));
    let hurt = fighter.hp * 100 < i32::from(sheet.max_hp) * 100 * powers::HEAL_BELOW;
    if hurt && let Some(cast) = known(Power::Heal).and_then(|(row, _)| press(row)) {
        return Some(cast);
    }
    if distance <= GRIP_REACH && force.grip_ready <= now && known(Power::Grip).is_some() {
        return Some(Cast::Hold(buttons::FORCE_GRIP, GRIP_HOLD_MS));
    }
    let lightning = known(Power::Lightning).is_some_and(|(_, level)| {
        distance
            <= if level == 3 {
                STORM_REACH
            } else {
                CHANNEL_REACH
            }
    });
    if lightning && force.lightning_ready <= now && pick < powers::LIGHTNING_CHANCE {
        return Some(Cast::Hold(buttons::FORCE_LIGHTNING, CHANNEL_HOLD_MS));
    }
    if distance <= CHANNEL_REACH && force.drain_ready <= now && known(Power::Drain).is_some() {
        return Some(Cast::Hold(buttons::FORCE_DRAIN, CHANNEL_HOLD_MS));
    }
    if distance <= powers::PUSH_WITHIN
        && force.push_pull_ready <= now
        && let Some(cast) = known(Power::Push).and_then(|(row, _)| press(row))
    {
        return Some(cast);
    }
    if !target_stunned && distance <= AIMED_REACH {
        let stun = (0..crate::combat::sheet::Rows::<5>::ROWS)
            .filter(|row| sheet.powers.has(*row) && affords(*row))
            .filter(|row| force::crowd::droids(*row) == target_droid)
            .find(|row| force::of_row(*row).is_some_and(|(power, _)| power == Power::Stun));
        if let Some(cast) = stun.and_then(press) {
            return Some(cast);
        }
    }
    let aimable = distance <= AIMED_REACH;
    for (power, wanted) in [
        (
            Power::Slow,
            aimable && !foe.slowed && !target_droid && pick < powers::SLOW_CHANCE,
        ),
        // Supress ends the first two tiers, Breach all of them.
        (
            Power::Dispel,
            aimable
                && pick < powers::DISPEL_CHANCE
                && known(Power::Dispel)
                    .is_some_and(|(_, level)| foe.buff.is_some_and(|buff| level >= 2 || buff <= 2)),
        ),
    ] {
        if wanted && let Some(cast) = known(power).and_then(|(row, _)| press(row)) {
            return Some(cast);
        }
    }
    let effects = &fighter.effects;
    let healthy = fighter.hp * 100 >= i32::from(sheet.max_hp) * 100 * powers::RAGE_ABOVE;
    let buffs = [
        (Power::Speed, force.speed_until <= now),
        (Power::Protect, force.protect_until <= now),
        (Power::Absorb, force.absorb_until <= now),
        (Power::Valor, effects.valor(now) == 0),
        (Power::Resist, effects.resisting(now) == 0),
        (
            Power::Rage,
            force.rage_until <= now && force.recover_until <= now && healthy,
        ),
    ];
    if pick < powers::BUFF_CHANCE {
        for (power, lacking) in buffs {
            if lacking && let Some(cast) = known(power).and_then(|(row, _)| press(row)) {
                return Some(cast);
            }
        }
    }
    None
}
