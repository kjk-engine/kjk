//! KOTOR's weapon sounds (`weaponsounds.2da`) for every weapon but the
//! lightsaber, whose sounds stay JA's (`docs/COMBAT.md`, "Sounds"), but for
//! KOTOR's own clips and rounds (`clip_events`, `in_hand`):
//!
//! - **The row** a striker's blows sound by: its appearance's own
//!   (`appearance.2da` `soundapptype`, a row of `appearancesndset.2da`
//!   whose `weapon` is set: a Gamorrean's), else its weapon in hand's
//!   `baseitems.2da` `weaponmattype` (0 fists, claws and the stun baton, 2
//!   blades and staves, 3 large blades), else 0.
//! - **A hit** on a body: the row's column for what the struck wears, one of
//!   its two (`leather0`, `leather1`) at random: `forcefield` behind an
//!   energy shield, else its appearance's sound set's `armortype` (droids'
//!   `armor`, beasts' `leather`), else its body armour's base item's
//!   `armortype` (heavy armour's `armor`: the module's item catalog's, so a
//!   module's own armour too), else `leather`, as the executable picks it.
//! - **A parry**: the striker's row's `parry0`/`parry1`.
//! - **A swing**: `swingshort0–2` (a fast whoosh), `swinglong0–2` (a
//!   medium or strong one), `swingtwirl0–2` (a spin).
//! - **A blade on the level**: its row's `metal`, `wood` or `stone` by the
//!   surface met (`surfacemat.2da` `sound`: `MT`, `WD`, the rest).
//! - **A grenade landing** (each bounce, and where it comes to rest): its
//!   `grenadesnd.2da` row by the surface met (`surfacemat.2da`'s row): K1
//!   gives every surface but the non-walkable ones `fs_metal_droid1`.
//! - **A roll** (JA's, pressed crouching on the run): row 0's swing, the
//!   body's whoosh (`cb_sw_unarmed1`), as it starts.
//! - **A blaster's shot** (`Ammo`): its base item's `ammunitiontype`, a
//!   row of `ammunitiontypes.2da`: `shotsound0` as it fires (`shotsound1`
//!   for a Power Blast), `impactsound0`/`1` where it hits a body, as the
//!   executable keeps them on its bolt. Meeting the level (KOTOR's misses
//!   meet nothing): the impact sound's family for the surface, `metal`,
//!   `wood` or `stne` in place of its `leth1` (`cb_ht_blastmetal`), where
//!   the install has it, else the impact sound. A bolt deflected:
//!   `cb_pa_blast1`.
//!
//! Sounds are named `k1/<resref>` in the session's bank (`sound::load_kotor`).

use std::collections::{BTreeMap, BTreeSet};

use asset_ja::fx::Rng;
use bevy::prelude::*;
use fmt_odyssey::ResType;
use fmt_odyssey::twoda::TwoDa;
use sim::combat::Delivery;
use sim::inventory::Catalog;
use sim::module::ObjectKind;
use sim::{EntityState, Instances, Kind, Snapshot};
use vfs::resolver::Resolver;

/// What a struck character wears when nothing says (the executable's last
/// word).
const WORN: &str = "leather";
/// Behind an energy shield.
const SHIELDED: &str = "forcefield";

/// A bolt turned by a blade.
const DEFLECTED: &str = "cb_pa_blast1";
/// The surface families of a bolt's impact sound (`surfacemat.2da`'s
/// `sound`: `MT`, `WD`, the rest).
const BOLT_WALLS: [&str; 3] = ["metal", "wood", "stne"];
/// K1's Sonic bolts' impact, which no K1 archive has, and the one TSL's
/// table gives that row.
const MISSING_IMPACT: (&str, &str) = ("cb_ht_sonblclth1", "cb_ht_hvyblleth1");

/// A row of `ammunitiontypes.2da` (lowercase resrefs): the bolt's model,
/// its shot and impact sounds (each normal, then a Power Blast's), its
/// muzzle flash's model, and its impact sound's for each of `BOLT_WALLS`
/// the install has.
#[derive(Debug, Clone, Default)]
pub struct Ammo {
    pub models: [Option<String>; 2],
    pub shots: [Option<String>; 2],
    pub impacts: [Option<String>; 2],
    pub muzzle_flash: Option<String>,
    walls: [Option<String>; 3],
}

/// What a swing sounds as (its columns, three each).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Swing {
    Short,
    Long,
    Twirl,
}

