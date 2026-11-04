//! What a door's clip keys on its meshes, as the doors play it
//! (`lifecycle_view::doors`): a mesh its clip fades (a force field opening,
//! Onderon's palace `DOR_OND05`) gets a material of its own, drawn blended
//! (`own_materials`), whose alpha follows the clip's keys as it plays
//! (`Lit::key`, `Lit::show`) and goes back to the mesh's own for a door
//! that has not moved (`Lit::reset`).

use asset_odyssey::model::{IDENTITY, MeshIr, ModelIr, NodeIr};
use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;

use super::{Lit, ModelFx, Surface, own_materials};
use crate::material::{self, AreaLight, Lighting, OdysseyMaterial};
use crate::probe_kit::{Checks, check};

/// The door's clip, its faded node and the clip's length (s).
const CLIP: &str = "opening1";
const FIELD: &str = "field";
const LENGTH: f32 = 2.0;

/// A door model of one node, `FIELD`, whose mesh is opaque at rest.
fn field_model() -> ModelIr {
    ModelIr {
        name: "probe_door".into(),
        supermodel: None,
        nodes: vec![NodeIr {
            name: FIELD.into(),
            parent: None,
            translation: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            world: IDENTITY,
            animated: true,
        }],
        meshes: vec![MeshIr {
            node: 0,
            positions: Vec::new(),
            normals: Vec::new(),
            uv1: Vec::new(),
            uv2: Vec::new(),
            indices: Vec::new(),
            texture: None,
            lightmap: None,
            transparency_hint: 0,
            skinned: false,
            self_illum: [0.0; 3],
            alpha: 1.0,
            ambient: [1.0; 3],
            diffuse: [1.0; 3],
            uv_scroll: None,
            skin: None,
        }],
        lights: Vec::new(),
        emitters: Vec::new(),
        animations: Vec::new(),
        animation_scale: 1.0,
        affected_by_fog: false,
        refusals: Vec::new(),
    }
}

/// An opaque material, as a door's mesh is spawned with.
fn opaque() -> OdysseyMaterial {
    let lighting = Lighting {
        self_illum: [0.0; 3],
        ambient: [1.0; 3],
        diffuse: [1.0; 3],
        alpha: 1.0,
        is_static: false,
        is_room: false,
        fogged: false,
    };
    OdysseyMaterial::new(
        Vec4::ONE,
        None,
        None,
        None,
        material::Surface::Opaque,
        &lighting,
        &AreaLight::ambient([1.0; 3]),
    )
}

/// What the field's drawn material shows: its alpha at the clip's start,
/// middle and end, its alpha once reset, and whether it is drawn blended.
type Shown = ([f32; 3], f32, bool);

/// The door's field faded by its clip (alpha 1 to 0 over `LENGTH`): the
/// drawn alpha follows the keys, a reset door shows its mesh's own, and the
/// faded mesh is drawn blended, no longer opaque.
fn door_clip_alpha() -> Result<String, String> {
    let ir = field_model();
    let mut fx = ModelFx::default();
    let keys = Surface {
        alpha: vec![(0.0, 1.0), (LENGTH, 0.0)],
        ..Surface::default()
    };
    fx.surfaces
        .insert(CLIP.into(), [(FIELD.to_string(), keys)].into());
    let mut app = crate::probe_kit::scene_app();
    let world = app.world_mut();
    let drawn = world
        .resource_mut::<Assets<OdysseyMaterial>>()
        .add(opaque());
    let node = world.spawn_empty().id();
    world.spawn((MeshMaterial3d(drawn), ChildOf(node)));
    let shown = world
        .run_system_once(
            move |children: Query<&Children>,
                  mut meshes: Query<&mut MeshMaterial3d<OdysseyMaterial>>,
                  mut materials: ResMut<Assets<OdysseyMaterial>>|
                  -> Option<Shown> {
                let mut lit = own_materials(
                    (&fx, &ir, &[node]),
                    &[],
                    &children,
                    &mut meshes,
                    &mut materials,
                )?;
                let [field] = &mut lit[..] else {
                    return None;
                };
                let alpha = |lit: &Lit, materials: &mut Assets<OdysseyMaterial>| {
                    lit.show(materials);
                    materials.get(&lit.material).map(|m| m.params.tint.w)
                };
                let mut keyed = [0.0; 3];
                for (at, shown) in [0.0, LENGTH / 2.0, LENGTH].into_iter().zip(&mut keyed) {
                    field.key((&fx, &ir), CLIP, at);
                    *shown = alpha(field, &mut materials)?;
                }
                field.reset(&ir);
                let reset = alpha(field, &mut materials)?;
                let blended = materials
                    .get(&field.material)
                    .is_some_and(|m| m.surface == material::Surface::Blend);
                Some((keyed, reset, blended))
            },
        )
        .map_err(|error| error.to_string())?;
    let Some((keyed, reset, blended)) = shown else {
        return Err("the field's mesh got no material of its own".into());
    };
    let near = |a: f32, b: f32| (a - b).abs() < 1e-4;
    check(
        near(keyed[0], 1.0)
            && near(keyed[1], 0.5)
            && near(keyed[2], 0.0)
            && near(reset, 1.0)
            && blended,
        format!("alpha over the clip {keyed:?}, reset {reset}, blended {blended}"),
    )
}

pub(super) fn probes() -> Checks {
    vec![("door_clip_alpha", door_clip_alpha())]
}
