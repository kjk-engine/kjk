//! The screens' 3D views (`ui::vm::SceneView`): KOTOR draws a model in some
//! controls (the main menu's turning model, the character page's party
//! member, the upgrade bench's weapon). Each view the screens draw gets an
//! image the control's size on screen (`ui_view` draws the control's quad
//! from it) and, once its models are at hand, a camera of its own that
//! draws them into it, on a render layer nothing else is on.
//!
//! - The main menu: the model `menu_figure` picks (Malak, the story's Sith
//!   Lord, the player's character on an empty stage) looping `default`,
//!   with its smoke and mist, seen from its `camerahook` through the lens
//!   the games' 3D screens share (`STAGE_FOV`), lit by its own lights.
//! - A party member (the character sheet): its body and what hangs from it
//!   (head, weapons) as the scene drew them, its standing clip, on the
//!   sheet's stage `charrec_light` as reone builds it:
//!   at the stage's root facing its `camerahook`, seen from the hook 2
//!   high and lifted 1.7, the control's aspect wide. Behind it the stage's
//!   emitters in the pose of its alignment (`emitter_view`), a clip each 5
//!   points: dark's red fog and sparks, neutral's grey cloud, light's blue
//!   gas, streaks and stars.
//! - An item: its model (`baseitems.2da` `itemclass`, its variation), read
//!   off the frame; seen across its longest side.
//! - A party selection's companion: its `appearance.2da` body dressed by the
//!   item in its body slot, as a creature in the world is
//!   (`CreatureLoad::place_appearance`), whole, standing at rest.
//! - Character creation's class tiles: the body the tile's player portrait
//!   gives its class (`portraits.2da`, reone's columns) in plain clothes,
//!   idling on KOTOR 2's class selection stage with its smoke and lights
//!   (`class_figure`); and the character being made, whole on its sheet and
//!   its head on the portrait picker (`made_figure`).
//! - A model by name (the galaxy map's planet): its `default` clip, seen
//!   from its `camerahook` (else its front), as tall as it is drawn, lit by
//!   its own lights (a planet's aurora light).
//!
//! A view no screen draws any more is despawned; its image stays for the
//! id (the screens' materials keep it), cleared when shown again.

use std::collections::BTreeMap;

use asset_odyssey::emitter::{EmitterIr, Params};
use asset_odyssey::hooks;
use asset_odyssey::model::Matrix;
use asset_odyssey::texture::{Blending, TextureIr};
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{
    ClearColorConfig, OrthographicProjection, PerspectiveProjection, Projection, RenderTarget,
    ScalingMode, SubCameraView,
};
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureFormat};
use bevy::render::storage::ShaderBuffer;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use fmt_odyssey::mdl;
use ui::draw::TextureId;
use ui::vm::SceneView;
use vfs::Game;
use vfs::resolver::Resolver;

use crate::lifecycle_view::OverlayCamera;
use crate::material::{AreaLight, GpuLight, LightList, OdysseyMaterial};
use crate::persona::Portraits;
use crate::scene::{ModelAssets, SceneContent, SceneModel, Spawner, bounds_where, to_mat4};
use crate::sim_host::{OwnSeat, PresentedSnapshot};
use crate::ui_composite::UiComposite;
use crate::ui_view::Ui;
use crate::viewer::{self, Placed};

/// The first render layer the views take, one each.
const FIRST_LAYER: usize = 16;
/// The character sheet's stage.
const STAGE: &str = "charrec_light";
/// The sheet's camera, as K1's executable has it: level at the stage's hook
/// raised by the figure's own `camerahook` (a human's 1.63 m puts the view's
/// middle at 0.9 m), seeing 22.726° from top to bottom.
const STAGE_FOV: f32 = 22.726;
/// How far a figure without a `camerahook` raises the camera: a human's.
const STAGE_RAISE: f32 = 1.634;
/// The members the sheet sees from a hook of their own, by tag (K1's
/// executable: T3-M4 lower down, HK-47 and Zaalbar from further off), the
/// lens the same; the others, and a stage without the hook, `camerahook`.
const STAGE_HOOKS: [(&str, &str); 3] = [
    ("t3m4", "camerahookt"),
    ("hk47", "camerahookh"),
    ("zaalbar", "camerahookz"),
];
/// Seconds the stage's emitters run before it shows, at least (a stage
/// whose particles live longer runs until they are all out).
const STAGE_WARM: f32 = 4.0;

pub struct GuiScenePlugin;

impl Plugin for GuiScenePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GuiScenes>()
            .add_systems(
                Update,
                (want, layer, crate::emitter_view::step)
                    .chain()
                    .after(crate::ui_view::Stepped),
            )
            .add_systems(PostUpdate, light_views);
    }
}

/// How a view's camera sees its models (game space): from `eye` (looking
/// along its -Z, its +Y up), an orthographic box `half` high each way, or
/// through `lens`.
#[derive(Clone, Copy)]
struct Frame {
    eye: Mat4,
    half: f32,
    lens: Option<Lens>,
}

/// A perspective view through a window: a plane `distance` ahead of the eye
/// and, on it, the window's `bottom` and `top` along the eye's up from its
/// axis (off the axis when the eye stands below the window); as wide as the
/// view's aspect.
#[derive(Clone, Copy)]
struct Lens {
    distance: f32,
    bottom: f32,
    top: f32,
}

