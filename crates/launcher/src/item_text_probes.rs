//! `kjk sim probes --only "item text"`: a weapon's stats on the item
//! screens show its critical threat as both games write it (issue 456). The
//! heading "Critical Threat:" stands on its own line, followed by the lowest
//! roll that threatens and the multiplier ("19-20,x2"). An item's
//! description begins with what a character needs to use it, as both games
//! write it. A game not installed is skipped.

use sim::probe::{Checks, check};
use vfs::Game;

/// Each game's weapons, by template, and the critical threat their
/// description should show.
const WEAPONS: [(Game, &str, &str); 4] = [
    (Game::Kotor1, "g_w_lghtsbr01", "19-20,x2"),
    (Game::Kotor1, "g_w_blstrpstl001", "20-20,x2"),
    (Game::Kotor2, "g_w_lghtsbr01", "19-20,x2"),
    (Game::Kotor2, "w_blaste_01", "20-20,x2"),
];

/// Each game's items, by template, and how their description begins: the
/// requirements both games list before an item's stats (the Discord report
/// "Armor Proficiency Requirements in Item Descriptions Missing"). An
/// armour names only its highest armour proficiency. A feat limitation
/// property is a requirement, not a stat. KOTOR 2 lists attribute minimums
/// before the feats, and gender and companion limitations after them.
const REQUIREMENTS: [(Game, &str, &str); 7] = [
    (Game::Kotor1, "g_a_class4001", "Feats Required:\nArmor Proficiency -  Light\n\nDefense"),
    (Game::Kotor1, "g_a_class8001", "Feats Required:\nArmor Proficiency -  Heavy\n\nDefense"),
    (Game::Kotor1, "g_i_implant101", "Feats Required:\nImplant Level 1\n\n"),
    (Game::Kotor1, "g1_i_drdcomspk01", "Feats Required:\n"),
    (Game::Kotor2, "a_heavy_01", "Feats Required:\nArmor Proficiency -  Heavy\n\nDefense"),
    (Game::Kotor2, "e_imp1_01", "Attributes Required:\nMinimum "),
    (Game::Kotor2, "g_danceroutfit", "Gender Required:\n"),
];

pub fn probes() -> Checks {
    let discovery = crate::discover();
    let mut checks: Checks = vec![edges_written()];
    for (game, template, threat) in WEAPONS {
        if discovery.get(game).is_none() {
            continue;
        }
        let name: &'static str =
            Box::leak(format!("{}: {template} shows its critical threat", game.key()).into_boxed_str());
        let want = format!("Critical Threat:\n{threat}\n");
        let shown = described(&discovery, game, template);
        checks.push((
            name,
            shown.and_then(|text| check(text.contains(&want), format!("text {text:?}"))),
        ));
    }
    for (game, template, begins) in REQUIREMENTS {
        if discovery.get(game).is_none() {
            continue;
        }
        let name: &'static str =
            Box::leak(format!("{}: {template} lists its requirements", game.key()).into_boxed_str());
        let shown = described(&discovery, game, template);
        checks.push((
            name,
            shown.and_then(|text| {
                let listed = text.starts_with(begins) && !text.contains("Feat Required:");
                check(listed, format!("text {text:?}"))
            }),
        ));
    }
    checks
}

/// The description of the item with template `template` in `game`, as the
/// item screens write it with its template's own upgrades fitted.
fn described(discovery: &vfs::Discovery, game: Game, template: &str) -> Result<String, String> {
    let mut assets = ui::assets::GuiAssets::open(discovery, game)?;
    let look = assets
        .item(template)
        .ok_or_else(|| format!("no item {template}"))?;
    let text = ui::vm::ItemText {
        requirements: look.requirements.clone(),
        stats: look.stats.clone(),
        fitted: look.upgrades,
        critical: look.critical.clone().map(|critical| (critical, look.edges.clone())),
        ..ui::vm::ItemText::default()
    };
    Ok(String::from_utf8_lossy(&text.written(text.fitted, std::iter::empty(), &[])).into_owned())
}

/// A lightsaber's critical threat with a Keen crystal and a Massive
/// Criticals crystal fitted, as both games write it. Keen doubles the
/// threat of 2 to 4, so 17 to 20 threaten. The 2d6 of Massive Criticals
/// follow as "+2-12". A crystal not fitted changes nothing.
fn edges_written() -> (&'static str, Result<String, String>) {
    use ui::assets::{Critical, Edge};
    let critical = Critical {
        heading: b"Critical Threat".to_vec(),
        threat: 2,
        multiplier: 2,
    };
    let edges = vec![
        Edge {
            upgrade: Some(3),
            massive: None,
        },
        Edge {
            upgrade: Some(4),
            massive: Some((2, 12)),
        },
    ];
    let text = ui::vm::ItemText {
        stats: vec![ui::assets::Stat {
            upgrade: None,
            line: b"Damage: 2-16".to_vec(),
        }],
        critical: Some((critical, edges)),
        ..ui::vm::ItemText::default()
    };
    let bare = String::from_utf8_lossy(&text.written(0, std::iter::empty(), &[])).into_owned();
    let fitted =
        String::from_utf8_lossy(&text.written(0b11000, std::iter::empty(), &[])).into_owned();
    (
        "a fitted Keen and Massive Criticals change the critical threat",
        check(
            bare == "Damage: 2-16\nCritical Threat:\n19-20,x2\n\n"
                && fitted == "Damage: 2-16\nCritical Threat:\n17-20,x2 +2-12\n\n",
            format!("bare {bare:?}; fitted {fitted:?}"),
        ),
    )
}
