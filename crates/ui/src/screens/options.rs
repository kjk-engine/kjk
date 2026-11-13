//! The options pages (`optionsmain`, `optgameplay`, `optmouse`,
//! `optgraphics`, `optgraphicsadv`, `optsound`, `optfeedback`), each a
//! table of what its controls do: a check box, slider or stepped button
//! bound to a setting, a button that opens another page, or greyed where
//! the game has nothing behind it yet (texture quality, anisotropy, saves
//! and levels for the world state). A page can also be a
//! list of option rows (`Line`). The feedback page is such a list, a check
//! box for each of KOTOR 2's feedback options (`feedback`). KOTOR 2's
//! pages (the skin) have two more check boxes, hidden. The controller's
//! page is ours, on the graphics page's layout, opened from the gameplay
//! page in its unused difficulty button's place. The Interface page is
//! ours too. It is a list on the feedback page's layout, opened from the
//! graphics page in its unused force speed box's place. The sabers' page is
//! ours as well, another list on the feedback page's layout, opened from
//! the gameplay page in its unused "reverse" box's place. Its rows are the
//! saber rules a session played here plays by (`docs/SABER-RULES.md`), the
//! duel options and the hilts KOTOR's lightsabers are drawn with. The main
//! page's unused auto-pause button is Debug: the
//! combat sandbox (only the main menu opens this page; play has its own,
//! `options_in_game`).

use fmt_odyssey::gui::Control;

use crate::draw::{
    CORNER_NOTICES, IMAGE_ASPECT, MenuWidth, SmallFonts, TARGET_TOP, TextScale, Textures,
};
use crate::feedback::{self, Choice};
use crate::input::Key;
use crate::intent::{GuiSound, Intent};
use crate::paint::{ControlState, ListView, Row};
use crate::screen::{Screen, ScreenId};
use crate::screens::key_mapping;
use crate::skin::Source;
use crate::strings;
use crate::vm::ViewModel;

/// What a control of a page does.
#[derive(Clone, Copy)]
enum Role {
    /// A check box: the setting is `true` or `false`.
    Toggle(&'static str),
    /// The same, labelled ours (KOTOR's label says another thing).
    ToggleAs(&'static str, &'static str),
    /// A check box stepped through its choices (value, label). Its label
    /// names the chosen one, and it is ticked unless that is the first.
    Stepped(&'static str, &'static [(&'static str, &'static str)]),
    /// A slider: the setting is 0 to 1.
    Slider(&'static str),
    /// A setting stepped through its choices (value, label): the button
    /// shows the chosen one's label and steps on, its arrows back and on.
    Cycle(&'static str, &'static [(&'static str, &'static str)], Step),
    Open(&'static str),
    /// The same, labelled ours.
    OpenAs(&'static str, &'static str),
    /// A label of the layout's, saying ours.
    Text(&'static str),
    /// A button labelled ours that starts the combat sandbox
    /// (`SessionIntent::Sandbox`).
    Sandbox(&'static str),
    /// Shown, not usable yet.
    Greyed,
    Hidden,
    Back,
    /// The page's settings put back as they were when it opened, then
    /// back.
    Cancel,
    /// The page's settings to their defaults (the launcher knows them).
    Defaults,
    /// A list of option rows (`Line`), as KOTOR 2's Feedback page lists
    /// its check boxes. Activating a row turns its check box on or off,
    /// steps its choice on, or opens its page. The description box tells
    /// the row last pointed at.
    List(&'static [Line]),
}

/// One row of a page built as a list.
#[derive(Clone, Copy)]
struct Line {
    /// The row's words, from KOTOR 2's talk table or ours.
    name: Help,
    /// What the description box says while the row is pointed at.
    help: Help,
    kind: LineKind,
}

/// What a list's row is and does.
#[derive(Clone, Copy)]
enum LineKind {
    /// A check box. The setting `key` is `true` or `false`, and `default`
    /// while it holds neither. A row that is not `live` is an option kjk
    /// does not do yet. It is drawn greyed, shows `default`, and does not
    /// change.
    Check {
        key: &'static str,
        default: bool,
        live: bool,
    },
    /// A setting stepped through its values. The row shows the option's
    /// name and the chosen value ("Menu width: Up to 16:9"). Activating
    /// the row steps on and wraps. Left and Right step back and on while
    /// the row is the list's cursor row.
    Step(&'static str, Values),
    /// A row that opens another page. No page of the game has such a row
    /// yet, so only the probes' page makes one (`SAMPLE`).
    #[cfg_attr(not(feature = "probes"), allow(dead_code))]
    Open(&'static str),
}

/// The values a stepped row goes through, in order.
#[derive(Clone, Copy)]
enum Values {
    /// Each value with its words. An unset setting shows the first.
    Named(&'static [(&'static str, &'static str)]),
    /// Percentages, each shown with a percent sign ("100%"). An unset
    /// setting shows the second field's value.
    Percents(&'static [&'static str], &'static str),
    /// Frame rates, each shown as "60 FPS", and 0 as Off. The row shows the
    /// setting's own rate as the game reads it (`frame_limit`), listed or
    /// not. A rate between two listed ones steps on to the higher and back
    /// to the lower.
    Rates(&'static [&'static str]),
}

impl Values {
    fn count(self) -> usize {
        match self {
            Values::Named(choices) => choices.len(),
            Values::Percents(percents, _) => percents.len(),
            Values::Rates(rates) => rates.len(),
        }
    }

    /// The value at `index`, as the setting holds it.
    fn value(self, index: usize) -> &'static str {
        match self {
            Values::Named(choices) => choices[index].0,
            Values::Percents(percents, _) => percents[index],
            Values::Rates(rates) => rates[index],
        }
    }

    /// `key`'s value as the row shows it.
    fn shown(self, vm: &ViewModel, key: &str) -> String {
        match self {
            Values::Named(choices) => choices[chosen(vm, key, choices)].1.to_string(),
            Values::Percents(percents, _) => strings::percent(percents[self.chosen(vm, key).0]),
            Values::Rates(_) => match frame_limit(setting(vm, key)) {
                Some(rate) => strings::fps(&rate.to_string()),
                None => strings::FRAME_LIMIT_OFF.to_string(),
            },
        }
    }

    /// The value a step from `key`'s value comes to, `back` or on, wrapping:
    /// the next one offered (`offered`). None when no other value is
    /// offered, as the saber rules have none without Movie Duels. A frame
    /// rate that is not listed sits just above the listed rate below it, so
    /// a step back comes to that listed rate.
    fn stepped(self, vm: &ViewModel, key: &str, back: bool) -> Option<usize> {
        let count = self.count();
        let by = if back { count - 1 } else { 1 };
        let (at, listed) = self.chosen(vm, key);
        let from = if back && !listed {
            (at + 1) % count
        } else {
            at
        };
        (1..count)
            .map(|steps| (from + steps * by) % count)
            .find(|next| offered(vm, key, self.value(*next)))
    }

    /// Where `key`'s value is among these values, and whether it is one of
    /// them. A frame rate that is not listed is placed at the highest listed
    /// rate below it.
    fn chosen(self, vm: &ViewModel, key: &str) -> (usize, bool) {
        match self {
            Values::Named(choices) => (chosen(vm, key, choices), true),
            Values::Percents(percents, unset) => {
                let value = vm.settings.get(key).map_or(unset, String::as_str);
                let at = |wanted: &str| percents.iter().position(|percent| *percent == wanted);
                (at(value).or_else(|| at(unset)).unwrap_or(0), true)
            }
            Values::Rates(rates) => {
                let rate = frame_limit(setting(vm, key)).unwrap_or(0);
                let listed = |value: &str| frame_limit(value).unwrap_or(0);
                let below = rates.iter().rposition(|value| listed(value) <= rate);
                let at = below.unwrap_or(0);
                (at, listed(rates[at]) == rate)
            }
        }
    }
}

impl LineKind {
    /// The setting the row changes.
    fn key(self) -> Option<&'static str> {
        match self {
            LineKind::Check { key, .. } | LineKind::Step(key, _) => Some(key),
            LineKind::Open(_) => None,
        }
    }

    /// Whether the row does nothing because Movie Duels is not installed
    /// (`JA_OWN`): a check box of its own, or a stepped row with no other
    /// value offered.
    fn needs_movie_duels(self, vm: &ViewModel) -> bool {
        match self {
            LineKind::Check { key, .. } => without_movie_duels(vm, key),
            LineKind::Step(key, values) => {
                !vm.movie_duels && values.stepped(vm, key, false).is_none()
            }
            LineKind::Open(_) => false,
        }
    }

    /// Whether the row's setting is kept by the session that runs
    /// (`session_note`).
    fn fixed(self, vm: &ViewModel) -> bool {
        self.key()
            .is_some_and(|key| session_note(vm, key).is_some())
    }
}

/// `key`'s value, empty while it is unset.
fn setting<'a>(vm: &'a ViewModel, key: &str) -> &'a str {
    vm.settings.get(key).map_or("", String::as_str)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    Shown,
    Back,
    On,
}

impl Role {
    /// The setting it changes.
    fn key(self) -> Option<&'static str> {
        match self {
            Toggle(key) | ToggleAs(key, _) | Stepped(key, _) | Slider(key) | Cycle(key, ..) => {
                Some(key)
            }
            _ => None,
        }
    }
}

/// What the description box (`LB_DESC`) says while a control has the
/// focus, as KOTOR 2's pages do.
#[derive(Clone, Copy)]
enum Help {
    /// KOTOR 2's own words: an entry of its talk table.
    Told(u32),
    /// Ours, where the setting is not KOTOR 2's.
    Ours(&'static str),
}

struct Spec {
    name: &'static str,
    layout: &'static str,
    roles: &'static [(&'static str, Role)],
    first: &'static str,
    help: &'static [(&'static str, Help)],
}

