//! A model's emitters drawn (`asset_odyssey::emitter`): each emitter's
//! particles are stepped every frame and laid out as quads into one mesh
//! per emitter, textured with its flipbook's frame, tinted and faded by the
//! particle's colour and alpha, and blended as the emitter says (`lighten`
//! adds by alpha, `normal` blends).
//!
//! Only the screens' 3D views draw emitters so far (the character sheet's
//! alignment backdrop, `gui_scene`); the area scene does not yet.

use std::collections::BTreeMap;

use asset_odyssey::emitter::{Blend, EmitterIr, Look, Params, Particle, Render, Stream, look};
use asset_odyssey::model::Matrix;
use asset_odyssey::texture::TextureIr;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::prelude::*;

use crate::material::{AreaLight, Lighting, OdysseyMaterial, Surface};
use crate::scene_fx::Batch;

/// One emitter being drawn: its kind, its numbers now, its particles and
/// the mesh they go into.
pub(crate) struct Drawn {
    emitter: EmitterIr,
    params: Params,
    /// The emitter node's transform in the model (game space).
    to_model: Matrix,
    stream: Stream,
    mesh: Handle<Mesh>,
}

/// A model's emitters, under a parent whose meshes they are.
#[derive(Component)]
pub(crate) struct Emitters {
    drawn: Vec<Drawn>,
    /// The eye's right and up (game space), for the quads that face it.
    eye: (Vec3, Vec3),
}

/// The emitters of a model at rest (`nodes`' transforms), their numbers as
/// `params` gives them, spawned as meshes under `parent`; those that make
/// no particles are left out. `seconds` runs them on first, so a backdrop
/// shows full when it opens.
#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn(
    commands: &mut Commands,
    parent: Entity,
    emitters: impl IntoIterator<Item = (EmitterIr, Params, Matrix)>,
    textures: &BTreeMap<String, TextureIr>,
    (images, meshes, materials): (
        &mut Assets<Image>,
        &mut Assets<Mesh>,
        &mut Assets<OdysseyMaterial>,
    ),
    eye: (Vec3, Vec3),
    seconds: f32,
) -> Emitters {
    let mut images_by_name: BTreeMap<String, Handle<Image>> = BTreeMap::new();
    let mut drawn = Vec::new();
    // Drawn by their render order, then as the model lists them
    // (`OdysseyMaterial::emitted`).
    let emitters: Vec<_> = emitters.into_iter().collect();
    let mut order: Vec<usize> = (0..emitters.len()).collect();
    order.sort_by_key(|at| (emitters[*at].0.render_order, *at));
    let mut ranks = vec![0u32; emitters.len()];
    for (rank, at) in order.into_iter().enumerate() {
        ranks[at] = rank as u32;
    }
    for (index, (emitter, params, to_model)) in emitters.into_iter().enumerate() {
        let rank = ranks[index];
        if params.birthrate + params.random_birthrate <= 0.0 || params.life == 0.0 {
            continue;
        }
        let texture = emitter.texture.as_deref().and_then(|name| {
            let texture = textures.get(name)?;
            Some(
                images_by_name
                    .entry(name.to_string())
                    .or_insert_with(|| images.add(crate::scene::image(texture)))
                    .clone(),
            )
        });
        let surface = match emitter.blend {
            Blend::Lighten => Surface::Lighten,
            Blend::Normal | Blend::PunchThrough => Surface::BlendVertex,
        };
        let material = materials.add(
            OdysseyMaterial::new(
                Vec4::ONE,
                texture,
                None,
                None,
                surface,
                &Lighting {
                    self_illum: [0.0; 3],
                    ambient: [1.0; 3],
                    diffuse: [1.0; 3],
                    alpha: 1.0,
                    is_static: false,
                    is_room: true,
                    fogged: false,
                },
                &AreaLight::ambient([1.0; 3]),
            )
            .emitted(rank),
        );
        let mesh = meshes.add(Batch::default().into_mesh());
        let child = commands
            .spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material),
                Transform::IDENTITY,
                NoFrustumCulling,
            ))
            .id();
        commands.entity(parent).add_child(child);
        let mut stream = Stream::new(0x9e37_79b9 ^ (index as u32 + 1).wrapping_mul(0x85eb_ca6b));
        // Run on in steps a frame long, as the screen would have.
        let step = 1.0 / 30.0;
        for _ in 0..(seconds / step) as usize {
            stream.step(&params, emitter.update, &to_model, step);
        }
        drawn.push(Drawn {
            emitter,
            params,
            to_model,
            stream,
            mesh,
        });
    }
    Emitters { drawn, eye }
}

/// Every model's emitters a frame on, into their meshes.
pub(crate) fn step(
    time: Res<Time>,
    mut all: Query<&mut Emitters>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let dt = time.delta_secs().min(0.1);
    for mut emitters in &mut all {
        let eye = emitters.eye;
        for drawn in &mut emitters.drawn {
            drawn
                .stream
                .step(&drawn.params, drawn.emitter.update, &drawn.to_model, dt);
            let mut batch = Batch::default();
            for particle in &drawn.stream.particles {
                quad(&mut batch, drawn, particle, eye);
            }
            if let Some(mut mesh) = meshes.get_mut(&drawn.mesh) {
                *mesh = batch.into_mesh();
            }
        }
    }
}

/// How much drawing a view's particles take, measured the same way on any
/// GPU: the quads drawn
/// and the pixels of a `size` image they cover, each counted as often as it
/// is drawn over (`clip_from_world` the view's camera).
#[derive(Default, Clone, Copy)]
pub(crate) struct Coverage {
    pub quads: usize,
    pub pixels: f64,
}

