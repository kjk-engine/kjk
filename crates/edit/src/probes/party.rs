//! Party changes queued in a save and edited saves checked: on the kitchen
//! sink without an install, and with K1's, on a save made in the Endar
//! Spire's first module, whose queued changes run as it loads.

use sim::inventory::{Catalog, PARTY};
use sim::script::calls;
use sim::{Save, Seat};

use crate::{Checked, Editor, PartyChange, Who};

/// K1's Bastila: her party number and template.
const BASTILA: u8 = 0;
const BASTILA_TEMPLATE: &str = "p_bastilla";
const MODULE: &str = "end_m01aa";
const SABER: &str = "g_w_lghtsbr01";

/// The four party changes queued on the kitchen sink read back in words
/// and in order; each queued run's program, read from its name, is the one
/// its calls make; a template with a space is refused, the save unchanged;
/// the save round-trips.
pub(super) fn party_queued() -> Result<String, String> {
    let (_, mut save) = sim::fixtures::kitchen_sink();
    let catalog = Catalog::new([]);
    let before = save.clone();
    if Editor::new(&mut save, &catalog)
        .make_available(BASTILA, "two words")
        .is_ok()
        || save != before
    {
        return Err("a template with a space was queued".into());
    }
    {
        let mut edit = Editor::new(&mut save, &catalog);
        edit.make_available(BASTILA, BASTILA_TEMPLATE)?;
        edit.join(BASTILA)?;
        edit.leave(BASTILA)?;
        edit.remove_available(BASTILA)?;
    }
    let listed = crate::pending_party(&save);
    let want = [
        PartyChange::MadeAvailable {
            npc: BASTILA,
            template: BASTILA_TEMPLATE.into(),
        },
        PartyChange::Joins { npc: BASTILA },
        PartyChange::Leaves { npc: BASTILA },
        PartyChange::Forgotten { npc: BASTILA },
    ];
    if listed != want {
        return Err(format!("queued {listed:?}"));
    }
    let routines = script_ncs::Catalog::parse(ROUTINES).map_err(|error| format!("{error:?}"))?;
    let made = |calls: Vec<calls::Call>| calls::program(&routines, &[calls]);
    let int = |npc: u8| calls::int(i32::from(npc));
    let built = [
        made(vec![
            calls::text(BASTILA_TEMPLATE),
            int(BASTILA),
            calls::Call::Routine("AddAvailableNPCByTemplate", 2),
        ])?,
        made(vec![
            int(BASTILA),
            calls::Call::Routine("RemovePartyMember", 1),
        ])?,
        made(vec![
            int(BASTILA),
            calls::Call::Routine("RemoveAvailableNPC", 1),
        ])?,
    ];
    let pending = &save.script.pending;
    let names = [&pending[0].script, &pending[2].script, &pending[3].script];
    for (name, built) in names.into_iter().zip(built) {
        let read = calls::read(name, &routines).ok_or(format!("{name}: not queued"))??;
        if read != built {
            return Err(format!("{name}: read another program"));
        }
    }
    if crate::round_trip(&save)? != Checked::RoundTrip {
        return Err("no round trip".into());
    }
    let words: Vec<String> = listed.iter().map(ToString::to_string).collect();
    Ok(words.join("; "))
}

/// A check that panics is told as an error naming what it was doing and
/// what the panic said, not unwound into its caller.
pub(super) fn panic_told() -> Result<String, String> {
    let told = crate::check::caught("stepping it", || -> Result<(), String> {
        panic!("a probe's own panic")
    });
    match told {
        Err(why) if why.contains("stepping it") && why.contains("a probe's own panic") => Ok(why),
        other => Err(format!("{other:?}")),
    }
}

/// The routines the queued runs above call, as `nwscript.nss` declares them.
const ROUTINES: &str = r#"
// 0
int AddAvailableNPCByTemplate(int nNPC, string sTemplate);
// 1
int RemovePartyMember(int nNPC);
// 2
int RemoveAvailableNPC(int nNPC);
"#;

/// A K1 save as kjk makes one in `MODULE`: its world from the plan, one
/// player (the dev soldier), stepped once.
fn k1_save(discovery: &vfs::Discovery) -> Result<Save, String> {
    let level = crate::check::planned(discovery, vfs::Game::Kotor1, MODULE, (1, 1))?;
    let config = sim::WorldConfig {
        seed: 1,
        seats: 1,
        wanderers: 0,
        spawn: [0.0; 3],
        yaw: 0.0,
        sheet: sim::combat::Sheet::dev_soldier(),
        guests: sim::Guests::Avatars,
    };
    let mut world = sim::SimWorld::new(config, level.clone()).map_err(|error| error.to_string())?;
    let rate = sim::TickRate::default();
    let input = sim::determinism::pilot(1, 1, 1);
    sim::step(
        &mut world,
        1,
        &input,
        rate.msec(1),
        sim::StepReason::Authority,
    );
    Ok(Save {
        hz: rate.hz(),
        level: level.id(),
        snapshot: world.snapshot(),
        script: world.script_state().clone(),
    })
}

