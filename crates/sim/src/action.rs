//! What a player asks of the world beside moving and fighting: opening,
//! closing and emptying containers; buying from and selling to a store it
//! trades with; wearing what the party carries, on the seat's own
//! character or, for the host, a companion nobody plays; taking a level (its
//! picks); Security on a lock; closing the party selection screen; making
//! a companion the party's leader; using an object (a door, a container, a
//! placeable, a creature); reviving a downed party member; using an item
//! (on itself, or a droid's on a foe); fitting an upgrade into an item and
//! taking one out; calling or ending a break; readiness for, or forcing, a
//! ready check; a move at the pazaak table; disarming or recovering a
//! trap; putting the weapon in hand away; the host's answer to a guest
//! asking it into a conversation; watching a pazaak match, or no longer;
//! breaking an item down and making one at TSL's workbench; an upgrade
//! bench's changes to an item, made together (its Assemble); exchanging
//! the weapon sets; quitting a TSL swoop race.
//! Actions reach the authority as a seat's entries in a tick's input
//! (`TickInput::act`), in the order the seat sent them; the step applies
//! them after the players' moves and uses, each checked against the world
//! as it is then, but for the holster key, which the moves take before the
//! body moves.

use crate::wire::{DecodeError, Reader, Writer};

/// A module object (a container) by its index in `ModuleData::objects`.
pub type ObjectIndex = u32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Hold a container open (as using it does), from within reach.
    Open(ObjectIndex),
    /// Stop holding a container open.
    Close(ObjectIndex),
    /// Take an item's whole stack (by script object id) out of an open
    /// container.
    Take { container: ObjectIndex, item: u32 },
    /// Take everything left in an open container.
    TakeAll(ObjectIndex),
    /// Put one of an item the party carries (by script object id) into an
    /// open container (KOTOR's container screen gives as well as takes:
    /// the Taris sewers' bait in the rancor's bone pile).
    Put { container: ObjectIndex, item: u32 },
    /// Buy one of an item in the stock of the store traded with (both by
    /// script object id).
    Buy { store: u32, item: u32 },
    /// Sell one of an item the party carries to the store traded with.
    Sell { store: u32, item: u32 },
    /// Stop trading with a store.
    Leave(u32),
    /// Wear one of an item the party carries (by script object id) in an
    /// `INVENTORY_SLOT_*`, on the seat's own character.
    Equip { item: u32, slot: u8 },
    /// Take off what the seat's character wears in a slot.
    Unequip(u8),
    /// Wear one of an item the party carries in an `INVENTORY_SLOT_*` on
    /// party member `who` (by object id): the seat's own character, or,
    /// for the host, a companion no seat plays (I30 P14).
    EquipOn { who: u32, item: u32, slot: u8 },
    /// Take off what party member `who` wears in a slot, by the same rule.
    UnequipFrom { who: u32, slot: u8 },
    /// Take a level with these picks (the level-up screen's).
    LevelUp(crate::levels::Choices),
    /// Party member `who` (by object id) takes a level with these picks, by
    /// the rule of `EquipOn`: the seat's own character, or, for the host, a
    /// companion no seat plays.
    LevelUpOn {
        who: u32,
        choices: crate::levels::Choices,
    },
    /// Security on a locked door or placeable within reach, with a
    /// security spike the party carries (by script object id) or none.
    Unlock {
        object: ObjectIndex,
        spike: Option<u32>,
    },
    /// Close the party selection screen a script showed, with the
    /// companions picked (`NPC_*` numbers, `u8::MAX` for none), or `None`
    /// for the party as it stands (the screen opens with it picked).
    ChooseParty(Option<[u8; 2]>),
    /// Show the party table (the party screen a player opened in play):
    /// the host answers with it (`notice::Screen::PartySelection`), a
    /// screen whose choice changes nothing.
    ShowParty,
    /// Make companion `NPC_*` the party's leader (KOTOR's portrait click),
    /// `u8::MAX` the player again. Its body stays the companion's mind's:
    /// the scripts see the leader (`GetPartyMemberByIndex(0)`).
    SetLeader(u8),
    /// Make party member `body` the party's leader (the HUD's portrait
    /// click, the leader key): a companion in the party by its creature,
    /// or the seat's own character, the player again. As `SetLeader`, the
    /// first seat's only.
    Lead(crate::snapshot::SimId),
    /// Solo mode on or off (KOTOR's solo-mode button): the companions
    /// stand where they are rather than following the leader
    /// (`GetSoloMode`).
    Solo(bool),
    /// Travel to planet `planetary.2da` row on the galaxy map a script
    /// showed (`ShowGalaxyMap`): its travel script runs
    /// (`k_sup_galaxymap`), which asks `GetSelectedPlanet`.
    Travel(u8),
    /// Use an object (KOTOR's click on it) with `verb`, from within the
    /// verb's reach and in sight (`world::uses`).
    Use { object: ObjectIndex, verb: Verb },
    /// Call a break: the world freezes for everyone (P10), unless the party
    /// is in combat.
    Break,
    /// End the break: its caller, or seat 0.
    Resume,
    /// Ready to go where the ready check proposes (P16).
    Ready,
    /// Seat 0: go now, ready or not, once the proposal has stood
    /// `party::FORCE_AFTER_MS`.
    Force,
    /// Revive this downed party member (the target block's Revive, sent as
    /// its hold begins): the seat's body revives it while it holds use
    /// (`world::falls`), if it is down, ahead and within reach.
    Revive(crate::snapshot::SimId),
    /// A move at the pazaak table a script opened (`PlayPazaak`): the first
    /// seat's (I30 P20), checked against the match as it stands.
    Pazaak(crate::pazaak::Move),
    /// Party member `who` (by the equipping rule's whose) uses item `item`
    /// (by script object id: one the party carries, or `who` wears) on
    /// `target` (`who` itself for the inventory's Use Item): the item's
    /// spell cast, one use taken (`script::items::State::use_item`).
    UseItem { who: u32, item: u32, target: u32 },
    /// Fit upgrade `upgrade` (an item the party carries, by script object
    /// id) into `item` (one the party carries, or a member wears whom the
    /// seat dresses), the fitted upgrade of `upgrade.2da` row `replace`
    /// taken out for it (`script::items::State::fit`).
    Fit {
        item: u32,
        upgrade: u32,
        replace: Option<u8>,
    },
    /// Take the upgrade of `upgrade.2da` row `row` out of `item`, back to
    /// the party, by the same rule.
    Unfit { item: u32, row: u8 },
    /// Disarm a trap the party detected, within reach (`world::traps`,
    /// Demolitions over 4.5 s).
    Disarm(ObjectIndex),
    /// Recover it: disarm it and keep its kit (10 harder).
    Recover(ObjectIndex),
    /// Set the party's strongest mine on a door within reach, to blast its
    /// lock (TSL's; `world::traps`).
    Mine(ObjectIndex),
    /// Put the weapon in hand away (the holster key): the seat's body's,
    /// the companion leading's for the first seat, until it attacks or
    /// changes weapons (`combat::Sheath::Holstered`).
    Holster,
    /// The host's answer to a guest asking it into conversation `talk`
    /// (`docs/DIALOGUE.md`): join it (`accept`), or end it.
    /// The first seat's.
    Request { talk: u32, accept: bool },
    /// Watch the match body `player` plays (R9a), or, `u32::MAX`, stop
    /// watching: any seat's, its body free (`script::pazaak`).
    Watch { player: u32 },
    /// At the workbench (or the lab station, `bench.lab`) a script opened,
    /// break one of `item` (one the party carries) down into components
    /// (chemicals), by party member `bench.maker`'s Repair
    /// (`inventory::workshop`).
    Breakdown { bench: Workbench, item: u32 },
    /// Make the item of row `row` of the bench's table, of its components.
    Create { bench: Workbench, row: u16 },
    /// Exchange the weapon sets (the swap key, the HUD's swap button) of the
    /// seat's body: its own character's, the companion leading's for the
    /// first seat (`script::items::State::switch_weapons`; in a fight it
    /// takes the round's second, `combat::SWAP_MS`).
    SwitchWeapons,
    /// Exchange party member `who`'s (the equip screen's swap button), by
    /// the equipping rule (`items::may_dress`).
    SwitchWeaponsOn { who: u32 },
    /// Fight in TSL's Form of `spells.2da` row `row` from now (the HUD's
    /// stance column, `combat::forms`): the seat's body's, one it knows; no
    /// action, no cost, in a fight too.
    Form(u16),
    /// Cast power `row` (`spells.2da`) at `target` (by script object id),
    /// as KOTOR's power list does: the seat's body queues the cast as
    /// `ActionCastSpellAtObject` would, one it knows and can pay for when
    /// the cast begins (`script::Step::cast`). JA's force button
    /// (`Command::force`) casts only the powers JA has a shape for; this
    /// casts any (TSL's Breath Control against the Jekk'Jekk Tarr's gas).
    Cast { row: u16, target: u32 },
    /// The seat opened an in-game screen (the map, the inventory, the
    /// journal, …): the host runs `k_sup_guiopen`, as KOTOR does each time
    /// one opens, which sets the map page's return button
    /// (`SetReturnStrref`).
    ScreenOpened,
    /// Quit the TSL swoop race playing (its pause screen's quit: the global
    /// `GBL_QUIT_SWOOP` set, which the race's heartbeat reads). The first
    /// seat's.
    Forfeit,
    /// An upgrade bench's Assemble: the changes its visit made to `item`
    /// (by the rule of `Fit`), made together, or none of them
    /// (`script::items::State::assemble`).
    Assemble { item: u32, changes: Vec<Fitting> },
    /// A combat arena's order (`world::arena`), on an arena level only.
    Arena(ArenaOrder),
}

