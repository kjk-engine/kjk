//! The `kjk` startup splash screen: draws the exact Möbius strip from
//! `docs/logo/kjk.svg` (40 cross-rulings + continuous boundary loop) in a
//! precision trace, glides the mark left while curtain-unmasking the
//! `kjk-engine` wordmark in Geist Mono Medium, fades in `Open source. Free
//! forever.` at the bottom in Geist Mono Regular, plays a restrained
//! procedural synth cue, and dissolves straight into the live main menu.
//!
//! Uses only the `kjk` design system tokens (`GROUND` `#121212`, `INK`
//! `#e6e9e7`, `INK_FAINT` `#868b88`, zero gradients/glows/shadows) and Bevy's
//! own UI, so it appears on frame 1 while KOTOR's installs and 3D main menu
//! warm in the background.

use audio::Clip;
use bevy::asset::RenderAssetUsages;
use bevy::camera::ClearColorConfig;
use bevy::image::{ImageSampler, ImageSamplerDescriptor};
use bevy::input::gamepad::GamepadButton;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::window::PrimaryWindow;

use crate::lifecycle_view::OverlayCamera;
use crate::sound::{Added, Now};

const FONT_MEDIUM: &[u8] = include_bytes!("../fonts/GeistMono-Medium.ttf");
const FONT_REGULAR: &[u8] = include_bytes!("../fonts/GeistMono-Regular.ttf");

/// Exact `kjk` design system tokens.
const GROUND_RGB: [f32; 3] = [
    0x12 as f32 / 255.0,
    0x12 as f32 / 255.0,
    0x12 as f32 / 255.0,
];
const INK_RGB: [f32; 3] = [
    0xe6 as f32 / 255.0,
    0xe9 as f32 / 255.0,
    0xe7 as f32 / 255.0,
];
const INK_FAINT_RGB: [f32; 3] = [
    0x86 as f32 / 255.0,
    0x8b as f32 / 255.0,
    0x88 as f32 / 255.0,
];
const INK_BYTES: [u8; 3] = [0xe6, 0xe9, 0xe7];

const WORDMARK: &str = "kjk-engine";
const FINE_PRINT: &str = "Open-source engine. Not affiliated with Lucasfilm, BioWare, or Obsidian.";
const SOUND_NAME: &str = "kjk/splash";

/// Environment variable that skips the startup splash (`kjk --no-splash` sets it).
pub const NO_SPLASH: &str = "KJK_NO_SPLASH";

/// Whether `NO_SPLASH` is set to a truthy value (anything other than `"0"`).
pub fn skipped() -> bool {
    std::env::var_os(NO_SPLASH).is_some_and(|value| value != "0")
}

/// Total splash duration (seconds) including the dissolve into the main menu.
const DURATION: f32 = 3.15;
const FADE_START: f32 = 2.65;
const FADE_DURATION: f32 = 0.50;
const SKIP_FADE: f32 = 0.22;

/// High-DPI raster target for the Möbius strip (aspect matches `kjk.svg` ~1.91).
const TEX_W: u32 = 512;
const TEX_H: u32 = 280;

/// Logical display dimensions of the lockup.
const MARK_H: f32 = 64.0;
const WORDMARK_SIZE: f32 = 28.0;
const FINE_SIZE: f32 = 11.0;
const LOCKUP_GAP: f32 = 20.0;
/// Geist Mono advance ratio (~0.60 of font size per glyph; 10 glyphs in `kjk-engine`).
const WORDMARK_EST_W: f32 = WORDMARK_SIZE * 0.60 * 10.0;

