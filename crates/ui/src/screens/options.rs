//! The options pages (`optionsmain`, `optgameplay`, `optmouse`,
//! `optgraphics`, `optgraphicsadv`, `optsound`, `optfeedback`), each a
//! table of what its controls do: a check box, slider or stepped button
//! bound to a setting, a button that opens another page, or greyed where
//! the game has nothing behind it yet (texture quality, frame buffer
//! effects, saves and levels for the world state). The feedback page is a
//! list of check boxes, one for each of KOTOR 2's feedback options
//! (`feedback`). KOTOR 2's
//! pages (the skin) have two more check boxes, hidden. The controller's
//! page is ours, on the graphics page's layout, opened from the gameplay
//! page in its unused difficulty button's place. The Interface page is
//! ours too, on the advanced graphics page's layout, opened from the
//! graphics page in its unused force speed box's place. The main page's unused
//! auto-pause button is Debug: the combat sandbox (only the main menu opens
//! this page; play has its own, `options_in_game`).

use fmt_odyssey::gui::Control;

use crate::draw::{CORNER_NOTICES, IMAGE_ASPECT, MenuWidth, SmallFonts, TARGET_TOP, Textures};
use crate::feedback::{self, Choice};
use crate::intent::Intent;
use crate::paint::{ControlState, ListView, Row};
use crate::screen::{Screen, ScreenId};
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
    /// A list of check boxes, one row for each option (KOTOR 2's feedback
    /// options). Activating a row turns its option on or off. The
    /// description box tells the row last pointed at.
    Checks(&'static [Choice]),
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
            Toggle(key) | ToggleAs(key, _) | Slider(key) | Cycle(key, ..) => Some(key),
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
    /// The row of the check box list last pointed at (`Checks`). It starts
    /// at the first row, as KOTOR 2 opens the page with it.
    row: usize,
}

use Help::{Ours, Told};
use Role::{
    Back, Cancel, Checks, Cycle, Defaults, Greyed, Hidden, Open, OpenAs, Sandbox, Slider, Text,
    Toggle, ToggleAs,
};

/// Anti-aliasing: the samples a pixel takes.
const SAMPLES: &[(&str, &str)] = &[
    ("1", strings::ANTI_ALIASING_OFF),
    ("2", strings::ANTI_ALIASING_2X),
    ("4", strings::ANTI_ALIASING_4X),
];

/// How far a pad's stick is pushed before it counts (a share of its
/// reach; worn sticks drift further).
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

/// How a page changes its layout.
enum Reshape {
    /// Copies of a button (`style`) stacked under a control (`after`).
    Stack((&'static str, &'static str), &'static [&'static str]),
    /// A control moved under another.
    Under(&'static str, &'static str),
    /// A control drawn as another is.
    Like(&'static str, &'static str),
    /// A control moved into another's row, level with it.
    Level(&'static str, &'static str),
}

/// Each page's changes to its layout: the camera page's three buttons under
/// its sliders, inside their box; the guns' page's preset there too, so it
/// reads as the guns' own; the gameplay page's way to the camera page and
/// the graphics page's way to the Interface page framed buttons, as Mouse
/// Settings and Advanced Options are, not check boxes. The Interface
/// page's check box sits in the hidden anti-aliasing row, so that it
/// follows the menu width it belongs with.
///
/// On the Interface page, the two check boxes under it then move up a row
/// each, so that no empty row is left where the aspect ratio box was. The
/// order matters: each box takes the row of the one above it before that
/// one moves.
const RESHAPED: [(&str, Reshape); 7] = [
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
    (
        "options_graphics",
        Reshape::Like("CB_FORCESPEED", "BTN_ADVANCED"),
    ),
    (
        "options_interface",
        Reshape::Level("CB_VSYNC", "CB_SOFTSHADOWS"),
    ),
    (
        "options_interface",
        Reshape::Level("CB_SOFTSHADOWS", "CB_FRAMEBUFF"),
    ),
    (
        "options_interface",
        Reshape::Level("CB_FRAMEBUFF", "BTN_ANTIALIAS"),
    ),
];
/// The gap between a moved or added control and the one above it (pixels).
const RESHAPE_GAP: i32 = 5;

