//! Creatures' character sheets, the faction table and the Force powers'
//! costs, from the module's files (`sim::combat::Sheet`,
//! `sim::module::Repute`, `ModuleData::force_costs`).
//!
//! A creature's UTC gives its scores, classes and levels, feats, hit and
//! force points, faction, flags and its items (`items::held`); the class
//! tables (`classes.2da`, `cls_atk_*`, `cls_st_*`, `acbonus.2da`) give its
//! base attack bonus, saves and class defence; its equipped items' facts
//! (`items`) dress the sheet (`sim::inventory::dress`: the weapons in its
//! hands, its armour's defence and DEX cap, every worn item's guards), and
//! the items it carries give what it uses in a fight (grenades, medpacs,
//! shields, stims). A template or table that does not read leaves the
//! creature without a sheet: it fights as `Sheet::commoner` (the plan
//! counts them). Its skill list gives its ranks, its `PerceptionRange` how
//! far it sees and hears (`ranges.2da`; I17).

use std::collections::BTreeMap;
use std::sync::Arc;

use asset_odyssey::module::{Source, Title, localized};
use fmt_odyssey::ResType;
use fmt_odyssey::gff::{Gff, GffStruct};
use fmt_odyssey::tlk::Tlk;
use fmt_odyssey::twoda::TwoDa;
use sim::combat::Sheet;
use sim::combat::damage::ROUND_MS;
use sim::combat::sheet::{Guards, Rows, flags, races};
use sim::inventory::{self, Held, ItemFacts, slots};
use sim::module::{
    Global, ModuleData, ObjectData, ObjectKind, Opens, Placement, Repute, Senses, Strike,
};

use crate::items;

/// A tag as its data writes it, when that has capitals; empty otherwise
/// (`ObjectData::cased_tag`).
pub(crate) fn cased(bytes: &[u8]) -> String {
    sim::module::cased(&String::from_utf8_lossy(bytes))
}

/// The 2DAs sheets read, each parsed once per plan, the item templates
/// read (`items::facts`) and the creature models' attack timings (`hit_ms`).
pub(crate) struct Tables<'a> {
    pub source: &'a dyn Source,
    cache: asset_odyssey::tables::Tables,
    pub items: BTreeMap<String, Option<ItemFacts>>,
    hits: BTreeMap<String, u16>,
    rigs: BTreeMap<String, bool>,
}

