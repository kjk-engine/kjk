//! Character creation, one screen whose layout follows its step:
//!
//! - class (`classsel`): six buttons, male scoundrel, scout, soldier, then
//!   female soldier, scout, scoundrel; the focused one's gender and class
//!   and its description shown; choosing one takes its recommended scores
//!   and the sex's first portrait;
//! - quick or custom (`qorcpnl` over `maincg`), each described when
//!   focused; quick takes the class's scores and recommended skills;
//! - the path (`quickpnl`: portrait, name, play; `custpnl`: portrait,
//!   abilities, skills, feats, name, play over `maincg`): only the next
//!   step is open; Back undoes the last step done, Cancel all of them;
//! - the pickers, each accepted (the path moves on) or backed out of (what
//!   it changed undone): portrait (`portcust`, cycling the sex's
//!   portraits), abilities (`abchrgen`, 30 points over scores from 8 to
//!   18, a raise costing the score's modifier from 1 to 3), skills
//!   (`skchrgen`, (points + INT modifier) / 2, TSL's not halved, at least
//!   1, times 4; a
//!   class skill costs 1 a rank up to 4, another 2 up to 2; recommended:
//!   the class's skills in its order, each as high as the points go),
//!   feats (`ftchrgen`: the class's first level's picks, listed as KOTOR's
//!   feat chart, a chain a row (`sim::levels::Classes::chart`), each feat
//!   had, picked, open to pick or not; the chosen one named over its
//!   description; Add picks or drops it, Recommended takes the first it
//!   may, recommended first), name (`name`;
//!   Random draws one from the letter tables, `fmt_odyssey::ltr`);
//! - `maincg` shows the class, name, portrait, the character in 3D
//!   (`MODEL_LBL`), and once scores are set vitality, defence, scores and
//!   saves; the portrait picker shows its head in 3D (`LBL_HEAD`).
//!
//! Play starts the game this creation was opened for (`chargen:solo`,
//! `chargen:host:<seats>:<port>`, the campaign's key after it when the menu
//! picked one: `chargen:solo:tsl`) as the character made.
//!
//! Level-up (`levelup`, a script's `ShowLevelUpGUI`) is the same screen
//! on the player's character: `leveluppnl` over `maincg`, abilities every
//! fourth level (one point, a point a raise), skills ((points + INT
//! modifier) / 2, at least 1; up to the level and 3 ranks, half for
//! another class's), feats and powers (`pwrlvlup`) as the level offers
//! them (hidden when it offers none); nothing goes below what the
//! character has. Back from the first step and Finish close it. The feats
//! and powers left unpicked are the game's (the host's `Classes::complete`).

use fmt_odyssey::gui::Control;

use crate::edit::EditBox;
use crate::intent::{CharacterDesc, GameAction, Intent, SessionIntent};
use crate::paint::{Cell, CellLook, ControlState};
use crate::screen::{Screen, ScreenId};
use crate::screens::tab::{disabled, enabled, hidden, image, shown, signed, text, told};
use crate::vm::{CharacterView, ClassView, Entry, PicksView, SceneView, ViewModel};

/// A class button: its tag, female, its `classes.2da` row, its tagline (a
/// strref).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Offer {
    pub button: &'static str,
    pub female: bool,
    pub row: u8,
    pub tagline: u32,
}

const fn offer(button: &'static str, female: bool, (row, tagline): (u8, u32)) -> Offer {
    Offer {
        button,
        female,
        row,
        tagline,
    }
}

/// The classes character creation offers, by game. The executable picks
/// them and their taglines (`classes.2da` `playerclass` is set on every
/// TSL row; no table or layout names a tagline), as reone
/// and xoreos do: the men's three, then the women's mirrored.
/// K1 Scoundrel, Scout, Soldier; TSL Consular, Sentinel, Guardian.
pub const K1_OFFER: [Offer; 6] = {
    let (scoundrel, scout, soldier) = ((2, 32109), (1, 32110), (0, 32111));
    [
        offer("BTN_SEL1", false, scoundrel),
        offer("BTN_SEL2", false, scout),
        offer("BTN_SEL3", false, soldier),
        offer("BTN_SEL4", true, soldier),
        offer("BTN_SEL5", true, scout),
        offer("BTN_SEL6", true, scoundrel),
    ]
};
pub const TSL_OFFER: [Offer; 6] = {
    let (consular, sentinel, guardian) = ((4, 48031), (5, 48032), (3, 48033));
    [
        offer("BTN_SEL1", false, consular),
        offer("BTN_SEL2", false, sentinel),
        offer("BTN_SEL3", false, guardian),
        offer("BTN_SEL4", true, guardian),
        offer("BTN_SEL5", true, sentinel),
        offer("BTN_SEL6", true, consular),
    ]
};

/// The offer of `game`'s character creation.
pub fn offer_of(game: vfs::Game) -> &'static [Offer] {
    match game {
        vfs::Game::Kotor2 => &TSL_OFFER,
        _ => &K1_OFFER,
    }
}

