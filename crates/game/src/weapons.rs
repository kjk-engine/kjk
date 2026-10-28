//! What the characters of `kjk play` hold: their weapon models in hand and
//! their lightsabers' blades, drawn from the presented snapshots.
//!
//! - **Models.** A creature's are its UTC's right and left weapon slot items
//!   (`viewer::CreatureLoad::place`, `creature::weapon_model`); a player's
//!   are its sheet's base items at model variation 1 until the inventory
//!   (I19).
//!   KOTOR hangs a weapon at the body's `rhand` or `lhand` node, its grip at
//!   the node and its blade up the node's +Z; on JA's own body (`--body ja`)
//!   it hangs at the `*r_hand` or `*l_hand` bolt, turned so its +Z runs
//!   along the bolt's −X, as a JA blade leaves its hilt.
//! - **Which.** The weapon of the hand the fighter has out (`cast::weapon`)
//!   is in the right hand, as JA holds every weapon; two blades fight
//!   together (the dual style), the second in the left hand. The other is
//!   not shown in a fight; out of one, someone holding theirs in hand holds
//!   the left slot's item in the left hand, as reone hangs each equipped
//!   item at its own hand (the Bith musicians' instruments, in the left
//!   slot beside an empty pistol in the right).
//! - **When.** Readied in a fight (`armed`): out while the saber is lit or
//!   away, the fighter is in combat (the sim's `combat_until`), fights
//!   someone or is fought, and once dead; otherwise the hand does not grip
//!   them. The players' characters carry theirs then (`Stow`), and once
//!   their player holsters them (H) until they attack; everyone else holds
//!   theirs in hand all the time, as reone hangs a creature's equipped
//!   weapons in its hands (`Hands::carries`). They go while an animation
//!   that hides equipped items plays (`acting::hides_items`) and as a
//!   conversation hides them (`Hide`). They appear and go at once.
//!   A character without a fighter keeps them out.
//! - **The grip.** A blade weapon out is held JA's way
//!   (`asset_ja::rig::Grip`, `cast::BodyRig::gripped`): the hand turned so
//!   the hilt points as JA's `single_1` (`dual_1` for a double-bladed one)
//!   points in JA's hand, where the sim has its blades. A double-bladed
//!   lightsaber hilt hangs by its middle (between its blades' starts), as
//!   JA's is held; KOTOR holds it by one end, its model's origin. Its
//!   blades start at its ends and reach as far as JA's (`dual_1`'s hilt is
//!   22 cm longer).
//! - **Guns.** A gun's shape is read once from its model (`GunShape`): its
//!   muzzle (the `bullethook`), barrel and up, and its underside along the
//!   barrel, cut from its triangles. `cast` holds a gun out up and aimed
//!   with it (`asset_ja::rig::Aim`): the barrel down its shots' way, the
//!   other hand under it at the foregrip (`GunShape::foregrip`), each shot
//!   kicking as its blaster does (`handling`).
//! - **Blades.** A lightsaber hilt carries each blade as a saber mesh with
//!   crossed planes of its texture (`kotor_blades`); they are dropped and
//!   JA's blade drawn instead, from where the planes leave the hilt, at JA's
//!   length and radius (`single_1`'s, `dual_1`'s), in the blade texture's
//!   colour (`SaberColor::of_texture`; the hilt's model variation picks it:
//!   1 blue, 2 red, 3 green, 4 yellow, 5 violet). A blade is lit
//!   as the sim lights it (`saber::blades::slots`: the saber on, a staff's
//!   second blade unless put out, none while the saber is away, none once
//!   its holder is dead), grows and shrinks as JA's do, and trails while the
//!   torso plays a saber move (`asset_ja::saber_trail_ms`). The gripping
//!   hand reaches for the sim's blade (`cast`, `Wielded::blades_in`), so it is drawn
//!   where it cuts.
//! - **Away.** A saber out of the hand (`SaberState::thrown`) hangs from the
//!   scene where the snapshots put it: flying out spun as JA spins it, back
//!   pointing away from its owner (`saber_flight`), lit and trailing (JA's
//!   `SABER_TRAIL_TIME`, 40 ms, a segment); knocked out it falls unlit,
//!   tumbling, then lies flat (stand-ins: JA tumbles it at random rates and
//!   bounces it).

use std::collections::BTreeMap;

use asset_ja::Bolt;
use asset_odyssey::hooks;
use asset_odyssey::model::{Matrix, ModelIr, Refusal};
use bevy::prelude::*;
use sim::combat::saber::throw::Flight;
use sim::combat::saber::{Kit, blades};
use sim::combat::sheet::{Blaster, Sheet};
use sim::combat::{Fighter, Sheath};
use sim::skeleton::Hand;
use vfs::resolver::Resolver;

use crate::scene::LiveBlades;
use crate::viewer::{self, BladeSpec, Placed, SaberColor};
use crate::viewer_ja::{self, Files};

/// Who holds weapons in a play scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Owner {
    /// A creature, by its object index (a module's, or one made in play).
    Creature(u32),
    Seat(u8),
}

/// A weapon model in the scene and, for a lightsaber hilt, its blades'
/// indices in `LoadedScene::blades` (a double-bladed hilt's upper first)
/// and where each leaves the hilt; for a gun, its shape (`GunShape`).
#[derive(Debug, Clone)]
pub struct Held {
    pub model: usize,
    pub name: String,
    pub blades: Vec<usize>,
    pub ends: Vec<BladeEnd>,
    pub gun: Option<GunShape>,
}

/// Where a blade leaves its hilt and which way it runs, in the hilt's space
/// (metres).
pub type BladeEnd = (Vec3, Vec3);

/// A character's weapon models by weapon slot (right, left).
pub type Arms = [Option<Held>; 2];

/// A weapon model spawned in the scene: its root entity, its name, its
/// blades' indices in `LiveBlades` and ends, and a gun's shape.
#[derive(Debug, Clone)]
pub struct Armed {
    pub root: Entity,
    pub name: String,
    pub blades: Vec<usize>,
    pub ends: Vec<BladeEnd>,
    pub gun: Option<GunShape>,
}

impl Held {
    /// The weapon as spawned, `models` being the scene's by index.
    pub fn armed(&self, models: &[Option<(Entity, Vec<Entity>)>]) -> Option<Armed> {
        let (root, _) = models.get(self.model)?.as_ref()?;
        Some(self.spawned(*root, 0))
    }

    /// The weapon spawned later as `root`, its blades `first_blade` on in
    /// `LiveBlades` (`scene::Spawner::live_blades`).
    pub fn spawned(&self, root: Entity, first_blade: usize) -> Armed {
        Armed {
            root,
            name: self.name.clone(),
            blades: self
                .blades
                .iter()
                .map(|blade| first_blade + blade)
                .collect(),
            ends: self.ends.clone(),
            gun: self.gun.clone(),
        }
    }
}

/// How far apart a gun's underside is read along its barrel (metres).
const PROFILE_STEP: f32 = 0.02;
/// The furthest from the grip the other hand holds a gun (metres).
const FOREGRIP: f32 = 0.3;

/// A gun model's shape as its hold reads it, in its own space (metres):
/// where its shots leave it (its `bullethook` node, KOTOR's, else its
/// `muzzlehook`; one that shoots nothing, a stun baton, its tip: the end of
/// its longest axis furthest from the grip, the model's origin, where a
/// first-person eye holds it up as a gun, `cast::gun_aim`); its barrel, the
/// model's axis it is longest along, toward
/// the muzzle; its up, of the other two axes the one the muzzle lies
/// furthest along, toward it (above the grip, the model's origin); and its
/// underside and middle across, every `PROFILE_STEP` along the barrel from
/// the grip to as far as it is held (`FOREGRIP`).
#[derive(Debug, Clone)]
pub struct GunShape {
    pub muzzle: Vec3,
    pub barrel: Vec3,
    pub up: Vec3,
    profile: Vec<(f32, f32)>,
}