impl<'a> Tables<'a> {
    pub fn new(source: &'a dyn Source) -> Tables<'a> {
        Tables {
            source,
            cache: asset_odyssey::tables::Tables::default(),
            items: BTreeMap::new(),
            hits: BTreeMap::new(),
            rigs: BTreeMap::new(),
        }
    }

    /// When model `name`'s melee attack lands (ms; `sim::module::Strike`):
    /// the `hit` event of its `m0a1`, else its `g0a1` (its own or a
    /// supermodel's), else half that animation, else half a round.
    fn hit_ms(&mut self, name: &str) -> u16 {
        if let Some(known) = self.hits.get(name) {
            return *known;
        }
        let mut chain = Vec::new();
        let mut next = Some(name.to_string());
        // A supermodel's supermodel, and so on (a loop stops at eight).
        for _ in 0..8 {
            let Some(model) = next.take() else {
                break;
            };
            let Some(read) = self
                .source
                .read(&model, ResType::MDL)
                .and_then(|bytes| fmt_odyssey::mdl::Model::timings(&bytes).ok())
            else {
                break;
            };
            next = read.0;
            chain.extend(read.1);
        }
        let attack = ["m0a1", "g0a1"]
            .iter()
            .find_map(|wanted| chain.iter().find(|timing| timing.name == *wanted));
        let seconds = attack.map_or(ROUND_MS as f32 / 2000.0, |attack| {
            attack
                .events
                .iter()
                .find(|(_, event)| event == "hit")
                .map_or(attack.length / 2.0, |(at, _)| *at)
        });
        let ms = (seconds * 1000.0).round().clamp(0.0, f32::from(u16::MAX)) as u16;
        self.hits.insert(name.to_string(), ms);
        ms
    }

    /// Whether model `name` has the humanoid skeleton JA's animations drive
    /// (`asset_ja::rig::is_humanoid`, from its node names): a creature model
    /// on it (a Gamorrean) is drawn retargeted, swinging JA's way.
    fn humanoid(&mut self, name: &str) -> bool {
        if let Some(known) = self.rigs.get(name) {
            return *known;
        }
        let humanoid = self
            .source
            .read(name, ResType::MDL)
            .and_then(|bytes| fmt_odyssey::mdl::Model::node_names(&bytes).ok())
            .is_some_and(|names| asset_ja::rig::humanoid_names(names.iter().map(String::as_str)));
        self.rigs.insert(name.to_string(), humanoid);
        humanoid
    }

    pub fn table(&mut self, name: &str) -> Option<Arc<TwoDa>> {
        self.cache.get(self.source, name)
    }

    /// Whether the install is TSL's: only its data has `itemcreate.2da`
    /// (its workbench's recipes). K1's `global.rim` ships an
    /// `upgradetypes.2da` too (six rows), so that one tells nothing.
    pub fn tsl(&mut self) -> bool {
        self.table("itemcreate").is_some()
    }

    /// A cell as an integer (`TwoDa::int`).
    pub fn int(&mut self, name: &str, row: usize, column: &str) -> Option<i64> {
        self.table(name)?.int(row, column)
    }

    fn float(&mut self, name: &str, row: usize, column: &str) -> Option<f32> {
        self.table(name)?.float(row, column)
    }

    /// A cell's text, trimmed (`TwoDa::text`).
    pub fn text(&mut self, name: &str, row: usize, column: &str) -> Option<String> {
        self.table(name)?.text(row, column)
    }
}

/// Fills in `module`'s creatures, its containers' items and its faction
/// table from `tables` (the module's resolver); how many creatures got a
/// sheet. A creature its GIT entry gives nothing of its own beyond its
/// place is its template as `creatures` has it (the library's, from the
/// store when imported), shared, its UTC not read again; another reads its
/// UTC: sheet, starting items, senses and the rest.
pub(crate) fn fill(
    tables: &mut Tables,
    module: &mut ModuleData,
    creatures: &BTreeMap<String, Arc<ObjectData>>,
) -> usize {
    module.repute = repute(tables);
    module.force_costs = force_costs(tables);
    if tables.tsl() {
        module.force = force_rules(tables);
    }
    module.traps = traps(tables);
    module.ranges = ranges(tables);
    module.globals = globals(tables);
    module.encounter_tables = encounter_tables(tables);
    // A placed trigger's DCs are its row's (KOTOR's load: only doors and
    // placeables keep their own `TrapDetectDC` and `DisarmDC`).
    let rows = module.traps.clone();
    module.change_each(
        |object| object.kind == ObjectKind::Trigger && object.trap.is_some(),
        |_, object| {
            if let Some(trap) = object.trap.as_mut()
                && let Some(row) = rows.get(usize::from(trap.row))
            {
                trap.detect_dc = row.detect_mod;
                trap.disarm_dc = row.disarm_mod;
            }
        },
    );
    let mut read = 0;
    for placement in &mut module.placements {
        let Placement::Own(object) = placement else {
            continue;
        };
        // A sound object's pitch variation (`SoundObjectGetPitchVariance`).
        if object.kind == ObjectKind::Sound {
            object.pitch_variation = tables
                .source
                .read(&object.template, ResType::UTS)
                .and_then(|bytes| Gff::parse(&bytes).ok())
                .and_then(|uts| uts.root.float("PitchVariation"))
                .unwrap_or(0.0);
            continue;
        }
        if object.kind != ObjectKind::Creature {
            continue;
        }
        if let Some(template) = creatures
            .get(&object.template)
            .filter(|template| as_template(template, object))
        {
            read += usize::from(template.sheet.is_some());
            module
                .templates
                .insert(object.template.clone(), template.clone());
            *placement = Placement::Template {
                template: object.template.clone(),
                position: object.position,
                yaw: object.yaw,
            };
            continue;
        }
        let Some(utc) = tables
            .source
            .read(&object.template, ResType::UTC)
            .and_then(|bytes| Gff::parse(&bytes).ok())
        else {
            continue;
        };
        creature(tables, &utc.root, object);
        read += usize::from(object.sheet.is_some());
    }
    items::fill_containers(tables, module);
    read
}

/// Whether the creature `placed` (as its GIT entry has it, before its UTC
/// is read) is `template` as it is: the entry gives it nothing of its own
/// beyond its place (everything else is read from the same UTC).
fn as_template(template: &ObjectData, placed: &ObjectData) -> bool {
    template.template == placed.template
        && template.tag == placed.tag
        && template.scripts == placed.scripts
        && template.conversation == placed.conversation
        && template.link == placed.link
        && template.geometry == placed.geometry
        && template.opens == placed.opens
        && template.state == placed.state
        && template.container == placed.container
}

/// What the game reads off a creature's `appearance.2da` row: its size,
/// its `moverate` as the `creaturespeed.2da` row it names (by `2daname`;
/// `MOVEMENT_SPEED_DEFAULT`, row 7, where none does), its walk and run
/// speeds (that row's, or the row its template's `walk_rate` names where
/// that is not 7), its head arc, and what its death leaves
/// (`sim::remains`): its remains' `bodybag.2da` row, its template's `bag`
/// unless that row has no appearance (row 0, every template's), then the
/// appearance's `body_bag`; how long until it is taken away and how long it
/// fades (seconds as ms).
fn body(tables: &mut Tables, row: usize, bag: u8, walk_rate: Option<i64>) -> sim::module::Body {
    const DEFAULT_SPEED: u8 = 7;
    let size = tables
        .int("appearance", row, "sizecategory")
        .map_or(0, |size| size.clamp(0, 255) as u8);
    let rate = tables.text("appearance", row, "moverate");
    let movement = rate
        .and_then(|rate| {
            let table = tables.table("creaturespeed")?;
            let column = table.column("2daname")?;
            (0..table.row_labels.len()).find(|&at| {
                table.get(at, column).is_some_and(|cell| {
                    String::from_utf8_lossy(cell)
                        .trim()
                        .eq_ignore_ascii_case(&rate)
                })
            })
        })
        .map_or(DEFAULT_SPEED, |at| at.min(255) as u8);
    let speeds = walk_rate
        .filter(|rate| *rate != i64::from(DEFAULT_SPEED))
        .and_then(|rate| usize::try_from(rate).ok())
        .unwrap_or(usize::from(movement));
    let mut speed = |column: &str| tables.float("creaturespeed", speeds, column).unwrap_or(0.0);
    let (walk_mps, run_mps) = (speed("walkrate"), speed("runrate"));
    let head_arc_h = tables.float("appearance", row, "head_arc_h").unwrap_or(0.0);
    let looks = tables
        .int("bodybag", usize::from(bag), "appearance")
        .is_some();
    let bag = match tables.int("appearance", row, "body_bag") {
        Some(own) if !looks => own.clamp(0, 255) as u8,
        _ => bag,
    };
    let mut ms = |column: &str| {
        tables
            .float("appearance", row, column)
            .map(|seconds| (seconds.max(0.0) * 1000.0).round() as u32)
    };
    sim::module::Body {
        appearance: row.min(usize::from(u16::MAX)) as u16,
        size,
        movement,
        walk_mps,
        run_mps,
        head_arc_h,
        bag,
        destroy_ms: ms("destroyobjectdelay").unwrap_or(sim::remains::DESTROY_MS),
        fade_ms: ms("fadedelayondeath"),
    }
}

/// `bodybag.2da` by row (empty where the game has none), named as the
/// game shows them (`talk`).
pub(crate) fn bags(tables: &mut Tables, talk: Option<&Tlk>) -> Vec<sim::remains::BagRow> {
    let rows = tables.table("bodybag").map_or(0, |table| table.cells.len());
    (0..rows)
        .map(|row| {
            let name = tables
                .int("bodybag", row, "name")
                .and_then(|strref| u32::try_from(strref).ok())
                .map(Title::StrRef);
            sim::remains::BagRow {
                appearance: tables
                    .int("bodybag", row, "appearance")
                    .and_then(|row| u16::try_from(row).ok()),
                name: name.map_or_else(String::new, |name| crate::objects::shown(&name, talk)),
                corpse: tables
                    .int("bodybag", row, "corpse")
                    .is_some_and(|flag| flag != 0),
            }
        })
        .collect()
}

/// A creature's facts from its UTC's root: its items, sheet, senses,
/// gender, subrace, personal space, challenge rating, name, classes and
/// whether it is interruptable.
fn creature(tables: &mut Tables, root: &GffStruct, object: &mut ObjectData) {
    object.interruptable = root.int("Interruptable").is_none_or(|flag| flag != 0);
    object.inventory = items::held(tables, root, false);
    object.sheet = sheet(tables, root, &object.inventory);
    object.senses = senses(tables, root);
    let byte = |label: &str| root.int(label).unwrap_or(0).clamp(0, 255) as u8;
    object.gender = byte("Gender");
    object.subrace = byte("SubraceIndex");
    let appearance = root
        .int("Appearance_Type")
        .and_then(|row| usize::try_from(row).ok());
    object.personal_space = appearance
        .and_then(|row| tables.float("appearance", row, "perspace"))
        .unwrap_or(0.0);
    object.strike = appearance.and_then(|row| strike(tables, row, &object.inventory));
    if let (Some(row), Some(strike), Some(sheet)) =
        (appearance, object.strike, object.sheet.as_mut())
    {
        swing_on_ja(tables, row, &strike, sheet);
    }
    object.body = appearance.map(|row| body(tables, row, byte("BodyBag"), root.int("WalkRate")));
    // Hurt as the template has it: `CurrentHitPoints` under `HitPoints`
    // (both without Constitution, which `MaxHitPoints` adds: Trask's 30,
    // 30, 36 is whole); `sim::install` starts the creature that much short.
    let hp = |label: &str| {
        root.int(label)
            .map(|hp| hp.clamp(0, i64::from(i32::MAX)) as i32)
    };
    object.hit_points = hp("CurrentHitPoints").zip(hp("HitPoints"));
    object.challenge = root.float("ChallengeRating").unwrap_or(0.0);
    object.multiplier_set = root
        .uint("MultiplierSet")
        .and_then(|set| u8::try_from(set).ok())
        .unwrap_or(0);
    let talk = tables.source.talk();
    let named = |label| crate::objects::shown(&localized(root.get(label)), talk.as_deref());
    object.name = crate::objects::full_name(named("FirstName"), named("LastName"));
    object.classes = root
        .list("ClassList")
        .iter()
        .filter_map(|class| {
            let row = u8::try_from(class.int("Class")?).ok()?;
            let levels = class.int("ClassLevel").unwrap_or(0).clamp(0, 255) as u8;
            Some((row, levels))
        })
        .collect();
}

/// A 2DA of `columns` and `rows` (each a row's cells, in order), made here.
fn made_table(columns: &[&str], rows: &[&[&str]]) -> Vec<u8> {
    let bytes = |text: &&str| text.as_bytes().to_vec();
    TwoDa {
        columns: columns.iter().map(bytes).collect(),
        row_labels: (0..rows.len())
            .map(|n| n.to_string().into_bytes())
            .collect(),
        cells: rows
            .iter()
            .map(|row| row.iter().map(bytes).collect())
            .collect(),
        column_separator: b'\t',
        row_separator: b'\t',
    }
    .write()
}

/// The creatures' speeds and head arc (`sim::module::Body`), for `kjk sim
/// probes`. From tables made here (K1's `creaturespeed.2da` rows 0 to 7,
/// one appearance moving `FAST` with a head arc of 30°): a UTC's
/// `WalkRate` 4 walks at 1.70 m/s and runs at 5.40, and 7 or none at the
/// appearance's `FAST` row (2.00, 6.00). With `k1` (K1's install), its own
/// table's player row walks at 3.20 m/s; without, that check is skipped.
pub fn speed_probes(k1: Option<&dyn Source>) -> Vec<(&'static str, Result<String, String>)> {
    use asset_odyssey::made::{Files, gff};
    use fmt_odyssey::gff::GffValue;
    let speeds = made_table(
        &["label", "2daname", "walkrate", "runrate"],
        &[
            &["PC_Movement", "PLAYER", "3.20", "5.40"],
            &["Immobile", "NOMOVE", "0.00", "0.00"],
            &["Very_Slow", "VSLOW", "0.75", "1.50"],
            &["Slow", "SLOW", "1.25", "2.50"],
            &["Normal", "NORM", "1.70", "5.40"],
            &["Fast", "FAST", "2.00", "6.00"],
            &["Very_Fast", "VFAST", "2.50", "6.50"],
            &["Default", "DEFAULT", "1.70", "5.40"],
        ],
    );
    let appearance = made_table(
        &["label", "moverate", "head_arc_h"],
        &[&["Probe", "FAST", "30"]],
    );
    let utc = |rate: Option<i32>| {
        let mut fields = vec![("Appearance_Type", GffValue::Word(0))];
        fields.extend(rate.map(|rate| ("WalkRate", GffValue::Int(rate))));
        gff(b"UTC ", fields)
    };
    let files = Files(BTreeMap::from([
        ("creaturespeed.2da".into(), speeds),
        ("appearance.2da".into(), appearance),
    ]));
    let mut tables = Tables::new(&files);
    let body = |tables: &mut Tables, rate: Option<i32>| {
        let root = Gff::parse(&utc(rate)).ok()?.root;
        let mut object = ObjectData::creature("probe", "probe", [0.0; 3], Sheet::dev_soldier());
        creature(tables, &root, &mut object);
        object
            .body
            .map(|body| (body.walk_mps, body.run_mps, body.head_arc_h))
    };
    let check = |ok: bool, detail: String| if ok { Ok(detail) } else { Err(detail) };
    let named = body(&mut tables, Some(4));
    let own = [body(&mut tables, Some(7)), body(&mut tables, None)];
    let rates = crate::library::move_rates(&files);
    let player = k1.map(|k1| {
        let mut tables = Tables::new(k1);
        (
            tables.float("creaturespeed", 0, "walkrate"),
            tables.text("creaturespeed", 0, "2daname"),
        )
    });
    vec![
        (
            "move_rates_rows",
            check(
                rates.player == Some((3.20, 5.40)) && rates.default == Some((1.70, 5.40)),
                format!("PLAYER and DEFAULT: {rates:?}"),
            ),
        ),
        (
            "walk_rate_row",
            check(
                named == Some((1.70, 5.40, 30.0)),
                format!("WalkRate 4: (walk, run, head arc) {named:?}"),
            ),
        ),
        (
            "walk_rate_default",
            check(
                own == [Some((2.00, 6.00, 30.0)); 2],
                format!("WalkRate 7, none: {own:?}"),
            ),
        ),
        (
            "k1_player_speed",
            match player {
                None => Ok("skipped: no K1 install (set KJK_KOTOR1 in .env)".into()),
                Some(row) => check(
                    row == (Some(3.20), Some("PLAYER".into())),
                    format!("K1's row 0: {row:?}"),
                ),
            },
        ),
    ]
}

/// How a creature attacks when its model has no humanoid skeleton
/// (`appearance.2da` `modeltype` S or L: its own bones, which JA's swings
/// cannot drive): its first creature weapon (right, left, bite), its
/// appearance's `hitdist` and its model's hit moment (`sim::module::Strike`).
/// `None` for a body of type B or F (the player bodies, the full-body
/// humanoids, astromechs), which swings JA's way.
fn strike(tables: &mut Tables, row: usize, held: &[Held]) -> Option<Strike> {
    let kind = tables.text("appearance", row, "modeltype")?;
    if !asset_odyssey::creature::creature_model(&kind) {
        return None;
    }
    let weapon = [
        slots::CREATURE_RIGHT,
        slots::CREATURE_LEFT,
        slots::CREATURE_BITE,
    ]
    .iter()
    .find_map(|slot| {
        let item = held.iter().find(|item| item.slot == Some(*slot))?;
        items::facts(tables, &item.template)?.weapon
    });
    let hit_ms = match tables.text("appearance", row, "race") {
        Some(model) if !model.is_empty() && model != "****" => {
            tables.hit_ms(&model.to_ascii_lowercase())
        }
        _ => (ROUND_MS / 2) as u16,
    };
    Some(Strike {
        weapon,
        hit_distance: tables.float("appearance", row, "hitdist").unwrap_or(0.0),
        hit_ms,
    })
}

/// A creature model on JA's skeleton (`modeltype` S or L whose model has the
/// humanoid bones, a Gamorrean: drawn retargeted) with nothing in its right
/// hand holds its strike's weapon there as a blade (its creature weapon,
/// else its bare body's blows, `Delivery::Melee`): it swings JA's way with
/// it, as one holding an axe or a sword does (TSL's armed Gamorreans), not
/// with its model's own attack clips. Its strike stays its creature's
/// (scripted rounds, `sim::module::Strike`).
fn swing_on_ja(tables: &mut Tables, row: usize, strike: &Strike, sheet: &mut Sheet) {
    if sheet.weapons[0].is_some() {
        return;
    }
    let Some(model) = tables
        .text("appearance", row, "race")
        .filter(|model| !model.is_empty() && model != "****")
    else {
        return;
    };
    if tables.humanoid(&model.to_ascii_lowercase()) {
        sheet.weapons[0] = Some(creature_blade(strike));
    }
}

/// `strike`'s weapon as a blade in the hand (`swing_on_ja`): its creature
/// weapon (a claw's base item is a melee one already), else its bare
/// body's blows as one.
fn creature_blade(strike: &Strike) -> sim::combat::Weapon {
    use sim::combat::{Weapon, sheet::Delivery};
    let weapon = strike.weapon.unwrap_or(Weapon::UNARMED);
    Weapon {
        delivery: Delivery::Melee,
        ..weapon
    }
}

/// The swing-on-JA rule's probes (`kjk sim probes --only creature_blades`),
/// on the installs this machine has: a Gamorrean (K1's `c_gammorean`, an
/// axe in hand; TSL's `g_gammorean01`, nothing) holds a blade and keeps its
/// strike, a kath hound (its own bones) strikes with nothing in hand. The
/// facts list every template the install lists that holds the creature
/// blade (K1's Taris Gamorreans are the modules' own: `kjk combat roster`).
pub fn blade_probes(
    k1: Option<&dyn Source>,
    tsl: Option<&dyn Source>,
) -> Vec<(&'static str, Result<String, String>)> {
    use sim::combat::sheet::Delivery;
    // Whether the right hand holds a blade, the creature blade (nothing in
    // its weapon slot), and whether it strikes.
    let held = |source: &dyn Source, template: &str| {
        let mut tables = Tables::new(source);
        let object = creature_template(&mut tables, template)?;
        let in_hand = (object.inventory.iter()).any(|item| item.slot == Some(slots::RIGHT_WEAPON));
        let blade =
            object.sheet?.weapons[0].is_some_and(|weapon| weapon.delivery == Delivery::Melee);
        Some((blade, blade && !in_hand, object.strike.is_some()))
    };
    let case = |name, game: &str, source: Option<&dyn Source>, cases: &[(&str, bool)]| {
        let Some(source) = source else {
            return (name, Ok(format!("skipped: no {game} install")));
        };
        let found: Vec<_> = cases
            .iter()
            .map(|(template, _)| held(source, template))
            .collect();
        let ok = cases.iter().zip(&found).all(|((_, blade), found)| {
            found.is_some_and(|found| (found.0, found.2) == (*blade, true))
        });
        let mut names = source.names(ResType::UTC);
        names.sort();
        names.retain(|name| held(source, name).is_some_and(|(_, creature, _)| creature));
        let detail = format!("{cases:?}: {found:?}; the templates with the blade: {names:?}");
        (name, if ok { Ok(detail) } else { Err(detail) })
    };
    vec![
        case(
            "k1_gamorreans_hold_blades",
            "K1",
            k1,
            &[("c_gammorean", true), ("c_khounda", false)],
        ),
        case(
            "tsl_gamorreans_hold_blades",
            "TSL",
            tsl,
            &[("g_gammorean01", true)],
        ),
    ]
}

/// The creature templates the party and scripts may make: every UTC the
/// install lists, and those `named`.
pub(crate) fn creature_catalog<'a>(
    tables: &mut Tables,
    named: impl IntoIterator<Item = &'a str>,
) -> BTreeMap<String, Arc<ObjectData>> {
    let mut names = tables.source.names(ResType::UTC);
    names.extend(named.into_iter().map(str::to_string));
    names.sort();
    names.dedup();
    names
        .into_iter()
        .filter_map(|name| Some((name.clone(), Arc::new(creature_template(tables, &name)?))))
        .collect()
}

