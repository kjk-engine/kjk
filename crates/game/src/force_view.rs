//! The force powers' effects in `kjk play`, drawn each frame from the
//! casters' channels and hands as JA's client draws them: lightning from
//! its effect files at the left hand, played back from every client frame
//! whose primitives may still live. A grip shows no puffs at the hand, and
//! a push or a pull shows no distortion in the air: JA draws both, and
//! kjk leaves them out because they look out of place in KOTOR.
//!
//! Everyone is drawn from the host's snapshots at the view's time, but a
//! joined player's own character, which shows from its predictions at its
//! own time (`shots::OwnFire`): its casts' effects are there at once
//! (QUIRKS.md Q-0071), where JA's cgame shows its own player's from the
//! snapshot's state, a round trip on.

use std::collections::BTreeMap;

use asset_ja::fx::{self, Rng};
use asset_odyssey::texture::TextureIr;
use bevy::prelude::*;
use collision::{Contents, Extents};
use console::DumpFacts;
use sim::EntityState;
use sim::combat::force::Power;
use sim::motion::Anim;

use crate::cast::Hitboxes;
use crate::lifecycle_view::SceneCamera;
use crate::model_fx::SprayStores;
use crate::scene::game_to_bevy;
use crate::scene_fx::{self, ShaderMeshes, View};
use crate::shots::OwnFire;
use crate::sim_host::{PresentedSnapshot, SceneSwap};
use crate::viewer::EffectBlend;
use crate::viewer_ja::Files;

const UNIT: f32 = asset_ja::UNIT;
/// JA's client frames, as the effects are timed (QUIRKS.md Q-0005).
const FRAME_MS: f32 = 1000.0 / 60.0;
/// A grip's puffs in the rig viewer (`force_fx`): how long each lives, how
/// fast it drifts (units a second, along the view's left), and its two
/// shaders.
pub(crate) const PUFF_MS: f32 = 120.0;
pub(crate) const PUFF_SPEED: f32 = 55.0;
pub(crate) const GRIP_SHADERS: [&str; 2] = ["gfx/effects/forcePush", "gfx/effects/sabers/red_glow"];
/// How long a lightning efx's primitives may live, at most (ms): how far
/// back its copies are played.
const LIGHTNING_BACK_MS: f32 = 250.0;
/// Lightning lights what is near in its own colour (its primitives' times
/// their shader's texture's, `glow_colour`), a fight's light that rooms take
/// too: at the hand and where its bolts end, this far (metres), this strong
/// with this many bolts alive, less with fewer. Ours: JA lights the caster.
const LIGHTNING_RADIUS: f32 = 4.5;
const LIGHTNING_STRENGTH: f32 = 1.2;
const LIGHTNING_FULL: f32 = 6.0;

/// The powers' lights as of this frame (lightning's), in Bevy's space:
/// candidates beside the scene's (`scene::pick_lights`).
#[derive(Resource, Default, PartialEq)]
pub(crate) struct ForceLights(pub Vec<crate::material::GpuLight>);

/// What an added texture lights with: its colour where it shows, each
/// texel weighted by how much it adds (its luma times its alpha), from its
/// smallest level; white for none.
fn glow_colour(texture: &TextureIr) -> Vec3 {
    let decoded = texture.decoded();
    let Some(level) = decoded.mips.last() else {
        return Vec3::ONE;
    };
    let (mut sum, mut weight) = (Vec3::ZERO, 0.0);
    for [r, g, b, a] in level.as_chunks::<4>().0.iter().copied() {
        let rgb = Vec3::new(r.into(), g.into(), b.into()) / 255.0;
        let w = rgb.dot(Vec3::new(0.299, 0.587, 0.114)) * f32::from(a) / 255.0;
        sum += rgb * w;
        weight += w;
    }
    if weight > f32::EPSILON {
        sum / weight
    } else {
        Vec3::ONE
    }
}

/// The effects' files, read from JA's.
pub struct Setup {
    /// Lightning at levels 1 and 2, and at 3.
    lightning: [fx::Effect; 2],
    textures: BTreeMap<String, TextureIr>,
    blends: BTreeMap<String, EffectBlend>,
}

/// Reads the powers' effects; what is missing goes to the notes.
pub fn load(files: &Files, notes: &mut Vec<String>) -> Setup {
    let lightning = [
        "effects/force/lightning.efx",
        "effects/force/lightningwide.efx",
    ]
    .map(|path| crate::viewer_ja::effect_file(files, path, notes));
    let mut shaders: Vec<String> = lightning
        .iter()
        .flat_map(|effect| &effect.templates)
        .flat_map(|template| &template.shaders)
        .map(|shader| shader.to_ascii_lowercase())
        .collect();
    shaders.sort();
    shaders.dedup();
    let (textures, missing) = crate::viewer_ja::textures(files, &shaders);
    if !missing.is_empty() {
        notes.push(format!(
            "force effects: no texture for {}",
            missing.join(", ")
        ));
    }
    let blends = crate::viewer_ja::effect_blends(files, shaders, notes);
    Setup {
        lightning,
        textures,
        blends,
    }
}

