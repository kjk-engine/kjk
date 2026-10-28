//! The player's settings, kept in a file of their own in the user's
//! config folder (never the game's; `KJK_SETTINGS` names another): the mouse's feel, the keys, the
//! sound's volumes and the window (`display`). The
//! options screens change them (`Intent::Setting`); each change is saved
//! at once and shown back to the screens. The file also keeps the
//! install's host id (`Settings::host_id`), made once.

use std::collections::BTreeMap;
use std::path::PathBuf;

use bevy::prelude::*;
use ui::intent::Intent;
use ui::vm::KeyRow;

use crate::display::{Display, DisplayMode};
use crate::sound::Volumes;
use crate::ui_view::{Ui, UiIntents};
use audio::Bus;

/// What a key does in a game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Action {
    Forward,
    Backward,
    Left,
    Right,
    Jump,
    Crouch,
    Walk,
    Use,
    Security,
    SaberStyle,
    Power,
    Grip,
    Lightning,
    Drain,
    Grenade,
    Medpac,
    Shield,
    Stim,
    Stealth,
    LayMine,
    /// Put the weapon in hand away (`sim::action::Action::Holster`).
    Holster,
    /// Exchange the weapon sets (`sim::action::Action::SwitchWeapons`).
    SwitchWeapons,
    /// TSL's next Form the character knows (`form_view`).
    Form,
    RightHand,
    LeftHand,
    NextPower,
    PreviousPower,
    Inventory,
    Journal,
    Map,
    Party,
    Abilities,
    QuickSave,
    QuickLoad,
    /// The next party member leads (KOTOR's `ChangeChar`, Tab).
    Leader,
    /// While the seat watches (I30 P3): follow the next party member.
    Follow,
    Break,
    Ready,
    /// Turn between the own eyes and JA's camera (`play_view::CameraMode`).
    View,
    /// Leave the conversation while it is unlocked (R6, `dialogue_view`).
    LeaveTalk,
    /// The host lets the first guest asking in join, or ends that
    /// conversation (R5, `dialogue_view`).
    AcceptCall,
    EndCall,
}