/// A creature made from its template alone (`SpawnAvailableNPC`,
/// `CreateObject`): its tag and event scripts from the UTC, its facts as a
/// module's creature's; `None` when the template does not read.
pub(crate) fn creature_template(tables: &mut Tables, template: &str) -> Option<ObjectData> {
    let utc = Gff::parse(&tables.source.read(template, ResType::UTC)?).ok()?;
    let mut object = ObjectData {
        kind: ObjectKind::Creature,
        tag: utc.root.resref("Tag").unwrap_or_else(String::new),
        cased_tag: cased(utc.root.bytes("Tag").unwrap_or(&[])),
        template: template.to_string(),
        position: [0.0; 3],
        yaw: 0.0,
        link: None,
        geometry: Vec::new(),
        use_points: Vec::new(),
        opens: Opens::Never,
        state: 0,
        ways: sim::module::Ways::One,
        scripts: asset_odyssey::module::scripts(&utc.root),
        sheet: None,
        senses: None,
        classes: Vec::new(),
        gender: 0,
        subrace: 0,
        personal_space: 0.0,
        strike: None,
        challenge: 0.0,
        multiplier_set: 0,
        name: String::new(),
        inventory: Vec::new(),
        container: false,
        useable: false,
        store: None,
        lock: None,
        hit_points: None,
        toughness: sim::module::Toughness::default(),
        faction: 0,
        body: None,
        map_note: None,
        trap: None,
        conversation: utc.root.resref("Conversation").unwrap_or_else(String::new),
        interruptable: true,
        pitch_variation: 0.0,
        encounter: None,
    };
    creature(tables, &utc.root, &mut object);
    Some(object)
}

