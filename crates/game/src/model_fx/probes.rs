//! What a door's clip keys on its meshes, as the doors play it
//! (`lifecycle_view::doors`): a mesh its clip fades (a force field opening,
//! Onderon's palace `DOR_OND05`) gets a material of its own, drawn blended
//! while its alpha is below 1 (`own_materials`, `Lit::surface`), whose
//! alpha follows the clip's keys as it plays (`Lit::key`, `Lit::show`) and
//! goes back to the mesh's own for a door that has not moved
//! (`Lit::reset`). Back at alpha 1, the mesh is drawn opaque again. A
//! mesh blended by its own alpha of 0 (a Star Forge captive's drained
//! body) is drawn opaque once its clip keys it to 1.

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
            dangly: None,
        }],
        lights: Vec::new(),
        emitters: Vec::new(),
        animations: Vec::new(),
        animation_scale: 1.0,
        affected_by_fog: false,
        refusals: Vec::new(),
        folded: Vec::new(),
    }
}

/// An opaque material, as a door's mesh is spawned with.
fn opaque() -> OdysseyMaterial {
    made(material::Surface::Opaque, 1.0)
}

/// A mesh's material of `surface`, its own alpha `alpha`.
fn made(surface: material::Surface, alpha: f32) -> OdysseyMaterial {
    let lighting = Lighting {
        self_illum: [0.0; 3],
        ambient: [1.0; 3],
        diffuse: [1.0; 3],
        alpha,
        is_static: false,
        is_room: false,
        fogged: false,
    };
    OdysseyMaterial::new(
        Vec4::ONE,
        None,
        None,
        None,
        surface,
        &lighting,
        &AreaLight::ambient([1.0; 3]),
    )
}

/// What the field's drawn material shows: its alpha at the clip's start,
/// middle and end, its alpha once reset, whether it is drawn blended at
/// the clip's middle, and whether it is drawn opaque once reset.
type Shown = ([f32; 3], f32, bool, bool);

/// The door's field faded by its clip (alpha 1 to 0 over `LENGTH`): the
/// drawn alpha follows the keys, a reset door shows its mesh's own, the
/// half-faded mesh is drawn blended, and the reset mesh is drawn opaque.
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
                    (&mut meshes, &mut materials, None),
                )?;
                let [field] = &mut lit[..] else {
                    return None;
                };
                // Written to the material itself, to be read back.
                let alpha = |lit: &Lit, materials: &mut Assets<OdysseyMaterial>| {
                    lit.show(materials, None);
                    materials.get(&lit.material).map(|m| m.params.tint.w)
                };
                let surface = |lit: &Lit, materials: &Assets<OdysseyMaterial>| {
                    materials.get(&lit.material).map(|m| m.surface)
                };
                let mut keyed = [0.0; 3];
                let mut blended = false;
                for (at, shown) in [0.0, LENGTH / 2.0, LENGTH].into_iter().zip(&mut keyed) {
                    field.key((&fx, &ir), CLIP, at);
                    *shown = alpha(field, &mut materials)?;
                    if at == LENGTH / 2.0 {
                        blended = surface(field, &materials) == Some(material::Surface::Blend);
                    }
                }
                field.reset(&ir);
                let reset = alpha(field, &mut materials)?;
                let opaque = surface(field, &materials) == Some(material::Surface::Opaque);
                Some((keyed, reset, blended, opaque))
            },
        )
        .map_err(|error| error.to_string())?;
    let Some((keyed, reset, blended, opaque)) = shown else {
        return Err("the field's mesh got no material of its own".into());
    };
    let near = |a: f32, b: f32| (a - b).abs() < 1e-4;
    check(
        near(keyed[0], 1.0)
            && near(keyed[1], 0.5)
            && near(keyed[2], 0.0)
            && near(reset, 1.0)
            && blended
            && opaque,
        format!(
            "alpha over the clip {keyed:?}, reset {reset}, blended {blended}, opaque {opaque}"
        ),
    )
}

/// A captive's drained body (`plc_endcorps`'s `torso2`, issue 408): its
/// mesh's own alpha is 0, so it is made blended, and its clip keys the
/// alpha from 0 to 1 over `LENGTH`. Its surface at the clip's start, its
/// end, and once reset to its mesh's own alpha: blended, opaque, blended.
fn captive_clip_alpha() -> Result<String, String> {
    let mut ir = field_model();
    ir.meshes[0].alpha = 0.0;
    let mut fx = ModelFx::default();
    let keys = Surface {
        alpha: vec![(0.0, 0.0), (LENGTH, 1.0)],
        ..Surface::default()
    };
    fx.surfaces
        .insert(CLIP.into(), [(FIELD.to_string(), keys)].into());
    let mut app = crate::probe_kit::scene_app();
    let world = app.world_mut();
    let blended = made(material::Surface::Blend, 0.0).whole_as(&opaque());
    let drawn = world
        .resource_mut::<Assets<OdysseyMaterial>>()
        .add(blended);
    let node = world.spawn_empty().id();
    world.spawn((MeshMaterial3d(drawn), ChildOf(node)));
    let shown = world
        .run_system_once(
            move |children: Query<&Children>,
                  mut meshes: Query<&mut MeshMaterial3d<OdysseyMaterial>>,
                  mut materials: ResMut<Assets<OdysseyMaterial>>|
                  -> Option<[material::Surface; 3]> {
                let mut lit = own_materials(
                    (&fx, &ir, &[node]),
                    &[],
                    &children,
                    (&mut meshes, &mut materials, None),
                )?;
                let [body] = &mut lit[..] else {
                    return None;
                };
                let mut surface = |lit: &Lit| {
                    lit.show(&mut materials, None);
                    materials.get(&lit.material).map(|m| m.surface)
                };
                body.key((&fx, &ir), CLIP, 0.0);
                let start = surface(body)?;
                body.key((&fx, &ir), CLIP, LENGTH);
                let end = surface(body)?;
                body.reset(&ir);
                Some([start, end, surface(body)?])
            },
        )
        .map_err(|error| error.to_string())?;
    let Some(shown) = shown else {
        return Err("the body's mesh got no material of its own".into());
    };
    use material::Surface::{Blend, Opaque};
    check(
        shown == [Blend, Opaque, Blend],
        format!("start, end and reset {shown:?}"),
    )
}

pub(super) fn probes() -> Checks {
    vec![
        ("door_clip_alpha", door_clip_alpha()),
        ("captive_clip_alpha", captive_clip_alpha()),
    ]
}
