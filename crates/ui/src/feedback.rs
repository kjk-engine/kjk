//! KOTOR 2's feedback options. Its Feedback page (`optfeedback`) lists
//! eight check boxes, and this module holds them in KOTOR 2's order. Each
//! one has its words and its help from KOTOR 2's talk table, the setting
//! that holds it, and the value it has when nothing has set it. KOTOR 2
//! keeps them in the "Game Options" section of its ini file.

use std::collections::BTreeMap;

use crate::draw::SmallFonts;

/// The Feedback page's screen name.
pub const PAGE: &str = "options_feedback";

/// One check box of the Feedback page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Choice {
    /// The setting that holds it.
    pub key: &'static str,
    /// The row's words: an entry of KOTOR 2's talk table.
    pub name: u32,
    /// What the description box says while the row is pointed at: an
    /// entry of KOTOR 2's talk table.
    pub help: u32,
    /// Whether the option is on when nothing has set it (KOTOR 2's
    /// default).
    pub default: bool,
    /// Whether kjk does what the option says. A row that kjk does not do
    /// yet is drawn greyed, shows KOTOR 2's default, and does not change.
    pub live: bool,
}

/// The equip screen leaves out the items the member may not wear.
pub const HIDE_UNEQUIPPABLE: &str = "feedback.hide_unequippable";
/// A tutorial window that a script shows comes up.
pub const TUTORIALS: &str = "feedback.tutorials";
/// Every line of a conversation is written out, voiced or not.
pub const SUBTITLES: &str = "feedback.subtitles";
/// The HUD shows the minimap.
pub const MINIMAP: &str = "feedback.minimap";
/// Experience and damage float over the creatures' heads.
pub const FLOATING_NUMBERS: &str = "feedback.floating_numbers";
/// The status summary pops up and pauses the game until its OK is pressed.
pub const STATUS_SUMMARY: &str = "feedback.status_summary";
/// The HUD hides its eight menu buttons and their backing.
pub const HIDE_MENU_BUTTONS: &str = "feedback.hide_menu_buttons";
/// Tooltips show.
pub const TOOLTIPS: &str = "feedback.tooltips";

/// The page's rows in KOTOR 2's order, as its executable fills the list.
/// The words, the help and the defaults are KOTOR 2's own.
pub const CHOICES: [Choice; 8] = [
    choice(HIDE_UNEQUIPPABLE, 42279, 42286, false, true),
    choice(TUTORIALS, 42280, 42287, true, true),
    choice(SUBTITLES, 42281, 42288, true, false),
    choice(MINIMAP, 42283, 42289, true, true),
    choice(FLOATING_NUMBERS, 42285, 42291, true, true),
    choice(STATUS_SUMMARY, 42450, 42451, true, true),
    choice(HIDE_MENU_BUTTONS, 48693, 48700, false, true),
    choice(TOOLTIPS, 48696, 48703, true, true),
];

const fn choice(key: &'static str, name: u32, help: u32, default: bool, live: bool) -> Choice {
    Choice {
        key,
        name,
        help,
        default,
        live,
    }
}

/// Whether the feedback option `key` is on in `settings`. A value of
/// `true` or `false` says so. Any other value, or none, gives KOTOR 2's
/// default.
pub fn on(settings: &BTreeMap<String, String>, key: &str) -> bool {
    match settings.get(key).map(String::as_str) {
        Some("true") => true,
        Some("false") => false,
        _ => CHOICES
            .iter()
            .find(|choice| choice.key == key)
            .is_some_and(|choice| choice.default),
    }
}

/// The settings of the options that kjk does, each with its default.
pub fn defaults() -> impl Iterator<Item = (&'static str, &'static str)> {
    CHOICES
        .iter()
        .filter(|choice| choice.live)
        .map(|choice| (choice.key, if choice.default { "true" } else { "false" }))
}

/// What the page's Default button puts back. These are the options that
/// kjk does. KOTOR 2's Default also turns "Use Small Fonts" off, which is
/// the Interface page's Text size set to Auto here.
pub fn reset_keys() -> impl Iterator<Item = &'static str> {
    defaults().map(|(key, _)| key).chain([SmallFonts::SETTING])
}