/// A placeable's facts from its UTP alone, as a script makes one
/// (`CreateObject`): its tag, name, scripts, conversation, whether it holds
/// items (and those), is used, and its lock. Its place is the script's.
pub(crate) fn placeable_template(tables: &mut Tables, template: &str) -> Option<ObjectData> {
    let utp = Gff::parse(&tables.source.read(template, ResType::UTP)?).ok()?;
    let root = &utp.root;
    let held = asset_odyssey::module::Lock::read(&[root]);
    let locked = root.int("Locked").is_some_and(|locked| locked != 0);
    let talk = tables.source.talk();
    let lower = |bytes: &[u8]| String::from_utf8_lossy(bytes).to_ascii_lowercase();
    Some(ObjectData {
        kind: ObjectKind::Placeable,
        tag: root.resref("Tag").unwrap_or_else(String::new),
        cased_tag: cased(root.bytes("Tag").unwrap_or(&[])),
        template: template.to_string(),
        position: [0.0; 3],
        yaw: 0.0,
        link: None,
        geometry: walkmesh_footprint(tables, root.int("Appearance")),
        use_points: placeable_model(tables, root.int("Appearance"))
            .map_or_else(Vec::new, |model| {
                crate::doors::use_points(tables.source, &model)
            }),
        opens: if locked { Opens::Locked } else { Opens::Never },
        state: 0,
        ways: sim::module::Ways::One,
        scripts: asset_odyssey::module::scripts(root),
        sheet: None,
        senses: None,
        classes: Vec::new(),
        gender: 0,
        subrace: 0,
        personal_space: 0.0,
        strike: None,
        challenge: 0.0,
        multiplier_set: 0,
        name: crate::objects::shown(&localized(root.get("LocName")), talk.as_deref()),
        inventory: crate::items::held(tables, root, true),
        container: asset_odyssey::module::container(root) == Some(true),
        useable: root.int("Useable").is_some_and(|useable| useable != 0),
        store: None,
        lock: (held != asset_odyssey::module::Lock::default()).then(|| sim::module::Lock {
            lockable: held.lockable,
            open_dc: held.open_dc,
            close_dc: held.close_dc,
            key: lower(&held.key),
            key_required: held.key_required,
            auto_remove_key: held.auto_remove_key,
        }),
        trap: asset_odyssey::module::TrapIr::read(&[root])
            .map(|found| crate::objects::trap(found, &held)),
        conversation: root.resref("Conversation").unwrap_or_else(String::new),
        hit_points: root.int("HP").map(|most| {
            let now = root.int("CurrentHP").unwrap_or(most);
            (
                i32::try_from(now).unwrap_or(i32::MAX),
                i32::try_from(most).unwrap_or(i32::MAX),
            )
        }),
        toughness: crate::objects::toughness(asset_odyssey::module::Toughness::read(&[root])),
        faction: root
            .int("Faction")
            .map_or(0, |faction| faction.clamp(0, 255) as u8),
        body: None,
        map_note: None,
        interruptable: true,
        pitch_variation: 0.0,
        encounter: None,
    })
}