pub struct Page {
    spec: &'static Spec,
    /// The page's settings when it opened (`Cancel`).
    before: Vec<(&'static str, String)>,
    /// The control with the focus (`Help`).
    focus: Option<String>,
    /// The row of the page's list last pointed at (`List`). It starts at
    /// the first row, as KOTOR 2 opens the page with it.
    row: usize,
}

use Help::{Ours, Told};
use Role::{
    Back, Cancel, Cycle, Defaults, Greyed, Hidden, List, Open, OpenAs, Sandbox, Slider, Stepped,
    Text, Toggle, ToggleAs,
};

/// The combat rules (`sim::combat::saber::SaberRules::name`), as the
/// sabers' page names them after "Combat rules".
const SABER_RULES: &[(&str, &str)] = &[
    ("ja", strings::SABER_RULES_JA),
    ("advanced", strings::SABER_RULES_ADVANCED),
];

/// The hilts a single-bladed KOTOR lightsaber may be drawn with (a `.sab`
/// name, `saber.hilt.single`): KOTOR's own, Jedi Academy's, and a few of
/// Movie Duels' (`kjk sabers` lists them all; one the install lacks leaves
/// KOTOR's, the run's notes say). The sabers' page names them after "Hilt".
const HILTS: &[(&str, &str)] = &[
    ("", strings::HILT_KOTOR),
    ("single_1", strings::HILT_JA),
    ("luke_ep6", strings::HILT_LUKE),
    ("obiwan_ep4", strings::HILT_OBI_WAN),
    ("vader", strings::HILT_VADER),
    ("ahsoka", strings::HILT_AHSOKA),
    ("dooku", strings::HILT_DOOKU),
    ("darksaber_reb", strings::HILT_DARKSABER),
];

/// The same for a double-bladed one (`saber.hilt.double`), named after
/// "Double-bladed hilt".
const STAFFS: &[(&str, &str)] = &[
    ("", strings::HILT_KOTOR),
    ("dual_1", strings::HILT_JA),
    ("bastila_staff", strings::STAFF_BASTILA),
    ("dual_maul", strings::STAFF_MAUL),
    ("inquisitor", strings::STAFF_INQUISITOR),
    ("cal_staff", strings::STAFF_CAL),
];

/// The settings with choices of Movie Duels' own (its stances and hilts),
/// and the choices offered without it installed beside Jedi Academy
/// (`ViewModel::movie_duels`).
const JA_OWN: &[(&str, &[&str])] = &[
    ("game.stance_sets", &[]),
    ("saber.hilt.single", &["", "single_1"]),
    ("saber.hilt.double", &["", "dual_1"]),
];

/// The settings a running session keeps as it started, each with what the
/// page says of it then. While a session runs, their rows are greyed and
/// change nothing. The combat rules' row shows the session's rules
/// (`ViewModel::session_rules`).
const FIXED_IN_SESSION: [(&str, &str); 2] = [
    ("game.saber_rules", strings::SABER_RULES_FIXED),
    ("game.stance_sets", strings::STANCE_SETS_FIXED),
];

/// What the page says of `key` while a session runs and keeps it.
fn session_note(vm: &ViewModel, key: &str) -> Option<&'static str> {
    vm.session_rules.as_ref()?;
    FIXED_IN_SESSION
        .iter()
        .find(|(fixed, _)| *fixed == key)
        .map(|(_, note)| *note)
}

/// The value a running session keeps for the combat rules.
fn kept_by_session<'a>(vm: &'a ViewModel, key: &str) -> Option<&'a str> {
    vm.session_rules
        .as_deref()
        .filter(|_| key == FIXED_IN_SESSION[0].0)
}

/// Whether `key` has choices of Movie Duels' own (`JA_OWN`) and it is not
/// installed.
fn without_movie_duels(vm: &ViewModel, key: &str) -> bool {
    !vm.movie_duels && JA_OWN.iter().any(|(own, _)| *own == key)
}

/// The anti-aliasing setting (`game::temporal`).
const ANTIALIAS: &str = "display.msaa";

/// Whether `value` of `key` may be chosen as far as Movie Duels goes:
/// every value with Movie Duels installed, and without it only those
/// `JA_OWN` lists. A value held back is shown and played as the first.
fn movie_duels_offers(vm: &ViewModel, key: &str, value: &str) -> bool {
    vm.movie_duels
        || JA_OWN
            .iter()
            .find(|(own, _)| *own == key)
            .is_none_or(|(_, values)| values.contains(&value))
}