/// What a player orders in a combat arena (`world::arena`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArenaOrder {
    /// A creature of this template, hostile, ahead of the seat's body.
    Spawn(String),
    /// Every creature not of the party gone.
    Clear,
    /// The seat's body's god mode (`sheet::flags::IMMORTAL`) on or off.
    God,
    /// The seat's body knocked out in JA's death `n` (1 to 19,
    /// `BOTH_DEATH1`…), revived where it lies `falls::ARENA_REVIVE_MS` later
    /// (a playtester's look at each death's get-up).
    Down(u8),
}

/// One change an upgrade bench's visit makes to an item: an upgrade the
/// party carries fitted, what `replace` names taken out for it (`Fit`'s), or
/// the upgrade of a row (TSL's: a slot) taken out (`Unfit`'s).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fitting {
    In { upgrade: u32, replace: Option<u8> },
    Out(u8),
}

impl Fitting {
    fn encode(&self, out: &mut Writer) {
        let (taken, upgrade, row) = match *self {
            Fitting::In { upgrade, replace } => (0, upgrade, replace.unwrap_or(u8::MAX)),
            Fitting::Out(row) => (1, 0, row),
        };
        out.u8(taken);
        out.u32(upgrade);
        out.u8(row);
    }

    fn decode(input: &mut Reader) -> Result<Fitting, DecodeError> {
        let at = input.at();
        let (taken, upgrade, row) = (input.u8()?, input.u32()?, input.u8()?);
        Ok(match (taken, upgrade) {
            (0, upgrade) => Fitting::In {
                upgrade,
                replace: Some(row).filter(|row| *row != u8::MAX),
            },
            (1, 0) => Fitting::Out(row),
            _ => return Err(input.invalid("fitting", at)),
        })
    }
}

