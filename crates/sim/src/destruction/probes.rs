//! Destruction's catalogue and maths (`kjk sim probes --only destruction`).

use super::{Class, Destructible, Material, Size, catalogue, most_hp, stage, worth};
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
        k1 == 92
            && tsl == 137
            && invisible.is_none()
            && footlocker == Some(want_footlocker)
            && panel == Some(want_panel),
        format!(
            "K1 {k1} rows (want 92), TSL {tsl} (want 137); k1/54 {invisible:?}, \
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

/// The probes.
pub fn probes() -> Checks {
    vec![
        ("destruction_catalogue", catalogue_probe()),
        ("destruction_worth", worth_probe()),
    ]
}