/// Quick and custom's descriptions: the same strrefs in both games' talk
/// tables, each in its own words (the layout's `LB_DESC` names none).
const PATH_TEXT: [u32; 2] = [241, 242];
/// The abilities' tag prefixes and descriptions, strength to charisma (the
/// same strrefs in both games' talk tables).
const ABILITIES: [(&str, u32); 6] = [
    ("STR", 222),
    ("DEX", 223),
    ("CON", 224),
    ("INT", 226),
    ("WIS", 225),
    ("CHA", 227),
];
/// The skills' tags (rank, buttons), by `skills.2da` row.
const SKILLS: [(&str, &str); 8] = [
    ("COMPUTER_USE", "COM"),
    ("DEMOLITIONS", "DEM"),
    ("STEALTH", "STE"),
    ("AWARENESS", "AWA"),
    ("PERSUADE", "PER"),
    ("REPAIR", "REP"),
    ("SECURITY", "SEC"),
    ("TREAT_INJURY", "TRE"),
];
/// The path panels' step buttons, first to last.
const STEP_BUTTONS: [&str; 6] = [
    "BTN_STEPNAME1",
    "BTN_STEPNAME2",
    "BTN_STEPNAME3",
    "BTN_STEPNAME4",
    "BTN_STEPNAME5",
    "BTN_STEPNAME6",
];
/// The frames of a power picked, and of one offered.
const PICKED: &str = "lbl_hex_3";
const OFFERED: &str = "lbl_hex";
/// An offer: what may be picked, how many, those picked.
type PickList<'a> = (&'a [(u16, Entry)], u8, &'a [u16]);
const POINTS: u8 = 30;
const LOWEST: u8 = 8;
const HIGHEST: u8 = 18;
const NAME_LONGEST: usize = 18;
/// The most letters Random gives a first or last name.
const NAME_LETTERS: usize = 8;
const QUICK: [Step; 3] = [Step::Portrait, Step::Name, Step::Play];
/// Level-up's steps (`leveluppnl`): abilities only every fourth level;
/// feats and powers when the level offers some.
const LEVEL_UP: [Step; 5] = [
    Step::Abilities,
    Step::Skills,
    Step::Feats,
    Step::Powers,
    Step::Play,
];
const CUSTOM: [Step; 6] = [
    Step::Portrait,
    Step::Abilities,
    Step::Skills,
    Step::Feats,
    Step::Name,
    Step::Play,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Class,
    QuickOrCustom,
    Path,
    Portrait,
    Abilities,
    Skills,
    Feats,
    Powers,
    Name,
    Play,
}

/// What the steps chose, kept to undo a picker backed out of.
#[derive(Debug, Clone, Default)]
struct Choices {
    portrait: usize,
    abilities: [u8; 6],
    /// Ability points left.
    points: u8,
    skills: [u8; 8],
    name: EditBox,
    feats: Vec<u16>,
    powers: Vec<u16>,
}

/// What the description shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Described {
    Text(u32),
    Skill(usize),
}

/// The game started once the character is made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Then {
    Solo,
    Host {
        seats: u8,
        port: u16,
    },
    /// Not a new character: party member `who`, taking `level`.
    LevelUp {
        who: u32,
        level: u8,
    },
}

pub struct Chargen {
    then: Then,
    /// The campaign the new game starts (none: K1's).
    campaign: Option<vfs::Game>,
    step: Step,
    quick: bool,
    /// Path steps done.
    done: usize,
    female: bool,
    class: u8,
    choices: Choices,
    kept: Choices,
    /// The description shown: the focused control's.
    described: Option<Described>,
    /// The scores and ranks it cannot go below.
    floor: ([u8; 6], [u8; 8]),
    /// The portrait it has, levelling up.
    portrait_fixed: Option<String>,
    /// Levelling up: the level offers no feats, no powers.
    offers_none: (bool, bool),
    /// The row chosen in the feats' or powers' list, and the feat chosen
    /// in a chart row.
    row: Option<usize>,
    cell: usize,
    /// The row and cell clicked (or entered): what Add acts on, outlined.
    /// `row` and `cell` are where the pointer or the cursor rests, read
    /// but not chosen.
    chosen: Option<(usize, usize)>,
    /// Random's draws so far (a name a press).
    draws: u64,
}

/// An ability score's modifier.
fn modifier(score: u8) -> i32 {
    (i32::from(score) - 10).div_euclid(2)
}

/// `row` dropped from `picked` if there, else added while fewer than
/// `most` are.
fn flip(picked: &mut Vec<u16>, row: u16, most: usize) {
    match picked.iter().position(|had| *had == row) {
        Some(at) => {
            picked.remove(at);
        }
        None if picked.len() < most => picked.push(row),
        None => {}
    }
}

/// What raising a score from `score` costs (lowering to it refunds).
fn cost(score: u8) -> u8 {
    modifier(score).clamp(1, 3) as u8
}

/// The class buttons `vm` offers (KOTOR's before it has said).
fn offered(vm: &ViewModel) -> &'static [Offer] {
    vm.chargen
        .as_ref()
        .map_or(&K1_OFFER, |chargen| chargen.offer)
}

fn offer_at(vm: &ViewModel, button: &str) -> Option<Offer> {
    offered(vm)
        .iter()
        .find(|offer| offer.button == button)
        .copied()
}

impl Chargen {
    /// For `solo`, or `host:<seats>:<port>`, each with the campaign's key
    /// after it when it names one (`solo:tsl`).
    pub fn new(then: &str) -> Option<Chargen> {
        let mut words: Vec<&str> = then.split(':').collect();
        let campaign = words.last().and_then(|key| vfs::Game::from_key(key));
        if campaign.is_some() {
            words.pop();
        }
        let then = match words[..] {
            ["solo"] => Then::Solo,
            ["host", seats, port] => Then::Host {
                seats: seats.parse().ok()?,
                port: port.parse().ok()?,
            },
            _ => return None,
        };
        Some(Chargen {
            then,
            campaign,
            step: Step::Class,
            quick: true,
            done: 0,
            female: false,
            class: 0,
            choices: Choices::default(),
            kept: Choices::default(),
            described: None,
            floor: ([LOWEST; 6], [0; 8]),
            portrait_fixed: None,
            offers_none: (false, false),
            row: None,
            cell: 0,
            chosen: None,
            draws: 0,
        })
    }

