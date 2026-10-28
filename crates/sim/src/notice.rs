//! What the authority tells one player, outside the snapshot: why an
//! action came to nothing, what a script said to it, a screen a script
//! opens for it. The notices since the last `SimWorld::take_notices` are
//! the authority's only; its host sends them on.

use std::collections::BTreeMap;

use bevy_ecs::prelude::*;

use crate::input::Seat;
use crate::wire::{DecodeError, Reader, Writer};

/// A screen the games' scripts open for a player (the GUI natives), with
/// what it is about (objects by script object id).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    /// `OpenStore`: the store object, its extra markup and markdown (%).
    Store {
        store: u32,
        markup: i32,
        markdown: i32,
    },
    /// `ShowUpgradeScreen`: the item, or none.
    Upgrade { item: Option<u32> },
    /// TSL's `ShowUpgradeScreen` with item creation (`ShowChemical…` for the
    /// lab station): the bench as it stands for its player (told again
    /// after each breakdown or item made).
    Workbench(Bench),
    /// `ShowLevelUpGUI`.
    LevelUp,
    /// `ShowPartySelectionGUI` (`open`), or the party screen a player
    /// opened (`Action::ShowParty`): the party table as it stands.
    PartySelection(Roster),
    /// `PopUpDeathGUIPanel`.
    Death,
    /// `DisplayMessageBox`: the text's strref.
    MessageBox { strref: i32 },
    /// `PlayMovie`, `StartNewModule`'s movies, TSL's `PlayMovieQueue`: the
    /// movie's resref, lowercase, padded with zeros; whether a skip ends it
    /// (`QueueMovie`'s `nSkippable`; the rest are); the queue's run it plays
    /// in (0: alone): a skip of a skippable one ends the rest of its run,
    /// as KOTOR's player drops the rest of its list.
    Movie {
        name: [u8; 16],
        skippable: bool,
        run: u16,
    },
    /// TSL's and K1's `ShowTutorialWindow`: the `tutorial.2da` row whose
    /// window shows (its `Message_PC0` to `2`).
    Tutorial { row: u8 },
    /// TSL's `DisplayDatapad`: the item (its template) whose
    /// `DescIdentified` shows in the message box.
    Datapad { template: String },
    /// `ShowGalaxyMap`: the planet it opens on (`planetary.2da` row), and
    /// the planets the scripts made available and selectable then (bit n:
    /// row n); the host travels with `Action::Travel`.
    GalaxyMap {
        planet: u8,
        available: u32,
        selectable: u32,
    },
    /// `EndGame(FALSE)`: the game is over, without the death screen: the
    /// main menu, once credits rolling have rolled (K1's ending, after its
    /// credits started; Manaan's life sentence, at once). With its death
    /// screen it is a loss instead (`Snapshot::lost`).
    End,
    /// `StartCreditSequence`: the credits roll (K1's ending).
    Credits,
    /// `PlayPazaak`: a match began; its table is the snapshot's
    /// (`Snapshot::pazaak`), which the screens follow.
    Pazaak,
}

/// A workbench or lab station a script opened (`Screen::Workbench`): the
/// item it upgrades alone, if any; the party member whose skills it uses;
/// whether upgrading may be chosen; the item creation table it lists
/// (`inventory::workshop`); the lab station's (chemicals) or the
/// workbench's (components); and how many of them the party has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bench {
    pub item: Option<u32>,
    pub maker: u32,
    pub upgrade: bool,
    pub list: String,
    pub lab: bool,
    pub stock: i32,
}

impl Bench {
    fn encode(&self, out: &mut Writer) {
        out.u8(u8::from(self.item.is_some()));
        out.u32(self.item.unwrap_or(0));
        out.u32(self.maker);
        out.u8(u8::from(self.upgrade) | u8::from(self.lab) << 1);
        out.string(&self.list);
        out.u32(self.stock as u32);
    }

    fn decode(input: &mut Reader) -> Result<Bench, DecodeError> {
        let some = input.u8()? != 0;
        let item = input.u32()?;
        let maker = input.u32()?;
        let flags = input.u8()?;
        Ok(Bench {
            item: some.then_some(item),
            maker,
            upgrade: flags & 1 != 0,
            lab: flags & 2 != 0,
            list: input.string()?,
            stock: input.u32()? as i32,
        })
    }
}

const STORE: u8 = 1;
const UPGRADE: u8 = 2;
const LEVEL_UP: u8 = 3;
const PARTY_SELECTION: u8 = 4;
const DEATH: u8 = 5;
const MESSAGE_BOX: u8 = 6;
const MOVIE: u8 = 7;
const GALAXY_MAP: u8 = 8;
const END: u8 = 9;
const CREDITS: u8 = 10;
const PAZAAK: u8 = 11;
const WORKBENCH: u8 = 12;
const TUTORIAL: u8 = 13;
const DATAPAD: u8 = 14;

