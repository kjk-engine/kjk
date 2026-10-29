//! The format fixtures: a save built here, from no game's data, with as
//! much of what a save holds as a code-built world can give, written to
//! `DIR` at every pin that moves a stored format (`save`, `snapshot`,
//! `pack`). The approved test `old_saves_load` loads every one on the
//! build of the day, so a bump whose old saves no longer read fails in CI
//! (docs/superpowers/specs/2026-10-28-save-migrations-design.md, 2a).

use crate::determinism::{GROUND, Run, Setup, pilot, proving_ground, proving_ground_named};
use crate::script::State;
use crate::script::party::Npc;
use crate::snapshot::Snapshot;
use crate::world::{Level, StepReason};
use crate::{Save, WorldConfig};

/// Where the fixtures are, from the workspace's root.
pub const DIR: &str = "crates/approved_tests/saves";

/// The versions of the formats a save holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Versions {
    pub save: u32,
    pub snapshot: u32,
    pub pack: u32,
}

/// This build's.
pub fn current() -> Versions {
    Versions {
        save: crate::save::VERSION,
        snapshot: crate::snapshot::VERSION,
        pack: crate::pack::VERSION,
    }
}

/// The oldest versions this build reads: a pending `breaking` bump's
/// already (`bumps/README.md`).
pub fn oldest() -> Versions {
    Versions {
        save: crate::save::OLDEST,
        snapshot: crate::snapshot::OLDEST,
        pack: crate::pack::OLDEST,
    }
}

/// The versions a fixture's file name gives (`file_name`).
pub fn versions_of(name: &str) -> Option<Versions> {
    let mut parts = name
        .strip_prefix("save-")?
        .strip_suffix(".kjkv")?
        .split('-');
    let mut next = || parts.next()?.parse().ok();
    Some(Versions {
        save: next()?,
        snapshot: next()?,
        pack: next()?,
    })
}

/// Whether a fixture of `versions` is older than this build reads.
pub fn refused(versions: Versions) -> bool {
    let oldest = oldest();
    versions.save < oldest.save
        || versions.snapshot < oldest.snapshot
        || versions.pack < oldest.pack
}

/// A fixture's file name: its three versions.
pub fn file_name(versions: Versions) -> String {
    format!(
        "save-{}-{}-{}.kjkv",
        versions.save, versions.snapshot, versions.pack
    )
}

/// The module the kitchen sink leaves behind.
const LEFT: &str = "proving_ground_left";

/// Two seats and a few wanderers keep a fixture small.
fn setup() -> Setup {
    let pinned = Setup::pinned();
    Setup {
        world: WorldConfig {
            seats: 2,
            wanderers: 4,
            ..pinned.world
        },
        ..pinned
    }
}

/// The proving ground (the level the save is in) and the kitchen sink: a
/// world that leaves the arena for a proving ground it then leaves for
/// another, stepped by the determinism run's pilot, saved, and given by
/// hand what the run leaves at its start.
pub fn kitchen_sink() -> (Level, Save) {
    let setup = setup();
    let (left, ground) = (proving_ground_named(LEFT), proving_ground());
    let mut run = Run::new(&setup);
    let mut tick = 0;
    let mut steps = |run: &mut Run, count: u64| {
        for _ in 0..count {
            tick += 1;
            run.step(&pilot(setup.pilot, tick, 2), StepReason::Authority);
        }
    };
    steps(&mut run, 200);
    run.install(&left);
    steps(&mut run, 200);
    run.install(&ground);
    steps(&mut run, 200);
    let mut save = Save {
        hz: setup.rate.hz(),
        level: ground.id(),
        snapshot: run.world.snapshot(),
        script: run.world.script_state().clone(),
    };
    fill(&mut save, &ground);
    (ground, save)
}

