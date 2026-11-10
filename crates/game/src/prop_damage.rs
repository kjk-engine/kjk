//! How damage shows on a prop before (or without) it breaking: chunks
//! knocked off and torn panels left hanging (`docs/superpowers/specs/
//! 2026-11-06-destruction-design.md`, "Third pass"). A damaged prop is cut
//! by the same fracture as breaking (`prop_looks::Fractures`); at each
//! stage some of its pieces leave, the outermost and highest likelier, never
//! its base. From the first, each of its cut meshes is drawn as its
//! remaining pieces in place (dented as the stage has it), so the gaps and
//! their capped cuts show; a flat piece leaving hinges on its edge nearest
//! the prop's middle instead of falling. Every choice is seeded from the
//! model, the object and the stage, so every seat, save and return shows
//! the same damage; none of it is synced. Collision never changes.

use std::collections::{BTreeMap, BTreeSet};

use asset_odyssey::model::{MeshIr, ModelIr};
use bevy::asset::RenderAssetUsages;
use bevy::mesh::Indices;
use bevy::prelude::*;
use ragdoll::shatter::{self, Piece, Plain};
use sim::destruction::{Class, Destructible, Material, Size};
use sim::look;

use crate::made::Made;
use crate::material::OdysseyMaterial;
use crate::prop_breaks::PropChanges;
use crate::prop_looks::{
    Fractures, MeshParts, PIECES, PropLooks, base, blow_from, dentable, dents, in_game, keyed,
    lay_pieces, model_of, noise, piece_meshes,
};
use crate::scene::{SceneContent, SceneModel, base_mesh};
use crate::sim_host::{PresentedSnapshot, SceneSwap};

/// How many pieces leave at stage 1 (the least and the most) and how many
/// more at stage 2, by size (small, medium, large).
const KNOCKED: [(usize, usize, usize); 3] = [(1, 1, 1), (1, 2, 2), (2, 3, 3)];
/// A piece is flat when its thinnest side is under this share of each of
/// its others.
const FLAT_SHARE: f32 = 1.0 / 3.0;
/// How far a hanging panel turns about its hinge (degrees, least and most).
const HANG_DEG: (f32, f32) = (25.0, 70.0);
/// The chance that stage 2 drops the panel stage 1 left hanging.
const FALLS: f32 = 0.5;
/// How far off a missing piece's box a mark still counts as over its gap
/// (metres).
const GAP_M: f32 = 0.02;
/// The most lasting emitters lit, and how near a player they must be
/// (metres): the nearest first.
const MOST_EMITTERS: usize = 24;
const EMITTERS_REACH_M: f32 = 30.0;
/// How long a broken prop's base smokes (s, least and most).
const BASE_SMOKE_S: (f64, f64) = (5.0, 10.0);
/// How high over its feet a broken prop's base smokes (metres).
const BASE_SMOKE_M: f32 = 0.2;

/// A hanging panel's hinge: the two ends of the edge it turns about (model
/// frame, metres), and how far it turned (degrees).
pub(crate) type Hinge = ([f32; 3], [f32; 3], f32);

/// The pieces gone by `stage` (cumulative: stage 2's hold stage 1's), never
/// one of `kept` (the base, and pieces that cannot leave): small 1 then 1
/// more, medium 1–2 then 2 more, large 2–3 then 3 more, as many as there
/// are; the outermost and highest likelier (`seed` weighs each), the first
/// to go first.
pub(crate) fn knocked(
    pieces: &[Piece],
    kept: &[usize],
    size: Size,
    stage: u8,
    seed: u64,
) -> Vec<usize> {
    let (least, most, more) = KNOCKED[size as usize];
    let first = least + (noise(&[], seed) * (most - least + 1) as f32) as usize;
    let count = match stage {
        0 => 0,
        1 => first.min(most),
        _ => first.min(most) + more,
    };
    let count_xy = pieces.len().max(1) as f32;
    let middle = [0, 1].map(|axis| pieces.iter().map(|p| p.middle[axis]).sum::<f32>() / count_xy);
    let mut order: Vec<(usize, f32)> = (0..pieces.len())
        .filter(|i| !kept.contains(i))
        .map(|i| {
            let [x, y, z] = pieces[i].middle;
            let outer = ((x - middle[0]).powi(2) + (y - middle[1]).powi(2)).sqrt();
            (i, (outer + z) * (0.5 + noise(&pieces[i].middle, seed)))
        })
        .collect();
    order.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    order.into_iter().take(count).map(|(i, _)| i).collect()
}

/// The box round `piece` (model frame): least and greatest corners.
fn extent(piece: &Piece) -> ([f32; 3], [f32; 3]) {
    let points = piece.hull.iter().chain(std::iter::once(&piece.middle));
    points.fold(([f32::MAX; 3], [f32::MIN; 3]), |(lo, hi), p| {
        (
            std::array::from_fn(|i| lo[i].min(p[i])),
            std::array::from_fn(|i| hi[i].max(p[i])),
        )
    })
}