impl GunShape {
    fn of(model: &ModelIr) -> Option<GunShape> {
        let hooked = ["bullethook", "muzzlehook"].iter().find_map(|hook| {
            let node = model.node(hook)?;
            Some(crate::scene::to_mat4(&node.world).w_axis.truncate())
        });
        // Its triangles in its own space.
        let triangles: Vec<[Vec3; 3]> = model
            .meshes
            .iter()
            .flat_map(|mesh| {
                let place = crate::scene::to_mat4(&model.nodes[mesh.node].world);
                let corner = move |i: u32| {
                    let p = mesh.positions.get(i as usize)?;
                    Some(place.transform_point3(Vec3::from_array(*p)))
                };
                let (corners, _) = mesh.indices.as_chunks::<3>();
                corners
                    .iter()
                    .filter_map(move |&[a, b, c]| Some([corner(a)?, corner(b)?, corner(c)?]))
            })
            .collect();
        let (lo, hi) = triangles.iter().flatten().fold(
            (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
            |(lo, hi), p| (lo.min(*p), hi.max(*p)),
        );
        let axes = [Vec3::X, Vec3::Y, Vec3::Z];
        let longest = |a: &Vec3, b: &Vec3| (hi - lo).dot(*a).total_cmp(&(hi - lo).dot(*b));
        let muzzle = match hooked {
            Some(muzzle) => muzzle,
            None => {
                let axis = axes.into_iter().max_by(longest)?;
                let (near, far) = (lo.dot(axis), hi.dot(axis));
                axis * if far.abs() >= near.abs() { far } else { near }
            }
        };
        let toward = |axis: Vec3| axis * if muzzle.dot(axis) < 0.0 { -1.0 } else { 1.0 };
        let barrel = toward(axes.into_iter().max_by(longest)?);
        let up = toward(
            axes.into_iter()
                .filter(|axis| axis.dot(barrel) == 0.0)
                .max_by(|a, b| muzzle.dot(*a).abs().total_cmp(&muzzle.dot(*b).abs()))?,
        );
        let side = barrel.cross(up);
        let steps = (muzzle.dot(barrel).min(FOREGRIP) / PROFILE_STEP)
            .ceil()
            .max(0.0) as usize;
        let profile = (0..=steps)
            .map(|step| {
                let at = step as f32 * PROFILE_STEP;
                // Where the triangles cross the plane square to the barrel
                // there: its lowest, and its middle across.
                let crossings = triangles.iter().flat_map(|t| {
                    (0..3).filter_map(move |e| {
                        let (a, b) = (t[e], t[(e + 1) % 3]);
                        let (da, db) = (a.dot(barrel) - at, b.dot(barrel) - at);
                        (da * db <= 0.0 && da != db).then(|| a + (b - a) * (da / (da - db)))
                    })
                });
                let (under, lo, hi) = crossings.fold(
                    (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY),
                    |(under, lo, hi), p| {
                        let across = p.dot(side);
                        (under.min(p.dot(up)), lo.min(across), hi.max(across))
                    },
                );
                if under.is_finite() {
                    (under, (lo + hi) / 2.0)
                } else {
                    (muzzle.dot(up), 0.0)
                }
            })
            .collect();
        Some(GunShape {
            muzzle,
            barrel,
            up,
            profile,
        })
    }

    /// Where the other hand holds it, `share` of the way from the grip to
    /// the muzzle but no further than `FOREGRIP` (an arm's comfortable
    /// reach past the other hand): under it there, in its middle across.
    pub fn foregrip(&self, share: f32) -> Vec3 {
        let at = (self.muzzle.dot(self.barrel) * share).min(FOREGRIP);
        let step = ((at / PROFILE_STEP).round().max(0.0) as usize).min(self.profile.len() - 1);
        let (under, across) = self.profile[step];
        self.barrel * at + self.up * under + self.barrel.cross(self.up) * across
    }
}

/// How a gun is held up and kicks (`asset_ja::rig::Aim`, `cast`): where the
/// other hand holds it, as a share of the way from the grip to the muzzle
/// (none for one hand); how far the wrist goes forward of where JA's pose
/// has it (metres); a shot's kick up (degrees) and back (metres), and how
/// long it takes to settle (ms).
#[derive(Debug, Clone, Copy)]
pub struct Handling {
    pub support: Option<f32>,
    pub forward: f32,
    pub kick_degrees: f32,
    pub kick_back: f32,
    pub settle_ms: f32,
}

impl Handling {
    /// What shoots nothing: one hand, no kick.
    pub const STILL: Handling = Handling {
        support: None,
        forward: 0.0,
        kick_degrees: 0.0,
        kick_back: 0.0,
        settle_ms: 1.0,
    };
}

/// Each blaster's `Handling`: the heavier the shot, the bigger and slower
/// the kick; a repeater's small and quick, at its cadence.
pub fn handling(blaster: Blaster) -> Handling {
    let (support, forward, kick_degrees, kick_back, settle_ms) = match blaster {
        Blaster::Pistol => (None, 0.0, 6.0, 0.03, 160.0),
        Blaster::Rifle => (Some(0.5), 0.0, 4.0, 0.04, 150.0),
        Blaster::Repeater => (Some(0.45), 0.08, 1.5, 0.015, 80.0),
        Blaster::Bowcaster => (Some(0.5), 0.0, 7.0, 0.05, 220.0),
        Blaster::Disruptor => (Some(0.5), 0.0, 9.0, 0.06, 280.0),
        Blaster::Ion => (Some(0.5), 0.0, 5.0, 0.04, 180.0),
        Blaster::Sonic => (Some(0.5), 0.0, 4.0, 0.035, 170.0),
    };
    Handling {
        support,
        forward,
        kick_degrees,
        kick_back,
        settle_ms,
    }
}

/// A JA saber the KOTOR hilts' blades take after: each blade's length and
/// the radius (units), and where each leaves its hilt (metres) and which
/// way it runs (the first's is what the hand's grip aims).
#[derive(Debug, Clone)]
pub struct JaSaber {
    pub lengths: Vec<f32>,
    pub radius: f32,
    pub ends: Vec<([f32; 3], [f32; 3])>,
}

impl JaSaber {
    /// The first blade's direction in the hilt.
    pub fn dir(&self) -> [f32; 3] {
        self.ends.first().map_or([0.0, 0.0, 1.0], |end| end.1)
    }
}

/// The JA sabers the sim's blades are (`sim::combat::saber::blades::SABERS`).
#[derive(Debug, Clone)]
pub struct Sabers {
    pub single: JaSaber,
    pub staff: JaSaber,
}

impl Sabers {
    pub fn load(files: &Files) -> Result<Sabers, String> {
        let one = |name: &str| -> Result<JaSaber, String> {
            let (saber, hilt) = viewer_ja::saber_and_hilt(files, name)?;
            let ends = (1..=saber.lengths.len())
                .map(|n| {
                    let tag = format!("*blade{n}");
                    asset_ja::bolt(&hilt, &tag)
                        .map(|bolt| viewer_ja::blade_axis(&bolt))
                        .ok_or(format!("{name}: no {tag}"))
                })
                .collect::<Result<_, _>>()?;
            Ok(JaSaber {
                lengths: saber.lengths,
                radius: saber.radius,
                ends,
            })
        };
        let [single, staff] = blades::SABERS;
        Ok(Sabers {
            single: one(single)?,
            staff: one(staff)?,
        })
    }

