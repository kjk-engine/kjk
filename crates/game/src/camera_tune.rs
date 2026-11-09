//! The player's third-person camera as the options set it: how far back,
//! how high and how far to the side it stands, how wide it sees, how
//! closely it follows, and whether the own body fades when it comes close.
//! JA's camera (range 80, 16 over the eye, `cg_fov` 80, its damping) is the
//! default, each slider's default position giving JA's number exactly.
//!
//! The settings are the options' sliders, 0 to 1 (`camera.range`,
//! `camera.height`, `camera.shoulder`, `camera.fov`), a choice
//! (`camera.tracking`) and a check box (`camera.body_fade`); a preset
//! (`camera.preset`) sets the four sliders at once, and names the one they
//! match (`custom` when none).

use bevy::prelude::*;

/// JA's numbers, each a slider's default.
const JA_RANGE: f32 = 80.0;
const JA_HEIGHT: f32 = sim::combat::mind::CROSSHAIR_ABOVE_EYE;
const JA_FOV: f32 = 80.0;

/// Each slider: its setting, its default position (as the settings keep
/// it), and how many units (or degrees) its whole travel spans. A slider
/// at its default gives JA's number; the ends give the nearest and
/// farthest the options allow.
struct Slider {
    key: &'static str,
    default: &'static str,
    ja: f32,
    span: f32,
}

impl Slider {
    fn default_share(&self) -> f32 {
        self.default.parse().unwrap_or(0.0)
    }

    fn value(&self, share: f32) -> f32 {
        self.ja + (share - self.default_share()) * self.span
    }
}

/// The range slider's setting (the zoom moves it).
pub(crate) const RANGE_KEY: &str = "camera.range";

/// 45 to 185 units back: the near end is where zooming in goes into the
/// own eyes (`zoomed`). No farther: a camera's line runs at most 63 units
/// off JA's in its command (`sim::combat::mind::AIM_STEP`), and the farther
/// back, the less of a lagging pitch that covers (20° at 185).
const RANGE: Slider = Slider {
    key: RANGE_KEY,
    default: "0.25",
    ja: JA_RANGE,
    span: 140.0,
};
/// The aim point 24 under to 56 over the eye.
const HEIGHT: Slider = Slider {
    key: "camera.height",
    default: "0.50",
    ja: JA_HEIGHT,
    span: 80.0,
};
/// 40 units to the left to 40 to the right (over the right shoulder).
const SHOULDER: Slider = Slider {
    key: "camera.shoulder",
    default: "0.50",
    ja: 0.0,
    span: 80.0,
};
/// 60° to 110° across a 4:3 screen, as JA's `cg_fov` says it, in either
/// camera.
const FOV: Slider = Slider {
    key: "camera.fov",
    default: "0.40",
    ja: JA_FOV,
    span: 50.0,
};

const SLIDERS: [&Slider; 4] = [&RANGE, &HEIGHT, &SHOULDER, &FOV];

pub(crate) const PRESET: &str = "camera.preset";
pub(crate) const TRACKING: &str = "camera.tracking";
pub(crate) const BODY_FADE: &str = "camera.body_fade";

/// The presets: each one's name and where it puts the range, height,
/// shoulder and field of view sliders.
const PRESETS: [(&str, [&str; 4]); 4] = [
    (
        "ja",
        [RANGE.default, HEIGHT.default, SHOULDER.default, FOV.default],
    ),
    // 73 units back, 10 over the eye, 22 to the right.
    ("shoulder", ["0.20", "0.42", "0.78", FOV.default]),
    // KOTOR's own camera (`camerastyle.2da`'s first row): 3.2 m (126
    // units) back, higher, a field of view of 55° up and down (70° across
    // 4:3).
    ("kotor", ["0.58", "0.60", SHOULDER.default, "0.20"]),
    // 52 units back, 12 over the eye, 16 to the right.
    ("close", ["0.05", "0.45", "0.70", FOV.default]),
];

/// Every camera setting and its default, for the settings' table.
pub(crate) const DEFAULTS: [(&str, &str); 7] = [
    (PRESET, "ja"),
    (RANGE.key, RANGE.default),
    (HEIGHT.key, HEIGHT.default),
    (SHOULDER.key, SHOULDER.default),
    (FOV.key, FOV.default),
    (TRACKING, "ja"),
    (BODY_FADE, "true"),
];

/// How closely the camera follows: JA's damping, a stiffer one, or none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tracking {
    #[default]
    Ja,
    Tight,
    Off,
}

impl Tracking {
    fn named(name: &str) -> Tracking {
        match name {
            "tight" => Tracking::Tight,
            "off" => Tracking::Off,
            _ => Tracking::Ja,
        }
    }