/// The bench an item creation action is at: its table, the lab station's
/// or the workbench's, and whose skills it uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workbench {
    pub list: String,
    pub lab: bool,
    pub maker: u32,
}

impl Workbench {
    fn encode(&self, out: &mut Writer) {
        out.string(&self.list);
        out.u8(u8::from(self.lab));
        out.u32(self.maker);
    }

    fn decode(input: &mut Reader) -> Result<Workbench, DecodeError> {
        Ok(Workbench {
            list: input.string()?,
            lab: input.u8()? != 0,
            maker: input.u32()?,
        })
    }
}

/// What a Use does to its object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    /// A door or a container.
    Open,
    /// A door.
    Close,
    /// A creature or placeable that speaks.
    Talk,
    /// A placeable (`OnUsed`), or what speaks.
    Use,
}

impl Verb {
    pub const ALL: [Verb; 4] = [Verb::Open, Verb::Close, Verb::Talk, Verb::Use];

    fn code(self) -> u8 {
        self as u8
    }

    fn of(code: u8) -> Option<Verb> {
        Verb::ALL.get(usize::from(code)).copied()
    }
}

const OPEN: u8 = 4;
const CLOSE: u8 = 1;
const TAKE: u8 = 2;
const TAKE_ALL: u8 = 3;
const BUY: u8 = 5;
const SELL: u8 = 6;
const LEAVE: u8 = 7;
const EQUIP: u8 = 8;
const UNEQUIP: u8 = 9;
const LEVEL_UP: u8 = 10;
const UNLOCK: u8 = 11;
const CHOOSE_PARTY: u8 = 12;
const SET_LEADER: u8 = 13;
const PUT: u8 = 14;
const TRAVEL: u8 = 15;
const USE: u8 = 16;
const BREAK: u8 = 17;
const EQUIP_ON: u8 = 18;
const UNEQUIP_FROM: u8 = 19;
const RESUME: u8 = 20;
const READY: u8 = 21;
const FORCE: u8 = 22;
const REVIVE: u8 = 23;
const SOLO: u8 = 24;
const USE_ITEM: u8 = 25;
const FIT: u8 = 26;
const UNFIT: u8 = 27;
const PAZAAK: u8 = 30;
const DISARM: u8 = 31;
const RECOVER: u8 = 32;
const LEAD: u8 = 33;
const SHOW_PARTY: u8 = 34;
const HOLSTER: u8 = 35;
const REQUEST: u8 = 36;
const WATCH: u8 = 37;
const MINE: u8 = 38;
const BREAKDOWN: u8 = 39;
const CREATE: u8 = 40;
const ASSEMBLE: u8 = 41;
const SCREEN_OPENED: u8 = 44;
const FORM: u8 = 47;
const SWITCH_WEAPONS: u8 = 51;
const SWITCH_WEAPONS_ON: u8 = 52;
const FORFEIT: u8 = 53;
const ARENA: u8 = 54;
const CAST: u8 = 55;
const LEVEL_UP_ON: u8 = 56;

