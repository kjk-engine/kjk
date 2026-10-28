//! The screens' 3D views (`ui::vm::SceneView`): KOTOR draws a model in some
//! controls (the main menu's turning model, the character page's party
//! member, the upgrade bench's weapon). Each view the screens draw gets an
//! image the control's size on screen (`ui_view` draws the control's quad
//! from it) and, once its models are at hand, a camera of its own that
//! draws them into it, on a render layer nothing else is on.
//!
//! - The main menu: the campaign's `mainmenu` model looping `default`, seen
//!   from its `camerahook` node, 1.4 high each way (as reone frames the main menu).
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
//! - Character creation's class: the first player portrait's body for it
//!   (`portraits.2da`, reone's columns) in plain clothes, standing; and the
//!   character being made, whole on its sheet and its head on the portrait
//!   picker (`made_figure`).
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
/// The main menu model's half height: it is framed 1.4 high each way from
/// its `camerahook`. Our chosen size for the menu.
const MENU_SCALE: f32 = 1.4;
/// The character sheet's stage.
const STAGE: &str = "charrec_light";
/// What the sheet shows of the figure: z from −0.04 to 1.85 on the plane of
/// its root, seen in perspective from the stage's `camerahook` (0.73 below
/// the floor, so from low down). Measured off KOTOR's own sheet with a
/// 1.6 m scoundrel (reone's orthographic 2 m, lifted 1.7, showed more).
const STAGE_WINDOW: (f32, f32) = (-0.04, 1.85);
/// Seconds the stage's emitters run before it shows.
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