/// The most companions a party selection screen shows (KOTOR 2's twelve;
/// KOTOR 1 has nine).
pub const NPCS: usize = 12;

/// The party table as a party selection screen shows it, by `NPC_*`
/// number (bit n: NPC n): each available companion's template, those the
/// player may not pick (`SetNPCSelectability`), those in the party, those
/// the screen's script puts in whatever is picked, and whether a script
/// showed it (its choice then makes the party; else the screen only shows
/// the table). `worn`: what each companion the table has a creature of
/// wears in its body slot now (an item template; `None`, nothing), which
/// its 3D view is dressed in, as KOTOR's screen shows the creature the
/// table keeps; one never made (absent) wears its template's.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Roster {
    pub available: BTreeMap<u8, String>,
    pub worn: BTreeMap<u8, Option<String>>,
    pub unselectable: u16,
    pub members: u16,
    pub forced: u16,
    pub open: bool,
}

impl Roster {
    /// `npc`'s template, when it is available.
    pub fn template(&self, npc: usize) -> Option<&str> {
        self.available
            .get(&u8::try_from(npc).ok()?)
            .map(String::as_str)
    }

    /// What `npc` wears in its body slot, when the table has a creature of
    /// it (`Some(None)`: nothing); `None` for one never made.
    pub fn worn(&self, npc: usize) -> Option<Option<&str>> {
        self.worn
            .get(&u8::try_from(npc).ok()?)
            .map(Option::as_deref)
    }

    /// Whether `npc` is in `bits` (`unselectable`, `members`, `forced`).
    pub fn has(bits: u16, npc: usize) -> bool {
        u32::try_from(npc)
            .ok()
            .and_then(|npc| bits.checked_shr(npc))
            .is_some_and(|bits| bits & 1 == 1)
    }

    /// The bits of `npcs` (those past `NPCS` left out).
    pub fn bits(npcs: impl IntoIterator<Item = u8>) -> u16 {
        npcs.into_iter()
            .filter_map(|npc| 1u16.checked_shl(u32::from(npc)))
            .fold(0, |bits, bit| bits | bit)
    }

    fn encode(&self, out: &mut Writer) {
        let available: Vec<_> = self
            .available
            .iter()
            .filter(|(npc, _)| usize::from(**npc) < NPCS)
            .collect();
        out.u8(available.len() as u8);
        for (npc, template) in available {
            out.u8(*npc);
            out.string(template);
        }
        out.u16(self.unselectable);
        out.u16(self.members);
        out.u16(self.forced);
        out.u8(u8::from(self.open));
        let worn: Vec<_> = self
            .worn
            .iter()
            .filter(|(npc, _)| usize::from(**npc) < NPCS)
            .collect();
        out.u8(worn.len() as u8);
        for (npc, armour) in worn {
            out.u8(*npc);
            out.u8(u8::from(armour.is_some()));
            out.string(armour.as_deref().unwrap_or(""));
        }
    }

    fn decode(input: &mut Reader) -> Result<Roster, DecodeError> {
        let mut roster = Roster::default();
        for _ in 0..input.u8()? {
            let npc = input.u8()?;
            roster.available.insert(npc, input.string()?);
        }
        roster.unselectable = input.u16()?;
        roster.members = input.u16()?;
        roster.forced = input.u16()?;
        roster.open = input.u8()? != 0;
        for _ in 0..input.u8()? {
            let npc = input.u8()?;
            let some = input.u8()? != 0;
            let armour = input.string()?;
            roster.worn.insert(npc, some.then_some(armour));
        }
        Ok(roster)
    }
}

/// A resref as 16 bytes: lowercase, cut to 16, padded with zeros.
fn padded(name: &str) -> [u8; 16] {
    let mut padded = [0; 16];
    for (to, from) in padded.iter_mut().zip(name.to_ascii_lowercase().bytes()) {
        *to = from;
    }
    padded
}

/// A resref `padded` made.
fn unpadded(name: &[u8; 16]) -> String {
    let end = name.iter().position(|b| *b == 0).unwrap_or(name.len());
    String::from_utf8_lossy(&name[..end]).into_owned()
}

impl Screen {
    /// A movie by resref (cut to 16 bytes, lowercase), alone and
    /// skippable.
    pub fn movie(name: &str) -> Screen {
        Screen::queued_movie(name, true, 0)
    }

    /// A movie of queue run `run`, skippable or not (`PlayMovieQueue`).
    pub fn queued_movie(name: &str, skippable: bool, run: u16) -> Screen {
        Screen::Movie {
            name: padded(name),
            skippable,
            run,
        }
    }