/// How `piece` hangs if it is flat (its thinnest side under a third of
/// each other): its hinge, the edge of its broad face nearest
/// `middle_of_prop`, and how far it turns (25–70°, seeded by where it is);
/// `None` for a piece that is not flat.
pub(crate) fn hangs(piece: &Piece, middle_of_prop: [f32; 3]) -> Option<Hinge> {
    let (lo, hi) = extent(piece);
    let sides: [f32; 3] = std::array::from_fn(|i| hi[i] - lo[i]);
    let thin = (0..3).min_by(|&a, &b| sides[a].total_cmp(&sides[b]))?;
    let [u, v] = [(thin + 1) % 3, (thin + 2) % 3];
    if sides[thin] >= sides[u] * FLAT_SHARE || sides[thin] >= sides[v] * FLAT_SHARE {
        return None;
    }
    let mut mid = [0.0; 3];
    mid[thin] = (lo[thin] + hi[thin]) / 2.0;
    // The four edges of the broad face, at its middle depth.
    let edge = |along: usize, across: usize, at: f32| {
        let mut a = mid;
        let mut b = mid;
        a[across] = at;
        b[across] = at;
        a[along] = lo[along];
        b[along] = hi[along];
        (a, b)
    };
    let edges = [
        edge(v, u, lo[u]),
        edge(v, u, hi[u]),
        edge(u, v, lo[v]),
        edge(u, v, hi[v]),
    ];
    let far = |(a, b): &([f32; 3], [f32; 3])| {
        (0..3)
            .map(|i| ((a[i] + b[i]) / 2.0 - middle_of_prop[i]).powi(2))
            .sum::<f32>()
    };
    let (a, b) = edges.into_iter().min_by(|x, y| far(x).total_cmp(&far(y)))?;
    let (lo_deg, hi_deg) = HANG_DEG;
    Some((
        a,
        b,
        lo_deg + (hi_deg - lo_deg) * noise(&piece.middle, 0x4a4e),
    ))
}

/// How a damaged prop's pieces are drawn at `stage`.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct Chunks {
    /// Knocked off: not drawn in place.
    pub gone: BTreeSet<usize>,
    /// Torn panels hanging on their hinges.
    pub hanging: Vec<(usize, Hinge)>,
    /// Those gone since `drawn` (seen live), to fly as debris; none for a
    /// prop first seen damaged.
    pub flies: Vec<usize>,
}

/// `pieces` of a prop drawn at `stage`, from the stage it was drawn at
/// (`drawn`; `None` when first seen): of those leaving at each stage
/// (`knocked`), the first flat one hangs, the rest are gone; stage 2 may
/// drop stage 1's panel (seeded). Only what leaves while it is seen flies.
pub(crate) fn chunks(
    pieces: &[Piece],
    kept: &[usize],
    (size, stage, seed): (Size, u8, u64),
    middle: [f32; 3],
    drawn: Option<u8>,
) -> Chunks {
    let at = |stage: u8| {
        let mut gone = BTreeSet::new();
        let mut hanging: Vec<(usize, Hinge)> = Vec::new();
        let mut before: Vec<usize> = Vec::new();
        for s in 1..=stage.min(2) {
            let now = knocked(pieces, kept, size, s, seed);
            let leaving: Vec<usize> = now
                .iter()
                .copied()
                .filter(|i| !before.contains(i))
                .collect();
            if s == 2 && noise(&[], seed ^ 0xfa11) < FALLS {
                gone.extend(hanging.drain(..).map(|(i, _)| i));
            }
            let hanger = leaving
                .iter()
                .find_map(|&i| hangs(&pieces[i], middle).map(|hinge| (i, hinge)));
            gone.extend(
                leaving
                    .iter()
                    .filter(|&&i| hanger.is_none_or(|(h, _)| h != i)),
            );
            hanging.extend(hanger);
            before = now;
        }
        (gone, hanging)
    };
    let (gone, hanging) = at(stage);
    let flies = match drawn {
        Some(was) if was < stage => {
            let (before, _) = at(was);
            gone.difference(&before).copied().collect()
        }
        _ => Vec::new(),
    };
    Chunks {
        gone,
        hanging,
        flies,
    }
}

/// Whether `point` (model frame) lies on one of `pieces` that is not
/// drawn in place (`missing`: knocked off or hanging), within `GAP_M` of
/// its box: a mark there would float over the gap.
pub(crate) fn over_gap(point: [f32; 3], pieces: &[Piece], missing: &BTreeSet<usize>) -> bool {
    missing.iter().filter_map(|&i| pieces.get(i)).any(|piece| {
        let (lo, hi) = extent(piece);
        (0..3).all(|axis| point[axis] >= lo[axis] - GAP_M && point[axis] <= hi[axis] + GAP_M)
    })
}