    pub fn of(&self, double: bool) -> &JaSaber {
        if double { &self.staff } else { &self.single }
    }
}

/// The JA shaders the blades of every colour are drawn with.
pub fn shaders() -> Vec<String> {
    SaberColor::ALL
        .iter()
        .flat_map(|color| [color.glow(), color.core()])
        .chain([SaberColor::TRAIL.to_string()])
        .collect()
}

/// Where a KOTOR hilt's blades run, and the hilt without them. A hilt's
/// blades are its saber meshes (the nodes' saber flag), which the model
/// reader refuses (with their boxes); most come with crossed planes drawn
/// flat with the same texture, some hilts (K1's and TSL's purple, gold,
/// cyan, viridian and silver ones) with saber meshes alone. Those planes
/// are dropped, and a JA blade is drawn for each group of planes and saber
/// mesh boxes: those above the grip run up +Z, those below it (a
/// double-bladed hilt's second blade) down −Z, each from its end nearest
/// the grip, as long. `None` for a model without blades (anything but a
/// lightsaber).
pub fn kotor_blades(mut ir: ModelIr) -> Option<(ModelIr, Vec<BladeSpec>)> {
    let textures: Vec<String> = ir.saber_textures().map(str::to_string).collect();
    let is_blade = |mesh: &asset_odyssey::model::MeshIr| {
        mesh.texture
            .as_ref()
            .is_some_and(|texture| textures.contains(texture))
    };
    let color = textures
        .first()
        .map_or(SaberColor::Blue, |texture| SaberColor::of_texture(texture));
    let place = |node: usize, p: [f32; 3]| {
        let w = ir.nodes[node].world;
        Vec3::from_array(
            [0, 1, 2].map(|r| w[r][0] * p[0] + w[r][1] * p[1] + w[r][2] * p[2] + w[r][3]),
        )
    };
    // Each plane's points, and each saber mesh's box's corners (a hilt
    // whose planes are all saber meshes has only those).
    let planes = ir.meshes.iter().filter(|m| is_blade(m)).map(|mesh| {
        mesh.positions
            .iter()
            .map(|&p| place(mesh.node, p))
            .collect::<Vec<_>>()
    });
    let boxes = ir.refusals.iter().filter_map(|(name, refusal)| {
        let Refusal::SaberBlade { bounds: b, .. } = refusal else {
            return None;
        };
        let node = ir.node_index(name)?;
        let corners = (0..8).map(|c: usize| {
            let pick = |axis: usize| b[axis + 3 * ((c >> axis) & 1)];
            place(node, [pick(0), pick(1), pick(2)])
        });
        Some(corners.collect())
    });
    // (low, high) of the blades above and below the grip.
    let mut groups = [(Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)); 2];
    for points in planes.chain(boxes) {
        let centre = points.iter().sum::<Vec3>() / points.len().max(1) as f32;
        let group = &mut groups[usize::from(centre.z < 0.0)];
        for p in points {
            group.0 = group.0.min(p);
            group.1 = group.1.max(p);
        }
    }
    let blades: Vec<BladeSpec> = groups
        .iter()
        .enumerate()
        .filter(|(_, (low, high))| low.z <= high.z)
        .map(|(below, (low, high))| {
            let centre = (*low + *high) / 2.0;
            let (start, dir) = if below == 0 {
                (low.z, 1.0)
            } else {
                (high.z, -1.0)
            };
            BladeSpec::new(
                [centre.x, centre.y, start],
                [0.0, 0.0, dir],
                (high.z - low.z) / asset_ja::UNIT,
                3.0,
                color,
            )
        })
        .collect();
    if blades.is_empty() {
        return None;
    }
    ir.meshes.retain(|m| !is_blade(m));
    Some((ir, blades))
}

/// Readies one character's weapon models (`slots`, by model index) to be
/// held: a lightsaber hilt loses its planes and gets a live JA blade for
/// each (see the module documentation), pushed onto `blades`.
pub fn arm(
    models: &mut [Placed],
    blades: &mut Vec<BladeSpec>,
    slots: [Option<usize>; 2],
    sabers: &Sabers,
    notes: &mut Vec<String>,
) -> Arms {
    slots.map(|model| {
        let placed = models.get_mut(model?)?;
        let mut held = Held {
            model: model?,
            name: placed.ir.name.clone(),
            blades: Vec::new(),
            ends: Vec::new(),
            gun: GunShape::of(&placed.ir),
        };
        if let Some((hilt, specs)) = kotor_blades(placed.ir.clone()) {
            placed.ir = hilt;
            let ja = sabers.of(specs.len() > 1);
            // A double-bladed hilt is held by its middle (`Wielded::new`);
            // each blade reaches as far past the middle as JA's does, its
            // hilt being shorter than JA's.
            let middle = match specs.as_slice() {
                [first, second, ..] => {
                    Some((Vec3::from_array(first.origin) + Vec3::from_array(second.origin)) / 2.0)
                }
                _ => None,
            };
            for (n, spec) in specs.into_iter().enumerate() {
                let longer = middle.zip(ja.ends.get(n)).map_or(0.0, |(middle, end)| {
                    (Vec3::from_array(end.0).length()
                        - Vec3::from_array(spec.origin).distance(middle))
                        / asset_ja::UNIT
                });
                let length = ja
                    .lengths
                    .get(n)
                    .or(ja.lengths.first())
                    .map(|length| length + longer);
                notes.push(format!(
                    "{}: blade {n} {} from {:?} along {:?}, KOTOR's planes {:.1} units, JA's {length:?}",
                    held.name, spec.glow, spec.origin, spec.dir, spec.length
                ));
                held.blades.push(blades.len());
                held.ends
                    .push((Vec3::from_array(spec.origin), Vec3::from_array(spec.dir)));
                blades.push(BladeSpec {
                    model: held.model,
                    length: length.unwrap_or(spec.length),
                    radius: ja.radius,
                    live: true,
                    ..spec
                });
            }
        }
        Some(held)
    })
}

/// The models of `sheet`'s weapons by slot, from KOTOR 1's tables and
/// models: the item `worn` in that hand's (its `ModelVariation`: a
/// lightsaber's colour), else (none worn, or its template not in
/// `resolver`) its base item's at variation 1 (a blue lightsaber).
pub fn sheet_models(
    resolver: &Resolver,
    cache: &mut BTreeMap<String, Option<ModelIr>>,
    sheet: &Sheet,
    worn: &[Option<String>; 2],
) -> Result<[Option<ModelIr>; 2], String> {
    let source = viewer::ResolverSource(resolver);
    let mut models = [None, None];
    for (slot, weapon) in sheet.weapons.iter().enumerate() {
        let name = weapon.and_then(|weapon| {
            worn[slot]
                .as_deref()
                .and_then(|template| asset_odyssey::creature::item_model(&source, template))
                .or_else(|| {
                    let base = i64::from(weapon.base_item);
                    asset_odyssey::creature::weapon_model(&source, base, 1)
                })
        });
        if let Some(name) = name {
            models[slot] = viewer::cached(cache, resolver, &name)?;
        }
    }
    Ok(models)
}

/// The KOTOR weapon nodes of the right and left hands.
pub const WEAPON_NODES: [&str; 2] = hooks::HANDS;

