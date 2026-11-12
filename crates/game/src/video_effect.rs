//! The video effect scripts enable (`EnableVideoEffect`,
//! `DisableVideoEffect`), or this player's body's effects give (TSL's
//! Force Sight and Fury; render side only: it reads the presented
//! snapshots' `sim::Snapshot::video_effect_for`, a `videoeffects.2da` row),
//! or the conversation's lines' cameras show (`CamVidEffect`, `LineEffect`,
//! over the others while the conversation shows one): a pass over the scene
//! camera's frame, after the scene and before the fade, the loading screen
//! and the screens over it.
//!
//! A row's columns: `enablesaturation` grades the frame, each pixel's
//! colour pulled toward its grey by `saturation` (0 grey, 1 as drawn) and
//! then multiplied by `modulationred`/`green`/`blue` (TSL's `_pc` columns;
//! its `_xbox` ones are the console's); `enablescannoise` adds film noise,
//! the game's procedural `filmnoisetex` (a 32 × 32 random texture whose TXI
//! scales each channel to 0.02), over it. The grey weighs the channels by
//! the weights TSL's executable carries (0.3086, 0.6094, 0.0820: a
//! saturation matrix), and the frame is graded in its stored (encoded)
//! values, as the game's framebuffer was. [verify in the game: the noise's
//! scale on the screen (here one texel a pixel, new each frame) and that it
//! is added after the grade.] TSL's Force views (`enableclairvoyance`,
//! `enableforcesight`, `enableclairvoyancefull`, `enablefury`: clairvoyance,
//! Force Sight, the freelooks of Visas and Kreia, Fury) are not drawn.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::core_pipeline::{Core3d, Core3dSystems, FullscreenShader};
use bevy::prelude::*;
use bevy::render::diagnostic::RecordDiagnostics;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::render_resource::binding_types::{sampler, texture_2d, uniform_buffer_sized};
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, BufferInitDescriptor,
    BufferUsages, CachedRenderPipelineId, ColorTargetState, ColorWrites, FilterMode, FragmentState,
    Operations, PipelineCache, RenderPassColorAttachment, RenderPassDescriptor,
    RenderPipelineDescriptor, Sampler, SamplerBindingType, SamplerDescriptor, ShaderStages,
    TextureFormat, TextureSampleType,
};
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::view::{ExtractedView, ViewTarget};
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};
use fmt_odyssey::twoda::TwoDa;

use crate::fade_view::{FadeCamera, FadeSet};
use crate::lifecycle::Installer;
use crate::lifecycle_view::SceneCamera;
use crate::sim_host::{PresentedSnapshot, SceneSwap};
use sim::combat::passive::Game;

const SHADER: Handle<Shader> = uuid_handle!("3c9e1f57-8b42-4d6a-a0f3-5e7b2c19d846");

/// How strong the film noise is on each channel (`filmnoisetex.txi`'s
/// `channelscale`).
const NOISE: f32 = 0.02;

/// A `videoeffects.2da` row as the pass draws it.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Row {
    /// The saturation (0 grey, 1 as drawn) and the modulation, where the
    /// row grades the frame.
    grade: Option<(f32, [f32; 3])>,
    noise: bool,
}

impl Row {
    fn read(table: &TwoDa, row: usize) -> Row {
        // TSL's PC columns carry `_pc`; K1's have no suffix.
        let float = |column: &str| {
            table
                .float(row, &format!("{column}_pc"))
                .or_else(|| table.float(row, column))
                .unwrap_or(0.0)
        };
        let on = |column: &str| table.int(row, column).is_some_and(|value| value != 0);
        Row {
            grade: on("enablesaturation").then(|| {
                (
                    float("saturation"),
                    ["modulationred", "modulationgreen", "modulationblue"].map(float),
                )
            }),
            noise: on("enablescannoise"),
        }
    }
}

/// The effect a conversation's lines' cameras show, line by line, as each
/// executable sets it when it puts a line's camera up
/// (`rules::dialogue::cameras::EVERY_CAMERA_EFFECT`): an animated camera's
/// line (`CameraAngle` 5) keeps what showed; in K1 a placed camera's line
/// (6) shows its `CamVidEffect` row (0 on) and keeps what showed on -1 or
/// -2, any other camera's line none; in TSL every other camera's line shows
/// its row, none on -1 or -2 (a script's effect then shows, as TSL's keeps
/// it). A conversation begins and ends with none.
#[derive(Default)]
struct LineEffect {
    /// The conversation and line last followed, and what it showed.
    line: Option<(u32, u32, u64)>,
    shown: Option<u16>,
}

/// A line shown: its conversation's id, its entry and when it showed.
type Line = (u32, u32, u64);