    /// The share of the way the aim point and the camera's place close on
    /// where they should be each 50 ms (JA's `cg_thirdPersonTargetDamp` and
    /// `cg_thirdPersonCameraDamp`); 1 is there at once.
    pub fn damps(self) -> (f32, f32) {
        match self {
            Tracking::Ja => (0.5, 0.3),
            Tracking::Tight => (0.8, 0.65),
            Tracking::Off => (1.0, 1.0),
        }
    }
}

/// The camera the options ask for (units, degrees).
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct CameraTune {
    pub range: f32,
    /// The aim point over the eye.
    pub height: f32,
    /// To the right of the view's line (left below 0).
    pub shoulder: f32,
    /// Across a 4:3 screen.
    pub fov: f32,
    pub tracking: Tracking,
    pub body_fade: bool,
}

impl Default for CameraTune {
    fn default() -> CameraTune {
        CameraTune::from_settings(|_| None)
    }
}

impl CameraTune {
    /// From the settings `value` reads (none: each default).
    pub fn from_settings<'a>(value: impl Fn(&str) -> Option<&'a str>) -> CameraTune {
        let share = |slider: &Slider| {
            value(slider.key)
                .and_then(|text| text.parse::<f32>().ok())
                .unwrap_or_else(|| slider.default_share())
                .clamp(0.0, 1.0)
        };
        CameraTune {
            range: RANGE.value(share(&RANGE)),
            height: HEIGHT.value(share(&HEIGHT)),
            shoulder: SHOULDER.value(share(&SHOULDER)),
            fov: FOV.value(share(&FOV)),
            tracking: Tracking::named(value(TRACKING).unwrap_or("ja")),
            body_fade: value(BODY_FADE).is_none_or(|on| on != "false"),
        }
    }

    /// The field of view up and down (degrees) for a view whose JA field
    /// of view (80) is `ja_vertical`: JA's across 4:3 as this tune has it.
    pub fn vertical_fov(&self, ja_vertical: f32) -> f32 {
        let vertical = |across: f32| ((across / 2.0).to_radians().tan() * 0.75).atan() * 2.0;
        ja_vertical * vertical(self.fov) / vertical(JA_FOV)
    }
}

/// The settings a preset's choice sets: its four sliders (none for a name
/// no preset has).
pub(crate) fn preset_values(name: &str) -> Option<[(&'static str, &'static str); 4]> {
    let (_, values) = PRESETS.iter().find(|(preset, _)| *preset == name)?;
    Some(std::array::from_fn(|i| (SLIDERS[i].key, values[i])))
}

/// Whether `key` is one of the camera's sliders (moving one names the
/// preset again, `preset_named`).
pub(crate) fn is_slider(key: &str) -> bool {
    SLIDERS.iter().any(|slider| slider.key == key)
}

/// The preset whose sliders `value` reads match, `custom` when none does.
pub(crate) fn preset_named<'a>(value: impl Fn(&str) -> Option<&'a str>) -> &'static str {
    PRESETS
        .iter()
        .find(|(name, _)| {
            preset_values(name).is_some_and(|wanted| {
                wanted.iter().all(|(key, share)| {
                    value(key)
                        .and_then(|set| set.parse::<f32>().ok())
                        .is_some_and(|set| {
                            (set - share.parse::<f32>().unwrap_or(-1.0)).abs() < 0.005
                        })
                })
            })
        })
        .map_or("custom", |(name, _)| name)
}

/// The range slider's share after a zoom `steps` in (out below 0): a
/// twentieth of its travel (10 units) a step. `None` when it is at the near
/// end already and zooms further in: the own eyes take over.
pub(crate) fn zoomed(range_share: Option<&str>, steps: f32) -> Option<String> {
    let now = range_share
        .and_then(|text| text.parse::<f32>().ok())
        .unwrap_or_else(|| RANGE.default_share())
        .clamp(0.0, 1.0);
    if steps > 0.0 && now <= 0.0 {
        return None;
    }
    Some(format!("{:.2}", (now - steps * ZOOM_STEP).clamp(0.0, 1.0)))
}

/// A zoom step's share of the range slider.
const ZOOM_STEP: f32 = 0.05;

/// The range slider's near end, where zooming out of the own eyes comes
/// back to.
pub(crate) const NEAREST: &str = "0.00";

/// The camera turned around the character while the orbit key is held
/// (degrees over the view: yaw to the left, pitch down), easing back
/// behind it once let go; and the zoom asked for and not taken yet (steps,
/// in above 0).
#[derive(Resource, Debug, Default)]
pub struct Orbit {
    pub yaw: f32,
    pub pitch: f32,
    pub held: bool,
    /// The third-person camera has the view (`play_view` says so each
    /// frame): the orbit key does nothing in the own eyes, a conversation
    /// or a fixed camera.
    pub(crate) free: bool,
    zoom: f32,
    /// A run's commands hold the orbit, turning to this yaw and pitch
    /// (`script`).
    scripted: Option<[f32; 2]>,
}