impl Lens {
    /// The camera for an image `aspect` wide: the whole view symmetric about
    /// the axis, as tall as the window's farther edge, of which the window is
    /// the part shown (sizes as ratios, in thousandths of the window).
    fn camera(&self, aspect: f32) -> (PerspectiveProjection, SubCameraView) {
        let reach = self.top.abs().max(self.bottom.abs());
        let height = (self.top - self.bottom).max(1e-3);
        let unit = 1000.0 / height;
        let size = Vec2::new(1000.0 * aspect, 1000.0);
        let full = Vec2::new(2.0 * reach * unit * aspect, 2.0 * reach * unit);
        (
            PerspectiveProjection {
                fov: 2.0 * (reach / self.distance).atan(),
                near: 0.05,
                far: 50.0,
                ..default()
            },
            SubCameraView {
                full_size: full.round().as_uvec2(),
                offset: Vec2::new((full.x - size.x) / 2.0, (reach - self.top) * unit),
                size: size.round().as_uvec2(),
            },
        )
    }
}

/// A stage's emitters, posed, with their nodes' transforms, and their
/// textures; and its lights, posed, which light the figure.
struct Stage {
    emitters: Vec<(EmitterIr, Params, Matrix)>,
    textures: BTreeMap<String, TextureIr>,
    lights: Vec<GpuLight>,
    /// The light the view's moving meshes take from every side.
    ambient: [f32; 3],
}

impl Stage {
    /// A stage of `lights` alone; none without lights (the view is lit
    /// flat).
    fn lit(lights: Vec<GpuLight>) -> Option<Stage> {
        (!lights.is_empty()).then(|| Stage {
            emitters: Vec::new(),
            textures: BTreeMap::new(),
            lights,
            ambient: [0.0; 3],
        })
    }
}

/// A view's models and their textures, and how to see them.
struct Ready {
    models: Vec<Placed>,
    textures: BTreeMap<String, TextureIr>,
    frame: Frame,
    stage: Option<Stage>,
}

enum State {
    /// Not drawn now.
    Idle,
    Loading(Task<Result<Ready, String>>),
    Shown {
        camera: Entity,
        root: Entity,
    },
    /// Could not be read; not tried again.
    Failed,
}

/// A view: its image, its layer, what it is up to.
struct Live {
    image: Handle<Image>,
    layer: usize,
    state: State,
}

/// The views, by their texture id.
#[derive(Resource, Default)]
pub struct GuiScenes {
    views: BTreeMap<u32, Live>,
}

impl GuiScenes {
    /// View `id`'s image (made the first time it is asked for).
    pub fn image(&mut self, id: TextureId, images: &mut Assets<Image>) -> Handle<Image> {
        let layer = FIRST_LAYER + self.views.len();
        self.views
            .entry(id.0)
            .or_insert_with(|| Live {
                image: images.add(target(1, 1)),
                layer,
                state: State::Idle,
            })
            .image
            .clone()
    }
}

/// A view's image keeps what its shaders output, the stored (encoded)
/// values, as the screens' image does: the screens read it as they read
/// any texture.
fn target(width: u32, height: u32) -> Image {
    Image::new_target_texture(width, height, TextureFormat::Rgba8Unorm, None)
}

/// A node's rest transform in its model (game space), by name.
fn node(models: &[Placed], name: &str) -> Option<Mat4> {
    let placed = models.first()?;
    let node = placed.ir.node(name)?;
    Some(to_mat4(&node.world))
}

/// The box the solid meshes of `models` fill: drawn (their texture at
/// hand), not glow (additive: a saber's blade planes), not hung from a hand
/// (a held weapon sits at its hand only once posed).
fn drawn_bounds(models: &[Placed], textures: &BTreeMap<String, TextureIr>) -> (Vec3, Vec3) {
    let unheld = models.iter().filter(|placed| placed.hook_local.is_none());
    bounds_where(unheld, |mesh| {
        mesh.texture
            .as_ref()
            .and_then(|texture| textures.get(texture))
            .is_some_and(|texture| texture.facts.blending != Blending::Additive)
    })
    .unwrap_or((Vec3::NEG_ONE, Vec3::ONE))
}

/// Seen from `from` (a direction, game space) with `up`, the box `models`
/// fill as drawn, with a margin.
fn framed(
    (models, textures): (&[Placed], &BTreeMap<String, TextureIr>),
    from: Vec3,
    up: Vec3,
    aspect: f32,
) -> Frame {
    let (min, max) = drawn_bounds(models, textures);
    let centre = (min + max) / 2.0;
    let size = max - min;
    let side = up.cross(from).normalize();
    let tall = size.dot(up.abs());
    let wide = size.dot(side.abs());
    let half = (tall / 2.0).max(wide / 2.0 / aspect.max(0.1)) * 1.1;
    let eye = Transform::from_translation(centre + from * (size.length() + 1.0))
        .looking_at(centre, up)
        .to_matrix();
    Frame {
        eye,
        half: half.max(0.05),
        lens: None,
    }
}

/// Seen from `eye` through the games' lens for their 3D screens,
/// `STAGE_FOV` from top to bottom (KOTOR 2's main menu too: a capture of
/// its 4:3 menu has its lines at layout rows 126 and 468, as ours).
fn through_lens(eye: Mat4) -> Frame {
    let reach = (STAGE_FOV.to_radians() / 2.0).tan();
    Frame {
        eye,
        half: 1.0,
        lens: Some(Lens {
            distance: 1.0,
            bottom: -reach,
            top: reach,
        }),
    }
}

/// `stage` lighting a whole 3D screen (its room is the screen): its lights
/// that only brighten the ambient become the view's ambient, whole, and the
/// others reach every mesh of `models`, where the world's reach only the
/// meshes whose origin they are near (the figure's stands on the floor,
/// out of the key light's radius).
fn lighting_all(stage: &mut Stage, models: &[Placed]) {
    let (ambient, lights): (Vec<GpuLight>, Vec<GpuLight>) = std::mem::take(&mut stage.lights)
        .into_iter()
        .partition(|light| light.kind.x != 0);
    stage.ambient = ambient
        .iter()
        .fold(Vec3::ZERO, |sum, light| {
            sum + light.color_multiplier.truncate() * light.color_multiplier.w
        })
        .min(Vec3::ONE)
        .to_array();
    stage.lights = lights;
    reaching_all(&mut stage.lights, models);
}