/// The model `placeables.2da` names for a placeable's appearance.
fn placeable_model(tables: &mut Tables, appearance: Option<i64>) -> Option<String> {
    appearance
        .and_then(|row| usize::try_from(row).ok())
        .and_then(|row| tables.text("placeables", row, "modelname"))
        .filter(|model| !model.is_empty() && model != "****")
}

/// A placeable template's collision footprint: the cells of a grid of
/// `FOOTPRINT_CELL_M` squares over its walkmesh (`<model>.pwk`, the model
/// `placeables.2da` names by its appearance, as a placement's collision is
/// built) that its faces cover, seen from above, shrunk by a cell, each as
/// its least and greatest corners in the template's own frame (metres),
/// its height the walkmesh's; only those along the footprint's edge (no
/// body comes inside). None without a walkmesh.
fn walkmesh_footprint(tables: &mut Tables, appearance: Option<i64>) -> Vec<[f32; 3]> {
    let walkmesh = placeable_model(tables, appearance)
        .and_then(|model| {
            tables
                .source
                .read(&model.to_ascii_lowercase(), ResType::PWK)
        })
        .and_then(|bytes| fmt_odyssey::bwm::Walkmesh::parse(&bytes).ok());
    let Some(walkmesh) = walkmesh.filter(|walkmesh| !walkmesh.vertices.is_empty()) else {
        return Vec::new();
    };
    let (mut least, mut most) = ([f32::MAX; 3], [f32::MIN; 3]);
    for vertex in &walkmesh.vertices {
        for axis in 0..3 {
            least[axis] = least[axis].min(vertex[axis]);
            most[axis] = most[axis].max(vertex[axis]);
        }
    }
    let count = |axis: usize| {
        ((most[axis] - least[axis]) / FOOTPRINT_CELL_M)
            .ceil()
            .max(1.0) as usize
    };
    let (across, along) = (count(0), count(1));
    let size = |axis: usize, n: usize| (most[axis] - least[axis]) / n as f32;
    let (dx, dy) = (size(0, across), size(1, along));
    let triangles: Vec<[[f32; 2]; 3]> = walkmesh
        .faces
        .iter()
        .filter_map(|face| {
            let corner = |k: usize| {
                let v = walkmesh.vertices.get(face.vertices[k] as usize)?;
                Some([v[0], v[1]])
            };
            Some([corner(0)?, corner(1)?, corner(2)?])
        })
        .collect();
    // A cell is covered when its middle is in a face, or a face's corner in
    // it (a sliver thinner than a cell).
    let covered = |i: usize, j: usize| {
        let (x0, y0) = (least[0] + i as f32 * dx, least[1] + j as f32 * dy);
        let middle = [x0 + dx / 2.0, y0 + dy / 2.0];
        triangles.iter().any(|triangle| {
            in_triangle(triangle, middle)
                || triangle
                    .iter()
                    .any(|p| (x0..=x0 + dx).contains(&p[0]) && (y0..=y0 + dy).contains(&p[1]))
        })
    };
    let covers: Vec<Vec<bool>> = (0..across)
        .map(|i| (0..along).map(|j| covered(i, j)).collect())
        .collect();
    // KOTOR keeps a body's middle off a placeable's walkmesh; a box as
    // wide as the body would keep half a body farther off (Calo Nord's
    // fallen rubble shut the way to the Ebon Hawk): the footprint shrunk by
    // a cell, unless that leaves nothing.
    let inner = |i: usize, j: usize| {
        covers[i][j]
            && i > 0
            && j > 0
            && i + 1 < across
            && j + 1 < along
            && covers[i - 1][j]
            && covers[i + 1][j]
            && covers[i][j - 1]
            && covers[i][j + 1]
    };
    let shrunk: Vec<Vec<bool>> = (0..across)
        .map(|i| (0..along).map(|j| inner(i, j)).collect())
        .collect();
    let grid = if shrunk.iter().flatten().any(|cell| *cell) {
        shrunk
    } else {
        covers
    };
    let at = |i: isize, j: isize| {
        usize::try_from(i)
            .ok()
            .zip(usize::try_from(j).ok())
            .and_then(|(i, j)| grid.get(i)?.get(j).copied())
            .unwrap_or(false)
    };
    let mut cells = Vec::new();
    for (i, j) in (0..across).flat_map(|i| (0..along).map(move |j| (i, j))) {
        let (si, sj) = (i as isize, j as isize);
        let edge = [(-1, 0), (1, 0), (0, -1), (0, 1)]
            .iter()
            .any(|(di, dj)| !at(si + di, sj + dj));
        if grid[i][j] && edge {
            let (x0, y0) = (least[0] + i as f32 * dx, least[1] + j as f32 * dy);
            cells.push([x0, y0, least[2]]);
            cells.push([x0 + dx, y0 + dy, most[2]]);
        }
    }
    cells
}

