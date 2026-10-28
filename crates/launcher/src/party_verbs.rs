//! `kjk script party <game> [module]`: the party table checked end to end
//! (I19, `sim::script::party`). Two scripts made here (`probe`) run as the
//! player in the module (the Endar Spire by default):
//!
//! 0. The goods tell the body armour of every creature placed wearing
//!    some, as its template put it on (what dresses its body).
//! 1. Three of its creatures made available (`NPC_*` 0 to 2), all asked to
//!    join, and one never made available asked too: the first two join
//!    (they follow the first seat, in the player's faction), the third is
//!    refused (the party is three), the unavailable one too; the party
//!    counts three and its second member is the first to join.
//! 2. The first leaves: it fights for itself again; a save carries the
//!    table.
//! 3. A companion spawned from its template joins as it is made; a
//!    creature made by `CreateObject` does not.
//! 4. The party selection screen a script shows (`ShowPartySelectionGUI`,
//!    forcing a fourth companion in), told with the table (each available
//!    companion's template, the members, the forced, and what the third,
//!    spawned from its template, wears on its body: its template's armour,
//!    which its 3D view is dressed in), closed with the
//!    second picked: the third leaves and goes, the fourth is made beside
//!    the player and joins, and the screen's script runs once the screen
//!    closes. The party screen opened in play (`Action::ShowParty`) is
//!    told the table too, to look at.
//! 5. The fourth made leader (`Action::SetLeader`, by its number):
//!    it is party member 0 to the scripts.
//! 6. The party follows into the next module.
//! 7. On the Endar Spire, Carth put in the party by hand (`--follow`) is
//!    `npc.2da`'s NPC 2 and stays through the next module's own scripts.

use std::process::ExitCode;

use serde_json::json;
use sim::notice::{Notice, Roster, Screen};
use vfs::Discovery;

use crate::probe::{Call, Probe, int, me, text};
use crate::scenes::Report;
use crate::script_verbs::{catalog, open};

/// `SetGlobalNumber(name, <value>)`: the value's calls, then the name.
fn keep(name: &str, value: Vec<Call>) -> Vec<Call> {
    let mut calls = value;
    calls.push(text(name));
    calls.push(Call::Routine("SetGlobalNumber", 2));
    calls
}

/// `GetObjectByTag(tag, nth)`.
fn by_tag(tag: &str, nth: i32) -> Vec<Call> {
    vec![int(nth), text(tag), Call::Routine("GetObjectByTag", 2)]
}

/// `routine(npc, GetObjectByTag(tag, nth))` kept as `name`.
fn with_creature(name: &str, routine: &'static str, npc: i32, tag: &str, nth: i32) -> Vec<Call> {
    let mut value = by_tag(tag, nth);
    value.push(int(npc));
    value.push(Call::Routine(routine, 2));
    keep(name, value)
}

/// The party tables the party selection screens told carry.
fn rosters(told: &[Notice]) -> Vec<Roster> {
    told.iter()
        .filter_map(|notice| match notice {
            Notice::Show(Screen::PartySelection(roster)) => Some(roster.clone()),
            _ => None,
        })
        .collect()
}

/// The screen with `roster` reads back from its bytes as it was.
fn told_again(roster: &Roster) -> bool {
    let screen = Screen::PartySelection(roster.clone());
    Screen::from_bytes(&screen.to_bytes()) == Ok(screen)
}