    /// The party member shown taking the level `vm.levelup` says, from its
    /// scores and ranks.
    pub fn level_up(vm: &ViewModel) -> Option<Chargen> {
        let up = vm.levelup.as_ref()?;
        let me = vm.character.clone().unwrap_or(CharacterView::default());
        let choices = Choices {
            abilities: me.abilities,
            points: 1,
            skills: up.skills,
            name: EditBox::with(&String::from_utf8_lossy(&me.name), NAME_LONGEST),
            ..Choices::default()
        };
        let mut chargen = Chargen {
            then: Then::LevelUp {
                who: up.who,
                level: up.level,
            },
            campaign: None,
            step: Step::Path,
            quick: false,
            done: 0,
            female: false,
            class: up.class,
            kept: choices.clone(),
            choices,
            described: None,
            floor: (me.abilities, up.skills),
            portrait_fixed: me.portrait,
            offers_none: (
                up.picks.feat_count == 0 || up.picks.feats.is_empty(),
                up.picks.power_count == 0 || up.picks.powers.is_empty(),
            ),
            row: None,
            cell: 0,
            chosen: None,
            draws: 0,
        };
        chargen.skip();
        Some(chargen)
    }

    fn levelling(&self) -> bool {
        matches!(self.then, Then::LevelUp { .. })
    }

    fn level(&self) -> u8 {
        match self.then {
            Then::LevelUp { level, .. } => level,
            _ => 1,
        }
    }

    /// Whether the path's step at `index` is left out.
    fn skipped(&self, index: usize) -> bool {
        match self.path().get(index) {
            Some(Step::Abilities) if self.levelling() => !self.level().is_multiple_of(4),
            Some(Step::Feats) => self.levelling() && self.offers_none.0,
            Some(Step::Powers) => self.levelling() && self.offers_none.1,
            _ => false,
        }
    }

    /// On past the steps left out.
    fn skip(&mut self) {
        while self.skipped(self.done) {
            self.done += 1;
        }
    }

    /// What raising a score from `score` costs here.
    fn raise_cost(&self, score: u8) -> u8 {
        if self.levelling() { 1 } else { cost(score) }
    }

    fn class(&self, vm: &ViewModel) -> ClassView {
        vm.chargen
            .as_ref()
            .and_then(|chargen| chargen.classes.get(usize::from(self.class)))
            .cloned()
            .unwrap_or(ClassView::default())
    }