/// `lights` reaching every mesh of `models`.
fn reaching_all(lights: &mut [GpuLight], models: &[Placed]) {
    let origins: Vec<Vec3> = models
        .iter()
        .flat_map(|placed| {
            let at = Mat4::from_translation(Vec3::from_array(placed.offset));
            placed.ir.meshes.iter().map(move |mesh| {
                at.transform_point3(to_mat4(&placed.ir.nodes[mesh.node].world).w_axis.truncate())
            })
        })
        .collect();
    for light in lights {
        let at = light.position_radius.truncate();
        let farthest = origins
            .iter()
            .map(|origin| origin.distance(at))
            .fold(0.0, f32::max);
        // Past the last tenth of its radius, where the world's fade begins.
        light.position_radius.w = light.position_radius.w.max(farthest / 0.9);
    }
}

/// The main menu's model (`menu_figure`: Malak, the story's Sith Lord, the
/// player's character on the empty stage), looping its `default` clip, with
/// its own smoke and mist and lit by its own lights, read on the task pool.
fn main_menu(resolver: Resolver, game: Game) -> Task<Result<Ready, String>> {
    AsyncComputeTaskPool::get().spawn(async move {
        let figure = crate::menu_figure::of(game);
        let name = &figure.model;
        info!(target: "ui", model = name, exile = figure.exile.is_some(), "the main menu's figure");
        let ir = viewer::model(&resolver, name)?.ok_or(format!("{name}.mdl: not found"))?;
        let mut cache = BTreeMap::new();
        let clip = viewer::clip(&mut cache, &resolver, game, &ir, "default")?;
        let mut models = vec![Placed {
            clip,
            ..Placed::at(ir, [0.0; 3])
        }];
        let eye = node(&models, hooks::CAMERA_HOOK).ok_or(format!("{name}.mdl: no camerahook"))?;
        if let Some(exile) = &figure.exile {
            stand_exile((&resolver, game), exile, &mut models, &mut cache)?;
        }
        let (textures, _) = viewer::textures(&resolver, &models);
        let menu = &models[0].ir;
        let default = menu.animations.iter().find(|clip| clip.name == "default");
        let mut stage = stage_of(&resolver, menu, default);
        lighting_all(&mut stage, &models);
        Ok(Ready {
            models,
            textures,
            frame: through_lens(eye),
            stage: Some(stage),
        })
    })
}

/// The player's character on the menu's empty stage: at its
/// `cutscenedummy` (else its middle), facing −Y, body and head in the
/// dark side's stance (`evil`), as KOTOR 2 stands its new Sith Lord.
fn stand_exile(
    (resolver, game): (&Resolver, Game),
    exile: &crate::menu_figure::Exile,
    models: &mut Vec<Placed>,
    cache: &mut BTreeMap<String, Option<asset_odyssey::model::ModelIr>>,
) -> Result<(), String> {
    let at = node(models, "cutscenedummy")
        .map_or(Vec3::ZERO, |dummy| dummy.transform_point3(Vec3::ZERO));
    let first = models.len();
    placed_body(
        (resolver, game),
        (exile.appearance, exile.armour.as_deref()),
        (at.to_array(), -std::f32::consts::FRAC_PI_2),
        models,
    )?;
    for model in &mut models[first..] {
        if let Some(evil) = viewer::clip(cache, resolver, game, &model.ir, "evil")? {
            model.clip = Some(evil);
        }
    }
    Ok(())
}

/// A model by name (the galaxy map's planet), looping its `default` clip
/// where it has one, seen from its `camerahook` as the main menu's is (a
/// planet's looks down its halo's axis), else from its front, lit by its own
/// lights where it has some; read on the task pool.
fn model(resolver: Resolver, game: Game, name: String, aspect: f32) -> Task<Result<Ready, String>> {
    AsyncComputeTaskPool::get().spawn(async move {
        let ir = viewer::model(&resolver, &name)?.ok_or(format!("{name}.mdl: not found"))?;
        let mut cache = BTreeMap::new();
        let clip = viewer::clip(&mut cache, &resolver, game, &ir, "default")?;
        let models = vec![Placed {
            clip,
            ..Placed::at(ir, [0.0; 3])
        }];
        let (textures, _) = viewer::textures(&resolver, &models);
        let front = framed((&models, &textures), Vec3::NEG_Y, Vec3::Z, aspect);
        let frame = match node(&models, hooks::CAMERA_HOOK) {
            Some(eye) => Frame { eye, ..front },
            None => front,
        };
        // Lit by its own lights (a planet's aurora light), as a stage is.
        let ir = &models[0].ir;
        let lights: Vec<GpuLight> = ir
            .lights
            .iter()
            .map(|light| posed_light(ir, light, None))
            .collect();
        let stage = Stage::lit(lights);
        Ok(Ready {
            models,
            textures,
            frame,
            stage,
        })
    })
}