/// The side (metres) of a placeable footprint's cells at most
/// (`walkmesh_footprint`).
const FOOTPRINT_CELL_M: f32 = 0.5;

/// Whether `point` lies in `triangle` (seen from above), edges included.
fn in_triangle(triangle: &[[f32; 2]; 3], point: [f32; 2]) -> bool {
    let side = |a: [f32; 2], b: [f32; 2]| {
        (b[0] - a[0]) * (point[1] - a[1]) - (b[1] - a[1]) * (point[0] - a[0])
    };
    let [a, b, c] = *triangle;
    let (d1, d2, d3) = (side(a, b), side(b, c), side(c, a));
    let negative = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let positive = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(negative && positive)
}

/// `repute.2da` as a square matrix: row `a` (a faction) gives how it
/// regards each faction from 1 (`hostile_1`, its first numeric column) on.
/// The table has no column for the player's own faction (row 0), so how a
/// faction regards the player is how the player regards it (the player's
/// row), and the player regards itself as friendly.
fn repute(tables: &mut Tables) -> Repute {
    let Some(table) = tables.table("repute") else {
        return Repute::default();
    };
    let columns: Vec<usize> = (0..table.columns.len())
        .filter(|&column| !table.columns[column].eq_ignore_ascii_case(b"label"))
        .collect();
    let count = table.cells.len();
    let value = |row: usize, faction: usize| {
        columns
            .get(faction.wrapping_sub(1))
            .and_then(|&column| table.get(row, column))
            .and_then(|cell| String::from_utf8_lossy(cell).trim().parse::<u8>().ok())
            .unwrap_or(50)
    };
    let rows = (0..count)
        .map(|a| {
            (0..count)
                .map(|b| match (a, b) {
                    (0, 0) => 100,
                    (a, 0) => value(0, a),
                    (a, b) => value(a, b),
                })
                .collect()
        })
        .collect();
    Repute { rows }
}

