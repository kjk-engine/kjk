//! What screens show, filled by the game from the presented snapshot and
//! its settings. Screens read it and nothing else.

/// A session starting: what the loading screen says.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionView {
    /// What is happening ("Connecting to …", "Loading …").
    pub status: String,
    /// The loading image (`load_<module>`), once the module is known.
    pub image: Option<String>,
    /// 0 to 1, when known.
    pub progress: Option<f32>,
    /// Why the last attempt failed, for the message box.
    pub error: Option<String>,
    /// The build ("kjk v0.1.0 (ab12cd34)"), in the main menu's corner.
    pub version: String,
}

/// An action a key does, and the key it is on.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct KeyRow {
    /// The settings' name (`forward`).
    pub action: String,
    pub label: String,
    /// The key's name (`KeyW`).
    pub key: String,
    /// A movement key (the key mapping's first filter), else the game's.
    pub movement: bool,
}

/// An item in an open container: a stack of what is alike (as KOTOR's
/// containers merge them), by the container's slots it holds and how many
/// items it is.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LootRow {
    /// The items of the stack (script object ids).
    pub items: Vec<u32>,
    pub count: u32,
    pub name: Vec<u8>,
    pub icon: Option<String>,
    /// Asked for, not yet confirmed by the host.
    pub pending: bool,
}

/// The container this player holds open, as the snapshot has it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LootView {
    /// Its index among the module's objects.
    pub container: u32,
    pub title: Vec<u8>,
    /// What is left in it.
    pub rows: Vec<LootRow>,
}

/// What an item is, as the inventory's and the store's filters sort it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ItemKind {
    Datapad,
    Weapon,
    /// Worn anywhere but the hands.
    Armour,
    /// Used from the inventory (a medpac, a grenade, a mine).
    Useable,
    /// A plot item.
    Quest,
    #[default]
    Misc,
}

/// A thing listed by name and icon (a skill, a power, an item), with a
/// count or rank where it has one, and its description; an item its kind.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Entry {
    pub name: Vec<u8>,
    pub icon: Option<String>,
    pub count: Option<u32>,
    pub description: Vec<u8>,
    pub kind: Option<ItemKind>,
}

/// A party member (the player's own character, or one the bar brings up),
/// as the in-game screens show it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CharacterView {
    /// Its object id: whom the equip page dresses.
    pub id: u32,
    /// Its class (`classes.2da` row) when its template says (a
    /// companion's); the level-up screen's class.
    pub class_row: Option<u8>,
    pub name: Vec<u8>,
    /// Its class's name, when known.
    pub class: Vec<u8>,
    pub portrait: Option<String>,
    pub level: u8,
    /// Current and most.
    pub hp: (i32, i32),
    pub fp: (i32, i32),
    pub defence: i32,
    /// Strength to charisma.
    pub abilities: [u8; 6],
    /// Fortitude, reflex, will, with abilities.
    pub saves: [i32; 3],
    /// Good and evil, 0 (dark) to 100 (light); 50 is neutral.
    pub alignment: u8,
    /// Right hand, left hand.
    pub weapons: [Option<Entry>; 2],
    /// Damage dice ("1-8") and attack bonus, by hand.
    pub damage: [String; 2],
    pub attack: [i32; 2],
    pub skills: Vec<Entry>,
    pub powers: Vec<Entry>,
    pub feats: Vec<Entry>,
    /// The party's XP, what the next level asks, and whether it may take
    /// it.
    pub xp: i32,
    pub next_xp: Option<i32>,
    pub may_level: bool,
    /// Its next level, while it may take one: the class it takes it in (its
    /// latest) and what it may pick.
    pub next_level: Option<(u8, PicksView)>,
    /// That level with the game's picks (`sim::levels::Classes::auto`): what
    /// Auto Level Up asks.
    pub auto_level: Option<AutoLevelView>,
    /// What the party carries: each item's id, how it shows, where it may
    /// be worn and whether this member may use it.
    pub owned: Vec<Owned>,
    /// What it wears, by `INVENTORY_SLOT_*`.
    pub worn: Vec<(u8, Owned)>,
    /// The party's credits.
    pub credits: i32,
}