/// Where a body holds a weapon in each hand (right, left): the node it
/// hangs from and its place there. A KOTOR body hangs it at its weapon
/// node as it is; JA's own model (`ja_hands`, its hand bolts) at the bolt's
/// bone (node `bone + 1`), its +Z (the blade) along the bolt's −X as a JA
/// hilt's blade leaves it (turned a quarter about Y; how it rolls about
/// the blade is a guess).
pub fn hand_nodes(
    body: &ModelIr,
    ja_hands: Option<&[Option<Bolt>; 2]>,
) -> [Option<(usize, Matrix)>; 2] {
    const Z_TO_MINUS_X: Matrix = [
        [0.0, 0.0, -1.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    std::array::from_fn(|hand| match ja_hands {
        Some(bolts) => {
            let bolt = bolts[hand].as_ref()?;
            let local = asset_odyssey::model::multiply(&bolt.matrix, &Z_TO_MINUS_X);
            Some((bolt.bone + 1, local))
        }
        None => body
            .node_index(WEAPON_NODES[hand])
            .map(|node| (node, asset_odyssey::model::IDENTITY)),
    })
}

/// Where a body holds weapons: each hand's node (right, left) and a
/// weapon's place under it, and where each side carries one not in hand
/// (`Stow`).
#[derive(Clone, Copy, Default)]
pub struct Hands {
    /// Its weapons out of a fight are carried (the players' characters,
    /// who holster them). Everyone else's stay in hand, as reone hangs a
    /// creature's equipped weapons in its hands (the owner's ruling,
    /// 2026-10-19).
    pub carries: bool,
    pub held: [Option<(Entity, Transform)>; 2],
    pub hips: [Option<(Entity, Transform)>; 2],
    pub thighs: [Option<(Entity, Transform)>; 2],
    /// The thighs' holsters as body nodes (`stow_nodes`): where a draw's
    /// hand takes the gun from (`draw_anim`).
    pub holsters: [Option<(usize, Transform)>; 2],
    pub backs: [Option<(Entity, Transform)>; 2],
    pub staffs: [Option<(Entity, Transform)>; 2],
}

/// Where a KOTOR body carries a weapon not in hand: a lightsaber clipped
/// to the belt at the hip, a pistol in a holster on the thigh (right, left
/// for the second hand's), a sword, vibroblade or long gun across the back
/// (two cross), a double-bladed one across the middle of the back.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stow {
    Hip,
    Thigh,
    Back,
    Staff,
}

impl Stow {
    /// Where `held` is carried, if anywhere: any gun of a `pair` (a pistol
    /// in each hand) on its thigh.
    fn of(held: sim::combat::saber::Held, pair: bool) -> Option<Stow> {
        use sim::combat::saber::Held;
        match held {
            Held::Blade(blade) if blade.saber => Some(Stow::Hip),
            Held::Blade(blade) if blade.double => Some(Stow::Staff),
            Held::Blaster(Blaster::Pistol) => Some(Stow::Thigh),
            Held::Blaster(_) if pair => Some(Stow::Thigh),
            Held::Blade(_) | Held::Blaster(_) => Some(Stow::Back),
            Held::Nothing | Held::Other => None,
        }
    }

    /// The body nodes its place (`place`) is measured from (right, left).
    fn nodes(self) -> [&'static str; 2] {
        match self {
            Stow::Hip => [hooks::PELVIS, hooks::PELVIS],
            Stow::Thigh => ["rthigh_g", "lthigh_g"],
            Stow::Back | Stow::Staff => [hooks::TORSO, hooks::TORSO],
        }
    }

    /// The node that moves the part of the body it lies against, where
    /// that is not the node it is measured from (`nodes`): the upper back,
    /// which turns over the lower torso as the body breathes (`pause1`,
    /// `stance`) and twists; hung from the lower, the weapon would stay
    /// still while the back moved under it.
    fn lies_on(self) -> Option<&'static str> {
        match self {
            Stow::Back | Stow::Staff => Some(hooks::UPPER_TORSO),
            Stow::Hip | Stow::Thigh => None,
        }
    }

    /// The right side's place under its node, tuning fitted by eye to the
    /// K1 bodies (the owner's approved look; neither reference carries a
    /// weapon away): x y z in metres (the node's x across the body, y forward,
    /// z up), then turns about x, y, z in degrees. A hilt hangs emitter
    /// down at the front of the hip; a pistol barrel down on the outer
    /// thigh, its grip at the hand; a blade flat across the back, its hilt
    /// over the right shoulder; a double-bladed one by its middle (where it
    /// is held) at the middle of the back, steeper.
    fn place(self) -> Transform {
        let [x, y, z, rx, ry, rz] = match self {
            Stow::Hip => [0.13, 0.08, -0.17, 160.0, 0.0, 0.0],
            Stow::Thigh => [0.11, -0.06, -0.11, -90.0, 0.0, 0.0],
            Stow::Back => [-0.15, -0.12, 0.38, 0.0, 150.0, 90.0],
            Stow::Staff => [0.0, -0.13, 0.12, 0.0, 160.0, 90.0],
        };
        Transform::from_translation(Vec3::new(x, y, z)).with_rotation(Quat::from_euler(
            EulerRot::XYZ,
            f32::to_radians(rx),
            f32::to_radians(ry),
            f32::to_radians(rz),
        ))
    }
}

/// Where `body` carries a `stow`ed weapon (right, left): its node and the
/// weapon's place under it, the left the right's mirrored across the
/// body's x; hung from the node it lies on (`Stow::lies_on`) where the
/// body has it, at the same place in the rest pose. None on JA's own
/// model.
pub fn stow_nodes(body: &ModelIr, ja: bool, stow: Stow) -> [Option<(usize, Transform)>; 2] {
    if ja {
        return [None, None];
    }
    let place = stow.place();
    let mirrored = Transform {
        translation: place.translation * Vec3::new(-1.0, 1.0, 1.0),
        rotation: Quat::from_xyzw(
            place.rotation.x,
            -place.rotation.y,
            -place.rotation.z,
            place.rotation.w,
        ),
        scale: place.scale,
    };
    let names = stow.nodes();
    let lies_on = stow.lies_on().and_then(|name| body.node_index(name));
    let rest = |node: usize| crate::scene::to_mat4(&body.nodes[node].world);
    std::array::from_fn(|side| {
        let node = body.node_index(names[side])?;
        let place = if side == 0 { place } else { mirrored };
        Some(match lies_on {
            Some(on) => (
                on,
                Transform::from_matrix(rest(on).inverse() * rest(node)) * place,
            ),
            None => (node, place),
        })
    })
}

/// Hangs a weapon model from `node` (a hand, the scene's root), placed at
/// `local` if given. A model gone by the time the commands run is a
/// warning, not a crash: what is drawn never stops the game.
fn hang(commands: &mut Commands, model: Entity, node: Entity, local: Option<Transform>) {
    commands.entity(model).queue_handled(
        move |mut model: EntityWorldMut| {
            model.insert(ChildOf(node));
            if let Some(local) = local {
                model.insert(local);
            }
        },
        bevy::ecs::error::warn,
    );
}

/// A character's weapons as drawn (see the module documentation).
pub struct Wielded {
    kit: Kit,
    hold: Hold,
    /// Its weapons out of a fight are carried (`Hands::carries`), else in
    /// hand.
    carries: bool,
    slots: [Option<Wield>; 2],
    /// The slot of the hand out, as last drawn.
    out: usize,
    /// What the last frame drew, for dumps.
    drawn: Drawn,
}