/// The main menu's model, read on the task pool.
fn main_menu(resolver: Resolver, game: Game) -> Task<Result<Ready, String>> {
    AsyncComputeTaskPool::get().spawn(async move {
        let ir = viewer::model(&resolver, "mainmenu")?.ok_or("mainmenu.mdl: not found")?;
        let mut cache = BTreeMap::new();
        let clip = viewer::clip(&mut cache, &resolver, game, &ir, "default")?;
        let models = vec![Placed {
            clip,
            ..Placed::at(ir, [0.0; 3])
        }];
        let eye = node(&models, hooks::CAMERA_HOOK).ok_or("mainmenu.mdl: no camerahook")?;
        let (textures, _) = viewer::textures(&resolver, &models);
        Ok(Ready {
            models,
            textures,
            frame: Frame {
                eye,
                half: MENU_SCALE,
                lens: None,
            },
            stage: None,
        })
    })
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
        let stage = (!lights.is_empty()).then(|| Stage {
            emitters: Vec::new(),
            textures: BTreeMap::new(),
            lights,
        });
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
            .find(&template, fmt_odyssey::ResType::UTI)
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

/// Character creation's figure of `class` and gender, read on the task
/// pool: the first player portrait's body for it, in plain clothes.
fn class_figure(
    resolver: Resolver,
    game: Game,
    (class, female): (u8, bool),
    aspect: f32,
) -> Task<Result<Ready, String>> {
    AsyncComputeTaskPool::get().spawn(async move {
        let portraits = Portraits::read(&resolver).ok_or("portraits.2da: not read")?;
        let appearance = portraits
            .first(female, class)
            .ok_or(format!("no player portrait for class {class}"))?;
        let mut models = Vec::new();
        // Facing +Y, as a model faces at rest.
        clothed(
            (&resolver, game),
            appearance,
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
            (u32::from(appearance), body, None),
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
        let portraits = Portraits::read(&resolver).ok_or("portraits.2da: not read")?;
        let persona = portraits
            .persona(sim::Seat(0), &portrait, class, "")
            .ok_or(format!("{portrait}: not a player portrait"))?;
        let (stage, hook, half) = match (head, persona.female()) {
            (false, _) => ("cgbody_light", hooks::CAMERA_HOOK, 1.1),
            (true, false) => ("cghead_light", "camerahookm", 0.2),
            (true, true) => ("cghead_light", "camerahookf", 0.2),
        };
        let stage = viewer::model(&resolver, stage)?.ok_or(format!("{stage}.mdl: not found"))?;
        let eye = stage
            .node(hook)
            .map(|node| to_mat4(&node.world))
            .ok_or(format!("{}.mdl: no {hook}", stage.name))?;
        // Facing the camera (a bearing from +X).
        let toward = eye.z_axis;
        let facing = toward.y.atan2(toward.x);
        let mut models = Vec::new();
        clothed(
            (&resolver, game),
            persona.appearance,
            ([0.0; 3], facing),
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
            stage: (!lights.is_empty()).then(|| Stage {
                emitters: Vec::new(),
                textures: BTreeMap::new(),
                lights,
            }),
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
/// from the stage's hook through `STAGE_WINDOW`; lit by the stage's lights.
fn staged(
    resolver: Resolver,
    game: Game,
    mut ready: Ready,
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
            .node(hooks::CAMERA_HOOK)
            .ok_or(format!("{STAGE}.mdl: no camerahook"))?;
        let eye = to_mat4(&hook.world);
        let seen = |z: f32| eye.inverse().transform_point3(Vec3::new(0.0, 0.0, z));
        let (bottom, top) = (seen(STAGE_WINDOW.0), seen(STAGE_WINDOW.1));
        ready.frame = Frame {
            eye,
            half: 1.0,
            lens: Some(Lens {
                distance: -bottom.z,
                bottom: bottom.y,
                top: top.y,
            }),
        };
        let pose = pose(alignment);
        let clip = stage.animations.iter().find(|clip| clip.name == pose);
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
            .filter_map(|name| Some((name.clone(), viewer::texture(&resolver, &name)?)))
            .collect();
        let lights = stage
            .lights
            .iter()
            .map(|light| posed_light(&stage, light, clip))
            .collect();
        ready.stage = Some(Stage {
            emitters,
            textures,
            lights,
        });
        Ok(ready)
    })
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

/// Each view the screens draw this frame, and its size in window pixels.
fn drawn(ui: &Ui, window: Vec2, scale: f32) -> BTreeMap<u32, UVec2> {
    let mut sizes = BTreeMap::new();
    for layer in &ui.layers {
        let (fitted, _) = layer.placed((window.x, window.y));
        for quad in layer
            .quads
            .iter()
            .filter(|quad| quad.texture.0 >= TextureId::SCENES)
        {
            let [_, _, width, height] = quad.rect;
            let pixels = Vec2::new(width, height) * fitted * scale;
            sizes.insert(quad.texture.0, pixels.round().as_uvec2().max(UVec2::ONE));
        }
    }
    sizes
}

/// What a party member's view is made from in play: the scene, the
/// presented snapshot, this player's body model and seat.
type InPlay<'w> = (
    Option<Res<'w, SceneContent>>,
    Option<Res<'w, PresentedSnapshot>>,
    Option<Res<'w, crate::play_view::OwnBody>>,
    Option<Res<'w, OwnSeat>>,
);

/// The views the screens draw: sized, read, spawned; the others put away.
fn want(
    mut commands: Commands,
    mut scenes: ResMut<GuiScenes>,
    ui: Option<Res<Ui>>,
    window: Query<&Camera, With<UiComposite>>,
    mut assets: ModelAssets,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    (content, presented, body, own): InPlay,
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
    for (id, live) in &mut scenes.views {
        let Some(size) = wanted.get(id) else {
            if let State::Shown { camera, root } = live.state {
                commands.entity(camera).despawn();
                commands.entity(root).despawn();
            }
            if !matches!(live.state, State::Failed) {
                live.state = State::Idle;
            }
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
                    State::Loading(item(gui.resolver().clone(), template.clone(), aspect))
                }
                SceneView::Dressed { appearance, armour } => State::Loading(creature_figure(
                    gui.resolver().clone(),
                    gui.game,
                    (*appearance, armour.clone()),
                    aspect,
                )),
                SceneView::Class { class, female } => State::Loading(class_figure(
                    gui.resolver().clone(),
                    gui.game,
                    (*class, *female),
                    aspect,
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
                    match ready {
                        Some(ready) => State::Loading(staged(
                            gui.resolver().clone(),
                            gui.game,
                            ready,
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
                Ok(ready) => spawn(&mut commands, live, ready, (&mut assets, &mut buffers)),
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
fn spawn(
    commands: &mut Commands,
    live: &Live,
    ready: Ready,
    (assets, buffers): (&mut ModelAssets, &mut Assets<ShaderBuffer>),
) -> State {
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
    // A staged figure is lit by its stage's lights alone (reone's GUI scenes
    // have no ambient light); the other views flat.
    let ambient = if stage.is_some() { [0.0; 3] } else { [1.0; 3] };
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
        let emitters = crate::emitter_view::spawn(
            commands,
            root,
            stage.emitters,
            &stage.textures,
            (images, stores.meshes, stores.materials),
            eye,
            STAGE_WARM,
        );
        commands.entity(root).insert(emitters);
        let lights = buffers.add(ShaderBuffer::from(LightList::of(stage.lights)));
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