/// The game's picks for a level: its class, the ability raised, the skill
/// ranks added, the feats and powers (`classes.2da`, `feat.2da`, `spells.2da`
/// rows).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AutoLevelView {
    pub class: u8,
    pub ability: Option<u8>,
    pub skills: [u8; 8],
    pub feats: Vec<u16>,
    pub powers: Vec<u16>,
}

/// An item in play: its object id, how it shows, the `INVENTORY_SLOT_*`
/// bits it may be worn in, and whether the member shown may use it on
/// itself (the inventory's Use Item); and, by weapon set, whether it may
/// not go in that set's off hand beside what its main hand holds (the
/// hands' pairing, `sim::inventory::pairs`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Owned {
    pub item: u32,
    pub entry: Entry,
    pub slots: u32,
    pub usable: bool,
    pub unpaired: [bool; 2],
}

/// A 3D view a screen shows in a control, as KOTOR draws a model there.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SceneView {
    /// The campaign's main menu model, animated.
    MainMenu,
    /// The party's nth member (`ViewModel::party`), standing on the
    /// character sheet's stage, its backdrop as its alignment (0 dark to
    /// 100 light) poses it.
    Member { nth: usize, alignment: u8 },
    /// An item template's model (a weapon on the upgrade bench).
    Item(String),
    /// Character creation's class (`classes.2da` row) of a gender, as its
    /// first portrait's body in plain clothes.
    Class { class: u8, female: bool },
    /// The character being made: the body player portrait `portrait` (its
    /// texture) gives `class`, in plain clothes, whole (`head` false) or its
    /// head close up (the portrait picker's).
    Made {
        portrait: String,
        class: u8,
        head: bool,
    },
    /// A model by name, looping its `default` clip (the galaxy map's
    /// planet).
    Model(String),
    /// A creature of `appearance.2da` row `appearance`, whole, dressed as
    /// the world dresses it by the item in its body slot, `armour` (an
    /// item template; none: nothing): the party selection's companion.
    Dressed {
        appearance: u16,
        armour: Option<String>,
    },
}

/// A party member as the menu bar and the HUD show it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MemberView {
    pub name: Vec<u8>,
    /// Its class's name, when known.
    pub class: Vec<u8>,
    pub level: u8,
    pub portrait: Option<String>,
    /// Current and most.
    pub hp: (i32, i32),
    pub fp: (i32, i32),
    /// The equip page dresses it: the player's own character, or for the
    /// host a companion no seat plays (I30 P14).
    pub dresses: bool,
    /// The party's XP lets it take a level, and this player gives it (as
    /// `dresses`): the mark over its portraits.
    pub may_level: bool,
    /// What the player playing it is in, its portrait's icon (R10).
    pub busy: Option<Busy>,
    /// Its body (`sim::SimId`): what a click on its portrait or its icon
    /// names.
    pub id: u32,
}

/// What a player is in, as the HUD's portraits show it (R10; the sim's
/// `Busy`): talking, trading, at a pazaak table, asking the host in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Busy {
    Talking,
    Shopping,
    Pazaak,
    Asking,
}

