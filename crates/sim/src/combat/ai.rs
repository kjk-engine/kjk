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
//!   paced as JA's Jedi (`combat::pace`). An enemy casts what its script
//!   chooses (`k_ai_master`'s talents). A companion chooses its own powers
//!   and grenades (`combat::choose`). On a stage without scripts, every
//!   creature chooses its own powers, items and grenades.
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
use crate::combat::grenades;
use crate::combat::pace::Pace;
use crate::combat::saber::Style;
use crate::combat::sheet::{Sheet, feats};
use crate::combat::specials::{self, Special};
use crate::combat::troop::Troop;
use crate::input::{Seat, buttons};
use crate::math::Vec3;
use crate::script::{Menu, Talent};
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
/// How near its leader (units) a companion that is going back comes before
/// it fights again.
pub const LEASH_BACK: f32 = tasks::LEASH_BACK;
pub const FOLLOW_NEAR: f32 = tasks::FOLLOW_NEAR;

/// Whether a companion `distance` units from its leader, whose task last
/// step was `task`, goes back to its leader rather than fighting. One that
/// strays more than `LEASH` goes back. One already going back keeps going
/// until it is within `LEASH_BACK`. A companion standing at the edge of the
/// leash so no longer turns between its foe and its leader on every step.
pub fn leashed(task: Task, distance: f32) -> bool {
    let reach = if task == Task::Follow {
        LEASH_BACK
    } else {
        LEASH
    };
    distance > reach
}
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
    /// The fight is intense for it until this time (sim ms,
    /// `combat::choose::intense`). A look that finds the fight intense moves
    /// it to `combat::choose::held_until`.
    pub intense_until: u64,
    /// When it last used each kind of use that its side waits for (sim ms,
    /// indexed by `combat::choose::SideKind as usize`). Zero means never.
    /// Its allies read it (`combat::choose::side_allows`).
    pub used_at: [u64; 5],
    /// When it last changed its feat line or saber style of its own choice
    /// (sim ms, `combat::choose::feat_kept`). Zero means never.
    pub feat_at: u64,
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
        out.u64(self.intense_until);
        for at in self.used_at {
            out.u64(at);
        }
        out.u64(self.feat_at);
        self.pace.encode_escape(out);
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
        let choices = input.since(crate::since::snapshot::AI_CHOICES_COMPANIONS);
        let mut mind = Mind {
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
            // A snapshot from before the fields reads a calm fight, in
            // which nobody has used anything yet.
            intense_until: if choices { input.u64()? } else { 0 },
            used_at: if choices {
                [
                    input.u64()?,
                    input.u64()?,
                    input.u64()?,
                    input.u64()?,
                    input.u64()?,
                ]
            } else {
                [0; 5]
            },
            // A snapshot from before the field reads a mind that never
            // changed its feat line.
            feat_at: if input.since(crate::since::snapshot::AI_CHOICES_FEATS) {
                input.u64()?
            } else {
                0
            },
        };
        // A snapshot from before the escape reads a mind with no escape under
        // way, which may start one at once.
        if input.since(crate::since::snapshot::AI_CHOICES_KATAS) {
            mind.pace.decode_escape(input)?;
        }
        Ok(mind)
    }
}

/// The mind's encoding (`sim::combat::ai`), checked by the approved tests.
#[cfg(feature = "probes")]
pub fn probes() -> crate::probe::Checks {
    vec![("intense_until_kept", intense_until_kept())]
}

