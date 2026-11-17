//! Feats, powers and the per-character numbers, each edit through kjk's
//! own rules, on a player and on a companion the party table keeps away.

use sim::combat::sheet::{Rows, Sheet, feats};
use sim::inventory::Catalog;
use sim::{Kind, Save, Seat};

use crate::{Character, Editor, Who};

type Edit = fn(&mut Editor, &Who) -> Result<(), String>;

/// The kitchen sink at `level`, its player's sheet that level, its
/// companion 0 kept away: the save and both characters.
fn sink_at(level: u8) -> Result<(sim::Level, Save, [Who; 2]), String> {
    let (world, mut save) = sim::fixtures::kitchen_sink();
    let away = super::kept_away(&mut save)?;
    for entity in &mut save.snapshot.entities {
        if let (Kind::Avatar(Seat(0)), Some(sheet)) = (entity.kind, entity.sheet.as_mut()) {
            sheet.level = level;
        }
    }
    if let Some(kept) = save
        .script
        .party
        .available
        .get_mut(&0)
        .and_then(|slot| slot.kept.as_mut())
    {
        kept.sheet.level = level;
    }
    Ok((world, save, [Who::Player(Seat(0)), away]))
}

fn character(save: &Save, who: Who) -> Result<Character, String> {
    crate::characters(save, None)
        .into_iter()
        .find(|character| character.who == who)
        .ok_or(format!("{who:?} is not listed"))
}

/// Every feat row granted then taken away, at levels 1, 10 and 20, on a
/// player and on one away: the save as it was each time, and Toughness's
/// and Force Sensitive's gifts seen while known (Review Focus 4).
pub(super) fn learn_unlearn_identity() -> Result<String, String> {
    let catalog = Catalog::new([]);
    let gifts = [
        feats::TOUGHNESS[0],
        feats::TOUGHNESS[1],
        feats::FORCE_SENSITIVE,
    ];
    let mut undone = 0;
    for level in [1, 10, 20] {
        let (_, mut save, whos) = sink_at(level)?;
        for who in whos {
            let before = save.clone();
            let was = character(&save, who)?.sheet;
            for feat in (0..sim::combat::sheet::Feats::ROWS).filter(|feat| !was.feats.has(*feat)) {
                Editor::new(&mut save, &catalog).grant_feat(&who, feat)?;
                let known = character(&save, who)?.sheet;
                let gave = (known.max_hp, known.max_fp) != (was.max_hp, was.max_fp);
                if gave != gifts.contains(&feat) {
                    return Err(format!("{who:?} level {level}: feat {feat} gave {gave}"));
                }
                Editor::new(&mut save, &catalog).remove_feat(&who, feat)?;
                if save != before {
                    return Err(format!("{who:?} level {level}: feat {feat} not undone"));
                }
                undone += 1;
            }
        }
    }
    Ok(format!("{undone} feats granted and taken back"))
}