    fn path(&self) -> &'static [Step] {
        match (self.levelling(), self.quick) {
            (true, _) => &LEVEL_UP,
            (false, true) => &QUICK,
            (false, false) => &CUSTOM,
        }
    }

    /// The portraits of the chosen sex.
    fn portraits(&self, vm: &ViewModel) -> Vec<String> {
        vm.chargen.as_ref().map_or(Vec::new(), |chargen| {
            chargen
                .portraits
                .iter()
                .filter(|portrait| portrait.female == self.female)
                .map(|portrait| portrait.texture.clone())
                .collect()
        })
    }

    fn portrait(&self, vm: &ViewModel) -> Option<String> {
        self.portrait_fixed
            .clone()
            .or_else(|| self.portraits(vm).get(self.choices.portrait).cloned())
    }

    /// The character being made as the 3D views show it: whole, or its
    /// head; none levelling up (its portrait is its body's).
    fn figure(&self, vm: &ViewModel, head: bool) -> Option<SceneView> {
        (!self.levelling()).then_some(())?;
        Some(SceneView::Made {
            portrait: self.portrait(vm)?,
            class: self.class,
            head,
        })
    }

    /// What this level may pick.
    fn picks(&self, vm: &ViewModel) -> PicksView {
        if self.levelling() {
            vm.levelup
                .as_ref()
                .map_or(PicksView::default(), |up| up.picks.clone())
        } else {
            self.class(vm).picks
        }
    }

    /// The feats' or the powers' offer: what may be picked, how many, and
    /// those picked.
    fn offer<'a>(&'a self, picks: &'a PicksView, powers: bool) -> PickList<'a> {
        if powers {
            (&picks.powers, picks.power_count, &self.choices.powers)
        } else {
            (&picks.feats, picks.feat_count, &self.choices.feats)
        }
    }

    /// Whether the level offers `feat`.
    fn offered(picks: &PicksView, feat: u16) -> bool {
        picks.feats.iter().any(|(row, _)| *row == feat)
    }

    /// The feat chosen in the chart.
    fn chosen_feat(&self, picks: &PicksView) -> Option<(u16, Entry)> {
        let (row, cell) = self.chosen?;
        picks.chart.get(row)?.get(cell)?.clone()
    }

    /// The feat read: under the pointer, else the chosen one.
    fn read_feat(&self, picks: &PicksView) -> Option<(u16, Entry)> {
        let (row, cell) = self.row.map(|row| (row, self.cell)).or(self.chosen)?;
        picks.chart.get(row)?.get(cell)?.clone()
    }

    /// The feat chart: a chain a row, each feat as it stands.
    fn feat_rows(&self, vm: &ViewModel) -> ControlState {
        let picks = self.picks(vm);
        let look = |feat: u16| {
            if picks.known.contains(&feat) {
                CellLook::Known
            } else if self.choices.feats.contains(&feat) {
                CellLook::Picked
            } else if Chargen::offered(&picks, feat) {
                CellLook::Open
            } else {
                CellLook::Closed
            }
        };
        let rows = picks
            .chart
            .iter()
            .map(|chain| crate::paint::Row {
                cells: chain
                    .iter()
                    .flatten()
                    .map(|(feat, entry)| Cell {
                        icon: entry.icon.clone(),
                        look: look(*feat),
                    })
                    .collect(),
                ..crate::paint::Row::default()
            })
            .collect();
        ControlState {
            list: Some(crate::paint::ListView {
                rows,
                selected: self.chosen.map(|(row, _)| row),
                cell: self.chosen.map_or(0, |(_, cell)| cell),
                hover: self.row.map(|row| (row, self.cell)),
                ..crate::paint::ListView::default()
            }),
            ..ControlState::default()
        }
    }

    /// Add on the chosen feat: dropped if picked, else picked when the
    /// level offers it and picks are left.
    fn toggle_feat(&mut self, vm: &ViewModel) {
        let picks = self.picks(vm);
        let Some((feat, _)) = self.chosen_feat(&picks) else {
            return;
        };
        let most = match Chargen::offered(&picks, feat) {
            true => usize::from(picks.feat_count),
            false => 0,
        };
        flip(&mut self.choices.feats, feat, most);
    }

    /// The powers' list: picked ones framed, the others faded once none
    /// are left to pick.
    fn power_rows(&self, vm: &ViewModel) -> ControlState {
        let picks = self.picks(vm);
        let (offered, count, picked) = self.offer(&picks, true);
        let left = usize::from(count).saturating_sub(picked.len());
        ControlState {
            list: Some(crate::paint::ListView {
                rows: offered
                    .iter()
                    .map(|(row, entry)| {
                        let taken = picked.contains(row);
                        crate::paint::Row {
                            text: entry.name.clone(),
                            icon: entry.icon.clone(),
                            frame: Some(if taken { PICKED } else { OFFERED }.to_string()),
                            dimmed: !taken && left == 0,
                            ..crate::paint::Row::default()
                        }
                    })
                    .collect(),
                selected: self.chosen.map(|(row, _)| row),
                ..crate::paint::ListView::default()
            }),
            ..ControlState::default()
        }
    }

    /// Picks left to make.
    fn picks_left(&self, vm: &ViewModel, powers: bool) -> usize {
        let picks = self.picks(vm);
        let (_, count, picked) = self.offer(&picks, powers);
        usize::from(count).saturating_sub(picked.len())
    }

    /// The chosen feat's or power's description.
    fn pick_text(&self, vm: &ViewModel, powers: bool) -> ControlState {
        let picks = self.picks(vm);
        let (offered, ..) = self.offer(&picks, powers);
        let entry = match powers {
            true => self
                .row
                .or(self.chosen.map(|(row, _)| row))
                .and_then(|row| offered.get(row))
                .cloned(),
            false => self.read_feat(&picks),
        };
        crate::screens::pages::description(
            entry
                .as_ref()
                .map_or(&[][..], |(_, entry)| &entry.description),
        )
    }

    /// Add: the chosen power picked, or dropped if it was.
    fn toggle_power(&mut self, vm: &ViewModel) {
        let picks = self.picks(vm);
        let (offered, count, _) = self.offer(&picks, true);
        let Some(&(row, _)) = self.chosen.and_then(|(at, _)| offered.get(at)) else {
            return;
        };
        flip(&mut self.choices.powers, row, usize::from(count));
    }

    /// Recommended: the first it may pick, as many as it may.
    fn recommend_picks(&mut self, vm: &ViewModel, powers: bool) {
        let picks = self.picks(vm);
        let (offered, count, _) = self.offer(&picks, powers);
        let chosen = offered
            .iter()
            .take(usize::from(count))
            .map(|(row, _)| *row)
            .collect();
        if powers {
            self.choices.powers = chosen;
        } else {
            self.choices.feats = chosen;
        }
    }

    /// Random: a first name of the sex's table and a last name, eight
    /// letters at most each (reone's name entry), from the next draws of
    /// the tables' seed.
    fn random_name(&mut self, vm: &ViewModel) {
        let Some(names) = vm
            .chargen
            .as_ref()
            .and_then(|chargen| chargen.names.as_ref())
        else {
            return;
        };
        self.draws += 1;
        let mut state = names.seed ^ self.draws.wrapping_mul(0x9e37_79b9_7f4a_7c15);
        let mut draw = || {
            // splitmix64, to 24 bits in [0, 1).
            state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            ((z ^ (z >> 31)) >> 40) as f32 / (1u64 << 24) as f32
        };
        let first = if self.female {
            &names.female
        } else {
            &names.male
        };
        let name = format!(
            "{} {}",
            first.name(NAME_LETTERS, &mut draw),
            names.last.name(NAME_LETTERS, &mut draw)
        );
        self.choices.name = EditBox::with(&name, NAME_LONGEST);
    }

    /// Skill points to spend, and those spent.
    fn skill_points(&self, vm: &ViewModel) -> (i32, i32) {
        let class = self.class(vm);
        let int = modifier(self.choices.abilities[3]);
        let first = if self.levelling() { 1 } else { 4 };
        let whole = vm
            .chargen
            .as_ref()
            .is_some_and(|chargen| chargen.whole_skill_points);
        let points = i32::from(class.skill_points) + int;
        let total = if whole { points } else { points / 2 }.max(1) * first;
        let spent = (0..SKILLS.len())
            .map(|skill| {
                let bought = self.choices.skills[skill].saturating_sub(self.floor.1[skill]);
                i32::from(bought) * self.skill_cost(&class, skill)
            })
            .sum();
        (total, spent)
    }

    fn skill_cost(&self, class: &ClassView, skill: usize) -> i32 {
        if class.class_skills[skill] { 1 } else { 2 }
    }

    /// The most ranks at this level: the level and 3, half that for
    /// another class's skill.
    fn skill_cap(&self, class: &ClassView, skill: usize) -> u8 {
        let most = self.level() + 3;
        if class.class_skills[skill] {
            most
        } else {
            most / 2
        }
    }

    fn can_lower_skill(&self, skill: usize) -> bool {
        self.choices.skills[skill] > self.floor.1[skill]
    }

    fn can_raise_skill(&self, vm: &ViewModel, skill: usize) -> bool {
        let class = self.class(vm);
        let (total, spent) = self.skill_points(vm);
        self.choices.skills[skill] < self.skill_cap(&class, skill)
            && spent + self.skill_cost(&class, skill) <= total
    }

    fn can_raise(&self, ability: usize) -> bool {
        let score = self.choices.abilities[ability];
        score < HIGHEST && self.raise_cost(score) <= self.choices.points
    }

    fn can_lower(&self, ability: usize) -> bool {
        self.choices.abilities[ability] > self.floor.0[ability]
    }

    /// Scores, vitality, defence and saves are known (`maincg`).
    fn scored(&self) -> bool {
        match self.step {
            Step::Class | Step::QuickOrCustom => false,
            _ => self.quick || self.levelling() || self.done >= 2,
        }
    }

    fn choose_class(&mut self, female: bool, class: u8, vm: &ViewModel) {
        self.female = female;
        self.class = class;
        self.choices = Choices {
            abilities: self.class(vm).abilities,
            name: EditBox::new(NAME_LONGEST),
            ..Choices::default()
        };
        self.step = Step::QuickOrCustom;
    }

    fn start_path(&mut self, quick: bool, vm: &ViewModel) {
        self.quick = quick;
        self.done = 0;
        let class = self.class(vm);
        self.choices.skills = [0; 8];
        if quick {
            self.choices.abilities = class.abilities;
            self.choices.points = 0;
            self.recommend(vm);
        } else {
            self.choices.abilities = [LOWEST; 6];
            self.choices.points = POINTS;
        }
        self.step = Step::Path;
    }

    fn open_step(&mut self, step: Step, vm: &ViewModel, out: &mut Vec<Intent>) {
        if step == Step::Play && self.levelling() {
            out.push(Intent::Game(self.level_taken()));
            out.push(Intent::Close);
            return;
        }
        if step == Step::Play {
            out.push(self.session(vm).map_or(Intent::Close, Intent::Session));
            return;
        }
        self.kept = self.choices.clone();
        self.described = None;
        // The chart opens on its first feat, as KOTOR's.
        self.row = (step == Step::Feats).then_some(0);
        self.cell = 0;
        self.chosen = None;
        self.step = step;
    }

    /// The class's recommended skills, each as high as the points go in
    /// turn.
    fn recommend(&mut self, vm: &ViewModel) {
        self.choices.skills = [0; 8];
        for skill in self.class(vm).skill_order {
            while skill < SKILLS.len() && self.can_raise_skill(vm, skill) {
                self.choices.skills[skill] += 1;
            }
        }
    }

    /// The description's strref.
    fn description(&self, vm: &ViewModel) -> Option<u32> {
        match self.described? {
            Described::Text(strref) => Some(strref),
            Described::Skill(skill) => vm
                .chargen
                .as_ref()
                .and_then(|chargen| chargen.skill_text.get(skill).copied()),
        }
    }

    /// A picker accepted: the path moves on.
    fn accept(&mut self) {
        if self.step == Step::Abilities {
            self.choices.skills = self.floor.1;
        }
        self.done += 1;
        self.skip();
        self.step = Step::Path;
    }

    /// The level chosen: its class, the ability raised, the ranks added;
    /// the host checks it and picks the feats and powers the screen does
    /// not offer yet.
    fn level_taken(&self) -> GameAction {
        let (scores, ranks) = self.floor;
        let ability = (0..6).find(|&at| self.choices.abilities[at] > scores[at]);
        let who = match self.then {
            Then::LevelUp { who, .. } => who,
            _ => 0,
        };
        GameAction::LevelUp {
            who,
            class: self.class,
            ability: ability.map(|at| at as u8),
            skills: std::array::from_fn(|at| self.choices.skills[at].saturating_sub(ranks[at])),
            feats: self.choices.feats.clone(),
            powers: self.choices.powers.clone(),
        }
    }

    /// The game to start as the character made; none levelling up.
    fn session(&self, vm: &ViewModel) -> Option<SessionIntent> {
        let character = Some(CharacterDesc {
            class: self.class,
            female: self.female,
            portrait: self.portrait(vm).unwrap_or(String::new()),
            abilities: self.choices.abilities,
            skills: self.choices.skills,
            name: self.choices.name.as_str().to_string(),
            feats: self.choices.feats.clone(),
        });
        Some(match self.then {
            Then::Solo => SessionIntent::Solo {
                save: None,
                character,
                campaign: self.campaign,
            },
            Then::Host { seats, port } => SessionIntent::Host {
                save: None,
                seats,
                port,
                character,
                campaign: self.campaign,
            },
            Then::LevelUp { .. } => return None,
        })
    }

    fn back_out(&mut self, out: &mut Vec<Intent>) {
        match self.step {
            Step::Class => {
                // The menu reads K1's data again.
                if self
                    .campaign
                    .is_some_and(|campaign| campaign != vfs::Game::Kotor1)
                {
                    out.push(Intent::Campaign(vfs::Game::Kotor1));
                }
                out.push(Intent::Close);
            }
            Step::QuickOrCustom => self.step = Step::Class,
            Step::Path if self.levelling() => {
                match (0..self.done).rev().find(|index| !self.skipped(*index)) {
                    Some(index) => self.done = index,
                    None => out.push(Intent::Close),
                }
            }
            Step::Path if self.done > 0 => self.done -= 1,
            Step::Path => self.step = Step::QuickOrCustom,
            _ => {
                self.choices = self.kept.clone();
                self.step = Step::Path;
            }
        }
    }

    fn over_sheet(&self) -> bool {
        matches!(self.step, Step::QuickOrCustom | Step::Path | Step::Name)
    }

    /// `maincg`'s controls.
    fn sheet(&self, tag: &str, vm: &ViewModel) -> Option<ControlState> {
        let class = self.class(vm);
        let scores = self.choices.abilities;
        let scored = self.scored();
        if let Some(at) = ABILITIES
            .iter()
            .position(|(ability, _)| tag == format!("{ability}_AB_LBL"))
        {
            return Some(if scored { text(scores[at]) } else { hidden() });
        }
        let saves = [2, 1, 4].map(|ability| modifier(scores[ability]));
        let state = match tag {
            "PORTRAIT_LBL" => image(self.portrait(vm)),
            "MODEL_LBL" => ControlState {
                scene: self.figure(vm, false),
                ..ControlState::default()
            },
            "LBL_CLASS" => ControlState {
                text: Some(class.name.clone()),
                ..ControlState::default()
            },
            "LBL_NAME" => ControlState {
                text: Some(self.choices.name.text.clone()),
                ..ControlState::default()
            },
            "LBL_VIT" if scored => text(i32::from(class.hit_die) + modifier(scores[2])),
            "LBL_DEF" if scored => text(10 + modifier(scores[1])),
            "NEW_FORT_LBL" if scored => text(class.saves[0] + saves[0]),
            "NEW_REFL_LBL" if scored => text(class.saves[1] + saves[1]),
            "NEW_WILL_LBL" if scored => text(class.saves[2] + saves[2]),
            "LBL_VIT" | "LBL_DEF" | "NEW_FORT_LBL" | "NEW_REFL_LBL" | "NEW_WILL_LBL" => hidden(),
            "LBL_LEVEL_VAL" if self.levelling() => text(self.level()),
            "LBL_LEVEL_VAL" => hidden(),
            _ => return None,
        };
        Some(state)
    }

    fn class_state(&self, tag: &str, vm: &ViewModel) -> ControlState {
        match tag {
            "LBL_CLASS" => {
                let mut text = vm.chargen.as_ref().map_or(Vec::new(), |chargen| {
                    chargen.genders[usize::from(self.female)].clone()
                });
                text.push(b' ');
                text.extend(self.class(vm).name);
                ControlState {
                    text: Some(text),
                    ..ControlState::default()
                }
            }
            "LBL_DESC" => told(self.description(vm)),
            // Each class button's figure.
            tag if tag.starts_with("3D_MODEL") => {
                let button = tag.replace("3D_MODEL", "BTN_SEL");
                ControlState {
                    scene: offer_at(vm, &button).map(|offer| SceneView::Class {
                        class: offer.row,
                        female: offer.female,
                    }),
                    ..ControlState::default()
                }
            }
            _ => ControlState::default(),
        }
    }

    fn path_state(&self, tag: &str) -> ControlState {
        let Some(number) = tag
            .strip_prefix("BTN_STEPNAME")
            .and_then(|number| number.parse::<usize>().ok())
        else {
            return ControlState::default();
        };
        ControlState {
            hidden: self.skipped(number - 1),
            disabled: number != self.done + 1,
            selected: number == self.done + 1,
            ..ControlState::default()
        }
    }

    fn abilities_state(&self, tag: &str, vm: &ViewModel) -> ControlState {
        for (at, (ability, _)) in ABILITIES.iter().enumerate() {
            if tag.strip_prefix("LBL_BONUS_") == Some(ability) {
                return text(signed(modifier(self.choices.abilities[at])));
            }
            match tag.strip_prefix(ability) {
                Some("_POINTS_BTN") => {
                    return ControlState {
                        disabled: true,
                        ..text(self.choices.abilities[at])
                    };
                }
                Some("_PLUS_BTN") => return shown(self.can_raise(at)),
                Some("_MINUS_BTN") => return shown(self.can_lower(at)),
                _ => {}
            }
        }
        match tag {
            "REMAINING_SELECTIONS_LBL" => text(self.choices.points),
            "COST_POINTS_LBL" => ABILITIES
                .iter()
                .position(|(_, text)| self.described == Some(Described::Text(*text)))
                .map_or(ControlState::default(), |at| {
                    text(cost(self.choices.abilities[at]))
                }),
            "LB_DESC" => told(self.description(vm)),
            "BTN_ACCEPT" => enabled(self.choices.points == 0),
            _ => ControlState::default(),
        }
    }

    fn skills_state(&self, tag: &str, vm: &ViewModel) -> ControlState {
        for (at, (long, short)) in SKILLS.iter().enumerate() {
            if tag.strip_prefix(long) == Some("_POINTS_BTN") {
                return ControlState {
                    disabled: true,
                    ..text(self.choices.skills[at])
                };
            }
            match tag.strip_prefix(short) {
                Some("_PLUS_BTN") => return shown(self.can_raise_skill(vm, at)),
                Some("_MINUS_BTN") => return shown(self.can_lower_skill(at)),
                _ => {}
            }
        }
        match tag {
            "REMAINING_SELECTIONS_LBL" => {
                let (total, spent) = self.skill_points(vm);
                text(total - spent)
            }
            "COST_POINTS_LBL" => match self.described {
                Some(Described::Skill(skill)) => text(self.skill_cost(&self.class(vm), skill)),
                _ => ControlState::default(),
            },
            "CLASSSKL_LBL" => {
                let class = self.class(vm);
                shown(matches!(
                    self.described,
                    Some(Described::Skill(skill)) if class.class_skills[skill]
                ))
            }
            "LB_DESC" => told(self.description(vm)),
            _ => ControlState::default(),
        }
    }

    fn press_abilities(&mut self, tag: &str, vm: &ViewModel) {
        for (at, (ability, _)) in ABILITIES.iter().enumerate() {
            let score = self.choices.abilities[at];
            match tag.strip_prefix(ability) {
                Some("_PLUS_BTN") if self.can_raise(at) => {
                    self.choices.points -= self.raise_cost(score);
                    self.choices.abilities[at] += 1;
                }
                Some("_MINUS_BTN") if self.can_lower(at) => {
                    self.choices.points += self.raise_cost(score - 1);
                    self.choices.abilities[at] -= 1;
                }
                _ => {}
            }
        }
        match tag {
            "BTN_RECOMMENDED" if !self.levelling() => {
                self.choices.abilities = self.class(vm).abilities;
                self.choices.points = 0;
            }
            "BTN_ACCEPT" if self.choices.points == 0 => self.accept(),
            _ => {}
        }
    }

    fn press_skills(&mut self, tag: &str, vm: &ViewModel) {
        for (at, (_, short)) in SKILLS.iter().enumerate() {
            match tag.strip_prefix(short) {
                Some("_PLUS_BTN") if self.can_raise_skill(vm, at) => self.choices.skills[at] += 1,
                Some("_MINUS_BTN") if self.can_lower_skill(at) => self.choices.skills[at] -= 1,
                _ => {}
            }
        }
        match tag {
            "BTN_RECOMMENDED" if !self.levelling() => self.recommend(vm),
            "BTN_ACCEPT" => self.accept(),
            _ => {}
        }
    }
}