/// Each action: its settings name, its label, its default key, and
/// whether it is a movement key.
const ACTIONS: [(Action, &str, &str, KeyCode, bool); 42] = [
    (Action::Forward, "forward", "Forward", KeyCode::KeyW, true),
    (
        Action::Backward,
        "backward",
        "Backward",
        KeyCode::KeyS,
        true,
    ),
    (Action::Left, "left", "Strafe left", KeyCode::KeyA, true),
    (Action::Right, "right", "Strafe right", KeyCode::KeyD, true),
    (Action::Jump, "jump", "Jump", KeyCode::Space, true),
    (Action::Crouch, "crouch", "Crouch", KeyCode::KeyC, true),
    (Action::Walk, "walk", "Walk", KeyCode::ShiftLeft, true),
    (Action::Use, "use", "Use", KeyCode::KeyE, false),
    (
        Action::Security,
        "security",
        "Security",
        KeyCode::KeyU,
        false,
    ),
    (
        Action::SaberStyle,
        "saber_style",
        "Saber style",
        KeyCode::KeyL,
        false,
    ),
    (
        Action::Power,
        "power",
        "Selected power",
        KeyCode::KeyF,
        false,
    ),
    (Action::Grip, "grip", "Grip", KeyCode::KeyG, false),
    (
        Action::Lightning,
        "lightning",
        "Lightning",
        KeyCode::KeyR,
        false,
    ),
    (Action::Drain, "drain", "Drain", KeyCode::KeyQ, false),
    (Action::Grenade, "grenade", "Grenade", KeyCode::KeyT, false),
    (Action::Medpac, "medpac", "Medpac", KeyCode::Digit3, false),
    (
        Action::Shield,
        "shield",
        "Energy shield",
        KeyCode::Digit5,
        false,
    ),
    (Action::Stim, "stim", "Stim", KeyCode::Digit4, false),
    (
        Action::Stealth,
        "stealth",
        "Stealth mode",
        KeyCode::KeyX,
        false,
    ),
    (
        Action::LayMine,
        "lay_mine",
        "Lay a mine",
        KeyCode::KeyO,
        false,
    ),
    (
        Action::Holster,
        "holster",
        "Holster weapon",
        KeyCode::KeyH,
        false,
    ),
    (Action::Form, "next_form", "Next Form", KeyCode::KeyV, false),
    (
        Action::SwitchWeapons,
        "switch_weapons",
        "Switch weapons",
        KeyCode::KeyB,
        false,
    ),
    (
        Action::RightHand,
        "right_hand",
        "Right hand",
        KeyCode::Digit1,
        false,
    ),
    (
        Action::LeftHand,
        "left_hand",
        "Left hand",
        KeyCode::Digit2,
        false,
    ),
    (
        Action::NextPower,
        "next_power",
        "Next power",
        KeyCode::BracketRight,
        false,
    ),
    (
        Action::PreviousPower,
        "previous_power",
        "Previous power",
        KeyCode::BracketLeft,
        false,
    ),
    (
        Action::Inventory,
        "inventory",
        "Inventory",
        KeyCode::KeyI,
        false,
    ),
    (Action::Journal, "journal", "Journal", KeyCode::KeyJ, false),
    (Action::Map, "map", "Map", KeyCode::KeyM, false),
    (Action::Party, "party", "Party", KeyCode::KeyP, false),
    (
        Action::Abilities,
        "abilities",
        "Abilities",
        KeyCode::KeyK,
        false,
    ),
    (
        Action::QuickSave,
        "quick_save",
        "Quick save",
        KeyCode::F5,
        false,
    ),
    (
        Action::QuickLoad,
        "quick_load",
        "Quick load",
        KeyCode::F9,
        false,
    ),
    (
        Action::Leader,
        "leader",
        "Switch party leader",
        KeyCode::Tab,
        false,
    ),
    (
        Action::Follow,
        "follow",
        "Follow the next (watching)",
        KeyCode::KeyN,
        false,
    ),
    (Action::Break, "break", "Take a break", KeyCode::KeyZ, false),
    (Action::Ready, "ready", "Ready to go", KeyCode::KeyY, false),
    (Action::View, "view", "First person", KeyCode::F1, false),
    (
        Action::LeaveTalk,
        "leave_talk",
        "Leave a conversation",
        KeyCode::KeyL,
        false,
    ),
    (
        Action::AcceptCall,
        "accept_call",
        "Accept a join request",
        KeyCode::KeyY,
        false,
    ),
    (
        Action::EndCall,
        "end_call",
        "End a join request",
        KeyCode::KeyN,
        false,
    ),
];

/// Keys a binding may name, by the name the screens and the file use.
const KEYS: [KeyCode; 80] = {
    use KeyCode::*;
    [
        KeyA,
        KeyB,
        KeyC,
        KeyD,
        KeyE,
        KeyF,
        KeyG,
        KeyH,
        KeyI,
        KeyJ,
        KeyK,
        KeyL,
        KeyM,
        KeyN,
        KeyO,
        KeyP,
        KeyQ,
        KeyR,
        KeyS,
        KeyT,
        KeyU,
        KeyV,
        KeyW,
        KeyX,
        KeyY,
        KeyZ,
        Digit0,
        Digit1,
        Digit2,
        Digit3,
        Digit4,
        Digit5,
        Digit6,
        Digit7,
        Digit8,
        Digit9,
        F1,
        F2,
        F3,
        F4,
        F5,
        F6,
        F7,
        F8,
        F9,
        F10,
        F11,
        F12,
        Space,
        Tab,
        Enter,
        Backspace,
        ShiftLeft,
        ShiftRight,
        ControlLeft,
        ControlRight,
        AltLeft,
        AltRight,
        CapsLock,
        BracketLeft,
        BracketRight,
        Comma,
        Period,
        Semicolon,
        Quote,
        Slash,
        Backslash,
        Minus,
        Equal,
        Backquote,
        ArrowUp,
        ArrowDown,
        ArrowLeft,
        ArrowRight,
        Insert,
        Delete,
        Home,
        End,
        PageUp,
        PageDown,
    ]
};

