//! The stealth field drawn (`docs/COMBAT.md`): every
//! character in stealth mode (`Fighter::covert`) wears KOTOR's stealth
//! shell, its meshes drawn again pushed out along their normals in the
//! field's texture, added to what is behind (`shell`), for every player.
//! Each of its meshes gets a child drawing the same mesh (skinned as it is)
//! with the shell's material; they go when the stealth ends or the
//! character does.

use std::collections::BTreeMap;

use bevy::mesh::skinning::SkinnedMesh;
use bevy::prelude::*;
use sim::SimId;

use crate::cast::BodyRoots;
use crate::made::Made;
use crate::material::OdysseyMaterial;
use crate::shell::{PUSH_M, ShellMaterial, ShellParams};
use crate::sim_host::PresentedSnapshot;

/// How fast the field's texture drifts across the body (UV a second).
const DRIFT: f32 = 0.15;

/// The shells drawn, by character, and the one shell material.
#[derive(Resource, Default)]
pub struct Shells {
    by: BTreeMap<SimId, Vec<Entity>>,
    material: Option<Handle<ShellMaterial>>,
}

/// Each frame: shells for those newly in stealth, none for those out of it.
#[allow(clippy::too_many_arguments)]
pub fn shells(
    presented: Res<PresentedSnapshot>,
    roots: Res<BodyRoots>,
    made: Res<Made>,
    time: Res<Time>,
    children: Query<&'static Children>,
    meshes: ShellMeshes,
    (mut shells, mut materials): (ResMut<Shells>, ResMut<Assets<ShellMaterial>>),
    mut commands: Commands,
) {
    let Some(texture) = made.stealth_texture.clone() else {
        return;
    };
    let material = shells
        .material
        .get_or_insert_with(|| {
            materials.add(ShellMaterial {
                params: ShellParams {
                    tint: Vec4::ONE,
                    push: Vec4::new(PUSH_M, 0.0, 0.0, 0.0),
                },
                texture: Some(texture),
            })
        })
        .clone();
    if let Some(mut shell) = materials.get_mut(&material) {
        shell.params.push.y = (time.elapsed_secs() * DRIFT).fract();
    }
    let stealthed: Vec<SimId> = presented
        .0
        .current()
        .entities
        .iter()
        .filter(|entity| entity.fighter.is_some_and(|fighter| fighter.covert.stealth))
        .map(|entity| entity.id)
        .collect();
    let bodies = Bodies {
        roots: &roots,
        children: &children,
        meshes: &meshes,
    };
    follow(
        &mut shells.by,
        &stealthed,
        &material,
        &bodies,
        &mut commands,
    );
}

/// Where the characters' meshes are, to draw shells over.
pub(crate) struct Bodies<'a, 'w, 's> {
    pub roots: &'a BodyRoots,
    pub children: &'a Query<'w, 's, &'static Children>,
    pub meshes: &'a ShellMeshes<'w, 's>,
}

/// The meshes a shell may cover: an Odyssey mesh, skinned or not.
pub(crate) type ShellMeshes<'w, 's> = Query<
    'w,
    's,
    (&'static Mesh3d, Option<&'static SkinnedMesh>),
    With<MeshMaterial3d<OdysseyMaterial>>,
>;

/// Shells in `material` over the characters `wanted` (by character, in
/// `by`): new ones for those newly wanted, none left over the others or
/// over a character no longer drawn.
pub(crate) fn follow(
    by: &mut BTreeMap<SimId, Vec<Entity>>,
    wanted: &[SimId],
    material: &Handle<ShellMaterial>,
    bodies: &Bodies,
    commands: &mut Commands,
) {
    let ended: Vec<SimId> = by
        .keys()
        .filter(|id| !wanted.contains(id) || !bodies.roots.0.contains_key(id))
        .copied()
        .collect();
    for id in ended {
        for entity in by.remove(&id).into_iter().flatten() {
            if let Ok(mut shell) = commands.get_entity(entity) {
                shell.despawn();
            }
        }
    }
    for &id in wanted {
        let Some(&root) = bodies.roots.0.get(&id) else {
            continue;
        };
        if by.contains_key(&id) {
            continue;
        }
        let mut drawn = Vec::new();
        for entity in bodies.children.iter_descendants(root) {
            let Ok((mesh, skinned)) = bodies.meshes.get(entity) else {
                continue;
            };
            let mut shell = commands.spawn((
                Mesh3d(mesh.0.clone()),
                MeshMaterial3d(material.clone()),
                Transform::IDENTITY,
                ChildOf(entity),
            ));
            if let Some(skinned) = skinned {
                shell.insert(skinned.clone());
            }
            drawn.push(shell.id());
        }
        by.insert(id, drawn);
    }
}