/// The pieces of `count` a prop breaking now throws: all but those already
/// knocked off (`gone`).
pub(crate) fn remaining(count: usize, gone: &BTreeSet<usize>) -> Vec<usize> {
    (0..count).filter(|i| !gone.contains(i)).collect()
}

/// What a lasting emitter on a damaged prop is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Kind {
    Sparks,
    Arcs,
    Smoke,
    Fire,
    Dust,
}

/// A lasting emitter: its kind and the KOTOR model whose emitters it
/// draws (an effect placeable's, `placeables.2da`), and whether that
/// model's meshes and lights are left out (a fire pit's logs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Emitter {
    pub kind: Kind,
    pub model: &'static str,
    pub bare: bool,
}

/// Each kind's emitter, from the effect placeables both games list as
/// intact (`placeables.2da`, the same rows and models in K1 and TSL), whose
/// models draw nothing but emitters unless said:
const EMITTERS: [Emitter; 5] = [
    // ElectricalSpark_small (31): `fx_star` flashes and `fx_spark`.
    Emitter {
        kind: Kind::Sparks,
        model: "plc_esparksm",
        bare: false,
    },
    // ElectricalSpark_large (30): the same, larger and wilder: arcing.
    Emitter {
        kind: Kind::Arcs,
        model: "plc_esparklg",
        bare: false,
    },
    // Smoke (100): two `fx_smoke` columns.
    Emitter {
        kind: Kind::Smoke,
        model: "plc_smoke",
        bare: false,
    },
    // Campfire (129): its `fx_fire` and `fx_smoke`, its logs and light
    // left out (no placeable draws fire alone).
    Emitter {
        kind: Kind::Fire,
        model: "plc_firepit2",
        bare: true,
    },
    // Stone's dust: the games have no dust emitter; Smoke's pale
    // `fx_smoke` stands in.
    Emitter {
        kind: Kind::Dust,
        model: "plc_smoke",
        bare: false,
    },
];

/// The emitters a damaged prop of `material` keeps at `stage`, or a broken
/// one's base (`broken`).
pub(crate) fn emitters(material: Material, stage: u8, broken: bool) -> &'static [Emitter] {
    const SPARKS: Emitter = EMITTERS[0];
    const ARCS: Emitter = EMITTERS[1];
    const SMOKE: Emitter = EMITTERS[2];
    const FIRE: Emitter = EMITTERS[3];
    const DUST: Emitter = EMITTERS[4];
    const SMOKES: &[Emitter] = &[SMOKE];
    if broken {
        return SMOKES;
    }
    match (material, stage) {
        (_, 0) => &[],
        (Material::Electronic, 1) => &[SPARKS],
        (Material::Electronic, _) => &[ARCS, SMOKE],
        (Material::Metal | Material::Plastic, _) => SMOKES,
        (Material::Wood | Material::Cloth | Material::Organic, 1) => SMOKES,
        (Material::Wood | Material::Cloth | Material::Organic, _) => &[FIRE, SMOKE],
        (Material::Stone | Material::Ceramic | Material::Debris, _) => &[DUST],
    }
}

/// Of `candidates` (each emitter's key and how far it is from the nearest
/// player, metres), the ones lit: within `reach_m`, nearest first, at most
/// `most`.
pub(crate) fn lit(candidates: &[(u32, f32)], most: usize, reach_m: f32) -> Vec<u32> {
    let mut near: Vec<(u32, f32)> = (candidates.iter())
        .filter(|(_, far)| *far <= reach_m)
        .copied()
        .collect();
    near.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    near.into_iter().take(most).map(|(key, _)| key).collect()
}

/// Lasting emitters by prop and slot: what each is and where it stands.
pub(crate) type Wanted = BTreeMap<(u32, u8), (Emitter, Vec3)>;

/// The lasting emitters wanted on props this frame, for `script_fx` to
/// draw: by prop and slot, the model (and whether bare) and where it stands
/// (game space, metres). Empty again with every scene swap.
#[derive(Resource, Default)]
pub(crate) struct PropEmitters {
    generation: u32,
    pub wanted: Wanted,
}

impl PropEmitters {
    /// What is wanted in scene swap `generation`: nothing while it still
    /// holds another's (the frame a scene swaps in, before `emit` runs).
    pub(crate) fn wanted_in(&self, generation: u32) -> Option<&Wanted> {
        (self.generation == generation).then_some(&self.wanted)
    }

    /// For scene swap `generation`: one from another starts empty.
    pub(crate) fn start(&mut self, generation: u32) {
        if self.generation != generation {
            *self = PropEmitters {
                generation,
                ..PropEmitters::default()
            };
        }
    }
}

