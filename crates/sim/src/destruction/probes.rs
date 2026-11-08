//! Destruction's catalogue and maths (`kjk sim probes --only destruction`).

use super::{Class, Destructible, Material, Size, catalogue, guarded, most_hp, stage, worth};
use crate::module::{ObjectData, ObjectKind, Opens};
use crate::probe::{Checks, check};

/// The catalogue's rows of `game` that break or show damage.
fn shown(game: &str) -> usize {
    (0..=u16::MAX)
        .filter(|row| catalogue(game, *row).is_some())
        .count()
}

fn catalogue_probe() -> Result<String, String> {
    let (k1, tsl) = (shown("k1"), shown("tsl"));
    let invisible = catalogue("k1", 54);
    let footlocker = catalogue("k1", 39);
    let panel = catalogue("tsl", 23);
    let want_footlocker = Destructible {
        class: Class::Break,
        material: Material::Metal,
        size: Size::Medium,
    };
    let want_panel = Destructible {
        class: Class::Damage,
        material: Material::Electronic,
        size: Size::Medium,
    };
    check(
        k1 == 90
            && tsl == 134
            && invisible.is_none()
            && footlocker == Some(want_footlocker)
            && panel == Some(want_panel),
        format!(
            "K1 {k1} rows (want 90), TSL {tsl} (want 134); k1/54 {invisible:?}, \
             k1/39 {footlocker:?}, tsl/23 {panel:?}"
        ),
    )
}

fn worth_probe() -> Result<String, String> {
    let of = |game, row| catalogue(game, row).ok_or(format!("{game}/{row} not destructible"));
    let chair = of("k1", 39)?;
    let pillar = of("k1", 93)?;
    let panel = of("tsl", 23)?;
    let pistol_chair = worth(350, Some(12), false, chair);
    let pistol_pillar = worth(350, Some(12), false, pillar);
    let blast_chair = worth(2000, None, true, chair);
    let stages = [0, 27, 54, 79].map(|lost| stage(panel, lost));
    check(
        pistol_chair == 320
            && pistol_pillar == 0
            && blast_chair == 5900
            && stages == [0, 1, 2, 2]
            && most_hp(chair) == 20
            && most_hp(panel) == 80,
        format!(
            "pistol on a chair {pistol_chair} (want 320), on stone {pistol_pillar} (0), \
             a blast on a chair {blast_chair} (5900); the panel's stages {stages:?} \
             ([0, 1, 2, 2]); most HP chair {} (20), panel {} (80)",
            most_hp(chair),
            most_hp(panel)
        ),
    )
}

/// A bare placeable of K1's row 39 (a footlocker), tagged `chest`.
pub(crate) fn bare_prop() -> ObjectData {
    let mut facts = ObjectData {
        kind: ObjectKind::Placeable,
        tag: "chest".into(),
        ..ObjectData::unknown("chest_tpl", [0.0; 3], 0.0)
    };
    super::fill(&mut facts, "k1", 39, |_| false);
    facts
}

fn guards_probe() -> Result<String, String> {
    type Guard = (&'static str, fn(&mut ObjectData));
    const GUARDS: [Guard; 8] = [
        ("plot", |facts| facts.toughness.plot = true),
        ("container", |facts| facts.container = true),
        ("useable", |facts| facts.useable = true),
        ("conversation", |facts| facts.conversation = "x".into()),
        ("script", |facts| {
            facts.scripts = vec![("OnHeartbeat".into(), "x".into())]
        }),
        ("min1hp", |facts| facts.toughness.min_one_hp = true),
        ("locked", |facts| facts.opens = Opens::Locked),
        ("tag named", |facts| facts.tag = "named".into()),
    ];
    let named = |text: &str| text == "named";
    let bare = bare_prop();
    let bare_guarded = guarded(&bare, named);
    let missed: Vec<&str> = GUARDS
        .iter()
        .filter(|(_, guard)| {
            let mut facts = bare_prop();
            guard(&mut facts);
            !guarded(&facts, named)
        })
        .map(|(name, _)| *name)
        .collect();
    let template_named = guarded(&bare, |text| text == "chest_tpl");
    check(
        bare.destructible.is_some() && !bare_guarded && missed.is_empty() && template_named,
        format!(
            "a bare footlocker guarded {bare_guarded} (want false); guards that missed: \
             {missed:?}; template named guards it {template_named}"
        ),
    )
}

/// A destructible prop placed in its dead pose (look `DEAD`, no damage)
/// still stands in the way; broken, it does not.
fn placed_dead_probe() -> Result<String, String> {
    let facts = bare_prop();
    let placed = crate::snapshot::ObjectState {
        state: crate::snapshot::look::DEAD,
        ..crate::snapshot::ObjectState::default()
    };
    let broken = crate::snapshot::ObjectState {
        damage: 20,
        ..placed
    };
    let (standing, gone) = (placed.passable(&facts), broken.passable(&facts));
    check(
        !standing && gone,
        format!("placed dead passable {standing} (want false), broken {gone} (want true)"),
    )
}

/// The owner's playtest classes: workbenches stay intact; desks, kiosks and
/// lockers crumple; no electronic or large kind ever breaks.
fn classes_revised_probe() -> Result<String, String> {
    const INTACT: [(&str, u16); 5] = [
        ("k1", 168),
        ("k1", 169),
        ("tsl", 168),
        ("tsl", 169),
        ("tsl", 336),
    ];
    const DAMAGE: [(&str, u16); 12] = [
        ("k1", 27),
        ("k1", 28),
        ("k1", 59),
        ("k1", 60),
        ("k1", 61),
        ("k1", 67),
        ("tsl", 27),
        ("tsl", 28),
        ("tsl", 60),
        ("tsl", 61),
        ("tsl", 67),
        ("tsl", 296),
    ];
    let not_intact: Vec<_> = INTACT
        .iter()
        .filter(|(game, row)| catalogue(game, *row).is_some())
        .collect();
    let not_damage: Vec<_> = DAMAGE
        .iter()
        .filter(|(game, row)| catalogue(game, *row).map(|d| d.class) != Some(Class::Damage))
        .collect();
    let big_breaks: Vec<(&str, u16)> = ["k1", "tsl"]
        .into_iter()
        .flat_map(|game| (0..=u16::MAX).map(move |row| (game, row)))
        .filter(|(game, row)| {
            catalogue(game, *row).is_some_and(|d| {
                d.class == Class::Break
                    && (d.material == Material::Electronic || d.size == Size::Large)
            })
        })
        .collect();
    check(
        not_intact.is_empty() && not_damage.is_empty() && big_breaks.is_empty(),
        format!(
            "still destructible {not_intact:?}, not damage {not_damage:?}, \
             electronic or large that break {big_breaks:?} (all want none)"
        ),
    )
}

/// The probes.
pub fn probes() -> Checks {
    vec![
        ("destruction_catalogue", catalogue_probe()),
        ("destruction_classes_revised", classes_revised_probe()),
        ("destruction_worth", worth_probe()),
        ("destruction_guards", guards_probe()),
        ("destruction_placed_dead", placed_dead_probe()),
    ]
    .into_iter()
    .chain(crate::world::destruction_probes::probes())
    .collect()
}