/// The map page: the area's name, its picture and what lies on it, each
/// place fractions across the part of the picture the map uses and down it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MapView {
    pub area: Vec<u8>,
    /// The area's picture (`lbl_map<area>`), where the area has a map and
    /// the install the picture.
    pub picture: Option<String>,
    /// The share of the picture's width the map uses (KOTOR 1's 440 of its
    /// 512 columns, KOTOR 2's all).
    pub used: f32,
    /// Where the player stands and how far its arrow turns from pointing
    /// up (radians, clockwise).
    pub player: Option<([f32; 2], f32)>,
    /// Where the rest of the party stands.
    pub party: Vec<[f32; 2]>,
    /// The notes shown (waypoints' `MapNote`s enabled), in the area's
    /// order.
    pub notes: Vec<MapNoteView>,
    /// What the party has explored of it, where the area keeps that.
    pub fog: Option<FogView>,
    /// The return button's label (a strref) as the scripts set it
    /// (`SetReturnStrref`); the button stays greyed.
    pub return_button: Option<u32>,
    /// Whether a script turned the return button off (`SetReturnStrref`'s
    /// `bShow` false).
    pub return_hidden: bool,
}

/// What the party has explored of an area's map: its cells across and
/// down the part of the picture the map uses, row by row from the top
/// left, explored or not.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FogView {
    pub cells: [usize; 2],
    pub explored: Vec<bool>,
}

/// A note on the map: where, and its words.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MapNoteView {
    pub at: [f32; 2],
    pub text: Vec<u8>,
}

/// A class character creation offers (a `classes.2da` row).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ClassView {
    pub name: Vec<u8>,
    pub hit_die: u8,
    /// Skill points a level, before intelligence.
    pub skill_points: u8,
    /// Its recommended scores, strength to charisma.
    pub abilities: [u8; 6],
    /// Level one's fortitude, reflex and will, before abilities.
    pub saves: [i32; 3],
    /// Which of the eight skills are its own.
    pub class_skills: [bool; 8],
    /// The skills it recommends, first first.
    pub skill_order: Vec<usize>,
    /// What its first level may pick.
    pub picks: PicksView,
}

/// TSL's workbench as its player sees it (`screens::workbench`): the bench,
/// whether upgrading may be chosen, the party's components (chemicals at
/// the lab station), what it may make and what it may break down.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WorkbenchView {
    pub bench: crate::intent::BenchDesc,
    pub upgrade: bool,
    pub stock: i32,
    pub recipes: Vec<RecipeView>,
    pub breakdowns: Vec<BreakdownView>,
}

/// An item the workbench may make: its table's row, the item, its tab
/// (the table's `group`: armour, ranged, melee, lightsaber, other), its
/// price, whether the maker's skill reaches it, the skill and the maker's
/// total in it ("Repair: 5"), how many the party carries.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RecipeView {
    pub row: u16,
    pub entry: Entry,
    pub group: u8,
    pub price: u32,
    pub open: bool,
    pub skill: Vec<u8>,
    pub held: u32,
}

/// An item the party carries that the workbench breaks down: its id, the
/// item, and what it yields.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BreakdownView {
    pub item: u32,
    pub entry: Entry,
    pub yields: u32,
}

/// A portrait a player character may have (a `portraits.2da` row).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PortraitView {
    pub texture: String,
    pub female: bool,
}

/// What character creation chooses from.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChargenView {
    /// The class buttons (`screens::chargen::offer_of`).
    pub offer: &'static [crate::screens::chargen::Offer],
    /// By `classes.2da` row.
    pub classes: Vec<ClassView>,
    /// "Male", "Female".
    pub genders: [Vec<u8>; 2],
    pub portraits: Vec<PortraitView>,
    /// The eight skills' descriptions (strrefs).
    pub skill_text: [u32; 8],
    /// Random's letter tables, when the install has them.
    pub names: Option<NameTables>,
    /// A level's skill points are the class's and the INT modifier whole
    /// (TSL's), not half their sum (K1's): `sim::levels::Classes`.
    pub whole_skill_points: bool,
}

/// The letter tables random names come from (`humanm`, `humanf`,
/// `humanl`), and a seed to draw them with.
#[derive(Debug, Clone, PartialEq)]
pub struct NameTables {
    pub male: fmt_odyssey::ltr::Ltr,
    pub female: fmt_odyssey::ltr::Ltr,
    pub last: fmt_odyssey::ltr::Ltr,
    pub seed: u64,
}