/// Exact 40 cross-ruling segments from `docs/logo/kjk.svg`.
const RULINGS: [[f32; 4]; 40] = [
    [67.390628, 47.671877, 90.567712, 44.562502],
    [67.937503, 49.869794, 91.729171, 46.989585],
    [68.119795, 52.098961, 91.791671, 49.468752],
    [67.932295, 54.328127, 90.760421, 51.911461],
    [67.348961, 56.541669, 88.661462, 54.208336],
    [66.380211, 58.708336, 85.578129, 56.291669],
    [65.010419, 60.791669, 81.619795, 58.088544],
    [63.229169, 62.781253, 76.921878, 59.520836],
    [61.052086, 64.619794, 71.630211, 60.541669],
    [58.489586, 66.281253, 65.911461, 61.119794],
    [55.567711, 67.739586, 59.958336, 61.239586],
    [52.317711, 68.937503, 53.947919, 60.890628],
    [48.802085, 69.869795, 48.062502, 60.088544],
    [45.06771, 70.468753, 42.458335, 58.880211],
    [41.197918, 70.718753, 37.302085, 57.291669],
    [37.302085, 70.598961, 32.697918, 55.380211],
    [33.447918, 70.098961, 28.791668, 53.208336],
    [29.791668, 69.19792, 25.619793, 50.869794],
    [26.41146, 67.921878, 23.270834, 48.421877],
    [23.447918, 66.270836, 21.729168, 45.932294],
    [21.020834, 64.270836, 21.020834, 43.489585],
    [19.229168, 61.979169, 21.098959, 41.16146],
    [18.171876, 59.432294, 21.921876, 39.000002],
    [17.921876, 56.697919, 23.401043, 37.06771],
    [18.531251, 53.838544, 25.447918, 35.41146],
    [20.041668, 50.947919, 28.000001, 34.052085],
    [22.447918, 48.109377, 30.921876, 33.010418],
    [25.729168, 45.38021, 34.119793, 32.31771],
    [29.81771, 42.869794, 37.500002, 31.968751],
    [34.63021, 40.640627, 40.968752, 31.947918],
    [40.041668, 38.760418, 44.432294, 32.260418],
    [45.91146, 37.291668, 47.81771, 32.869793],
    [52.078127, 36.281252, 51.062502, 33.760418],
    [58.359378, 35.750002, 54.109377, 34.901043],
    [64.578128, 35.718752, 56.921877, 36.270835],
    [70.54167, 36.187502, 59.458336, 37.828127],
    [76.062503, 37.119793, 61.697919, 39.56771],
    [80.968753, 38.489585, 63.619794, 41.437502],
    [85.098962, 40.229168, 65.218753, 43.432294],
    [88.338545, 42.281252, 66.47917, 45.520835],
];