/// Each damaged prop as the sim has it and as drawn, for the scene swap
/// they belong to.
#[derive(Resource, Default)]
pub(crate) struct PropDamage {
    generation: u32,
    /// The stage each prop was first seen at: what it is drawn from
    /// (nothing knocked off by then flies).
    seen: BTreeMap<u32, u8>,
    /// Each damaged prop's stage as the sim has it, its kind and where it
    /// stands (game space, metres).
    wanted: BTreeMap<u32, (u8, Destructible, [f32; 3])>,
    drawn: BTreeMap<u32, Drawn>,
    /// Broken props' bases smoking: until when (ms of `Time`) and where.
    smoking: BTreeMap<u32, (f64, Vec3)>,
}

/// A damaged prop as drawn: its stage, its chunks, its hanging panels.
#[derive(Default)]
struct Drawn {
    stage: u8,
    chunks: Chunks,
    hangers: Vec<Entity>,
}

impl PropDamage {
    /// Prop `index`'s pieces not drawn in place: knocked off or hanging.
    pub(crate) fn missing(&self, index: u32) -> BTreeSet<usize> {
        let Some(drawn) = self.drawn.get(&index) else {
            return BTreeSet::new();
        };
        let hanging = drawn.chunks.hanging.iter().map(|(i, _)| *i);
        drawn.chunks.gone.iter().copied().chain(hanging).collect()
    }

    /// Prop `index`'s pieces knocked off, if it is drawn damaged.
    pub(crate) fn gone(&self, index: u32) -> Option<&BTreeSet<usize>> {
        self.drawn.get(&index).map(|drawn| &drawn.chunks.gone)
    }
}

pub struct PropDamagePlugin;

impl Plugin for PropDamagePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PropDamage>()
            .init_resource::<PropEmitters>()
            .add_systems(
                PostUpdate,
                (draw, emit)
                    .chain()
                    .after(crate::prop_looks::stages)
                    .before(crate::prop_looks::wrecks)
                    .run_if(resource_exists::<PresentedSnapshot>),
            );
    }
}

/// Whether `mesh` can be drawn again from its pieces: not skinned, dangly
/// or lightmapped (pieces carry neither bones nor lightmap uvs).
fn redrawn(mesh: &MeshIr) -> bool {
    mesh.skin.is_none()
        && mesh.dangly.is_none()
        && !(mesh.lightmap.is_some() && mesh.uv2.len() == mesh.positions.len())
}