/// An item in a trade: its object id, how it shows (its count none when
/// it never runs out), and its price for one.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Ware {
    pub item: u32,
    pub entry: Entry,
    pub price: i32,
}

/// A trade with a store (`OpenStore`): the store (its object id), what it
/// sells at its prices, what the party may sell it at what it pays, the
/// party's credits, and whether it sells and buys at all.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StoreView {
    pub store: u32,
    pub stock: Vec<Ware>,
    pub offers: Vec<Ware>,
    pub credits: i32,
    pub sells: bool,
    pub buys: bool,
}

/// What kind of item an upgrade bench takes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum UpgradeKind {
    #[default]
    Ranged,
    Lightsaber,
    Melee,
    Armour,
}

/// An item that takes upgrades (by object id), with its slots.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UpgradeItem {
    pub id: u32,
    pub item: Entry,
    /// Its template (its model, in the bench's 3D view).
    pub template: String,
    pub kind: UpgradeKind,
    pub slots: Vec<UpgradeSlot>,
}

/// An upgrade slot: what is in it (an upgrade fitted, with its
/// `upgrade.2da` row to take it out, TSL's its slot; a lightsaber's colour
/// crystal, which stays), what the party carries of its kind, and (TSL's)
/// the slot an upgrade chosen goes into while it is empty.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UpgradeSlot {
    pub fitted: Option<(Option<u8>, Entry)>,
    pub choices: Vec<UpgradeChoice>,
    pub at: Option<u8>,
}

/// An upgrade the party carries that a slot lists: its object id, how it
/// shows, and whether the item takes it (TSL's bench lists one of an
/// upgrade level above the item's, greyed).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UpgradeChoice {
    pub id: u32,
    pub entry: Entry,
    pub fits: bool,
}

/// A level to take (`ShowLevelUpGUI`): who takes it (its object id), the
/// class (a `classes.2da` row) and the level reached.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LevelUpView {
    pub who: u32,
    pub class: u8,
    pub level: u8,
    /// Skill ranks now.
    pub skills: [u8; 8],
    /// The feats and powers it may pick.
    pub picks: PicksView,
}

/// What a level may pick: the feats and powers it may (by `feat.2da` and
/// `spells.2da` row, recommended first, as the pages show them) and how
/// many of each.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PicksView {
    pub feats: Vec<(u16, Entry)>,
    pub feat_count: u8,
    pub powers: Vec<(u16, Entry)>,
    pub power_count: u8,
    /// The feat chart, a chain a row (`sim::levels::Classes::chart`).
    pub chart: Vec<[Option<(u16, Entry)>; 3]>,
    /// The chart's feats the character has (or the level grants).
    pub known: Vec<u16>,
}

/// What KOTOR's HUD shows of the player in play.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HudView {
    /// Current and most.
    pub hp: (i32, i32),
    pub fp: (i32, i32),
    pub portrait: Option<String>,
    /// The action slots, left to right: an icon, and uses left.
    pub actions: Vec<(String, Option<u32>)>,
    /// The key that uses each action, as the screens name it (`3`, `F`).
    pub action_keys: Vec<Vec<u8>>,
    /// The status icons lit (`LBL_JOURNAL`, `LBL_PLOTXP`, …): what the
    /// last status summary told, a moment ago.
    pub lit: Vec<&'static str>,
    /// The character is in stealth mode (KOTOR's stealth toggle lit).
    pub stealth: bool,
    /// The minimap, where the area has a picture.
    pub minimap: Option<MinimapView>,
    /// What the player is in, its portrait's icon (R10).
    pub busy: Option<Busy>,
}