/// One weapon model.
struct Wield {
    root: Entity,
    name: String,
    blades: Vec<usize>,
    ends: Vec<BladeEnd>,
    gun: Option<GunShape>,
    /// The hand node it hangs from, and its place under it.
    hand: Entity,
    local: Transform,
    /// Where it is held and turns about, in its own space (a double-bladed
    /// hilt's middle).
    middle: Vec3,
    /// Away from the hand, hung from the scene's root.
    away: bool,
    /// Where it is carried while not in hand (`Stow`).
    belt: Option<(Entity, Transform)>,
    /// A gun's holster on the thigh, as a body node (`Hands::holsters`).
    holster: Option<(usize, Transform)>,
    /// The left slot's item out of a fight: the left hand, and its place
    /// there (`Hung::Rest`).
    rest: Option<(Entity, Transform)>,
    /// Where it hangs now.
    hung: Hung,
}

/// Where a weapon model hangs: the hand that holds it in a fight, where it
/// is carried (`Stow`), or the left slot's item at rest in the left hand.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Hung {
    Hand,
    Belt,
    Rest,
}

/// What a character's weapons drew last frame.
#[derive(Default, Clone)]
pub struct Drawn {
    pub shown: Vec<String>,
    pub lit: usize,
    pub away: Option<&'static str>,
    /// Each lit blade in hand: its place among the sim's blades
    /// (`blades::slots`) and its index in `LiveBlades`.
    pub blades: Vec<(usize, usize)>,
}

/// A character's state for its weapons at the drawn moment.
pub struct Moment<'a> {
    pub fighter: &'a Fighter,
    pub alive: bool,
    /// Sim ms.
    pub ms: f64,
    /// Its origin, and its saber's place away from the hand (units).
    pub origin: [f32; 3],
    pub thrown_at: Option<[f32; 3]>,
    /// What its torso plays.
    pub torso: &'static str,
    /// Its weapons are out (`armed`).
    pub armed: bool,
    /// What a conversation hides of them.
    pub hidden: Hide,
    /// Its holstered guns in hand or not, as a draw has them
    /// (`draw_anim`); none: as `in_hand` says.
    pub guns_in_hand: Option<bool>,
}

/// Which of a character's item models a conversation hides (S8): its
/// DLG's `UnequipHItem` the held ones, drawn neither in the hand nor where
/// they are carried, `UnequipItems` every one, the belted too. Visual only.
/// Ordered so that, for a body in two conversations, the larger wins.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Hide {
    None,
    Held,
    All,
}

/// Where a weapon model is drawn (`place`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Hand,
    /// Where it is carried (`Stow`), if the body carries it.
    Carried,
    Nowhere,
}

/// Where a weapon is drawn, as the body holds it (`in_hand`: its hand is
/// the one out, or both are) with its weapons `out` (or being lowered)
/// and what a conversation hides: a held weapon is hidden from `Held` up,
/// a carried one by `All`.
pub fn place(hidden: Hide, in_hand: bool, out: bool) -> Place {
    if hidden >= if in_hand { Hide::Held } else { Hide::All } {
        Place::Nowhere
    } else if in_hand && out {
        Place::Hand
    } else {
        Place::Carried
    }
}

/// How a sheet's two hands hold their weapons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hold {
    /// Two blades, fought with together.
    pub dual: bool,
    /// A pistol in each hand, both out (`sim::combat::pair`).
    pub pair: bool,
    /// A weapon in the left hand and none in the right: held in the left
    /// hand whatever the right does, as reone hangs the left slot's model
    /// on `lhand` whatever it wields (the wield is the right hand's: fists).
    pub left_only: bool,
}

impl Hold {
    pub fn of(sheet: &Sheet) -> Hold {
        let kit = Kit::of(sheet);
        Hold {
            dual: kit.blade(0).is_some() && kit.blade(1).is_some(),
            pair: sheet.pistol_pair(),
            left_only: sheet.weapons[0].is_none() && sheet.weapons[1].is_some(),
        }
    }

    /// Where slot `slot`'s weapon is drawn with the hand `out` out, the
    /// body holding its weapons (`held`, `in_hand`), readied (`armed`) or
    /// `lowering` them, and with a left hand to rest one in (`rest`); and
    /// whether it rests there. Out of a fight the left slot's item rests in
    /// the left hand while the right slot's is the one out; with nothing in
    /// the right (`left_only`), in a fight too.
    pub fn place(
        &self,
        (slot, out): (usize, usize),
        (held, armed, lowering, rest): (bool, bool, bool, bool),
        hidden: Hide,
    ) -> (Place, bool) {
        let resting = held && !lowering && out == 0 && rest && (self.left_only || !armed);
        let in_hand = self.dual || self.pair || slot == out || resting;
        (place(hidden, in_hand, held), resting)
    }
}

/// Whether a body's weapons are held in hand (`place`'s `out`): readied
/// (`armed`), a gun being lowered into its holster (`lowering`), or, for one
/// who does not `carry` them (`Hands::carries`), whenever no animation that
/// hides equipped items plays (`hiding`, `acting::hides_items`).
pub fn in_hand(armed: bool, lowering: bool, (carries, hiding): (bool, bool)) -> bool {
    armed || lowering || (!carries && !hiding)
}

/// Whether a fighter has its weapons readied at `ms` (sim ms), only in a
/// fight (`in_hand` says where they are drawn otherwise): its saber lit or
/// away, in combat (the sim's `combat_until`: 8 s after its last hit or
/// attack), 8 s after its own
/// last attack (`SaberState::out_until`), fighting someone, or someone
/// fighting it (`targeted`); not while it plays an animation that
/// hides its equipped items (`acting::hides_items`: Security, a mine's),
/// and once its player put them away (`Sheath::Holstered`) only while its
/// saber is still lit or away (a swing finishing) or its put-away plays.
/// The dead keep what they fell with.
pub fn armed(fighter: &Fighter, ms: f64, targeted: bool) -> bool {
    if !fighter.alive() {
        return true;
    }
    if fighter
        .acting
        .is_some_and(|acting| crate::acting::hides_items(&acting, ms))
    {
        return false;
    }
    // A put-away flourish holds the hilt until it is done.
    let held = fighter.saber.on
        || fighter.saber.thrown.is_some()
        || fighter.saber.current_move == sim::combat::saber::Move::PUTAWAY;
    held || !fighter.weapon_away(ms as u64) || (targeted && fighter.sheath == Sheath::Fight)
}

/// What weapons draw with: the transforms (shared with the bodies), the
/// models' visibility, the live blades, and the scene's root to hang a
/// saber away from the hand from.
pub struct Props<'a, 'w, 's, 'v, 'cw, 'cs> {
    pub visibility: &'a mut Query<'w, 's, &'v mut Visibility>,
    pub lives: &'a mut LiveBlades,
    pub commands: &'a mut Commands<'cw, 'cs>,
    pub root: Option<Entity>,
}