/// Whether a step comes to `value` of `key`. An anti-aliasing choice is
/// offered when this machine has it (DLSS needs its card). A choice of
/// Movie Duels' own is offered with Movie Duels installed
/// (`movie_duels_offers`).
fn offered(vm: &ViewModel, key: &str, value: &str) -> bool {
    if key == ANTIALIAS {
        return vm.antialias.is_empty() || vm.antialias.iter().any(|offered| offered == value);
    }
    movie_duels_offers(vm, key, value)
}

/// Anti-aliasing: the samples a pixel takes, temporal anti-aliasing, or
/// DLSS (offered where the machine has it, `ViewModel::antialias`).
const SAMPLES: &[(&str, &str)] = &[
    ("1", strings::ANTI_ALIASING_OFF),
    ("2", strings::ANTI_ALIASING_2X),
    ("4", strings::ANTI_ALIASING_4X),
    ("taa", strings::ANTI_ALIASING_TAA),
    ("dlaa", strings::ANTI_ALIASING_DLAA),
    ("dlss_quality", strings::ANTI_ALIASING_DLSS_QUALITY),
    ("dlss_balanced", strings::ANTI_ALIASING_DLSS_BALANCED),
    ("dlss_performance", strings::ANTI_ALIASING_DLSS_PERFORMANCE),
    (
        "dlss_ultra_performance",
        strings::ANTI_ALIASING_DLSS_ULTRA_PERFORMANCE,
    ),
];

/// How far a pad's stick is pushed before it counts (a share of its
/// reach; worn sticks drift further).
/// Shadows: what casts them (`display.shadows`).
const SHADOWS: &[(&str, &str)] = &[
    ("off", strings::SHADOWS_OFF),
    ("low", strings::SHADOWS_LOW),
    ("high", strings::SHADOWS_HIGH),
];

/// Lighting: the original game's lighting, or kjk's own with glow
/// (`display.lighting`).
const LIGHTING: &[(&str, &str)] = &[
    ("classic", strings::LIGHTING_CLASSIC),
    ("enhanced", strings::LIGHTING_ENHANCED),
];

const DEAD_ZONES: &[(&str, &str)] = &[
    ("0.10", strings::DEAD_ZONE_SMALL),
    ("0.15", strings::DEAD_ZONE_MEDIUM),
    ("0.25", strings::DEAD_ZONE_LARGE),
];

/// The camera's presets (`camera_tune`): the four sliders at once.
const CAMERA_PRESETS: &[(&str, &str)] = &[
    ("ja", strings::CAMERA_JA),
    ("shoulder", strings::CAMERA_SHOULDER_PRESET),
    ("kotor", strings::CAMERA_KOTOR),
    ("close", strings::CAMERA_CLOSE),
    ("custom", strings::CAMERA_CUSTOM),
];

/// How closely the camera follows: JA's damping, a stiffer one, none.
const CAMERA_FOLLOW: &[(&str, &str)] = &[
    ("ja", strings::CAMERA_LAG_JA),
    ("tight", strings::CAMERA_LAG_TIGHT),
    ("off", strings::CAMERA_LAG_OFF),
];

const ON_OFF: &[(&str, &str)] = &[
    ("true", strings::BODY_FADE_ON),
    ("false", strings::BODY_FADE_OFF),
];

/// Where the camera goes while a gun is in hand: a preset's framing, or
/// the camera's own.
const GUN_CAMERAS: &[(&str, &str)] = &[
    ("shoulder", strings::GUN_CAMERA_SHOULDER),
    ("close", strings::GUN_CAMERA_CLOSE),
    ("kotor", strings::GUN_CAMERA_KOTOR),
    ("ja", strings::GUN_CAMERA_JA),
    ("same", strings::GUN_CAMERA_SAME),
    ("custom", strings::GUN_CAMERA_CUSTOM),
];

/// How wide the menus are drawn (`MenuWidth`).
const MENU_WIDTHS: &[(&str, &str)] = &[
    ("fill", strings::MENU_WIDTH_FILL),
    ("16x9", strings::MENU_WIDTH_16X9),
    ("4x3", strings::MENU_WIDTH_4X3),
];

/// Which of each font's sheets the text is read from (`SmallFonts`).
const TEXT_SIZES: &[(&str, &str)] = &[
    ("auto", strings::TEXT_SIZE_AUTO),
    ("small", strings::TEXT_SIZE_SMALL),
    ("large", strings::TEXT_SIZE_LARGE),
];

/// The setting of the frame rate limit (`game::display`): frames a second,
/// 0 for none. KOTOR 2 has no such limit, so it is on the Interface page.
pub const FRAME_LIMIT: &str = "display.frame_limit";

/// The frame rate limit a setting holds, as the game reads it: a number of
/// frames a second, none for 0 or anything that is not a number.
pub fn frame_limit(value: &str) -> Option<u32> {
    value.trim().parse().ok().filter(|fps| *fps > 0)
}

/// The frame rate limits offered, from none up to the refresh rates of
/// common screens.
const FRAME_LIMITS: &[&str] = &["0", "30", "60", "90", "120", "144", "165", "240"];

/// How a page changes its layout.
enum Reshape {
    /// Copies of a button (`style`) stacked under a control (`after`).
    Stack((&'static str, &'static str), &'static [&'static str]),
    /// A control moved under another.
    Under(&'static str, &'static str),
    /// A control drawn as another is.
    Like(&'static str, &'static str),
}

/// Each page's changes to its layout: the camera page's three buttons under
/// its sliders, inside their box; the guns' page's preset there too, so it
/// reads as the guns' own; the gameplay page's ways to the camera page and
/// to the sabers' page and the graphics page's way to the Interface page
/// framed buttons, as Mouse Settings and Advanced Options are, not check
/// boxes.
const RESHAPED: [(&str, Reshape); 5] = [
    (
        "options_camera",
        Reshape::Stack(
            ("BTN_ADVANCED", "SLI_MOVIE"),
            &["BTN_GUN", "BTN_FOLLOW", "BTN_FADE"],
        ),
    ),
    (
        "options_camera_guns",
        Reshape::Under("BTN_ADVANCED", "SLI_MOVIE"),
    ),
    (
        "options_gameplay",
        Reshape::Like("CB_REVERSE_INGAME", "BTN_MOUSE"),
    ),
    ("options_gameplay", Reshape::Like("CB_REVERSE", "BTN_MOUSE")),
    (
        "options_graphics",
        Reshape::Like("CB_FORCESPEED", "BTN_ADVANCED"),
    ),
];
/// The gap between a moved or added control and the one above it (pixels).
const RESHAPE_GAP: i32 = 5;

