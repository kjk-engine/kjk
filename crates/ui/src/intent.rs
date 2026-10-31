//! What screens ask of the game. Opening and closing screens the stack
//! does itself; the rest leaves the UI.

/// The port a hosted game listens on unless told otherwise.
pub const DEFAULT_PORT: u16 = 29170;
/// The module a new game of `game` starts in: K1 the Endar Spire's first,
/// TSL the Ebon Hawk's (the executable's choice: reone's
/// end of character generation, xoreos's start of it).
pub const fn new_game_module(game: vfs::Game) -> &'static str {
    match game {
        vfs::Game::Kotor2 => "001ebo",
        _ => "end_m01aa",
    }
}

/// The character made in character creation (`classes.2da` row, sex,
/// portrait texture, scores, skill ranks, name, feats picked: `feat.2da`
/// rows, none for the game's picks).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CharacterDesc {
    pub class: u8,
    pub female: bool,
    pub portrait: String,
    pub abilities: [u8; 6],
    pub skills: [u8; 8],
    pub name: String,
    /// A save from before feats were picked has none: the game's.
    #[serde(default)]
    pub feats: Vec<u16>,
}

/// Start a game, join one, or leave one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionIntent {
    /// A new game (no save) or a save, played alone, as the character
    /// made (none: the default one); a new game of `campaign` (none: K1's;
    /// a save's module names its own).
    Solo {
        save: Option<String>,
        character: Option<CharacterDesc>,
        campaign: Option<vfs::Game>,
    },
    /// The same, with room for `seats` players on `port`.
    Host {
        save: Option<String>,
        seats: u8,
        port: u16,
        character: Option<CharacterDesc>,
        campaign: Option<vfs::Game>,
    },
    /// Someone's game at `address` (`host` or `host:port`), as `name`.
    Join { address: String, name: String },
    /// Back to the main menu.
    ToMenu,
}

/// Something the host decides: about a container by its index among the
/// module's objects, a trade with a store (by object ids), what a party
/// member wears (an item by object id, in a slot), or the
/// character taking a level (its class, the ability raised, the ranks
/// added by skill), or the party a script's selection screen asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameAction {
    Open(u32),
    Close(u32),
    Take {
        container: u32,
        item: u32,
    },
    TakeAll(u32),
    /// One of the party's items put in a container (its give mode).
    Put {
        container: u32,
        item: u32,
    },
    Buy {
        store: u32,
        item: u32,
    },
    Sell {
        store: u32,
        item: u32,
    },
    Leave(u32),
    /// Wear an item the party carries (by object id) in a slot, on party
    /// member `who`: the player's own character, or one the host dresses.
    Equip {
        who: u32,
        item: u32,
        slot: u8,
    },
    /// Take off what party member `who` wears in a slot.
    Unequip {
        who: u32,
        slot: u8,
    },
    /// Exchange party member `who`'s weapon sets (TSL's second set, in both
    /// games), by the same rule.
    SwitchWeapons {
        who: u32,
    },
    /// Party member `who` uses an item the party carries (by object id) on
    /// itself, by the same rule.
    UseItem {
        who: u32,
        item: u32,
    },
    /// The upgrade bench's Assemble: the changes its visit made to `item`,
    /// made together.
    Assemble {
        item: u32,
        changes: Vec<Fitting>,
    },
    /// At the workbench (`bench`), break one of `item` (one the party
    /// carries) down.
    Breakdown {
        bench: BenchDesc,
        item: u32,
    },
    /// At the workbench, make the item of row `row` of its table.
    Create {
        bench: BenchDesc,
        row: u16,
    },
    /// Party member `who` takes a level: the player's own character, or
    /// one the host dresses.
    LevelUp {
        who: u32,
        class: u8,
        ability: Option<u8>,
        skills: [u8; 8],
        /// Feats and powers picked (`feat.2da`, `spells.2da` rows); those
        /// left unpicked the game's.
        feats: Vec<u16>,
        powers: Vec<u16>,
    },
    /// The party selection screen closed: the companions picked
    /// (`NPC_*`, `u8::MAX` for none).
    ChooseParty(Option<[u8; 2]>),
    /// The party screen opened in play: the host tells the party table.
    ShowParty,
    /// An in-game screen opened (the menu's pages, the party screen): the
    /// host runs the game's screen script (`k_sup_guiopen`).
    ScreenOpened,
    /// The galaxy map's Accept: travel to a planet (`planetary.2da` row).
    Travel(u8),
    /// Call a break (P10).
    Break,
    /// End the break.
    Resume,
    /// A move at the pazaak table.
    Pazaak(PazaakMove),
    /// Stop watching another player's pazaak table (R9a: its Leave).
    StopWatching,
}