/// A key's name (`KeyW`).
pub fn key_name(key: KeyCode) -> String {
    format!("{key:?}")
}

/// The key a name names, among those a binding may use.
pub fn key_named(name: &str) -> Option<KeyCode> {
    KEYS.iter().copied().find(|key| key_name(*key) == name)
}

/// Which key does what.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct KeyMap(BTreeMap<Action, KeyCode>);

impl Default for KeyMap {
    fn default() -> KeyMap {
        KeyMap(
            ACTIONS
                .iter()
                .map(|(action, _, _, key, _)| (*action, *key))
                .collect(),
        )
    }
}

impl KeyMap {
    /// The defaults, made once: for a system that reads a key with no
    /// settings in place (a headless or scripted run).
    pub fn defaults() -> &'static KeyMap {
        static DEFAULTS: std::sync::LazyLock<KeyMap> = std::sync::LazyLock::new(KeyMap::default);
        &DEFAULTS
    }

    pub fn key(&self, action: Action) -> KeyCode {
        self.0[&action]
    }

    pub fn held(&self, keys: &ButtonInput<KeyCode>, action: Action) -> bool {
        keys.pressed(self.key(action))
    }

    pub fn pressed_now(&self, keys: &ButtonInput<KeyCode>, action: Action) -> bool {
        keys.just_pressed(self.key(action))
    }

    /// The key bound to `action` as the screens name it (`W`, `1`, `F5`).
    pub fn shown(&self, action: Action) -> Vec<u8> {
        let name = key_name(self.key(action));
        ui::screens::key_mapping::key_name(&name)
            .as_bytes()
            .to_vec()
    }
}

/// How the mouse turns the view and which button attacks.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct MouseFeel {
    /// Times the default turn.
    pub scale: f32,
    pub invert: bool,
    /// The right button attacks, the left one is the other mode.
    pub swap: bool,
}

impl Default for MouseFeel {
    fn default() -> MouseFeel {
        MouseFeel {
            scale: 1.0,
            invert: false,
            swap: false,
        }
    }
}

/// The settings, by key, as kept in the file.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct Settings {
    values: BTreeMap<String, String>,
    path: Option<PathBuf>,
}

/// Each setting the screens change, and its default.
const DEFAULTS: [(&str, &str); 17] = [
    ("mouse.sensitivity", "0.50"),
    ("camera.invert", "false"),
    ("camera.view", "third"),
    ("mouse.reverse", "false"),
    ("sound.music", "1.00"),
    ("sound.voice", "1.00"),
    ("sound.effects", "1.00"),
    ("sound.movie", "1.00"),
    ("display.gamma", "0.50"),
    ("display.grass", "true"),
    ("display.mode", "window"),
    ("display.size", ""),
    ("display.vsync", "true"),
    ("display.msaa", "4"),
    ("display.shadows", "true"),
    ("display.soft_shadows", "true"),
    ("feedback.tutorials", "true"),
];

/// Each part of the mix and the setting of its slider.
const VOLUMES: [(Bus, &str); 4] = [
    (Bus::Music, "sound.music"),
    (Bus::Voice, "sound.voice"),
    (Bus::Effects, "sound.effects"),
    (Bus::Movie, "sound.movie"),
];

/// The key of this install's host id (`Settings::host_id`).
const HOST_ID: &str = "host.id";

/// The key of the layout of the key bindings a file was saved with, and
/// the layout now: 2 moved the medpac, stim and shield to the number row,
/// H holstering.
const KEY_LAYOUT: (&str, &str) = ("keys.layout", "2");

/// The bindings layout 2 moved, by their old default: a file of an older
/// layout still on one gets the new default.
const MOVED: [(&str, &str); 3] = [
    ("key.medpac", "KeyH"),
    ("key.stim", "KeyV"),
    ("key.shield", "KeyB"),
];