/// K1's install, else why the check is skipped.
fn k1() -> Result<vfs::Discovery, String> {
    let discovery = super::discovery();
    match discovery.get(vfs::Game::Kotor1) {
        Some(_) => Ok(discovery),
        None => Err("skipped: no k1 install".into()),
    }
}

/// With K1: Bastila made available and joined in a save, the save loaded
/// and stepped, she is in the party on a creature of her own; sent off
/// and loaded again, she is not.
pub(super) fn party_joins_k1() -> Result<String, String> {
    let discovery = match k1() {
        Ok(discovery) => discovery,
        Err(skipped) => return Ok(skipped),
    };
    let catalog = Catalog::new([]);
    let mut save = k1_save(&discovery)?;
    {
        let mut edit = Editor::new(&mut save, &catalog);
        edit.make_available(BASTILA, BASTILA_TEMPLATE)?;
        edit.join(BASTILA)?;
    }
    let mut world = crate::check::stepped(&save, &discovery, 10)?;
    let snapshot = world.snapshot();
    let party = &world.script_state().party;
    let body = party.available.get(&BASTILA).and_then(|slot| slot.object);
    let standing = body.is_some_and(|id| snapshot.get(sim::SimId(id)).is_some());
    if !party.members.contains(&BASTILA) || !standing {
        return Err(format!(
            "not joined: members {:?}, body {body:?}",
            party.members
        ));
    }
    let mut joined = saved(world, &save);
    Editor::new(&mut joined, &catalog).leave(BASTILA)?;
    let world = crate::check::stepped(&joined, &discovery, 10)?;
    let members = &world.script_state().party.members;
    if members.contains(&BASTILA) {
        return Err(format!("still a member after leaving: {members:?}"));
    }
    Ok(format!("joined on {body:?}, then left"))
}

/// `world` saved as `like` was (its rate and level).
fn saved(mut world: sim::SimWorld, like: &Save) -> Save {
    Save {
        hz: like.hz,
        level: like.level.clone(),
        snapshot: world.snapshot(),
        script: world.script_state().clone(),
    }
}

/// With K1: credits and a lightsaber given in a real module's save, the
/// saber held, are in the snapshot's goods after a step there (its
/// scripts gather them), and the save passes `load_and_step`; one whose
/// module is not in the install is refused naming it.
pub(super) fn checked_k1() -> Result<String, String> {
    let discovery = match k1() {
        Ok(discovery) => discovery,
        Err(skipped) => return Ok(skipped),
    };
    let source = crate::InstallSource::open(&discovery, vfs::Game::Kotor1)?;
    let saber = session::item_facts(&source, SABER).ok_or("no lightsaber in the install")?;
    let catalog = Catalog::new([saber]);
    let mut save = k1_save(&discovery)?;
    let saber = {
        let mut edit = Editor::new(&mut save, &catalog);
        edit.set_credits(777)?;
        let saber = edit.add_item(PARTY, SABER, 1)?;
        edit.equip(
            &Who::Player(Seat(0)),
            saber,
            sim::inventory::slots::RIGHT_WEAPON,
        )?;
        saber
    };
    let checked = crate::load_and_step(&save, &discovery, 60)?;
    if checked != (Checked::Stepped { ticks: 60 }) {
        return Err(format!("{checked:?}"));
    }
    let mut world = crate::check::stepped(&save, &discovery, 1)?;
    let goods = world.snapshot().goods;
    let held = goods
        .items
        .iter()
        .any(|(id, item)| *id == saber && item.template == SABER);
    if goods.gold != 777 || !held {
        return Err(format!("goods: {} credits, saber {held}", goods.gold));
    }
    let mut lost = save.clone();
    if let Some(place) = lost.snapshot.place.as_mut() {
        place.module = "kjk_no_module".into();
    }
    match crate::load_and_step(&lost, &discovery, 1) {
        Err(why) if why.contains("kjk_no_module") => {
            Ok(format!("{checked:?}, goods hold both; lost: {why}"))
        }
        other => Err(format!("a missing module: {other:?}")),
    }
}
