//! Companions where a save really keeps them: a member placed by its module
//! (no sheet of its own), one away whose creature stands in the module, one
//! the table keeps dressed otherwise than its items say; party changes held
//! to the table; a queued change that cannot happen; an equip the rules
//! would refuse, warned of and made.

use sim::inventory::PARTY;
use sim::inventory::slots::{COUNT, RIGHT_WEAPON};
use sim::module::ObjectKind;
use sim::script::party::Npc;
use sim::{Kind, Save, Seat, SimId};

use super::party::{k1, k1_save};
use crate::{Editor, Who};

/// Companion `npc` of `save` on body `id`, in the party or not.
fn on_body(save: &mut Save, npc: u8, id: SimId, member: bool) {
    let slot = save
        .script
        .party
        .available
        .entry(npc)
        .or_insert_with(|| Npc {
            template: "p_probe".into(),
            object: None,
            kept: None,
            template_data: None,
        });
    slot.object = Some(id.0);
    if let Some(kept) = slot.kept.as_mut() {
        kept.from = id.0;
    }
    if member {
        save.script.party.members.push(npc);
    }
}

/// With K1: a creature `end_m01aa` places, its body holding no sheet, made
/// a member, is refused an edit without the level in words that say so,
/// and edited from its module's sheet with it.
pub(super) fn placed_member_k1() -> Result<String, String> {
    let discovery = match k1() {
        Ok(discovery) => discovery,
        Err(skipped) => return Ok(skipped),
    };
    let (level, mut save) = k1_save(&discovery)?;
    let placed = save.snapshot.entities.iter().find(|entity| {
        entity.sheet.is_none()
            && matches!(entity.kind, Kind::Object(ObjectKind::Creature, _))
            && crate::characters::module_sheet(entity.kind, Some(&level)).is_some()
    });
    let id = placed.ok_or("no placed creature without a sheet")?.id;
    on_body(&mut save, 1, id, true);
    let who = Who::Companion { npc: 1, id };
    let catalog = sim::inventory::Catalog::new([]);
    let refused = Editor::new(&mut save, &catalog).set_alignment(&who, 77);
    let words = match refused {
        Err(why) if why.contains("with_level") => why,
        other => return Err(format!("without the level: {other:?}")),
    };
    Editor::new(&mut save, &catalog)
        .with_level(&level)
        .set_alignment(&who, 77)?;
    let sheet = save.snapshot.get(id).and_then(|entity| entity.sheet);
    if sheet.map(|sheet| sheet.good_evil) != Some(77) {
        return Err(format!("not written: {:?}", sheet.map(|s| s.good_evil)));
    }
    crate::round_trip(&save)?;
    Ok(format!(
        "{id:?} edited from its module's sheet; without the level: {words}"
    ))
}

/// The kitchen sink's companion 0, away and kept, with a creature of its
/// own standing in the module: the body made for it.
fn standing_away(save: &mut Save) -> Result<(Who, SimId), String> {
    let away = super::kept_away(save)?;
    let mut body = save.snapshot.entities[0];
    body.id = SimId(
        save.snapshot
            .entities
            .iter()
            .map(|e| e.id.0)
            .max()
            .unwrap_or(0)
            + 1,
    );
    body.kind = Kind::Object(ObjectKind::Creature, 9_999);
    body.sheet = Some(sim::combat::sheet::Sheet::commoner());
    body.fighter = None;
    save.snapshot.entities.push(body);
    on_body(save, 0, body.id, false);
    Ok((away, body.id))
}

/// A companion away whose creature stands in the module is edited on
/// that body (what joining takes) and in the table both, and listed so.
pub(super) fn away_standing() -> Result<String, String> {
    let (_, mut save) = sim::fixtures::kitchen_sink();
    let (away, id) = standing_away(&mut save)?;
    let catalog = sim::inventory::Catalog::new([]);
    Editor::new(&mut save, &catalog).set_alignment(&away, 80)?;
    let body = save
        .snapshot
        .get(id)
        .and_then(|entity| entity.sheet)
        .map(|s| s.good_evil);
    let kept = save.script.party.available[&0]
        .kept
        .map(|kept| kept.sheet.good_evil);
    let listed = super::sheet_of(&save, away)?.good_evil;
    if (body, kept, listed) != (Some(80), Some(80), 80) {
        return Err(format!("body {body:?}, kept {kept:?}, listed {listed}"));
    }
    Ok("its body, the table and the list at 80".into())
}