impl Wielded {
    /// `sheet`'s weapons in `arms`, each hung from the hand it is held in
    /// (`hands`).
    pub fn new(
        sheet: &Sheet,
        arms: [Option<Armed>; 2],
        hands: &Hands,
        commands: &mut Commands,
    ) -> Wielded {
        let kit = Kit::of(sheet);
        let hold = Hold::of(sheet);
        let Hold { dual, pair, .. } = hold;
        let mut arms = arms.map(Some);
        let slots = std::array::from_fn(|slot| {
            let armed = arms[slot].take().flatten()?;
            let side = usize::from((dual || pair) && slot == 1);
            let (hand, local) = hands.held[side]?;
            // A double-bladed hilt is held at its middle, where JA's is (its
            // model's origin, where KOTOR holds it, is at one end).
            let middle = match armed.ends.as_slice() {
                [(first, _), (second, _), ..] => (*first + *second) / 2.0,
                _ => Vec3::ZERO,
            };
            let local = local * Transform::from_translation(-middle);
            hang(commands, armed.root, hand, Some(local));
            // Not in hand, it is carried on its side (`Stow`).
            let stows = |stow| match stow {
                Stow::Hip => hands.hips,
                Stow::Thigh => hands.thighs,
                Stow::Back => hands.backs,
                Stow::Staff => hands.staffs,
            };
            let stow = Stow::of(kit.held(slot as u8), pair);
            let belt = stow
                .and_then(|stow| stows(stow)[usize::from(slot == 1)])
                .map(|(node, place)| (node, place * Transform::from_translation(-middle)));
            let holster =
                hands.holsters[usize::from(slot == 1)].filter(|_| stow == Some(Stow::Thigh));
            let rest = hands.held[1]
                .filter(|_| !dual && !pair && slot == 1)
                .map(|(node, place)| (node, place * Transform::from_translation(-middle)));
            Some(Wield {
                root: armed.root,
                name: armed.name,
                blades: armed.blades,
                ends: armed.ends,
                gun: armed.gun,
                hand,
                local,
                middle,
                away: false,
                belt,
                holster,
                rest,
                hung: Hung::Hand,
            })
        });
        Wielded {
            kit,
            hold,
            carries: hands.carries,
            slots,
            out: 0,
            drawn: Drawn::default(),
        }
    }

    /// The blade weapon out in `hand`, if one is: held with a grip.
    pub fn blade(&self, hand: u8) -> Option<sim::combat::saber::Blade> {
        self.kit.blade(hand.min(1))
    }

    pub fn dual(&self) -> bool {
        self.hold.dual
    }

    /// A pistol in each hand, both held up (`sim::combat::pair`).
    pub fn pair(&self) -> bool {
        self.hold.pair
    }

    /// Puts the weapons away for good: their models despawned; their
    /// blades' indices, to retire (`scene::RetiredBlades`).
    pub fn retire(&self, commands: &mut Commands) -> Vec<usize> {
        self.slots
            .iter()
            .flatten()
            .flat_map(|wield| {
                commands.entity(wield.root).despawn();
                wield.blades.iter().copied()
            })
            .collect()
    }

    /// Its weapon models.
    pub fn models(&self) -> impl Iterator<Item = Entity> + '_ {
        self.slots.iter().flatten().map(|wield| wield.root)
    }

    /// The weapons as armed, to hang in the hands again (`Wielded::new`).
    pub fn into_arms(self) -> [Option<Armed>; 2] {
        self.slots.map(|wield| {
            wield.map(|wield| Armed {
                root: wield.root,
                name: wield.name,
                blades: wield.blades,
                ends: wield.ends,
                gun: wield.gun,
            })
        })
    }

    /// The gun in the hand out as last drawn, if it is one with a muzzle:
    /// the model's root and where shots leave it, in the model's space
    /// (metres).
    pub fn gun(&self) -> Option<(Entity, Vec3)> {
        let wield = self.slots[self.out].as_ref()?;
        Some((wield.root, self.held_gun()?.1.muzzle))
    }

    /// The guns its shots leave, by the hand a shot names
    /// (`Shooting::hands`): a pair's right and left; else the gun in the
    /// hand out first.
    pub fn guns(&self) -> [Option<(Entity, Vec3)>; 2] {
        if !self.hold.pair {
            return [self.gun(), None];
        }
        std::array::from_fn(|slot| {
            let wield = self.slots[slot].as_ref()?;
            Some((wield.root, wield.gun.as_ref()?.muzzle))
        })
    }

    /// The gun in the hand out as last drawn, if it is one with a muzzle:
    /// which blaster, and its shape (a KOTOR body hangs it as it is at the
    /// hand's weapon node, so in that node's space).
    pub fn held_gun(&self) -> Option<(Blaster, &GunShape)> {
        let (blaster, shape) = self.held_up()?;
        Some((blaster?, shape))
    }

    /// What the hand out holds up as a gun: a gun (which blaster, and its
    /// shape), or an item that is neither gun nor blade (a stun baton) by
    /// its shape alone.
    pub fn held_up(&self) -> Option<(Option<Blaster>, &GunShape)> {
        let blaster = match self.kit.hands[self.out] {
            sim::combat::saber::Held::Blaster(blaster) => Some(blaster),
            sim::combat::saber::Held::Other => None,
            _ => return None,
        };
        Some((blaster, self.slots[self.out].as_ref()?.gun.as_ref()?))
    }

    /// Each held-up gun's holster (as `held_ups` orders them), for a body
    /// that carries its guns.
    pub fn holsters(&self) -> [Option<(usize, Transform)>; 2] {
        let slot = |slot: usize| self.slots[slot].as_ref().and_then(|wield| wield.holster);
        if self.hold.pair {
            [slot(0), slot(1)]
        } else {
            [slot(self.out), None]
        }
    }

    /// What each hand holds up (right, left): a pair's two guns, else the
    /// hand out's (`held_up`) first.
    pub fn held_ups(&self) -> [Option<(Option<Blaster>, &GunShape)>; 2] {
        if !self.hold.pair {
            return [self.held_up(), None];
        }
        std::array::from_fn(|slot| {
            let blaster = match self.kit.hands[slot] {
                sim::combat::saber::Held::Blaster(blaster) => blaster,
                _ => return None,
            };
            Some((Some(blaster), self.slots[slot].as_ref()?.gun.as_ref()?))
        })
    }

    pub fn drawn(&self) -> &Drawn {
        &self.drawn
    }

    /// The blades of the weapon in `hand` with `out` the hand out, as the
    /// weapon node holds them (`BladeEnd`, through the hilt's place under
    /// the node); a weapon without a lightsaber's blade (a sword) one from
    /// its grip up its +Z.
    pub fn blades_in(&self, out: u8, hand: Hand) -> Vec<BladeEnd> {
        let slot = match hand {
            Hand::Right if self.hold.dual => Some(0),
            Hand::Right => Some(usize::from(out.min(1))),
            Hand::Left => self.hold.dual.then_some(1),
        };
        let Some(wield) = slot.and_then(|slot| self.slots[slot].as_ref()) else {
            return Vec::new();
        };
        if wield.away {
            return Vec::new();
        }
        let ends = if wield.ends.is_empty() {
            &[(Vec3::ZERO, Vec3::Z)][..]
        } else {
            &wield.ends[..]
        };
        ends.iter()
            .map(|&(origin, along)| {
                (
                    wield.local.transform_point(origin),
                    wield.local.rotation * along,
                )
            })
            .collect()
    }

    /// Shows the weapons out, lights their blades and flies a saber away
    /// from the hand, at `at`.
    pub fn draw(&mut self, at: &Moment, transforms: &mut Query<&mut Transform>, props: &mut Props) {
        let fighter = at.fighter;
        let out = usize::from(fighter.hand.min(1));
        self.out = out;
        let saber = &fighter.saber;
        // The slot in the right hand, and whose blades a throw takes.
        let right = if self.hold.dual { 0 } else { out };
        let lit = blades::slots(&self.kit, saber, fighter.hand);
        let trail_ms = asset_ja::saber_trail_ms(at.torso);
        let mut drawn = Drawn::default();
        for (slot, wield) in self.slots.iter_mut().enumerate() {
            let Some(wield) = wield else { continue };
            let lowering = at.torso == "TORSO_DROPWEAP1";
            let hiding = (fighter.acting).is_some_and(|a| crate::acting::hides_items(&a, at.ms));
            let held = match (at.guns_in_hand, wield.holster) {
                (Some(drawn), Some(_)) => drawn && !hiding,
                _ => in_hand(at.armed, lowering, (self.carries, hiding)),
            };
            let (placed, resting) = self.hold.place(
                (slot, out),
                (held, at.armed, lowering, wield.rest.is_some()),
                at.hidden,
            );
            let shown = placed == Place::Hand;
            // Not in hand, it is carried (`Stow`), a lightsaber unlit.
            let belted = placed == Place::Carried
                && at.alive
                && saber.thrown.is_none()
                && wield.belt.is_some();
            if let Ok(mut visibility) = props.visibility.get_mut(wield.root) {
                visibility.set_if_neq(if shown || belted {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                });
            }
            let hung = if belted {
                Hung::Belt
            } else if resting && shown {
                Hung::Rest
            } else {
                Hung::Hand
            };
            if hung != wield.hung {
                let (to, local) = match hung {
                    Hung::Belt => wield.belt,
                    Hung::Rest => wield.rest,
                    Hung::Hand => None,
                }
                .unwrap_or((wield.hand, wield.local));
                hang(props.commands, wield.root, to, Some(local));
                wield.hung = hung;
            }
            if shown {
                drawn.shown.push(wield.name.clone());
            }
            let hand = if slot == right {
                Hand::Right
            } else {
                Hand::Left
            };
            let thrown = saber.thrown.filter(|_| slot == right);
            let flying = thrown.is_some_and(|thrown| {
                matches!(thrown.flight, Flight::Leaving | Flight::Out | Flight::Back)
            });
            let place = thrown.zip(at.thrown_at);
            // Away from the hand: hung from the scene where the snapshots
            // have it; back in it, hung from the hand again.
            match (place, wield.away, props.root) {
                (Some((thrown, units)), _, Some(root)) => {
                    if !wield.away {
                        hang(props.commands, wield.root, root, None);
                        wield.away = true;
                    }
                    let turn = away_turn(&thrown, at.origin, at.ms);
                    if let Ok(mut transform) = transforms.get_mut(wield.root) {
                        *transform =
                            Transform::from_translation(Vec3::from_array(units) * asset_ja::UNIT)
                                .with_rotation(turn)
                                * Transform::from_translation(-wield.middle);
                    }
                    drawn.away = Some(flight_name(thrown.flight));
                }
                (None, true, _) => {
                    hang(props.commands, wield.root, wield.hand, None);
                    if let Ok(mut transform) = transforms.get_mut(wield.root) {
                        *transform = wield.local;
                    }
                    wield.away = false;
                }
                _ => {}
            }
            for (n, &spec) in wield.blades.iter().enumerate() {
                let sim = lit.iter().position(|lit| {
                    lit.is_some_and(|lit| lit.id.hand == hand && lit.id.blade == n)
                });
                let on = at.alive && shown && (flying || sim.is_some());
                if let Some(sim) = sim.filter(|_| on && !flying) {
                    drawn.blades.push((sim, spec));
                }
                if let Some(live) = props.lives.0.get_mut(spec) {
                    *live = crate::scene::LiveBlade {
                        lit: on,
                        trail_ms: if flying { THROWN_TRAIL_MS } else { trail_ms },
                    };
                }
                drawn.lit += usize::from(on);
            }
        }
        self.drawn = drawn;
    }
}