const PAGES: [Spec; 12] = [
    Spec {
        name: "options",
        layout: "optionsmain",
        roles: &[
            ("BTN_GAMEPLAY", Open("options_gameplay")),
            ("BTN_FEEDBACK", Open("options_feedback")),
            ("BTN_GRAPHICS", Open("options_graphics")),
            ("BTN_SOUND", Open("options_sound")),
            ("BTN_AUTOPAUSE", Sandbox(strings::DEBUG)),
            ("BTN_BACK", Back),
        ],
        first: "BTN_GAMEPLAY",
        help: &[],
    },
    Spec {
        name: "options_gameplay",
        layout: "optgameplay",
        roles: &[
            (
                "CB_INVERTCAM",
                ToggleAs("camera.invert", strings::INVERT_CAMERA),
            ),
            ("CB_LEVELUP", Greyed),
            ("CB_AUTOSAVE", Toggle("game.autosave")),
            // KOTOR 2's reverse box, unused: the sabers' page.
            ("CB_REVERSE", OpenAs("options_sabers", strings::SABERS)),
            ("CB_DISABLEMOVE", Hidden),
            // TSL's second reverse box, unused: the camera's page.
            (
                "CB_REVERSE_INGAME",
                OpenAs("options_camera", strings::CAMERA),
            ),
            (
                "BTN_DIFFICULTY",
                OpenAs("options_controller", strings::CONTROLLER),
            ),
            ("BTN_DIFFLEFT", Hidden),
            ("BTN_DIFFRIGHT", Hidden),
            ("BTN_KEYMAP", Open("options_keymap")),
            ("BTN_MOUSE", Open("options_mouse")),
            ("BTN_DEFAULT", Defaults),
            ("BTN_BACK", Back),
        ],
        first: "CB_INVERTCAM",
        help: &[
            ("CB_INVERTCAM", Ours(strings::INVERT_CAMERA_HELP)),
            ("CB_AUTOSAVE", Ours(strings::AUTOSAVE_HELP)),
            ("CB_LEVELUP", Told(42266)),
            ("CB_REVERSE", Ours(strings::SABERS_HELP)),
            ("BTN_DIFFICULTY", Ours(strings::CONTROLLER_HELP)),
            ("CB_REVERSE_INGAME", Ours(strings::CAMERA_HELP)),
            ("BTN_KEYMAP", Told(48698)),
            ("BTN_MOUSE", Told(48721)),
        ],
    },
    Spec {
        name: "options_mouse",
        layout: "optmouse",
        roles: &[
            ("SLI_MOUSESEN", Slider("mouse.sensitivity")),
            // The attacks are bindings of their own (the key mapping).
            ("CB_REVBUTTONS", Hidden),
            ("BTN_DEFAULT", Defaults),
            ("BTN_BACK", Back),
        ],
        first: "SLI_MOUSESEN",
        help: &[("SLI_MOUSESEN", Told(48721))],
    },
    Spec {
        name: "options_controller",
        layout: "optgraphics",
        roles: &[
            ("LBL_TITLE", Text(strings::CONTROLLER_OPTIONS)),
            ("SLI_GAMMA", Slider("pad.sensitivity")),
            ("LBL_GAMMA", Text(strings::LOOK_SPEED)),
            (
                "BTN_RESOLUTION",
                Cycle("pad.deadzone", DEAD_ZONES, Step::Shown),
            ),
            ("CB_SHADOWS", ToggleAs("pad.invert", strings::INVERT_LOOK)),
            ("CB_GRASS", ToggleAs("pad.rumble", strings::VIBRATION)),
            ("CB_FORCESPEED", Hidden),
            ("BTN_ADVANCED", Hidden),
            ("BTN_DEFAULT", Defaults),
            ("BTN_BACK", Back),
        ],
        first: "SLI_GAMMA",
        help: &[
            ("SLI_GAMMA", Ours(strings::LOOK_SPEED_HELP)),
            ("BTN_RESOLUTION", Ours(strings::DEAD_ZONE_HELP)),
            ("CB_SHADOWS", Told(42453)),
            ("CB_GRASS", Told(42290)),
        ],
    },
    Spec {
        name: "options_graphics",
        layout: "optgraphics",
        roles: &[
            ("SLI_GAMMA", Slider("display.gamma")),
            ("BTN_RESOLUTION", Open("options_resolution")),
            ("CB_SHADOWS", Stepped("display.shadows", SHADOWS)),
            ("CB_GRASS", Toggle("display.grass")),
            // KOTOR 2's force speed box, unused: the Interface page.
            (
                "CB_FORCESPEED",
                OpenAs("options_interface", strings::INTERFACE),
            ),
            ("BTN_ADVANCED", Open("options_graphics_advanced")),
            ("BTN_DEFAULT", Defaults),
            ("BTN_BACK", Back),
        ],
        first: "SLI_GAMMA",
        help: &[
            ("SLI_GAMMA", Told(42310)),
            ("BTN_RESOLUTION", Told(48707)),
            ("CB_SHADOWS", Ours(strings::SHADOWS_HELP)),
            ("CB_GRASS", Told(48709)),
            ("BTN_ADVANCED", Ours(strings::ADVANCED_GRAPHICS_HELP)),
            ("CB_FORCESPEED", Ours(strings::INTERFACE_HELP)),
        ],
    },
    Spec {
        name: "options_graphics_advanced",
        layout: "optgraphicsadv",
        roles: &[
            ("BTN_ANTIALIAS", Cycle("display.msaa", SAMPLES, Step::Shown)),
            (
                "BTN_ANTIALIASLEFT",
                Cycle("display.msaa", SAMPLES, Step::Back),
            ),
            (
                "BTN_ANTIALIASRIGHT",
                Cycle("display.msaa", SAMPLES, Step::On),
            ),
            // V-Sync sits in the row of KOTOR 2's Soft Shadows box, which
            // kjk does not use (Shadows has a Low and a High), so the page
            // has no empty row above it.
            ("CB_VSYNC", Hidden),
            // Lighting sits in KOTOR 2's Frame Buffer Effects box. That box
            // turned the game's own glow on and off.
            ("CB_FRAMEBUFF", Stepped("display.lighting", LIGHTING)),
            ("BTN_TEXQUAL", Greyed),
            ("BTN_TEXQUALLEFT", Greyed),
            ("BTN_TEXQUALRIGHT", Greyed),
            ("BTN_ANISOTROPY", Greyed),
            ("BTN_ANISOTROPYLEFT", Greyed),
            ("BTN_ANISOTROPYRIGHT", Greyed),
            ("CB_SOFTSHADOWS", ToggleAs("display.vsync", strings::V_SYNC)),
            ("BTN_DEFAULT", Defaults),
            ("BTN_CANCEL", Cancel),
            ("BTN_BACK", Back),
        ],
        first: "BTN_ANTIALIAS",
        help: &[
            ("BTN_ANTIALIAS", Ours(strings::ANTI_ALIASING_HELP)),
            ("BTN_ANTIALIASLEFT", Ours(strings::ANTI_ALIASING_HELP)),
            ("BTN_ANTIALIASRIGHT", Ours(strings::ANTI_ALIASING_HELP)),
            ("BTN_TEXQUAL", Told(48712)),
            ("CB_FRAMEBUFF", Ours(strings::LIGHTING_HELP)),
            ("BTN_ANISOTROPY", Told(48715)),
            ("CB_SOFTSHADOWS", Told(48716)),
        ],
    },
    // Ours, a list on the feedback page's layout: how the menus are drawn,
    // and kjk's own changes to KOTOR 2's interface (`INTERFACE`).
    Spec {
        name: "options_interface",
        layout: "optfeedback",
        roles: &[
            ("LBL_TITLE", Text(strings::INTERFACE)),
            ("LB_OPTIONS", List(&INTERFACE)),
            ("BTN_DEFAULT", Defaults),
            ("BTN_BACK", Back),
        ],
        first: "LB_OPTIONS",
        help: &[],
    },
    // Ours, a list on the feedback page's layout as well: the saber rules a
    // session plays by, the duel options and the hilts (`SABERS`).
    Spec {
        name: "options_sabers",
        layout: "optfeedback",
        roles: &[
            ("LBL_TITLE", Text(strings::SABER_OPTIONS)),
            ("LB_OPTIONS", List(&SABERS)),
            ("BTN_DEFAULT", Defaults),
            ("BTN_BACK", Back),
        ],
        first: "LB_OPTIONS",
        help: &[],
    },
    Spec {
        name: "options_sound",
        layout: "optsound",
        roles: &[
            ("SLI_MUSIC", Slider("sound.music")),
            ("SLI_VO", Slider("sound.voice")),
            ("SLI_FX", Slider("sound.effects")),
            ("SLI_MOVIE", Slider("sound.movie")),
            ("BTN_ADVANCED", Greyed),
            ("BTN_DEFAULT", Defaults),
            ("BTN_BACK", Back),
        ],
        first: "SLI_MUSIC",
        help: &[
            ("SLI_MUSIC", Told(42311)),
            ("SLI_VO", Told(42312)),
            ("SLI_FX", Told(42313)),
            ("SLI_MOVIE", Told(47921)),
        ],
    },
    // Ours, on the sound page's layout: the camera's four sliders, its
    // preset, how closely it follows and whether the body fades near it.
    Spec {
        name: "options_camera",
        layout: "optsound",
        roles: &[
            ("LBL_TITLE", Text(strings::CAMERA_OPTIONS)),
            ("LBL_MUSIC", Text(strings::CAMERA_DISTANCE)),
            ("SLI_MUSIC", Slider("camera.range")),
            ("LBL_VO", Text(strings::CAMERA_HEIGHT)),
            ("SLI_VO", Slider("camera.height")),
            ("LBL_FX", Text(strings::CAMERA_SHOULDER)),
            ("SLI_FX", Slider("camera.shoulder")),
            ("LBL_MOVIE", Text(strings::CAMERA_FOV)),
            ("SLI_MOVIE", Slider("camera.fov")),
            (
                "BTN_FOLLOW",
                Cycle("camera.tracking", CAMERA_FOLLOW, Step::Shown),
            ),
            ("BTN_FADE", Cycle("camera.body_fade", ON_OFF, Step::Shown)),
            (
                "BTN_GUN",
                OpenAs("options_camera_guns", strings::GUN_CAMERA),
            ),
            (
                "BTN_ADVANCED",
                Cycle("camera.preset", CAMERA_PRESETS, Step::Shown),
            ),
            ("BTN_DEFAULT", Defaults),
            ("BTN_BACK", Back),
        ],
        first: "SLI_MUSIC",
        help: &[
            ("SLI_MUSIC", Ours(strings::CAMERA_DISTANCE_HELP)),
            ("SLI_VO", Ours(strings::CAMERA_HEIGHT_HELP)),
            ("SLI_FX", Ours(strings::CAMERA_SHOULDER_HELP)),
            ("SLI_MOVIE", Ours(strings::CAMERA_FOV_HELP)),
            ("BTN_GUN", Ours(strings::GUN_CAMERA_HELP)),
            ("BTN_FOLLOW", Ours(strings::CAMERA_FOLLOW_HELP)),
            ("BTN_FADE", Ours(strings::BODY_FADE_HELP)),
            ("BTN_ADVANCED", Ours(strings::CAMERA_PRESET_HELP)),
        ],
    },
    // Ours, the same layout: the guns' camera, while one is readied.
    Spec {
        name: "options_camera_guns",
        layout: "optsound",
        roles: &[
            ("LBL_TITLE", Text(strings::GUN_CAMERA_OPTIONS)),
            ("LBL_MUSIC", Text(strings::CAMERA_DISTANCE)),
            ("SLI_MUSIC", Slider("camera.gun.range")),
            ("LBL_VO", Text(strings::CAMERA_HEIGHT)),
            ("SLI_VO", Slider("camera.gun.height")),
            ("LBL_FX", Text(strings::CAMERA_SHOULDER)),
            ("SLI_FX", Slider("camera.gun.shoulder")),
            ("LBL_MOVIE", Text(strings::CAMERA_FOV)),
            ("SLI_MOVIE", Slider("camera.gun.fov")),
            (
                "BTN_ADVANCED",
                Cycle("camera.gun", GUN_CAMERAS, Step::Shown),
            ),
            ("BTN_DEFAULT", Defaults),
            ("BTN_BACK", Back),
        ],
        first: "SLI_MUSIC",
        help: &[
            ("SLI_MUSIC", Ours(strings::CAMERA_DISTANCE_HELP)),
            ("SLI_VO", Ours(strings::CAMERA_HEIGHT_HELP)),
            ("SLI_FX", Ours(strings::CAMERA_SHOULDER_HELP)),
            ("SLI_MOVIE", Ours(strings::CAMERA_FOV_HELP)),
            ("BTN_ADVANCED", Ours(strings::GUN_CAMERA_HELP)),
        ],
    },
    // KOTOR 2's feedback options, a check box for each in its list. The
    // description box tells the row last pointed at, and Default puts back
    // KOTOR 2's defaults.
    Spec {
        name: feedback::PAGE,
        layout: "optfeedback",
        roles: &[
            ("LB_OPTIONS", List(&FEEDBACK)),
            ("BTN_DEFAULT", Defaults),
            ("BTN_BACK", Back),
        ],
        first: "LB_OPTIONS",
        help: &[],
    },
];