/// A companion the table keeps with a saber in its kept slots, though the
/// item says it is worn nowhere (stripped after it was kept), takes it off
/// as the table keeps it: the saber back with the party, the slot empty.
pub(super) fn kept_slots_rule() -> Result<String, String> {
    let (_, mut save) = sim::fixtures::kitchen_sink();
    let away = super::kept_away(&mut save)?;
    let catalog = super::armory();
    let from = save.script.party.available[&0].kept.ok_or("not kept")?.from;
    let saber = Editor::new(&mut save, &catalog).add_item(from, super::SABER, 1)?;
    let mut worn = [0; COUNT as usize];
    worn[usize::from(RIGHT_WEAPON)] = saber;
    if let Some(kept) = save
        .script
        .party
        .available
        .get_mut(&0)
        .and_then(|slot| slot.kept.as_mut())
    {
        kept.worn = Some(worn);
    }
    Editor::new(&mut save, &catalog).unequip(&away, RIGHT_WEAPON)?;
    let holder = save.script.items.get(&saber).map(|item| item.holder);
    let kept = save.script.party.available[&0]
        .kept
        .and_then(|kept| kept.worn);
    let slot = kept.map(|worn| worn[usize::from(RIGHT_WEAPON)]);
    if holder != Some(PARTY) || slot != Some(0) {
        return Err(format!("saber with {holder:?}, kept slot {slot:?}"));
    }
    Ok("taken off as the table keeps it".into())
}

/// Party changes the table would not take are refused, nothing queued:
/// joining twice, joining one not available, sending off one not in the
/// party, forgetting one not available.
pub(super) fn party_held_to_table() -> Result<String, String> {
    let (_, mut save) = sim::fixtures::kitchen_sink();
    let catalog = sim::inventory::Catalog::new([]);
    Editor::new(&mut save, &catalog).join(0)?;
    let before = save.clone();
    type Change = fn(&mut Editor) -> Result<(), String>;
    let refusals: [(&str, Change); 4] = [
        ("join twice", |e| e.join(0)),
        ("join one not available", |e| e.join(9)),
        ("send off one not in the party", |e| e.leave(9)),
        ("forget one not available", |e| e.remove_available(9)),
    ];
    let mut told = Vec::new();
    for (name, change) in refusals {
        match change(&mut Editor::new(&mut save, &catalog)) {
            Err(why) if save == before => told.push(why),
            other => return Err(format!("{name}: {other:?}")),
        }
    }
    Ok(told.join("; "))
}

/// With K1: a companion made available by a template the install lacks
/// and joined cannot join as the save loads: `load_and_step` says so,
/// naming the change.
pub(super) fn unmade_change_k1() -> Result<String, String> {
    let discovery = match k1() {
        Ok(discovery) => discovery,
        Err(skipped) => return Ok(skipped),
    };
    let (_, mut save) = k1_save(&discovery)?;
    let catalog = sim::inventory::Catalog::new([]);
    {
        let mut edit = Editor::new(&mut save, &catalog);
        edit.make_available(3, "kjk_no_template")?;
        edit.join(3)?;
    }
    match crate::load_and_step(&save, &discovery, 5) {
        Err(why) if why.contains("companion 3 joins the party") => Ok(why),
        other => Err(format!("{other:?}")),
    }
}

/// A blade that needs a feat the player lacks is put on all the same (the
/// spec: warn, don't block), the warning naming the feat.
pub(super) fn equip_warned() -> Result<String, String> {
    let (_, mut save) = sim::fixtures::kitchen_sink();
    let catalog = super::armory();
    let player = Who::Player(Seat(0));
    let mut edit = Editor::new(&mut save, &catalog);
    let blade = edit.add_item(PARTY, super::MASTERS, 1)?;
    let warning = edit.equip_warning(&player, blade)?.ok_or("no warning")?;
    edit.equip(&player, blade, RIGHT_WEAPON)?;
    let slot = save.script.items.get(&blade).and_then(|item| item.slot);
    if !warning.contains("feat 255") || slot != Some(RIGHT_WEAPON) {
        return Err(format!("{warning}; slot {slot:?}"));
    }
    Ok(format!("worn, warned: {warning}"))
}
