//! The game's own things as the engine reads them, for the pages about
//! them (`kjk rules`, the wiki beside the rulebook): every weapon base
//! item, item template, Force power, feat, skill and class, and the
//! experience table, read through `source` (so with the rulebook's table
//! changes in, `vfs`'s rulebook layer) by the same code a module's install
//! reads them with (`items::facts`, `Delivery::of_base_item`). The tags
//! each carries are what the rulebook's `applies` names
//! (`rules::applying`).

use asset_odyssey::module::Source;
use fmt_odyssey::ResType;
use sim::combat::sheet::{Blaster, Delivery, SaberShape, Weapon};
use sim::inventory::Armour;

use crate::items;
use crate::sheets::Tables;

/// A row of one of the game's tables, named as the game names it.
#[derive(Debug, Clone)]
pub struct Row {
    pub row: u16,
    /// Its `label` cell.
    pub label: String,
    /// Its name and description in the talk table (empty without one).
    pub name: String,
    pub description: String,
    /// The rulebook's tags for it (`spell:FORCE_POWER_FORCE_PUSH`,
    /// `kind:weapon`).
    pub tags: Vec<String>,
}

/// A base item combat knows as a weapon (`baseitems.2da`).
#[derive(Debug, Clone)]
pub struct BaseWeapon {
    pub row: Row,
    /// Its numbers before any item's properties.
    pub weapon: Weapon,
    /// A blaster's primary fire's time between shots (its cadence for the
    /// pace); none for a blade, whose cadence is its swing's.
    pub refire_ms: Option<u32>,
}

/// An item template (UTI) as the game reads it.
#[derive(Debug, Clone)]
pub struct Item {
    pub template: String,
    pub name: String,
    pub description: String,
    /// Its base item's row and label.
    pub base_item: u16,
    pub base_label: String,
    pub cost: u32,
    /// Its weapon with its properties in (a weapon's).
    pub weapon: Option<Weapon>,
    pub armour: Option<Armour>,
    /// The `spells.2da` row it casts (a grenade's, a medpac's), if any.
    pub casts: Option<u16>,
    pub tags: Vec<String>,
}

/// A row of `spells.2da`: a power, a grenade's or an item's ability.
#[derive(Debug, Clone)]
pub struct Spell {
    pub row: Row,
    /// `forcepoints` (TSL adjusts it by side and Charisma when cast).
    pub cost: Option<i64>,
    /// `goodevil`: `L`, `D` or none.
    pub side: String,
    /// The level each Jedi class may learn it at (`guardian`, `consular`,
    /// `sentinel`).
    pub levels: [Option<i64>; 3],
    /// The script that decides its impact (`impactscript`).
    pub impact: String,
    /// The JA power it plays as and its level there (`force::of_row`).
    pub delivered: Option<(String, u8)>,
}

/// A row of `classes.2da`.
#[derive(Debug, Clone)]
pub struct Class {
    pub row: Row,
    pub hit_die: Option<i64>,
    pub force_die: Option<i64>,
    pub skill_points: Option<i64>,
    /// `attackbonustable` and `savingthrowtable`.
    pub attack_table: String,
    pub save_table: String,
}

/// Everything above, for one game.
#[derive(Debug, Clone, Default)]
pub struct Codex {
    pub weapons: Vec<BaseWeapon>,
    pub items: Vec<Item>,
    pub spells: Vec<Spell>,
    pub feats: Vec<Row>,
    pub skills: Vec<Row>,
    pub classes: Vec<Class>,
    /// The experience each level starts at (`exptable.2da` `xp`).
    pub experience: Vec<i64>,
}