/// A mind written now reads back with `intense_until`, `used_at`, `feat_at`
/// and its escape kept. A mind written before the escape came in reads no
/// escape under way. A mind written before `feat_at` came in reads
/// `feat_at == 0`. A mind written before `intense_until` and `used_at` came
/// in reads them as zero too, as a default mind has.
#[cfg(feature = "probes")]
fn intense_until_kept() -> Result<String, String> {
    use crate::wire::Versions;
    let mut mind = Mind {
        cast_after: 3_000,
        intense_until: 12_000,
        used_at: [1_000, 0, 2_000, 0, 3_000],
        feat_at: 4_000,
        ..Mind::default()
    };
    mind.pace.escape = 2;
    mind.pace.escape_side = 1;
    mind.pace.escape_since = 5_000;
    mind.pace.escape_after = 6_000;
    mind.pace.drift = [40.0, -2.5];
    mind.pace.special_began = 7_000;
    let mut out = Writer::default();
    mind.encode(&mut out);
    let bytes = out.0;
    let again = Mind::decode(&mut Reader::new("mind", &bytes)).map_err(|e| e.to_string())?;
    let mut wrong = Vec::new();
    if again != mind {
        wrong.push(format!("read back {again:?}"));
    }
    // Each version gate, with the bytes that came in with it and after it.
    // The bytes before them are the old layout. The escape is one byte, two
    // times of eight bytes each, two speeds of four bytes and one more time.
    let escape =
        std::mem::size_of::<u8>() + 3 * std::mem::size_of::<u64>() + 2 * std::mem::size_of::<f32>();
    let older = |since: crate::wire::Since, cut: usize| -> Result<Mind, String> {
        let versions = Versions {
            snapshot: since.1 - 1,
            ..Versions::THIS_BUILD
        };
        let old = &bytes[..bytes.len() - cut];
        let mut input = Reader::new("mind", old).with_versions(versions);
        let read = Mind::decode(&mut input).map_err(|e| e.to_string())?;
        input.finish().map_err(|e| e.to_string())?;
        Ok(read)
    };
    let mut still = mind;
    still.pace.escape = 0;
    still.pace.escape_side = 0;
    still.pace.escape_since = 0;
    still.pace.escape_after = 0;
    still.pace.drift = [0.0; 2];
    still.pace.special_began = 0;
    let before_escape = older(crate::since::snapshot::AI_CHOICES_KATAS, escape)?;
    if before_escape != still {
        wrong.push(format!("before the escape {before_escape:?}"));
    }
    let before_feats = older(crate::since::snapshot::AI_CHOICES_FEATS, escape + 8)?;
    if before_feats
        != (Mind {
            feat_at: 0,
            ..still
        })
    {
        wrong.push(format!("before feat_at {before_feats:?}"));
    }
    let before_choices = older(
        crate::since::snapshot::AI_CHOICES_COMPANIONS,
        escape + 7 * 8,
    )?;
    let calm = Mind {
        intense_until: 0,
        used_at: [0; 5],
        feat_at: 0,
        ..still
    };
    if before_choices != calm {
        wrong.push(format!("before intense_until {before_choices:?}"));
    }
    crate::probe::check(wrong.is_empty(), format!("wrong: {wrong:?}"))
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
    Press(u16),
    Hold(u16, u64),
}

/// How near a mind casts (units): grip's reach; lightning's line and drain
/// within drain's area's reach (well inside the line's); the level-3
/// lightning's cone; the crowd-control rows' aimed reach for the aimed.
const GRIP_REACH: f32 = force::ja::GRIP_RANGE;
const CHANNEL_REACH: f32 = force::ja::DRAIN_RADIUS;
const STORM_REACH: f32 = force::ja::LIGHTNING_RADIUS;
const AIMED_REACH: f32 = force::ja::STUN_RANGE;

/// What a mind reads of a power only its impact script casts
/// (`force::scripted`): its row's talent facts, or `None` for any other row.
/// The minds' step answers it from the scripts' library, for a row with an
/// impact script.
pub type Scripted<'a> = &'a dyn Fn(u16) -> Option<Talent>;

/// No row is scripted (no scripts run).
pub fn no_scripted(_: u16) -> Option<Talent> {
    None
}

/// How near its target a mind casts `row` (units). A mind casts grip
/// within grip's reach. It casts lightning and drain within drain's area,
/// and the level-3 lightning within its cone. It casts the crowd-control
/// rows within their aimed reach. It casts a push or a pull where the
/// shove still moves the target, which is the power's strength times its
/// level over the shove's fall-off per unit. It casts a power on itself or
/// its side at any distance. It casts a scripted power from a foe's menu
/// within the row's range (`force::scripted_reach`).
pub fn power_reach(row: u16, scripted: Scripted) -> f32 {
    if let Some(talent) = scripted(row) {
        return match talent.menu {
            Menu::Friendly => f32::MAX,
            _ => force::scripted_reach(talent),
        };
    }
    match force::of_row(row) {
        Some((Power::Grip, _)) => GRIP_REACH,
        Some((Power::Lightning, 3)) => STORM_REACH,
        Some((Power::Lightning | Power::Drain, _)) => CHANNEL_REACH,
        Some((Power::Push | Power::Pull, level)) => {
            force::ja::PUSH_PULL_STRENGTH * f32::from(level) / force::ja::SHOVE_FALLOFF
        }
        Some((power, _)) if force::on_own_side(power) => f32::MAX,
        _ => AIMED_REACH,
    }
}

/// How long a mind holds grip, and how long it holds lightning or drain
/// (ms). It holds grip for as long as KOTOR 1's choke ticks. The grip lets
/// go by itself once the ticks are done (`powers::Cast::squeeze`).
const SCRIPTED_GRIP_HOLD_MS: u64 = powers::SCRIPTED_GRIP_HOLD_MS;
const CHANNEL_HOLD_MS: u64 = powers::CHANNEL_HOLD_MS;

/// The power button with `row` selected; `None` for pull's value, which
/// no row of the table is.
fn press(row: u16) -> Option<Cast> {
    Some(row).filter(|row| *row != force::PULL).map(Cast::Press)
}

