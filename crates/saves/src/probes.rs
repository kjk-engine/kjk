//! The crate's probes (`kjk sim probes`).

use super::*;
use sim::probe::check;

/// A refusal case: its name, the refusal expected, the change made.
type Case<'a> = (
    &'static str,
    Option<&'a str>,
    &'a dyn Fn(&mut sim::Snapshot),
);

/// The proving ground with Carth standing by the entry: a creature named
/// as a guest's companion is.
fn with_carth() -> Result<sim::Level, String> {
    let ground = sim::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    let carth = sim::module::ObjectData {
        name: "Carth".into(),
        ..sim::module::ObjectData::unknown("carth", data.entry.position, 0.0)
    };
    data.placements.push(sim::module::Placement::Own(carth));
    Ok(ground.with_module(data))
}

/// `refusal` on a quiet moment and on each thing that refuses a save, made
/// on the proving ground with Carth (`with_carth`; `kjk sim probes`, group
/// "save refusals").
pub fn refusal_probes() -> Vec<(&'static str, Result<String, String>)> {
    let made = with_carth().and_then(|level| {
        let mut world = sim::SimWorld::new(sim::WorldConfig::one_seat(), level.clone())
            .map_err(|why| why.to_string())?;
        Ok((world.snapshot(), level))
    });
    let (quiet, level) = match made {
        Ok(made) => made,
        Err(why) => return vec![("refusal_world", Err(why))],
    };
    let seat = sim::Seat(0);
    let with = |change: &dyn Fn(&mut sim::Snapshot)| {
        let mut snapshot = quiet.clone();
        change(&mut snapshot);
        refusal(&snapshot, seat, &level)
    };
    // A match `player` plays (`sim::pazaak::Match::player`).
    let table = |player: u32| {
        let deck = [sim::pazaak::Card::of(7).expect("+1"); sim::pazaak::SIDE_DECK];
        let new = sim::pazaak::Match::new(deck, Vec::new(), (0, 0), false, (String::new(), 0));
        sim::pazaak::Match { player, ..new }
    };
    // A player's own conversation `listener` stands in.
    let talk = |listener: u32| {
        let mut own = sim::script::dialogue_probes::conversation();
        own.listener.0 = listener;
        own.joined.clear();
        own.party = false;
        own
    };
    let own = quiet.body(seat).map_or(0, |body| body.id.0);
    let objects = sim::Instances::in_snapshot(&level, &quiet);
    let carth = quiet
        .entities
        .iter()
        .find(|entity| {
            objects
                .of_entity(entity)
                .is_some_and(|facts| facts.name == "Carth")
        })
        .map_or(0, |entity| entity.id.0);
    // Carth driven by a second seat, as a guest's companion is.
    let guest = |s: &mut sim::Snapshot, carth: u32| {
        s.seats_held = 2;
        let fighter = s
            .entities
            .iter_mut()
            .find(|e| e.id.0 == carth)
            .and_then(|e| e.fighter.as_mut());
        if let Some(fighter) = fighter {
            fighter.mind.driver = Some(sim::Seat(1));
        }
    };
    let cases: [Case<'_>; 14] = [
        ("refusal_quiet", None, &|_| {}),
        (
            "refusal_in_combat",
            Some("You cannot save during combat."),
            &|s| s.in_combat = true,
        ),
        ("refusal_combat_timer_alone", None, &|s| {
            let now = s.time_ms;
            for fighter in s.entities.iter_mut().filter_map(|e| e.fighter.as_mut()) {
                fighter.combat_until = now + 1000;
            }
        }),
        (
            "refusal_break",
            Some("You cannot save during a break."),
            &|s| s.paused_by = Some(sim::Seat(1)),
        ),
        (
            "refusal_ready_check",
            Some("You cannot save while the party waits at an exit."),
            &|s| {
                s.ready = Some(sim::ReadyCheck {
                    module: "probe_b".into(),
                    tag: "exit".into(),
                    via: sim::SimId(0),
                    asked_by: sim::Seat(0),
                    ready: 1,
                    since_ms: s.time_ms,
                })
            },
        ),
        (
            "refusal_minigame",
            Some("You cannot save during a minigame."),
            &|s| {
                let rules = sim::minigame::Rules::bare(sim::minigame::Kind::Turret);
                s.minigame = Some(sim::minigame::Course::new(&rules));
            },
        ),
        (
            "refusal_pazaak",
            Some("You cannot save during a minigame."),
            &|s| s.pazaak = vec![table(own)],
        ),
        // Another player's match or conversation (R11): anyone's refuses,
        // in its body's name; one with no name as the saving seat's own.
        (
            "refusal_pazaak_other",
            Some("Carth is playing pazaak."),
            &|s| s.pazaak = vec![table(carth)],
        ),
        (
            "refusal_pazaak_unnamed",
            Some("You cannot save during a minigame."),
            &|s| s.pazaak = vec![table(own + 1000)],
        ),
        (
            "refusal_talk_own",
            Some("You cannot save during a conversation."),
            &|s| s.talk.conversations = vec![talk(own)],
        ),
        (
            "save_refusal_names",
            Some("Carth is in a conversation."),
            &|s| {
                guest(s, carth);
                s.talk.conversations = vec![talk(carth)];
            },
        ),
        // Two others talking, Carth no seat's: in KOTOR's words.
        (
            "refusal_talk_npcs",
            Some("You cannot save during a conversation."),
            &|s| s.talk.conversations = vec![talk(carth)],
        ),
        (
            "refusal_talk_party",
            Some("You cannot save during a conversation."),
            &|s| {
                s.talk.conversations = vec![sim::script::dialogue::Conversation {
                    party: true,
                    ..talk(carth)
                }]
            },
        ),
        (
            "refusal_down",
            Some("You cannot save while you are down."),
            &|s| {
                if let Some(fighter) = s
                    .entities
                    .iter_mut()
                    .find(|e| e.kind == sim::Kind::Avatar(sim::Seat(0)))
                    .and_then(|e| e.fighter.as_mut())
                {
                    fighter.life = sim::combat::fighter::Life::Dead;
                }
            },
        ),
    ];
    cases
        .into_iter()
        .map(|(name, expect, change)| {
            let got = with(change);
            let detail = format!("{got:?}");
            (
                name,
                if got.as_deref() == expect {
                    Ok(detail)
                } else {
                    Err(detail)
                },
            )
        })
        .collect()
}