/// The rulebook's kinds a weapon is (`kind:weapon`, `kind:blaster`, …).
pub fn kinds(delivery: Delivery) -> &'static [&'static str] {
    match delivery {
        Delivery::Unarmed => &["weapon", "unarmed"],
        Delivery::Baton => &["weapon", "melee", "baton"],
        Delivery::Melee => &["weapon", "melee"],
        Delivery::Saber(SaberShape::Single) => &["weapon", "lightsaber"],
        Delivery::Saber(SaberShape::Staff) => &["weapon", "lightsaber", "double"],
        Delivery::Saber(SaberShape::Short) => &["weapon", "lightsaber", "short"],
        Delivery::Blaster(blaster) => match blaster {
            Blaster::Pistol => &["weapon", "blaster", "pistol"],
            Blaster::Rifle => &["weapon", "blaster", "rifle"],
            Blaster::Repeater => &["weapon", "blaster", "heavy", "repeater"],
            Blaster::Bowcaster => &["weapon", "blaster", "rifle", "bowcaster"],
            Blaster::Disruptor => &["weapon", "blaster", "disruptor"],
            Blaster::Ion => &["weapon", "blaster", "ion"],
            Blaster::Sonic => &["weapon", "blaster", "sonic"],
        },
    }
}

/// The tags of an item of base item `row` (labelled `label`) casting
/// `casts` (its `spells.2da` label): its base item's, its kinds', the
/// spell's.
pub fn item_tags(row: u16, label: &str, casts: Option<&str>) -> Vec<String> {
    let mut tags = vec![format!("baseitem:{label}")];
    if let Some(delivery) = Delivery::of_base_item(row) {
        tags.extend(kinds(delivery).iter().map(|kind| format!("kind:{kind}")));
    }
    if let Some(spell) = casts {
        tags.push(format!("spell:{spell}"));
    }
    tags
}

