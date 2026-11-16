//! A companion drawn from what the party table keeps of it (issue 309).
//! Dark side Bastila's template (`p_bastilla004`) ships only in the Temple
//! Summit (`unk_m44ac`). A session that loads a save on the Star Forge
//! (`sta_m45aa`) has never opened that module, so it must draw her from
//! the party table's facts.

use fmt_odyssey::gff::Gff;
use sim::inventory::Held;
use sim::module::{Body, ObjectData};

use super::{CreatureLoad, Kept, Placed, resolver};
use crate::probe_kit::{Checks, check, not_installed};

const TEMPLATE: &str = "p_bastilla004";
const OWN_MODULE: &str = "unk_m44ac";
const ELSEWHERE: &str = "sta_m45aa";

/// A creature with no `appearance.2da` row keeps nothing. With a K1
/// install, dark side Bastila placed on the Star Forge from her kept facts
/// is drawn with the same models her own module draws her with.
pub fn probes() -> Checks {
    vec![
        ("kept_needs_an_appearance", no_appearance()),
        ("kept_companion_drawn", drawn_elsewhere()),
    ]
}

fn no_appearance() -> Result<String, String> {
    let kept = Kept::of(&ObjectData::unknown(TEMPLATE, [0.0; 3], 0.0));
    check(kept.is_none(), format!("{kept:?}"))
}

/// The facts the session reads off a creature's UTC that `Kept` uses: its
/// appearance and its worn items, each in the slot its struct id's bit
/// names.
fn facts(utc: &Gff) -> Result<ObjectData, String> {
    let appearance = utc
        .root
        .int("Appearance_Type")
        .and_then(|row| u16::try_from(row).ok())
        .ok_or("no Appearance_Type")?;
    let mut facts = ObjectData::unknown(TEMPLATE, [0.0; 3], 0.0);
    facts.body = Some(Body {
        appearance,
        size: 0,
        movement: 0,
        walk_mps: 0.0,
        run_mps: 0.0,
        head_arc_h: 0.0,
        bag: 0,
        destroy_ms: 0,
        fade_ms: None,
        bulk: None,
    });
    facts.inventory = utc
        .root
        .list("Equip_ItemList")
        .iter()
        .filter_map(|item| {
            Some(Held {
                template: item.resref("EquippedRes")?,
                stack: 1,
                slot: Some(item.id.trailing_zeros() as u8),
                droppable: false,
                infinite: false,
            })
        })
        .collect();
    Ok(facts)
}

fn names(models: &[Placed]) -> Vec<String> {
    models.iter().map(|placed| placed.ir.name.clone()).collect()
}

fn drawn_elsewhere() -> Result<String, String> {
    let discovery = crate::discover();
    let game = vfs::Game::Kotor1;
    if discovery.get(game).is_none() {
        return Ok(not_installed(game));
    }
    let own = resolver(&discovery, game, Some(OWN_MODULE))?;
    let elsewhere = resolver(&discovery, game, Some(ELSEWHERE))?;
    let utc = own
        .read(TEMPLATE, fmt_odyssey::ResType::UTC)
        .and_then(|bytes| Gff::parse(&bytes).ok())
        .ok_or(format!("{OWN_MODULE} has no {TEMPLATE}"))?;
    let kept = Kept::of(&facts(&utc)?).ok_or("nothing kept")?;
    let at = ([0.0; 3], 0.0);
    let mut load = CreatureLoad::default();
    let lacks = !load.has(&elsewhere, TEMPLATE);
    let mut kept_models = Vec::new();
    let kept_view = load.place_kept(
        (&elsewhere, game),
        (1, TEMPLATE, &kept),
        at,
        &mut kept_models,
    )?;
    let mut own_models = Vec::new();
    let own_view =
        CreatureLoad::default().place((&own, game), (1, TEMPLATE), at, &mut own_models)?;
    let (kept_names, own_names) = (names(&kept_models), names(&own_models));
    check(
        lacks
            && kept_view.is_some()
            && own_view.is_some()
            && !kept_names.is_empty()
            && kept_names == own_names,
        format!(
            "{ELSEWHERE} lacks the template {lacks}, kept {kept:?} draws {kept_names:?}, \
             {OWN_MODULE} draws {own_names:?}"
        ),
    )
}