/// JA's `SABER_TRAIL_TIME`: a flying saber's trail segments' life (ms).
const THROWN_TRAIL_MS: f32 = 40.0;

/// How fast a knocked-out saber tumbles as it falls (degrees a second, a
/// stand-in for JA's random 200 to 800 about each axis).
const TUMBLE: f32 = 500.0;

/// How a saber away from the hand is turned at `ms` (sim ms), its owner at
/// `owner` (units): see the module documentation.
fn away_turn(thrown: &sim::combat::saber::Thrown, owner: [f32; 3], ms: f64) -> Quat {
    let since = (ms - thrown.since as f64).max(0.0) as f32;
    let [x, y, _] = thrown.velocity;
    let heading = y.atan2(x);
    match thrown.flight {
        Flight::Leaving | Flight::Out => crate::saber_flight::turn_out(since, heading),
        Flight::Back => {
            crate::saber_flight::turn_back(Vec3::from_array(thrown.at) - Vec3::from_array(owner))
        }
        Flight::Falling => {
            let turned = (TUMBLE * since / 1000.0).to_radians();
            Quat::from_rotation_z(heading + turned) * Quat::from_rotation_y(turned)
        }
        Flight::Lying => Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
    }
}

fn flight_name(flight: Flight) -> &'static str {
    match flight {
        Flight::Leaving => "leaving",
        Flight::Out => "out",
        Flight::Back => "back",
        Flight::Falling => "falling",
        Flight::Lying => "lying",
    }
}

/// The view's weapon and stance rules (`kjk sim probes --only "weapons
/// view"`): a script's `ANIMATION_LOOPING_READY` shows each wield's ready
/// stance (`acting::stance_of`), a creature model's own; who holds weapons
/// in hand out of a fight (`in_hand`) and what a conversation then hides
/// (`place`); and, with K1 installed, a weapon template only a module has
/// (Endar Spire's pack vibroblade) read through that module's resources,
/// its base item's model where the template cannot be read.
#[cfg(feature = "probes")]
pub fn view_probes() -> sim::probe::Checks {
    use crate::acting::{LOOPING_READY, READY, ready_names, stance_of};
    use sim::probe::check;
    let mut checks: sim::probe::Checks = Vec::new();
    let wrong: Vec<u8> = (1..=9)
        .filter(|&wield| {
            stance_of(LOOPING_READY, wield) != [Some(READY + u16::from(wield)), Some(READY)]
        })
        .collect();
    let named = |creature| {
        ready_names(creature)
            .find(|(a, _)| *a == READY)
            .map(|(_, name)| name)
    };
    let others = [0, 5, 26, 104]
        .iter()
        .all(|&a| stance_of(a, 2) == [Some(a), None]);
    checks.push((
        "ready stance",
        check(
            wrong.is_empty() && named(true).as_deref() == Some("creadyr") && named(false).is_none() && others,
            format!(
                "19 plays g<wield>r1 for wields 1-9 (wrong: {wrong:?}), a creature model's creadyr ({:?}, none for a humanoid: {:?}); others their own: {others}",
                named(true),
                named(false)
            ),
        ),
    ));
    // (armed, lowering, carries, hiding) → in hand.
    let cases = [
        ((false, false, false, false), true),
        ((false, false, false, true), false),
        ((false, false, true, false), false),
        ((false, true, true, false), true),
        ((true, false, true, false), true),
        ((true, false, false, true), true),
    ];
    let wrong: Vec<_> = cases
        .iter()
        .filter(|&&((armed, lowering, carries, hiding), want)| {
            in_hand(armed, lowering, (carries, hiding)) != want
        })
        .collect();
    let npc = |hidden| place(hidden, true, in_hand(false, false, (false, false)));
    let hides = [npc(Hide::None), npc(Hide::Held), npc(Hide::All)];
    checks.push((
        "npc weapons in hand",
        check(
            wrong.is_empty() && hides == [Place::Hand, Place::Nowhere, Place::Nowhere],
            format!(
                "out of a fight an NPC holds its weapons, a player carries them, an NPC's hidden by Security's or a mine's animation (wrong: {wrong:?}); a conversation's UnequipHItem or UnequipItems hides an NPC's: {hides:?}"
            ),
        ),
    ));
    // In a fight with the right hand out: a left-only weapon rests in the
    // left hand; beside a right one (not a pair), it is put away.
    let fight = |left_only| {
        let hold = Hold {
            dual: false,
            pair: false,
            left_only,
        };
        hold.place((1, 0), (true, true, false, true), Hide::None)
    };
    let fights = [fight(true), fight(false)];
    checks.push((
        "left hand alone",
        check(
            fights == [(Place::Hand, true), (Place::Carried, false)],
            format!("left-only, beside a right: {fights:?}"),
        ),
    ));
    checks.push(("module weapon", module_weapon()));
    checks.push(("item on the floor", floor_item()));
    checks.push(("draw clip", draw_clips()));
    checks
}