    /// A movie screen's resref.
    pub fn movie_name(&self) -> Option<String> {
        match self {
            Screen::Movie { name, .. } => Some(unpadded(name)),
            _ => None,
        }
    }

    /// A galaxy map's planet `row` is in `bits` (its `available` or
    /// `selectable`).
    pub fn has_planet(bits: u32, row: usize) -> bool {
        u32::try_from(row)
            .ok()
            .and_then(|row| bits.checked_shr(row))
            .is_some_and(|bits| bits & 1 == 1)
    }

    /// The screen as bytes of its own (the network's message).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Writer::default();
        match *self {
            Screen::Store {
                store,
                markup,
                markdown,
            } => {
                out.u8(STORE);
                out.u32(store);
                out.u32(markup as u32);
                out.u32(markdown as u32);
            }
            Screen::Upgrade { item } => {
                out.u8(UPGRADE);
                out.u8(u8::from(item.is_some()));
                out.u32(item.unwrap_or(0));
            }
            Screen::LevelUp => out.u8(LEVEL_UP),
            Screen::PartySelection(ref roster) => {
                out.u8(PARTY_SELECTION);
                roster.encode(&mut out);
            }
            Screen::Death => out.u8(DEATH),
            Screen::MessageBox { strref } => {
                out.u8(MESSAGE_BOX);
                out.u32(strref as u32);
            }
            Screen::Movie {
                name,
                skippable,
                run,
            } => {
                out.u8(MOVIE);
                out.bytes(&name);
                out.u8(u8::from(skippable));
                out.u16(run);
            }
            Screen::Tutorial { row } => {
                out.u8(TUTORIAL);
                out.u8(row);
            }
            Screen::Datapad { ref template } => {
                out.u8(DATAPAD);
                out.string(template);
            }
            Screen::GalaxyMap {
                planet,
                available,
                selectable,
            } => {
                out.u8(GALAXY_MAP);
                out.u8(planet);
                out.u32(available);
                out.u32(selectable);
            }
            Screen::End => out.u8(END),
            Screen::Credits => out.u8(CREDITS),
            Screen::Pazaak => out.u8(PAZAAK),
            Screen::Workbench(ref bench) => {
                out.u8(WORKBENCH);
                bench.encode(&mut out);
            }
        }
        out.0
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Screen, DecodeError> {
        let mut input = Reader::new("screen", bytes);
        let at = input.at();
        let screen = match input.u8()? {
            STORE => Screen::Store {
                store: input.u32()?,
                markup: input.u32()? as i32,
                markdown: input.u32()? as i32,
            },
            UPGRADE => {
                let at = input.at();
                let some = input.u8()?;
                let item = input.u32()?;
                match (some, item) {
                    (0, 0) => Screen::Upgrade { item: None },
                    (1, item) => Screen::Upgrade { item: Some(item) },
                    _ => return Err(input.invalid("upgrade item", at)),
                }
            }
            LEVEL_UP => Screen::LevelUp,
            PARTY_SELECTION => Screen::PartySelection(Roster::decode(&mut input)?),
            DEATH => Screen::Death,
            MESSAGE_BOX => Screen::MessageBox {
                strref: input.u32()? as i32,
            },
            MOVIE => {
                let mut name = [0; 16];
                for byte in &mut name {
                    *byte = input.u8()?;
                }
                Screen::Movie {
                    name,
                    skippable: input.u8()? != 0,
                    run: input.u16()?,
                }
            }
            TUTORIAL => Screen::Tutorial { row: input.u8()? },
            DATAPAD => Screen::Datapad {
                template: input.string()?,
            },
            END => Screen::End,
            CREDITS => Screen::Credits,
            PAZAAK => Screen::Pazaak,
            WORKBENCH => Screen::Workbench(Bench::decode(&mut input)?),
            GALAXY_MAP => Screen::GalaxyMap {
                planet: input.u8()?,
                available: input.u32()?,
                selectable: input.u32()?,
            },
            _ => return Err(input.invalid("screen", at)),
        };
        input.finish()?;
        Ok(screen)
    }
}

/// What a sound's resref is when it names the object's own locked sound
/// (its sound set's `Locked`, `placeableobjsnds.2da`), which the views
/// know and the sim does not: no resref holds `*`.
pub const LOCKED_SOUND: &str = "*locked";

/// What a sound's resref starts with when it names a line of the object's
/// own sound set (the line's number follows: `*ssf22`), which the views
/// know and the sim does not.
pub const SOUND_SET_LINE: &str = "*ssf";