/// Exact 201-point continuous boundary loop from `docs/logo/kjk.svg`.
const LOOP: [[f32; 2]; 201] = [
    [90.567712, 44.562502],
    [91.171879, 45.520835],
    [91.588546, 46.500002],
    [91.828129, 47.489585],
    [91.901046, 48.479169],
    [91.791671, 49.468752],
    [91.510421, 50.447919],
    [91.052087, 51.432294],
    [90.421879, 52.380211],
    [89.619795, 53.312502],
    [88.661462, 54.208336],
    [87.54167, 55.067711],
    [86.270837, 55.901044],
    [84.859379, 56.682294],
    [83.302087, 57.411461],
    [81.619795, 58.088544],
    [79.817712, 58.708336],
    [77.911462, 59.260419],
    [75.901045, 59.760419],
    [73.802086, 60.182294],
    [71.630211, 60.541669],
    [69.380211, 60.828128],
    [67.078128, 61.041669],
    [64.739586, 61.182294],
    [62.359378, 61.250003],
    [59.958336, 61.239586],
    [57.552086, 61.151044],
    [55.151044, 61.000003],
    [52.760419, 60.770836],
    [50.390627, 60.458336],
    [48.062502, 60.088544],
    [45.781252, 59.661461],
    [43.552085, 59.151044],
    [41.390627, 58.588544],
    [39.302085, 57.968752],
    [37.302085, 57.291669],
    [35.38021, 56.562502],
    [33.56771, 55.781252],
    [31.859376, 54.958336],
    [30.270835, 54.109377],
    [28.791668, 53.208336],
    [27.432293, 52.291669],
    [26.187501, 51.348961],
    [25.088543, 50.380211],
    [24.109376, 49.41146],
    [23.270834, 48.421877],
    [22.552084, 47.421877],
    [21.968751, 46.432294],
    [21.531251, 45.437502],
    [21.208334, 44.458335],
    [21.020834, 43.489585],
    [20.958334, 42.541669],
    [21.020834, 41.609377],
    [21.208334, 40.708335],
    [21.510418, 39.838543],
    [21.921876, 39.000002],
    [22.432293, 38.197918],
    [23.052084, 37.432293],
    [23.760418, 36.708335],
    [24.567709, 36.041668],
    [25.447918, 35.41146],
    [26.421876, 34.81771],
    [27.447918, 34.291668],
    [28.552085, 33.812501],
    [29.708335, 33.390626],
    [30.921876, 33.010418],
    [32.171876, 32.697918],
    [33.458335, 32.432293],
    [34.791668, 32.218751],
    [36.13021, 32.06771],
    [37.500002, 31.968751],
    [38.88021, 31.921876],
    [40.270835, 31.932293],
    [41.66146, 31.989585],
    [43.052085, 32.09896],
    [44.432294, 32.260418],
    [45.802085, 32.468751],
    [47.151044, 32.729168],
    [48.479169, 33.031251],
    [49.791669, 33.369793],
    [51.062502, 33.760418],
    [52.312502, 34.187502],
    [53.520836, 34.651043],
    [54.687502, 35.16146],
    [55.828127, 35.697918],
    [56.921877, 36.270835],
    [57.968752, 36.869793],
    [58.968753, 37.510418],
    [59.932294, 38.171877],
    [60.838544, 38.84896],
    [61.697919, 39.56771],
    [62.510419, 40.302085],
    [63.260419, 41.062502],
    [63.968753, 41.828127],
    [64.619794, 42.619794],
    [65.218753, 43.432294],
    [65.760419, 44.260419],
    [66.250003, 45.088544],
    [66.687503, 45.937502],
    [67.067711, 46.802085],
    [67.390628, 47.671877],
    [67.651045, 48.552085],
    [67.859378, 49.432294],
    [68.000003, 50.317711],
    [68.088545, 51.208336],
    [68.119795, 52.098961],
    [68.088545, 52.989586],
    [68.000003, 53.890627],
    [67.848961, 54.781252],
    [67.630211, 55.661461],
    [67.348961, 56.541669],
    [67.01042, 57.411461],
    [66.609378, 58.281253],
    [66.140628, 59.130211],
    [65.598961, 59.968753],
    [65.010419, 60.791669],
    [64.338544, 61.598961],
    [63.619794, 62.390628],
    [62.817711, 63.161461],
    [61.968753, 63.901044],
    [61.052086, 64.619794],
    [60.067711, 65.312503],
    [59.031253, 65.968753],
    [57.932294, 66.588545],
    [56.781252, 67.187503],
    [55.567711, 67.739586],
    [54.312502, 68.250003],
    [52.989586, 68.72917],
    [51.640627, 69.151045],
    [50.239586, 69.531253],
    [48.802085, 69.869795],
    [47.328127, 70.151045],
    [45.828127, 70.369795],
    [44.302085, 70.552086],
    [42.760419, 70.661461],
    [41.197918, 70.718753],
    [39.640627, 70.718753],
    [38.078127, 70.661461],
    [36.520835, 70.531253],
    [34.979168, 70.348961],
    [33.447918, 70.098961],
    [31.958335, 69.79167],
    [30.500001, 69.421878],
    [29.088543, 68.97917],
    [27.718751, 68.47917],
    [26.41146, 67.921878],
    [25.171876, 67.302086],
    [24.010418, 66.630211],
    [22.921876, 65.901044],
    [21.921876, 65.109378],
    [21.020834, 64.270836],
    [20.218751, 63.390628],
    [19.531251, 62.458336],
    [18.947918, 61.489586],
    [18.500001, 60.479169],
    [18.171876, 59.432294],
    [17.968751, 58.348961],
    [17.901042, 57.250002],
    [17.968751, 56.130211],
    [18.182293, 54.989586],
    [18.531251, 53.838544],
    [19.031251, 52.687502],
    [19.671876, 51.531252],
    [20.447918, 50.380211],
    [21.380209, 49.229169],
    [22.447918, 48.109377],
    [23.661459, 47.000002],
    [25.010418, 45.91146],
    [26.489585, 44.859377],
    [28.088543, 43.84896],
    [29.81771, 42.869794],
    [31.66146, 41.937502],
    [33.609376, 41.062502],
    [35.671877, 40.229168],
    [37.812502, 39.468752],
    [40.041668, 38.760418],
    [42.338544, 38.119793],
    [44.708335, 37.552085],
    [47.13021, 37.052085],
    [49.588544, 36.63021],
    [52.078127, 36.281252],
    [54.588544, 36.010418],
    [57.109377, 35.81771],
    [59.619794, 35.708335],
    [62.119794, 35.682293],
    [64.578128, 35.718752],
    [67.01042, 35.84896],
    [69.380211, 36.052085],
    [71.687503, 36.338543],
    [73.921878, 36.687502],
    [76.062503, 37.119793],
    [78.109378, 37.619793],
    [80.04167, 38.182293],
    [81.859378, 38.812502],
    [83.552087, 39.489585],
    [85.098962, 40.229168],
    [86.51042, 41.020835],
    [87.770837, 41.84896],
    [88.859379, 42.718752],
    [89.802087, 43.63021],
    [90.567712, 44.562502],
];