/// Each setter on a player and on one away, the save round-tripped and
/// stepped after it, the character as the setter says; refusals change
/// nothing.
pub(super) fn sheet_setters() -> Result<String, String> {
    type Check = fn(&Save, &Character, &Character) -> bool;
    let cases: [(&str, Edit, Check); 10] = [
        (
            "constitution +4",
            |e, w| e.set_ability(w, 2, character(e.save, *w)?.sheet.abilities[2] + 4),
            |_, was, now| {
                now.sheet.max_hp == was.sheet.max_hp + 2 * u16::from(was.sheet.level.max(1))
            },
        ),
        (
            "strength",
            |e, w| e.set_ability(w, 0, 18),
            |_, _, now| now.sheet.abilities[0] == 18,
        ),
        (
            "skill",
            |e, w| e.set_skill(w, 3, 7),
            |_, _, now| now.sheet.skills[3] == 7,
        ),
        (
            "alignment",
            |e, w| e.set_alignment(w, 80),
            |_, _, now| now.sheet.good_evil == 80,
        ),
        (
            "hp past most",
            |e, w| e.set_hp(w, i32::MAX),
            |_, _, now| now.hp == i32::from(now.sheet.max_hp) * 100,
        ),
        ("hp", |e, w| e.set_hp(w, 150), |_, _, now| now.hp == 150),
        ("fp", |e, w| e.set_fp(w, 0), |_, _, now| now.fp == 0),
        (
            "power",
            |e, w| e.grant_power(w, 37),
            |_, _, now| now.sheet.powers.has(37),
        ),
        (
            "toughness",
            |e, w| e.grant_feat(w, feats::TOUGHNESS[0]),
            |_, was, now| now.sheet.max_hp == was.sheet.max_hp + u16::from(was.sheet.level),
        ),
        (
            "xp",
            |e, w| e.set_xp(w, 1_234),
            |save, _, _| save.script.xp.values().any(|xp| *xp == 1_234),
        ),
    ];
    let refusals: [(&str, Edit); 5] = [
        ("alignment 101", |e, w| e.set_alignment(w, 101)),
        ("ability 6", |e, w| e.set_ability(w, 6, 10)),
        ("score 0", |e, w| e.set_ability(w, 0, 0)),
        ("feat past the rows", |e, w| {
            e.grant_feat(w, sim::combat::sheet::Feats::ROWS)
        }),
        ("unknown power", |e, w| e.remove_power(w, 0)),
    ];
    let catalog = Catalog::new([]);
    for level in [1, 12] {
        let (world, base, whos) = sink_at(level)?;
        for who in whos {
            let was = character(&base, who)?;
            for (name, edit, check) in cases {
                let mut save = base.clone();
                edit(&mut Editor::new(&mut save, &catalog), &who)
                    .map_err(|why| format!("{name}: {why}"))?;
                super::stepped(&save, &world).map_err(|why| format!("{name}: {why}"))?;
                if !check(&save, &was, &character(&save, who)?) {
                    return Err(format!("{who:?} level {level}: {name} not as set"));
                }
            }
            for (name, edit) in refusals {
                let mut save = base.clone();
                let refused = edit(&mut Editor::new(&mut save, &catalog), &who);
                if refused.is_ok() || save != base {
                    return Err(format!("{who:?}: {name} was not refused cleanly"));
                }
            }
        }
    }
    Ok(format!(
        "{} setters and {} refusals on a player and one away",
        cases.len(),
        refusals.len()
    ))
}

/// A level-1 soldier (class 0) is warned off Master Speed (power 37) and
/// Master Two-Weapon Fighting (feat 85), in words, and not off every feat
/// its class lists.
pub(super) fn pick_warnings(game: vfs::Game) -> Result<String, String> {
    let discovery = super::discovery();
    if discovery.get(game).is_none() {
        return Ok(format!("skipped: no {} install", game.key()));
    }
    let classes = session::classes::classes(&crate::InstallSource::open(&discovery, game)?);
    let sheet = Sheet {
        level: 1,
        ..Sheet::commoner()
    };
    let (hp, fp) = crate::characters::points(None, &sheet);
    let soldier = Character {
        who: Who::Player(Seat(0)),
        template: String::new(),
        sheet,
        hp,
        fp,
        career: vec![(0, 1)],
    };
    let speed = crate::power_warning(&classes, &soldier, 37).ok_or("Master Speed: no warning")?;
    let master = crate::feat_warning(&classes, &soldier, feats::TWO_WEAPON_FIGHTING[2])
        .ok_or("Master Two-Weapon Fighting: no warning")?;
    let class = classes.classes.first().ok_or("no classes")?;
    let allowed = class
        .feats
        .iter()
        .filter(|feat| crate::feat_warning(&classes, &soldier, **feat).is_none())
        .count();
    if allowed == 0 {
        return Err("every feat of the class warned".into());
    }
    Ok(format!(
        "Master Speed: {speed}; Master TWF: {master}; {allowed} feats free"
    ))
}