/// One change an upgrade bench's visit makes (`sim::action::Fitting`): an
/// upgrade the party carries (by object id) fitted, the one of
/// `upgrade.2da` row (TSL's: slot) `replace` taken out for it; or the
/// upgrade of a row (a slot) taken out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fitting {
    In { upgrade: u32, replace: Option<u8> },
    Out(u8),
}

/// The workbench a script opened (`sim::action::Workbench`): its item
/// creation table, the lab station's or not, and the party member whose
/// skills it uses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BenchDesc {
    pub list: String,
    pub lab: bool,
    pub maker: u32,
}

/// A move at the pazaak table (`sim::pazaak::Move`), cards by number (the
/// item's `ModelVariation`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PazaakMove {
    Wager(i32),
    Quit,
    Deck([u8; 10]),
    Play { slot: u8, minus: bool, two: bool },
    EndTurn,
    Stand,
    Forfeit,
}

/// An interface sound, by its `guisounds.2da` row's label (the game reads
/// the row's `soundresref`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GuiSound {
    /// A button or a list's row clicked or activated.
    Clicked,
    /// The focus or the pointer come to a control, or a list's cursor to
    /// a row.
    Entered,
    /// A check box clicked.
    Check,
    /// An in-game screen over the world opened, and the last one closed.
    PanelAdded,
    PanelRemoved,
    /// An item bought, worn, and sold or taken off.
    InventoryAdd,
    InventorySelect,
    InventoryDrop,
    /// A party member may take a level.
    LevelUp,
}

impl GuiSound {
    pub const fn label(self) -> &'static str {
        match self {
            GuiSound::Clicked => "Clicked_Default",
            GuiSound::Entered => "Entered_Default",
            GuiSound::Check => "Checkbox_Check",
            GuiSound::PanelAdded => "Fullscreen_Panel_Added",
            GuiSound::PanelRemoved => "Fullscreen_Panel_Removed",
            GuiSound::InventoryAdd => "Inventory_Add",
            GuiSound::InventorySelect => "Inventory_Select",
            GuiSound::InventoryDrop => "Inventory_Drop",
            GuiSound::LevelUp => "Level_Up_Notify",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intent {
    /// Play an interface sound (the stack hands it to the game in
    /// `Frame::sounds`).
    Sound(GuiSound),
    /// Open the named screen over the current one.
    Open(&'static str),
    /// Close the top screen.
    Close,
    /// Make a bug report with the player's note (`report`).
    Report(String),
    /// Open the named screen for something (`key_entry` for `forward`).
    OpenFor(&'static str, String),
    /// Close the top screen for the named one (the menu bar's tabs).
    Swap(&'static str),
    /// Close every screen.
    CloseAll,
    Session(SessionIntent),
    /// The screens read `game`'s data from now on (the menu's New Game of
    /// one campaign: its character creation's tables and art).
    Campaign(vfs::Game),
    Game(GameAction),
    /// A setting changed: its key and value.
    Setting(String, String),
    /// Show the party's nth member on the in-game pages (`ViewModel::party`).
    View(usize),
    /// A conversation's answer: the nth reply offered (from 0), or none to
    /// end the line where it may be skipped.
    Answer(Option<u8>),
    /// Leave the conversation (R6: its Leave row).
    Leave,
    /// Save the game as `name`, over the save `slot` (its id) or as a new
    /// one.
    Save {
        slot: Option<String>,
        name: String,
    },
    /// Delete the save with this id.
    DeleteSave(String),
    /// The party fallen (I30 P13): the host loads its latest save, for
    /// every seat.
    ReloadLatest,
    /// Leave the program.
    Quit,
}