/// How fast a run's commands turn the orbit (degrees a second).
const SCRIPT_TURN: f32 = 120.0;

/// The farthest the orbit pitches the camera over or under the view.
const ORBIT_PITCH: f32 = 60.0;
/// Let go, the orbit closes this share of its way back each second: behind
/// the character again in about a sixth of a second.
const ORBIT_RETURN: f32 = 6.0;
/// Under this (degrees) the way back is done.
const ORBIT_DONE: f32 = 0.1;
/// Zoom steps per degree the pad's right stick pushes up or down while the
/// orbit is held.
const PAD_ZOOM: f32 = 0.05;
/// A wheel's pixels per line (a touchpad scrolls by pixels).
const WHEEL_PIXELS: f32 = 40.0;

impl Orbit {
    /// A run's request (`ui camera …`): `orbit YAW PITCH` holds the orbit
    /// and turns it there, `orbit off` lets it go, `zoom STEPS` zooms (in
    /// above 0). False for anything else.
    pub(crate) fn script(&mut self, words: &[&str]) -> bool {
        let number = |word: &str| word.parse::<f32>().ok();
        match words {
            ["orbit", "off"] => self.scripted = None,
            ["orbit", yaw, pitch] => match (number(yaw), number(pitch)) {
                (Some(yaw), Some(pitch)) => self.scripted = Some([around(yaw), pitch]),
                _ => return false,
            },
            ["zoom", steps] => match number(steps) {
                Some(steps) => self.zoom += steps,
                None => return false,
            },
            _ => return false,
        }
        true
    }

    /// The whole zoom steps asked for (in above 0), the rest kept.
    pub(crate) fn take_zoom(&mut self) -> f32 {
        let whole = self.zoom.trunc();
        self.zoom -= whole;
        whole
    }
}

/// `degrees` within half a turn either way, so the way back is the short
/// one.
fn around(degrees: f32) -> f32 {
    (degrees + 180.0).rem_euclid(360.0) - 180.0
}

/// The input the camera reads this frame.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct CameraInput<'w, 's> {
    actions: Option<Res<'w, crate::pad::Actions>>,
    feel: Option<Res<'w, crate::settings::MouseFeel>>,
    motion: Option<Res<'w, bevy::input::mouse::AccumulatedMouseMotion>>,
    wheel: MessageReader<'w, 's, bevy::input::mouse::MouseWheel>,
    screens: Option<Res<'w, crate::ui_view::UiOpen>>,
    cursors: Query<'w, 's, &'static bevy::window::CursorOptions, With<bevy::window::PrimaryWindow>>,
}

/// The orbit and the zoom from this frame's input: the zoom keys; with the
/// orbit held, the mouse turning the camera around (a pad's right stick
/// across, its push up or down zooming) and the wheel zooming.
pub(crate) fn steer(mut input: CameraInput, time: Res<Time>, mut orbit: ResMut<Orbit>) {
    use crate::settings::Action;
    let wheel: f32 = input
        .wheel
        .read()
        .map(|event| match event.unit {
            bevy::input::mouse::MouseScrollUnit::Line => event.y,
            bevy::input::mouse::MouseScrollUnit::Pixel => event.y / WHEEL_PIXELS,
        })
        .sum();
    if let Some(to) = orbit.scripted {
        let most = SCRIPT_TURN * time.delta_secs();
        orbit.held = true;
        orbit.yaw += (to[0] - orbit.yaw).clamp(-most, most);
        orbit.pitch += (to[1].clamp(-ORBIT_PITCH, ORBIT_PITCH) - orbit.pitch).clamp(-most, most);
        return;
    }
    let open = input.screens.as_ref().is_some_and(|open| open.0);
    let Some(actions) = input.actions.as_deref().filter(|_| !open) else {
        orbit.held = false;
        return;
    };
    orbit.zoom += f32::from(u8::from(actions.pressed(Action::ZoomIn)))
        - f32::from(u8::from(actions.pressed(Action::ZoomOut)));
    // The wheel zooms with the orbit key held in either camera (out of the
    // own eyes too); the camera turns around only in the third person's.
    let key = actions.held(Action::Orbit);
    if key {
        orbit.zoom += wheel;
    }
    orbit.held = orbit.free && key;
    if orbit.held {
        let grabbed = input
            .cursors
            .iter()
            .any(|cursor| cursor.grab_mode != bevy::window::CursorGrabMode::None);
        let feel = input
            .feel
            .as_deref()
            .map_or_else(crate::settings::MouseFeel::default, |feel| *feel);
        let up = if feel.invert { -1.0 } else { 1.0 };
        let mouse = input
            .motion
            .as_ref()
            .filter(|_| grabbed)
            .map_or(Vec2::ZERO, |motion| {
                Vec2::new(motion.delta.x, up * motion.delta.y)
                    * crate::sim_host::MOUSE_DEGREES
                    * feel.scale
            });
        orbit.yaw = around(orbit.yaw - mouse.x - actions.turn.x);
        orbit.pitch = (orbit.pitch + mouse.y).clamp(-ORBIT_PITCH, ORBIT_PITCH);
        orbit.zoom -= actions.turn.y * PAD_ZOOM;
        return;
    }
    let back = (time.delta_secs() * ORBIT_RETURN).min(1.0);
    orbit.yaw -= orbit.yaw * back;
    orbit.pitch -= orbit.pitch * back;
    if orbit.yaw.abs() < ORBIT_DONE && orbit.pitch.abs() < ORBIT_DONE {
        orbit.yaw = 0.0;
        orbit.pitch = 0.0;
    }
}