/// A line's `CameraAngle` of a placed camera.
const PLACED_CAMERA: u32 = 6;
/// A line's `CameraAngle` of an animated camera.
const ANIMATED_CAMERA: u32 = 5;

impl LineEffect {
    /// Follows the line `(conversation id, entry, shown ms)` with its
    /// `CamVidEffect` and `CameraAngle` (none: no conversation shown) in
    /// `game`; what it shows now.
    fn follow(&mut self, line: Option<(Line, (i32, u32))>, game: Game) -> Option<u16> {
        let Some((key, (effect, angle))) = line else {
            *self = LineEffect::default();
            return None;
        };
        if self.line.is_some_and(|(id, ..)| id != key.0) {
            self.shown = None;
        }
        if self.line != Some(key) {
            self.line = Some(key);
            let row = u16::try_from(effect).ok();
            let every =
                rules::dialogue::cameras::EVERY_CAMERA_EFFECT[usize::from(game == Game::Tsl)];
            self.shown = match angle {
                ANIMATED_CAMERA => self.shown,
                _ if every => row,
                PLACED_CAMERA => row.or(self.shown),
                _ => None,
            };
        }
        self.shown
    }
}

/// The lines' video effects' probes (`kjk sim probes --only "line video
/// effect"`): the rule, and on K1's data where this machine has the
/// install, the Endar Spire's Carth (`end_carth001`: his security-camera
/// lines noised, his line at the pod clear) and his call to Trask
/// (`end_trask01`: noised, Trask's answer clear).
#[cfg(feature = "probes")]
pub fn line_probes() -> sim::probe::Checks {
    use sim::probe::check;
    let lines = [
        (3, PLACED_CAMERA),
        (-1, PLACED_CAMERA),
        (-2, ANIMATED_CAMERA),
        (3, 1),
        (-1, 0),
        (1, PLACED_CAMERA),
    ];
    let mut effect = LineEffect::default();
    let shown: Vec<Option<u16>> = (0..)
        .zip(&lines)
        .map(|(entry, node)| effect.follow(Some(((7, entry, u64::from(entry)), *node)), Game::K1))
        .collect();
    let again = effect.follow(Some(((8, 0, 9), lines[1])), Game::K1);
    let mut checks = vec![(
        "a placed camera's row shows, -1 and an animated camera keep it, \
         another camera takes it off",
        check(
            shown == [Some(3), Some(3), Some(3), None, None, Some(1)] && again.is_none(),
            format!("{shown:?}; a new conversation's keep: {again:?}"),
        ),
    )];
    let tsl_lines = [
        (3, 1),
        (-1, PLACED_CAMERA),
        (2, PLACED_CAMERA),
        (-2, ANIMATED_CAMERA),
        (-1, 1),
    ];
    let mut effect = LineEffect::default();
    let tsl: Vec<Option<u16>> = (0..)
        .zip(&tsl_lines)
        .map(|(entry, node)| effect.follow(Some(((7, entry, u64::from(entry)), *node)), Game::Tsl))
        .collect();
    checks.push((
        "TSL: every camera's row shows but an animated one's, -1 and -2 take it off",
        check(
            tsl == [Some(3), None, Some(2), Some(2), None],
            format!("{tsl:?}"),
        ),
    ));
    let Some(index) = crate::k1_index() else {
        return checks;
    };
    // What `dlg` of `module` shows on its `entries` in turn.
    let followed = |module: &str, dlg: &str, entries: &[u32]| {
        let resolver = crate::k1_open(&index, Some(module)).ok()?;
        let bytes = resolver.read(dlg, fmt_odyssey::ResType::DLG)?;
        let dlg = fmt_odyssey::dlg::Dlg::parse(&bytes).ok()?;
        let mut effect = LineEffect::default();
        entries
            .iter()
            .map(|&entry| {
                let node = dlg.entries.get(entry as usize)?;
                Some(effect.follow(
                    Some((
                        (1, entry, u64::from(entry)),
                        (node.cam_vid_effect, node.camera_angle),
                    )),
                    Game::K1,
                ))
            })
            .collect::<Option<Vec<Option<u16>>>>()
    };
    if let Some(shown) = followed("end_m01ab", "end_carth001", &Vec::from_iter(0..12)) {
        checks.push((
            "the Endar Spire's Carth: the security camera's lines noised",
            check(
                shown[..3] == [None, None, None] && shown[3..].iter().all(|row| *row == Some(0)),
                format!("{shown:?}"),
            ),
        ));
    }
    if let Some(shown) = followed("end_m01aa", "end_trask01", &[32, 33, 34, 35]) {
        checks.push((
            "Carth's call to Trask noised, Trask's lines after it clear",
            check(shown == [Some(0), None, None, None], format!("{shown:?}")),
        ));
    }
    checks
}