/// An item template's model, read on the task pool.
fn item(resolver: Resolver, template: String, aspect: f32) -> Task<Result<Ready, String>> {
    AsyncComputeTaskPool::get().spawn(async move {
        use fmt_odyssey::gff::Gff;
        let bytes = resolver
            .find(fmt_odyssey::resref_of(&template), fmt_odyssey::ResType::UTI)
            .ok_or(format!("{template}.uti: not found"))?
            .location
            .read()
            .map_err(|error| error.to_string())?;
        let uti = Gff::parse(&bytes).map_err(|error| format!("{template}.uti: {error}"))?;
        let number = |label: &str| uti.root.int(label).unwrap_or(0);
        let source = viewer::ResolverSource(&resolver);
        let name = asset_odyssey::creature::weapon_model(
            &source,
            number("BaseItem"),
            number("ModelVariation").max(1),
        )
        .ok_or(format!("{template}: no model"))?;
        let ir = viewer::model(&resolver, &name)?.ok_or(format!("{name}.mdl: not found"))?;
        let models = vec![Placed::at(ir, [0.0; 3])];
        let (textures, _) = viewer::textures(&resolver, &models);
        // Across its longest side, its shortest towards the camera.
        let (min, max) = drawn_bounds(&models, &textures);
        let size = max - min;
        let axes = [Vec3::X, Vec3::Y, Vec3::Z];
        let by = |axis: &Vec3| size.dot(*axis);
        let from = *axes
            .iter()
            .min_by(|a, b| by(a).total_cmp(&by(b)))
            .unwrap_or(&Vec3::Y);
        let long = *axes
            .iter()
            .max_by(|a, b| by(a).total_cmp(&by(b)))
            .unwrap_or(&Vec3::Z);
        // A tall control stands the item up; a wide one lays it along.
        let up = if aspect < 1.0 {
            long
        } else {
            from.cross(long).normalize()
        };
        let frame = framed((&models, &textures), from, up, aspect);
        Ok(Ready {
            models,
            textures,
            frame,
            stage: None,
        })
    })
}

/// A stage model, read, and the transform of its node `hook` (game space).
fn hooked_stage(
    resolver: &Resolver,
    name: &str,
    hook: &str,
) -> Result<(asset_odyssey::model::ModelIr, Mat4), String> {
    let stage = viewer::model(resolver, name)?.ok_or(format!("{name}.mdl: not found"))?;
    let eye = stage
        .node(hook)
        .map(|node| to_mat4(&node.world))
        .ok_or(format!("{name}.mdl: no {hook}"))?;
    Ok((stage, eye))
}

/// The bearing from +X that turns a figure at a stage's root to the camera
/// at `eye`.
fn facing(eye: Mat4) -> f32 {
    let toward = eye.z_axis;
    toward.y.atan2(toward.x)
}

/// The persona player portrait `portrait` gives `class`.
fn persona_of(
    resolver: &Resolver,
    portrait: &str,
    class: u8,
) -> Result<sim::persona::Persona, String> {
    let portraits = Portraits::read(resolver).ok_or("portraits.2da: not read")?;
    portraits
        .persona(sim::Seat(0), portrait, class, "")
        .ok_or(format!("{portrait}: not a player portrait"))
}

/// The stage of KOTOR 2's class selection: each class tile's figure stands
/// on it, seen from its `camerahook`.
const CLASS_STAGE: &str = "cgbody_light";
/// The idles a class tile's figure plays. KOTOR 2's executable picks one
/// of the two at random.
const CLASS_IDLES: [&str; 2] = ["pause1", "pause2"];

/// Character creation's class tile, read on the task pool, as KOTOR 2's
/// class selection builds it. The body that player portrait `portrait`
/// gives `class` stands in plain clothes at the root of `cgbody_light`,
/// turned to its `camerahook`, and idles in `pause1` or `pause2`. The view
/// looks from the hook through the games' lens (`STAGE_FOV`). The stage's
/// two smoke emitters stand behind the figure, and its lights light it,
/// both in the stage's `default` pose.
fn class_figure(
    resolver: Resolver,
    game: Game,
    (portrait, class): (String, u8),
) -> Task<Result<Ready, String>> {
    AsyncComputeTaskPool::get().spawn(async move {
        let persona = persona_of(&resolver, &portrait, class)?;
        let (stage, eye) = hooked_stage(&resolver, CLASS_STAGE, hooks::CAMERA_HOOK)?;
        let mut models = Vec::new();
        clothed(
            (&resolver, game),
            persona.appearance,
            ([0.0; 3], facing(eye)),
            &mut models,
        )?;
        // The body already plays `pause1` (`CreatureLoad::place_appearance`).
        // In microseconds: the clocks of macOS and Windows count in steps
        // of 1000 and 100 nanoseconds, so a count of nanoseconds is always
        // even there.
        let micros = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.subsec_micros());
        let idle = CLASS_IDLES[micros as usize % CLASS_IDLES.len()];
        if idle != CLASS_IDLES[0] {
            let mut cache = BTreeMap::new();
            for model in &mut models {
                if let Some(clip) = viewer::clip(&mut cache, &resolver, game, &model.ir, idle)? {
                    model.clip = Some(clip);
                }
            }
        }
        let (textures, _) = viewer::textures(&resolver, &models);
        let default = stage.animations.iter().find(|clip| clip.name == "default");
        let mut lit = stage_of(&resolver, &stage, default);
        lighting_all(&mut lit, &models);
        Ok(Ready {
            models,
            textures,
            frame: through_lens(eye),
            stage: Some(lit),
        })
    })
}

/// A creature of `appearance.2da` row `appearance` dressed in `armour` (its
/// body slot's item; none: nothing) as the world dresses one, facing +Y as
/// a model does at rest, framed whole, read on the task pool (the party
/// selection's companion).
fn creature_figure(
    resolver: Resolver,
    game: Game,
    (appearance, armour): (u16, Option<String>),
    aspect: f32,
) -> Task<Result<Ready, String>> {
    AsyncComputeTaskPool::get().spawn(async move {
        let mut models = Vec::new();
        placed_body(
            (&resolver, game),
            (appearance, armour.as_deref()),
            ([0.0; 3], std::f32::consts::FRAC_PI_2),
            &mut models,
        )?;
        let (textures, _) = viewer::textures(&resolver, &models);
        let frame = framed((&models, &textures), Vec3::Y, Vec3::Z, aspect);
        Ok(Ready {
            models,
            textures,
            frame,
            stage: None,
        })
    })
}