/// The HUD's minimap: the area's picture (`lbl_map<area>`), where the
/// player stands on it (fractions of the whole texture, across and down),
/// how far the arrow turns from pointing up (radians, clockwise), the
/// share of the picture's width the map uses, and what of it the party
/// has explored.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MinimapView {
    pub picture: String,
    pub at: [f32; 2],
    pub spin: f32,
    pub used: f32,
    pub fog: Option<FogView>,
}

/// What the HUD's target block shows (`docs/UI.md`): the target's name,
/// its vitality (a creature's), what may be
/// done to it and which is chosen; `busy` while the player walks there or
/// holds a revive;
/// `progress` (0 to 1) while a revive is under way, of the downed member
/// targeted or, on the downed player's own screen, of itself, its
/// reviver named (I30 P11).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TargetView {
    pub name: Vec<u8>,
    /// A foe: its name and vitality drawn red.
    pub hostile: bool,
    pub hp: Option<(i32, i32)>,
    pub actions: Vec<TargetAction>,
    pub selected: usize,
    pub busy: bool,
    pub progress: Option<f32>,
}

/// One thing that may be done to a target: its icon and its words.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TargetAction {
    pub icon: String,
    pub label: Vec<u8>,
}

/// A computer's conversation (its DLG's `ConversationType` 1) as its
/// screen shows it: the line, the replies offered (none while the line's
/// one reply is silent), and the player's Computer Use and Repair ranks
/// with the spikes and parts the party carries.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ComputerView {
    pub message: Vec<u8>,
    pub replies: Vec<Vec<u8>>,
    pub computer_use: i32,
    pub spikes: u32,
    pub repair: i32,
    pub parts: u32,
    /// The Leave row under the replies and its key, as the conversation
    /// screen's (`DialogView::leave`).
    pub leave: Option<bool>,
    pub leave_key: Vec<u8>,
}

/// A saved game as the save and load screens list it: its folder's id,
/// the name it was given, whose game it is when it is a copy of a host's
/// save (shown beside the name, never part of it), the area's name and
/// module, and when it was saved, as shown.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SaveView {
    pub id: String,
    pub name: String,
    pub from: Option<String>,
    pub area: String,
    pub module: String,
    pub when: String,
}

/// The whole party down (I30 P13): whether this player hosts, whose Load
/// loads the latest save for everyone.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LostView {
    pub host: bool,
}

/// A planet the galaxy map shows (a `planetary.2da` row the scripts made
/// available): its button's tag (`guitag`), name, description and model,
/// and whether the party may travel there.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlanetView {
    pub row: u8,
    pub tag: String,
    pub name: Vec<u8>,
    pub description: Vec<u8>,
    pub model: Option<String>,
    pub selectable: bool,
}

/// The galaxy map a script showed (`ShowGalaxyMap`): the planets it shows,
/// the one it opened on (where the party is), and whether this player
/// decides where it goes (the host, I30 P6).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GalaxyView {
    pub planets: Vec<PlanetView>,
    pub current: u8,
    pub host: bool,
}

/// A companion the party selection screen offers: its portrait
/// (`portraits.2da`'s, by its template's `PortraitId`), its name, and
/// whether the player may pick it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CompanionView {
    pub portrait: Option<String>,
    /// Its `appearance.2da` row and what it wears on its body (an item
    /// template), for its 3D view.
    pub appearance: Option<u16>,
    pub armour: Option<String>,
    pub name: Vec<u8>,
    pub selectable: bool,
}

/// The party table as the party selection screen shows it
/// (`screens::party`, from the host's `sim::notice::Roster`): each
/// companion by `NPC_*` number (none: not available), those in the party,
/// those the screen's script puts in whatever is picked, and whether a
/// script showed it (its choice makes the party; else it only shows).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PartySelectionView {
    /// The companion slots the campaign has (its own layout's).
    pub slots: usize,
    pub companions: Vec<Option<CompanionView>>,
    pub members: Vec<u8>,
    pub forced: Vec<u8>,
    pub open: bool,
}