impl Screen for Chargen {
    fn id(&self) -> ScreenId {
        ScreenId(if self.levelling() {
            "levelup"
        } else {
            "chargen"
        })
    }

    fn layout(&self) -> &str {
        match self.step {
            Step::Class => "classsel",
            Step::QuickOrCustom => "qorcpnl",
            Step::Path if self.levelling() => "leveluppnl",
            Step::Path if self.quick => "quickpnl",
            Step::Path => "custpnl",
            Step::Portrait => "portcust",
            Step::Abilities => "abchrgen",
            Step::Skills => "skchrgen",
            Step::Feats => "ftchrgen",
            Step::Powers => "pwrlvlup",
            Step::Name | Step::Play => "name",
        }
    }

    /// The sheet (`maincg`) under the choice of path, the path, and the
    /// name.
    fn under(&self) -> Option<&str> {
        self.over_sheet().then_some("maincg")
    }

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        let tag = control.tag.as_str();
        // The class's recommendation is a new character's.
        if self.levelling() && tag == "BTN_RECOMMENDED" {
            return disabled();
        }
        if self.over_sheet()
            && let Some(state) = self.sheet(tag, vm)
        {
            return state;
        }
        match self.step {
            Step::Class => self.class_state(tag, vm),
            Step::QuickOrCustom if tag == "LB_DESC" => told(self.description(vm)),
            Step::Path => self.path_state(tag),
            Step::Portrait if tag == "LBL_PORTRAIT" => image(self.portrait(vm)),
            Step::Portrait if tag == "LBL_HEAD" => ControlState {
                scene: self.figure(vm, true),
                ..ControlState::default()
            },
            Step::Abilities => self.abilities_state(tag, vm),
            Step::Skills => self.skills_state(tag, vm),
            Step::Feats => match tag {
                "LB_FEATS" => self.feat_rows(vm),
                "LB_DESC" => self.pick_text(vm, false),
                "LBL_NAME" => {
                    self.read_feat(&self.picks(vm))
                        .map_or(ControlState::default(), |(_, entry)| ControlState {
                            text: Some(entry.name),
                            ..ControlState::default()
                        })
                }
                "STD_REMAINING_SELECTIONS_LBL" => text(self.picks_left(vm, false)),
                "BTN_SELECT" => {
                    let picks = self.picks(vm);
                    enabled(self.chosen_feat(&picks).is_some_and(|(feat, _)| {
                        self.choices.feats.contains(&feat) || Chargen::offered(&picks, feat)
                    }))
                }
                _ => ControlState::default(),
            },
            Step::Powers => match tag {
                "LB_POWERS" => self.power_rows(vm),
                "LB_DESC" => self.pick_text(vm, true),
                "REMAINING_SELECTIONS_LBL" => text(self.picks_left(vm, true)),
                "SELECT_BTN" => enabled(self.chosen.is_some()),
                _ => ControlState::default(),
            },
            Step::Name => match tag {
                "NAME_BOX_EDIT" => ControlState {
                    text: Some(self.choices.name.text.clone()),
                    ..ControlState::default()
                },
                "END_BTN" => enabled(!self.choices.name.as_str().trim().is_empty()),
                "BTN_RANDOM" => enabled(
                    vm.chargen
                        .as_ref()
                        .is_some_and(|chargen| chargen.names.is_some()),
                ),
                _ => ControlState::default(),
            },
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, vm: &ViewModel, out: &mut Vec<Intent>) {
        if tag == "BTN_BACK" {
            self.back_out(out);
            return;
        }
        match self.step {
            Step::Class => {
                if let Some(offer) = offer_at(vm, tag) {
                    self.choose_class(offer.female, offer.row, vm);
                }
            }
            Step::QuickOrCustom => match tag {
                "QUICK_CHAR_BTN" => self.start_path(true, vm),
                "CUST_CHAR_BTN" => self.start_path(false, vm),
                _ => {}
            },
            Step::Path => {
                let number = tag
                    .strip_prefix("BTN_STEPNAME")
                    .and_then(|number| number.parse::<usize>().ok());
                if number == Some(self.done + 1)
                    && let Some(step) = self.path().get(self.done)
                {
                    self.open_step(*step, vm, out);
                } else if tag == "BTN_CANCEL" {
                    self.done = 0;
                    self.step = Step::QuickOrCustom;
                }
            }
            Step::Portrait => {
                let count = self.portraits(vm).len().max(1);
                match tag {
                    "BTN_ARRL" => {
                        self.choices.portrait = (self.choices.portrait + count - 1) % count
                    }
                    "BTN_ARRR" => self.choices.portrait = (self.choices.portrait + 1) % count,
                    "BTN_ACCEPT" => self.accept(),
                    _ => {}
                }
            }
            Step::Abilities => self.press_abilities(tag, vm),
            Step::Skills => self.press_skills(tag, vm),
            Step::Feats => match tag {
                "BTN_SELECT" => self.toggle_feat(vm),
                "BTN_RECOMMENDED" => self.recommend_picks(vm, false),
                "BTN_ACCEPT" => self.accept(),
                _ => {}
            },
            Step::Powers => match tag {
                "SELECT_BTN" => self.toggle_power(vm),
                "RECOMMENDED_BTN" => self.recommend_picks(vm, true),
                "ACCEPT_BTN" => self.accept(),
                "BACK_BTN" => self.back_out(out),
                _ => {}
            },
            Step::Name if tag == "END_BTN" && !self.choices.name.as_str().trim().is_empty() => {
                self.accept();
            }
            Step::Name if tag == "BTN_RANDOM" => self.random_name(vm),
            _ => {}
        }
    }

