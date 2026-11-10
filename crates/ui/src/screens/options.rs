//! The options pages (`optionsmain`, `optgameplay`, `optmouse`,
//! `optgraphics`, `optgraphicsadv`, `optsound`, `optfeedback`), each a
//! table of what its controls do: a check box, slider or stepped button
//! bound to a setting, a button that opens another page, or greyed where
//! the game has nothing behind it yet (texture quality, frame buffer
//! effects, the feedback list, saves and levels for the world state). KOTOR 2's
//! pages (the skin) have two more check boxes, hidden. The controller's
//! page is ours, on the graphics page's layout, opened from the gameplay
//! page in its unused difficulty button's place. The sabers' page is ours
//! too, on the gameplay page's own layout, opened from that page's unused
//! "reverse" box: its difficulty, stepped by its arrows, picks the saber
//! rules a session played here plays by (`docs/SABER-RULES.md`), its check
//! boxes the duel modes' options and Movie Duels' stances, its mouse and
//! key mapping buttons the hilts KOTOR's lightsabers are drawn with. The main
//! page's unused auto-pause button is Debug: the combat sandbox (only the
//! main menu opens this page; play has its own, `options_in_game`).

use fmt_odyssey::gui::Control;

use crate::intent::Intent;
use crate::paint::ControlState;
use crate::screen::{Screen, ScreenId};
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
}

use Help::{Ours, Told};
use Role::{
    Back, Cancel, Cycle, Defaults, Greyed, Hidden, Open, OpenAs, Sandbox, Slider, Text, Toggle,
    ToggleAs,
};

/// The saber rules (`sim::combat::saber::SaberRules::name`).
const SABER_RULES: &[(&str, &str)] = &[
    ("ja", strings::SABER_RULES_JA),
    ("md", strings::SABER_RULES_MD),
    ("mda", strings::SABER_RULES_MDA),
];

/// The hilts a single-bladed KOTOR lightsaber may be drawn with (a `.sab`
/// name, `saber.hilt.single`): KOTOR's own, Jedi Academy's, and a few of
/// Movie Duels' (`kjk sabers` lists them all; one the install lacks leaves
/// KOTOR's, the run's notes say).
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

/// The same for a double-bladed one (`saber.hilt.double`).
const STAFFS: &[(&str, &str)] = &[
    ("", strings::STAFF_KOTOR),
    ("dual_1", strings::STAFF_JA),
    ("bastila_staff", strings::STAFF_BASTILA),
    ("dual_maul", strings::STAFF_MAUL),
    ("inquisitor", strings::STAFF_INQUISITOR),
    ("cal_staff", strings::STAFF_CAL),
];

/// The settings with choices of Movie Duels' own (its saber rules and
/// hilts, its stances), and the choices offered without it installed
/// beside Jedi Academy (`ViewModel::movie_duels`): kjk's rules need none of
/// its files, but they are its creators' design.
const JA_OWN: &[(&str, &[&str])] = &[
    ("game.saber_rules", &["ja"]),
    ("game.duel_stances", &[]),
    ("saber.hilt.single", &["", "single_1"]),
    ("saber.hilt.double", &["", "dual_1"]),
];

/// Whether `key` has choices of Movie Duels' own (`JA_OWN`) and it is not
/// installed.
fn without_movie_duels(vm: &ViewModel, key: &str) -> bool {
    !vm.movie_duels && JA_OWN.iter().any(|(own, _)| *own == key)
}

/// Whether `key`'s `value` is offered: every one with Movie Duels
/// installed, else those `JA_OWN` lists.
fn offered(vm: &ViewModel, key: &str, value: &str) -> bool {
    vm.movie_duels
        || JA_OWN
            .iter()
            .find(|(own, _)| *own == key)
            .is_none_or(|(_, values)| values.contains(&value))
}

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