/// Whether the startup splash is currently on screen (blocks menu input and
/// holds the main menu music until the dissolve completes).
#[derive(Resource, Default)]
pub struct SplashShowing(pub bool);

/// Adds the startup splash when `enabled` is true.
pub struct SplashPlugin {
    pub enabled: bool,
}

impl Plugin for SplashPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SplashShowing>();
        if !self.enabled {
            return;
        }
        app.insert_resource(SplashShowing(true))
            .add_systems(Startup, setup)
            .add_systems(Update, step);
    }
}

#[derive(Component)]
struct SplashCamera;

#[derive(Component)]
struct SplashRoot;

#[derive(Component)]
struct SplashMark;

#[derive(Component)]
struct SplashWordClip;

#[derive(Component)]
struct SplashWordText;

#[derive(Component)]
struct SplashFineText;

/// Normalized `kjk.svg` geometry and precomputed cumulative loop lengths.
struct Geometry {
    aspect: f32,
    rulings: Vec<[f32; 4]>,
    loop_pts: Vec<[f32; 2]>,
    loop_len: Vec<f32>,
    total_len: f32,
}

impl Geometry {
    fn build() -> Geometry {
        let (mut min_x, mut max_x) = (f32::INFINITY, f32::NEG_INFINITY);
        let (mut min_y, mut max_y) = (f32::INFINITY, f32::NEG_INFINITY);
        for [x, y] in LOOP {
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }
        let cx = (min_x + max_x) * 0.5;
        let cy = (min_y + max_y) * 0.5;
        let h = max_y - min_y;
        let norm = |x: f32, y: f32| [(x - cx) / h, (y - cy) / h];
        let rulings = RULINGS
            .iter()
            .map(|&[x1, y1, x2, y2]| {
                let [a, b] = norm(x1, y1);
                let [c, d] = norm(x2, y2);
                [a, b, c, d]
            })
            .collect();
        let loop_pts: Vec<[f32; 2]> = LOOP.iter().map(|&[x, y]| norm(x, y)).collect();
        let mut loop_len = Vec::with_capacity(loop_pts.len());
        loop_len.push(0.0);
        for i in 1..loop_pts.len() {
            let dx = loop_pts[i][0] - loop_pts[i - 1][0];
            let dy = loop_pts[i][1] - loop_pts[i - 1][1];
            loop_len.push(loop_len[i - 1] + dx.hypot(dy));
        }
        let total_len = *loop_len.last().unwrap_or(&1.0);
        Geometry {
            aspect: (max_x - min_x) / h,
            rulings,
            loop_pts,
            loop_len,
            total_len,
        }
    }
}

#[derive(Resource)]
struct SplashState {
    elapsed: f32,
    skip_at: Option<f32>,
    warmup_frames: u8,
    started: bool,
    clip: Option<Clip>,
    image: Handle<Image>,
    camera: Entity,
    root: Entity,
    geometry: Geometry,
    coverage: Vec<u8>,
}