/// A bark as the bubble shows it (`launcher::bark_view`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BarkView {
    /// The words, in the game's code page.
    pub text: Vec<u8>,
    /// The point over the speaker's head, as shares of the window across
    /// and down; none when the speaker is not drawn.
    pub over: Option<[f32; 2]>,
}

/// The Force power carousel (`screens::power_pick`): every power the
/// player can pick, in the order [ and ] step through them, which is
/// chosen, the line under them (its cost and place), and how much of it is
/// left to fade (1 new, 0 gone).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PowerPickView {
    /// Each power's name, in the game's code page.
    pub names: Vec<Vec<u8>>,
    pub chosen: usize,
    pub under: Vec<u8>,
    pub alpha: f32,
}

/// Words floating over a creature's head (`screens::floaty`,
/// `launcher::floaty_view`): a kill's XP over its dead.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FloatyView {
    /// The words, in the game's code page.
    pub text: Vec<u8>,
    pub color: [f32; 3],
    /// How much of it is left to fade: 1 new, 0 gone.
    pub alpha: f32,
    /// The point over its creature's head, as shares of the window across
    /// and down.
    pub over: [f32; 2],
    /// How many newer ones over the same creature stand under it.
    pub stacked: usize,
}

/// KOTOR's status summary as it stands at the window's right edge
/// (`screens::status_summary`): its rows that apply, each the layout's
/// row (`Row`) with its words in the game's code page ("Experience Points
/// (XP) Received: 200"), in the layout's order.
#[derive(Debug, Clone, PartialEq)]
pub struct SummaryView {
    pub rows: Vec<(crate::screens::status_summary::Row, Vec<u8>)>,
    /// How far it has come in from the right (0 out, 1 in) and how much of
    /// it is left to fade (1 new, 0 gone).
    pub slide: f32,
    pub alpha: f32,
}

/// A cinematic conversation as its screen shows it (`screens::dialog`,
/// `launcher::dialogue_view`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DialogView {
    /// The line said, in the game's code page.
    pub said: Vec<u8>,
    /// The replies offered, "1. …", on the player's turn (the line has
    /// played); none while it plays.
    pub replies: Vec<Vec<u8>>,
    /// The reply highlighted.
    pub focus: u8,
    /// In a session of more than one seat, who points at which reply (its
    /// place, the seats' names); none alone.
    pub marks: Option<Vec<(u8, Vec<u8>)>>,
    /// By place, the replies only the host takes (the story's, I30 P6),
    /// drawn faded: this player points at them; empty where it takes all.
    pub host_only: Vec<bool>,
    /// The Leave row under the replies (R6): none where this player may not
    /// leave (alone, watching, a guest in a party conversation), faded while
    /// the conversation is locked, offered while it is not.
    pub leave: Option<bool>,
    /// The key that leaves, as the row names it.
    pub leave_key: Vec<u8>,
}

/// The requests to join a guest's conversation (R5) as the host's card
/// shows them: each asking guest's name and the NPC's, in the order they
/// came, and the keys that accept and end the first, or why those keys
/// wait (a ready check up, whose ready key is the accept key by default;
/// the host down), shown in their place.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RequestsView {
    pub asks: Vec<(Vec<u8>, Vec<u8>)>,
    pub accept: Vec<u8>,
    pub end: Vec<u8>,
    pub waiting: Option<&'static str>,
}

/// A break and a ready check as this player's screens show them
/// (`launcher::party_calls`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CallsView {
    /// The line over everything: "Paused by player 2", or the ready check's
    /// prompt; none for neither.
    pub banner: Option<String>,
    /// What the in-game menu's break button does.
    pub button: BreakButton,
}

/// The in-game menu's break button (KOTOR's autopause button).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BreakButton {
    #[default]
    Break,
    Resume,
    /// Greyed: the party is in combat, or another player's break this one
    /// cannot end.
    Off,
}

