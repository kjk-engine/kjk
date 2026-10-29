//! A module's IR as the sim's facts (`sim::module::ModuleData`).
//!
//! Everything here becomes sim state or decides it, so it keeps the sim's
//! determinism rules: facings come from the stored values through the
//! sim's own `atan2_deg` (a bearing is only scaled), never the platform's
//! trigonometry.

use asset_odyssey::module::{Direction, Instance, Kind, ModuleIr, Title};
use fmt_odyssey::tlk::Tlk;
use sim::math;
use sim::module::{
    Link, Lock, MapNote, ModuleData, ObjectData, ObjectKind, Opens, Placement, Spot, Toughness,
    Ways,
};

use crate::links::LinkOverride;

fn kind(kind: Kind) -> ObjectKind {
    match kind {
        Kind::Creature => ObjectKind::Creature,
        Kind::Door => ObjectKind::Door,
        Kind::Placeable => ObjectKind::Placeable,
        Kind::Trigger => ObjectKind::Trigger,
        Kind::Waypoint => ObjectKind::Waypoint,
        Kind::Sound => ObjectKind::Sound,
        Kind::Encounter => ObjectKind::Encounter,
        Kind::Store => ObjectKind::Store,
    }
}

/// Degrees from +X.
fn yaw(direction: Direction) -> f32 {
    match direction {
        Direction::Bearing(radians) => radians * (180.0 / std::f32::consts::PI),
        Direction::Vector([x, y]) => math::atan2_deg(y, x),
    }
}

/// A localized string as the game shows it: its own text, else its line
/// in the talk table (one character a byte, the game's code page); empty
/// when neither is there.
pub(crate) fn shown(title: &Title, talk: Option<&Tlk>) -> String {
    let text = match title {
        Title::Text(text) => Some(text.as_slice()),
        Title::StrRef(strref) => talk.and_then(|talk| talk.text(*strref)),
        Title::None => None,
    };
    text.map_or_else(String::new, |bytes| {
        bytes.iter().map(|&byte| char::from(byte)).collect()
    })
}

/// An instance's name (`GetName`): a creature's first and last names.
fn name(instance: &Instance, talk: Option<&Tlk>) -> String {
    full_name(
        shown(&instance.name, talk),
        shown(&instance.last_name, talk),
    )
}

/// A first and a last name as one.
pub(crate) fn full_name(first: String, last: String) -> String {
    match (first.is_empty(), last.is_empty()) {
        (_, true) => first,
        (true, false) => last,
        (false, false) => format!("{first} {last}"),
    }
}

fn lower(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).to_ascii_lowercase()
}

/// A door's or placeable's toughness as the sim keeps it.
pub(crate) fn toughness(read: asset_odyssey::module::Toughness) -> Toughness {
    Toughness {
        hardness: read.hardness,
        plot: read.plot,
        min_one_hp: read.min_one_hp,
    }
}

/// A trap as the sim knows it: its fields and its lock's key.
pub(crate) fn trap(
    trap: asset_odyssey::module::TrapIr,
    lock: &asset_odyssey::module::Lock,
) -> sim::traps::Trap {
    sim::traps::Trap {
        row: trap.trap_type.min(255) as u8,
        detectable: trap.detectable,
        disarmable: trap.disarmable,
        flagged: false,
        one_shot: trap.one_shot,
        detect_dc: trap.detect_dc,
        disarm_dc: trap.disarm_dc,
        key: lower(&lock.key),
        auto_remove_key: lock.auto_remove_key,
    }
}