fn setup(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut fonts: ResMut<Assets<Font>>,
) {
    let geometry = Geometry::build();
    let mut image = Image::new_fill(
        Extent3d {
            width: TEX_W,
            height: TEX_H,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 0],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::linear());
    let image = images.add(image);

    let medium = fonts.add(Font::from_bytes(FONT_MEDIUM.to_vec()));
    let regular = fonts.add(Font::from_bytes(FONT_REGULAR.to_vec()));

    let camera = commands
        .spawn((
            Camera2d,
            Camera {
                order: 20,
                clear_color: ClearColorConfig::None,
                ..default()
            },
            OverlayCamera,
            SplashCamera,
        ))
        .id();

    let mark_w = MARK_H * geometry.aspect;
    let ground = Color::srgba(GROUND_RGB[0], GROUND_RGB[1], GROUND_RGB[2], 1.0);
    let ink_clear = Color::srgba(INK_RGB[0], INK_RGB[1], INK_RGB[2], 0.0);
    let faint_clear = Color::srgba(INK_FAINT_RGB[0], INK_FAINT_RGB[1], INK_FAINT_RGB[2], 0.0);

    let root = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            BackgroundColor(ground),
            UiTargetCamera(camera),
            SplashRoot,
        ))
        .with_children(|parent| {
            // Möbius strip mark (positioned dynamically each frame).
            parent.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    top: Val::Px(0.0),
                    width: Val::Px(mark_w),
                    height: Val::Px(MARK_H),
                    ..default()
                },
                ImageNode::new(image.clone()),
                SplashMark,
            ));

            // Curtain-clipped container for `kjk-engine` (left edge tracks
            // the moving right edge of the Möbius mark so there is zero
            // overlap at any frame).
            parent
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(0.0),
                        top: Val::Px(0.0),
                        width: Val::Px(0.0),
                        height: Val::Px(MARK_H),
                        overflow: Overflow::clip(),
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    SplashWordClip,
                ))
                .with_children(|clip| {
                    clip.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(0.0),
                            ..default()
                        },
                        Text::new(WORDMARK),
                        TextFont {
                            font: FontSource::Handle(medium),
                            font_size: FontSize::Px(WORDMARK_SIZE),
                            ..default()
                        },
                        TextColor(ink_clear),
                        TextLayout::no_wrap(),
                        SplashWordText,
                    ));
                });

            // Quiet fine print centered at the bottom.
            parent.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    right: Val::Px(0.0),
                    bottom: Val::Px(34.0),
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                children![(
                    Text::new(FINE_PRINT),
                    TextFont {
                        font: FontSource::Handle(regular),
                        font_size: FontSize::Px(FINE_SIZE),
                        ..default()
                    },
                    TextColor(faint_clear),
                    SplashFineText,
                )],
            ));
        })
        .id();

    commands.insert_resource(SplashState {
        elapsed: 0.0,
        skip_at: None,
        warmup_frames: 0,
        started: false,
        clip: Some(synth_splash_clip()),
        image,
        camera,
        root,
        geometry,
        coverage: vec![0; (TEX_W * TEX_H) as usize],
    });
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn step(
    mut commands: Commands,
    time: Res<Time>,
    state: Option<ResMut<SplashState>>,
    mut showing: ResMut<SplashShowing>,
    mut images: ResMut<Assets<Image>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    gamepads: Query<&Gamepad>,
    mut added: Option<ResMut<Added>>,
    mut now: Option<ResMut<Now>>,
    mut roots: Query<&mut BackgroundColor, With<SplashRoot>>,
    mut marks: Query<
        &mut Node,
        (
            With<SplashMark>,
            Without<SplashWordClip>,
            Without<SplashWordText>,
        ),
    >,
    mut clips: Query<
        &mut Node,
        (
            With<SplashWordClip>,
            Without<SplashMark>,
            Without<SplashWordText>,
        ),
    >,
    mut words: Query<
        (&mut Node, &mut TextColor),
        (
            With<SplashWordText>,
            Without<SplashMark>,
            Without<SplashWordClip>,
            Without<SplashFineText>,
        ),
    >,
    mut fines: Query<&mut TextColor, (With<SplashFineText>, Without<SplashWordText>)>,
) {
    let Some(mut state) = state else {
        return;
    };

    // Wait until the window and GPU pipelines have finished initial startup
    // frames so the Möbius trace and audio begin in lockstep once visible.
    if !state.started {
        state.warmup_frames = state.warmup_frames.saturating_add(1);
        if state.warmup_frames < 3 || time.delta_secs() > 0.1 {
            return;
        }
        state.started = true;
        if let (Some(clip), Some(added), Some(now)) =
            (state.clip.take(), added.as_deref_mut(), now.as_deref_mut())
        {
            added.0.push((SOUND_NAME.into(), clip));
            now.0.push(SOUND_NAME.into());
        }
    }

    // Any skip input starts a fast 0.22s fade-out rather than a hard cut.
    let skip_pressed = keys.as_deref().is_some_and(|keys| {
        keys.just_pressed(KeyCode::Escape)
            || keys.just_pressed(KeyCode::Space)
            || keys.just_pressed(KeyCode::Enter)
    }) || mouse
        .as_deref()
        .is_some_and(|mouse| mouse.just_pressed(MouseButton::Left))
        || gamepads.iter().any(|pad| {
            pad.just_pressed(GamepadButton::South)
                || pad.just_pressed(GamepadButton::East)
                || pad.just_pressed(GamepadButton::Start)
        });
    if skip_pressed && state.skip_at.is_none() && state.elapsed > 0.12 {
        state.skip_at = Some(state.elapsed);
    }

    // Clamp huge first-frame dt spikes so the trace always starts smoothly.
    state.elapsed += time.delta_secs().min(0.05);
    let t = state.elapsed;

    let fade_out = match state.skip_at {
        Some(at) => 1.0 - ease_in_out_cubic(clamp01((t - at) / SKIP_FADE)),
        None => 1.0 - ease_in_out_cubic(clamp01((t - FADE_START) / FADE_DURATION)),
    };

    if fade_out <= 0.001 || t >= DURATION {
        commands.entity(state.root).despawn();
        commands.entity(state.camera).despawn();
        commands.remove_resource::<SplashState>();
        showing.0 = false;
        return;
    }

    // Update backdrop alpha for smooth dissolve straight into the live menu.
    if let Ok(mut bg) = roots.get_mut(state.root) {
        bg.0 = Color::srgba(GROUND_RGB[0], GROUND_RGB[1], GROUND_RGB[2], fade_out);
    }

    let (win_w, win_h) = windows
        .iter()
        .next()
        .map_or((1280.0, 720.0), |w| (w.width(), w.height()));
    let mark_w = MARK_H * state.geometry.aspect;
    let total_w = mark_w + LOCKUP_GAP + WORDMARK_EST_W;
    let slide = ease_in_out_cubic(clamp01((t - 0.85) / 0.55));
    let rest_mark_cx = win_w * 0.5 - total_w * 0.5 + mark_w * 0.5;
    let mark_cx = win_w * 0.5 + (rest_mark_cx - win_w * 0.5) * slide;
    let mark_top = (win_h - MARK_H) * 0.5 - 8.0;

    if let Ok(mut node) = marks.single_mut() {
        node.left = Val::Px(mark_cx - mark_w * 0.5);
        node.top = Val::Px(mark_top);
        node.width = Val::Px(mark_w);
        node.height = Val::Px(MARK_H);
    }

    // Curtain-clipped wordmark reveal: clip edge tracks the moving right edge
    // of the Möbius strip so the text never overlaps a single pixel.
    let word_t = ease_out_cubic(clamp01((t - 0.92) / 0.52));
    let rest_text_x = rest_mark_cx + mark_w * 0.5 + LOCKUP_GAP;
    let clip_left = rest_text_x.max(mark_cx + mark_w * 0.5 + LOCKUP_GAP * 0.75);
    let clip_right = rest_text_x + WORDMARK_EST_W + 40.0;
    let clip_w = (clip_right - clip_left).max(0.0);

    if let Ok(mut clip) = clips.single_mut() {
        clip.left = Val::Px(clip_left);
        clip.top = Val::Px(mark_top);
        clip.width = Val::Px(clip_w);
        clip.height = Val::Px(MARK_H);
    }
    if let Ok((mut word_node, mut color)) = words.single_mut() {
        let offset_x = (1.0 - word_t) * -10.0;
        word_node.left = Val::Px(rest_text_x - clip_left + offset_x);
        color.0 = Color::srgba(INK_RGB[0], INK_RGB[1], INK_RGB[2], fade_out * word_t);
    }

    let fine_t = ease_out_cubic(clamp01((t - 1.25) / 0.50));
    if let Ok(mut color) = fines.single_mut() {
        color.0 = Color::srgba(
            INK_FAINT_RGB[0],
            INK_FAINT_RGB[1],
            INK_FAINT_RGB[2],
            fade_out * fine_t,
        );
    }

    // Rasterize the precision trace + ruling sweep into the mark's texture.
    let SplashState {
        geometry,
        coverage,
        image,
        ..
    } = &mut *state;
    if let Some(mut target) = images.get_mut(image) {
        rasterize_mark(geometry, coverage, &mut target, t, fade_out);
    }
}