    fn row(&mut self, list: &str, index: usize, _: &ViewModel, _: &mut Vec<Intent>) {
        if matches!(list, "LB_FEATS" | "LB_POWERS") {
            self.row = Some(index);
            let cell = if list == "LB_FEATS" { self.cell } else { 0 };
            self.chosen = Some((index, cell));
        }
    }

    /// A double click on a feat or a power is Add on it.
    fn double(&mut self, tag: &str, vm: &ViewModel, _: &mut Vec<Intent>) {
        match tag {
            "LB_FEATS" => self.toggle_feat(vm),
            "LB_POWERS" => self.toggle_power(vm),
            _ => {}
        }
    }

    /// The pointer went off the list: what it read gives way to the chosen.
    fn left(&mut self, list: &str) {
        if matches!(list, "LB_FEATS" | "LB_POWERS") {
            let (row, cell) = self.chosen.unwrap_or((0, 0));
            self.row = Some(row);
            self.cell = cell;
        }
    }

    fn pointed(&mut self, list: &str, index: usize, _: &ViewModel) {
        if matches!(list, "LB_FEATS" | "LB_POWERS") {
            self.row = Some(index);
        }
    }

    fn cell(&mut self, list: &str, index: usize, cell: usize, _: &ViewModel) {
        if list == "LB_FEATS" {
            self.row = Some(index);
            self.cell = cell;
        }
    }