/// What a page's Default button puts back.
fn page_keys(page: &str) -> &'static [&'static str] {
    match page {
        "options_gameplay" => &["camera.invert"],
        "options_mouse" => &["mouse.sensitivity", "mouse.reverse"],
        "options_sound" => &["sound.music", "sound.voice", "sound.effects", "sound.movie"],
        "options_graphics" => &["display.gamma", "display.shadows", "display.grass"],
        "options_graphics_advanced" => &["display.vsync", "display.msaa", "display.soft_shadows"],
        _ => &[],
    }
}

/// The setting holding the folder the player chose for `game` (`setup`).
pub(crate) fn install_key(game: vfs::Game) -> String {
    format!("install.{}", game.key())
}

/// The folders the player chose for the games, by their variables' names
/// (`KJK_KOTOR1`, …), read from the settings file alone: `discover` reads
/// them before any app, and saves nothing.
pub(crate) fn chosen_installs() -> std::collections::HashMap<String, String> {
    let path = std::env::var_os("KJK_SETTINGS")
        .map(PathBuf::from)
        .or_else(|| folder().map(|folder| folder.join("settings.json")));
    let values = path
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<BTreeMap<String, String>>(&text).ok())
        .unwrap_or_else(BTreeMap::new);
    vfs::Game::ALL
        .into_iter()
        .filter_map(|game| {
            let chosen = values.get(&install_key(game))?;
            Some((game.env_var().to_string(), chosen.clone()))
        })
        .collect()
}

/// The folder for this program's settings: the platform's own place.
pub(crate) fn folder() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let base = if cfg!(target_os = "macos") {
        home.map(|home| home.join("Library").join("Application Support"))
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| home.map(|home| home.join(".config")))
    };
    base.map(|base| base.join("kjk"))
        .inspect(|folder| store::carry_old_name(folder))
}

impl Settings {
    fn default_values() -> BTreeMap<String, String> {
        let mut values: BTreeMap<String, String> = DEFAULTS
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        for (_, name, _, key, _) in ACTIONS {
            values.insert(format!("key.{name}"), key_name(key));
        }
        values.insert(KEY_LAYOUT.0.into(), KEY_LAYOUT.1.into());
        values
    }

    /// The defaults alone: no file, no path, nothing saved.
    pub fn defaults() -> Settings {
        Settings {
            values: Settings::default_values(),
            path: None,
        }
    }

    /// The settings in the user's file, over the defaults; a file that
    /// does not read is left for the defaults (and a warning). A file with
    /// no host id gets one, made at random and saved.
    pub fn load() -> Settings {
        // `KJK_SETTINGS` names another file (scripted runs, tests).
        let path = std::env::var_os("KJK_SETTINGS")
            .map(PathBuf::from)
            .or_else(|| folder().map(|folder| folder.join("settings.json")));
        let mut settings = Settings::defaults();
        let mut changed = false;
        if let Some(path) = &path
            && let Ok(text) = std::fs::read_to_string(path)
        {
            match serde_json::from_str::<BTreeMap<String, String>>(&text) {
                Ok(mut read) => {
                    changed = relayout(&mut read);
                    settings.values.extend(read);
                }
                Err(error) => warn!("{}: {error}; the defaults stand", path.display()),
            }
        }
        settings.path = path;
        if !settings.values.contains_key(HOST_ID) {
            let key = crate::solo::random_key();
            let id = key[..8].iter().map(|byte| format!("{byte:02x}")).collect();
            settings.values.insert(HOST_ID.into(), id);
            changed = true;
        }
        if changed {
            settings.save();
        }
        settings
    }