/// Renders Concept A (Precision Trace: continuous boundary loop 0.0..0.85s,
/// 40 cross-rulings fanning in 0.22..1.05s) using analytical capsule SDF
/// anti-aliasing into a max-coverage buffer so junctions never double-brighten.
fn rasterize_mark(geo: &Geometry, coverage: &mut [u8], image: &mut Image, t: f32, alpha: f32) {
    coverage.fill(0);
    let pad = 12.0_f32;
    let scale = (TEX_H as f32 - pad * 2.0).min((TEX_W as f32 - pad * 2.0) / geo.aspect);
    let cx = TEX_W as f32 * 0.5;
    let cy = TEX_H as f32 * 0.5;
    let radius = (scale * 0.0155).max(1.8);

    // 1. Continuous outer boundary loop (0.0..0.85s).
    let loop_frac = ease_in_out_cubic(clamp01(t / 0.85));
    let target_len = loop_frac * geo.total_len;
    if target_len > 0.0 {
        for i in 1..geo.loop_pts.len() {
            let prev_len = geo.loop_len[i - 1];
            if prev_len >= target_len {
                break;
            }
            let p0 = geo.loop_pts[i - 1];
            let p1 = geo.loop_pts[i];
            let seg_len = geo.loop_len[i] - prev_len;
            let f = if geo.loop_len[i] <= target_len || seg_len <= 1e-6 {
                1.0
            } else {
                (target_len - prev_len) / seg_len
            };
            let x0 = cx + p0[0] * scale;
            let y0 = cy + p0[1] * scale;
            let x1 = cx + (p0[0] + (p1[0] - p0[0]) * f) * scale;
            let y1 = cy + (p0[1] + (p1[1] - p0[1]) * f) * scale;
            stamp_capsule(coverage, x0, y0, x1, y1, radius, 1.0);
        }
    }

    // 2. 40 cross-rulings fanning in sequentially around the ribbon (0.22..1.05s).
    let count = geo.rulings.len() as f32;
    for (i, &[x1, y1, x2, y2]) in geo.rulings.iter().enumerate() {
        let delay = 0.22 + (i as f32 / count) * 0.55;
        let p = ease_out_cubic(clamp01((t - delay) / 0.28));
        if p <= 0.0 {
            continue;
        }
        let ax = cx + x1 * scale;
        let ay = cy + y1 * scale;
        let bx = cx + (x1 + (x2 - x1) * p) * scale;
        let by = cy + (y1 + (y2 - y1) * p) * scale;
        stamp_capsule(coverage, ax, ay, bx, by, radius, p);
    }

    let Some(data) = image.data.as_mut() else {
        return;
    };
    for (i, &cov) in coverage.iter().enumerate() {
        let off = i * 4;
        let a = ((f32::from(cov) * alpha).round() as u32).min(255) as u8;
        data[off] = INK_BYTES[0];
        data[off + 1] = INK_BYTES[1];
        data[off + 2] = INK_BYTES[2];
        data[off + 3] = a;
    }
}