/// The tables, read once a session.
#[derive(Resource)]
pub struct WeaponSounds {
    /// `weaponsounds.2da` by row: column (lowercase) to resref, and its
    /// label.
    rows: Vec<BTreeMap<String, String>>,
    labels: Vec<Option<String>>,
    /// `baseitems.2da` by row: `weaponmattype`, `armortype`.
    materials: Vec<Option<usize>>,
    armour_types: Vec<Option<String>>,
    /// `appearance.2da` `soundapptype` by row; `appearancesndset.2da`'s
    /// `armortype` and `weapon` by row.
    sound_sets: Vec<Option<usize>>,
    sets: Vec<(Option<String>, Option<usize>)>,
    /// `surfacemat.2da` `sound` by row.
    surfaces: Vec<Option<String>>,
    /// `grenadesnd.2da` `sound` by `surfacemat.2da` row: a grenade landing.
    grenade_lands: Vec<Option<String>>,
    /// `baseitems.2da` `ammunitiontype` by row, and `ammunitiontypes.2da`.
    ammo_types: Vec<Option<usize>>,
    ammo: Vec<Ammo>,
}

/// Where a struck body is met: the snapshot, its instances, and the
/// module's item catalog (`sim::Level`'s), for what it wears.
pub type Met<'a> = (&'a Snapshot, &'a Instances, Option<&'a Catalog>);

impl WeaponSounds {
    /// The tables from `resolver` (none: every row empty, nothing sounds).
    pub fn read(resolver: Option<&Resolver>) -> WeaponSounds {
        let table = |name: &str| resolver?.two_da(name);
        let sounds = table("weaponsounds");
        let rows = sounds.as_ref().map_or_else(Vec::new, |table| {
            (0..table.cells.len())
                .map(|row| {
                    (0..table.columns.len())
                        .filter_map(|column| {
                            let name = String::from_utf8_lossy(&table.columns[column])
                                .to_ascii_lowercase();
                            let sound = !matches!(name.as_str(), "label" | "pitchoffset");
                            Some((name, table.named(row, column).filter(|_| sound)?))
                        })
                        .collect()
                })
                .collect()
        });
        let base = table("baseitems");
        let sets = table("appearancesndset");
        WeaponSounds {
            rows,
            labels: names(sounds.as_ref(), "label"),
            materials: numbers(base.as_ref(), "weaponmattype"),
            armour_types: names(base.as_ref(), "armortype"),
            sound_sets: numbers(table("appearance").as_ref(), "soundapptype"),
            sets: names(sets.as_ref(), "armortype")
                .into_iter()
                .zip(numbers(sets.as_ref(), "weapon"))
                .collect(),
            surfaces: names(table("surfacemat").as_ref(), "sound"),
            grenade_lands: names(table("grenadesnd").as_ref(), "sound"),
            ammo_types: numbers(base.as_ref(), "ammunitiontype"),
            ammo: ammo(resolver, table("ammunitiontypes").as_ref()),
        }
    }

    /// Every sound its rows name (resrefs), for the bank.
    pub fn files(&self) -> Vec<String> {
        let files: BTreeSet<&String> = self
            .rows
            .iter()
            .flat_map(BTreeMap::values)
            .chain(self.grenade_lands.iter().flatten())
            .chain(self.ammo.iter().flat_map(|ammo| {
                ammo.shots
                    .iter()
                    .chain(&ammo.impacts)
                    .chain(&ammo.walls)
                    .flatten()
            }))
            .collect();
        let mut files: Vec<String> = files.into_iter().cloned().collect();
        files.push(DEFLECTED.to_string());
        files
    }

    /// The `ammunitiontypes.2da` row a weapon of `base_item` fires.
    pub fn ammo(&self, base_item: u16) -> Option<&Ammo> {
        let row = self.ammo_types.get(usize::from(base_item)).copied()??;
        self.ammo.get(row)
    }

    /// Every model the blasters' rows name: bolts and muzzle flashes.
    pub fn blaster_models(&self) -> Vec<String> {
        let models: BTreeSet<&String> = self
            .ammo
            .iter()
            .flat_map(|ammo| ammo.models.iter().chain([&ammo.muzzle_flash]))
            .flatten()
            .collect();
        models.into_iter().cloned().collect()
    }

    /// A shot of `base_item` firing (`power`: a Power Blast's).
    pub fn shot(&self, base_item: u16, power: bool) -> Option<String> {
        let ammo = self.ammo(base_item)?;
        let sound = ammo.shots[usize::from(power)]
            .as_ref()
            .or(ammo.shots[0].as_ref())?;
        Some(format!("k1/{sound}"))
    }

    /// A bolt of `base_item` striking a body.
    pub fn bolt_hit(&self, base_item: u16, power: bool) -> Option<String> {
        let ammo = self.ammo(base_item)?;
        let sound = ammo.impacts[usize::from(power)]
            .as_ref()
            .or(ammo.impacts[0].as_ref())?;
        Some(format!("k1/{sound}"))
    }