    /// The value set for `key`, if any.
    pub fn value(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    /// This install's host id: its players' copies of its saves say it,
    /// telling them from another host's (`saves::copies::Received::host`).
    pub fn host_id(&self) -> &str {
        self.values.get(HOST_ID).map_or("", String::as_str)
    }

    fn save(&self) {
        let Some(path) = &self.path else {
            return;
        };
        let written = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| {
                let text = serde_json::to_string_pretty(&self.values).expect("strings serialize");
                std::fs::write(path, text)
            });
        if let Err(error) = written {
            warn!("{}: {error}", path.display());
        }
    }

    /// `key` set to `value`; `reset` puts a page's settings (or `keys`)
    /// back. Keys must name a key a binding may use.
    pub fn apply(&mut self, key: &str, value: &str) {
        let defaults = Settings::default_values();
        match (key, value) {
            ("reset", "keys") => {
                for (name, default) in defaults.iter().filter(|(name, _)| name.starts_with("key."))
                {
                    self.values.insert(name.clone(), default.clone());
                }
            }
            ("reset", page) => {
                for name in page_keys(page) {
                    self.values
                        .insert(name.to_string(), defaults[*name].clone());
                }
            }
            (name, key) if name.starts_with("key.") && key_named(key).is_none() => {
                warn!("{key}: not a key a binding may use");
                return;
            }
            (name, value) => {
                self.values.insert(name.to_string(), value.to_string());
            }
        }
        self.save();
    }

    pub fn keys(&self) -> KeyMap {
        KeyMap(
            ACTIONS
                .iter()
                .map(|(action, name, _, default, _)| {
                    let bound = self
                        .values
                        .get(&format!("key.{name}"))
                        .and_then(|key| key_named(key));
                    (*action, bound.unwrap_or(*default))
                })
                .collect(),
        )
    }

    pub fn mouse(&self) -> MouseFeel {
        let sensitivity = self
            .values
            .get("mouse.sensitivity")
            .and_then(|value| value.parse::<f32>().ok())
            .unwrap_or(0.5)
            .clamp(0.0, 1.0);
        MouseFeel {
            // A quarter to twice the default, the default halfway.
            scale: if sensitivity < 0.5 {
                0.25 + 1.5 * sensitivity
            } else {
                2.0 * sensitivity
            },
            invert: self
                .values
                .get("camera.invert")
                .is_some_and(|v| v == "true"),
            swap: self
                .values
                .get("mouse.reverse")
                .is_some_and(|v| v == "true"),
        }
    }

    /// KOTOR's "Tutorial Popups" option (on by default, as the game's): a
    /// tutorial window a script shows comes up (`script_screens`).
    pub fn tutorials(&self) -> bool {
        self.values
            .get("feedback.tutorials")
            .is_none_or(|v| v != "false")
    }

    /// The player looks through its own eyes (`play_view::CameraMode`).
    pub fn first_person(&self) -> bool {
        self.values.get("camera.view").is_some_and(|v| v == "first")
    }

    /// Keeps whether the player looks through its own eyes.
    pub fn keep_first_person(&mut self, first: bool) {
        self.apply("camera.view", if first { "first" } else { "third" });
    }

    /// A slider's setting, 0 to 1 (`default` when unset or not a number).
    fn fraction(&self, key: &str, default: f32) -> f32 {
        self.values
            .get(key)
            .and_then(|value| value.parse::<f32>().ok())
            .unwrap_or(default)
            .clamp(0.0, 1.0)
    }

    /// Each part of the mix's volume: the slider squared, so its middle is
    /// a quarter (-12 dB), as loud sounds half as loud.
    pub fn volumes(&self) -> Volumes {
        Volumes(
            VOLUMES
                .iter()
                .map(|(bus, key)| (*bus, self.fraction(key, 1.0).powi(2)))
                .collect(),
        )
    }

    /// How the window is shown.
    pub fn display(&self) -> Display {
        let get = |key: &str| self.values.get(key).map_or("", String::as_str);
        Display {
            mode: DisplayMode::named(get("display.mode")),
            size: crate::display::size_named(get("display.size")),
            vsync: get("display.vsync") != "false",
            samples: match get("display.msaa") {
                "1" => 1,
                "2" => 2,
                _ => 4,
            },
            gamma: crate::display::gamma(self.fraction("display.gamma", 0.5)),
            grass: get("display.grass") != "false",
        }
    }