/// The game `source` reads, as the pages list it.
pub fn codex(source: &dyn Source) -> Codex {
    let mut tables = Tables::new(source);
    let talk = source.talk();
    let said = |strref: Option<i64>| -> String {
        strref
            .and_then(|strref| u32::try_from(strref).ok())
            .and_then(|strref| talk.as_ref()?.text(strref).map(game_text))
            .unwrap_or_else(String::new)
    };
    let row_of = |tables: &mut Tables, table: &str, row: usize, name: &str, desc: &str| {
        let label = tables.text(table, row, "label").unwrap_or_else(String::new);
        Row {
            row: row as u16,
            name: said(tables.int(table, row, name)),
            description: said(tables.int(table, row, desc)),
            tags: Vec::new(),
            label,
        }
    };
    let rows = |tables: &mut Tables, table: &str| tables.table(table).map_or(0, |t| t.cells.len());

    let mut codex = Codex::default();
    for row in 0..rows(&mut tables, "baseitems") {
        let Some(weapon) = items::weapon(&mut tables, row as u16) else {
            continue;
        };
        let mut entry = row_of(&mut tables, "baseitems", row, "name", "description");
        entry.tags = item_tags(weapon.base_item, &entry.label, None);
        let refire_ms = match weapon.delivery {
            Delivery::Blaster(blaster) => Some(sim::combat::weapons::primary(blaster).refire_ms),
            _ => None,
        };
        codex.weapons.push(BaseWeapon {
            row: entry,
            weapon,
            refire_ms,
        });
    }
    for row in 0..rows(&mut tables, "spells") {
        let mut entry = row_of(&mut tables, "spells", row, "name", "spelldesc");
        if entry.label.is_empty() {
            continue;
        }
        entry.tags = vec![format!("spell:{}", entry.label)];
        let int = |tables: &mut Tables, column| tables.int("spells", row, column);
        let delivered = sim::combat::force::of_row(row as u16)
            .map(|(power, level)| (format!("{power:?}"), level));
        if delivered.is_some() {
            entry.tags.push("kind:force".to_string());
        }
        codex.spells.push(Spell {
            cost: int(&mut tables, "forcepoints"),
            side: tables
                .text("spells", row, "goodevil")
                .unwrap_or_else(String::new),
            levels: ["guardian", "consular", "sentinel"].map(|class| int(&mut tables, class)),
            impact: tables
                .text("spells", row, "impactscript")
                .unwrap_or_else(String::new),
            delivered,
            row: entry,
        });
    }
    for row in 0..rows(&mut tables, "feat") {
        let mut entry = row_of(&mut tables, "feat", row, "name", "description");
        if entry.label.is_empty() || entry.name.is_empty() {
            continue;
        }
        entry.tags = vec![format!("feat:{}", entry.label)];
        codex.feats.push(entry);
    }
    for row in 0..rows(&mut tables, "skills") {
        let mut entry = row_of(&mut tables, "skills", row, "name", "description");
        entry.tags = vec![format!("skill:{}", entry.label)];
        codex.skills.push(entry);
    }
    for row in 0..rows(&mut tables, "classes") {
        let mut entry = row_of(&mut tables, "classes", row, "name", "description");
        entry.tags = vec![format!("class:{}", entry.label)];
        let int = |tables: &mut Tables, column| tables.int("classes", row, column);
        let text = |tables: &mut Tables, column| {
            tables
                .text("classes", row, column)
                .unwrap_or_else(String::new)
        };
        codex.classes.push(Class {
            hit_die: int(&mut tables, "hitdie"),
            force_die: int(&mut tables, "forcedie"),
            skill_points: int(&mut tables, "skillpointbase"),
            attack_table: text(&mut tables, "attackbonustable"),
            save_table: text(&mut tables, "savingthrowtable"),
            row: entry,
        });
    }
    codex.experience = (0..rows(&mut tables, "exptable"))
        .filter_map(|row| tables.int("exptable", row, "xp"))
        .collect();
    let spell_labels: Vec<(u16, String)> = codex
        .spells
        .iter()
        .map(|spell| (spell.row.row, spell.row.label.clone()))
        .collect();
    let mut templates = source.names(ResType::UTI);
    templates.sort();
    for template in templates {
        let Some(facts) = items::facts(&mut tables, &template) else {
            continue;
        };
        let base = usize::from(facts.base_item);
        let base_label = tables
            .text("baseitems", base, "label")
            .unwrap_or_else(String::new);
        let casts = facts.spell.map(|spell| spell.row);
        let cast_label = casts.and_then(|row| {
            spell_labels
                .iter()
                .find(|(at, _)| *at == row)
                .map(|(_, label)| label.as_str())
        });
        let name = talk
            .as_ref()
            .and_then(|talk| talk.text(facts.name).map(game_text))
            .unwrap_or_else(String::new);
        codex.items.push(Item {
            tags: item_tags(facts.base_item, &base_label, cast_label),
            name,
            description: item_description(source, &template, talk.as_deref()),
            base_item: facts.base_item,
            base_label,
            cost: facts.cost,
            weapon: facts.weapon,
            armour: facts.armour,
            casts,
            template,
        });
    }
    codex
}

/// A template's `DescIdentified`, from the talk table or its own text.
fn item_description(
    source: &dyn Source,
    template: &str,
    talk: Option<&fmt_odyssey::tlk::Tlk>,
) -> String {
    use fmt_odyssey::gff::{Gff, GffValue};
    let Some(gff) = source
        .read(template, ResType::UTI)
        .and_then(|bytes| Gff::parse(&bytes).ok())
    else {
        return String::new();
    };
    match gff.root.get("DescIdentified") {
        Some(GffValue::LocString(text)) => talk
            .and_then(|talk| talk.text(text.strref))
            .map(game_text)
            .unwrap_or_else(|| {
                text.strings
                    .first()
                    .map_or_else(String::new, |(_, words)| game_text(words))
            }),
        _ => String::new(),
    }
}

/// The talk table's bytes (Windows-1252) as text.
pub fn game_text(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&byte| match byte {
            0x85 => '…',
            0x91 => '‘',
            0x92 => '’',
            0x93 => '“',
            0x94 => '”',
            0x96 => '–',
            0x97 => '—',
            byte => char::from(byte),
        })
        .collect::<String>()
        .trim()
        .to_string()
}