/// `plains` (model frame) as one mesh in node `node`'s frame of `ir`.
fn joined(plains: &[Plain], ir: &ModelIr, node: usize) -> Mesh {
    let world = ir
        .nodes
        .get(node)
        .map_or(asset_odyssey::model::IDENTITY, |n| n.world);
    let to_node = crate::scene::to_mat4(&world).inverse();
    let (mut positions, mut normals, mut uvs, mut indices) = (vec![], vec![], vec![], vec![]);
    for plain in plains {
        let offset = positions.len() as u32;
        positions.extend(
            (plain.positions.iter())
                .map(|p| to_node.transform_point3(Vec3::from_array(*p)).to_array()),
        );
        normals.extend((plain.normals.iter()).map(|n| {
            (to_node.transform_vector3(Vec3::from_array(*n)))
                .normalize_or_zero()
                .to_array()
        }));
        uvs.extend_from_slice(&plain.uvs);
        indices.extend(plain.indices.iter().map(|i| i + offset));
    }
    let mut mesh = base_mesh(&positions, &normals, &uvs, RenderAssetUsages::RENDER_WORLD);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Where a panel hanging on `hinge` is drawn (model frame): turned about
/// its hinge the way that lowers it.
fn hung(piece: &Piece, (a, b, degrees): Hinge) -> Transform {
    let (a, b) = (Vec3::from_array(a), Vec3::from_array(b));
    let axis = (b - a).normalize_or_zero();
    let turn = |sign: f32| {
        Transform::from_translation(a)
            * Transform::from_rotation(Quat::from_axis_angle(axis, sign * degrees.to_radians()))
            * Transform::from_translation(-a)
    };
    let middle = Vec3::from_array(piece.middle);
    let (down, up) = (turn(1.0), turn(-1.0));
    if down.transform_point(middle).z <= up.transform_point(middle).z {
        down
    } else {
        up
    }
}

/// Damaged props drawn at their stage from their pieces: each mesh that
/// can be drawn as its remaining pieces in place (dented, a damage-class
/// one), the panels hanging on their hinges, and the chunks knocked off
/// while seen flying as debris.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw(
    mut commands: Commands,
    (presented, swap, scene, made): (
        Res<PresentedSnapshot>,
        Res<SceneSwap>,
        Res<SceneContent>,
        Option<Res<Made>>,
    ),
    changes: Res<PropChanges>,
    (mut fractures, mut looks, mut damage): (
        ResMut<Fractures>,
        ResMut<PropLooks>,
        ResMut<PropDamage>,
    ),
    scene_models: Query<(Entity, &SceneModel)>,
    (globals, children, parts): (Query<&GlobalTransform>, Query<&Children>, MeshParts),
    mut meshes: ResMut<Assets<Mesh>>,
    time: Res<Time>,
) {
    if damage.generation != swap.generation {
        *damage = PropDamage {
            generation: swap.generation,
            ..PropDamage::default()
        };
    }
    if !looks.current(swap.generation) {
        return;
    }
    let now_ms = time.elapsed_secs_f64() * 1000.0;
    for change in &changes.changes {
        if change.now.0 == look::DEAD {
            damage.wanted.remove(&change.index);
            // Broken while seen: its base smokes a while.
            if change.was.is_some_and(|was| was.0 != look::DEAD) {
                let (lo, hi) = BASE_SMOKE_S;
                let seconds = lo + (hi - lo) * f64::from(noise(&change.position, 0x5304e));
                let at = Vec3::from_array(change.position) + Vec3::Z * BASE_SMOKE_M;
                (damage.smoking).insert(change.index, (now_ms + seconds * 1000.0, at));
            }
            continue;
        }
        let stage = sim::destruction::stage(change.d, change.now.1);
        let first = change
            .was
            .map_or(stage, |was| sim::destruction::stage(change.d, was.1));
        damage.seen.entry(change.index).or_insert(first);
        if stage > 0 {
            (damage.wanted).insert(change.index, (stage, change.d, change.position));
        }
    }
    let made = made.as_deref();
    let newest = presented.1.last().map(|snapshot| &**snapshot);
    let due: Vec<(u32, u8, Destructible, [f32; 3])> = (damage.wanted.iter())
        .filter(|(index, (stage, ..))| damage.drawn.get(index).is_none_or(|d| d.stage < *stage))
        .map(|(&index, &(stage, d, feet))| (index, stage, d, feet))
        .collect();
    for (index, stage, d, feet) in due {
        let Some((model, root)) = model_of(index, (&scene, made), &scene_models) else {
            continue;
        };
        let ir = &scene.0.models[model].ir;
        let Some(fracture) = fractures.get(ir).cloned() else {
            fractures.cut(ir, &scene.0.textures, PIECES[d.size as usize]);
            continue;
        };
        let cut = fractures.plains(ir, &scene.0.textures);
        let whole: Vec<bool> = (cut.1.iter()).map(|&n| redrawn(&ir.meshes[n])).collect();
        let standing = base(&fracture.pieces, fracture.height, 0.0);
        let kept: Vec<usize> = (0..fracture.pieces.len())
            .filter(|i| {
                standing.contains(i)
                    || (fracture.pieces[*i].meshes.iter()).any(|(source, _)| !whole[*source])
            })
            .collect();
        let seed =
            shatter::seed_of(&ir.name) ^ u64::from(index).wrapping_mul(0x9e37_79b9_7f4a_7c15);
        let count = fracture.pieces.len().max(1) as f32;
        let middle: [f32; 3] = std::array::from_fn(|axis| {
            fracture.pieces.iter().map(|p| p.middle[axis]).sum::<f32>() / count
        });
        let from = damage.drawn.get(&index).map_or_else(
            || damage.seen.get(&index).copied().unwrap_or(0),
            |d| d.stage,
        );
        let chunks = chunks(
            &fracture.pieces,
            &kept,
            (d.size, stage, seed),
            middle,
            Some(from),
        );
        let hanging: BTreeSet<usize> = chunks.hanging.iter().map(|(i, _)| *i).collect();
        debug!(
            "prop #{index} ({}) chunks at stage {stage}: gone {:?}, hanging {hanging:?}, flying {:?}",
            ir.name, chunks.gone, chunks.flies
        );
        let pressed = match d.class {
            Class::Damage => dents(seed, stage, d.size, &cut.0),
            Class::Break => Vec::new(),
        };
        let drawn_parts: Vec<(Entity, usize, Handle<OdysseyMaterial>)> = (children
            .iter_descendants(root))
        .filter_map(|entity| {
            let (part, material) = parts.get(entity).ok()?;
            Some((entity, part.0, material.0.clone()))
        })
        .collect();
        let mut materials: BTreeMap<usize, Handle<OdysseyMaterial>> = (drawn_parts.iter())
            .map(|(_, number, material)| (*number, material.clone()))
            .collect();
        materials.extend(looks.copies(index));
        for (entity, number, _) in &drawn_parts {
            let mesh = &ir.meshes[*number];
            let Some(source) = cut.1.iter().position(|n| n == number) else {
                continue;
            };
            if !whole[source] {
                continue;
            }
            let mut plains: Vec<Plain> = (fracture.pieces.iter().enumerate())
                .filter(|(i, _)| !chunks.gone.contains(i) && !hanging.contains(i))
                .flat_map(|(_, piece)| &piece.meshes)
                .filter(|(from, _)| *from == source)
                .map(|(_, plain)| plain.clone())
                .collect();
            if dentable(mesh, keyed(ir, mesh.node)) {
                for &(_, centre, radius, depth) in pressed.iter().filter(|dent| dent.0 == source) {
                    for plain in &mut plains {
                        shatter::dent(&mut plain.positions, &plain.normals, centre, radius, depth);
                    }
                }
            }
            let rebuilt = joined(&plains, ir, mesh.node);
            commands
                .entity(*entity)
                .try_insert(Mesh3d(meshes.add(rebuilt)));
        }
        // Marks laid on what just left would float over the gap.
        let missing: BTreeSet<usize> = chunks.gone.union(&hanging).copied().collect();
        looks.drop_marks(
            index,
            |at| over_gap(at.to_array(), &fracture.pieces, &missing),
            &mut commands,
        );
        let drawn = damage.drawn.entry(index).or_default();
        for gone in drawn.hangers.drain(..) {
            commands.entity(gone).try_despawn();
        }
        for &(number, hinge) in &chunks.hanging {
            let piece = &fracture.pieces[number];
            let panel = commands
                .spawn((
                    hung(piece, hinge),
                    Visibility::default(),
                    Name::new(format!("{} panel {number}", ir.name)),
                    ChildOf(root),
                ))
                .id();
            piece_meshes(
                &mut commands,
                (&fracture.meshes_of(&mut meshes, number), &fracture.meshes, &materials),
                panel,
            );
            drawn.hangers.push(panel);
        }
        drawn.stage = stage;
        let flies: Vec<(usize, bool)> = chunks.flies.iter().map(|&i| (i, false)).collect();
        drawn.chunks = chunks;
        if flies.is_empty() {
            continue;
        }
        let Ok(global) = globals.get(root) else {
            continue;
        };
        let (from, blast) = looks
            .blow_on(index)
            .unwrap_or((blow_from(feet, &changes.creatures), false));
        lay_pieces(
            &mut commands,
            &mut meshes,
            looks.split(),
            (&swap.level, newest),
            (&fracture, &flies),
            (in_game(global), &ir.name, &materials),
            (from, blast),
            now_ms,
        );
    }
}

