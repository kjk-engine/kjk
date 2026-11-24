//! The in-game menu bar (`top`, KOTOR 2's), under every in-game screen: a
//! tab's icon lit while its screen shows, its button (or the shoulders, or
//! the screen's own key) swapping to another tab, and the party member the
//! pages show: portrait, vitality and force, name, class and level; the
//! other two members' portraits beside it, each choosing its member for the
//! pages (KOTOR 2 makes it the leader; here only the pages follow, as the
//! sim moves one character a player); the level-up mark over each portrait
//! whose member may take a level. The pages keep KOTOR 2's opaque frame.

use crate::input::Key;
use crate::intent::Intent;
use crate::paint::ControlState;
use crate::screens::action_bar::ActionBar;
use crate::screens::hud::level_up_mark;
use crate::screens::tab::{Page, hidden, image, text};
use crate::vm::ViewModel;

/// Each tab: its button, its icon, its screen; in the bar's order. KOTOR
/// 2's fifth tab is the party (its icon); the messages are the journal's.
pub const TABS: [(&str, &str, &str); 8] = [
    ("BTN_EQU", "LBLH_EQU", "equip"),
    ("BTN_INV", "LBLH_INV", "inventory"),
    ("BTN_CHAR", "LBLH_CHA", "character"),
    ("BTN_ABI", "LBLH_ABI", "abilities"),
    ("BTN_MSG", "LBLH_MSG", "party"),
    ("BTN_JOU", "LBLH_JOU", "journal"),
    ("BTN_MAP", "LBLH_MAP", "map"),
    ("BTN_OPT", "LBLH_OPT", "options_in_game"),
];

/// The bar's layout.
pub const BAR: &str = "top";

/// The tab a screen is under. The messages are under the journal's tab, and
/// the action bar setup is under the abilities' tab.
fn tab_of(screen: &str) -> &str {
    match screen {
        "messages" => "journal",
        ActionBar::NAME => "abilities",
        _ => screen,
    }
}

/// Whether `screen` is a page under one of the bar's tabs, so that a page's
/// key turns the open menu to it as a tab's button does.
pub fn under_tabs(screen: &str) -> bool {
    let tab = tab_of(screen);
    TABS.iter().any(|(_, _, shown)| *shown == tab)
}

/// The other two members' portrait, its frame, its button and its level-up
/// mark, by place.
const OTHERS: [[&str; 4]; 2] = [
    ["LBL_CHAR2", "LBL_BACK2", "BTN_CHANGE2", "LBL_LEVELUP2"],
    ["LBL_CHAR3", "LBL_BACK3", "BTN_CHANGE3", "LBL_LEVELUP3"],
];

/// The bar's party controls with nothing behind them yet: the states over
/// the portraits.
const PARTY_LATER: [&str; 4] = [
    "LBL_DISABLE1",
    "LBL_DEBILATATED1",
    "LBL_CMBTEFCTINC1",
    "LBL_CMBTEFCTRED1",
];

/// A share of `(current, most)`, 0 to 1.
fn share((current, most): (i32, i32)) -> ControlState {
    ControlState {
        value: Some(current as f32 / most.max(1) as f32),
        ..ControlState::default()
    }
}

/// How the bar shows `tag` while `current` is open; `None` for a control
/// not the bar's.
pub fn state(tag: &str, current: &str, vm: &ViewModel) -> Option<ControlState> {
    party(tag, vm).or_else(|| icon(tag, current))
}

/// How the bar shows the tab icon `tag` while `current` is open: lit when
/// it is `current`'s tab. `None` for a control that is no tab icon.
pub fn icon(tag: &str, current: &str) -> Option<ControlState> {
    let (_, _, screen) = TABS.iter().find(|(_, icon, _)| *icon == tag)?;
    Some(ControlState {
        selected: *screen == tab_of(current),
        ..ControlState::default()
    })
}

/// The bar's controls that show the member the pages are about: the
/// portrait, its frame, the vitality and Force bars, the name, the classes
/// and the level-up mark.
const VIEWED: [&str; 10] = [
    "LBL_BACK1",
    "LBL_CHAR1",
    "PB_VIT1",
    "PB_FORCE1",
    "LBL_CHARNAME",
    "LBL_TOP_CLASS1",
    "LBL_TOP_CLASS2",
    "LBL_TOP_CLASS1LEVEL",
    "LBL_TOP_CLASS2LEVEL",
    "LBL_LEVELUP1",
];

