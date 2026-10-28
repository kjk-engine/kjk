//! What reaches the sim from outside: one command per seat per tick.
//!
//! A seat is a party slot. The local player, a remote player, an AI
//! companion and a replay all arrive the same way, as a seat's command; the
//! sim never knows which of them sent it. The commands are kept ordered by
//! seat, so the order they arrived in (network, threads) cannot change a
//! step: that is the canonical input order.

use std::collections::BTreeMap;

use crate::action::Action;
use crate::wire::{DecodeError, Reader, Writer};

/// A party slot, from 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Seat(pub u8);

/// One seat's command for one tick, shaped like JA's user command
/// (`usercmd_t`): where the player looks, signed axes from -127 to 127
/// (up jumps, down crouches) and button bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Command {
    /// Pitch, yaw and roll as JA sends them: 65 536 to the turn, pitch
    /// positive looking down, yaw positive turning left (from +X).
    pub angles: [i16; 3],
    pub forward: i8,
    pub right: i8,
    pub up: i8,
    pub buttons: u16,
    /// Which weapon to hold (JA's `weapon`): 0 the right hand's, 1 the left
    /// hand's (or a second saber).
    pub weapon: u8,
    /// Which power the force button uses (JA's `forcesel`): a `spells.2da`
    /// row the character knows, or 0 for none.
    pub force: u8,
    /// The player's answer to its conversation (`script::dialogue::answer`):
    /// held until the next one; the authority answers when its count moves.
    pub reply: u8,
    /// Where the line through the middle of the player's screen runs, off
    /// the third-person one (`combat::mind::crosshair`), across the view,
    /// in `mind::AIM_STEP`s along x, y and z: none for JA's third-person
    /// camera, the eyes' drawn place for the player's own eyes
    /// (`mind::aim_for`). Its shots go where that line meets.
    pub aim: [i8; 3],
}

impl Command {
    /// This command's view and selections (weapon, power, answer) with
    /// nothing moved or pressed: a seat that is there but sends nothing.
    pub fn idle(&self) -> Command {
        Command {
            angles: self.angles,
            weapon: self.weapon,
            force: self.force,
            reply: self.reply,
            aim: self.aim,
            ..Command::default()
        }
    }
}

/// Button bits of `Command::buttons`, JA's where JA has them.
pub mod buttons {
    /// Fire, or swing (`BUTTON_ATTACK`).
    pub const ATTACK: u16 = 1;
    /// The weapon's other mode (`BUTTON_ALT_ATTACK`).
    pub const ALT_ATTACK: u16 = 1 << 7;
    /// Walk instead of run (`BUTTON_WALKING`; JA's client sends it with
    /// axes of 64).
    pub const WALKING: u16 = 1 << 4;
    /// Use what the player looks at (`BUTTON_USE`): open a door, go
    /// through one that leads out.
    pub const USE: u16 = 1 << 5;
    /// Use held without using what it faces: what revives a downed party
    /// member ahead (`world::falls`; `USE` alone does not). A revive
    /// chosen in the target block holds it (`target::Reviving`), as does
    /// `--moves`' `use` (`moves::USE`). The use key's press is the
    /// target's chosen action (`Action::Use`,
    /// `docs/UI.md`).
    pub const USE_HELD: u16 = 1 << 8;
    /// Grip (`BUTTON_FORCEGRIP`).
    pub const FORCE_GRIP: u16 = 1 << 6;
    /// The selected power (`BUTTON_FORCEPOWER`).
    pub const FORCE_POWER: u16 = 1 << 9;
    /// Lightning (`BUTTON_FORCE_LIGHTNING`).
    pub const FORCE_LIGHTNING: u16 = 1 << 10;
    /// Drain (`BUTTON_FORCE_DRAIN`).
    pub const FORCE_DRAIN: u16 = 1 << 11;
    /// The next saber style, on a press: JA's `saberAttackCycle` console
    /// command, carried with the command so it replays and predicts.
    pub const SABER_STYLE: u16 = 1 << 12;
    /// Charge a grenade while held, throw it on release (JA throws its
    /// thermal detonator as a weapon; here grenades are KOTOR's items, on a
    /// button of their own).
    pub const GRENADE: u16 = 1 << 13;
    /// Use a healing item: an antidote while poisoned or weakened, else
    /// the first medpac (a droid: repair kit) or squad stim carried (JA's
    /// bacta is a holdable item; KOTOR's are consumables, `combat::items`).
    pub const MEDPAC: u16 = 1 << 14;
    /// Put up the first energy shield carried.
    pub const SHIELD: u16 = 1 << 15;
    /// Take the first stim carried.
    pub const STIM: u16 = 1 << 3;
    /// Turn stealth mode on or off, on a press (`world::covert`, spec S11).
    pub const STEALTH: u16 = 1 << 1;
    /// Lay the best trap kit the party carries at the player's feet, on a
    /// press (`traps::best_kit`, spec S9).
    pub const LAY_MINE: u16 = 1 << 2;
}

/// How many seats there can be: every value of a `Seat`.
pub const SEATS: usize = 256;