/// What the run leaves at its start, given by hand: plausible, not played.
fn fill(save: &mut Save, ground: &Level) {
    let snapshot = &mut save.snapshot;
    snapshot.video_effect = Some(3);
    snapshot.lost = Some(1_000);
    snapshot.in_combat = true;
    snapshot.seats_held = 3;
    snapshot.paused_by = Some(crate::Seat(1));
    snapshot.kept.push(crate::snapshot::Kept {
        seat: crate::Seat(1),
        npc: 0,
        at_ms: 500,
    });
    snapshot.leading = snapshot.entities.first().map(|entity| entity.id);
    snapshot.personas.push(crate::persona::Persona {
        seat: crate::Seat(0),
        appearance: 91,
        gender: 1,
        portrait: 7,
        name: "Fixture".into(),
    });
    snapshot.spawned.push(crate::snapshot::Spawned {
        kind: crate::module::ObjectKind::Placeable,
        template: "plc_fixture".into(),
        yaw: 10.0,
        holds: vec![("g_i_credits".into(), 3)],
    });
    snapshot
        .progress
        .journal
        .push(("k_fixture_quest".into(), 20));
    snapshot.progress.xp = 1_500;
    snapshot.goods.gold = 250;
    snapshot.screens.map_disabled = true;

    let script = &mut save.script;
    script.numbers.insert("K_FIXTURE_NUMBER".into(), 7);
    script.booleans.insert("K_FIXTURE_BOOLEAN".into(), true);
    script
        .strings
        .insert("K_FIXTURE_STRING".into(), "kept".into());
    script
        .places
        .insert("K_FIXTURE_PLACE".into(), ([1.0, 2.0, 0.0], 90.0));
    script.unescapable.insert(GROUND.into());
    script.locals.insert((1, 0, 3), 1);
    script.locals.insert((1, 1, 4), 12);
    script.spawned.insert(LEFT.into());
    script.journal.push(("k_fixture_quest".into(), 20));
    script.xp.insert(crate::inventory::PARTY, 1_500);
    script.gold = 250;
    script.purses.insert(2, 15);
    script.components = 4;
    script.chemicals = 6;
    script.hoards.insert(2, (1, 2));
    script.transit.insert(GROUND.into(), true);
    script.kept.insert(2);
    script.heartless.insert(2);
    script.listening.insert(2);
    script.uncommandable.insert(2);
    script.unoriented.insert(2);
    script.pazaak_won = true;
    script.last_detect_ms = 2_000;
    script.next = 9;
    script.next_effect = 3;
    script.return_button = (true, 42_000, 42_001);
    script.swung.insert(2, 1_500);
    script.rounds.insert(2, 3_000);
    script.beats.insert(2, 2_500);
    script.stealth_loss.insert(2, 2_750);
    script.galaxy.available.extend([1, 2]);
    script.galaxy.selectable.insert(1);
    script.galaxy.selected = Some(1);
    script.stealth_xp.insert(
        GROUND.into(),
        crate::script::StealthXp {
            max: 100,
            current: 60,
            loss: 10,
            enabled: true,
        },
    );
    script.locations.push(([3.0, 1.0, 0.0], 45.0));
    script.last.insert((2, 0), 1);
    script.occupants.insert(5, vec![1]);
    script.patterns.insert(2, vec![("HELP".into(), 1)]);
    script.next_item = 5;
    script.left.insert(
        LEFT.into(),
        crate::script::Left {
            at_ms: 2_000,
            uncommandable: [3].into(),
            ..Default::default()
        },
    );
    // A companion made available by template, kept with it
    // (`Npc::template_data`): the ground's door as a creature's facts will
    // do, as only its encoding matters here.
    let facts = ground
        .module()
        .and_then(|module| {
            module
                .placements
                .iter()
                .find_map(|placement| match placement {
                    crate::module::Placement::Own(facts) => Some(facts.clone()),
                    _ => None,
                })
        })
        .expect("the proving ground places its door");
    script.party.available.insert(
        0,
        Npc {
            template: "p_fixture".into(),
            object: None,
            kept: None,
            template_data: Some(crate::pack::creature_bytes(&facts).into()),
        },
    );
}

/// `save`'s level stamp and place digest made `level`'s: what a load does
/// once its data changed (part 3 of the design does it at load; till then
/// only the fixtures need it, as the proving ground's digest moves with
/// the code that builds it).
pub fn restamp(save: &mut Save, level: &Level) {
    save.level = level.id();
    if let Some(place) = &mut save.snapshot.place {
        place.digest = level.digest();
    }
}

/// The fields of the kitchen sink still at their start (the snapshot's
/// against a new world's, the scripts' against `State::default`).
/// Each is either not saved (it is a client's or a step's), or needs a
/// scripted or fought run to hold anything real; a sink that runs a
/// module's scripts would give them values.
pub const LEFT_AT_START: &[&str] = &[
    // The step's or the view's alone.
    "snapshot.transition",
    "snapshot.ready",
    "snapshot.departing",
    "snapshot.authority",
    // Need a module's sounds, a conversation, a minigame, pazaak,
    // a map, scripts' visuals.
    "snapshot.area_sound",
    "snapshot.world_fade_in",
    "snapshot.talk",
    "snapshot.minigame",
    "snapshot.pazaak",
    "snapshot.explored",
    "snapshot.visuals",
    // The scripts' running state: needs scripts run.
    "script.pending",
    "script.delayed",
    "script.actions",
    "script.perceived",
    "script.effects",
    "script.spell_events",
    "script.applied",
    "script.removed",
    "script.bashes",
    "script.talk",
    "script.strikes",
    "script.goals",
    "script.intents",
    "script.watch",
    "script.perception",
    "script.corpses",
    "script.encounters",
    // Items, trades, careers, traps, factions, pazaak: need a module's
    // catalog, store, classes, traps or factions.
    "script.items",
    "script.trades",
    "script.careers",
    "script.levelling",
    "script.known",
    "script.reputation",
    "script.traps",
    "script.skill_uses",
    "script.door_mines",
    "script.pazaak",
    "script.floor",
    // Not saved (KOTOR's client keeps them).
    "script.movies",
    "script.movie_runs",
    "script.no_clicks_until",
    "script.keep_stealth",
    "script.encounter_creatures",
];