/// Its systems; a session comes in with `install`.
pub struct ForceViewPlugin;

/// A session's powers' effects, put into `app`: their meshes are made on
/// the next frame.
pub fn install(app: &mut World, setup: Setup) {
    app.insert_resource(Pending(Some(setup)));
}

/// The setup until the first frame builds the meshes.
#[derive(Resource)]
struct Pending(Option<Setup>);

/// The effects and the meshes they are drawn into.
#[derive(Resource)]
struct Drawn {
    lightning: [fx::Effect; 2],
    shaders: ShaderMeshes,
    /// Client frames drawn with lightning.
    frames: usize,
    /// Each shader's light colour (`glow_colour`), by lowercase name.
    glows: BTreeMap<String, Vec3>,
}

impl Plugin for ForceViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ForceLights>()
            .add_systems(PreUpdate, spawn.run_if(resource_exists::<Pending>))
            .add_systems(
                PostUpdate,
                // After the bodies are placed (the hands), before the
                // meshes' changes are sent on.
                draw.run_if(resource_exists::<Drawn>)
                    .after(bevy::transform::TransformSystems::Propagate)
                    .before(bevy::asset::AssetEventSystems),
            );
    }
}

/// A mesh and a material per shader; they outlive the scenes.
fn spawn(
    mut commands: Commands,
    mut pending: ResMut<Pending>,
    mut images: ResMut<Assets<Image>>,
    mut stores: SprayStores,
) {
    let Some(setup) = pending.0.take() else {
        return;
    };
    let shaders = ShaderMeshes::spawn(
        &mut commands,
        (&setup.blends, &setup.textures),
        &mut images,
        stores.assets(),
    );
    let glows = (setup.textures.iter())
        .map(|(shader, texture)| (shader.to_ascii_lowercase(), glow_colour(texture)))
        .collect();
    commands.insert_resource(Drawn {
        lightning: setup.lightning,
        shaders,
        frames: 0,
        glows,
    });
    commands.remove_resource::<Pending>();
}

/// A caster's lightning bolts alive this frame: where they end (units)
/// and their colours, summed.
#[derive(Default)]
struct Bolts {
    ends: Vec3,
    colour: Vec3,
    count: f32,
}

impl Bolts {
    fn add(&mut self, end: Vec3, colour: Vec3) {
        self.ends += end;
        self.colour += colour;
        self.count += 1.0;
    }

    /// Their lights (`LIGHTNING_*`): at `hand` and where they end on
    /// average (units), in their colour on average; none with none.
    fn lights(&self, hand: Vec3, to_bevy: impl Fn(Vec3) -> Vec3) -> Vec<crate::material::GpuLight> {
        use crate::material::{GpuLight, fight_kind};
        if self.count < 1.0 {
            return Vec::new();
        }
        let colour = self.colour / self.count;
        let strength = LIGHTNING_STRENGTH * (self.count / LIGHTNING_FULL).min(1.0);
        [hand, self.ends / self.count]
            .map(|at| GpuLight {
                position_radius: to_bevy(at).extend(LIGHTNING_RADIUS),
                color_multiplier: colour.extend(strength),
                kind: fight_kind(false),
                ..Default::default()
            })
            .to_vec()
    }
}

/// The client frames' times from `from` to `to` (ms).
fn frames(from: f32, to: f32) -> impl Iterator<Item = f32> {
    let first = (from / FRAME_MS).ceil() as i64;
    (first..)
        .map(|k| k as f32 * FRAME_MS)
        .take_while(move |&t| t <= to)
}

/// The grip's tint at the client's clock `t` (ms): a slow pulse of red.
pub(crate) fn grip_red(t: f32) -> u8 {
    let wave = (t * 0.004).sin() * 0.08 + 0.1;
    (200.0 + wave * 255.0).min(255.0) as u8
}