/// The session's video effects: the install they are read from and the
/// table, once read (one for the install: it is global).
#[derive(Resource)]
struct VideoEffects {
    installer: Option<Arc<Installer>>,
    rows: Option<Vec<Row>>,
}

/// A session's video effects, their table read from `installer`.
pub fn install(world: &mut World, installer: Option<Arc<Installer>>) {
    world.insert_resource(VideoEffects {
        installer,
        rows: None,
    });
}

/// On the last camera drawing the world (`grade`) while a row grades or noises its frame: the
/// modulation (xyz) and saturation (w), the noise's strength (x) and the
/// frame's seed (y).
#[derive(Component, ExtractComponent, Clone, Copy, PartialEq)]
pub struct VideoGrade {
    tint: Vec4,
    noise: Vec4,
}

pub struct VideoEffectPlugin;

impl Plugin for VideoEffectPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "video_effect.wgsl", Shader::from_wgsl);
        app.add_plugins(ExtractComponentPlugin::<VideoGrade>::default())
            .add_systems(
                Update,
                grade.after(FadeSet).run_if(
                    resource_exists::<PresentedSnapshot>.and_then(resource_exists::<VideoEffects>),
                ),
            );
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .add_systems(RenderStartup, init)
            .add_systems(Render, prepare.in_set(RenderSystems::Prepare))
            .add_systems(
                Core3d,
                pass.in_set(Core3dSystems::PostProcess)
                    .in_set(VideoEffectPass),
            );
    }
}

/// The pass grading the world's frame. The screens' composite runs after
/// it: both post-process the same camera when the scene's hosts the
/// composite, and the render schedule's executor ran the two in either
/// order frame to frame. Run out of order, about one frame in seven lost the
/// screens and showed the graded world bare: the security camera's
/// letterbox and the terminals flickered.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct VideoEffectPass;

/// Puts the presented row's grade on the last camera drawing the world, the
/// fade's while it draws (a grade on the scene's camera is lost under it, and
/// the frame shows its natural colours through the fade), or takes it off.
#[allow(clippy::too_many_arguments)]
fn grade(
    presented: Res<PresentedSnapshot>,
    own: Option<Res<crate::sim_host::OwnSeat>>,
    swap: Option<Res<SceneSwap>>,
    watching: Option<Res<crate::dialogue_view::Watching>>,
    mut effects: ResMut<VideoEffects>,
    cameras: Query<(Entity, Option<&VideoGrade>), SceneCamera>,
    faders: Query<(Entity, &Camera, Option<&VideoGrade>), With<FadeCamera>>,
    mut facts: ResMut<console::DumpFacts>,
    (mut frame, mut lines): (Local<u32>, Local<LineEffect>),
    mut commands: Commands,
) {
    let seat = own.map_or(sim::Seat(0), |own| own.0);
    let snapshot = presented.0.current();
    // The line's camera's, else this player's body's own (TSL's Force
    // Sight and Fury) before the scripts'.
    let talk = crate::dialogue_view::shown(snapshot, seat, watching.and_then(|w| w.0));
    let line = talk.zip(swap.as_ref()).and_then(|(talk, swap)| {
        let dlg = swap.level.scripts()?.dialogue(&talk.dialog)?;
        let node = dlg.entries.get(talk.entry as usize)?;
        Some((
            (talk.id, talk.entry, talk.shown_ms),
            (node.cam_vid_effect, node.camera_angle),
        ))
    });
    let game = swap
        .as_ref()
        .map_or_else(Game::default, |swap| swap.level.game());
    let current = lines
        .follow(line, game)
        .or_else(|| snapshot.video_effect_for(seat));
    facts
        .0
        .insert("video_effect".into(), serde_json::json!(current));
    if effects.rows.is_none() && current.is_some() {
        let module = swap.as_ref().map(|swap| swap.level.resources().to_string());
        effects.rows = effects
            .installer
            .as_ref()
            .zip(module)
            .and_then(|(installer, module)| installer.resolver(&module).ok())
            .and_then(|resolver| resolver.two_da("videoeffects"))
            .map(|table| {
                (0..table.cells.len())
                    .map(|row| Row::read(&table, row))
                    .collect()
            });
    }
    let row = current.and_then(|row| effects.rows.as_ref()?.get(usize::from(row)).copied());
    let Some(scene) = cameras.iter().next() else {
        return;
    };
    let fader = faders.iter().next();
    let (camera, had) = match fader.filter(|(_, camera, _)| camera.is_active) {
        Some((fader, _, had)) => {
            if scene.1.is_some() {
                commands.entity(scene.0).remove::<VideoGrade>();
            }
            (fader, had)
        }
        None => {
            if let Some((fader, _, Some(_))) = fader {
                commands.entity(fader).remove::<VideoGrade>();
            }
            scene
        }
    };
    let wanted = row
        .filter(|row| row.grade.is_some() || row.noise)
        .map(|row| {
            let (saturation, [r, g, b]) = row.grade.unwrap_or((1.0, [1.0; 3]));
            let noise = if row.noise {
                *frame = (*frame + 1) % 4096;
                Vec4::new(NOISE, *frame as f32, 0.0, 0.0)
            } else {
                Vec4::ZERO
            };
            VideoGrade {
                tint: Vec4::new(r, g, b, saturation),
                noise,
            }
        });
    if facts.wanted() {
        facts.0.insert(
            "video_grade".into(),
            serde_json::json!({
                "rows": effects.rows.as_ref().map(Vec::len),
                "cameras": cameras.iter().count(),
                "wanted": wanted.is_some(),
                "had": had.is_some(),
            }),
        );
    }
    match (wanted, had) {
        (Some(wanted), had) if had != Some(&wanted) => {
            commands.entity(camera).insert(wanted);
        }
        (None, Some(_)) => {
            commands.entity(camera).remove::<VideoGrade>();
        }
        _ => {}
    }
}