fn object(instance: &Instance, talk: Option<&Tlk>) -> ObjectData {
    let kind = kind(instance.kind);
    let link = instance
        .linked_to
        .as_ref()
        .filter(|(module, tag)| !module.is_empty() || !tag.is_empty())
        .map(|(module, tag)| Link {
            module: module.to_ascii_lowercase(),
            tag: lower(tag),
            destination: instance.destination,
        });
    let opens = match kind {
        ObjectKind::Door if instance.is_static => Opens::Never,
        ObjectKind::Door if instance.locked => Opens::Locked,
        ObjectKind::Door => Opens::Yes,
        ObjectKind::Placeable if instance.locked => Opens::Locked,
        _ => Opens::Never,
    };
    let held = &instance.lock;
    let lock = (matches!(kind, ObjectKind::Door | ObjectKind::Placeable)
        && *held != asset_odyssey::module::Lock::default())
    .then(|| Lock {
        lockable: held.lockable,
        open_dc: held.open_dc,
        close_dc: held.close_dc,
        key: lower(&held.key),
        key_required: held.key_required,
        auto_remove_key: held.auto_remove_key,
    });
    let state = match kind {
        ObjectKind::Door if instance.animation_state <= 2 => instance.animation_state as u8,
        ObjectKind::Placeable => sim::look::placed(instance.animation_state),
        _ => 0,
    };
    ObjectData {
        kind,
        tag: lower(&instance.tag),
        cased_tag: crate::sheets::cased(&instance.tag),
        template: instance.template.clone(),
        position: instance.position,
        yaw: yaw(instance.direction),
        link,
        geometry: instance.geometry.clone(),
        use_points: Vec::new(),
        opens,
        state,
        // Read with the module's walkmeshes, after (`crate::doors`).
        ways: Ways::One,
        scripts: instance.scripts.clone(),
        conversation: instance.conversation.clone(),
        // Read with the module's files, after (`crate::sheets`).
        sheet: None,
        senses: None,
        classes: Vec::new(),
        gender: 0,
        subrace: 0,
        personal_space: 0.0,
        strike: None,
        challenge: 0.0,
        multiplier_set: 0,
        name: name(instance, talk),
        inventory: Vec::new(),
        container: instance.container == Some(true),
        useable: instance.useable == Some(true),
        store: None,
        lock,
        hit_points: instance.hit_points,
        toughness: toughness(instance.toughness),
        faction: instance.faction.map_or(0, |faction| faction.min(255) as u8),
        body: None,
        map_note: instance.map_note.as_ref().map(|note| MapNote {
            text: shown(&note.text, talk),
            enabled: note.enabled,
        }),
        // A placed trigger's DCs are its `traps.2da` row's
        // (`crate::sheets::fill`); a door's or placeable's its own.
        trap: instance.trap.map(|found| trap(found, held)),
        // A creature's is read with its sheet (`crate::sheets`).
        interruptable: true,
        // A sound object's is read with its UTS (`crate::sheets::fill`).
        pitch_variation: 0.0,
        // Read with the module's 2DAs, after (`encounters`).
        encounter: None,
    }
}

/// Each encounter of `module` (the entry area `area`'s, placed in its
/// order) given its UTE: its creatures by challenge, highest first, its
/// difficulty `encdifficulty.2da`'s value for its row where the module's
/// table has the row (`ModuleData::encounter_tables`, read first), else
/// the value its UTE writes, as both games' executables load it.
pub(crate) fn encounters(module: &mut ModuleData, area: &asset_odyssey::module::AreaIr) {
    let values = module.encounter_tables.difficulties.clone();
    for (placement, instance) in module.placements.iter_mut().zip(&area.instances) {
        let (Placement::Own(object), Some(ute)) = (placement, &instance.encounter) else {
            continue;
        };
        let difficulty = usize::try_from(ute.difficulty_index)
            .ok()
            .and_then(|row| values.get(row).copied())
            .unwrap_or(ute.difficulty);
        let creatures = ute
            .creatures
            .iter()
            .map(|creature| sim::spawner::Creature {
                template: creature.template.to_ascii_lowercase(),
                cr: creature.cr,
                single: creature.single,
                guaranteed: creature.guaranteed,
            })
            .collect();
        object.encounter = Some(Box::new(sim::spawner::Encounter {
            active: ute.active,
            reset: ute.reset,
            reset_time_s: ute.reset_time,
            respawns: ute.respawns,
            spawn_option: ute.spawn_option,
            max_creatures: ute.max_creatures,
            rec_creatures: ute.rec_creatures,
            player_only: ute.player_only,
            difficulty,
            creatures: sim::spawner::Encounter::sorted(creatures),
            points: ute
                .spawn_points
                .iter()
                .map(|&(at, orientation)| sim::spawner::Point { at, orientation })
                .collect(),
        }));
    }
}