/// A body of `appearance.2da` row `appearance` in plain clothes
/// (`g_a_clothes01`, as character creation shows it), placed so.
fn clothed(
    (resolver, game): (&Resolver, Game),
    appearance: u16,
    at: ([f32; 3], f32),
    models: &mut Vec<Placed>,
) -> Result<(), String> {
    placed_body(
        (resolver, game),
        (appearance, Some("g_a_clothes01")),
        at,
        models,
    )
}

/// A body of `appearance.2da` row `appearance` in `body` (an armour's
/// template; none: the creature's own), placed so.
fn placed_body(
    (resolver, game): (&Resolver, Game),
    (appearance, body): (u16, Option<&str>),
    at: ([f32; 3], f32),
    models: &mut Vec<Placed>,
) -> Result<(), String> {
    crate::viewer::CreatureLoad::default()
        .place_appearance(
            (resolver, game),
            (u32::from(appearance), body, None, None),
            at,
            models,
        )?
        .ok_or(format!("appearance {appearance}: not drawn"))?;
    Ok(())
}

/// The character being made (as reone's character creation does), read on the task pool: the body portrait
/// `portrait` gives `class`, in plain clothes, turned to the camera, on
/// `cgbody_light` seen from its `camerahook` 1.1 high each way; or its head
/// on `cghead_light` from `camerahookm` (`camerahookf` for a woman) 0.2
/// high, the body lowered by its own `camerahook`'s height. Lit by the
/// stage's lights.
fn made_figure(
    resolver: Resolver,
    game: Game,
    (portrait, class, head): (String, u8, bool),
) -> Task<Result<Ready, String>> {
    AsyncComputeTaskPool::get().spawn(async move {
        let persona = persona_of(&resolver, &portrait, class)?;
        let (stage, hook, half) = match (head, persona.female()) {
            (false, _) => (CLASS_STAGE, hooks::CAMERA_HOOK, 1.1),
            (true, false) => ("cghead_light", "camerahookm", 0.2),
            (true, true) => ("cghead_light", "camerahookf", 0.2),
        };
        let (stage, eye) = hooked_stage(&resolver, stage, hook)?;
        let mut models = Vec::new();
        clothed(
            (&resolver, game),
            persona.appearance,
            ([0.0; 3], facing(eye)),
            &mut models,
        )?;
        if head {
            let height = node(&models, hooks::CAMERA_HOOK).map_or(0.0, |hook| hook.w_axis.z);
            models[0].offset[2] -= height;
        }
        let lights: Vec<GpuLight> = stage
            .lights
            .iter()
            .map(|light| posed_light(&stage, light, None))
            .collect();
        let (textures, _) = viewer::textures(&resolver, &models);
        Ok(Ready {
            models,
            textures,
            frame: Frame {
                eye,
                half,
                lens: None,
            },
            stage: Stage::lit(lights),
        })
    })
}

/// The stage's pose for an alignment (0 dark to 100 light): `evil` below
/// 5, then a clip each 5 points (`align1` to `align19`), `good` at 100.
fn pose(alignment: u8) -> String {
    match alignment.min(100) / 5 {
        0 => "evil".into(),
        20 => "good".into(),
        n => format!("align{n}"),
    }
}

/// The figure's clip for an alignment: its side's (`good`, `neutral`,
/// `evil`, in the humanoid bodies' chain beside `pause1`), as the sheet
/// stands a light-sider looking up and a dark-sider brooding.
fn stance(alignment: u8) -> &'static str {
    use sim::combat::sheet::Side;
    match sim::combat::sheet::side(alignment) {
        Side::Light => "good",
        Side::Neutral => "neutral",
        Side::Dark => "evil",
    }
}