/// The pass's bindings (the frame, a sampler, the grade) and its pipeline
/// for each target format.
#[derive(Resource)]
struct Pipeline {
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    fullscreen: FullscreenShader,
    ids: HashMap<TextureFormat, CachedRenderPipelineId>,
}

#[derive(Component)]
struct PipelineId(CachedRenderPipelineId);

fn init(mut commands: Commands, device: Res<RenderDevice>, fullscreen: Res<FullscreenShader>) {
    let layout = BindGroupLayoutDescriptor::new(
        "video_effect",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                uniform_buffer_sized(false, None),
            ),
        ),
    );
    let sampler = device.create_sampler(&SamplerDescriptor {
        mag_filter: FilterMode::Nearest,
        min_filter: FilterMode::Nearest,
        ..SamplerDescriptor::default()
    });
    commands.insert_resource(Pipeline {
        layout,
        sampler,
        fullscreen: fullscreen.clone(),
        ids: HashMap::new(),
    });
}

fn prepare(
    mut commands: Commands,
    cache: Res<PipelineCache>,
    mut pipeline: ResMut<Pipeline>,
    views: Query<(Entity, &ExtractedView), With<VideoGrade>>,
) {
    let pipeline = &mut *pipeline;
    for (entity, view) in &views {
        let format = view.target_format;
        let id = *pipeline.ids.entry(format).or_insert_with(|| {
            cache.queue_render_pipeline(RenderPipelineDescriptor {
                label: Some("video_effect".into()),
                layout: vec![pipeline.layout.clone()],
                vertex: pipeline.fullscreen.to_vertex_state(),
                fragment: Some(FragmentState {
                    shader: SHADER,
                    targets: vec![Some(ColorTargetState {
                        format,
                        blend: None,
                        write_mask: ColorWrites::ALL,
                    })],
                    ..default()
                }),
                ..default()
            })
        });
        commands.entity(entity).insert(PipelineId(id));
    }
}

fn pass(
    view: ViewQuery<(&ViewTarget, &VideoGrade, &PipelineId)>,
    pipeline: Res<Pipeline>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    mut ctx: RenderContext,
) {
    let (target, grade, id) = view.into_inner();
    let Some(render) = cache.get_render_pipeline(id.0) else {
        return;
    };
    let post = target.post_process_write();
    let settings = device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("video_effect"),
        contents: &[grade.tint.to_array(), grade.noise.to_array()]
            .concat()
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect::<Vec<u8>>(),
        usage: BufferUsages::UNIFORM,
    });
    let bindings = device.create_bind_group(
        "video_effect",
        &cache.get_bind_group_layout(&pipeline.layout),
        &BindGroupEntries::sequential((
            post.source,
            &pipeline.sampler,
            settings.as_entire_binding(),
        )),
    );
    // The pass's time on the GPU, where a run measures it
    // (`console::gpu_times`).
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let time_span = diagnostics.time_span(ctx.command_encoder(), "video_effect");
    {
        let mut pass = ctx
            .command_encoder()
            .begin_render_pass(&RenderPassDescriptor {
                label: Some("video_effect"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: post.destination,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations::default(),
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        pass.set_pipeline(render);
        pass.set_bind_group(0, &bindings, &[]);
        pass.draw(0..3, 0..1);
    }
    time_span.end(ctx.command_encoder());
}