/// A pazaak card as its screens show it: the words on its face and its
/// art (a texture); a hand card whose sign is the player's choice has its
/// face for each choice, by `minus | two << 1` (TSL's ±1/2 alone takes
/// `two`), and its art turned negative.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PazaakCard {
    pub face: String,
    pub art: String,
    pub signed: Option<[String; 4]>,
    pub minus_art: String,
    pub sized: bool,
}

/// One side of the pazaak table: its cards in play, its hand by slot (the
/// opponent's face down: art only), its total, the sets it won, whether it
/// stands.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PazaakSide {
    pub table: Vec<PazaakCard>,
    pub hand: [Option<PazaakCard>; 4],
    pub total: i32,
    pub sets: u8,
    pub stood: bool,
}

/// Which of pazaak's three screens a match is at.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum PazaakStage {
    #[default]
    Wager,
    Setup,
    Game,
}

/// A card the side deck may be chosen from: its number (the item's
/// `ModelVariation`), how many the party has, and how it shows.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PazaakOffer {
    pub card: u8,
    pub count: u8,
    pub look: PazaakCard,
}

/// Another player's pazaak table this player watches (R9a): that player's
/// name, for the title ("Watching Carth"), the name of the key that
/// leaves, for the Leave button ("Leave (L)"), the stage its player is at
/// (the watcher sees the table throughout: a wager or a side deck being
/// chosen is told in words), and why the match is paused, if it is (R39:
/// told in words too, the table kept).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PazaakWatch {
    pub name: Vec<u8>,
    pub leave_key: Vec<u8>,
    pub stage: PazaakStage,
    pub held: Option<PazaakHeld>,
}

/// Why a watched match is paused (R39, `sim::pazaak::Match`): its player's
/// seat dropped (`away`), or its player was hurt and is still in a fight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PazaakHeld {
    Away,
    Hurt,
}

/// A set's or the match's result over the table: its player's talk-table
/// line ("You have won the set."), or a watcher's words ("Carth won the
/// set.").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PazaakTold {
    Line(u32),
    Said(Vec<u8>),
}

/// A pazaak match (`sim::pazaak::Match`) as its screens show it: whether
/// this player watches it rather than plays it, the most it may wager, the
/// cards offered, the two sides (the player's first), whose turn, the set's
/// or match's result shown, and the art of a set won
/// and of the backs of cards.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PazaakView {
    pub stage: PazaakStage,
    pub watching: Option<PazaakWatch>,
    pub most: i32,
    /// "Maximum wager: N", in the game's words.
    pub maximum: Vec<u8>,
    pub offers: Vec<PazaakOffer>,
    pub sides: [PazaakSide; 2],
    /// Whose turn it is (0 the player's, 1 the opponent's); none between
    /// sets.
    pub turn: Option<usize>,
    pub told: Option<PazaakTold>,
    pub mark: String,
}

impl PazaakView {
    /// This player plays the match (none of it is read-only).
    pub fn plays(&self) -> bool {
        self.watching.is_none()
    }
}