    /// A bolt of `base_item` meeting a surface of `surfacemat.2da` row
    /// `material`.
    pub fn bolt_wall(&self, base_item: u16, material: u16) -> Option<String> {
        let ammo = self.ammo(base_item)?;
        let code = self.surfaces.get(usize::from(material)).cloned().flatten();
        let family = match code.as_deref() {
            Some("mt") => 0,
            Some("wd") => 1,
            _ => 2,
        };
        let sound = ammo.walls[family].as_ref().or(ammo.impacts[0].as_ref())?;
        Some(format!("k1/{sound}"))
    }

    /// A bolt turned by a blade.
    pub fn deflected(&self) -> String {
        format!("k1/{DEFLECTED}")
    }

    /// A grenade landing on a surface of `surfacemat.2da` row `material`
    /// (none where the table has none: a pit, a non-walkable face).
    pub fn grenade_land(&self, material: u16) -> Option<String> {
        let resref = self.grenade_lands.get(usize::from(material))?.as_ref()?;
        Some(format!("k1/{resref}"))
    }

    /// The row of `weaponsounds.2da` labelled `label` (without case).
    pub fn labelled(&self, label: &str) -> Option<usize> {
        self.labels.iter().position(|name| {
            name.as_deref()
                .is_some_and(|name| name.eq_ignore_ascii_case(label))
        })
    }

    /// The row `entity`'s blows sound by (`snapshot`'s, `objects` its
    /// instances); `None` for a lightsaber.
    pub fn row(
        &self,
        entity: &EntityState,
        snapshot: &Snapshot,
        objects: &Instances,
    ) -> Option<usize> {
        let sheet = crate::cast::sheet(entity, objects);
        let hand = entity.fighter.map_or(0, |f| usize::from(f.hand).min(1));
        let weapon = sheet.weapons[hand].or(sheet.weapons[0]);
        if weapon.is_some_and(|weapon| matches!(weapon.delivery, Delivery::Saber(_))) {
            return None;
        }
        let own = appearance(entity, snapshot, objects)
            .and_then(|row| self.set(row))
            .and_then(|set| set.1)
            .filter(|row| *row != 0);
        Some(own.unwrap_or_else(|| {
            weapon
                .and_then(|weapon| {
                    self.materials
                        .get(usize::from(weapon.base_item))
                        .copied()
                        .flatten()
                })
                .unwrap_or(0)
        }))
    }

    /// The row a KOTOR clip's own events on `entity` sound by (its swings,
    /// parries and clashes, `clip_events`): its weapon in hand's
    /// `weaponmattype` (a lightsaber's row 1 too), else 0, as the
    /// executable reads it for them (no appearance's own row).
    pub fn in_hand(&self, entity: &EntityState, objects: &Instances) -> usize {
        let sheet = crate::cast::sheet(entity, objects);
        let hand = entity.fighter.map_or(0, |f| usize::from(f.hand).min(1));
        sheet.weapons[hand]
            .or(sheet.weapons[0])
            .and_then(|weapon| {
                self.materials
                    .get(usize::from(weapon.base_item))
                    .copied()
                    .flatten()
            })
            .unwrap_or(0)
    }

    /// A hit of row `row` on `struck`, one of its column's two.
    pub fn hit(&self, row: usize, struck: &EntityState, met: Met, rng: &mut Rng) -> Option<String> {
        let worn = self.worn_by(struck, met);
        self.pick(row, &worn, 2, rng)
            .or_else(|| self.pick(row, WORN, 2, rng))
    }

    /// Row `row`'s parry.
    pub fn parry(&self, row: usize, rng: &mut Rng) -> Option<String> {
        self.pick(row, "parry", 2, rng)
    }

    /// Row `row`'s clash (blades meeting).
    pub fn clash(&self, row: usize, rng: &mut Rng) -> Option<String> {
        self.pick(row, "clash", 2, rng)
    }

    /// Row `row`'s swing of kind `swing`.
    pub fn swing(&self, row: usize, swing: Swing, rng: &mut Rng) -> Option<String> {
        let column = match swing {
            Swing::Short => "swingshort",
            Swing::Long => "swinglong",
            Swing::Twirl => "swingtwirl",
        };
        self.pick(row, column, 3, rng)
    }

