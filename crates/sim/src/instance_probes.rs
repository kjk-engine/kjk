//! The probes of an area's instance table asked for again (`kjk sim probes
//! --only instances`; `instance::Instances::of`), no install. A list of
//! 800 objects made in play stands for a much visited Ebon Hawk:
//! - the table asked for 100 times, each from a fresh copy of the list,
//!   is made once;
//! - a table kept answers as one made anew does, for every instance;
//! - a list one longer, a level with other companions' templates and a
//!   level of its own are each made anew;
//! - an instance made in a table taken from the kept ones leaves the kept
//!   table as it was.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::combat::Sheet;
use crate::instance::{Instances, MADE_TABLES};
use crate::module::{ObjectData, ObjectKind};
use crate::probe::{Checks, check};
use crate::script::Library;
use crate::snapshot::Spawned;
use crate::world::Level;

/// Objects in the list: about what the Ebon Hawk's table held after a
/// long K1 game (instance numbers past 880 in a player's report).
const MADE: usize = 800;

/// The companion whose template the level's scripts hold. They lack the
/// gizka's.
const COMPANION: &str = "p_carth";
const GIZKA: &str = "g_gizka016";

pub fn probes() -> Checks {
    vec![
        ("asked_again_made_once", asked_again()),
        ("kept_table_answers_as_made", kept_answers()),
        ("changed_sources_made_anew", made_anew()),
        ("making_leaves_kept_alone", making_apart()),
    ]
}

fn spawned(template: &str) -> Spawned {
    Spawned {
        kind: ObjectKind::Creature,
        template: template.to_string(),
        yaw: 0.0,
        holds: Vec::new(),
    }
}

/// The list: a companion, then a gizka, over and over.
fn list() -> Vec<Spawned> {
    (0..MADE)
        .map(|n| spawned(if n % 2 == 0 { COMPANION } else { GIZKA }))
        .collect()
}

/// A level whose scripts hold `COMPANION`'s template.
fn level() -> Level {
    let facts = ObjectData::creature(COMPANION, COMPANION, [0.0; 3], Sheet::commoner());
    let library = Library::new(Default::default(), Default::default(), Default::default())
        .with_creatures(BTreeMap::from([(COMPANION.to_string(), Arc::new(facts))]));
    Level::arena().with_scripts(library)
}

/// How many tables `ask` made on this thread.
fn tables_made(ask: impl FnOnce()) -> usize {
    let before = MADE_TABLES.with(std::cell::Cell::get);
    ask();
    MADE_TABLES.with(std::cell::Cell::get) - before
}

fn asked_again() -> Result<String, String> {
    let level = level();
    let made = tables_made(|| {
        for _ in 0..100 {
            Instances::of(&level, &list());
        }
    });
    check(
        made == 1,
        format!("100 asks for a table of {MADE} objects made {made} tables"),
    )
}

/// Every instance's kind and tag, and whether it has facts.
fn answers(table: &Instances) -> Vec<(Option<ObjectKind>, Option<String>, bool)> {
    (0..=MADE as u32)
        .map(|number| {
            (
                table.kind(number),
                table.tag(number).map(str::to_string),
                table.get(number).is_some(),
            )
        })
        .collect()
}

fn kept_answers() -> Result<String, String> {
    let level = level();
    Instances::of(&level, &list());
    let kept = Instances::of(&level, &list());
    // The same templates, with a companions' map of its own: made anew.
    let anew = Instances::of(&level.clone().with_companions(BTreeMap::new()), &list());
    check(
        answers(&kept) == answers(&anew),
        format!("all {MADE} instances answer alike, kept or made anew"),
    )?;
    check(
        kept.get(0).is_some_and(|facts| facts.tag == COMPANION)
            && kept.get(1).is_none()
            && kept.tag(1) == Some(GIZKA),
        "the companion has its template's facts, and the gizka, whose template the level lacks, is named by its template".into(),
    )
}

fn made_anew() -> Result<String, String> {
    let level = level();
    Instances::of(&level, &list());
    let mut longer = list();
    longer.push(spawned(COMPANION));
    let mut size = 0;
    let made = tables_made(|| size = Instances::of(&level, &longer).made().len());
    check(
        made == 1 && size == MADE + 1,
        format!("a list one longer made {made} tables, of {size} objects"),
    )?;
    let made = tables_made(|| {
        Instances::of(&level.clone().with_companions(BTreeMap::new()), &list());
    });
    check(
        made == 1,
        format!("other companions' templates made {made} tables"),
    )?;
    let made = tables_made(|| {
        Instances::of(&self::level(), &list());
    });
    check(made == 1, format!("another level made {made} tables"))
}

fn making_apart() -> Result<String, String> {
    let level = level();
    let mut taken = Instances::of(&level, &list());
    let number = taken.make(&level, spawned(GIZKA));
    let again = Instances::of(&level, &list());
    check(
        number == MADE as u32 && taken.made().len() == MADE + 1 && again.made().len() == MADE,
        format!(
            "the taken table made number {number} and holds {}, the kept one holds {}",
            taken.made().len(),
            again.made().len()
        ),
    )
}
