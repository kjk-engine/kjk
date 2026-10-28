//! The first-person camera (`CameraMode::First`): the eye in the player's
//! own head as drawn, looking along the player's view, and the head hidden
//! so the body, arms and weapons show below it. JA had the like (`cg_fpls`,
//! left disabled): a headless body with the view at its head's bolt. JA's
//! first-person eye otherwise stands at the eye height over the origin;
//! here the head's eyes are measured from that point and the offset eased
//! (up and down slower, damping the stride's bob), so the view keeps the
//! body's sway without its animations' jolts. A step up or down eases out
//! as JA's client eases it, if quicker (`step_lag`).

use std::collections::BTreeMap;
use std::sync::Arc;

use asset_odyssey::hooks;
use asset_odyssey::model::{MeshIr, ModelIr};
use bevy::math::Affine3A;
use bevy::prelude::*;

use sim::{SimId, Snapshot, motion};

use crate::cast;
use crate::scene::{SceneModel, game_to_bevy, to_mat4};
use crate::viewer::LoadedScene;

/// How long the eye takes to close most (63%) of a change in where the
/// head has it from the eye height (seconds): across, and up and down.
const SETTLE_S: f32 = 0.06;
const SETTLE_UP_S: f32 = 0.15;
/// How long a step's rise takes to ease out (seconds): JA's `STEP_TIME` is
/// 0.2, which leaves the eye in the drawn chest on a flight of stairs.
const STEP_S: f32 = 0.12;
/// The most of the steps left to ease out (metres): half JA's
/// `MAX_STEP_CHANGE` (32 units), for the same reason.
const MOST_STEP_M: f32 = 16.0 * collision::UNIT;
/// The farthest the eye strays from the eye height over the origin
/// (metres): a roll's or a knockdown's head goes farther, the eye stays.
const STRAY_M: f32 = 0.5;
/// Where the eyes are in a head model's box: across its middle, this far
/// behind its front (metres), this share of its height up (tuning, the
/// owner's approved look, as are the distances below).
const EYES_BEHIND_FRONT: f32 = 0.05;
const EYES_UP: f32 = 0.6;
/// How far the eye leans out looking straight down (metres): past the
/// chest, so the legs and feet show.
const LEAN_M: f32 = 0.3;
/// How far the eye stands back along the view with a blade in hand
/// (metres): a swing's arm passes before the eyes, not through them.
const BLADE_BACK_M: f32 = 0.12;
/// A head of the body's own (`head_g`, no head model): the eyes this far
/// over its node (metres).
const EYES_OVER_NODE: f32 = 0.1;

/// How much smaller the own muzzle flashes play before the eyes than JA
/// plays them (its flashes are sized to be seen from its camera).
pub const FLASH_SCALE: f32 = 0.4;

/// Whose eyes the camera looks through this frame, if anyone's.
#[derive(Resource, Default)]
pub struct Eyes(pub Option<SimId>);

/// The eye's state from frame to frame.
#[derive(Default)]
pub(crate) struct FirstPerson {
    /// The eye's eased offset from the eye height over the origin (game
    /// space, metres), and where the eye was from it last frame, all told.
    offset: Option<Vec3>,
    shown: Option<Vec3>,
    /// The steps' rise still eased out (metres), and how long ago the
    /// last came (seconds).
    step: (f32, f32),
    /// What of the head is hidden.
    hidden: Vec<Entity>,
    /// Each head model's eyes in its root's space, by scene model.
    eyes: BTreeMap<usize, Option<HungEyes>>,
    /// Each body's head, by scene model.
    heads: BTreeMap<usize, Option<Head>>,
}

/// What the eye follows this frame.
pub(crate) struct Looking<'a> {
    /// The body: its scene model and its node entities.
    pub body: (usize, &'a [Entity]),
    /// The eye height over the origin (game space, metres).
    pub eye_height: Vec3,
    /// The view angles (degrees).
    pub angles: [f32; 3],
    /// The hand grips a blade.
    pub blade: bool,
    /// How far the player stepped up (or down) since the last frame
    /// (metres, `stepped`).
    pub stepped: f32,
    /// The seconds since the last frame.
    pub dt: f32,
}