/// The lasting emitters wanted this frame (`PropEmitters`): each damaged
/// prop's (`emitters`) at seeded points over the top of its model's box,
/// each broken one's base smoking while it does; of them the nearest a
/// player within reach, at most `MOST_EMITTERS`.
#[allow(clippy::too_many_arguments)]
fn emit(
    (presented, swap, scene, made): (
        Res<PresentedSnapshot>,
        Res<SceneSwap>,
        Res<SceneContent>,
        Option<Res<Made>>,
    ),
    mut damage: ResMut<PropDamage>,
    mut lasting: ResMut<PropEmitters>,
    scene_models: Query<(Entity, &SceneModel)>,
    globals: Query<&GlobalTransform>,
    time: Res<Time>,
) {
    lasting.start(swap.generation);
    let now_ms = time.elapsed_secs_f64() * 1000.0;
    damage.smoking.retain(|_, (until, _)| *until > now_ms);
    let made = made.as_deref();
    let mut all: Vec<((u32, u8), Emitter, Vec3)> = Vec::new();
    for (&index, &(stage, d, _)) in &damage.wanted {
        let Some((model, root)) = model_of(index, (&scene, made), &scene_models) else {
            continue;
        };
        let Ok(global) = globals.get(root) else {
            continue;
        };
        let ir = &scene.0.models[model].ir;
        let boxed = crate::prop_looks::Boxed::of(ir, d.size, in_game(global));
        let seed = shatter::seed_of(&ir.name) ^ u64::from(index);
        for (slot, emitter) in emitters(d.material, stage, false).iter().enumerate() {
            let slot = slot as u8;
            // Over the middle half of its top.
            let roll = |salt: u64| noise(&[], seed ^ u64::from(slot) << 8 ^ salt) * 0.5 + 0.25;
            let (lo, hi) = (boxed.min, boxed.max);
            let top = Vec3::new(
                lo.x + (hi.x - lo.x) * roll(1),
                lo.y + (hi.y - lo.y) * roll(2),
                hi.z,
            );
            all.push(((index, slot), *emitter, boxed.game.transform_point(top)));
        }
    }
    for (&index, &(_, at)) in &damage.smoking {
        let smoke = emitters(Material::Debris, 0, true)[0];
        all.push(((index, u8::MAX), smoke, at));
    }
    let players: Vec<Vec3> = (presented.1.last().into_iter())
        .flat_map(|snapshot| &snapshot.entities)
        .filter(|entity| matches!(entity.kind, sim::Kind::Avatar(_)))
        .map(|entity| Vec3::from(entity.position))
        .collect();
    let candidates: Vec<(u32, f32)> = (all.iter().enumerate())
        .map(|(key, (_, _, at))| {
            let far = (players.iter())
                .map(|player| player.distance(*at))
                .fold(f32::INFINITY, f32::min);
            (key as u32, far)
        })
        .collect();
    let chosen = lit(&candidates, MOST_EMITTERS, EMITTERS_REACH_M);
    lasting.wanted = (chosen.into_iter())
        .map(|key| {
            let (slot, emitter, at) = all[key as usize];
            (slot, (emitter, at))
        })
        .collect();
}

