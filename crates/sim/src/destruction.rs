//! Destruction: which props break or show damage, and what a blow is worth
//! to them (`docs/superpowers/specs/2026-11-06-destruction-design.md`).

#[cfg(feature = "probes")]
pub mod probes;

/// Whether a prop goes, or only shows damage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Break,
    Damage,
}

/// What a prop is made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Material {
    Ceramic,
    Organic,
    Wood,
    Plastic,
    Metal,
    Electronic,
    Stone,
    Cloth,
    Debris,
}

/// How big a prop is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    Small,
    Medium,
    Large,
}

/// A prop destruction reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Destructible {
    pub class: Class,
    pub material: Material,
    pub size: Size,
}

/// Every placeable kind of both games, as destruction takes it.
const CATALOGUE: &str = include_str!("../../../data/destruction/props.csv");

/// The catalogue's breakable and damageable kinds, by game and row.
type Kinds = std::collections::HashMap<(String, u16), Destructible>;

fn kinds() -> &'static Kinds {
    static KINDS: std::sync::OnceLock<Kinds> = std::sync::OnceLock::new();
    KINDS.get_or_init(|| {
        CATALOGUE
            .lines()
            .filter(|line| !line.starts_with('#'))
            .skip(1)
            .filter_map(|line| {
                let [game, row, _label, class, material, size] =
                    line.split(',').collect::<Vec<_>>()[..]
                else {
                    return None;
                };
                let class = match class {
                    "break" => Class::Break,
                    "damage" => Class::Damage,
                    _ => return None,
                };
                let material = match material {
                    "ceramic" => Material::Ceramic,
                    "organic" => Material::Organic,
                    "wood" => Material::Wood,
                    "plastic" => Material::Plastic,
                    "metal" => Material::Metal,
                    "electronic" => Material::Electronic,
                    "stone" => Material::Stone,
                    "cloth" => Material::Cloth,
                    "debris" => Material::Debris,
                    _ => return None,
                };
                let size = match size {
                    "small" => Size::Small,
                    "medium" => Size::Medium,
                    "large" => Size::Large,
                    _ => return None,
                };
                let found = Destructible {
                    class,
                    material,
                    size,
                };
                Some(((game.to_string(), row.parse().ok()?), found))
            })
            .collect()
    })
}

/// What destruction makes of `game`'s (`k1`, `tsl`) `placeables.2da` row
/// `appearance`: `None` when it stays intact or has no row.
pub fn catalogue(game: &str, appearance: u16) -> Option<Destructible> {
    kinds().get(&(game.to_string(), appearance)).copied()
}

/// The hit points (whole) a prop takes before it breaks, or is wrecked.
pub fn most_hp(d: Destructible) -> i32 {
    let hp = rules::destruction::hit_points::BY_SIZE[d.size as usize];
    match d.class {
        Class::Break => hp,
        Class::Damage => hp * rules::destruction::hit_points::DAMAGE_TIMES,
    }
}

/// The hit points (whole) each blow on the prop loses before it counts.
pub fn hardness(d: Destructible) -> i32 {
    rules::destruction::hardness::BY_MATERIAL[d.material as usize]
}

/// What damage of `damage_type` (a damage-type row) counts for on
/// `material`, in thousandths.
fn multiplier(material: Material, damage_type: Option<u8>) -> i32 {
    use rules::destruction::multipliers;
    let by_material = match damage_type {
        Some(0) => &multipliers::BLUDGEONING,
        Some(2) => &multipliers::SLASHING,
        Some(7) => &multipliers::ELECTRICAL,
        Some(8) => &multipliers::FIRE,
        Some(10) => &multipliers::SONIC,
        Some(11) => &multipliers::ION,
        Some(12) => &multipliers::ENERGY,
        _ => return 1000,
    };
    by_material[material as usize]
}

/// The hundredths of an HP a blow of `damage` hundredths of `damage_type`
/// takes off `d`, an area blow's (`area`) counting more; never below 0.
pub fn worth(damage: i32, damage_type: Option<u8>, area: bool, d: Destructible) -> i32 {
    let times = if area {
        rules::destruction::area::TIMES
    } else {
        1
    };
    let counted = i64::from(damage) * i64::from(multiplier(d.material, damage_type)) / 1000;
    let worth = counted * i64::from(times) - i64::from(hardness(d)) * 100;
    i32::try_from(worth.max(0)).unwrap_or(i32::MAX)
}

/// The stage `d` shows having lost `damage` whole hit points: 0 intact, 1
/// damaged, 2 wrecked, 3 broken (a prop that breaks, having lost them all).
pub fn stage(d: Destructible, damage: u16) -> u8 {
    let (lost, most) = (i64::from(damage), i64::from(most_hp(d)));
    let [one, two] = rules::destruction::stages::AT.map(i64::from);
    if d.class == Class::Break && lost >= most {
        3
    } else if lost * 1000 >= two * most {
        2
    } else if lost * 1000 >= one * most {
        1
    } else {
        0
    }
}