/// `ready` on the character sheet's stage in `alignment`'s pose, read on
/// the task pool: the body and the head both play the side's clip (`stance`;
/// else the body stands in the clip the scene gave it, or `pause1`); seen
/// from the stage's hook (`own_hook`, the member's own where the stage has
/// it) raised to the figure's `camerahook` (`STAGE_FOV`); lit by the stage's
/// lights.
fn staged(
    resolver: Resolver,
    game: Game,
    (mut ready, own_hook): (Ready, &'static str),
    alignment: u8,
) -> Task<Result<Ready, String>> {
    AsyncComputeTaskPool::get().spawn(async move {
        let mut cache = BTreeMap::new();
        for (at, model) in ready.models.iter_mut().enumerate() {
            let side = viewer::clip(&mut cache, &resolver, game, &model.ir, stance(alignment))?;
            model.clip = match side {
                Some(clip) => Some(clip),
                None if at == 0 && model.clip.is_none() => {
                    viewer::clip(&mut cache, &resolver, game, &model.ir, "pause1")?
                }
                None => model.clip.take(),
            };
        }
        let stage = viewer::model(&resolver, STAGE)?.ok_or(format!("{STAGE}.mdl: not found"))?;
        let hook = stage
            .node(own_hook)
            .or_else(|| stage.node(hooks::CAMERA_HOOK))
            .ok_or(format!("{STAGE}.mdl: no camerahook"))?;
        let at = |world: &Matrix| to_mat4(world).transform_point3(Vec3::ZERO);
        let raised = (ready.models.first())
            .and_then(|body| body.ir.node(hooks::CAMERA_HOOK))
            .map_or(STAGE_RAISE, |own| at(&own.world).z);
        let eye = Mat4::from_translation(Vec3::Z * raised) * to_mat4(&hook.world);
        ready.frame = through_lens(eye);
        let pose = pose(alignment);
        let clip = stage.animations.iter().find(|clip| clip.name == pose);
        ready.stage = Some(stage_of(&resolver, &stage, clip));
        Ok(ready)
    })
}

/// `stage`'s emitters and lights in the pose of `clip` (its rows set an
/// emitter's parameters and a light's colour, radius and multiplier), and
/// the emitters' textures.
fn stage_of(
    resolver: &Resolver,
    stage: &asset_odyssey::model::ModelIr,
    clip: Option<&asset_odyssey::anim::AnimationIr>,
) -> Stage {
    let emitters: Vec<(EmitterIr, Params, Matrix)> = stage
        .emitters
        .iter()
        .map(|emitter| {
            let mut params = emitter.params;
            let set = clip.and_then(|clip| {
                clip.settings
                    .iter()
                    .find(|set| set.node.eq_ignore_ascii_case(&emitter.name))
            });
            for (kind, values) in set.map_or(&[][..], |set| &set.keys) {
                params.set(*kind, values);
            }
            (emitter.clone(), params, stage.nodes[emitter.node].world)
        })
        .collect();
    let textures = emitters
        .iter()
        .filter_map(|(emitter, ..)| emitter.texture.clone())
        .filter_map(|name| Some((name.clone(), viewer::texture(resolver, &name)?)))
        .collect();
    let lights = stage
        .lights
        .iter()
        .map(|light| posed_light(stage, light, clip))
        .collect();
    Stage {
        emitters,
        textures,
        lights,
        ambient: [0.0; 3],
    }
}

/// A stage light as its pose sets it (the clip's rows replace its colour,
/// radius and multiplier, and move it), in the stage's space.
fn posed_light(
    stage: &asset_odyssey::model::ModelIr,
    light: &asset_odyssey::model::LightIr,
    clip: Option<&asset_odyssey::anim::AnimationIr>,
) -> GpuLight {
    let node = &stage.nodes[light.node];
    let name = |set: &str| set.eq_ignore_ascii_case(&node.name);
    let (mut colour, mut radius, mut multiplier) = (light.color, light.radius, light.multiplier);
    let set = clip.and_then(|clip| clip.settings.iter().find(|set| name(&set.node)));
    for (kind, values) in set.map_or(&[][..], |set| &set.keys) {
        match (*kind, values.as_slice()) {
            (mdl::CONTROLLER_LIGHT_COLOR, [r, g, b, ..]) => colour = [*r, *g, *b],
            (mdl::CONTROLLER_LIGHT_RADIUS, [value, ..]) => radius = *value,
            (mdl::CONTROLLER_LIGHT_MULTIPLIER, [value, ..]) => multiplier = *value,
            _ => {}
        }
    }
    // Where the pose puts it (its first keys, `anim::posed`).
    let world = clip.map_or(node.world, |clip| {
        let bound = clip.bind(stage);
        asset_odyssey::anim::posed(
            stage,
            (clip, &bound),
            stage.animation_scale,
            light.node,
            0.0,
        )
    });
    let at = to_mat4(&world).transform_point3(Vec3::ZERO);
    // The key light's multiplier is 50: summed channel by channel and
    // clamped, its dark-side red-orange (0.85, 0.18, 0) lit the figure
    // yellow. Its hue is kept, at most full strength (an assumption: how the
    // game takes such a light is not known; KOTOR 2's dark sheet is red-lit).
    let strength = Vec3::from_array(colour) * multiplier;
    let peak = strength.max_element();
    let (colour, multiplier) = if peak > 1.0 {
        (strength / peak, 1.0)
    } else {
        (Vec3::from_array(colour), multiplier)
    };
    GpuLight {
        position_radius: at.extend(radius),
        color_multiplier: colour.extend(multiplier),
        kind: UVec4::new(u32::from(light.ambient_only), light.dynamic_type, 0, 0),
    }
}

/// Party member `nth`'s models as the scene drew them: its body and what
/// hangs from it but weapons, their hooks counted among themselves, the
/// body first, facing the stage's camera (+X: reone's facing of −90°).
fn member(
    nth: usize,
    scene: &viewer::LoadedScene,
    snapshot: &sim::Snapshot,
    (own, body): (sim::Seat, Option<usize>),
) -> Option<Ready> {
    let party = crate::cast::party_of(snapshot, own);
    let entity = party.get(nth)?;
    let (first, pause) = match entity.kind {
        sim::Kind::Object(_, index) => {
            let view = scene
                .creature_views
                .iter()
                .find(|view| view.object == index)?;
            (view.body, view.clips.pause.clone())
        }
        _ => (body?, None),
    };
    // The body and its head: the sheet shows no weapons (reone dresses the
    // figure in its body's armour only), so nothing hung in a hand.
    let hung: Vec<usize> = std::iter::once(first)
        .chain((0..scene.models.len()).filter(|at| {
            let placed = &scene.models[*at];
            placed.hook.is_some_and(|(to, _)| to == first) && placed.hook_local.is_none()
        }))
        .collect();
    let models: Vec<Placed> = hung
        .iter()
        .map(|at| {
            let placed = &scene.models[*at];
            Placed {
                offset: [0.0; 3],
                facing: -std::f32::consts::FRAC_PI_2,
                hook: placed
                    .hook
                    .map(|(to, node)| (hung.iter().position(|at| *at == to).unwrap_or(0), node)),
                clip: if *at == first {
                    pause.clone().or(placed.clip.clone())
                } else {
                    placed.clip.clone()
                },
                ..placed.clone()
            }
        })
        .collect();
    let names: Vec<&String> = models
        .iter()
        .flat_map(|placed| &placed.ir.meshes)
        .flat_map(|mesh| [mesh.texture.as_ref(), mesh.lightmap.as_ref()])
        .flatten()
        .collect();
    let textures = scene
        .textures
        .iter()
        .filter(|(name, _)| names.contains(name))
        .map(|(name, texture)| (name.clone(), texture.clone()))
        .collect();
    Some(Ready {
        models,
        textures,
        // Seen from the stage's hook once the stage is read (`staged`).
        frame: Frame {
            eye: Mat4::IDENTITY,
            half: 1.0,
            lens: None,
        },
        stage: None,
    })
}

/// The sheet's hook for the member tagged `tag` (`STAGE_HOOKS`).
fn stage_hook(tag: &str) -> Option<&'static str> {
    STAGE_HOOKS
        .iter()
        .find(|(member, _)| tag.eq_ignore_ascii_case(member))
        .map(|(_, hook)| *hook)
}

