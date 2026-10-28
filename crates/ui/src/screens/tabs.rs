//! The in-game menu bar (`top`, KOTOR 2's), under every in-game screen: a
//! tab's icon lit while its screen shows, its button (or the shoulders, or
//! the screen's own key) swapping to another tab, and the party member the
//! pages show: portrait, vitality and force, name, class and level; the
//! other two members' portraits beside it, each choosing its member for the
//! pages (KOTOR 2 makes it the leader; here only the pages follow, as the
//! sim moves one character a player). The pages keep KOTOR 2's opaque
//! frame.

use crate::input::Key;
use crate::intent::Intent;
use crate::paint::ControlState;
use crate::screens::tab::{hidden, image, text};
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

/// The tab a screen is under: the messages are the journal's.
fn tab_of(screen: &str) -> &str {
    if screen == "messages" {
        "journal"
    } else {
        screen
    }
}

/// The other two members' portrait, its frame and its button, by place.
const OTHERS: [[&str; 3]; 2] = [
    ["LBL_CHAR2", "LBL_BACK2", "BTN_CHANGE2"],
    ["LBL_CHAR3", "LBL_BACK3", "BTN_CHANGE3"],
];

/// The bar's party controls with nothing behind them yet: the states over
/// the portraits, a second class.
const PARTY_LATER: [&str; 9] = [
    "LBL_LEVELUP1",
    "LBL_LEVELUP2",
    "LBL_LEVELUP3",
    "LBL_DISABLE1",
    "LBL_DEBILATATED1",
    "LBL_CMBTEFCTINC1",
    "LBL_CMBTEFCTRED1",
    "LBL_TOP_CLASS2",
    "LBL_TOP_CLASS2LEVEL",
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
    if let Some(state) = party(tag, vm) {
        return Some(state);
    }
    let (_, _, screen) = TABS.iter().find(|(_, icon, _)| *icon == tag)?;
    Some(ControlState {
        selected: *screen == tab_of(current),
        ..ControlState::default()
    })
}

/// The party's members other than the one shown, in the party's order.
fn others(vm: &ViewModel) -> Vec<usize> {
    (0..vm.party.len()).filter(|at| *at != vm.viewed).collect()
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
    if let Some((member, _)) = other(tag, vm) {
        out.extend(member.map(Intent::View));
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
            _ => ControlState::default(),
        });
    }
    let me = vm.character.as_ref();
    Some(match tag {
        "LBL_CHAR1" => image(me.and_then(|me| me.portrait.clone())),
        "PB_VIT1" => share(me.map_or((0, 1), |me| me.hp)),
        "PB_FORCE1" => share(me.map_or((0, 1), |me| me.fp)),
        "LBL_CHARNAME" => ControlState {
            text: Some(me.map_or(Vec::new(), |me| me.name.clone())),
            ..ControlState::default()
        },
        "LBL_TOP_CLASS1" => ControlState {
            text: Some(me.map_or(Vec::new(), |me| me.class.clone())),
            ..ControlState::default()
        },
        "LBL_TOP_CLASS1LEVEL" => text(me.map_or(0, |me| me.level)),
        tag if PARTY_LATER.contains(&tag) => hidden(),
        _ => return None,
    })
}