/// The story record (`story`, group "save story"):
/// - the kitchen sink's story has its party, journal, globals, galaxy and
///   the modules it visited by name;
/// - a module with a roster keeps its destroyed objects, moved doors and
///   locals by origin key, one without keeps its spawns alone;
/// - the JSON round trip is equal, and a JSON without most fields reads;
/// - a slot write puts `story.json` in the folder and `story` in the
///   meta's formats; a slot without one, or with one that does not read,
///   has no story, and an older meta's formats read;
/// - a story restored and written again is the story (spec 4c), but for
///   its build, its time and the place within the 2 m snap, and for what
///   each case names as lost, each with why.
pub fn story_probes() -> sim::probe::Checks {
    let (level, save) = sim::fixtures::kitchen_sink();
    let story = Story::of(&save, &level);
    let root = std::env::temp_dir().join(format!("kjk-save-story-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let checks = vec![
        ("story_of_sink", of_sink(&story)),
        ("story_roster_module", roster_module(&level, &save)),
        ("story_companions_items", companions_items(&level, &save)),
        ("story_json_round_trip", json_round_trip(&story)),
        ("story_reads_few_fields", reads_few_fields()),
        ("story_written_with_slot", slot_write(&root, &story)),
        ("story_missing_is_none", missing_is_none(&root)),
        ("formats_without_story_read", old_formats()),
        ("story_restores_sink", restores_sink(&level, &save)),
        ("story_restores_roster_module", restores_roster_module(&level, &save)),
        ("story_restores_left_and_here", restores_left_and_here(&save)),
    ];
    let _ = std::fs::remove_dir_all(&root);
    checks
}

fn of_sink(story: &Story) -> Result<String, String> {
    // Each name the sink's save holds, and what the story has of it.
    let named = [
        ("schema", story.schema == STORY_SCHEMA),
        (
            "number",
            story.globals.numbers.get("K_FIXTURE_NUMBER") == Some(&7),
        ),
        (
            "boolean",
            story.globals.booleans.get("K_FIXTURE_BOOLEAN") == Some(&true),
        ),
        (
            "string",
            story
                .globals
                .strings
                .get("K_FIXTURE_STRING")
                .map(String::as_str)
                == Some("kept"),
        ),
        (
            "place",
            story.globals.places.get("K_FIXTURE_PLACE") == Some(&([1.0, 2.0, 0.0], 90.0)),
        ),
        (
            "journal",
            story.journal == [("k_fixture_quest".to_string(), 20)],
        ),
        (
            "companion",
            story
                .party
                .iter()
                .any(|c| c.npc == Some(0) && c.template == "p_fixture"),
        ),
        ("player", story.party.iter().any(|c| c.seat == Some(0))),
        ("gold", story.gold == 250),
        ("components", (story.components, story.chemicals) == (4, 6)),
        (
            "galaxy",
            story.galaxy.available == [1, 2] && story.galaxy.selected == Some(1),
        ),
        (
            "unescapable",
            story.unescapable == [sim::determinism::GROUND],
        ),
        (
            "transit",
            story.transit.get(sim::determinism::GROUND) == Some(&true),
        ),
        (
            "left module",
            story
                .visited
                .get("proving_ground_left")
                .is_some_and(|m| m.spawns_ran),
        ),
        ("leader module", !story.place.module.is_empty()),
    ];
    let missing: Vec<&str> = named
        .iter()
        .filter(|(_, ok)| !ok)
        .map(|(name, _)| *name)
        .collect();
    check(missing.is_empty(), format!("missing: {missing:?}"))
}

/// The sink with a roster of a door, a placeable and a creature, and
/// objects of them in play: the door open and locked, the placeable
/// destroyed, the creature gone, a local on the door.
fn roster_save(save: &sim::Save) -> Result<sim::Save, String> {
    use sim::module::ObjectKind;
    let mut save = save.clone();
    let Some(proto) = save.snapshot.entities.first().copied() else {
        return Err("the sink has no entity".into());
    };
    save.snapshot.roster = Some(vec![
        origin(ObjectKind::Door, "door_a"),
        origin(ObjectKind::Placeable, "plc_b"),
        origin(ObjectKind::Creature, "cre_c"),
        origin(ObjectKind::Creature, "cre_d"),
    ]);
    let made = |id, kind, number, state| sim::EntityState {
        id: sim::SimId(id),
        kind: sim::Kind::Object(kind, number),
        object: Some(state),
        ..proto
    };
    // A body lying dead (cre_d) is destroyed too.
    let mut dead = made(9_003, ObjectKind::Creature, 3, sim::ObjectState::default());
    let mut fighter = sim::combat::fighter::Fighter::fresh(10, 0);
    fighter.life = sim::combat::fighter::Life::Dead;
    dead.fighter = Some(fighter);
    let open_locked = sim::ObjectState {
        state: 1,
        lock: sim::lock::LOCKED,
        ..sim::ObjectState::default()
    };
    save.snapshot.entities.extend([
        made(9_001, ObjectKind::Door, 0, open_locked),
        made(9_002, ObjectKind::Placeable, 1, sim::ObjectState::GONE),
        dead,
    ]);
    save.script.locals.insert((9_001, 1, 5), 7);
    Ok(save)
}

/// A hand-made roster's placement of `kind` and `template`, its tag the
/// template.
fn origin(kind: sim::module::ObjectKind, template: &str) -> sim::reconcile::Origin {
    sim::reconcile::Origin {
        kind,
        template: template.into(),
        tag: template.into(),
    }
}

fn roster_module(level: &sim::Level, save: &sim::Save) -> Result<String, String> {
    use sim::module::ObjectKind;
    let save = roster_save(save)?;
    let module = save
        .snapshot
        .place
        .as_ref()
        .map(|place| place.module.to_ascii_lowercase());
    let story = Story::of(&save, level);
    let key = |index, kind, template| story::OriginKey::of(index, &origin(kind, template));
    let door = key(0, ObjectKind::Door, "door_a");
    let kept = module.and_then(|module| story.visited.get(&module));
    let ok = kept.is_some_and(|kept| {
        kept.doors == [(door.clone(), 1, sim::lock::LOCKED)]
            && kept.destroyed
                == [
                    key(1, ObjectKind::Placeable, "plc_b"),
                    key(2, ObjectKind::Creature, "cre_c"),
                    key(3, ObjectKind::Creature, "cre_d"),
                ]
            && kept.locals == [(door.clone(), 1, 5, 7)]
    });
    check(ok, format!("{kept:?}"))
}

fn json_round_trip(story: &Story) -> Result<String, String> {
    let text = serde_json::to_string_pretty(story).map_err(|error| error.to_string())?;
    let back: Story = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    check(back == *story, format!("{} bytes", text.len()))
}

fn reads_few_fields() -> Result<String, String> {
    let text = r#"{"schema":9,"future":{"a":1},"globals":{"numbers":{"N":2}},"party":[{"npc":3}]}"#;
    let story: Story = serde_json::from_str(text).map_err(|error| error.to_string())?;
    check(
        story.schema == 9
            && story.globals.numbers.get("N") == Some(&2)
            && story.party.len() == 1
            && story.visited.is_empty(),
        format!("{story:?}"),
    )
}

fn slot_write(root: &Path, story: &Story) -> Result<String, String> {
    let meta = Meta {
        build: Some("ab12cd34ef".into()),
        saved: 77,
        formats: Some(Formats {
            story: STORY_SCHEMA,
            ..Formats::default()
        }),
        ..Meta::default()
    };
    let id = write_encoded(root, Some("0001"), b"world", &meta, Some(story))?;
    let read = read_story(root, &id);
    let formats = read_meta(&root.join(&id))?
        .formats
        .map(|formats| formats.story);
    let stamped = Story {
        build: "ab12cd34ef".into(),
        saved: 77,
        ..story.clone()
    };
    check(
        root.join(&id).join(STORY).exists()
            && read.as_ref() == Some(&stamped)
            && formats == Some(STORY_SCHEMA),
        format!("{:?}", read.map(|story| (story.build, story.saved))),
    )
}

fn missing_is_none(root: &Path) -> Result<String, String> {
    let meta = Meta::default();
    write_encoded(root, Some("0002"), b"world", &meta, None)?;
    write_encoded(root, Some("0003"), b"world", &meta, Some(&Story::default()))?;
    std::fs::write(root.join("0003").join(STORY), "not json").map_err(|error| error.to_string())?;
    let found = ["0002", "0003", "0004", "../0001"].map(|id| read_story(root, id).is_some());
    check(found == [false; 4], format!("{found:?}"))
}

fn old_formats() -> Result<String, String> {
    let text = r#"{"name":"old","module":"m","area":"a","saved":1,"seats":1,"formats":{"save":59,"snapshot":72}}"#;
    let (meta, _) = parse_meta(text)?;
    check(
        meta.formats
            == Some(Formats {
                save: 59,
                snapshot: 72,
                story: 0,
            }),
        format!("{:?}", meta.formats),
    )
}

/// The sink with a player's equipped and carried items, an away companion
/// (`Kept`, no creature) holding gear, a career and XP under its old
/// creature's id, and a companion leading: each is in the story by name.
fn companions_items(level: &sim::Level, save: &sim::Save) -> Result<String, String> {
    use sim::inventory::Item;
    use sim::script::party::Kept;
    let mut save = save.clone();
    let Some(player) = save
        .snapshot
        .entities
        .iter()
        .find(|entity| matches!(entity.kind, sim::Kind::Avatar(_)))
        .map(|entity| entity.id.0)
    else {
        return Err("the sink has no player".into());
    };
    let item = |template: &str, holder, slot| Item {
        template: template.into(),
        holder,
        slot,
        stack: 1,
        charges: 0,
        droppable: true,
        plot: false,
        identified: true,
        infinite: false,
        non_equippable: false,
        upgrades: 0,
        fitted: Vec::new(),
    };
    let away = 777;
    let mut sheet = sim::combat::sheet::Sheet::commoner();
    sheet.feats.insert(5);
    for (id, item) in [
        (9_101, item("w_blade", player, Some(4))),
        (9_102, item("g_i_medpac", player, None)),
        (9_103, item("w_pistol", away, Some(4))),
        (9_104, item("g_i_belt", away, None)),
    ] {
        save.script.items.insert(id, item);
    }
    save.script.careers.insert(away, vec![(0, 3)]);
    save.script.xp.insert(away, 450);
    let companion = save.script.party.available.get_mut(&0);
    let Some(companion) = companion else {
        return Err("the sink has no companion".into());
    };
    companion.kept = Some(Kept {
        from: away,
        sheet,
        hp: 100,
        fp: 50,
        worn: None,
    });
    save.script.party.leader = Some(0);
    let story = Story::of(&save, level);
    let held = |who: &str| -> Vec<(&str, Option<u8>)> {
        story.items.get(who).map_or_else(Vec::new, |items| {
            items
                .iter()
                .map(|i| (i.template.as_str(), i.slot))
                .collect()
        })
    };
    let companion = story.party.iter().find(|c| c.npc == Some(0));
    let named = [
        (
            "player's items",
            held("player0") == [("w_blade", Some(4)), ("g_i_medpac", None)],
        ),
        (
            "away companion's items",
            held("npc0") == [("w_pistol", Some(4)), ("g_i_belt", None)],
        ),
        (
            "away companion's career, xp and feats",
            companion.is_some_and(|c| c.classes == [(0, 3)] && c.xp == 450 && c.feats == [5]),
        ),
        ("leader", story.leader == "npc0"),
    ];
    let missing: Vec<&str> = named
        .iter()
        .filter(|(_, ok)| !ok)
        .map(|(name, _)| *name)
        .collect();
    check(missing.is_empty(), format!("missing: {missing:?}"))
}

/// What a round trip leaves out of the comparison: what it is, why it
/// cannot come back, and the change that blanks it in both stories.
type Exclusion = (&'static str, &'static str, fn(&mut Story));

/// `story` restored in `level` for one seat and written again
/// (`Story::of`): equal to it but for `build`, `saved`, the place within
/// the 2 m snap and `excluded`, and what the restore said it lost holds
/// each of `lost` (a line's words).
fn round_trip(
    story: &Story,
    level: &sim::Level,
    excluded: &[Exclusion],
    lost: &[&str],
) -> Result<String, String> {
    let (mut world, said) = story.restore(level, 1)?;
    let again = sim::Save {
        hz: sim::TICK_HZ,
        level: level.id(),
        snapshot: world.snapshot(),
        script: world.script_state().clone(),
    };
    let (mut before, mut after) = (story.clone(), Story::of(&again, level));
    for (_, _, blank) in excluded {
        blank(&mut before);
        blank(&mut after);
    }
    let (differ, off) = before.compare(&after);
    let unsaid: Vec<&&str> = lost
        .iter()
        .filter(|words| !said.iter().any(|line| line.contains(**words)))
        .collect();
    let named: Vec<&str> = excluded.iter().map(|(what, _, _)| *what).collect();
    check(
        differ.is_empty() && off <= story::PLACE_SLACK_M && unsaid.is_empty(),
        format!(
            "differ {differ:?}, place {off:.2} m off, lost {said:?}, unsaid {unsaid:?}, excluded {named:?}"
        ),
    )
}

fn restores_sink(level: &sim::Level, save: &sim::Save) -> Result<String, String> {
    round_trip(&Story::of(save, level), level, &[], &[])
}

fn restores_roster_module(level: &sim::Level, save: &sim::Save) -> Result<String, String> {
    let story = Story::of(&roster_save(save)?, level);
    let excluded: [Exclusion; 1] = [(
        "the module here's facts",
        "its roster is made by hand: none of its placements are the proving ground's, so the restore finds none of them",
        |story| {
            let here = story.place.module.to_ascii_lowercase();
            if let Some(kept) = story.visited.get_mut(&here) {
                (kept.destroyed, kept.doors, kept.locals) = Default::default();
            }
        },
    )];
    round_trip(&story, level, &excluded, &["no such placement here"])
}

/// The key of the first placement of `kind` in `level`.
fn first_of(level: &sim::Level, kind: sim::module::ObjectKind) -> Option<story::OriginKey> {
    let placed = level.placed();
    let index = placed.iter().position(|facts| facts.kind == kind)?;
    let facts = &placed[index];
    let origin = sim::reconcile::Origin {
        kind,
        template: facts.template.clone(),
        tag: facts.tag.clone(),
    };
    Some(story::OriginKey::of(index as u32, &origin))
}

/// Here the proving ground with Carth: the gate open and locked with a
/// local, Carth destroyed. Left, the proving ground it was entered from:
/// its gate open and locked with a local, its way out destroyed. Restored
/// and written again, the story is itself; the module left entered again,
/// its gate stands at its placement open and locked with its local, its
/// way out gone.
fn restores_left_and_here(save: &sim::Save) -> Result<String, String> {
    use sim::module::ObjectKind;
    const LEFT: &str = "proving_ground_left";
    let (level, left) = (with_carth()?, sim::fixtures::left());
    let mut story = Story::of(save, &level);
    let here = story.place.module.to_ascii_lowercase();
    let (Some(gate), Some(carth), Some(left_gate), Some(way_out)) = (
        first_of(&level, ObjectKind::Door),
        first_of(&level, ObjectKind::Creature),
        first_of(&left, ObjectKind::Door),
        first_of(&left, ObjectKind::Trigger),
    ) else {
        return Err("the grounds lack a gate, Carth or a way out".into());
    };
    let open_locked = |key: &story::OriginKey| story::StoryModule {
        doors: vec![(key.clone(), 1, sim::lock::LOCKED)],
        locals: vec![(key.clone(), 1, 5, 7)],
        ..story::StoryModule::default()
    };
    // The proving ground has no scripts, so no spawns ran here; a module
    // left with its objects destroyed had its spawns run.
    story.visited.insert(
        here,
        story::StoryModule {
            destroyed: vec![carth],
            ..open_locked(&gate)
        },
    );
    story.visited.insert(
        LEFT.into(),
        story::StoryModule {
            spawns_ran: true,
            destroyed: vec![way_out.clone()],
            ..open_locked(&left_gate)
        },
    );
    let trip = round_trip(&story, &level, &[], &[])?;
    let (mut world, _) = story.restore(&level, 1)?;
    world
        .install(left.clone(), None)
        .map_err(|error| format!("entering the module left: {error}"))?;
    let snapshot = world.snapshot();
    let object = |key: &story::OriginKey| {
        snapshot
            .entities
            .iter()
            .find(|entity| matches!(entity.kind, sim::Kind::Object(_, number) if number == key.index))
    };
    let gate = object(&left_gate);
    let placed_at = left.placed()[left_gate.index as usize].position;
    let local = gate.and_then(|gate| world.script_state().locals.get(&(gate.id.0, 1, 5)).copied());
    let gate = gate.map(|gate| {
        let state = gate.object.unwrap_or(sim::ObjectState::GONE);
        (state.state, state.lock, gate.position == placed_at)
    });
    let gone = object(&way_out).is_none_or(|way_out| way_out.object == Some(sim::ObjectState::GONE));
    check(
        gate == Some((1, sim::lock::LOCKED, true)) && local == Some(7) && gone,
        format!("{trip}; entered again: gate (state, lock, at its placement) {gate:?}, local {local:?}, way out gone {gone}"),
    )
}