/// Each view the screens draw this frame, and its image's size in window
/// pixels: its quad's, or the size the screen keeps it at
/// (`DrawList::sized`).
fn drawn(ui: &Ui, window: Vec2, scale: f32) -> BTreeMap<u32, UVec2> {
    let mut sizes = BTreeMap::new();
    for layer in &ui.layers {
        let (fitted, _) = layer.placed((window.x, window.y));
        for quad in layer
            .quads
            .iter()
            .filter(|quad| quad.texture.0 >= TextureId::SCENES)
        {
            let [width, height] = (layer.sized.iter())
                .find(|(texture, _)| *texture == quad.texture)
                .map_or([quad.rect[2], quad.rect[3]], |(_, size)| *size);
            // A wide view is the window's width (`DrawList::wide`).
            let width = if layer.wide.contains(&quad.texture) {
                window.x / fitted
            } else {
                width
            };
            let pixels = Vec2::new(width, height) * fitted * scale;
            sizes.insert(quad.texture.0, pixels.round().as_uvec2().max(UVec2::ONE));
        }
    }
    sizes
}

/// What a party member's view is made from in play: the scene, the
/// presented snapshot, this player's body model and seat, the level.
type InPlay<'w> = (
    Option<Res<'w, SceneContent>>,
    Option<Res<'w, PresentedSnapshot>>,
    Option<Res<'w, crate::play_view::OwnBody>>,
    Option<Res<'w, OwnSeat>>,
    Option<Res<'w, crate::sim_host::SceneSwap>>,
);

/// The views the screens draw: sized, read, spawned; the others put away.
fn want(
    mut commands: Commands,
    mut scenes: ResMut<GuiScenes>,
    ui: Option<Res<Ui>>,
    window: Query<&Camera, With<UiComposite>>,
    mut assets: ModelAssets,
    (content, presented, body, own, swap): InPlay,
) {
    let (Some(ui), Some(camera)) = (ui, window.iter().next()) else {
        return;
    };
    let (Some(logical), Some(physical)) = (
        camera.logical_viewport_size(),
        camera.physical_viewport_size(),
    ) else {
        return;
    };
    let wanted = drawn(&ui, logical, physical.x as f32 / logical.x.max(1.0));
    let Some(gui) = ui.assets.as_ref() else {
        return;
    };
    // A view put away: drawn again from its models when next wanted.
    let put_away = |commands: &mut Commands, live: &mut Live| {
        if let State::Shown { camera, root } = live.state {
            commands.entity(camera).despawn();
            commands.entity(root).despawn();
        }
        if !matches!(live.state, State::Failed) {
            live.state = State::Idle;
        }
    };
    for (id, live) in &mut scenes.views {
        let Some(size) = wanted.get(id) else {
            put_away(&mut commands, live);
            continue;
        };
        let aspect = size.x as f32 / size.y as f32;
        if let Some(mut image) = assets.split().0.get_mut(&live.image)
            && (image.width(), image.height()) != (size.x, size.y)
        {
            image.resize(Extent3d {
                width: size.x,
                height: size.y,
                depth_or_array_layers: 1,
            });
            // Its camera is made for the image's shape (a wide view's
            // follows the window's).
            put_away(&mut commands, live);
        }
        let Some(view) = gui.scene_of(TextureId(*id)) else {
            continue;
        };
        if let State::Idle = live.state {
            live.state = match view {
                SceneView::MainMenu => State::Loading(main_menu(gui.resolver().clone(), gui.game)),
                SceneView::Model(name) => State::Loading(model(
                    gui.resolver().clone(),
                    gui.game,
                    name.clone(),
                    aspect,
                )),
                SceneView::Item(template) => {
                    State::Loading(item(gui.item_resolver(template), template.clone(), aspect))
                }
                SceneView::Dressed { appearance, armour } => State::Loading(creature_figure(
                    gui.resolver().clone(),
                    gui.game,
                    (*appearance, armour.clone()),
                    aspect,
                )),
                SceneView::Class { portrait, class } => State::Loading(class_figure(
                    gui.resolver().clone(),
                    gui.game,
                    (portrait.clone(), *class),
                )),
                SceneView::Made {
                    portrait,
                    class,
                    head,
                } => State::Loading(made_figure(
                    gui.resolver().clone(),
                    gui.game,
                    (portrait.clone(), *class, *head),
                )),
                SceneView::Member { nth, alignment } => {
                    let ready = (|| {
                        let snapshot = presented.as_ref()?.0.current();
                        let body = body.as_ref().and_then(|body| body.0);
                        member(
                            *nth,
                            &content.as_ref()?.0,
                            snapshot,
                            (own.as_ref()?.0, body),
                        )
                    })();
                    let hook = (|| {
                        let snapshot = presented.as_ref()?.0.current();
                        let party = crate::cast::party_of(snapshot, own.as_ref()?.0);
                        let sim::Kind::Object(_, index) = party.get(*nth)?.kind else {
                            return None;
                        };
                        let instances =
                            sim::Instances::in_snapshot(&swap.as_ref()?.level, snapshot);
                        stage_hook(instances.tag(index)?)
                    })();
                    match ready {
                        Some(ready) => State::Loading(staged(
                            gui.resolver().clone(),
                            gui.game,
                            (ready, hook.unwrap_or(hooks::CAMERA_HOOK)),
                            *alignment,
                        )),
                        None => State::Failed,
                    }
                }
            };
        }
        if let State::Loading(task) = &mut live.state
            && let Some(read) = block_on(poll_once(task))
        {
            live.state = match read {
                Ok(ready) => spawn(&mut commands, live, ready, &mut assets),
                Err(why) => {
                    warn!(target: "ui", ?view, %why, "a 3D view is not drawn");
                    State::Failed
                }
            };
        }
    }
}