/// The Interface page's rows: the menu width, whether pictures keep their
/// shape, the text size, the text scale, the notifications in the corner,
/// the target block at the top centre and the frame rate limit. Each one is
/// off or at KOTOR 2's value while its setting is unset.
const INTERFACE: [Line; 7] = [
    stepped(
        strings::MENU_WIDTH,
        strings::MENU_WIDTH_HELP,
        MenuWidth::SETTING,
        Values::Named(MENU_WIDTHS),
    ),
    ticked_by(
        IMAGE_ASPECT,
        strings::IMAGE_ASPECT,
        strings::IMAGE_ASPECT_HELP,
    ),
    stepped(
        strings::TEXT_SIZE,
        strings::TEXT_SIZE_HELP,
        SmallFonts::SETTING,
        Values::Named(TEXT_SIZES),
    ),
    stepped(
        strings::TEXT_SCALE,
        strings::TEXT_SCALE_HELP,
        TextScale::SETTING,
        Values::Percents(&TextScale::PERCENTS, TextScale::DEFAULT),
    ),
    ticked_by(
        CORNER_NOTICES,
        strings::CORNER_NOTICES,
        strings::CORNER_NOTICES_HELP,
    ),
    ticked_by(TARGET_TOP, strings::TARGET_TOP, strings::TARGET_TOP_HELP),
    stepped(
        strings::FRAME_LIMIT,
        strings::FRAME_LIMIT_HELP,
        FRAME_LIMIT,
        Values::Rates(FRAME_LIMITS),
    ),
];