/// Stamps one anti-aliased round-cap segment into `coverage` using `max` blend.
fn stamp_capsule(
    coverage: &mut [u8],
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    radius: f32,
    strength: f32,
) {
    let margin = radius + 1.5;
    let min_x = ((x0.min(x1) - margin).floor() as i32).max(0) as u32;
    let max_x = ((x0.max(x1) + margin).ceil() as i32).min(TEX_W as i32 - 1) as u32;
    let min_y = ((y0.min(y1) - margin).floor() as i32).max(0) as u32;
    let max_y = ((y0.max(y1) + margin).ceil() as i32).min(TEX_H as i32 - 1) as u32;
    let dx = x1 - x0;
    let dy = y1 - y0;
    let len_sq = dx * dx + dy * dy;

    for y in min_y..=max_y {
        let py = y as f32 + 0.5;
        let row = (y * TEX_W) as usize;
        for x in min_x..=max_x {
            let px = x as f32 + 0.5;
            let t = if len_sq > 1e-6 {
                (((px - x0) * dx + (py - y0) * dy) / len_sq).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let dist = (px - (x0 + dx * t)).hypot(py - (y0 + dy * t));
            let aa = (radius + 0.75 - dist).clamp(0.0, 1.5) / 1.5;
            if aa > 0.0 {
                let val = (aa * strength * 255.0).round().clamp(0.0, 255.0) as u8;
                let slot = &mut coverage[row + x as usize];
                if val > *slot {
                    *slot = val;
                }
            }
        }
    }
}

/// Option 3 — Bowed Glass: pure, diffused harmonic bloom on D (`D3`, `A3`,
/// `F#4`, `A4`) with staggered silky swells, gentle stereo spread, and a
/// two-stage velvet-noise stereo hall diffuser (2,304 smooth reflections,
/// zero metallic comb ringing) that melts cleanly into the main menu music.
fn synth_splash_clip() -> Clip {
    const RATE: u32 = 24_000;
    const SECONDS: f32 = 3.15;
    let total = (RATE as f32 * SECONDS) as usize;
    let mut dry_l = vec![0.0_f32; total];
    let mut dry_r = vec![0.0_f32; total];
    let tau = std::f32::consts::TAU;

    // Pure harmonic series on D: (freq_hz, onset_s, gain, pan).
    let partials: [(f32, f32, f32, f32); 4] = [
        (146.83, 0.00, 0.42, -0.20), // D3
        (220.00, 0.12, 0.32, 0.25),  // A3
        (369.99, 0.35, 0.16, -0.40), // F#4
        (440.00, 0.55, 0.10, 0.40),  // A4
    ];

    for i in 0..total {
        let t = i as f32 / RATE as f32;
        // Master bloom envelope: smooth rise to 1.10s, graceful decay to 3.00s.
        let master = if t < 1.10 {
            ease_in_out_cubic(t / 1.10)
        } else {
            (1.0 - ease_in_out_cubic(clamp01((t - 1.10) / 1.90))).powf(1.35)
        };

        let mut l = 0.0_f32;
        let mut r = 0.0_f32;
        for (freq, onset, vol, pan) in partials {
            if t >= onset {
                let dt = t - onset;
                let rise = ease_in_out_cubic(clamp01(dt / 0.75));
                let fall = 1.0 - ease_in_out_cubic(clamp01((t - 1.15) / 1.85));
                let env = rise * fall * vol;
                let s = (tau * freq * dt).sin() * env;
                let gl = (0.5 - pan * 0.5).sqrt();
                let gr = (0.5 + pan * 0.5).sqrt();
                l += s * gl;
                r += s * gr;
            }
        }
        dry_l[i] = l * master * 0.22;
        dry_r[i] = r * master * 0.22;
    }

    // Two-stage cascaded velvet-noise stereo diffusion (smooth exponential
    // hall tail with zero metallic comb resonance).
    let wet_l = velvet_hall(&dry_l, RATE, 0xA1B2_C3D4);
    let wet_r = velvet_hall(&dry_r, RATE, 0x9E37_79B9);

    let mut frames = Vec::with_capacity(total);
    for i in 0..total {
        let t = i as f32 / RATE as f32;
        let tail = 1.0 - ease_in_out_cubic(clamp01((t - (SECONDS - 0.30)) / 0.30));
        let l = (dry_l[i] * 0.45 + wet_l[i] * 0.85) * tail;
        let r = (dry_r[i] * 0.45 + wet_r[i] * 0.85) * tail;
        frames.push([l.clamp(-0.95, 0.95), r.clamp(-0.95, 0.95)]);
    }

    Clip::from_stereo(RATE, frames)
}

/// Two-stage cascaded velvet-noise diffuser: 48 sparse taps × 48 sparse taps
/// produces 2,304 smooth, non-periodic reflections with warm HF damping.
fn velvet_hall(input: &[f32], rate: u32, mut seed: u32) -> Vec<f32> {
    let mut next_f32 = || -> f32 {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as f32 / u32::MAX as f32
    };

    let stage = |src: &[f32],
                 taps: usize,
                 span_s: f32,
                 decay: f32,
                 next: &mut dyn FnMut() -> f32|
     -> Vec<f32> {
        let n = src.len();
        let grid = (span_s * rate as f32 / taps as f32).max(1.0);
        let mut ir: Vec<(usize, f32)> = Vec::with_capacity(taps);
        let mut energy = 0.0_f32;
        for k in 0..taps {
            let pos = ((k as f32 + next()) * grid) as usize;
            let frac = k as f32 / taps as f32;
            let sign = if next() < 0.5 { -1.0 } else { 1.0 };
            let amp = sign * (1.0 - frac).powf(decay);
            energy += amp * amp;
            ir.push((pos, amp));
        }
        let norm = 1.0 / energy.sqrt().max(1e-6);
        let mut out = vec![0.0_f32; n];
        for (delay, amp) in ir {
            let g = amp * norm;
            for i in delay..n {
                out[i] += src[i - delay] * g;
            }
        }
        // Gentle warm lowpass damping on the diffused tail.
        let mut lp = 0.0_f32;
        for sample in &mut out {
            lp = lp * 0.68 + *sample * 0.32;
            *sample = lp;
        }
        out
    };

    let mid = stage(input, 48, 0.42, 1.8, &mut next_f32);
    stage(&mid, 48, 1.45, 2.6, &mut next_f32)
}

fn clamp01(t: f32) -> f32 {
    t.clamp(0.0, 1.0)
}

fn ease_out_cubic(t: f32) -> f32 {
    let inv = 1.0 - t;
    1.0 - inv * inv * inv
}

fn ease_in_out_cubic(t: f32) -> f32 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) * 0.5
    }
}