/// Every creature template the install lists (`creature_catalog`), read
/// once and shared by each module's `placed`.
pub struct Templates(std::collections::BTreeMap<String, std::sync::Arc<sim::module::ObjectData>>);

/// The templates `source` (an install's shared layers) lists.
pub fn templates(source: &dyn Source) -> Templates {
    let mut tables = Tables::new(source);
    Templates(crate::sheets::creature_catalog(
        &mut tables,
        std::iter::empty(),
    ))
}

/// A creature as a module places it, read as its install reads it (its
/// template, its placement's own facts, the module's encounter overrides:
/// `data/encounters/`).
#[derive(Debug, Clone)]
pub struct Placed {
    pub area: String,
    pub tag: String,
    pub template: String,
    pub name: String,
    pub sheet: sim::combat::Sheet,
    /// Its classes: (`classes.2da` row, levels).
    pub classes: Vec<(u8, u8)>,
    /// Its most HP with one player, and with four (`None`: the overrides
    /// take it away for four).
    pub hp_one: Option<u16>,
    pub hp_four: Option<u16>,
    /// Whether it regards the player as hostile (`repute.2da`).
    pub hostile: bool,
}

/// The creatures module `module` of `game` places (`source` opened with
/// it), as its install plans them (`Plan`'s steps: its entry area, its
/// doors, the sheets, its encounters, its overrides) for one player and
/// for four; one only there with four (an override's) has no HP alone.
pub fn placed(
    source: &dyn Source,
    game: &str,
    module: &str,
    templates: &Templates,
) -> Result<Vec<Placed>, String> {
    use sim::combat::sheet::factions;
    use sim::module::ObjectKind;
    let ir = asset_odyssey::module::ModuleIr::build(module, source)
        .map_err(|refusal| format!("{module}: {refusal:?}"))?;
    let area = ir
        .areas
        .iter()
        .position(|area| area.resref == ir.entry_area)
        .or((!ir.areas.is_empty()).then_some(0))
        .ok_or_else(|| format!("{module}: no area"))?;
    let talk = source.talk();
    let (mut data, _) = crate::objects::module_data(game, &ir, area, &[], talk.as_deref())?;
    crate::doors::fill(source, &mut data, &ir.areas[area]);
    let mut tables = Tables::new(source);
    let mut creatures = templates.0.clone();
    for name in source.own_names(ResType::UTC) {
        if let Some(facts) = crate::sheets::creature_template(&mut tables, &name) {
            creatures.insert(name, std::sync::Arc::new(facts));
        }
    }
    crate::sheets::fill(&mut tables, &mut data, &creatures);
    crate::objects::encounters(&mut data, &ir.areas[area]);
    let changes = crate::encounters::built_in(game, module)?;
    let mut four = data.clone();
    crate::encounters::apply(&mut data, &changes, 1);
    crate::encounters::apply(&mut four, &changes, 4);
    // A placement's number is the same in both (an override's removal
    // leaves a waypoint, its addition goes at the end).
    let creature = |object: &sim::module::ObjectData| {
        object.sheet.filter(|_| object.kind == ObjectKind::Creature)
    };
    let one: Vec<_> = data.objects().collect();
    let four: Vec<_> = four.objects().collect();
    let mut out = Vec::new();
    for at in 0..one.len().max(four.len()) {
        let alone = one.get(at).and_then(|object| creature(object));
        let shared = four.get(at).and_then(|object| creature(object));
        let (Some(object), Some(sheet)) = (
            one.get(at).filter(|_| alone.is_some()).or(four.get(at)),
            alone.or(shared),
        ) else {
            continue;
        };
        out.push(Placed {
            area: ir.areas[area].resref.clone(),
            tag: object.tag.clone(),
            template: object.template.clone(),
            name: object.name.clone(),
            classes: object.classes.clone(),
            hp_one: alone.map(|sheet| sheet.max_hp),
            hp_four: shared.map(|sheet| sheet.max_hp),
            hostile: data.repute.hostile(sheet.faction, factions::PLAYER),
            sheet,
        });
    }
    Ok(out)
}