/// The two puffs a grip adds each time, in order: (speed along the view's
/// left in units a second, rotation in degrees, colour, shader). The first
/// drifts left tinted `red`, the second right, white and turned over.
pub(crate) fn grip_pair(red: u8) -> [(f32, f32, [u8; 3], &'static str); 2] {
    [
        (PUFF_SPEED, 0.0, [red, 0, 0], GRIP_SHADERS[0]),
        (-PUFF_SPEED, 180.0, [255, 255, 255], GRIP_SHADERS[1]),
    ]
}

#[allow(clippy::too_many_arguments)]
fn draw(
    presented: Res<PresentedSnapshot>,
    swap: Res<SceneSwap>,
    hitboxes: Res<Hitboxes>,
    fire: Res<OwnFire>,
    drawn: Option<ResMut<Drawn>>,
    places: Query<&GlobalTransform>,
    cameras: Query<(&GlobalTransform, &Projection, &Camera), SceneCamera>,
    mut stores: SprayStores,
    (mut lights, mut facts): (ResMut<ForceLights>, ResMut<DumpFacts>),
) {
    let Some(mut drawn) = drawn else { return };
    let mut lit = Vec::new();
    let drawn = &mut *drawn;
    let Ok((camera, projection, info)) = cameras.single() else {
        return;
    };
    let view = View::of(camera, projection, info);
    let (previous, current) = (presented.0.previous(), presented.0.current());
    let now = previous.time_ms as f64
        + (current.time_ms as f64 - previous.time_ms as f64) * f64::from(presented.0.alpha());
    let now = now as f32;
    // Everyone at the view's time, the own character at its own.
    let own = fire.own.zip(fire.shift);
    let mut casters: Vec<(&EntityState, f32)> = current
        .entities
        .iter()
        .filter(|entity| own.is_none_or(|(own, _)| own.id != entity.id))
        .map(|entity| (entity, now))
        .collect();
    if let Some((own, shift)) = &own {
        casters.push((own, now + *shift as f32));
    }
    let level = &swap.level;
    // The doors as they stand, found only when a bolt is traced.
    let doors = std::cell::OnceCell::new();
    let ends = |from: Vec3, to: Vec3| {
        let doors = doors.get_or_init(|| level.states_in(current));
        let hit = level.with_unremembered(doors.as_deref(), |world| {
            world.trace(
                (from * UNIT).to_array(),
                (to * UNIT).to_array(),
                &Extents::POINT,
                Contents::SOLID,
            )
        });
        from + (to - from) * hit.fraction
    };
    let to_game = game_to_bevy().inverse();
    let to_bevy = |p: Vec3| game_to_bevy() * (p * UNIT);
    let eye = view.eye();
    let mut batches = BTreeMap::new();
    for (entity, t) in casters {
        let (Some(fighter), Some(ps)) = (entity.fighter, entity.player) else {
            continue;
        };
        if !fighter.alive() {
            continue;
        }
        let Some(channel) = fighter
            .force
            .channel
            .filter(|c| c.power == Power::Lightning)
        else {
            continue;
        };
        let spot = |location: sim::combat::Location| {
            hitboxes
                .0
                .get(&entity.id)
                .and_then(|spots| spots[location as usize])
                .and_then(|node| places.get(node).ok())
                .map_or_else(
                    || Vec3::from_array(sim::combat::mind::chest(&ps)),
                    |place| to_game * place.translation() / UNIT,
                )
        };
        // A character with no weapon casts with both hands (JA's two-handed
        // lightning), and the bolts leave from each.
        let both = ps.torso.anim == Anim::BOTH_FORCE_2HANDEDLIGHTNING_HOLD;
        let hands = [
            sim::combat::Location::HandLeft,
            sim::combat::Location::HandRight,
        ];
        let hands = &hands[..if both { 2 } else { 1 }];
        let (forward, ..) = sim::math::angle_vectors(ps.view);
        let axis = fx::axis(Vec3::from_array(forward));
        let effect = &drawn.lightning[usize::from(channel.level > 2)];
        let from = channel.from as f32;
        for (side, &location) in hands.iter().enumerate() {
            let hand = spot(location);
            let mut bolts = Bolts::default();
            for start in frames(from.max(t - LIGHTNING_BACK_MS), t) {
                let seed = (entity.id.0 as i32) ^ (start / FRAME_MS) as i32;
                // The second hand's bolts are seeded apart from the first's.
                let mut rng = Rng(seed ^ ((side as i32) << 16));
                for spawned in fx::play(effect, hand, axis, start, &mut rng, &ends) {
                    if !spawned.alive(t) {
                        continue;
                    }
                    let shader = spawned.shader.to_ascii_lowercase();
                    if spawned.kind == fx::Kind::Electricity {
                        let glow = drawn.glows.get(&shader).copied().unwrap_or(Vec3::ONE);
                        bolts.add(spawned.origin2, spawned.rgb_start * glow);
                    }
                    if let Some(look) = drawn.shaders.look(&shader) {
                        let batch = batches.entry(shader).or_default();
                        let mut rng = Rng(spawned.seed ^ (t / FRAME_MS) as i32);
                        scene_fx::primitive(
                            batch,
                            &spawned,
                            look,
                            (t, FRAME_MS),
                            &mut rng,
                            &eye,
                            to_bevy,
                        );
                    }
                }
            }
            lit.extend(bolts.lights(hand, to_bevy));
        }
        drawn.frames += 1;
    }
    lights.set_if_neq(ForceLights(lit));
    drawn.shaders.fill(batches, stores.assets());
    if facts.wanted() {
        facts.0.insert(
            "force_fx".into(),
            serde_json::json!({
                "lightning_frames": drawn.frames,
                "lights": lights.0.iter().map(|light| {
                    let [r, g, b, m] = light.color_multiplier.to_array();
                    [r, g, b, m].map(|x| (x * 100.0).round() / 100.0)
                }).collect::<Vec<_>>(),
            }),
        );
    }
}