/// An area's map as the sim keeps it (`sim::fog`).
fn chart(map: &asset_odyssey::map::AreaMap) -> sim::fog::Chart {
    let axes = map.axes().map(|axis| sim::fog::Axis {
        world: axis.world,
        scale: axis.scale,
        offset: axis.offset,
    });
    sim::fog::Chart::new(axes, map.res_x)
}

/// The entry area `area` of `ir` as the sim runs it, with the link
/// overrides applied; how many objects they changed. An override whose
/// tag names no door or trigger is an error.
pub fn module_data(
    game: &str,
    ir: &ModuleIr,
    area: usize,
    overrides: &[LinkOverride],
    talk: Option<&Tlk>,
) -> Result<(ModuleData, usize), String> {
    let area_ir = &ir.areas[area];
    let mut objects: Vec<ObjectData> = area_ir
        .instances
        .iter()
        .map(|instance| object(instance, talk))
        .collect();
    let mut overridden = 0;
    for link in overrides {
        let mut found = 0;
        for object in objects.iter_mut().filter(|object| {
            matches!(object.kind, ObjectKind::Door | ObjectKind::Trigger) && object.tag == link.tag
        }) {
            object.link = Some(Link {
                module: link.module.clone(),
                tag: link.arrival.clone(),
                destination: object.link.as_ref().and_then(|old| old.destination),
            });
            found += 1;
        }
        if found == 0 {
            return Err(format!(
                "data/links/{game}/{}.links line {}: no door or trigger is tagged {}",
                ir.name, link.line, link.tag
            ));
        }
        overridden += found;
    }
    let [dx, dy] = ir.entry_direction;
    Ok((
        ModuleData {
            minutes_per_hour: ir.minutes_per_hour,
            game: game.to_string(),
            name: ir.name.clone(),
            title: shown(&ir.title, talk),
            area: area_ir.resref.clone(),
            area_title: shown(
                &area_ir.name_strref.map_or(Title::None, Title::StrRef),
                talk,
            ),
            entry: Spot {
                position: ir.entry_position,
                yaw: math::atan2_deg(dy, dx),
            },
            // Creatures made as their templates are share them after
            // (`crate::sheets::fill`).
            placements: objects.into_iter().map(Placement::Own).collect(),
            templates: Default::default(),
            repute: sim::module::Repute::default(),
            force_costs: Vec::new(),
            globals: std::collections::BTreeMap::new(),
            module_scripts: ir.scripts.clone(),
            area_scripts: area_ir.scripts.clone(),
            // Planned for one until its encounters say more
            // (`crate::encounters::apply`).
            players: 1,
            map: area_ir.map.as_ref().map(chart),
            // Read with the module's 2DAs (`crate::sheets::fill`).
            traps: Vec::new(),
            bags: Vec::new(),
            area_stealth: {
                let stealth = &area_ir.stealth;
                sim::module::AreaStealth {
                    mod_listen: stealth.mod_listen.clamp(-128, 127) as i8,
                    xp_max: stealth.xp_max,
                    xp_current: stealth.xp_current,
                    xp_loss: stealth.xp_loss,
                    xp_enabled: stealth.xp_enabled,
                }
            },
            ranges: Default::default(),
            force: Default::default(),
            disable_transit: area_ir.disable_transit,
            encounter_tables: Default::default(),
        },
        overridden,
    ))
}