/// A sound a script plays (`PlaySound`): its resref, at the object that
/// played it (a script object id; the module or the area: nowhere, heard
/// alike everywhere); or the engine's (`LOCKED_SOUND`, a sound set's line).
/// Views play it once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sound {
    pub object: u32,
    pub name: [u8; 16],
}

impl Sound {
    pub fn new(object: u32, name: &str) -> Sound {
        Sound {
            object,
            name: padded(name),
        }
    }

    /// Line `slot` of `object`'s sound set (`SOUND_SET_LINE`), said by it.
    pub fn line(object: u32, slot: usize) -> Sound {
        Sound::new(object, &format!("{SOUND_SET_LINE}{slot}"))
    }

    /// Its resref.
    pub fn resref(&self) -> String {
        unpadded(&self.name)
    }

    /// The line of its object's sound set it names (`line`), if it names
    /// one.
    pub fn sound_set_line(&self) -> Option<usize> {
        self.resref().strip_prefix(SOUND_SET_LINE)?.parse().ok()
    }

    /// The sound as bytes of its own (the network's message).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Writer::default();
        out.u32(self.object);
        out.bytes(&self.name);
        out.0
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Sound, DecodeError> {
        let mut input = Reader::new("sound", bytes);
        let object = input.u32()?;
        let mut name = [0; 16];
        for byte in &mut name {
            *byte = input.u8()?;
        }
        input.finish()?;
        Ok(Sound { object, name })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// The item (a template) was gone: taken this step by `by`, or before.
    Gone {
        item: String,
        by: Option<Seat>,
    },
    /// Not open by this seat, or not a container.
    NotOpen,
    /// The player is down or too far away.
    OutOfReach,
    /// Nothing was left to take.
    Empty,
    /// Not trading with that store (`Action::Buy`, `Sell`).
    NoTrade,
    /// The store does not sell that, or does not buy it (it does not buy
    /// at all, the party does not carry it, or it is a plot item).
    NotForSale,
    /// The party's gold does not reach the price.
    CannotAfford,
    /// The party carries no such item, or it does not fit that slot.
    CannotEquip,
    /// That party member is not this seat's to dress: another seat plays
    /// it (`by`), or nobody does and only the host dresses it (I30 P14).
    NotYours {
        by: Option<Seat>,
    },
    /// A script's words to the player (`SendMessageToPC`).
    Said(String),
    /// The party gained XP (lost, if negative).
    Experience(i32),
    /// The level asked for could not be taken (its picks were not the
    /// level's, or the XP was short).
    LevelRefused,
    Show(Screen),
    /// A sound a script played, for every player.
    Sound(Sound),
    /// A line of the game's own feedback, by talk-table strref
    /// (`crate::locks::lines`).
    Line(u32),
    /// A line of the game's own feedback whose `<CUSTOM0>` is a count
    /// (TSL's "Acquired 5 Components.", `script::workbench`).
    Count {
        line: u32,
        count: i32,
    },
    /// A skill check (the game's line 1406): what was tried (a talk-table
    /// strref: Security, Disable Mine, Recover Mine, Set Mine), the d20
    /// rolled (`None`: out of combat, taking 20), the skill's total with
    /// its bonuses, the DC.
    Check {
        skill: u32,
        roll: Option<u8>,
        total: i16,
        dc: u8,
        success: bool,
    },
    /// The party detected a trap: its `traps.2da` name (a strref).
    TrapSeen {
        name: u32,
    },
    /// A mine set on a door blasted its lock (TSL's line 128535): the sum,
    /// the setter's Demolitions, the mine's strength, the lock's DC
    /// (`traps::blasts`).
    Blast {
        sum: i16,
        demolitions: i16,
        strength: u8,
        dc: u8,
        success: bool,
    },
    /// Refused: the party is in combat (a break, a walk into an exit, a
    /// forced move: KOTOR's "enemies nearby").
    InCombat,
    /// Only the seat that called the break, or seat 0, ends it.
    NotYourBreak,
    /// Only seat 0 forces a ready check, once it has stood
    /// `party::FORCE_AFTER_MS`.
    CannotForce,
    /// Not revived (`Action::Revive`): it is not a downed party member, or
    /// another player revives it.
    CannotRevive,
    /// Not used (`Action::UseItem`): no such item, none of its uses left,
    /// not for this character, or not on that target.
    CannotUse,
    /// Not fitted or taken out (`Action::Fit`, `Unfit`): the item does not
    /// take that upgrade, its kind is full, or the upgrade is not there.
    CannotUpgrade,
}

/// The notices since the last `SimWorld::take_notices`, by seat.
#[derive(Resource, Default)]
pub(crate) struct Notices(pub Vec<(Seat, Notice)>);