/// Which of `save`'s fields are still at their start. Every field is named
/// (no `..`), so a new one fails to build here until it is given a value
/// in `fill` or named in `LEFT_AT_START`.
pub fn left_at_start(save: &Save) -> Vec<&'static str> {
    let start = Run::new(&setup()).last;
    let blank = State::default();
    let mut out = Vec::new();
    macro_rules! check {
        ($in:literal, $now:expr, $start:expr; $($field:ident),* $(,)?) => {
            $( if $now.$field == $start.$field { out.push(concat!($in, ".", stringify!($field))); } )*
        };
    }
    let Snapshot {
        tick: _,
        time_ms: _,
        place: _,
        entities: _,
        transition: _,
        stash: _,
        area_sound: _,
        fade: _,
        world_fade_in: _,
        video_effect: _,
        talk: _,
        goods: _,
        spawned: _,
        progress: _,
        personas: _,
        seats_held: _,
        lost: _,
        in_combat: _,
        paused_by: _,
        ready: _,
        kept: _,
        leading: _,
        minigame: _,
        pazaak: _,
        explored: _,
        visuals: _,
        departing: _,
        screens: _,
        shared: _,
        authority: _,
        next_id: _,
    } = &save.snapshot;
    check!("snapshot", save.snapshot, start;
        tick, time_ms, place, entities, transition, stash, area_sound, fade,
        world_fade_in, video_effect, talk, goods, spawned, progress, personas,
        seats_held, lost, in_combat, paused_by, ready, kept, leading, minigame,
        pazaak, explored, visuals, departing, screens, shared, authority, next_id,
    );
    let State {
        numbers: _,
        booleans: _,
        strings: _,
        places: _,
        unescapable: _,
        left: _,
        locals: _,
        pending: _,
        delayed: _,
        next: _,
        actions: _,
        locations: _,
        last: _,
        uncommandable: _,
        unoriented: _,
        occupants: _,
        spawned: _,
        perceived: _,
        rounds: _,
        effects: _,
        spell_events: _,
        applied: _,
        removed: _,
        bashes: _,
        next_effect: _,
        listening: _,
        patterns: _,
        items: _,
        next_item: _,
        gold: _,
        purses: _,
        party: _,
        talk: _,
        heartless: _,
        trades: _,
        journal: _,
        xp: _,
        careers: _,
        levelling: _,
        strikes: _,
        swung: _,
        known: _,
        kept: _,
        galaxy: _,
        corpses: _,
        goals: _,
        intents: _,
        watch: _,
        reputation: _,
        traps: _,
        skill_uses: _,
        door_mines: _,
        last_detect_ms: _,
        perception: _,
        stealth_xp: _,
        stealth_loss: _,
        pazaak: _,
        pazaak_won: _,
        components: _,
        chemicals: _,
        hoards: _,
        beats: _,
        transit: _,
        movies: _,
        movie_runs: _,
        no_clicks_until: _,
        keep_stealth: _,
        return_button: _,
        encounters: _,
        encounter_creatures: _,
        floor: _,
    } = &save.script;
    check!("script", save.script, blank;
        numbers, booleans, strings, places, unescapable, left, locals, pending,
        delayed, next, actions, locations, last, uncommandable, unoriented,
        occupants, spawned, perceived, rounds, effects, spell_events, applied,
        removed, bashes, next_effect, listening, patterns, items, next_item,
        gold, purses, party, talk, heartless, trades, journal, xp, careers,
        levelling, strikes, swung, known, kept, galaxy, corpses, goals, intents,
        watch, reputation, traps, skill_uses, door_mines, last_detect_ms,
        perception, stealth_xp, stealth_loss, pazaak, pazaak_won, components,
        chemicals, hoards, beats, transit, movies, movie_runs, no_clicks_until,
        keep_stealth, return_button, encounters, encounter_creatures, floor,
    );
    out
}