    /// Whether characters cast shadows, and soft ones.
    pub fn shadows(&self) -> crate::shadows::ShadowSettings {
        let on = |key: &str| self.values.get(key).is_none_or(|value| value != "false");
        crate::shadows::ShadowSettings {
            on: on("display.shadows"),
            soft: on("display.soft_shadows"),
        }
    }

    /// The key rows the key mapping shows.
    pub fn rows(&self) -> Vec<KeyRow> {
        let map = self.keys();
        ACTIONS
            .iter()
            .map(|(action, name, label, _, movement)| KeyRow {
                action: name.to_string(),
                label: label.to_string(),
                key: key_name(map.key(*action)),
                movement: *movement,
            })
            .collect()
    }
}

/// Each action's label and its default key as the key mapping shows it
/// (`W`, `Space`), in the mapping's order, the mouse's two buttons first:
/// the controls the rulebook's `[Action]` names (`kjk rules`).
pub fn key_bindings() -> Vec<(String, String)> {
    let mouse = MouseFeel::default();
    let (attack, alt) = if mouse.swap {
        ("Right mouse button", "Left mouse button")
    } else {
        ("Left mouse button", "Right mouse button")
    };
    let mut rows = vec![
        ("Attack".to_string(), attack.to_string()),
        ("Alt attack".to_string(), alt.to_string()),
    ];
    rows.extend(Settings::defaults().rows().into_iter().map(|row| {
        let shown = ui::screens::key_mapping::key_name(&row.key).to_string();
        (row.label, shown)
    }));
    rows
}

/// A file's settings brought to the bindings' layout now: those that moved
/// and it kept on their old default dropped (the new default stands), and
/// the layout set. Whether it was of an older layout.
fn relayout(read: &mut BTreeMap<String, String>) -> bool {
    let (key, layout) = KEY_LAYOUT;
    if read.get(key).is_some_and(|saved| saved == layout) {
        return false;
    }
    for (name, old) in MOVED {
        if read.get(name).is_some_and(|saved| saved == old) {
            read.remove(name);
        }
    }
    read.insert(key.into(), layout.into());
    true
}

pub struct SettingsPlugin;

impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        let settings = Settings::load();
        app.insert_resource(settings.keys())
            .insert_resource(settings.mouse())
            .insert_resource(settings.volumes())
            .insert_resource(settings.display())
            .insert_resource(settings.shadows())
            .insert_resource(settings)
            .add_systems(Startup, show)
            .add_systems(Update, apply.after(crate::ui_view::Stepped));
    }
}

/// The settings as the screens show them.
fn show(settings: Res<Settings>, ui: Option<ResMut<Ui>>) {
    if let Some(mut ui) = ui {
        ui.vm.settings = settings.values.clone();
        ui.vm.keys = settings.rows();
    }
}

/// The screens' setting changes, applied, saved and shown back.
fn apply(
    mut intents: ResMut<UiIntents>,
    mut settings: ResMut<Settings>,
    mut keys: ResMut<KeyMap>,
    mut mouse: ResMut<MouseFeel>,
    (mut volumes, mut display, mut shadows): (
        ResMut<Volumes>,
        ResMut<Display>,
        ResMut<crate::shadows::ShadowSettings>,
    ),
    ui: Option<ResMut<Ui>>,
) {
    let mut changed = false;
    intents.0.retain(|intent| match intent {
        Intent::Setting(key, value) => {
            settings.apply(key, value);
            changed = true;
            false
        }
        _ => true,
    });
    if changed {
        *keys = settings.keys();
        *mouse = settings.mouse();
        volumes.set_if_neq(settings.volumes());
        display.set_if_neq(settings.display());
        shadows.set_if_neq(settings.shadows());
        if let Some(mut ui) = ui {
            ui.vm.settings = settings.values.clone();
            ui.vm.keys = settings.rows();
        }
    }
}