/// Every power's and feat's talent facts (`spells.2da`, `feat.2da`:
/// `category`, a hex code, and `maxcr`; 0 where a row has none).
pub fn talents(source: &dyn Source) -> sim::script::Talents {
    let mut tables = Tables::new(source);
    let mut table = |name: &str| {
        let rows = tables.table(name).map_or(0, |table| table.cells.len());
        (0..rows)
            .map(|row| sim::script::Talent {
                category: tables
                    .text(name, row, "category")
                    .and_then(|text| {
                        let digits = text.trim_start_matches("0x").trim_start_matches("0X");
                        u16::from_str_radix(digits, 16).ok()
                    })
                    .unwrap_or(0),
                maxcr: tables.int(name, row, "maxcr").unwrap_or(0).clamp(0, 255) as u8,
                hostile: tables.int(name, row, "hostilesetting") == Some(1),
                reach_dm: tables
                    .text(name, row, "range")
                    .and_then(|range| {
                        ["P", "T", "S", "M", "L"]
                            .iter()
                            .position(|code| *code == range)
                    })
                    .and_then(|at| tables.float("ranges", at, "primaryrange"))
                    .map_or(0, |metres| {
                        (metres * 10.0).round().clamp(0.0, 65535.0) as u16
                    }),
                cast_ms: ["conjtime", "casttime"]
                    .iter()
                    .map(|column| tables.int(name, row, column).unwrap_or(0).max(0))
                    .sum::<i64>()
                    .clamp(0, i64::from(u16::MAX)) as u16,
            })
            .collect()
    };
    let (spells, feats) = (table("spells"), table("feat"));
    let impacts = {
        let rows = tables.table("spells").map_or(0, |table| table.cells.len());
        (0..rows)
            .map(|row| {
                tables
                    .text("spells", row, "impactscript")
                    .map_or_else(String::new, |name| name.to_ascii_lowercase())
            })
            .collect()
    };
    let skill_abilities = {
        let rows = tables.table("skills").map_or(0, |table| table.cells.len());
        (0..rows)
            .map(|row| {
                tables
                    .text("skills", row, "keyability")
                    .and_then(|name| crate::classes::ability(&name))
                    .unwrap_or(u8::MAX)
            })
            .collect()
    };
    let shields = {
        let rows = tables
            .table("forceshields")
            .map_or(0, |table| table.cells.len());
        let mut bits = |row, column| {
            tables
                .int("forceshields", row, column)
                .unwrap_or(0)
                .clamp(0, i64::from(u16::MAX)) as u16
        };
        (0..rows)
            .map(|row| sim::script::ShieldRow {
                types: bits(row, "damageflags"),
                points: bits(row, "amount"),
            })
            .collect()
    };
    let poisons = {
        let rows = tables.table("poison").map_or(0, |table| table.cells.len());
        let mut number = |row, column, most: i64| {
            tables
                .int("poison", row, column)
                .unwrap_or(0)
                .clamp(0, most)
        };
        (0..rows)
            .map(|row| {
                let [every_ms, for_ms, drain_ms] = ["period", "duration", "abil_dur"]
                    .map(|column| (number(row, column, 3_600) * 1000) as u32);
                sim::combat::items::Course {
                    points: number(row, "dam_hp", 255) as u8,
                    drains: [
                        "dam_str", "dam_dex", "dam_con", "dam_int", "dam_wis", "dam_chr",
                    ]
                    .map(|column| number(row, column, 255) as u8),
                    every_ms: every_ms.max(1000),
                    for_ms,
                    drain_ms,
                    dc: number(row, "dc_save", 255) as u8,
                }
            })
            .collect()
    };
    sim::script::Talents {
        spells,
        feats,
        impacts,
        skill_abilities,
        shields,
        poisons,
    }
}

/// `spells.2da`'s `forcepoints` by row (0 where it has none).
fn force_costs(tables: &mut Tables) -> Vec<u16> {
    let rows = tables.table("spells").map_or(0, |table| table.cells.len());
    (0..rows)
        .map(|row| {
            tables
                .int("spells", row, "forcepoints")
                .unwrap_or(0)
                .clamp(0, i64::from(u16::MAX)) as u16
        })
        .collect()
}

/// TSL's Force facts from its tables (`sim::module::ForceRules`, rooms
/// aside): each `spells.2da` row's side (`goodevil` `G`, `E`) and
/// `formmask`, and `forceadjust.2da`'s `goodcost` and `evilcost` by row.
fn force_rules(tables: &mut Tables) -> sim::module::ForceRules {
    use sim::combat::sheet::Side;
    let rows = tables.table("spells").map_or(0, |table| table.cells.len());
    let sides = (0..rows)
        .map(
            |row| match tables.text("spells", row, "goodevil").as_deref() {
                Some("G" | "g") => Side::Light,
                Some("E" | "e") => Side::Dark,
                _ => Side::Neutral,
            },
        )
        .collect();
    let masks = (0..rows)
        .map(|row| {
            tables
                .text("spells", row, "formmask")
                .and_then(|mask| u8::from_str_radix(mask.trim_start_matches("0x"), 16).ok())
                .unwrap_or(0)
        })
        .collect();
    let adjust_rows = tables
        .table("forceadjust")
        .map_or(0, |table| table.cells.len());
    let adjust = (0..adjust_rows)
        .map(|row| {
            ["goodcost", "evilcost"]
                .map(|column| tables.float("forceadjust", row, column).unwrap_or(1.0))
        })
        .collect();
    sim::module::ForceRules {
        sides,
        masks,
        adjust,
        rooms: Vec::new(),
    }
}

/// `traps.2da` by row (empty where the game has none).
fn traps(tables: &mut Tables) -> Vec<sim::traps::TrapRow> {
    let rows = tables.table("traps").map_or(0, |table| table.cells.len());
    let byte = |value: Option<i64>| value.unwrap_or(0).clamp(0, 255) as u8;
    (0..rows)
        .map(|row| {
            let mut text = |column: &str| {
                tables
                    .text("traps", row, column)
                    .map_or_else(String::new, |text| text.to_ascii_lowercase())
            };
            let (label, script, kit, model, sound) = (
                text("label"),
                text("trapscript"),
                text("resref"),
                text("model"),
                text("explosionsound"),
            );
            sim::traps::TrapRow {
                label,
                script,
                set_dc: byte(tables.int("traps", row, "setdc")),
                detect_mod: byte(tables.int("traps", row, "detectdcmod")),
                disarm_mod: byte(tables.int("traps", row, "disarmdcmod")),
                name: tables
                    .int("traps", row, "trapname")
                    .and_then(|strref| u32::try_from(strref).ok())
                    .unwrap_or(u32::MAX),
                kit,
                model,
                sound,
            }
        })
        .collect()
}

/// The 2DAs encounters read: `encdifficulty.2da`'s `value` and
/// `fractionalcr.2da`'s `min`, by row (empty where the game has none).
fn encounter_tables(tables: &mut Tables) -> sim::spawner::Tables {
    let mut column = |table: &str, column: &str| {
        let rows = tables.table(table).map_or(0, |table| table.cells.len());
        (0..rows)
            .map(|row| tables.float(table, row, column).unwrap_or(0.0))
            .collect::<Vec<f32>>()
    };
    sim::spawner::Tables {
        difficulties: column("encdifficulty", "value")
            .into_iter()
            .map(|value| value as i32)
            .collect(),
        fractional: column("fractionalcr", "min"),
    }
}

/// `globalcat.2da`: each declared global's kind by its lowercase name.
fn globals(tables: &mut Tables) -> BTreeMap<String, Global> {
    let rows = tables
        .table("globalcat")
        .map_or(0, |table| table.cells.len());
    (0..rows)
        .filter_map(|row| {
            let name = tables.text("globalcat", row, "name")?;
            let kind = Global::named(&tables.text("globalcat", row, "type")?)?;
            Some((name.to_ascii_lowercase(), kind))
        })
        .collect()
}

