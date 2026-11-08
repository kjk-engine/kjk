//! Motion proxies: depth and motion in a camera's prepass for the blended
//! opaque surfaces (`Surface::OpaqueAlpha`: characters' skins, hair,
//! foliage), which Bevy's prepass leaves out with everything blended.
//!
//! While the anti-aliasing resolves frames over time (`temporal`), each mesh
//! drawn so has a proxy: its own entity, not a child (whatever walks a
//! model's meshes to fade, tint or copy them never meets one), on the same
//! mesh, skin, place and visibility, every frame, with a copy of its
//! material that draws only into the prepass (`OdysseyMaterial::
//! motion_only`): depth and motion where the surface is solid
//! (`material::PROXY_SOLID`), and nothing in the main pass or the shadow
//! maps. The mesh itself draws as before; its pixels the proxy covers now
//! say how they moved, as the opaque ones' do.
//!
//! A proxy goes with its mesh (despawned, or drawn otherwise), and every
//! proxy goes when the anti-aliasing is no longer temporal.

use bevy::asset::AssetEventSystems;
use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::{NoFrustumCulling, RenderLayers, VisibilitySystems};
use bevy::ecs::lifecycle::HookContext;
use bevy::ecs::world::DeferredWorld;
use bevy::light::NotShadowCaster;
use bevy::mesh::skinning::SkinnedMesh;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::transform::TransformSystems;
use console::{DumpDue, DumpFacts};

use crate::material::{MotionFacts, OdysseyMaterial};
use crate::temporal::Temporal;

pub struct MotionProxyPlugin;

impl Plugin for MotionProxyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ProxyMaterials>().add_systems(
            PostUpdate,
            // Where the meshes stand and whether they show this frame, and
            // before the frame's visibility and asset changes are taken.
            (stand_in, follow)
                .chain()
                .after(TransformSystems::Propagate)
                .after(VisibilitySystems::VisibilityPropagate)
                .before(VisibilitySystems::CheckVisibility)
                .before(AssetEventSystems),
        );
    }
}

/// A proxy, standing for the mesh entity `of`.
#[derive(Component)]
pub struct MotionProxy {
    of: Entity,
}

/// A mesh entity's proxy, which goes when this does.
#[derive(Component)]
#[component(on_remove = drop_proxy)]
struct Proxied(Entity);

fn drop_proxy(mut world: DeferredWorld, context: HookContext) {
    if let Some(&Proxied(proxy)) = world.get::<Proxied>(context.entity) {
        world.commands().entity(proxy).try_despawn();
    }
}

/// The proxies' materials, one for each material a proxied mesh draws with,
/// and what of it each copied (made anew when that changes).
#[derive(Resource, Default)]
struct ProxyMaterials(HashMap<AssetId<OdysseyMaterial>, (Handle<OdysseyMaterial>, MotionFacts)>);

impl ProxyMaterials {
    /// `source`'s proxy material, made now if it has none.
    fn of(
        &mut self,
        source: &Handle<OdysseyMaterial>,
        materials: &mut Assets<OdysseyMaterial>,
    ) -> Option<Handle<OdysseyMaterial>> {
        if let Some((proxy, _)) = self.0.get(&source.id()) {
            return Some(proxy.clone());
        }
        let copied = materials.get(source)?.clone();
        let facts = copied.motion_facts();
        let proxy = materials.add(copied.motion_only());
        self.0.insert(source.id(), (proxy.clone(), facts));
        Some(proxy)
    }

    /// Each proxy material copied again where its source changed, and
    /// dropped where its source is gone.
    fn refresh(&mut self, materials: &mut Assets<OdysseyMaterial>) {
        self.0.retain(|&source, (proxy, facts)| {
            let Some(now) = materials.get(source) else {
                return false;
            };
            let now_facts = now.motion_facts();
            if now_facts != *facts {
                let copied = now.clone().motion_only();
                *facts = now_facts;
                if let Some(mut material) = materials.get_mut(&*proxy) {
                    *material = copied;
                }
            }
            true
        });
    }
}

/// A mesh entity a proxy may stand for, and what the proxy copies of it.
type Source = (
    Entity,
    &'static Mesh3d,
    &'static MeshMaterial3d<OdysseyMaterial>,
    Option<&'static SkinnedMesh>,
    &'static GlobalTransform,
    &'static InheritedVisibility,
    Option<&'static RenderLayers>,
    Option<&'static Aabb>,
    Has<NoFrustumCulling>,
    Has<Proxied>,
);