/// How a mind casts a power that its script picked (C14,
/// `ActionUseTalentOnObject`) or that it chose itself (`combat::choose`).
/// It casts the power only when it knows the power and has at least
/// `cost` of the row in Force points. It holds the button for grip,
/// lightning and drain. It presses the button once for every other power,
/// a scripted power too.
pub fn power_cast(
    row: u16,
    sheet: &Sheet,
    fighter: &Fighter,
    cost: &dyn Fn(u16) -> Option<i32>,
    scripted: Scripted,
) -> Option<Cast> {
    let affords = cost(row).is_some_and(|cost| fighter.fp >= cost);
    if !sheet.powers.has(row) || !affords {
        return None;
    }
    if scripted(row).is_some() {
        return press(row);
    }
    match force::of_row(row)?.0 {
        Power::Grip => Some(Cast::Hold(buttons::FORCE_GRIP, SCRIPTED_GRIP_HOLD_MS)),
        Power::Lightning => Some(Cast::Hold(buttons::FORCE_LIGHTNING, CHANNEL_HOLD_MS)),
        Power::Drain => Some(Cast::Hold(buttons::FORCE_DRAIN, CHANNEL_HOLD_MS)),
        _ => press(row),
    }
}

/// A Force Push a mind starts itself now (`Pace::may_push`), with how far it
/// reaches (units), if it knows the power, can pay for it and its wait since
/// its last push or pull is over.
pub fn push_cast(
    sheet: &Sheet,
    fighter: &Fighter,
    cost: &dyn Fn(u16) -> Option<i32>,
    now: u64,
) -> Option<(Cast, f32)> {
    let (row, _) = force::best(&sheet.powers, Power::Push)?;
    if fighter.force.push_pull_ready > now {
        return None;
    }
    Some((
        power_cast(row, sheet, fighter, cost, &no_scripted)?,
        power_reach(row, &no_scripted),
    ))
}

/// What a feat its script chose comes to (C14): a melee feat's saber
/// style (Power Attack strong, Critical Strike medium, Flurry fast: C8;
/// two blades or a double-bladed weapon carry it instead, `combat::picks`);
/// a ranged feat's alternate, the special its gun takes (`special`,
/// `Special::of`, the line chosen first) when the bar can pay.
pub enum FeatUse {
    Style(Style),
    Cast(Cast),
}

pub fn feat_use(
    row: u16,
    sheet: &Sheet,
    fighter: &Fighter,
    special: Option<Special>,
    duel: bool,
) -> Option<FeatUse> {
    let styles: [(&[u16], Style); 3] = [
        (&feats::POWER_ATTACK, Style::Strong),
        (&feats::CRITICAL_STRIKE, Style::Medium),
        (&feats::FLURRY_RANKS, Style::Fast),
    ];
    if let Some((_, style)) = styles.iter().find(|(rows, _)| rows.contains(&row)) {
        return Some(FeatUse::Style(*style));
    }
    let ranged: [&[u16]; 3] = [&feats::POWER_BLAST, &feats::RAPID_SHOT, &feats::SNIPER_SHOT];
    if !ranged.iter().any(|rows| rows.contains(&row)) {
        return None;
    }
    choose_special(special, specials::Bar::of(sheet, fighter, duel)).map(FeatUse::Cast)
}

/// A gunner's feat special (`combat::specials`) when its `bar` can pay:
/// the weapon's other mode (`Command::other_mode`, the feat attack with a
/// blaster) held for a full Power Blast charge or for the steadying of a
/// Sniper Shot from the hip, or pressed once for a Rapid Shot's burst.
pub fn choose_special(special: Option<Special>, bar: specials::Bar) -> Option<Cast> {
    let special = special.filter(|_| bar.affordable())?;
    let ms = match special {
        // A tick past the last charge step.
        Special::PowerBlast(_) => {
            specials::CHARGE_STEP_MS * specials::CHARGE_STEPS + rules::minds::items::CHARGE_PAST_MS
        }
        Special::RapidShot(_) => 1,
        Special::SniperShot(_) => {
            rules::feats::specials::sniper_shot::STEADY_MS[0] + rules::minds::items::CHARGE_PAST_MS
        }
    };
    Some(Cast::Hold(buttons::ALT_ATTACK, ms))
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

/// A grenade thrown in a held scene at a target `distance` units away, however
/// far (KOTOR's thrower has no farthest throw): the button held for the
/// charge that lobs it there (`grenades::lob_strength`), looking up as it
/// charges.
pub fn scripted_throw(distance: f32) -> Cast {
    let charge = grenades::lob_strength(distance) * grenades::CHARGE_MS as f32;
    Cast::Hold(
        buttons::GRENADE,
        (charge as u64).clamp(1, grenades::CHARGE_MS),
    )
}