/// The draws (`acting::draw_names`, `DRAW` + wield): `g1w1` … `g7w1` are
/// the `animations.2da` rows the exe's table of wield types names for its
/// draw (91, 132, 173, 214, 222, 236, 244), and K1's humanoid supermodel
/// `s_male02` has each but the two-handed sword's and the double blade's
/// (`g3w1`, `g4w1`, in no model of the install: such a body draws with no
/// clip, as the exe's check for one finds none).
#[cfg(feature = "probes")]
fn draw_clips() -> Result<String, String> {
    use sim::probe::check;
    const EXE_ROWS: [usize; 7] = [91, 132, 173, 214, 222, 236, 244];
    let Some(index) = crate::k1_index() else {
        return Ok("skipped: no K1 install".into());
    };
    let resolver = crate::k1_open(&index, None)?;
    let table = resolver
        .read("animations", fmt_odyssey::ResType::TWO_DA)
        .and_then(|bytes| fmt_odyssey::twoda::TwoDa::parse(&bytes).ok())
        .ok_or("no animations.2da")?;
    let column = table.column("name").ok_or("no name column")?;
    let rows: Vec<String> = EXE_ROWS
        .iter()
        .map(|&row| {
            table
                .get(row, column)
                .and_then(|cell| std::str::from_utf8(cell).ok())
                .map_or_else(String::new, |name| name.trim().to_ascii_lowercase())
        })
        .collect();
    let clips = session::clip_names(&viewer::ResolverSource(&resolver), "s_male02");
    let names: Vec<String> = crate::acting::draw_names()
        .take(7)
        .map(|(_, name)| name)
        .collect();
    let missing: Vec<&String> = names.iter().filter(|name| !clips.contains(name)).collect();
    check(
        rows == names && missing == ["g3w1", "g4w1"],
        format!(
            "the exe's draw rows name {rows:?}, the view's draws {names:?}; missing from s_male02: {missing:?}"
        ),
    )
}

/// K1's blaster pistol (`g_w_blstrpstl001`, the one Taris's duel drops)
/// lying on the floor: its model `w_blstrpstl_001`, its base item's
/// `rotateonground` 1, so turned a quarter about its Y axis it lies flat,
/// its height the least of its extents (standing as modelled, it is not).
#[cfg(feature = "probes")]
fn floor_item() -> Result<String, String> {
    use sim::probe::check;
    const TEMPLATE: &str = "g_w_blstrpstl001";
    let Some(index) = crate::k1_index() else {
        return Ok("skipped: no K1 install".into());
    };
    let resolver = crate::k1_open(&index, None)?;
    let (model, turn) =
        asset_odyssey::creature::floor_model(&viewer::ResolverSource(&resolver), TEMPLATE)
            .ok_or(format!("{TEMPLATE}: no floor model"))?;
    let parsed = viewer::parsed(&resolver, &model)?.ok_or(format!("no model {model}"))?;
    let ir = asset_odyssey::model::ModelIr::from_model(&parsed);
    let extents = |by: &asset_odyssey::model::Matrix| {
        let (mut low, mut high) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for mesh in &ir.meshes {
            let world = asset_odyssey::model::multiply(by, &ir.nodes[mesh.node].world);
            for p in &mesh.positions {
                let at = Vec3::from_array([0, 1, 2].map(|r| {
                    world[r][0] * p[0] + world[r][1] * p[1] + world[r][2] * p[2] + world[r][3]
                }));
                (low, high) = (low.min(at), high.max(at));
            }
        }
        high - low
    };
    let flat = |size: Vec3| size.z < size.x.min(size.y);
    let lying = extents(&asset_odyssey::item::ground_turn(turn));
    let standing = extents(&asset_odyssey::model::IDENTITY);
    check(
        model.eq_ignore_ascii_case("w_blstrpstl_001")
            && turn == 1
            && flat(lying)
            && !flat(standing),
        format!(
            "{TEMPLATE}: model {model}, rotateonground {turn}; lying {lying:.3?} m, as modelled {standing:.3?} m"
        ),
    )
}

/// `g_w_vbroshort002`, in `end_m01aa`'s archives only: its model through
/// the module's resources, its base item's through the install's alone.
#[cfg(feature = "probes")]
fn module_weapon() -> Result<String, String> {
    use sim::combat::sheet::Weapon;
    use sim::probe::check;
    const TEMPLATE: &str = "g_w_vbroshort002";
    let Some(index) = crate::k1_index() else {
        return Ok("skipped: no K1 install".into());
    };
    let (module, base) = (
        crate::k1_open(&index, Some("end_m01aa"))?,
        crate::k1_open(&index, None)?,
    );
    let item = |resolver: &Resolver| {
        asset_odyssey::creature::item_model(&viewer::ResolverSource(resolver), TEMPLATE)
    };
    let base_item = module
        .read(TEMPLATE, fmt_odyssey::ResType::UTI)
        .and_then(|bytes| fmt_odyssey::gff::Gff::parse(&bytes).ok())
        .and_then(|uti| uti.root.int("BaseItem"))
        .and_then(|row| u16::try_from(row).ok())
        .ok_or(format!("{TEMPLATE}: no UTI in end_m01aa"))?;
    let mut sheet = Sheet::commoner();
    sheet.weapons[0] = Some(Weapon {
        base_item,
        ..Weapon::UNARMED
    });
    let worn = [Some(TEMPLATE.to_string()), None];
    let name = |resolver: &Resolver| -> Result<Option<String>, String> {
        let [model, _] = sheet_models(resolver, &mut BTreeMap::new(), &sheet, &worn)?;
        Ok(model.map(|ir| ir.name.to_ascii_lowercase()))
    };
    let (through_module, through_base) = (name(&module)?, name(&base)?);
    let fallback = asset_odyssey::creature::weapon_model(
        &viewer::ResolverSource(&base),
        i64::from(base_item),
        1,
    )
    .map(|name| name.to_ascii_lowercase());
    let ok = item(&base).is_none()
        && item(&module).is_some()
        && through_module.is_some()
        && through_base.is_some()
        && through_base == fallback;
    check(
        ok,
        format!(
            "{TEMPLATE} (base item {base_item}): the install alone reads no template ({:?}), the module does ({:?}); held through the module {through_module:?}, through the install alone its base item's {through_base:?} (baseitems.2da: {fallback:?})",
            item(&base),
            item(&module)
        ),
    )
}

/// The templates `entity` (its sheet `sheet`) wears in its right and left
/// hands in `snapshot`, as the armory reads them to load their models.
pub fn worn(
    snapshot: &sim::Snapshot,
    objects: &sim::Instances,
    catalog: Option<&sim::inventory::Catalog>,
    (entity, sheet): (&sim::EntityState, &Sheet),
) -> [Option<String>; 2] {
    crate::armory::worn(snapshot, objects, catalog, (entity, sheet))
}