pub fn party(discovery: &Discovery, game: &str, module: &str) -> ExitCode {
    let mut report = Report::default();
    let run = |report: &mut Report| -> Result<serde_json::Value, String> {
        let (game, _, base) = open(discovery, game)?;
        let routines = catalog(&base)?;
        let mut chosen = Vec::new();
        let mut templates = Vec::new();
        let mut probe = Probe::new(discovery, game, module, |level| {
            let data = level.module().ok_or("no module")?;
            let own = level.scripts().ok_or("the module has no scripts")?;
            // A companion's template (KOTOR's are `p_…`) and another
            // creature's, from the library's.
            let creatures = own.creatures().clone();
            let companion = creatures
                .keys()
                .find(|name| name.starts_with("p_"))
                .or_else(|| creatures.keys().next())
                .ok_or("no creature templates")?
                .clone();
            let other = creatures
                .keys()
                .find(|name| **name != companion)
                .ok_or("one creature template")?
                .clone();
            let creature_type = routines
                .constant_named("OBJECT_TYPE_CREATURE")
                .and_then(|constant| match constant.value {
                    script_ncs::catalog::Value::Int(value) => Some(value),
                    _ => None,
                })
                .ok_or("no OBJECT_TYPE_CREATURE")?;
            let here = || vec![me(), Call::Routine("GetLocation", 1)];
            let mut third = vec![
                text(&companion),
                int(3),
                Call::Routine("AddAvailableNPCByTemplate", 2),
            ];
            let mut spawn_join = here();
            spawn_join.extend([
                int(3),
                Call::Routine("SpawnAvailableNPC", 2),
                int(3),
                Call::Routine("AddPartyMember", 2),
            ]);
            third.extend(keep("kjk_spawned", spawn_join));
            let mut create = vec![int(0)];
            create.extend(here());
            create.extend([
                text(&other),
                int(creature_type),
                Call::Routine("CreateObject", 4),
                Call::Routine("GetIsObjectValid", 1),
            ]);
            third.extend(keep("kjk_created", create));
            let screen = vec![
                text(&companion),
                int(4),
                Call::Routine("AddAvailableNPCByTemplate", 2),
                int(-1),
                int(4),
                text("kjk_party5"),
                Call::Routine("ShowPartySelectionGUI", 3),
            ];
            let exit = keep("kjk_exit", vec![Call::Routine("GetPartyMemberCount", 0)]);
            templates = vec![companion, other];
            // Three creatures with sheets, by tag and which of that tag.
            let mut seen = std::collections::BTreeMap::new();
            for object in data.objects() {
                if object.kind == sim::module::ObjectKind::Creature && object.sheet.is_some() {
                    let nth = seen.entry(object.tag.clone()).or_insert(0);
                    if chosen.len() < 3 {
                        chosen.push((object.tag.clone(), *nth));
                    }
                    *nth += 1;
                }
            }
            if chosen.len() < 3 {
                return Err("fewer than three creatures".to_string());
            }
            let mut first = Vec::new();
            for (npc, (tag, nth)) in chosen.iter().enumerate() {
                let npc = npc as i32;
                first.push(with_creature(
                    &format!("kjk_avail{npc}"),
                    "AddAvailableNPCByObject",
                    npc,
                    tag,
                    *nth,
                ));
            }
            for (npc, (tag, nth)) in chosen.iter().enumerate() {
                let npc = npc as i32;
                first.push(with_creature(
                    &format!("kjk_join{npc}"),
                    "AddPartyMember",
                    npc,
                    tag,
                    *nth,
                ));
            }
            let (tag, nth) = &chosen[0];
            first.push(with_creature("kjk_join5", "AddPartyMember", 5, tag, *nth));
            first.push(keep(
                "kjk_count",
                vec![Call::Routine("GetPartyMemberCount", 0)],
            ));
            let second = vec![
                keep(
                    "kjk_left",
                    vec![int(0), Call::Routine("RemovePartyMember", 1)],
                ),
                keep(
                    "kjk_still",
                    vec![int(0), Call::Routine("IsNPCPartyMember", 1)],
                ),
            ];
            let mut scripts = std::collections::BTreeMap::new();
            scripts.insert(
                "kjk_party1".to_string(),
                crate::probe::program(&routines, &first)?,
            );
            scripts.insert(
                "kjk_party2".to_string(),
                crate::probe::program(&routines, &second)?,
            );
            scripts.insert(
                "kjk_party3".to_string(),
                crate::probe::program(&routines, &[third])?,
            );
            scripts.insert(
                "kjk_party4".to_string(),
                crate::probe::program(&routines, &[screen])?,
            );
            scripts.insert(
                "kjk_party5".to_string(),
                crate::probe::program(&routines, &[exit])?,
            );
            // Party member 0's tag.
            let leading = vec![vec![
                int(0),
                Call::Routine("GetPartyMemberByIndex", 1),
                Call::Routine("GetTag", 1),
                text("kjk_first"),
                Call::Routine("SetGlobalString", 2),
            ]];
            scripts.insert(
                "kjk_party6".to_string(),
                crate::probe::program(&routines, &leading)?,
            );
            Ok(
                sim::script::Library::new(routines, scripts, sim::script::Talents::default())
                    .with_items(own.items().clone())
                    .with_creatures(creatures),
            )
        })?;
        let numbers = |probe: &Probe, name: &str| {
            sim::script::global(&probe.world.script_state().numbers, name)
                .copied()
                .unwrap_or(-1)
        };
        // Each chosen creature's id: its tag's nth creature.
        let snapshot = probe.world.snapshot();
        let creatures: Vec<sim::SimId> = chosen
            .iter()
            .map(|(tag, nth)| {
                crate::scenes::tagged(probe.world.level(), &snapshot, tag)
                    .get(*nth as usize)
                    .map(|&at| snapshot.entities[at].id)
                    .ok_or("a chosen creature")
            })
            .collect::<Result<_, _>>()?;
        let tags: Vec<&str> = chosen.iter().map(|(tag, _)| tag.as_str()).collect();

        // 0. The goods tell what every creature here wears on its body (its
        // body is dressed by it, `made::guise`): its template's armour, as
        // placed.
        let start = probe.step(1);
        let objects = sim::Instances::in_snapshot(probe.world.level(), &start);
        let body = Some(sim::inventory::slots::BODY);
        let armoured: Vec<(u32, String, Option<&str>)> = start
            .entities
            .iter()
            .filter_map(|entity| {
                let facts = objects.of_entity(entity)?;
                let held = facts.inventory.iter().find(|held| held.slot == body)?;
                Some((
                    entity.id.0,
                    held.template.clone(),
                    start.worn(entity.id.0, sim::inventory::slots::BODY),
                ))
            })
            .collect();
        report.check(
            !armoured.is_empty()
                && armoured.iter().all(|(_, template, worn)| {
                    worn.is_some_and(|worn| worn.eq_ignore_ascii_case(template))
                }),
            format!(
                "the goods tell the body armour of the {} creatures placed wearing some: {armoured:?}",
                armoured.len()
            ),
        );

        probe.queue("kjk_party1", probe.me);
        let after = probe.step(2);
        let fighter = |snapshot: &sim::Snapshot, id: sim::SimId| {
            snapshot.get(id).and_then(|e| e.fighter).ok_or("a fighter")
        };
        let joined: Vec<i32> = (0..3)
            .map(|npc| numbers(&probe, &format!("kjk_join{npc}")))
            .collect();
        let available: Vec<i32> = (0..3)
            .map(|npc| numbers(&probe, &format!("kjk_avail{npc}")))
            .collect();
        report.check(
            available == [1, 1, 1] && joined == [1, 1, 0] && numbers(&probe, "kjk_join5") == 0,
            format!(
                "{tags:?} made available {available:?}; joining {joined:?} (the party is three); an unavailable one refused"
            ),
        );
        let first = fighter(&after, creatures[0])?;
        let third = fighter(&after, creatures[2])?;
        report.check(
            first.mind.leader == Some(sim::Seat(0))
                && first.faction == Some(sim::combat::sheet::factions::PLAYER)
                && third.mind.leader.is_none(),
            format!(
                "{} follows the first seat in the player's faction; {} does not",
                tags[0], tags[2]
            ),
        );
        report.check(
            numbers(&probe, "kjk_count") == 3,
            format!("GetPartyMemberCount: {}", numbers(&probe, "kjk_count")),
        );

        probe.queue("kjk_party2", probe.me);
        let after = probe.step(2);
        let first = fighter(&after, creatures[0])?;
        report.check(
            numbers(&probe, "kjk_left") == 1
                && numbers(&probe, "kjk_still") == 0
                && first.mind.leader.is_none()
                && first.faction.is_none(),
            format!("{} leaves and fights for itself again", tags[0]),
        );
        let state = probe.world.script_state().clone();
        let save = sim::Save {
            hz: sim::TICK_HZ,
            level: probe.world.level().id(),
            snapshot: probe.world.snapshot(),
            script: state.clone(),
        };
        let loaded = sim::Save::decode(&save.encode()).map_err(|e| e.to_string())?;
        report.check(
            loaded.script.party == state.party,
            format!(
                "a save carries the table ({} available, members {:?})",
                state.party.available.len(),
                state.party.members
            ),
        );

        // 3. A companion made from its template joins as it is made; a
        // creature made by `CreateObject`.
        let before = probe.world.snapshot();
        probe.queue("kjk_party3", probe.me);
        let after = probe.step(2);
        let made: Vec<&sim::EntityState> = after
            .entities
            .iter()
            .filter(|e| matches!(e.kind, sim::Kind::Object(..)) && before.get(e.id).is_none())
            .collect();
        let spawned: Vec<&str> = after.spawned.iter().map(|s| s.template.as_str()).collect();
        let companion = made.first().map(|e| e.id).ok_or("nothing made")?;
        report.check(
            numbers(&probe, "kjk_spawned") == 1
                && numbers(&probe, "kjk_created") == 1
                && spawned == [templates[0].as_str(), templates[1].as_str()]
                && made.len() == 2
                && made[0].fighter.is_some_and(|f| f.mind.leader == Some(sim::Seat(0)))
                && made[1].fighter.is_some_and(|f| f.mind.leader.is_none())
                && made.iter().all(|e| e.sheet.is_some()),
            format!(
                "{} spawned from its template joins as it is made; CreateObject makes {}: the module's spawned {spawned:?} (results {} {}; leaders {:?}; sheets {})",
                templates[0],
                templates[1],
                numbers(&probe, "kjk_spawned"),
                numbers(&probe, "kjk_created"),
                made.iter().map(|e| e.fighter.map(|f| f.mind.leader)).collect::<Vec<_>>(),
                made.iter().all(|e| e.sheet.is_some())
            ),
        );

        // 4. The selection screen: NPC 4 forced, NPC 1 picked; NPC 3 goes.
        probe.queue("kjk_party4", probe.me);
        let (_, told) = probe.step_told(2);
        let shown = probe.world.script_state().party.selection.clone();
        let members = probe.world.script_state().party.members.clone();
        let roster = rosters(&told).into_iter().next();
        let library = probe.world.level().scripts().ok_or("no scripts")?;
        let armour = library
            .creature(&templates[0])
            .and_then(|data| {
                data.inventory
                    .iter()
                    .find(|held| held.slot == Some(sim::inventory::slots::BODY))
            })
            .map(|held| held.template.as_str());
        report.check(
            roster.as_ref().is_some_and(|roster| {
                roster.open
                    && roster.template(4) == Some(templates[0].as_str())
                    && roster.forced == Roster::bits([4])
                    && roster.members == Roster::bits(members.iter().copied())
                    && roster.worn(3) == Some(armour)
                    && told_again(roster)
            }),
            format!(
                "the selection screen told with the table, NPC 3 in its template's {armour:?}: {roster:?}, members {members:?}"
            ),
        );
        let mut input = sim::TickInput::default();
        input.act(
            sim::Seat(0),
            sim::action::Action::ChooseParty(Some([1, u8::MAX])),
        );
        probe.step_with(input);
        let after = probe.step(2);
        let party = probe.world.script_state().party.clone();
        let made_now = party.available.get(&4).and_then(|npc| npc.object);
        let fourth = made_now.and_then(|id| after.get(sim::SimId(id)));
        report.check(
            shown.is_some_and(|s| s.exit == "kjk_party5" && s.forced == [4])
                && party.selection.is_none()
                && party.members == [1, 4]
                && after.get(companion).is_none()
                && fourth.is_some_and(|e| {
                    e.fighter
                        .is_some_and(|f| f.mind.leader == Some(sim::Seat(0)))
                })
                && numbers(&probe, "kjk_exit") == 3,
            format!(
                "the selection screen (forcing NPC 4) closed with NPC 1 picked: members {:?}, NPC 3 gone {}, NPC 4 made and following {}, its script counts {}",
                party.members,
                after.get(companion).is_none(),
                fourth.is_some(),
                numbers(&probe, "kjk_exit")
            ),
        );
        let companion = sim::SimId(made_now.ok_or("NPC 4 made")?);

        // 4b. Shown again and closed unchanged (the screen opens with the
        // party picked): NPC 1, not forced, stays.
        probe.queue("kjk_party4", probe.me);
        probe.step(2);
        let mut input = sim::TickInput::default();
        input.act(sim::Seat(0), sim::action::Action::ChooseParty(None));
        probe.step_with(input);
        let after = probe.step(2);
        let party = probe.world.script_state().party.clone();
        report.check(
            party.selection.is_none()
                && party.members == [1, 4]
                && after.get(companion).is_some()
                && numbers(&probe, "kjk_exit") == 3,
            format!(
                "the selection screen closed unchanged keeps the party: members {:?}, its script counts {}",
                party.members,
                numbers(&probe, "kjk_exit")
            ),
        );

        // 4c. The party screen opened in play: the table, to look at.
        let (_, told) = probe.act(sim::action::Action::ShowParty);
        let roster = rosters(&told).into_iter().next();
        report.check(
            roster.as_ref().is_some_and(|roster| {
                !roster.open && roster.members == Roster::bits([1, 4]) && told_again(roster)
            }) && sim::action::Action::from_bytes(&sim::action::Action::ShowParty.to_bytes())
                == Ok(sim::action::Action::ShowParty),
            format!("the party screen opened in play is told the table: {roster:?}"),
        );

        // 5. NPC 4 made leader.
        let mut input = sim::TickInput::default();
        input.act(sim::Seat(0), sim::action::Action::SetLeader(4));
        probe.step_with(input);
        probe.queue("kjk_party6", probe.me);
        probe.step(2);
        let strings = &probe.world.script_state().strings;
        let tag = |name: &str| sim::script::global(strings, name).cloned().unwrap_or_else(String::new);
        let fourth_tag = probe
            .world
            .level()
            .scripts()
            .and_then(|library| library.creature(&templates[0]))
            .map(|data| data.tag.clone())
            .ok_or("NPC 4's template")?;
        report.check(
            probe.world.script_state().party.leader == Some(4)
                && tag("kjk_first").eq_ignore_ascii_case(&fourth_tag),
            format!(
                "NPC 4 made leader: party member 0 is {:?} (its tag {fourth_tag:?})",
                tag("kjk_first")
            ),
        );

        // 6. The party follows into the next module: the companion made
        // here and the one from the module's own creatures, same ids,
        // following; the module left keeps neither.
        let joined = creatures[1];
        let next = probe.install("end_m01ab")?;
        let objects = sim::Instances::in_snapshot(probe.world.level(), &next);
        let followed = [companion, joined].iter().all(|id| {
            next.get(*id).is_some_and(|e| {
                matches!(e.kind, sim::Kind::Object(_, index) if objects.made_from(index).is_some())
                    && e.fighter
                        .is_some_and(|f| f.mind.leader == Some(sim::Seat(0)))
            })
        });
        let left_behind = next
            .stash
            .iter()
            .flat_map(|stashed| &stashed.entities)
            .any(|e| e.id == companion || e.id == joined);
        report.check(
            followed && !left_behind,
            format!(
                "both companions follow into end_m01ab as its spawned {:?}; the Endar Spire's stash keeps neither",
                next.spawned.iter().map(|s| s.template.as_str()).collect::<Vec<_>>()
            ),
        );
        let faults = probe.faults();
        report.check(
            faults.is_empty(),
            format!("the scripts ran without faults ({faults:?})"),
        );

        // 7. A companion put in the party by hand (`--follow`) takes the
        // number `npc.2da` gives its tag: Carth (2) stays through the
        // Endar Spire's next module's own scripts, which take Trask's 0
        // away (`k_pend_modab`).
        if module == "end_m01aa" {
            let carth = "carth";
            let empty = sim::script::Library::new(
                catalog(&base)?,
                Default::default(),
                sim::script::Talents::default(),
            );
            let mut by_hand =
                Probe::new(discovery, game, module, |_| Ok(empty))?.follow(&[carth.to_string()])?;
            let numbered = by_hand.world.script_state().party.members.clone();
            by_hand.install("end_m01ab")?;
            // Its scripts run in the steps after the install.
            let next = by_hand.step(25);
            let party = &by_hand.world.script_state().party;
            let objects = sim::Instances::in_snapshot(by_hand.world.level(), &next);
            let stays = party.creatures().any(|id| {
                next.get(sim::SimId(id)).is_some_and(|e| {
                    objects
                        .of_entity(e)
                        .is_some_and(|o| o.tag.eq_ignore_ascii_case(carth))
                        && e.fighter
                            .is_some_and(|f| f.mind.leader == Some(sim::Seat(0)))
                })
            });
            report.check(
                numbered == [2] && stays,
                format!(
                    "Carth by hand is NPC {numbered:?} and in end_m01ab's party after its scripts ran: {stays} (members {:?})",
                    party.members
                ),
            );
        }
        Ok(json!({ "module": module, "creatures": tags }))
    };
    match run(&mut report) {
        Ok(facts) => report.finish(facts),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