impl Emitters {
    /// The quads in the emitters' meshes now, and the pixels they cover.
    pub(crate) fn coverage(
        &self,
        meshes: &Assets<Mesh>,
        clip_from_world: Mat4,
        size: Vec2,
    ) -> Coverage {
        let mut coverage = Coverage::default();
        for drawn in &self.drawn {
            let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) = meshes
                .get(&drawn.mesh)
                .and_then(|mesh| mesh.attribute(Mesh::ATTRIBUTE_POSITION))
            else {
                continue;
            };
            for corners in positions.as_chunks::<4>().0 {
                let pixels = covered(corners, clip_from_world, size);
                // An empty mesh keeps one quad of nothing (`scene_fx::quads`).
                if pixels > 0.0 {
                    coverage.quads += 1;
                    coverage.pixels += pixels;
                }
            }
        }
        coverage
    }
}

/// The pixels of a `size` image a quad covers, cut to the image and to what
/// lies in front of the eye.
fn covered(corners: &[[f32; 3]; 4], clip_from_world: Mat4, size: Vec2) -> f64 {
    let clip: Vec<Vec4> = corners
        .iter()
        .map(|corner| clip_from_world * Vec3::from_array(*corner).extend(1.0))
        .collect();
    // Cut at the near side (w small), then by each edge of the image in
    // clip space (-w ≤ x, y ≤ w).
    let planes: [fn(Vec4) -> f32; 5] = [
        |p| p.w - 1e-4,
        |p| p.w + p.x,
        |p| p.w - p.x,
        |p| p.w + p.y,
        |p| p.w - p.y,
    ];
    let mut polygon = clip;
    for plane in planes {
        let mut kept = Vec::with_capacity(polygon.len() + 1);
        for (at, &point) in polygon.iter().enumerate() {
            let next = polygon[(at + 1) % polygon.len()];
            let (here, there) = (plane(point), plane(next));
            if here >= 0.0 {
                kept.push(point);
            }
            if (here >= 0.0) != (there >= 0.0) {
                kept.push(point + (next - point) * (here / (here - there)));
            }
        }
        polygon = kept;
        if polygon.len() < 3 {
            return 0.0;
        }
    }
    let screen: Vec<Vec2> = polygon
        .iter()
        .map(|p| (p.truncate().truncate() / p.w) * 0.5 * size)
        .collect();
    let twice: f32 = (0..screen.len())
        .map(|at| screen[at].perp_dot(screen[(at + 1) % screen.len()]))
        .sum();
    f64::from(twice.abs() / 2.0)
}

/// The column of `m` (row-major) as a direction.
fn axis(m: &Matrix, column: usize) -> Vec3 {
    Vec3::new(m[0][column], m[1][column], m[2][column])
}

/// A particle's quad into `batch`, laid out as its emitter renders.
fn quad(batch: &mut Batch, drawn: &Drawn, particle: &Particle, (right, up): (Vec3, Vec3)) {
    let Look {
        colour,
        alpha,
        size,
        frame,
    } = look(&drawn.params, drawn.emitter.grid, particle);
    if alpha <= 0.0 || size[0] <= 0.0 {
        return;
    }
    let centre = Vec3::from_array(particle.position);
    let (half_x, half_y) = (size[0] / 2.0, size[1] / 2.0);
    let (across, along) = match drawn.emitter.render {
        Render::BillboardToLocalZ => {
            let (x, y) = (
                axis(&drawn.to_model, 0).normalize_or_zero(),
                axis(&drawn.to_model, 1).normalize_or_zero(),
            );
            turned(x, y, particle.spin)
        }
        Render::BillboardToWorldZ => turned(Vec3::X, Vec3::Y, particle.spin),
        Render::MotionBlur | Render::AlignedToParticleDir => {
            let velocity = Vec3::from_array(particle.velocity);
            let speed = velocity.length();
            // Facing the eye, long along the flight seen from it.
            let facing = right.cross(up);
            let flight = (velocity - facing * velocity.dot(facing)).normalize_or(up);
            let length = if drawn.emitter.render == Render::MotionBlur {
                half_y.max(speed * drawn.params.blur_length * 0.01)
            } else {
                half_y
            };
            let side = flight.cross(facing).normalize_or(right);
            return corners(
                batch,
                centre,
                (side * half_x, flight * length),
                colour,
                alpha,
                (drawn, frame),
            );
        }
        Render::Normal | Render::AlignedToWorldZ | Render::Linked => {
            turned(right, up, particle.spin)
        }
    };
    corners(
        batch,
        centre,
        (across * half_x, along * half_y),
        colour,
        alpha,
        (drawn, frame),
    );
}

/// `x` and `y` turned by `angle` in their plane.
fn turned(x: Vec3, y: Vec3, angle: f32) -> (Vec3, Vec3) {
    let (sin, cos) = angle.sin_cos();
    (x * cos + y * sin, y * cos - x * sin)
}

fn corners(
    batch: &mut Batch,
    centre: Vec3,
    (across, along): (Vec3, Vec3),
    colour: [f32; 3],
    alpha: f32,
    (drawn, frame): (&Drawn, u32),
) {
    let [columns, rows] = drawn.emitter.grid;
    let (column, row) = (frame % columns, (frame / columns) % rows);
    let (w, h) = (1.0 / columns as f32, 1.0 / rows as f32);
    let (u0, v0) = (column as f32 * w, row as f32 * h);
    let (u1, v1) = (u0 + w, v0 + h);
    batch.quad(
        [
            // Low v at the foot: the texture's rows go up bottom first.
            (centre - across - along, [u0, v0]),
            (centre + across - along, [u1, v0]),
            (centre + across + along, [u1, v1]),
            (centre - across + along, [u0, v1]),
        ],
        [colour[0], colour[1], colour[2], alpha],
        |p| p,
    );
}