#[cfg(feature = "probes")]
pub fn probes() -> sim::probe::Checks {
    use sim::probe::check;
    /// A cube of `side` m standing with its middle at `middle`.
    fn block(middle: [f32; 3], side: [f32; 3]) -> Piece {
        let hull = (0..8)
            .map(|corner| {
                std::array::from_fn(|i| {
                    middle[i] + side[i] / 2.0 * if corner >> i & 1 == 1 { 1.0 } else { -1.0 }
                })
            })
            .collect();
        Piece {
            middle,
            hull,
            ..Piece::default()
        }
    }
    // A console's pieces: a base row on the floor, a middle row, a top row,
    // and two thin side panels.
    let mut set: Vec<Piece> = (0..3)
        .flat_map(|row| {
            (0..3).map(move |col| {
                block(
                    [col as f32 * 0.4 - 0.4, 0.0, 0.2 + row as f32 * 0.4],
                    [0.4, 0.4, 0.4],
                )
            })
        })
        .collect();
    set.push(block([-0.7, 0.0, 0.6], [0.05, 0.4, 1.0]));
    set.push(block([0.7, 0.0, 0.6], [0.05, 0.4, 1.0]));
    let base = [0, 1, 2];
    let counts = [Size::Small, Size::Medium, Size::Large].map(|size| {
        let one = knocked(&set, &base, size, 1, 7);
        let two = knocked(&set, &base, size, 2, 7);
        (one.len(), two.len(), two.starts_with(&one))
    });
    let same = knocked(&set, &base, Size::Medium, 2, 7) == knocked(&set, &base, Size::Medium, 2, 7);
    let others: BTreeSet<Vec<usize>> = (0..8u64)
        .map(|index| {
            knocked(
                &set,
                &base,
                Size::Medium,
                2,
                7 ^ index.wrapping_mul(0x9e37_79b9_7f4a_7c15),
            )
        })
        .collect();
    let never_base = (0..32u64).all(|seed| {
        knocked(&set, &base, Size::Large, 2, seed)
            .iter()
            .all(|i| !base.contains(i))
    });
    let panel = block([0.0, 0.0, 0.5], [1.0, 0.05, 1.0]);
    let hung = hangs(&panel, [0.0, 1.0, 0.5]);
    let cube = hangs(&block([0.0; 3], [0.4; 3]), [0.0, 1.0, 0.0]);
    let middle = [0.0, 0.0, 0.6];
    let first = chunks(&set, &base, (Size::Large, 2, 7), middle, None);
    let live = chunks(&set, &base, (Size::Large, 2, 7), middle, Some(1));
    let at_one = chunks(&set, &base, (Size::Large, 1, 7), middle, Some(0));
    let thrown = remaining(set.len(), &first.gone);
    // Piece 6 (a top corner block, middle (-0.4, 0, 1.0)) gone.
    let missing = BTreeSet::from([6]);
    let gaps = [
        over_gap([-0.4, 0.2, 1.1], &set, &missing),
        over_gap([0.4, 0.2, 1.1], &set, &missing),
        over_gap([-0.4, 0.21, 1.0], &set, &missing),
    ];
    let flat_ok = |chunks: &Chunks| {
        chunks
            .hanging
            .iter()
            .all(|(i, _)| hangs(&set[*i], middle).is_some() && !chunks.gone.contains(i))
    };
    let kinds_of = |material, stage, broken| -> Vec<Kind> {
        emitters(material, stage, broken)
            .iter()
            .map(|e| e.kind)
            .collect()
    };
    use Kind::*;
    let kinds: Vec<(String, Vec<Kind>, Vec<Kind>)> = [
        (Material::Electronic, 0, false, vec![]),
        (Material::Electronic, 1, false, vec![Sparks]),
        (Material::Electronic, 2, false, vec![Arcs, Smoke]),
        (Material::Metal, 1, false, vec![Smoke]),
        (Material::Metal, 2, false, vec![Smoke]),
        (Material::Wood, 1, false, vec![Smoke]),
        (Material::Cloth, 2, false, vec![Fire, Smoke]),
        (Material::Organic, 2, false, vec![Fire, Smoke]),
        (Material::Stone, 1, false, vec![Dust]),
        (Material::Stone, 2, false, vec![Dust]),
        (Material::Ceramic, 0, true, vec![Smoke]),
    ]
    .into_iter()
    .map(|(material, stage, broken, want)| {
        (
            format!("{material:?} {stage} {broken}"),
            kinds_of(material, stage, broken),
            want,
        )
    })
    .collect();
    // 30 emitters, key k at k + 1 m, given out of order.
    let candidates: Vec<(u32, f32)> = (0..30u32).rev().map(|k| (k, k as f32 + 1.0)).collect();
    let capped = lit(&candidates, 24, 30.0);
    let swap = |generation| {
        let mut emitters = PropEmitters::default();
        emitters.start(1);
        emitters.wanted.insert((3, 0), (EMITTERS[0], Vec3::ZERO));
        emitters.start(generation);
        emitters.wanted.len()
    };
    let (left, kept) = (swap(2), swap(1));
    let mut stale = PropEmitters::default();
    stale.start(1);
    stale.wanted.insert((3, 0), (EMITTERS[0], Vec3::ZERO));
    let (old, new) = (stale.wanted_in(1).is_some(), stale.wanted_in(2).is_some());
    vec![
        (
            "damage_knocked_cumulative",
            check(
                counts.iter().all(|c| c.2)
                    && counts[0].0 == 1
                    && counts[0].1 == 2
                    && (1..=2).contains(&counts[1].0)
                    && counts[1].1 == counts[1].0 + 2
                    && (2..=3).contains(&counts[2].0)
                    && counts[2].1 == counts[2].0 + 3,
                format!(
                    "small, medium, large: (stage 1, stage 2, stage 2 holds stage 1) {counts:?} \
                     (want 1/2, 1–2/+2, 2–3/+3, all holding)"
                ),
            ),
        ),
        (
            "damage_knocked_same",
            check(
                same && others.len() > 1,
                format!(
                    "same seed same set {same}; 8 objects gave {} sets (want more than 1)",
                    others.len()
                ),
            ),
        ),
        (
            "damage_knocked_never_base",
            check(
                never_base,
                format!("no base piece knocked over 32 seeds: {never_base}"),
            ),
        ),
        (
            "damage_hangs_flat_only",
            check(
                hung.is_some_and(|(.., deg)| (25.0..=70.0).contains(&deg)) && cube.is_none(),
                format!(
                    "a 1×1×0.05 m panel hangs {hung:?} (want 25–70°), a cube {cube:?} (want none)"
                ),
            ),
        ),
        (
            "damage_first_seen",
            check(
                first.flies.is_empty()
                    && !first.gone.is_empty()
                    && flat_ok(&first)
                    && first.gone == live.gone
                    && !live.flies.is_empty()
                    && live.flies.iter().all(|i| !at_one.gone.contains(i)),
                format!(
                    "first seen at stage 2: gone {:?}, hanging {:?}, flying {:?} (want some gone, \
                     none flying); seen going 1→2: flying {:?}, stage 1 had gone {:?}",
                    first.gone,
                    first.hanging.iter().map(|(i, _)| i).collect::<Vec<_>>(),
                    first.flies,
                    live.flies,
                    at_one.gone
                ),
            ),
        ),
        (
            "damage_marks_over_gap",
            check(
                gaps == [true, false, true],
                format!(
                    "a mark on the gone corner's face, on a piece still drawn, 1 cm off the gone \
                     one: {gaps:?} (want [true, false, true])"
                ),
            ),
        ),
        (
            "damage_break_remaining",
            check(
                thrown.len() + first.gone.len() == set.len()
                    && thrown.iter().all(|i| !first.gone.contains(i)),
                format!(
                    "broken after stage 2 ({} gone): throws {thrown:?} (want every piece not gone)",
                    first.gone.len()
                ),
            ),
        ),
        (
            "emitters_by_material",
            check(
                kinds.iter().all(|(_, got, want)| got == want),
                format!("material, stage, broken → kinds: {kinds:?}"),
            ),
        ),
        (
            "emitters_capped",
            check(
                capped.len() == 24
                    && capped.iter().all(|key| *key < 24)
                    && capped.windows(2).all(|pair| pair[0] < pair[1]),
                format!(
                    "of 30 candidates 1–30 m (keys by distance): {capped:?} (want the 24 nearest, nearest first)"
                ),
            ),
        ),
        (
            "emitters_leave_with_area",
            check(
                left == 0 && kept == 1 && old && !new,
                format!(
                    "emitters wanted after a scene swap {left} (want 0); in the same scene {kept} \
                     (want 1); the old scene's read in it {old}, in the next {new} (want true, false)"
                ),
            ),
        ),
    ]
}