/// The commands of one tick, in canonical order, and how far behind each
/// seat's player saw the world when it made its command (its view lag,
/// I22 C18: the authority tests its hits against the world as it was
/// then; none for a seat on the host or unknown).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TickInput {
    commands: BTreeMap<Seat, Command>,
    views: BTreeMap<Seat, u16>,
    /// Seats' actions, by seat, each seat's in the order it sent them.
    actions: Vec<(Seat, Action)>,
}

impl TickInput {
    /// Sets a seat's command, returning the one it replaces. A transport
    /// delivers at most one per seat per tick; a replaced command is the
    /// caller's bug to report.
    pub fn insert(&mut self, seat: Seat, command: Command) -> Option<Command> {
        self.commands.insert(seat, command)
    }

    /// Commands by seat, lowest first.
    pub fn iter(&self) -> impl Iterator<Item = (Seat, &Command)> {
        self.commands.iter().map(|(seat, command)| (*seat, command))
    }

    pub fn get(&self, seat: Seat) -> Option<&Command> {
        self.commands.get(&seat)
    }

    /// Sets how many milliseconds behind the authority `seat`'s player saw
    /// the world (the time its presented view showed, against the tick its
    /// command is for); 0 clears it.
    pub fn set_view_lag(&mut self, seat: Seat, ms: u16) {
        if ms == 0 {
            self.views.remove(&seat);
        } else {
            self.views.insert(seat, ms);
        }
    }

    /// `seat`'s view lag (ms), 0 when none was given.
    pub fn view_lag(&self, seat: Seat) -> u16 {
        self.views.get(&seat).copied().unwrap_or(0)
    }

    /// Adds `seat`'s action after any it already has this tick.
    pub fn act(&mut self, seat: Seat, action: Action) {
        let at = self.actions.partition_point(|(other, _)| *other <= seat);
        self.actions.insert(at, (seat, action));
    }

    /// The actions in canonical order: by seat, then as each seat sent
    /// them.
    pub fn actions(&self) -> &[(Seat, Action)] {
        &self.actions
    }

    pub fn len(&self) -> usize {
        self.commands.len()
    }

    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// The count takes two bytes: every seat a `Seat` can name (256) has
    /// room, so encoding cannot fail.
    pub(crate) fn encode(&self, out: &mut Writer) {
        out.u16(self.commands.len() as u16);
        for (seat, command) in &self.commands {
            out.u8(seat.0);
            // Every field by name: a new one does not compile until it is
            // written here and read in `decode`.
            let Command {
                angles,
                forward,
                right,
                up,
                buttons,
                weapon,
                force,
                reply,
                aim,
            } = *command;
            for angle in angles {
                out.u16(angle as u16);
            }
            out.i8(forward);
            out.i8(right);
            out.i8(up);
            out.u16(buttons);
            out.u8(weapon);
            out.u8(force);
            out.u8(reply);
            for step in aim {
                out.i8(step);
            }
        }
        out.u16(self.views.len() as u16);
        for (seat, lag) in &self.views {
            out.u8(seat.0);
            out.u16(*lag);
        }
        out.u32(u32::try_from(self.actions.len()).expect("fewer than 2^32 actions"));
        for (seat, action) in &self.actions {
            out.u8(seat.0);
            action.encode(out);
        }
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Self, DecodeError> {
        let at = input.at();
        let count = input.u16()?;
        if usize::from(count) > SEATS {
            return Err(input.invalid("seat count", at));
        }
        let mut commands = BTreeMap::new();
        let mut last: Option<u8> = None;
        for _ in 0..count {
            let at = input.at();
            let seat = input.u8()?;
            // Encoded in seat order, each seat once: anything else was not
            // written by `encode`.
            if last.is_some_and(|last| seat <= last) {
                return Err(input.invalid("seat order", at));
            }
            last = Some(seat);
            let mut angles = [0i16; 3];
            for angle in &mut angles {
                *angle = input.u16()? as i16;
            }
            let command = Command {
                angles,
                forward: input.i8()?,
                right: input.i8()?,
                up: input.i8()?,
                buttons: input.u16()?,
                weapon: input.u8()?,
                force: input.u8()?,
                reply: input.u8()?,
                aim: [input.i8()?, input.i8()?, input.i8()?],
            };
            commands.insert(Seat(seat), command);
        }
        let at = input.at();
        let count = input.u16()?;
        if usize::from(count) > SEATS {
            return Err(input.invalid("view count", at));
        }
        let mut views = BTreeMap::new();
        let mut last: Option<u8> = None;
        for _ in 0..count {
            let at = input.at();
            let seat = input.u8()?;
            let lag = input.u16()?;
            if last.is_some_and(|last| seat <= last) || lag == 0 {
                return Err(input.invalid("view", at));
            }
            last = Some(seat);
            views.insert(Seat(seat), lag);
        }
        let mut actions: Vec<(Seat, Action)> = Vec::new();
        for _ in 0..input.u32()? {
            let at = input.at();
            let seat = Seat(input.u8()?);
            if actions.last().is_some_and(|(last, _)| *last > seat) {
                return Err(input.invalid("action order", at));
            }
            actions.push((seat, Action::decode(input)?));
        }
        Ok(TickInput {
            commands,
            views,
            actions,
        })
    }
}