const PAGES: [Spec; 11] = [
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
            ("CB_REVERSE", Hidden),
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
            ("CB_SHADOWS", Toggle("display.shadows")),
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
            ("CB_SHADOWS", Told(48708)),
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
            ("CB_VSYNC", Toggle("display.vsync")),
            ("CB_FRAMEBUFF", Greyed),
            ("BTN_TEXQUAL", Greyed),
            ("BTN_TEXQUALLEFT", Greyed),
            ("BTN_TEXQUALRIGHT", Greyed),
            ("BTN_ANISOTROPY", Greyed),
            ("BTN_ANISOTROPYLEFT", Greyed),
            ("BTN_ANISOTROPYRIGHT", Greyed),
            ("CB_SOFTSHADOWS", Toggle("display.soft_shadows")),
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
            ("CB_FRAMEBUFF", Told(48714)),
            ("BTN_ANISOTROPY", Told(48715)),
            ("CB_SOFTSHADOWS", Told(48711)),
            ("CB_VSYNC", Told(48716)),
        ],
    },
    // Ours, on the advanced graphics page's layout: how the menus are
    // drawn, and kjk's own changes to KOTOR 2's interface. Its rows are the
    // menu width, whether pictures keep their shape, the text size, the
    // notifications in the corner (the V-Sync box) and the target block at
    // the top centre (the soft shadows box). Each one starts as KOTOR 2
    // does it.
    Spec {
        name: "options_interface",
        layout: "optgraphicsadv",
        roles: &[
            ("LBL_TITLE", Text(strings::INTERFACE)),
            (
                "BTN_TEXQUAL",
                Cycle(MenuWidth::SETTING, MENU_WIDTHS, Step::Shown),
            ),
            (
                "BTN_TEXQUALLEFT",
                Cycle(MenuWidth::SETTING, MENU_WIDTHS, Step::Back),
            ),
            (
                "BTN_TEXQUALRIGHT",
                Cycle(MenuWidth::SETTING, MENU_WIDTHS, Step::On),
            ),
            ("BTN_ANTIALIAS", Hidden),
            ("BTN_ANTIALIASLEFT", Hidden),
            ("BTN_ANTIALIASRIGHT", Hidden),
            (
                "CB_FRAMEBUFF",
                ToggleAs(IMAGE_ASPECT, strings::IMAGE_ASPECT),
            ),
            (
                "BTN_ANISOTROPY",
                Cycle(SmallFonts::SETTING, TEXT_SIZES, Step::Shown),
            ),
            (
                "BTN_ANISOTROPYLEFT",
                Cycle(SmallFonts::SETTING, TEXT_SIZES, Step::Back),
            ),
            (
                "BTN_ANISOTROPYRIGHT",
                Cycle(SmallFonts::SETTING, TEXT_SIZES, Step::On),
            ),
            (
                "CB_VSYNC",
                ToggleAs(CORNER_NOTICES, strings::CORNER_NOTICES),
            ),
            ("CB_SOFTSHADOWS", ToggleAs(TARGET_TOP, strings::TARGET_TOP)),
            ("BTN_DEFAULT", Defaults),
            ("BTN_CANCEL", Cancel),
            ("BTN_BACK", Back),
        ],
        first: "BTN_TEXQUAL",
        help: &[
            ("BTN_TEXQUAL", Ours(strings::MENU_WIDTH_HELP)),
            ("BTN_TEXQUALLEFT", Ours(strings::MENU_WIDTH_HELP)),
            ("BTN_TEXQUALRIGHT", Ours(strings::MENU_WIDTH_HELP)),
            ("CB_FRAMEBUFF", Ours(strings::IMAGE_ASPECT_HELP)),
            ("BTN_ANISOTROPY", Ours(strings::TEXT_SIZE_HELP)),
            ("BTN_ANISOTROPYLEFT", Ours(strings::TEXT_SIZE_HELP)),
            ("BTN_ANISOTROPYRIGHT", Ours(strings::TEXT_SIZE_HELP)),
            ("CB_VSYNC", Ours(strings::CORNER_NOTICES_HELP)),
            ("CB_SOFTSHADOWS", Ours(strings::TARGET_TOP_HELP)),
        ],
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
            ("LB_OPTIONS", Checks(&feedback::CHOICES)),
            ("BTN_DEFAULT", Defaults),
            ("BTN_BACK", Back),
        ],
        first: "LB_OPTIONS",
        help: &[],
    },
];