/// The scene as drawn this frame: where an entity is (`place`), the head
/// models hung on a node (`hung`), and the models.
pub(crate) struct Drawn<'a> {
    pub place: &'a dyn Fn(Entity) -> Option<Affine3A>,
    pub hung: &'a dyn Fn(Entity) -> Vec<(Entity, usize)>,
    pub scene: &'a LoadedScene,
}

/// Where `entity` is drawn this frame (game space, metres), from its and
/// its ancestors' transforms as posed now (`GlobalTransform` is the last
/// frame's until they propagate).
pub(crate) fn place(
    entity: Entity,
    transforms: &Query<&mut Transform>,
    parents: &Query<&ChildOf>,
) -> Option<Affine3A> {
    let mut place = transforms.get(entity).ok()?.compute_affine();
    let mut at = entity;
    while let Ok(parent) = parents.get(at) {
        at = parent.parent();
        place = transforms.get(at).ok()?.compute_affine() * place;
    }
    Some(Affine3A::from_quat(game_to_bevy().inverse()) * place)
}

/// The models hung on `node` (a body's `headhook`), by their roots and
/// scene models.
pub(crate) fn hung(
    node: Entity,
    children: &Query<&Children>,
    models: &Query<(Entity, &SceneModel)>,
) -> Vec<(Entity, usize)> {
    children
        .get(node)
        .into_iter()
        .flatten()
        .filter_map(|&child| Some((child, models.get(child).ok()?.1.index)))
        .collect()
}

/// A body's head by its nodes: the `headhook` a head model hangs on, and
/// the head it has of its own; either or both.
#[derive(Debug, Clone, PartialEq)]
struct Head {
    hook: Option<usize>,
    own: Option<OwnHead>,
}

impl Head {
    fn of(body: &ModelIr) -> Option<Head> {
        let hook = body.node_index(hooks::HEAD_HOOK);
        let own = own_head(body);
        (hook.is_some() || own.is_some()).then_some(Head { hook, own })
    }
}

/// The head a body has of its own, modelled on it rather than hung on its
/// `headhook` (a helmet's, as a disguise's `N_SithSoldier`, a companion's
/// face, HK-47's): its head bone (`head_g`, else a creature's `head`), the
/// nodes that draw it, and where its eyes are in the bone's space. A node
/// draws the head when every mesh on it and under it does: one under the
/// bone, or one skinned mostly to the bone and what hangs under it.
#[derive(Debug, Clone, PartialEq)]
struct OwnHead {
    bone: usize,
    parts: Vec<usize>,
    eyes: Option<Vec3>,
}

fn own_head(body: &ModelIr) -> Option<OwnHead> {
    let bone = [hooks::HEAD, "head"]
        .iter()
        .find_map(|name| body.node_index(name))?;
    let under = |mut node: usize, above: usize| loop {
        if node == above {
            return true;
        }
        match body.nodes[node].parent {
            Some(parent) => node = parent,
            None => return false,
        }
    };
    let draws_head = |mesh: &MeshIr| match &mesh.skin {
        Some(skin) => {
            let (mut head, mut all) = (0.0, 0.0);
            for (joints, weights) in skin.vertex_joints.iter().zip(&skin.vertex_weights) {
                for (&slot, &weight) in joints.iter().zip(weights) {
                    all += weight;
                    if skin
                        .joints
                        .get(usize::from(slot))
                        .is_some_and(|&joint| under(joint, bone))
                    {
                        head += weight;
                    }
                }
            }
            head > all / 2.0
        }
        None => under(mesh.node, bone),
    };
    let mut parts = vec![bone];
    for mesh in body.meshes.iter().filter(|mesh| draws_head(mesh)) {
        let node = mesh.node;
        let whole = body
            .meshes
            .iter()
            .filter(|other| under(other.node, node))
            .all(draws_head);
        if whole && !under(node, bone) && !parts.contains(&node) {
            parts.push(node);
        }
    }
    let eyes = box_eyes(body, |mesh| draws_head(mesh)).map(|eyes| {
        let bone_world = to_mat4(&body.nodes[bone].world);
        bone_world.inverse().transform_point3(eyes.at)
    });
    Some(OwnHead { bone, parts, eyes })
}