/// How near the third-person camera stands to the own character's eye
/// (units), this frame: none while another camera has the view (the own
/// eyes, a conversation's, a fixed one). `play_view` says it.
#[derive(Resource, Debug, Default)]
pub struct OwnNear(pub Option<(sim::SimId, f32)>);

/// Nearer than this to the eye (units) the own body starts to fade, and
/// at this it is as faint as it goes (`FAINTEST`): a camera pressed into
/// it by a wall shows through it, not its insides.
const FADE_FROM: f32 = 28.0;
const FADE_TO: f32 = 10.0;
const FAINTEST: f32 = 0.15;

/// The own body faded: each of its meshes, its own material and the
/// faded copy it wears meanwhile, with the copy's alpha unfaded.
#[derive(Default)]
pub(crate) struct Faded {
    root: Option<Entity>,
    meshes: Vec<(
        Entity,
        Handle<crate::material::OdysseyMaterial>,
        Handle<crate::material::OdysseyMaterial>,
        f32,
    )>,
}

/// The own body faded while the camera is near its eye, as the options
/// ask (`CameraTune::body_fade`); whole again, its own materials back,
/// once the camera is out.
pub(crate) fn fade_near(
    (near, tune, roots): (
        Res<OwnNear>,
        Option<Res<CameraTune>>,
        Res<crate::cast::BodyRoots>,
    ),
    children: Query<&Children>,
    mut meshes: Query<&mut MeshMaterial3d<crate::material::OdysseyMaterial>>,
    mut materials: ResMut<Assets<crate::material::OdysseyMaterial>>,
    mut faded: Local<Faded>,
) {
    let fades = tune.is_none_or(|tune| tune.body_fade);
    let wanted = near.0.filter(|_| fades).and_then(|(id, distance)| {
        let alpha = ((distance - FADE_TO) / (FADE_FROM - FADE_TO)).clamp(FAINTEST, 1.0);
        (alpha < 1.0).then_some((*roots.0.get(&id)?, alpha))
    });
    // Another body, or none to fade: the faded one's own materials back.
    if faded.root.is_some() && faded.root != wanted.map(|(root, _)| root) {
        // A mesh dressed anew meanwhile keeps what it was given.
        for (entity, own, copy, _) in faded.meshes.drain(..) {
            if let Ok(mut worn) = meshes.get_mut(entity)
                && worn.0 == copy
            {
                worn.0 = own;
            }
        }
        faded.root = None;
    }
    let Some((root, alpha)) = wanted else {
        return;
    };
    if faded.root.is_none() {
        for entity in children.iter_descendants(root) {
            let Ok(mut worn) = meshes.get_mut(entity) else {
                continue;
            };
            let Some(copy) = materials.get(&worn.0).map(crate::going_view::blended) else {
                continue;
            };
            let was = copy.params.tint.w;
            let own = std::mem::replace(&mut worn.0, materials.add(copy));
            faded.meshes.push((entity, own, worn.0.clone(), was));
        }
        faded.root = Some(root);
    }
    for (_, _, copy, was) in &faded.meshes {
        if let Some(mut material) = materials.get_mut(copy) {
            material.params.tint.w = was * alpha;
        }
    }
}

pub struct CameraTunePlugin;

impl Plugin for CameraTunePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Orbit>()
            .init_resource::<OwnNear>()
            .add_systems(PreUpdate, steer.after(crate::pad::Sampled))
            .add_systems(
                PostUpdate,
                fade_near
                    .run_if(resource_exists::<crate::cast::BodyRoots>)
                    .after(crate::play_view::Drawn),
            );
    }
}