/// The rows of `ranges.2da` the rules read by label (`PartyCombat`,
/// `TrapRadius`); K1's for any the table lacks.
fn ranges(tables: &mut Tables) -> sim::module::Ranges {
    let mut ranges = sim::module::Ranges::K1;
    let rows = tables.table("ranges").map_or(0, |table| table.cells.len());
    for row in 0..rows {
        let (Some(label), Some(metres)) = (
            tables.text("ranges", row, "label"),
            tables.float("ranges", row, "primaryrange"),
        ) else {
            continue;
        };
        if label.eq_ignore_ascii_case("PartyCombat") {
            ranges.party_combat_m = metres;
        } else if label.eq_ignore_ascii_case("TrapRadius") {
            ranges.trap_radius_m = metres;
        }
    }
    ranges
}

/// A creature's senses: its `PerceptionRange` row of `ranges.2da`.
fn senses(tables: &mut Tables, root: &GffStruct) -> Option<Senses> {
    let row = usize::try_from(root.int("PerceptionRange")?).ok()?;
    Some(Senses {
        sight: tables.float("ranges", row, "primaryrange")?,
        hearing: tables.float("ranges", row, "secondaryrange")?,
    })
}

fn sheet(tables: &mut Tables, root: &GffStruct, held: &[Held]) -> Option<Sheet> {
    let score = |label: &str| root.int(label).unwrap_or(10).clamp(0, 255) as u8;
    let abilities = ["Str", "Dex", "Con", "Int", "Wis", "Cha"].map(score);
    let (mut level, mut bab, mut saves, mut class_ac) = (0i64, 0i64, [0i64; 3], 0i64);
    let mut powers = Rows::<5>::default();
    for class in root.list("ClassList") {
        let (Some(row), Some(levels)) = (class.int("Class"), class.int("ClassLevel")) else {
            continue;
        };
        let (row, levels) = (usize::try_from(row).ok()?, levels.max(0));
        level += levels;
        let at = usize::try_from(levels - 1).unwrap_or(0);
        // The class's defence bonus: its `acbonus.2da` column at its level.
        if let Some(column) = tables.text("classes", row, "armorclasscolumn") {
            class_ac += tables
                .int("acbonus", at, &column.to_ascii_lowercase())
                .unwrap_or(0);
        }
        if let Some(table) = tables.text("classes", row, "attackbonustable") {
            bab += tables.int(&table, at, "bab").unwrap_or(0);
        }
        if let Some(table) = tables.text("classes", row, "savingthrowtable") {
            for (save, column) in saves.iter_mut().zip(["fortsave", "refsave", "willsave"]) {
                *save += tables.int(&table, at, column).unwrap_or(0);
            }
        }
        for known in class.list("KnownList0") {
            if let Some(spell) = known.int("Spell").and_then(|v| u16::try_from(v).ok()) {
                powers.insert(spell);
            }
        }
    }
    let feats = root
        .list("FeatList")
        .iter()
        .filter_map(|feat| feat.int("Feat").and_then(|v| u16::try_from(v).ok()))
        .fold(Rows::<4>::default(), |rows, feat| rows.with(feat));
    let defence = root.int("NaturalAC").unwrap_or(0) + class_ac;
    let mut flag = 0;
    if root.int("Plot").unwrap_or(0) != 0 {
        flag |= flags::PLOT;
    }
    if root.int("Min1HP").unwrap_or(0) != 0 {
        flag |= flags::MIN_ONE_HP;
    }
    let race = root
        .int("Race")
        .map_or(races::HUMAN, |race| race.clamp(0, 255) as u8);
    if race == races::DROID {
        flag |= flags::DROID;
    }
    if tables.tsl() {
        flag |= flags::TSL;
    }
    let points = |label: &str, fallback: &str| {
        root.int(label)
            .or_else(|| root.int(fallback))
            .unwrap_or(0)
            .clamp(0, i64::from(u16::MAX)) as u16
    };
    // The sheet without its items, then what they add (`sim::inventory`).
    let naked = Sheet {
        abilities,
        level: level.clamp(0, 255) as u8,
        bab: bab.clamp(0, 255) as u8,
        saves: saves.map(|save| save.clamp(-128, 127) as i8),
        max_hp: points("MaxHitPoints", "HitPoints").max(1),
        max_fp: points("ForcePoints", "CurrentForce"),
        defence: defence.clamp(-128, 127) as i8,
        dex_cap: None,
        feats,
        powers,
        faction: root.int("FactionID").unwrap_or(0).clamp(0, 255) as u8,
        race,
        guards: Guards::default(),
        flags: flag,
        weapons: [None; 2],
        items: [None; sim::combat::items::KINDS],
        skills: {
            let mut skills = [0u8; 8];
            for (rank, skill) in skills.iter_mut().zip(root.list("SkillList")) {
                *rank = skill.int("Rank").unwrap_or(0).clamp(0, 255) as u8;
            }
            skills
        },
        gear: Default::default(),
        attack: 0,
        good_evil: root
            .int("GoodEvil")
            .map_or(sim::combat::sheet::NEUTRAL, |value| {
                value.clamp(0, 100) as u8
            }),
        // TSL's: its bonus Force points, base save bonuses and Form in use
        // (none in K1's templates).
        bonus_fp: root
            .int("BonusForcePoints")
            .unwrap_or(0)
            .clamp(-32768, 32767) as i16,
        save_bonus: ["fortbonus", "refbonus", "willbonus"]
            .map(|label| root.int(label).unwrap_or(0).clamp(-128, 127) as i8),
        form: root.int("CurrentForm").unwrap_or(0).clamp(0, 65535) as u16,
    };
    let mut worn = Vec::new();
    let mut usable = Vec::new();
    for item in held {
        let Some(facts) = items::facts(tables, &item.template) else {
            continue;
        };
        if facts.ready(item.slot.is_some()) {
            usable.push((facts.uses_left(item.stack, facts.charges), facts.clone()));
        }
        if let Some(slot) = item.slot {
            worn.push((slot, facts));
        }
    }
    // Consumables cast a spell whose row names an impact script.
    let scripted: Vec<u16> = usable
        .iter()
        .filter_map(|(_, facts)| facts.combat_row())
        .filter(|row| {
            tables
                .text("spells", usize::from(*row), "impactscript")
                .is_some_and(|script| !script.is_empty())
        })
        .collect();
    Some(Sheet {
        items: inventory::usable(
            race,
            usable.iter().map(|(uses, facts)| (*uses, facts)),
            |row| scripted.contains(&row),
        ),
        ..inventory::dress(naked, worn.iter().map(|(slot, facts)| (*slot, facts)))
    })
}