/// A level-8 soldier (class 0), a player and one away, changed to a Jedi
/// Guardian (class 3) and back: the Jedi has Force dice the soldier lacks,
/// the round trip gives back the soldier's base attack bonus, saves,
/// defence, hit and Force dice and career, and the save still steps. A
/// second class is added with no levels and a third is refused. A TSL
/// companion's influence is set within 0 to the most and refused past it.
pub(super) fn class_and_influence(game: vfs::Game) -> Result<String, String> {
    use sim::script::party::{INFLUENCE_SLOTS, MAX_INFLUENCE};
    let discovery = super::discovery();
    if discovery.get(game).is_none() {
        return Ok(format!("skipped: no {} install", game.key()));
    }
    let classes = session::classes::classes(&crate::InstallSource::open(&discovery, game)?);
    let catalog = Catalog::new([]);
    let (world, mut save, whos) = sink_at(8)?;
    for who in whos {
        let holder = crate::holder_of(&save, &who).ok_or(format!("{who:?}: no holder"))?;
        save.script.careers.insert(holder, vec![(0, 8)]);
    }
    let numbers = |sheet: &Sheet| {
        (
            sheet.bab,
            sheet.saves,
            sheet.defence,
            sheet.max_hp,
            sheet.max_fp,
        )
    };
    for who in whos {
        let was = character(&save, who)?;
        let mut edit = Editor::new(&mut save, &catalog).with_classes(&classes);
        edit.change_class(&who, 0, 3)?;
        let jedi = character(edit.save(), who)?;
        if jedi.career != vec![(3, 8)] || jedi.sheet.max_fp <= was.sheet.max_fp {
            return Err(format!("{who:?} as a Jedi: {:?}", numbers(&jedi.sheet)));
        }
        edit.change_class(&who, 3, 0)?;
        let back = character(edit.save(), who)?;
        if numbers(&back.sheet) != numbers(&was.sheet) || back.career != was.career {
            return Err(format!(
                "{who:?} back: {:?}, not {:?}",
                numbers(&back.sheet),
                numbers(&was.sheet)
            ));
        }
        edit.add_class(&who, 3)?;
        if character(edit.save(), who)?.career != vec![(0, 8), (3, 0)] {
            return Err(format!("{who:?}: the second class is not added"));
        }
        edit.set_class_levels(&who, 3, 2)?;
        let raised = character(edit.save(), who)?;
        let needed = classes.next(9).ok_or("no XP for level 10")?;
        let party = edit.save().script.xp(sim::inventory::PARTY);
        if raised.career != vec![(0, 8), (3, 2)] || raised.sheet.level != 10 || party < needed {
            return Err(format!(
                "{who:?} raised: {:?} at level {}, party XP {party} (needs {needed})",
                raised.career, raised.sheet.level
            ));
        }
        if raised.sheet.max_fp <= was.sheet.max_fp || raised.sheet.max_hp <= was.sheet.max_hp {
            return Err(format!(
                "{who:?}: two Jedi levels gave no hit or Force points"
            ));
        }
        let before = edit.save().clone();
        let refused = [
            edit.add_class(&who, 4).is_ok(),
            edit.change_class(&who, 1, 2).is_ok(),
            edit.change_class(&who, 0, 3).is_ok(),
            edit.set_class_levels(&who, 3, 1).is_ok(),
            edit.set_class_levels(&who, 1, 3).is_ok(),
        ];
        if refused.contains(&true) || *edit.save() != before {
            return Err(format!("{who:?}: a class change was not refused cleanly"));
        }
    }
    super::stepped(&save, &world)?;
    let mut edit = Editor::new(&mut save, &catalog);
    edit.set_influence(0, MAX_INFLUENCE)?;
    let before = edit.save().clone();
    let refused = [
        edit.set_influence(0, MAX_INFLUENCE + 1).is_ok(),
        edit.set_influence(INFLUENCE_SLOTS, 50).is_ok(),
    ];
    if refused.contains(&true) || *edit.save() != before {
        return Err("an influence past its limits was not refused cleanly".into());
    }
    if save.script.party.influence(0) != i32::from(MAX_INFLUENCE) {
        return Err("the influence is not set".into());
    }
    Ok("soldier to Jedi and back on a player and one away; influence set".into())
}