/// `ready`'s models under a root of their own on `live`'s layer, and a
/// camera drawing them into its image.
fn spawn(commands: &mut Commands, live: &Live, ready: Ready, assets: &mut ModelAssets) -> State {
    let Ready {
        models,
        textures,
        frame,
        stage,
    } = ready;
    let root = commands
        .spawn((
            Transform::IDENTITY,
            Visibility::default(),
            ViewRoot(live.layer),
        ))
        .id();
    let (images, mut stores) = assets.split();
    // A staged figure is lit by its stage's lights and ambient (none on
    // the sheet: reone's GUI scenes have no ambient light); the other views
    // flat.
    let ambient = stage.as_ref().map_or([1.0; 3], |stage| stage.ambient);
    let aspect = images.get(&live.image).map_or(1.0, |image| {
        image.width() as f32 / image.height().max(1) as f32
    });
    let mut spawner = Spawner::new(&textures, AreaLight::ambient(ambient), images);
    let mut nodes = Vec::new();
    for (index, placed) in models.iter().enumerate() {
        let (model, its) = spawner.placed(
            commands,
            &mut stores,
            (placed, usize::MAX - index, false),
            (root, &nodes, 0),
        );
        // Not the scene's: the views that find models by index pass it by.
        commands.entity(model).remove::<SceneModel>();
        nodes.push(its);
    }
    // A stage's backdrop fills the view, drawn on black: its particles add
    // light, which a clear view would not keep. Lights alone keep it clear.
    let clear = if let Some(stage) = stage {
        let backdrop = !stage.emitters.is_empty();
        let eye = (
            frame.eye.x_axis.truncate().normalize(),
            frame.eye.y_axis.truncate().normalize(),
        );
        // Run long enough that its longest-lived particles are all out (the
        // main menu's mist lives 10 s).
        let warm = (stage.emitters.iter())
            .map(|(_, params, _)| params.life)
            .fold(STAGE_WARM, f32::max);
        let emitters = crate::emitter_view::spawn(
            commands,
            root,
            stage.emitters,
            &stage.textures,
            (images, stores.meshes, stores.materials),
            eye,
            warm,
        );
        commands.entity(root).insert(emitters);
        let lights = stores
            .buffers
            .add(ShaderBuffer::from(LightList::of(stage.lights)));
        commands.entity(root).insert(ViewLights(lights));
        if backdrop { Color::BLACK } else { Color::NONE }
    } else {
        Color::NONE
    };
    let (projection, sub_camera_view) = match frame.lens {
        Some(lens) => {
            let (perspective, sub) = lens.camera(aspect);
            (Projection::Perspective(perspective), Some(sub))
        }
        None => (
            Projection::Orthographic(OrthographicProjection {
                scaling_mode: ScalingMode::FixedVertical {
                    viewport_height: frame.half * 2.0,
                },
                near: -50.0,
                far: 50.0,
                ..OrthographicProjection::default_3d()
            }),
            None,
        ),
    };
    let camera = commands
        .spawn((
            Camera3d::default(),
            Camera {
                order: -5,
                clear_color: ClearColorConfig::Custom(clear),
                sub_camera_view,
                ..default()
            },
            RenderTarget::Image(live.image.clone().into()),
            projection,
            Tonemapping::None,
            DebandDither::Disabled,
            Transform::from_matrix(frame.eye),
            RenderLayers::layer(live.layer),
            OverlayCamera,
        ))
        .id();
    State::Shown { camera, root }
}

/// A view's root: its meshes go on this layer.
#[derive(Component)]
struct ViewRoot(usize);

/// A view's own lights (its stage's), for its materials.
#[derive(Component)]
struct ViewLights(Handle<ShaderBuffer>);

/// Each lit view's materials given its lights rather than the area's.
fn light_views(
    roots: Query<(Entity, &ViewLights)>,
    children: Query<&Children>,
    surfaces: Query<&MeshMaterial3d<OdysseyMaterial>>,
    mut materials: ResMut<Assets<OdysseyMaterial>>,
) {
    for (root, ViewLights(lights)) in &roots {
        for entity in children.iter_descendants(root) {
            let Ok(handle) = surfaces.get(entity) else {
                continue;
            };
            let stale = materials
                .get(&handle.0)
                .is_some_and(|material| material.lights != *lights);
            if stale && let Some(mut material) = materials.get_mut(&handle.0) {
                material.lights = lights.clone();
            }
        }
    }
}

/// Each view's meshes on its layer, as they are spawned.
fn layer(
    mut commands: Commands,
    roots: Query<(Entity, &ViewRoot)>,
    children: Query<&Children>,
    meshes: Query<(), (With<Mesh3d>, Without<RenderLayers>)>,
) {
    for (root, ViewRoot(layer)) in &roots {
        for entity in children.iter_descendants(root) {
            if meshes.contains(entity) {
                commands.entity(entity).insert(RenderLayers::layer(*layer));
            }
        }
    }
}