const PAGES: [Spec; 9] = [
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
            ("CB_REVERSE", OpenAs("options_sabers", strings::SABERS)),
            ("CB_DISABLEMOVE", Hidden),
            ("CB_REVERSE_INGAME", Hidden),
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
        name: "options_sabers",
        layout: "optgameplay",
        roles: &[
            ("LBL_TITLE", Text(strings::SABER_OPTIONS)),
            (
                "BTN_DIFFICULTY",
                Cycle("game.saber_rules", SABER_RULES, Step::Shown),
            ),
            (
                "BTN_DIFFLEFT",
                Cycle("game.saber_rules", SABER_RULES, Step::Back),
            ),
            (
                "BTN_DIFFRIGHT",
                Cycle("game.saber_rules", SABER_RULES, Step::On),
            ),
            ("CB_LEVELUP", ToggleAs("game.duel_bars", strings::DUEL_BARS)),
            (
                "CB_INVERTCAM",
                ToggleAs("game.duel_warning", strings::DUEL_WARNING),
            ),
            (
                "CB_AUTOSAVE",
                ToggleAs("game.duel_slow_motion", strings::DUEL_SLOW_MOTION),
            ),
            (
                "CB_REVERSE",
                ToggleAs("game.duel_stances", strings::DUEL_STANCES),
            ),
            ("CB_DISABLEMOVE", Hidden),
            ("CB_REVERSE_INGAME", Hidden),
            ("BTN_MOUSE", Cycle("saber.hilt.single", HILTS, Step::Shown)),
            (
                "BTN_KEYMAP",
                Cycle("saber.hilt.double", STAFFS, Step::Shown),
            ),
            ("BTN_DEFAULT", Defaults),
            ("BTN_BACK", Back),
        ],
        first: "BTN_DIFFICULTY",
        help: &[
            ("BTN_DIFFICULTY", Ours(strings::SABER_RULES_HELP)),
            ("BTN_DIFFLEFT", Ours(strings::SABER_RULES_HELP)),
            ("BTN_DIFFRIGHT", Ours(strings::SABER_RULES_HELP)),
            ("CB_LEVELUP", Ours(strings::DUEL_BARS_HELP)),
            ("CB_INVERTCAM", Ours(strings::DUEL_WARNING_HELP)),
            ("CB_AUTOSAVE", Ours(strings::DUEL_SLOW_MOTION_HELP)),
            ("CB_REVERSE", Ours(strings::DUEL_STANCES_HELP)),
            ("BTN_MOUSE", Ours(strings::HILT_HELP)),
            ("BTN_KEYMAP", Ours(strings::STAFF_HELP)),
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
            ("CB_FORCESPEED", Hidden),
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
    Spec {
        name: "options_feedback",
        layout: "optfeedback",
        roles: &[
            ("LB_OPTIONS", Greyed),
            ("BTN_DEFAULT", Greyed),
            ("BTN_BACK", Back),
        ],
        first: "BTN_BACK",
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

    /// The description box's words for the focused control; one with
    /// Movie Duels' choices says they need it, while it is not installed.
    fn help(&self, vm: &ViewModel) -> ControlState {
        let focus = self.focus.as_deref();
        // The hilts keep KOTOR's and JA's own: only Movie Duels' need it.
        let needs = focus
            .and_then(|tag| self.role(tag))
            .and_then(Role::key)
            .filter(|key| without_movie_duels(vm, key))
            .map(|key| match key.starts_with("saber.hilt.") {
                true => strings::MOVIE_DUELS_HILTS,
                false => strings::NEEDS_MOVIE_DUELS,
            });
        match self.spec.help.iter().find(|(tag, _)| Some(*tag) == focus) {
            Some((_, Told(strref))) => ControlState {
                strref: Some(*strref),
                skin_strref: true,
                ..ControlState::default()
            },
            Some((_, Ours(words))) if let Some(needs) = needs => ControlState {
                text: Some(strings::bytes(&format!("{needs}\n\n{words}"))),
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

/// Where `key`'s value is among `choices` (the first when it is none of
/// them, or not offered: what is played then).
fn chosen(vm: &ViewModel, key: &str, choices: &[(&str, &str)]) -> usize {
    vm.settings
        .get(key)
        .filter(|value| offered(vm, key, value))
        .and_then(|value| choices.iter().position(|(choice, _)| choice == value))
        .unwrap_or(0)
}

/// How many of `key`'s `choices` are offered.
fn offered_count(vm: &ViewModel, key: &str, choices: &[(&str, &str)]) -> usize {
    choices
        .iter()
        .filter(|(value, _)| offered(vm, key, value))
        .count()
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

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
        if control.tag == "LB_DESC" {
            return self.help(vm);
        }
        match self.role(&control.tag) {
            // Movie Duels' stances, greyed without it (still focused, its
            // description saying why; a press does nothing).
            Some(ToggleAs(key, label)) if without_movie_duels(vm, key) => ControlState {
                text: Some(strings::bytes(label)),
                greyed: true,
                ..ControlState::default()
            },
            // A cycle with one choice offered (the saber rules without
            // Movie Duels: JA's), the same.
            Some(Cycle(key, choices, step)) if offered_count(vm, key, choices) < 2 => ControlState {
                text: (step == Step::Shown)
                    .then(|| strings::bytes(choices[chosen(vm, key, choices)].1)),
                greyed: true,
                ..ControlState::default()
            },
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
            Some(Toggle(key) | ToggleAs(key, _)) if !without_movie_duels(vm, key) => {
                out.push(Intent::Setting(key.into(), (!on(vm, key)).to_string()));
            }
            Some(Cycle(key, choices, step)) if offered_count(vm, key, choices) > 1 => {
                let by = if step == Step::Back {
                    choices.len() - 1
                } else {
                    1
                };
                // On to the next offered choice (JA's own without Movie
                // Duels).
                let mut next = chosen(vm, key, choices);
                loop {
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