/// The sabers' page's rows: the saber rules a session played here plays by,
/// the three duel options, the hilts KOTOR's single- and double-bladed
/// lightsabers are drawn with, and whether characters use Movie Duels'
/// stances. Each one is off or at its first value while
/// its setting is unset. All seven are kjk's own: KOTOR 2 has no such
/// options. The rows with choices of Movie Duels' own are greyed or offer
/// fewer values without it installed (`JA_OWN`). The combat rules' row is
/// greyed while a session runs (`FIXED_IN_SESSION`).
const SABERS: [Line; 7] = [
    stepped(
        strings::SABER_RULES,
        strings::SABER_RULES_HELP,
        "game.saber_rules",
        Values::Named(SABER_RULES),
    ),
    ticked_by(
        "game.duel_bars",
        strings::DUEL_BARS,
        strings::DUEL_BARS_HELP,
    ),
    ticked_by(
        "game.duel_warning",
        strings::DUEL_WARNING,
        strings::DUEL_WARNING_HELP,
    ),
    ticked_by(
        "game.duel_slow_motion",
        strings::DUEL_SLOW_MOTION,
        strings::DUEL_SLOW_MOTION_HELP,
    ),
    stepped(
        strings::HILT,
        strings::HILT_HELP,
        "saber.hilt.single",
        Values::Named(HILTS),
    ),
    stepped(
        strings::STAFF,
        strings::STAFF_HELP,
        "saber.hilt.double",
        Values::Named(STAFFS),
    ),
    ticked_on_by(
        "game.stance_sets",
        strings::STANCE_SETS,
        strings::STANCE_SETS_HELP,
    ),
];

/// A stepped row of kjk's own, with its name and its help.
const fn stepped(
    name: &'static str,
    help: &'static str,
    key: &'static str,
    values: Values,
) -> Line {
    Line {
        name: Ours(name),
        help: Ours(help),
        kind: LineKind::Step(key, values),
    }
}

/// A check box row of kjk's own, with its name and its help. It is off
/// while its setting is unset (`draw::kjk_option`).
const fn ticked_by(key: &'static str, name: &'static str, help: &'static str) -> Line {
    checkbox(key, name, help, false)
}

/// The same, on while its setting is unset.
const fn ticked_on_by(key: &'static str, name: &'static str, help: &'static str) -> Line {
    checkbox(key, name, help, true)
}

const fn checkbox(
    key: &'static str,
    name: &'static str,
    help: &'static str,
    default: bool,
) -> Line {
    Line {
        name: Ours(name),
        help: Ours(help),
        kind: LineKind::Check {
            key,
            default,
            live: true,
        },
    }
}

/// The Feedback page's rows: KOTOR 2's eight check boxes in its order
/// (`feedback::CHOICES`), each with KOTOR 2's words, help and default.
const FEEDBACK: [Line; feedback::CHOICES.len()] = {
    let mut rows = [feedback_row(&feedback::CHOICES[0]); feedback::CHOICES.len()];
    let mut at = 1;
    while at < rows.len() {
        rows[at] = feedback_row(&feedback::CHOICES[at]);
        at += 1;
    }
    rows
};

const fn feedback_row(choice: &Choice) -> Line {
    Line {
        name: Told(choice.name),
        help: Told(choice.help),
        kind: LineKind::Check {
            key: choice.key,
            default: choice.default,
            live: choice.live,
        },
    }
}

/// A list page with one row of each kind, for the probes
/// (`probes::menus::option_list_rows`). No menu opens it, because no page
/// of the game has an open row or a stepped row named by KOTOR 2's talk
/// table yet. Its rows are KOTOR 2's "Tutorial Popups" check box, a
/// stepped row under KOTOR 2's "Mini Map" words, and a row that opens the
/// sound page.
#[cfg(feature = "probes")]
const SAMPLE: Spec = Spec {
    name: "options_list_sample",
    layout: "optfeedback",
    roles: &[
        (
            "LB_OPTIONS",
            List(&[
                feedback_row(&feedback::CHOICES[1]),
                Line {
                    name: Told(feedback::CHOICES[3].name),
                    help: Told(feedback::CHOICES[3].help),
                    kind: LineKind::Step(SmallFonts::SETTING, Values::Named(TEXT_SIZES)),
                },
                Line {
                    name: Ours(strings::CAMERA),
                    help: Ours(strings::CAMERA_HELP),
                    kind: LineKind::Open("options_sound"),
                },
            ]),
        ),
        ("BTN_BACK", Back),
    ],
    first: "LB_OPTIONS",
    help: &[],
};

/// The probes' list page (`SAMPLE`).
#[cfg(feature = "probes")]
pub(crate) fn sample_list(vm: &ViewModel) -> Box<dyn Screen> {
    opened(&SAMPLE, vm)
}

/// The page named `name`, its settings as `vm` has them now.
pub fn page(name: &str, vm: &ViewModel) -> Option<Box<dyn Screen>> {
    let spec = PAGES.iter().find(|page| page.name == name)?;
    Some(opened(spec, vm))
}