/// A way the game's window may be shown (`optresolution`): its mode
/// (`window`, `borderless` or `fullscreen`), its size in the screen's
/// pixels (`1920x1080`; none for borderless, the screen's own), how the
/// list says it, and whether the window is shown so now.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DisplayChoice {
    pub mode: String,
    pub size: String,
    pub label: String,
    pub current: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ViewModel {
    /// The party member the in-game pages show (`viewed`'s).
    pub character: Option<CharacterView>,
    /// The party: the player's own character, then its companions.
    pub party: Vec<MemberView>,
    /// Which of `party` the pages show (the menu bar's portraits choose;
    /// the player's own while no page is open).
    pub viewed: usize,
    /// The party has fallen (`screens::lost`).
    pub lost: Option<LostView>,
    pub map: Option<MapView>,
    /// The players in the session, by seat.
    pub players: Vec<String>,
    pub loot: Option<LootView>,
    /// What the game told this player, oldest first.
    pub feedback: Vec<String>,
    /// The conversation lines and the fighting seen, oldest first (the
    /// messages page's Dialog and Combat tabs).
    pub dialog_log: Vec<String>,
    pub combat_log: Vec<String>,
    /// Each setting by its key (`mouse.sensitivity` = `0.5`).
    pub settings: std::collections::BTreeMap<String, String>,
    pub keys: Vec<KeyRow>,
    /// The window's modes and sizes this screen offers.
    pub displays: Vec<DisplayChoice>,
    pub session: SessionView,
    /// The saved games found, the latest first.
    pub saves: Vec<SaveView>,
    /// A new game's row: the module it starts in and that area's name.
    pub new_game: SaveView,
    /// The campaigns a new game may begin: the games installed, K1's first
    /// (both: the menu asks which, `screens::campaign`).
    pub campaigns: Vec<vfs::Game>,
    /// In play, what a save made now would be (its name the area's, no id
    /// or time yet); none where this player cannot save (a guest).
    pub here: Option<SaveView>,
    /// A save may be loaded during play (alone: a host loads from the
    /// main menu).
    pub load_in_play: bool,
    pub chargen: Option<ChargenView>,
    /// The journal's quests, the latest updated first: the quest's name,
    /// the entry reached (its text), and a count of 1 for one done.
    pub journal: Vec<Entry>,
    pub store: Option<StoreView>,
    pub upgrades: Vec<UpgradeItem>,
    /// TSL's workbench a script opened, while it is open.
    pub workbench: Option<WorkbenchView>,
    pub levelup: Option<LevelUpView>,
    /// In play: the HUD's.
    pub hud: Option<HudView>,
    pub computer: Option<ComputerView>,
    /// A computer's conversation is on a camera feed (`screens::feed`).
    pub camera_feed: bool,
    /// What the player looks at, for the HUD's target block.
    pub target: Option<TargetView>,
    /// The area the door or exit the player faces leads to, as the
    /// game names it (`screens::transition`).
    pub transition: Option<Vec<u8>>,
    /// The crosshair shows (the game in play, its camera behind).
    pub crosshair: bool,
    /// A bark in play (`screens::bark`).
    pub bark: Option<BarkView>,
    /// Words floating over creatures in play (`screens::floaty`).
    pub floaties: Vec<FloatyView>,
    /// The Force power carousel, while the player picks a power
    /// (`screens::power_pick`).
    pub power_pick: Option<PowerPickView>,
    /// The combat arena's line of keys at the top of the window, in the
    /// arena (`screens::power_pick::line`).
    pub arena: Option<Vec<u8>>,
    /// The status summaries up at the right edge, oldest first
    /// (`screens::status_summary`).
    pub summaries: Vec<SummaryView>,
    /// The cinematic conversation this player is in.
    pub dialog: Option<DialogView>,
    /// The guests asking the host in (R5), on the host's card whether it
    /// talks or walks (`screens::dialog::Dialog::card`); none elsewhere.
    pub requests: Option<RequestsView>,
    /// A break and a ready check (P10, P16).
    pub calls: CallsView,
    /// The galaxy map a script showed (`screens::galaxy_map`).
    pub galaxy: Option<GalaxyView>,
    /// The party table the host last told this player
    /// (`screens::party`).
    pub party_selection: Option<PartySelectionView>,
    /// The credits, a block of names each (`credits.2da`)
    /// (`screens::credits`).
    pub credits: Vec<Vec<u8>>,
    /// The pazaak match under way (`screens::pazaak`).
    pub pazaak: Option<PazaakView>,
    /// The pages of the message box a script last showed this player
    /// (`screens::message::Pages`): a tutorial window's
    /// (`ShowTutorialWindow`), a datapad's words (`DisplayDatapad`).
    pub pages: Vec<Vec<u8>>,
}