    /// Row `row` meeting a surface of `surfacemat.2da` row `material`.
    pub fn wall(&self, row: usize, material: u16, rng: &mut Rng) -> Option<String> {
        let code = self.surfaces.get(usize::from(material)).cloned().flatten();
        let column = match code.as_deref() {
            Some("mt") => "metal",
            Some("wd") => "wood",
            _ => "stone",
        };
        self.pick(row, column, 2, rng)
    }

    /// One of `count` numbered columns `stem0`, `stem1`, … of `row`, as the
    /// bank names it.
    fn pick(&self, row: usize, stem: &str, count: i32, rng: &mut Rng) -> Option<String> {
        let n = rng.roll_between(0, count - 1);
        let resref = self.rows.get(row)?.get(&format!("{stem}{n}"))?;
        Some(format!("k1/{resref}"))
    }

    fn set(&self, appearance: usize) -> Option<&(Option<String>, Option<usize>)> {
        let set = self.sound_sets.get(appearance).copied().flatten()?;
        self.sets.get(set)
    }

    /// What `struck` wears as its hits sound.
    fn worn_by(&self, struck: &EntityState, (snapshot, objects, catalog): Met) -> String {
        let now = snapshot.time_ms;
        if struck
            .fighter
            .is_some_and(|f| f.pack.shield.is_some_and(|shield| shield.until > now))
        {
            return SHIELDED.to_string();
        }
        let own = appearance(struck, snapshot, objects)
            .and_then(|row| self.set(row))
            .and_then(|set| set.0.clone());
        own.or_else(|| {
            let template = armour(struck, snapshot)?;
            let base = catalog?.get(&template.to_ascii_lowercase())?.base_item;
            self.armour_types.get(usize::from(base)).cloned().flatten()
        })
        .unwrap_or_else(|| WORN.to_string())
    }
}

/// `entity`'s `appearance.2da` row: a creature's, a player's persona's.
fn appearance(entity: &EntityState, snapshot: &Snapshot, objects: &Instances) -> Option<usize> {
    match entity.kind {
        Kind::Avatar(seat) => {
            sim::persona::dressed(snapshot, seat).map(|(row, _)| usize::from(row))
        }
        Kind::Object(ObjectKind::Creature, _) => {
            Some(usize::from(objects.of_entity(entity)?.body?.appearance))
        }
        _ => None,
    }
}

/// The template of the armour `entity` wears on its body now (the goods
/// tell every character's in the module).
fn armour(entity: &EntityState, snapshot: &Snapshot) -> Option<String> {
    snapshot
        .worn(entity.id.0, sim::inventory::slots::BODY)
        .map(str::to_string)
}

/// `ammunitiontypes.2da`'s rows, each impact sound's surface families
/// looked for in `resolver`.
fn ammo(resolver: Option<&Resolver>, table: Option<&TwoDa>) -> Vec<Ammo> {
    let has = |resref: &str| {
        resolver.is_some_and(|resolver| resolver.find(resref, ResType::WAV).is_some())
    };
    // K1's Sonic row names an impact the install lacks; TSL's names this.
    let found = |sound: Option<String>| match sound {
        Some(sound) if !has(&sound) && sound == MISSING_IMPACT.0 => {
            Some(MISSING_IMPACT.1.to_string())
        }
        sound => sound,
    };
    cells(table, |table, row| {
        let named = |column: &str| table.resref(row, column);
        let impacts = [named("impactsound0"), named("impactsound1")].map(found);
        let stem = impacts[0]
            .as_deref()
            .and_then(|sound| sound.strip_suffix("leth1"))
            .map(str::to_string);
        let walls = BOLT_WALLS.map(|family| {
            let sound = format!("{}{family}", stem.as_deref()?);
            has(&sound).then_some(sound)
        });
        Some(Ammo {
            models: [named("model0"), named("model1")],
            shots: [named("shotsound0"), named("shotsound1")],
            impacts,
            muzzle_flash: named("muzzleflash"),
            walls,
        })
    })
    .into_iter()
    .map(|ammo| ammo.unwrap_or_else(Ammo::default))
    .collect()
}

/// Column `column` of every row, as numbers.
fn numbers(table: Option<&TwoDa>, column: &str) -> Vec<Option<usize>> {
    cells(table, |table, row| {
        usize::try_from(table.int(row, column)?).ok()
    })
}

/// Column `column` of every row, as lowercase names.
fn names(table: Option<&TwoDa>, column: &str) -> Vec<Option<String>> {
    cells(table, |table, row| table.resref(row, column))
}

fn cells<T>(table: Option<&TwoDa>, read: impl Fn(&TwoDa, usize) -> Option<T>) -> Vec<Option<T>> {
    table.map_or_else(Vec::new, |table| {
        (0..table.cells.len()).map(|row| read(table, row)).collect()
    })
}
