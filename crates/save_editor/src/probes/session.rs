//! The editing session: changes replayed onto the opened save, so undo is
//! exact and a refused change leaves nothing (Task 4, Review Focus 5).

use crate::model::catalog::Catalog;
use crate::model::session::{Change, Ground, Session};

const SABER: &str = "g_w_lghtsbr01";

/// The kitchen sink as opened, in a session.
fn sink() -> Session {
    let save = sim::fixtures::kitchen_sink().1;
    Session::new(edit::Opened {
        id: "0001".into(),
        meta: saves::Meta::default(),
        save,
    })
}

/// A saber the party may take, as its only item.
fn armory() -> sim::inventory::Catalog {
    sim::inventory::Catalog::new([sim::inventory::ItemFacts::plain(SABER)])
}

fn credits(save: &sim::Save) -> i32 {
    save.script.gold
}

/// Credits then a saber, undone once: only the credits remain; a refused
/// item is never listed, and undo then takes back the credits.
pub fn undo_exact() -> Result<String, String> {
    let items = armory();
    let ground = Ground::items(&items);
    let mut session = sink();
    let before = session.edited.clone();
    session.apply(Change::Credits(4321), &ground)?;
    session.apply(
        Change::AddItem {
            holder: sim::inventory::PARTY,
            template: SABER.into(),
            stack: 1,
        },
        &ground,
    )?;
    if session.changes.len() != 2
        || !session
            .edited
            .script
            .items
            .values()
            .any(|item| item.template == SABER)
    {
        return Err(format!(
            "{} changes, the saber not given",
            session.changes.len()
        ));
    }
    session.undo(&ground)?;
    let saber_left = session
        .edited
        .script
        .items
        .values()
        .any(|item| item.template == SABER);
    if session.changes.len() != 1 || saber_left || credits(&session.edited) != 4321 {
        return Err(format!(
            "after one undo: {} changes, saber left {saber_left}, {} credits",
            session.changes.len(),
            credits(&session.edited)
        ));
    }
    let refused = session.apply(
        Change::AddItem {
            holder: sim::inventory::PARTY,
            template: "nope".into(),
            stack: 1,
        },
        &ground,
    );
    let Err(why) = refused else {
        return Err("an item no install holds was taken".into());
    };
    if session.changes.len() != 1 || credits(&session.edited) != 4321 {
        return Err(format!("refused, yet {} changes", session.changes.len()));
    }
    session.undo(&ground)?;
    if !session.changes.is_empty() || session.edited != before {
        return Err("undoing the credits left the save changed".into());
    }
    let words = session
        .changes
        .iter()
        .map(|applied| applied.words.clone())
        .collect::<Vec<_>>();
    Ok(format!("undo exact; refused: {why}; {words:?}"))
}

/// Every change is told in words, naming the item by its game name.
pub fn told_in_words() -> Result<String, String> {
    let items = armory();
    let ground = Ground::items(&items);
    let mut session = sink();
    session.apply(Change::Credits(250), &ground)?;
    session.apply(
        Change::AddItem {
            holder: sim::inventory::PARTY,
            template: SABER.into(),
            stack: 2,
        },
        &ground,
    )?;
    let words: Vec<&str> = session
        .changes
        .iter()
        .map(|applied| applied.words.as_str())
        .collect();
    if words != ["credits set to 250", "party: + g_w_lghtsbr01 ×2"] {
        return Err(format!("{words:?}"));
    }
    Ok(format!("{words:?}"))
}

/// With K1's data, a feat whose prerequisites the player lacks is granted
/// all the same and warned of (spec: warn, don't block).
pub fn feat_warned_k1() -> Result<String, String> {
    let discovery = crate::model::locate::discovery();
    if discovery.get(vfs::Game::Kotor1).is_none() {
        return Ok("skipped: no k1 install".into());
    }
    let catalog = Catalog::load(&discovery, vfs::Game::Kotor1)?;
    let ground = Ground::of(Some(&catalog));
    let mut session = sink();
    let player = edit::Who::Player(sim::Seat(0));
    // Master Two-Weapon Fighting: needs the two lesser ranks first.
    session.apply(Change::GrantFeat(player, 85), &ground)?;
    let applied = session.changes.last().ok_or("not listed")?;
    match &applied.warning {
        Some(warning) => Ok(format!("{}: ⚠ {warning}", applied.words)),
        None => Err(format!("{}: no warning", applied.words)),
    }
}