/// How far `id` stepped up or down (metres) as drawn between `after` and
/// `until` (ms): its rise over the ticks its movement stepped, as far as
/// the view has blended into each.
pub(crate) fn stepped(history: &[Arc<Snapshot>], id: SimId, (after, until): (f64, f64)) -> f32 {
    let rise = |pair: &[Arc<Snapshot>]| {
        let [was, now] = pair else { return None };
        let (from, to) = (was.time_ms as f64, now.time_ms as f64);
        if to <= after || from >= until {
            return None;
        }
        let (was, now) = (
            cast::entity(was, id)?.player?,
            cast::entity(now, id)?.player?,
        );
        let shown = |ms: f64| ((ms - from) / (to - from).max(1.0)).clamp(0.0, 1.0) as f32;
        (now.events & motion::events::STEP != 0)
            .then(|| (now.origin[2] - was.origin[2]) * (shown(until) - shown(after)))
    };
    history.windows(2).filter_map(rise).sum::<f32>() * collision::UNIT
}

/// Where a model hung on a head's hook has its eyes, and how big it is
/// (the box's diagonal, metres): the head is the biggest of those hung
/// there (a mask or a hat is smaller).
#[derive(Debug, Clone, Copy)]
struct HungEyes {
    at: Vec3,
    size: f32,
}

/// Where a head model's eyes are in its root's space: from the box its
/// meshes fill at rest (models face +Y, Z up).
fn eyes_of(ir: &ModelIr) -> Option<HungEyes> {
    box_eyes(ir, |_| true)
}

/// Where the eyes are in the box `ir`'s meshes that `head` takes fill at
/// rest, in its root's space.
fn box_eyes(ir: &ModelIr, head: impl Fn(&MeshIr) -> bool) -> Option<HungEyes> {
    let (mut min, mut max) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for mesh in ir.meshes.iter().filter(|mesh| head(mesh)) {
        let world = to_mat4(&ir.nodes[mesh.node].world);
        for p in &mesh.positions {
            let v = world.transform_point3(Vec3::from_array(*p));
            min = min.min(v);
            max = max.max(v);
        }
    }
    (min.x <= max.x).then(|| HungEyes {
        at: Vec3::new(
            (min.x + max.x) / 2.0,
            max.y - EYES_BEHIND_FRONT,
            min.z + EYES_UP * (max.z - min.z),
        ),
        size: (max - min).length(),
    })
}