    fn back(&mut self, out: &mut Vec<Intent>) {
        self.back_out(out);
    }

    fn focused(&mut self, tag: &str, vm: &ViewModel) {
        if self.step == Step::Class
            && let Some(offer) = offer_at(vm, tag)
        {
            self.female = offer.female;
            self.class = offer.row;
            self.described = Some(Described::Text(offer.tagline));
            return;
        }
        let path = match tag {
            "QUICK_CHAR_BTN" => Some(PATH_TEXT[0]),
            "CUST_CHAR_BTN" => Some(PATH_TEXT[1]),
            _ => None,
        };
        let ability = ABILITIES
            .iter()
            .find(|(ability, _)| tag.starts_with(&format!("{ability}_")))
            .map(|(_, text)| *text);
        let skill = SKILLS.iter().position(|(long, short)| {
            tag.starts_with(&format!("{long}_")) || tag.starts_with(&format!("{short}_"))
        });
        let described = path
            .or(ability)
            .map(Described::Text)
            .or(skill.map(Described::Skill));
        if described.is_some() {
            self.described = described;
        }
    }

    fn edit(&mut self, tag: &str) -> Option<&mut EditBox> {
        (self.step == Step::Name && tag == "NAME_BOX_EDIT").then_some(&mut self.choices.name)
    }

    fn typing(&self) -> Option<&str> {
        (self.step == Step::Name).then_some("NAME_BOX_EDIT")
    }

    fn first_focus(&self) -> Option<&str> {
        Some(match self.step {
            Step::Class => "BTN_SEL1",
            Step::QuickOrCustom => "QUICK_CHAR_BTN",
            Step::Path => STEP_BUTTONS.get(self.done)?,
            Step::Abilities => "STR_PLUS_BTN",
            Step::Skills => "COM_PLUS_BTN",
            Step::Name | Step::Play => "END_BTN",
            Step::Portrait | Step::Feats => "BTN_ACCEPT",
            Step::Powers => "ACCEPT_BTN",
        })
    }
}