/// Takes the bar's party panel out of `gui`, a layout the bar was put
/// under (`Screen::under`), and leaves the bar's tabs. The party selection
/// screen shows the whole party itself, and its layout has controls with
/// the same tags as the panel's portraits (`LBL_CHAR1` to `LBL_CHAR3`).
/// The bar's controls come first in `gui`, so of two controls with one tag
/// the first is the bar's.
pub fn without_party(gui: &mut fmt_odyssey::gui::Gui) {
    let panel = |tag: &str| {
        VIEWED.contains(&tag)
            || PARTY_LATER.contains(&tag)
            || OTHERS.iter().flatten().any(|other| *other == tag)
    };
    let mut seen: Vec<String> = Vec::new();
    gui.controls.retain(|control| {
        let first = !seen.contains(&control.tag);
        seen.push(control.tag.clone());
        !(first && panel(&control.tag))
    });
}

/// The members the bar's two small portraits show: the party's members
/// other than the one shown, in the party's order. At a base the pages list
/// more than three characters (the companions waiting there). Then the
/// portraits show the one before the shown one and the one after it, going
/// round, so the player can step through them all, as K1's buttons for
/// another character do there.
fn others(vm: &ViewModel) -> Vec<usize> {
    let count = vm.party.len();
    if count > OTHERS.len() + 1 {
        let viewed = vm.viewed.min(count - 1);
        return vec![(viewed + count - 1) % count, (viewed + 1) % count];
    }
    (0..count).filter(|at| *at != vm.viewed).collect()
}

/// The member at the bar's `place`th small portrait, and which of its
/// controls `tag` is.
fn other(tag: &str, vm: &ViewModel) -> Option<(Option<usize>, usize)> {
    OTHERS.iter().enumerate().find_map(|(place, controls)| {
        let control = controls.iter().position(|control| *control == tag)?;
        Some((others(vm).get(place).copied(), control))
    })
}

/// A bar button pressed on `current`: true when it was the bar's.
pub fn press(tag: &str, current: &str, vm: &ViewModel, out: &mut Vec<Intent>) -> bool {
    if let Some((member, control)) = other(tag, vm) {
        if control != 3 {
            out.extend(member.map(Intent::View));
        }
        return true;
    }
    let Some((_, _, screen)) = TABS.iter().find(|(button, _, _)| *button == tag) else {
        return false;
    };
    if *screen != current {
        out.push(Intent::Swap(screen));
    }
    true
}

/// The shoulders: the next or previous tab, around.
pub fn key(key: Key, current: &str, out: &mut Vec<Intent>) -> bool {
    let step = match key {
        Key::NextTab => 1,
        Key::PrevTab => TABS.len() - 1,
        _ => return false,
    };
    let at = TABS
        .iter()
        .position(|(_, _, screen)| *screen == tab_of(current))
        .unwrap_or(0);
    out.push(Intent::Swap(TABS[(at + step) % TABS.len()].2));
    true
}

/// The bar's party panel.
fn party(tag: &str, vm: &ViewModel) -> Option<ControlState> {
    if let Some((member, control)) = other(tag, vm) {
        let member = member.and_then(|at| vm.party.get(at));
        return Some(match (member, control) {
            (None, _) => hidden(),
            (Some(member), 0) => image(member.portrait.clone()),
            (Some(member), 3) => level_up_mark(member.may_level),
            _ => ControlState::default(),
        });
    }
    let me = vm.character.as_ref();
    Some(match tag {
        "LBL_LEVELUP1" => level_up_mark(
            vm.party
                .get(vm.viewed)
                .is_some_and(|member| member.may_level),
        ),
        "LBL_CHAR1" => image(me.and_then(|me| me.portrait.clone())),
        "PB_VIT1" => share(me.map_or((0, 1), |me| me.hp)),
        "PB_FORCE1" => share(me.map_or((0, 1), |me| me.fp)),
        "LBL_CHARNAME" => ControlState {
            text: Some(me.map_or(Vec::new(), |me| me.name.clone())),
            ..ControlState::default()
        },
        "LBL_TOP_CLASS1" | "LBL_TOP_CLASS2" | "LBL_TOP_CLASS1LEVEL" | "LBL_TOP_CLASS2LEVEL" => {
            // Each class it has and its levels in it: a second class only
            // once it has taken one up (a Jedi's on top of its first).
            let at = usize::from(tag.starts_with("LBL_TOP_CLASS2"));
            let class = me.and_then(|me| me.classes.get(at));
            match (class, tag.ends_with("LEVEL")) {
                (None, _) if at > 0 => hidden(),
                (Some((_, levels)), true) => text(levels),
                (None, true) => text(me.map_or(0, |me| me.level)),
                (class, false) => ControlState {
                    text: Some(class.map_or(Vec::new(), |(name, _)| name.clone())),
                    ..ControlState::default()
                },
            }
        }
        tag if PARTY_LATER.contains(&tag) => hidden(),
        _ => return None,
    })
}
