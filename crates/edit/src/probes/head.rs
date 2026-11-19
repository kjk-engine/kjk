//! A player's head preset changed: another player portrait of the same
//! gender, with the body the player's first class takes, kept through a
//! save and a load; and the body following a change of first class.

use session::portraits::Portraits;
use sim::Seat;
use sim::inventory::Catalog;
use sim::persona::Persona;

use crate::{Editor, Who};

const PLAYER: Who = Who::Player(Seat(0));

/// A `portraits.2da` of four rows: 0 a woman's player portrait (bodies 90,
/// 91 and 92 by build), 1 another (100, 101, 102), 2 a man's (110, 111,
/// 112), 3 a woman's that is not a player's.
fn table() -> fmt_odyssey::twoda::TwoDa {
    let text = |cells: &[&str]| cells.iter().map(|cell| cell.as_bytes().to_vec()).collect();
    fmt_odyssey::twoda::TwoDa {
        columns: text(&[
            "baseresref",
            "sex",
            "forpc",
            "appearance_l",
            "appearancenumber",
            "appearance_s",
        ]),
        row_labels: text(&["0", "1", "2", "3"]),
        cells: vec![
            text(&["po_pfa1", "1", "1", "90", "91", "92"]),
            text(&["po_pfa2", "1", "1", "100", "101", "102"]),
            text(&["po_pma1", "0", "1", "110", "111", "112"]),
            text(&["po_npc", "1", "0", "120", "121", "122"]),
        ],
        column_separator: b'\t',
        row_separator: b'\t',
    }
}

fn persona(save: &sim::Save) -> Result<Persona, String> {
    sim::persona::of(&save.snapshot.personas, Seat(0))
        .cloned()
        .ok_or("the player has no persona".into())
}

/// The kitchen sink's player, a woman and a scoundrel (the small build),
/// changed to portrait 1: its head and its small body, her gender and name
/// kept. A man's portrait, a portrait not a player's, a companion and an
/// editor without the game's portraits are refused, the save unchanged.
/// The persona reads back from the save's bytes and stands in the world
/// loaded from them. Without a class recorded, the body keeps the build
/// worn.
pub(super) fn head_preset() -> Result<String, String> {
    let (level, mut save) = sim::fixtures::kitchen_sink();
    let catalog = Catalog::new([]);
    let portraits = Portraits::of(&table());
    let holder = crate::holder_of(&save, &PLAYER).ok_or("the player has no holder")?;
    save.script.careers.insert(holder, vec![(2, 8)]);
    let was = persona(&save)?;
    Editor::new(&mut save, &catalog)
        .with_portraits(&portraits)
        .set_portrait(&PLAYER, 1)?;
    let want = Persona {
        appearance: 102,
        portrait: 1,
        ..was.clone()
    };
    if persona(&save)? != want {
        return Err(format!("changed to {:?}, not {want:?}", persona(&save)?));
    }
    let before = save.clone();
    let refusals = {
        let mut edit = Editor::new(&mut save, &catalog).with_portraits(&portraits);
        [
            edit.set_portrait(&PLAYER, 2).is_err(),
            edit.set_portrait(&PLAYER, 3).is_err(),
            edit.set_portrait(&Who::Away { npc: 0 }, 0).is_err(),
        ]
    };
    let bare = Editor::new(&mut save, &catalog).set_portrait(&PLAYER, 0);
    if refusals.contains(&false) || bare.is_ok() || save != before {
        return Err(format!(
            "not refused cleanly: {refusals:?}, without portraits {bare:?}"
        ));
    }
    let mut world = super::stepped(&save, &level)?;
    let loaded = sim::persona::of(&world.snapshot().personas, Seat(0)).cloned();
    if loaded.as_ref() != Some(&want) {
        return Err(format!("loaded as {loaded:?}"));
    }
    save.script.careers.remove(&holder);
    Editor::new(&mut save, &catalog)
        .with_portraits(&portraits)
        .set_portrait(&PLAYER, 0)?;
    let classless = persona(&save)?;
    if classless.appearance != 92 || classless.portrait != 0 {
        return Err(format!("with no class recorded: {classless:?}"));
    }
    Ok(format!(
        "portrait {} body {} to portrait 1 body 102, kept through a load; 3 refusals; no class: body 92",
        was.portrait, was.appearance
    ))
}

/// With `game`'s classes and portraits, the player given the first player
/// portrait of her gender as a soldier wears its large body. Her first
/// class changed to Jedi Consular, she wears its small body. Her second
/// class changed, the body stays.
pub(super) fn class_body(game: vfs::Game) -> Result<String, String> {
    let discovery = super::discovery();
    if discovery.get(game).is_none() {
        return Ok(format!("skipped: no {} install", game.key()));
    }
    let source = crate::InstallSource::open(&discovery, game)?;
    let classes = session::classes::classes(&source);
    let portraits = Portraits::read(&source).ok_or("portraits.2da does not read")?;
    let catalog = Catalog::new([]);
    let (_, mut save) = sim::fixtures::kitchen_sink();
    let holder = crate::holder_of(&save, &PLAYER).ok_or("the player has no holder")?;
    save.script.careers.insert(holder, vec![(0, 4), (5, 2)]);
    let was = persona(&save)?;
    let row = portraits
        .all()
        .iter()
        .find(|portrait| portrait.female == was.female())
        .ok_or("no player portrait of her gender")?
        .row;
    let worn = |class: u8| portraits.worn(&was, row, Some(class));
    let mut edit = Editor::new(&mut save, &catalog)
        .with_classes(&classes)
        .with_portraits(&portraits);
    edit.set_portrait(&PLAYER, row)?;
    let soldier = persona(edit.save())?;
    edit.change_class(&PLAYER, 0, 4)?;
    let consular = persona(edit.save())?;
    edit.change_class(&PLAYER, 5, 1)?;
    let second = persona(edit.save())?;
    if Some(&soldier) != worn(0).as_ref()
        || Some(&consular) != worn(4).as_ref()
        || soldier.appearance == consular.appearance
        || second != consular
    {
        return Err(format!(
            "soldier {}, consular {}, second class changed {}",
            soldier.appearance, consular.appearance, second.appearance
        ));
    }
    Ok(format!(
        "portrait {row}: soldier body {}, Jedi Consular body {}",
        soldier.appearance, consular.appearance
    ))
}