/// Gives each mesh that wants one a proxy, and takes it from one that no
/// longer does (or from every mesh, when the anti-aliasing is not temporal).
#[allow(clippy::type_complexity)]
fn stand_in(
    temporal: Option<Res<Temporal>>,
    mut proxies: ResMut<ProxyMaterials>,
    mut materials: ResMut<Assets<OdysseyMaterial>>,
    sources: Query<Source, Without<MotionProxy>>,
    proxied: Query<Entity, With<Proxied>>,
    mut commands: Commands,
) {
    if !temporal.is_some_and(|temporal| temporal.used.temporal()) {
        for entity in &proxied {
            commands.entity(entity).remove::<Proxied>();
        }
        proxies.0.clear();
        return;
    }
    proxies.refresh(&mut materials);
    let scene = RenderLayers::default();
    for (entity, mesh, material, skin, global, shown, layers, aabb, unculled, has_proxy) in &sources
    {
        let wants = layers.is_none_or(|layers| layers.intersects(&scene))
            && materials
                .get(&material.0)
                .is_some_and(OdysseyMaterial::wants_proxy);
        if !wants {
            if has_proxy {
                commands.entity(entity).remove::<Proxied>();
            }
            continue;
        }
        if has_proxy {
            continue;
        }
        let Some(proxy_material) = proxies.of(&material.0, &mut materials) else {
            continue;
        };
        let mut proxy = commands.spawn((
            MotionProxy { of: entity },
            Mesh3d(mesh.0.clone()),
            MeshMaterial3d(proxy_material),
            *global,
            *shown,
            NotShadowCaster,
        ));
        if let Some(skin) = skin {
            proxy.insert(skin.clone());
        }
        if let Some(aabb) = aabb {
            proxy.insert(*aabb);
        }
        if unculled {
            proxy.insert(NoFrustumCulling);
        }
        let proxy = proxy.id();
        commands.entity(entity).try_insert(Proxied(proxy));
    }
}

/// Each proxy where its mesh is this frame: its mesh, material, skin,
/// place, visibility and bounds. And in the dump: how many there are.
#[allow(clippy::type_complexity)]
fn follow(
    mut proxies: Query<(
        Entity,
        &MotionProxy,
        &mut Mesh3d,
        &mut MeshMaterial3d<OdysseyMaterial>,
        Option<&mut SkinnedMesh>,
        &mut GlobalTransform,
        &mut InheritedVisibility,
        Option<&mut Aabb>,
        Has<NoFrustumCulling>,
    )>,
    sources: Query<Source, Without<MotionProxy>>,
    materials: Res<ProxyMaterials>,
    (mut facts, dump_due): (ResMut<DumpFacts>, Res<DumpDue>),
    mut commands: Commands,
) {
    for (entity, proxy, mut mesh, mut material, skin, mut global, mut shown, aabb, unculled) in
        &mut proxies
    {
        let Ok((
            _,
            its_mesh,
            its_material,
            its_skin,
            its_global,
            its_shown,
            _,
            its_aabb,
            its_unculled,
            _,
        )) = sources.get(proxy.of)
        else {
            commands.entity(entity).despawn();
            continue;
        };
        if mesh.0 != its_mesh.0 {
            mesh.0 = its_mesh.0.clone();
        }
        if let Some((handle, _)) = materials.0.get(&its_material.0.id())
            && material.0 != *handle
        {
            material.0 = handle.clone();
        }
        global.set_if_neq(*its_global);
        shown.set_if_neq(*its_shown);
        // What the mesh took since its proxy was made.
        let mut proxy = commands.entity(entity);
        match (skin, its_skin) {
            (Some(mut skin), Some(its_skin)) => {
                if skin.joints != its_skin.joints
                    || skin.inverse_bindposes != its_skin.inverse_bindposes
                {
                    *skin = its_skin.clone();
                }
            }
            (None, Some(its_skin)) => {
                proxy.insert(its_skin.clone());
            }
            _ => {}
        }
        match (aabb, its_aabb) {
            (Some(mut aabb), Some(its_aabb)) => {
                aabb.set_if_neq(*its_aabb);
            }
            (None, Some(its_aabb)) => {
                proxy.insert(*its_aabb);
            }
            _ => {}
        }
        if its_unculled && !unculled {
            proxy.insert(NoFrustumCulling);
        }
    }
    if dump_due.0 {
        facts.0.insert(
            "motion_proxies".into(),
            serde_json::json!(proxies.iter().len()),
        );
    }
}