impl FirstPerson {
    /// The eye this frame (game space, metres): the eyes of the body's
    /// head, the biggest model hung on its `headhook`, else its own (stood
    /// back with a blade in hand), eased as an offset from the eye height,
    /// its steps eased out, and leant out along the view as it looks down.
    /// The head, hung and its own, is hidden. None for a body with no head.
    pub fn eye(
        &mut self,
        looking: &Looking,
        drawn: &Drawn,
        visibility: &mut Query<&mut Visibility>,
    ) -> Option<Vec3> {
        let &Looking {
            body: (model, nodes),
            eye_height,
            angles,
            blade,
            stepped,
            dt,
        } = looking;
        let head = self
            .heads
            .entry(model)
            .or_insert_with(|| Head::of(&drawn.scene.models.get(model)?.ir))
            .clone()?;
        let node = |index: usize| nodes.get(index).copied();
        let hook = head.hook.and_then(node);
        let hung = hook.map_or_else(Vec::new, |hook| (drawn.hung)(hook));
        let eyes = hung
            .iter()
            .filter_map(|&(root, model)| {
                let local = (*self
                    .eyes
                    .entry(model)
                    .or_insert_with(|| eyes_of(&drawn.scene.models.get(model)?.ir)))?;
                Some((root, local))
            })
            .max_by(|a, b| a.1.size.total_cmp(&b.1.size))
            .and_then(|(root, local)| Some((drawn.place)(root)?.transform_point3(local.at)));
        let own = head.own.as_ref();
        let bone = own.and_then(|own| node(own.bone));
        let eyes = eyes
            .or_else(|| Some((drawn.place)(bone?)?.transform_point3(own?.eyes?)))
            .or_else(|| {
                let at = (drawn.place)(hook.or(bone)?)?.translation;
                Some(Vec3::from(at) + Vec3::Z * EYES_OVER_NODE)
            });
        let mut parts: Vec<Entity> = hung.iter().map(|&(root, _)| root).collect();
        parts.extend(
            own.into_iter()
                .flat_map(|own| own.parts.iter().filter_map(|&n| node(n))),
        );
        self.hide(parts, visibility);
        let (pitch, yaw) = (angles[0].to_radians(), angles[1].to_radians());
        let ahead = Vec3::new(yaw.cos(), yaw.sin(), 0.0);
        let back = if blade { BLADE_BACK_M } else { 0.0 };
        let wanted = eyes.map_or(Vec3::ZERO, |eyes| {
            (eyes - eye_height).clamp_length_max(STRAY_M) - ahead * back
        });
        let offset = match self.offset {
            Some(was) if dt > 0.0 => {
                let closed = |settle: f32| 1.0 - (-dt / settle).exp();
                let (across, up) = (closed(SETTLE_S), closed(SETTLE_UP_S));
                was + (wanted - was) * Vec3::new(across, across, up)
            }
            _ => wanted,
        };
        self.offset = Some(offset);
        let lag = step_lag(&mut self.step, stepped, dt);
        // Looking down, the eye leans out over the chest, so the legs and
        // feet show past it.
        let lean = LEAN_M * pitch.sin().max(0.0);
        let shown = offset - Vec3::Z * lag + ahead * lean;
        self.shown = Some(shown);
        Some(eye_height + shown)
    }

    /// Where the eye was last frame from the eye height over the origin
    /// (metres), while the player looks through it.
    pub fn shown(&self) -> Option<Vec3> {
        self.shown
    }

    /// Hides `parts`, showing again what was hidden and is not among them.
    fn hide(&mut self, parts: Vec<Entity>, visibility: &mut Query<&mut Visibility>) {
        if parts == self.hidden {
            return;
        }
        self.show(visibility);
        for &part in &parts {
            if let Ok(mut shown) = visibility.get_mut(part) {
                *shown = Visibility::Hidden;
            }
        }
        self.hidden = parts;
    }

    /// Starts over in a new world, what it hid still known (a body kept
    /// from the last is shown again when another camera takes the view).
    pub fn restart(&mut self) {
        *self = FirstPerson {
            hidden: std::mem::take(&mut self.hidden),
            ..FirstPerson::default()
        };
    }

    /// The head shown again and the eye let go: another camera has the
    /// view.
    pub fn show(&mut self, visibility: &mut Query<&mut Visibility>) {
        for part in self.hidden.drain(..) {
            if let Ok(mut shown) = visibility.get_mut(part) {
                *shown = Visibility::Inherited;
            }
        }
        self.offset = None;
        self.shown = None;
        self.step = (0.0, 0.0);
    }
}

/// The first-person eye's lag behind a step up or down, caught up over
/// `STEP_S`. `step` is the step being caught up (its change in metres and
/// the seconds since it was taken); `stepped` is how far the player
/// stepped since the last frame (0 for none) and `dt` the frame's seconds.
/// A new step adds to what is left of the one before, within the most a
/// step can be; returns how far the eye lags this frame.
fn step_lag(step: &mut (f32, f32), stepped: f32, dt: f32) -> f32 {
    let (change, since) = *step;
    *step = if stepped == 0.0 {
        (change, since + dt)
    } else {
        let left = change * (1.0 - since / STEP_S).max(0.0);
        ((left + stepped).clamp(-MOST_STEP_M, MOST_STEP_M), 0.0)
    };
    let (change, since) = *step;
    change * (1.0 - since / STEP_S).max(0.0)
}
