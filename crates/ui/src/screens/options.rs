//! The options pages (`optionsmain`, `optgameplay`, `optmouse`,
//! `optgraphics`, `optgraphicsadv`, `optsound`, `optfeedback`), each a
//! table of what its controls do: a check box, slider or stepped button
//! bound to a setting, a button that opens another page, or greyed where
//! the game has nothing behind it yet (texture quality, frame buffer
//! effects, the feedback list, saves and levels for the world state). KOTOR 2's
//! pages (the skin) have two more check boxes, hidden.

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

struct Spec {
    name: &'static str,
    layout: &'static str,
    roles: &'static [(&'static str, Role)],
    first: &'static str,
}

pub struct Page {
    spec: &'static Spec,
    /// The page's settings when it opened (`Cancel`).
    before: Vec<(&'static str, String)>,
}

use Role::{Back, Cancel, Cycle, Defaults, Greyed, Hidden, Open, Slider, Toggle, ToggleAs};

/// Anti-aliasing: the samples a pixel takes.
const SAMPLES: &[(&str, &str)] = &[
    ("1", strings::ANTI_ALIASING_OFF),
    ("2", strings::ANTI_ALIASING_2X),
    ("4", strings::ANTI_ALIASING_4X),
];

const PAGES: [Spec; 7] = [
    Spec {
        name: "options",
        layout: "optionsmain",
        roles: &[
            ("BTN_GAMEPLAY", Open("options_gameplay")),
            ("BTN_FEEDBACK", Open("options_feedback")),
            ("BTN_GRAPHICS", Open("options_graphics")),
            ("BTN_SOUND", Open("options_sound")),
            ("BTN_AUTOPAUSE", Hidden),
            ("BTN_BACK", Back),
        ],
        first: "BTN_GAMEPLAY",
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
            ("CB_AUTOSAVE", Greyed),
            ("CB_REVERSE", Greyed),
            ("CB_DISABLEMOVE", Greyed),
            ("CB_REVERSE_INGAME", Hidden),
            ("BTN_DIFFICULTY", Greyed),
            ("BTN_DIFFLEFT", Greyed),
            ("BTN_DIFFRIGHT", Greyed),
            ("BTN_KEYMAP", Open("options_keymap")),
            ("BTN_MOUSE", Open("options_mouse")),
            ("BTN_DEFAULT", Defaults),
            ("BTN_BACK", Back),
        ],
        first: "CB_INVERTCAM",
    },
    Spec {
        name: "options_mouse",
        layout: "optmouse",
        roles: &[
            ("SLI_MOUSESEN", Slider("mouse.sensitivity")),
            ("CB_REVBUTTONS", Toggle("mouse.reverse")),
            ("BTN_DEFAULT", Defaults),
            ("BTN_BACK", Back),
        ],
        first: "SLI_MOUSESEN",
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
    Some(Box::new(Page { spec, before }))
}

impl Page {
    fn role(&self, tag: &str) -> Option<Role> {
        self.spec
            .roles
            .iter()
            .find(|(control, _)| *control == tag)
            .map(|(_, role)| *role)
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
/// them).
fn chosen(vm: &ViewModel, key: &str, choices: &[(&str, &str)]) -> usize {
    vm.settings
        .get(key)
        .and_then(|value| choices.iter().position(|(choice, _)| choice == value))
        .unwrap_or(0)
}

impl Screen for Page {
    fn id(&self) -> ScreenId {
        ScreenId(self.spec.name)
    }

    fn layout(&self) -> &str {
        self.spec.layout
    }

    fn state(&self, control: &Control, vm: &ViewModel) -> ControlState {
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
            Some(Open(screen)) => out.push(Intent::Open(screen)),
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
        if self
            .spec
            .roles
            .iter()
            .any(|(_, role)| matches!(role, Cancel))
        {
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