/// The page `spec` describes, its settings as `vm` has them now.
fn opened(spec: &'static Spec, vm: &ViewModel) -> Box<dyn Screen> {
    let mut before: Vec<(&'static str, String)> = spec
        .roles
        .iter()
        .filter_map(|(_, role)| role.key())
        .filter_map(|key| Some((key, vm.settings.get(key)?.clone())))
        .collect();
    before.dedup();
    Box::new(Page {
        spec,
        before,
        focus: None,
        row: 0,
    })
}

impl Page {
    fn role(&self, tag: &str) -> Option<Role> {
        self.spec
            .roles
            .iter()
            .find(|(control, _)| *control == tag)
            .map(|(_, role)| *role)
    }

    /// The page's list of option rows, if it has one: its tag and its rows.
    fn list(&self) -> Option<(&'static str, &'static [Line])> {
        self.spec.roles.iter().find_map(|(tag, role)| match role {
            List(lines) => Some((*tag, *lines)),
            _ => None,
        })
    }

    /// The description box's words for the focused control. On a page with
    /// a list of option rows, they are the help of the row last pointed at,
    /// whatever has the focus, as KOTOR 2 sets them only when the pointer
    /// or the cursor comes to a row. A row with choices of Movie Duels' own
    /// first says that they need it, while it is not installed. The saber
    /// rules' row first says that they are chosen before a game starts,
    /// while a session runs.
    fn help(&self, vm: &ViewModel) -> ControlState {
        if let Some(line) = self.list().and_then(|(_, lines)| lines.get(self.row)) {
            // The hilts keep KOTOR's and JA's own: only Movie Duels' need it.
            let needs = (line.kind.key())
                .filter(|key| without_movie_duels(vm, key))
                .map(|key| match key.starts_with("saber.hilt.") {
                    true => strings::MOVIE_DUELS_HILTS,
                    false => strings::NEEDS_MOVIE_DUELS,
                });
            let needs = (line.kind.key())
                .and_then(|key| session_note(vm, key))
                .or(needs);
            return line.help.state_after(needs, vm);
        }
        let focus = self.focus.as_deref();
        self.spec
            .help
            .iter()
            .find(|(tag, _)| Some(*tag) == focus)
            .map_or(ControlState::default(), |(_, help)| help.state(vm))
    }

    /// The row `index` of the list `list` activated, or stepped `back` by
    /// the Left key. A check box turns over, a stepped row goes to its next
    /// or its last value and wraps, and an open row opens its page. A
    /// greyed row does nothing, and Left does nothing but on a stepped row.
    /// A stepped row passes the values that are not offered (`offered`).
    /// Returns whether the row did something.
    fn act(
        &self,
        list: &str,
        index: usize,
        back: bool,
        vm: &ViewModel,
        out: &mut Vec<Intent>,
    ) -> bool {
        let Some(List(lines)) = self.role(list) else {
            return false;
        };
        match lines.get(index).map(|line| line.kind) {
            Some(LineKind::Check {
                key,
                default,
                live: true,
            }) if !back && !without_movie_duels(vm, key) && session_note(vm, key).is_none() => {
                let now = set_or(vm, key, default);
                out.push(Intent::Setting(key.into(), (!now).to_string()));
            }
            Some(LineKind::Step(key, values)) => {
                if session_note(vm, key).is_some() {
                    return false;
                }
                let Some(next) = values.stepped(vm, key, back) else {
                    return false;
                };
                out.push(Intent::Setting(key.into(), values.value(next).into()));
            }
            Some(LineKind::Open(screen)) if !back => out.push(Intent::Open(screen)),
            _ => return false,
        }
        true
    }

    /// The row last pointed at activated or stepped by a key (`act`). A
    /// click is heard when the row did something, as a pointer's click on
    /// the row is heard.
    fn act_heard(&self, list: &str, back: bool, vm: &ViewModel, out: &mut Vec<Intent>) -> bool {
        let mut raised = Vec::new();
        let acted = self.act(list, self.row, back, vm, &mut raised);
        if acted {
            out.push(Intent::Sound(GuiSound::Clicked));
            out.append(&mut raised);
        }
        acted
    }
}

impl Help {
    /// These words as a control's state. Words of ours name an action in
    /// braces where they mean its key (`key_mapping::with_bindings`).
    fn state(self, vm: &ViewModel) -> ControlState {
        self.state_after(None, vm)
    }

    /// The same, with `first` in a paragraph of its own before words of
    /// ours.
    fn state_after(self, first: Option<&str>, vm: &ViewModel) -> ControlState {
        let first = first.map_or(String::new(), |first| format!("{first}\n\n"));
        match self {
            Told(strref) => ControlState {
                strref: Some(strref),
                skin_strref: true,
                ..ControlState::default()
            },
            Ours(words) => ControlState {
                text: Some(strings::bytes(&format!(
                    "{first}{}",
                    key_mapping::with_bindings(words, vm)
                ))),
                ..ControlState::default()
            },
        }
    }
}

/// A setting's value as a number (0 when unset or not one).
fn number(vm: &ViewModel, key: &str) -> f32 {
    vm.settings
        .get(key)
        .and_then(|value| value.parse().ok())
        .unwrap_or(0.0)
}

fn on(vm: &ViewModel, key: &str) -> bool {
    vm.settings.get(key).is_some_and(|value| value == "true")
}

/// Whether the setting `key` is on. `true` and `false` say so. Any other
/// value, or none, gives `default`.
fn set_or(vm: &ViewModel, key: &str, default: bool) -> bool {
    match vm.settings.get(key).map(String::as_str) {
        Some("true") => true,
        Some("false") => false,
        _ => default,
    }
}

/// A list's rows as it shows them. A check box row has its words and its
/// box, ticked while its option is on. It is greyed, and shows its default,
/// while kjk does not do the option. A stepped row has its name and its
/// chosen value, and an open row its words. Neither has a box. A row that
/// does nothing without Movie Duels installed is greyed too
/// (`LineKind::needs_movie_duels`): its box is empty, or it shows the one
/// value offered.
fn list_rows(vm: &ViewModel, lines: &[Line]) -> ListView {
    let named = |name: Help, value: Option<String>| match (name, value) {
        (Told(strref), value) => Row {
            told: Some(strref),
            value: value.map(|value| strings::bytes(&value)),
            ..Row::default()
        },
        (Ours(name), Some(value)) => Row {
            text: strings::option(name, &value),
            ..Row::default()
        },
        (Ours(name), None) => Row {
            text: strings::bytes(name),
            ..Row::default()
        },
    };
    let row = |line: &Line| match line.kind {
        LineKind::Check { .. } if line.kind.needs_movie_duels(vm) => Row {
            greyed: true,
            ..named(line.name, None)
        },
        LineKind::Check { key, default, live } => Row {
            checked: if live {
                set_or(vm, key, default)
            } else {
                default
            },
            greyed: !live || line.kind.fixed(vm),
            ..named(line.name, None)
        },
        LineKind::Step(key, values) => Row {
            unmarked: true,
            greyed: line.kind.needs_movie_duels(vm) || line.kind.fixed(vm),
            ..named(line.name, Some(values.shown(vm, key)))
        },
        LineKind::Open(_) => Row {
            unmarked: true,
            ..named(line.name, None)
        },
    };
    ListView {
        rows: lines.iter().map(row).collect(),
        ..ListView::default()
    }
}

/// Where `key`'s value is among `choices` (the first when it is none of
/// them, or when Movie Duels' absence holds it back: what is played then).
/// A value a running session keeps is the session's (`kept_by_session`).
fn chosen(vm: &ViewModel, key: &str, choices: &[(&str, &str)]) -> usize {
    kept_by_session(vm, key)
        .or_else(|| {
            vm.settings
                .get(key)
                .map(String::as_str)
                .filter(|value| movie_duels_offers(vm, key, value))
        })
        .and_then(|value| choices.iter().position(|(choice, _)| *choice == value))
        .unwrap_or(0)
}

impl Screen for Page {
    /// A page with Cancel (the advanced graphics) puts back what changed.
    fn cancels(&self) -> bool {
        self.spec
            .roles
            .iter()
            .any(|(_, role)| matches!(role, Cancel))
    }

    /// Its OK or Close, which keeps what changed (Menu; B may cancel).
    fn accept(&self) -> Option<&str> {
        self.spec
            .roles
            .iter()
            .find(|(_, role)| matches!(role, Back))
            .map(|(tag, _)| *tag)
    }

    fn id(&self) -> ScreenId {
        ScreenId(self.spec.name)
    }

    fn layout(&self) -> &str {
        self.spec.layout
    }

    fn focused(&mut self, tag: &str, _vm: &ViewModel) {
        self.focus = Some(tag.to_string());
    }

    fn shape(&self, gui: &mut fmt_odyssey::gui::Gui) {
        for (_, reshape) in RESHAPED.iter().filter(|(page, _)| *page == self.spec.name) {
            match reshape {
                Reshape::Stack(at, tags) => {
                    crate::screens::stack_under(gui, *at, tags, RESHAPE_GAP)
                }
                Reshape::Under(tag, after) => {
                    crate::screens::move_under(gui, (tag, after), RESHAPE_GAP)
                }
                Reshape::Like(tag, like) => crate::screens::look_like(gui, (tag, like)),
            }
        }
    }

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        if control.tag == "LB_DESC" {
            return self.help(vm);
        }
        match self.role(&control.tag) {
            Some(Toggle(key)) => ControlState {
                selected: on(vm, key),
                ..ControlState::default()
            },
            Some(ToggleAs(key, label)) => ControlState {
                selected: on(vm, key),
                text: Some(strings::bytes(label)),
                ..ControlState::default()
            },
            Some(Stepped(key, choices)) => {
                let at = chosen(vm, key, choices);
                ControlState {
                    selected: at != 0,
                    text: Some(strings::bytes(choices[at].1)),
                    ..ControlState::default()
                }
            }
            Some(Slider(key)) => ControlState {
                value: Some(number(vm, key)),
                ..ControlState::default()
            },
            Some(OpenAs(_, label) | Text(label) | Sandbox(label)) => ControlState {
                text: Some(strings::bytes(label)),
                ..ControlState::default()
            },
            Some(Cycle(key, choices, Step::Shown)) => ControlState {
                text: Some(strings::bytes(choices[chosen(vm, key, choices)].1)),
                ..ControlState::default()
            },
            Some(List(lines)) => ControlState {
                list: Some(list_rows(vm, lines)),
                ..ControlState::default()
            },
            Some(Greyed) => ControlState {
                disabled: true,
                ..ControlState::default()
            },
            Some(Hidden) => ControlState {
                hidden: true,
                ..ControlState::default()
            },
            _ => ControlState::default(),
        }
    }

    fn press(&mut self, tag: &str, vm: &ViewModel, out: &mut Vec<Intent>) {
        match self.role(tag) {
            Some(Toggle(key) | ToggleAs(key, _)) => {
                out.push(Intent::Setting(key.into(), (!on(vm, key)).to_string()));
            }
            Some(Stepped(key, choices)) => {
                let next = (chosen(vm, key, choices) + 1) % choices.len();
                out.push(Intent::Setting(key.into(), choices[next].0.into()));
            }
            Some(Cycle(key, choices, step)) => {
                let by = if step == Step::Back {
                    choices.len() - 1
                } else {
                    1
                };
                let mut next = chosen(vm, key, choices);
                for _ in 0..choices.len() {
                    next = (next + by) % choices.len();
                    if offered(vm, key, choices[next].0) {
                        break;
                    }
                }
                out.push(Intent::Setting(key.into(), choices[next].0.into()));
            }
            Some(Open(screen) | OpenAs(screen, _)) => out.push(Intent::Open(screen)),
            Some(Sandbox(_)) => out.extend([
                Intent::CloseAll,
                Intent::Session(crate::intent::SessionIntent::Sandbox),
            ]),
            Some(Back) => out.push(Intent::Close),
            Some(Cancel) => self.back(out),
            Some(Defaults) => out.push(Intent::Setting("reset".into(), self.spec.name.into())),
            _ => {}
        }
    }

    /// A list's row activated (a click, Enter or the pad's A). A check box
    /// row turns its option on or off, a stepped row steps on, and an open
    /// row opens its page. A greyed row does nothing.
    fn row(&mut self, list: &str, index: usize, vm: &ViewModel, out: &mut Vec<Intent>) {
        self.act(list, index, false, vm, out);
    }

    /// On a page with a list of option rows, the keyboard's X key activates
    /// the row last pointed at, wherever the focus is, as KOTOR 2's
    /// Feedback page turns a check box on or off with it.
    fn raw(&mut self, key: &str, vm: &ViewModel, out: &mut Vec<Intent>) -> bool {
        let Some((list, _)) = self.list().filter(|_| key == "KeyX") else {
            return false;
        };
        self.act_heard(list, false, vm, out);
        true
    }

    /// Left and Right, on the keyboard and the pad, step the list's cursor
    /// row back and on while the list has the focus and the row is a
    /// stepped one. On any other row the keys move the focus as on every
    /// page.
    fn key(&mut self, key: Key, vm: &ViewModel, out: &mut Vec<Intent>) -> bool {
        let back = match key {
            Key::Left => true,
            Key::Right => false,
            _ => return false,
        };
        let Some((list, lines)) = self
            .list()
            .filter(|(tag, _)| self.focus.as_deref() == Some(*tag))
        else {
            return false;
        };
        let stepped = lines
            .get(self.row)
            .is_some_and(|line| matches!(line.kind, LineKind::Step(..)));
        stepped && self.act_heard(list, back, vm, out)
    }

    /// On a page with a list of option rows, the right stick scrolls the
    /// help, as on KOTOR 2's Feedback page, even when the list itself
    /// overflows.
    fn scrolls(&self) -> Option<&str> {
        self.list().map(|_| "LB_DESC")
    }

    /// The pointer or the cursor came to a row: the description box tells
    /// it from now on.
    fn pointed(&mut self, list: &str, index: usize, _vm: &ViewModel) {
        if self.list().is_some_and(|(tag, _)| tag == list) {
            self.row = index;
        }
    }

    /// KOTOR 2 makes a list's row as tall as the layout's row times its
    /// text size: 1.5 times at 2560 pixels wide. kjk's text scale is part
    /// of that text size (`draw::Sizing`).
    fn fit_read(
        &mut self,
        gui: &mut fmt_odyssey::gui::Gui,
        textures: &mut dyn Textures,
        _: Source,
    ) {
        let Some((tag, _)) = self.list() else {
            return;
        };
        // The sizing's text counts layout pixels to a font pixel, and its
        // pixel counts layout pixels to a physical pixel. So the text size
        // is the one divided by the other.
        let sizing = textures.sizing();
        let size = sizing.text / sizing.pixel.max(f32::EPSILON);
        if let Some(proto) = gui
            .controls
            .iter_mut()
            .find(|control| control.tag == tag)
            .and_then(|list| list.proto.as_deref_mut())
        {
            proto.extent.height = (proto.extent.height as f32 * size) as i32;
        }
    }

    fn slide(&mut self, tag: &str, value: f32, out: &mut Vec<Intent>) {
        if let Some(Slider(key)) = self.role(tag) {
            out.push(Intent::Setting(key.into(), format!("{value:.2}")));
        }
    }

    /// Escape: a page with Cancel cancels, the others keep what changed.
    fn back(&mut self, out: &mut Vec<Intent>) {
        if self.cancels() {
            for (key, value) in &self.before {
                out.push(Intent::Setting((*key).into(), value.clone()));
            }
        }
        out.push(Intent::Close);
    }

    fn first_focus(&self) -> Option<&str> {
        Some(self.spec.first)
    }
}