impl Action {
    pub(crate) fn encode(&self, out: &mut Writer) {
        match self {
            Action::Open(container) => {
                out.u8(OPEN);
                out.u32(*container);
            }
            Action::Close(container) => {
                out.u8(CLOSE);
                out.u32(*container);
            }
            Action::Take { container, item } => {
                out.u8(TAKE);
                out.u32(*container);
                out.u32(*item);
            }
            Action::TakeAll(container) => {
                out.u8(TAKE_ALL);
                out.u32(*container);
            }
            Action::Put { container, item } => {
                out.u8(PUT);
                out.u32(*container);
                out.u32(*item);
            }
            Action::LevelUp(choices) => {
                out.u8(LEVEL_UP);
                choices.encode(out);
            }
            Action::LevelUpOn { who, choices } => {
                out.u8(LEVEL_UP_ON);
                out.u32(*who);
                choices.encode(out);
            }
            Action::Buy { store, item } => {
                out.u8(BUY);
                out.u32(*store);
                out.u32(*item);
            }
            Action::Sell { store, item } => {
                out.u8(SELL);
                out.u32(*store);
                out.u32(*item);
            }
            Action::Leave(store) => {
                out.u8(LEAVE);
                out.u32(*store);
            }
            Action::Equip { item, slot } => {
                out.u8(EQUIP);
                out.u32(*item);
                out.u8(*slot);
            }
            Action::Unequip(slot) => {
                out.u8(UNEQUIP);
                out.u8(*slot);
            }
            Action::EquipOn { who, item, slot } => {
                out.u8(EQUIP_ON);
                out.u32(*who);
                out.u32(*item);
                out.u8(*slot);
            }
            Action::UnequipFrom { who, slot } => {
                out.u8(UNEQUIP_FROM);
                out.u32(*who);
                out.u8(*slot);
            }
            Action::Unlock { object, spike } => {
                out.u8(UNLOCK);
                out.u32(*object);
                out.u8(u8::from(spike.is_some()));
                out.u32(spike.unwrap_or(0));
            }
            Action::Use { object, verb } => {
                out.u8(USE);
                out.u32(*object);
                out.u8(verb.code());
            }
            Action::ChooseParty(picks) => {
                out.u8(CHOOSE_PARTY);
                out.u8(u8::from(picks.is_some()));
                let [a, b] = picks.unwrap_or([u8::MAX; 2]);
                out.u8(a);
                out.u8(b);
            }
            Action::ShowParty => out.u8(SHOW_PARTY),
            Action::ScreenOpened => out.u8(SCREEN_OPENED),
            Action::SetLeader(npc) => {
                out.u8(SET_LEADER);
                out.u8(*npc);
            }
            Action::Lead(body) => {
                out.u8(LEAD);
                out.u32(body.0);
            }
            Action::Solo(on) => {
                out.u8(SOLO);
                out.u8(u8::from(*on));
            }
            Action::Travel(planet) => {
                out.u8(TRAVEL);
                out.u8(*planet);
            }
            Action::Break => out.u8(BREAK),
            Action::Resume => out.u8(RESUME),
            Action::Ready => out.u8(READY),
            Action::Force => out.u8(FORCE),
            Action::Revive(body) => {
                out.u8(REVIVE);
                out.u32(body.0);
            }
            Action::Pazaak(played) => {
                out.u8(PAZAAK);
                played.encode(out);
            }
            Action::UseItem { who, item, target } => {
                out.u8(USE_ITEM);
                out.u32(*who);
                out.u32(*item);
                out.u32(*target);
            }
            Action::Fit {
                item,
                upgrade,
                replace,
            } => {
                out.u8(FIT);
                out.u32(*item);
                out.u32(*upgrade);
                out.u8(replace.unwrap_or(u8::MAX));
            }
            Action::Unfit { item, row } => {
                out.u8(UNFIT);
                out.u32(*item);
                out.u8(*row);
            }
            Action::Disarm(object) => {
                out.u8(DISARM);
                out.u32(*object);
            }
            Action::Recover(object) => {
                out.u8(RECOVER);
                out.u32(*object);
            }
            Action::Mine(object) => {
                out.u8(MINE);
                out.u32(*object);
            }
            Action::Holster => out.u8(HOLSTER),
            Action::Request { talk, accept } => {
                out.u8(REQUEST);
                out.u32(*talk);
                out.u8(u8::from(*accept));
            }
            Action::Watch { player } => {
                out.u8(WATCH);
                out.u32(*player);
            }
            Action::Breakdown { bench, item } => {
                out.u8(BREAKDOWN);
                bench.encode(out);
                out.u32(*item);
            }
            Action::Create { bench, row } => {
                out.u8(CREATE);
                bench.encode(out);
                out.u16(*row);
            }
            Action::SwitchWeapons => out.u8(SWITCH_WEAPONS),
            Action::SwitchWeaponsOn { who } => {
                out.u8(SWITCH_WEAPONS_ON);
                out.u32(*who);
            }
            Action::Form(row) => {
                out.u8(FORM);
                out.u16(*row);
            }
            Action::Cast { row, target } => {
                out.u8(CAST);
                out.u16(*row);
                out.u32(*target);
            }
            Action::Assemble { item, changes } => {
                out.u8(ASSEMBLE);
                out.u32(*item);
                out.u8(changes.len().min(usize::from(u8::MAX)) as u8);
                for change in changes.iter().take(usize::from(u8::MAX)) {
                    change.encode(out);
                }
            }
            Action::Forfeit => out.u8(FORFEIT),
            Action::Arena(order) => {
                out.u8(ARENA);
                match order {
                    ArenaOrder::Spawn(template) => {
                        out.u8(0);
                        out.string(template);
                    }
                    ArenaOrder::Clear => out.u8(1),
                    ArenaOrder::God => out.u8(2),
                    ArenaOrder::Down(n) => {
                        out.u8(3);
                        out.u8(*n);
                    }
                }
            }
        }
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Action, DecodeError> {
        let at = input.at();
        Ok(match input.u8()? {
            OPEN => Action::Open(input.u32()?),
            CLOSE => Action::Close(input.u32()?),
            TAKE => Action::Take {
                container: input.u32()?,
                item: input.u32()?,
            },
            TAKE_ALL => Action::TakeAll(input.u32()?),
            BUY => Action::Buy {
                store: input.u32()?,
                item: input.u32()?,
            },
            SELL => Action::Sell {
                store: input.u32()?,
                item: input.u32()?,
            },
            LEAVE => Action::Leave(input.u32()?),
            EQUIP => Action::Equip {
                item: input.u32()?,
                slot: input.u8()?,
            },
            UNEQUIP => Action::Unequip(input.u8()?),
            EQUIP_ON => Action::EquipOn {
                who: input.u32()?,
                item: input.u32()?,
                slot: input.u8()?,
            },
            UNEQUIP_FROM => Action::UnequipFrom {
                who: input.u32()?,
                slot: input.u8()?,
            },
            LEVEL_UP => Action::LevelUp(crate::levels::Choices::decode(input)?),
            LEVEL_UP_ON => Action::LevelUpOn {
                who: input.u32()?,
                choices: crate::levels::Choices::decode(input)?,
            },
            UNLOCK => {
                let object = input.u32()?;
                let at = input.at();
                let spike = match (input.u8()?, input.u32()?) {
                    (0, 0) => None,
                    (1, item) => Some(item),
                    _ => return Err(input.invalid("spike", at)),
                };
                Action::Unlock { object, spike }
            }
            USE => {
                let object = input.u32()?;
                let at = input.at();
                let verb = Verb::of(input.u8()?).ok_or_else(|| input.invalid("verb", at))?;
                Action::Use { object, verb }
            }
            CHOOSE_PARTY => {
                let at = input.at();
                let picked = input.u8()?;
                let picks = [input.u8()?, input.u8()?];
                Action::ChooseParty(match picked {
                    0 => None,
                    1 => Some(picks),
                    _ => return Err(input.invalid("picks", at)),
                })
            }
            SHOW_PARTY => Action::ShowParty,
            SCREEN_OPENED => Action::ScreenOpened,
            SET_LEADER => Action::SetLeader(input.u8()?),
            LEAD => Action::Lead(crate::snapshot::SimId(input.u32()?)),
            TRAVEL => Action::Travel(input.u8()?),
            SOLO => Action::Solo(input.u8()? != 0),
            PUT => Action::Put {
                container: input.u32()?,
                item: input.u32()?,
            },
            BREAK => Action::Break,
            RESUME => Action::Resume,
            READY => Action::Ready,
            FORCE => Action::Force,
            REVIVE => Action::Revive(crate::snapshot::SimId(input.u32()?)),
            PAZAAK => Action::Pazaak(crate::pazaak::Move::decode(input)?),
            USE_ITEM => Action::UseItem {
                who: input.u32()?,
                item: input.u32()?,
                target: input.u32()?,
            },
            FIT => Action::Fit {
                item: input.u32()?,
                upgrade: input.u32()?,
                replace: Some(input.u8()?).filter(|row| *row != u8::MAX),
            },
            UNFIT => Action::Unfit {
                item: input.u32()?,
                row: input.u8()?,
            },
            DISARM => Action::Disarm(input.u32()?),
            RECOVER => Action::Recover(input.u32()?),
            MINE => Action::Mine(input.u32()?),
            HOLSTER => Action::Holster,
            REQUEST => {
                let talk = input.u32()?;
                let at = input.at();
                let accept = match input.u8()? {
                    0 => false,
                    1 => true,
                    _ => return Err(input.invalid("accept", at)),
                };
                Action::Request { talk, accept }
            }
            WATCH => Action::Watch {
                player: input.u32()?,
            },
            BREAKDOWN => Action::Breakdown {
                bench: Workbench::decode(input)?,
                item: input.u32()?,
            },
            CREATE => Action::Create {
                bench: Workbench::decode(input)?,
                row: input.u16()?,
            },
            FORM => Action::Form(input.u16()?),
            CAST => Action::Cast {
                row: input.u16()?,
                target: input.u32()?,
            },
            SWITCH_WEAPONS => Action::SwitchWeapons,
            SWITCH_WEAPONS_ON => Action::SwitchWeaponsOn { who: input.u32()? },
            ASSEMBLE => {
                let item = input.u32()?;
                let changes = (0..input.u8()?)
                    .map(|_| Fitting::decode(input))
                    .collect::<Result<_, _>>()?;
                Action::Assemble { item, changes }
            }
            FORFEIT => Action::Forfeit,
            ARENA => {
                let at = input.at();
                Action::Arena(match input.u8()? {
                    0 => ArenaOrder::Spawn(input.string()?),
                    1 => ArenaOrder::Clear,
                    2 => ArenaOrder::God,
                    3 => ArenaOrder::Down(input.u8()?),
                    _ => return Err(input.invalid("arena order", at)),
                })
            }
            _ => return Err(input.invalid("action", at)),
        })
    }

    /// The container it is about, if it is about one.
    pub fn container(&self) -> Option<ObjectIndex> {
        match *self {
            Action::Open(container)
            | Action::Close(container)
            | Action::Take { container, .. }
            | Action::TakeAll(container)
            | Action::Put { container, .. } => Some(container),
            Action::Buy { .. }
            | Action::Sell { .. }
            | Action::Leave(_)
            | Action::Equip { .. }
            | Action::Unequip(_)
            | Action::EquipOn { .. }
            | Action::UnequipFrom { .. }
            | Action::LevelUp(_)
            | Action::LevelUpOn { .. }
            | Action::Unlock { .. }
            | Action::ChooseParty(_)
            | Action::ShowParty
            | Action::SetLeader(_)
            | Action::Lead(_)
            | Action::Solo(_)
            | Action::Travel(_)
            | Action::Use { .. }
            | Action::Break
            | Action::Resume
            | Action::Ready
            | Action::Force
            | Action::Revive(_)
            | Action::Pazaak(_)
            | Action::UseItem { .. }
            | Action::Fit { .. }
            | Action::Unfit { .. }
            | Action::Disarm(_)
            | Action::Recover(_)
            | Action::Mine(_)
            | Action::Holster
            | Action::Request { .. }
            | Action::Watch { .. }
            | Action::Breakdown { .. }
            | Action::Create { .. }
            | Action::SwitchWeapons
            | Action::SwitchWeaponsOn { .. }
            | Action::Assemble { .. }
            | Action::Form(_)
            | Action::Cast { .. }
            | Action::ScreenOpened
            | Action::Forfeit
            | Action::Arena(_) => None,
        }
    }

    /// The action as bytes of its own (the network's message).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Writer::default();
        self.encode(&mut out);
        out.0
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Action, DecodeError> {
        let mut input = Reader::new("action", bytes);
        let action = Action::decode(&mut input)?;
        input.finish()?;
        Ok(action)
    }
}