/// The page named `name`, its settings as `vm` has them now.
pub fn page(name: &str, vm: &ViewModel) -> Option<Box<dyn Screen>> {
    let spec = PAGES.iter().find(|page| page.name == name)?;
    let mut before: Vec<(&'static str, String)> = spec
        .roles
        .iter()
        .filter_map(|(_, role)| role.key())
        .filter_map(|key| Some((key, vm.settings.get(key)?.clone())))
        .collect();
    before.dedup();
    Some(Box::new(Page {
        spec,
        before,
        focus: None,
        row: 0,
    }))
}

impl Page {
    fn role(&self, tag: &str) -> Option<Role> {
        self.spec
            .roles
            .iter()
            .find(|(control, _)| *control == tag)
            .map(|(_, role)| *role)
    }

    /// The page's check box list, if it has one: its tag and its options.
    fn checks(&self) -> Option<(&'static str, &'static [Choice])> {
        self.spec.roles.iter().find_map(|(tag, role)| match role {
            Checks(choices) => Some((*tag, *choices)),
            _ => None,
        })
    }

    /// The description box's words for the focused control. On a page with
    /// a check box list, they are the help of the row last pointed at,
    /// whatever has the focus, as KOTOR 2 sets them only when the pointer
    /// or the cursor comes to a row.
    fn help(&self) -> ControlState {
        if let Some(choice) = self.checks().and_then(|(_, choices)| choices.get(self.row)) {
            return ControlState {
                strref: Some(choice.help),
                skin_strref: true,
                ..ControlState::default()
            };
        }
        let focus = self.focus.as_deref();
        match self.spec.help.iter().find(|(tag, _)| Some(*tag) == focus) {
            Some((_, Told(strref))) => ControlState {
                strref: Some(*strref),
                skin_strref: true,
                ..ControlState::default()
            },
            Some((_, Ours(words))) => ControlState {
                text: Some(strings::bytes(words)),
                ..ControlState::default()
            },
            None => ControlState::default(),
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

/// Whether a check box list's option is ticked. An option that kjk does
/// not do yet shows KOTOR 2's default.
fn ticked(vm: &ViewModel, choice: &Choice) -> bool {
    if choice.live {
        feedback::on(&vm.settings, choice.key)
    } else {
        choice.default
    }
}

/// A check box list's rows: each option's words from KOTOR 2's talk table,
/// its box ticked while it is on, and greyed while kjk does not do it.
fn check_rows(vm: &ViewModel, choices: &[Choice]) -> ListView {
    ListView {
        rows: choices
            .iter()
            .map(|choice| Row {
                told: Some(choice.name),
                checked: ticked(vm, choice),
                greyed: !choice.live,
                ..Row::default()
            })
            .collect(),
        ..ListView::default()
    }
}

/// Where `key`'s value is among `choices` (the first when it is none of
/// them).
fn chosen(vm: &ViewModel, key: &str, choices: &[(&str, &str)]) -> usize {
    vm.settings
        .get(key)
        .and_then(|value| choices.iter().position(|(choice, _)| choice == value))
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
                Reshape::Level(tag, with) => crate::screens::level_with(gui, (tag, with)),
            }
        }
    }

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        if control.tag == "LB_DESC" {
            return self.help();
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
            Some(Checks(choices)) => ControlState {
                list: Some(check_rows(vm, choices)),
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
            Some(Cycle(key, choices, step)) => {
                let by = if step == Step::Back {
                    choices.len() - 1
                } else {
                    1
                };
                let next = (chosen(vm, key, choices) + by) % choices.len();
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

    /// A check box row activated (a click, Enter or the pad's A) turns its
    /// option on or off. A greyed row does nothing.
    fn row(&mut self, list: &str, index: usize, vm: &ViewModel, out: &mut Vec<Intent>) {
        let Some(Checks(choices)) = self.role(list) else {
            return;
        };
        if let Some(choice) = choices.get(index).filter(|choice| choice.live) {
            let now = feedback::on(&vm.settings, choice.key);
            out.push(Intent::Setting(choice.key.into(), (!now).to_string()));
        }
    }

    /// On a page with a check box list, the keyboard's X key turns the row
    /// last pointed at on or off, wherever the focus is, as KOTOR 2's
    /// Feedback page does.
    fn raw(&mut self, key: &str, vm: &ViewModel, out: &mut Vec<Intent>) -> bool {
        let Some((list, choices)) = self.checks().filter(|_| key == "KeyX") else {
            return false;
        };
        if choices.get(self.row).is_some_and(|choice| choice.live) {
            out.push(Intent::Sound(crate::intent::GuiSound::Clicked));
        }
        self.row(list, self.row, vm, out);
        true
    }

    /// On a page with a check box list, the right stick scrolls the help,
    /// as on KOTOR 2's Feedback page, even when the list itself overflows.
    fn scrolls(&self) -> Option<&str> {
        self.checks().map(|_| "LB_DESC")
    }

    /// The pointer or the cursor came to a row: the description box tells
    /// it from now on.
    fn pointed(&mut self, list: &str, index: usize, _vm: &ViewModel) {
        if self.checks().is_some_and(|(tag, _)| tag == list) {
            self.row = index;
        }
    }

    /// KOTOR 2 makes a check box row as tall as the layout's row times its
    /// text size (`draw::text_size`): 1.5 times at 2560 pixels wide.
    fn fit_read(
        &mut self,
        gui: &mut fmt_odyssey::gui::Gui,
        textures: &mut dyn Textures,
        _: Source,
    ) {
        let Some((tag, _)) = self.checks() else {
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
