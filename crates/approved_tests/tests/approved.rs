//! The owner-approved scenarios (2026-09-30; `script_vm` and
//! `weapon_holdings` 2026-10-01; the `dialogue_…` three 2026-10-02;
//! `blank_cells_name_nothing` proposed 2026-10-02; `saves_round_trip`
//! 2026-10-02; the I31 co-op ten 2026-10-03; `scene_mode` 2026-10-17;
//! `old_saves_load` and `bump_files` proposed 2026-10-28), by the names
//! approved.
//! What each checks and how to re-pin: `docs/TESTS.md`.

use approved_tests::{discovery, hash_left_to_merge, module_source, pinned_hash};
use sim::determinism::{PINNED_TICKS, Setup};

/// The full determinism run (`kjk sim determinism`): every way agrees,
/// the run exercises the built level (the door opens), and the final hash
/// is the one both workflows pin, unless a bump file leaves it to the
/// merge (`bumps/README.md`).
#[test]
fn sim_determinism() {
    let pinned = pinned_hash().expect("the workflows pin one hash");
    let left = hash_left_to_merge().expect("the bump files read");
    let report = sim::determinism::run(&Setup::pinned(), PINNED_TICKS);
    let ground = &report.ground;
    println!(
        "final {}; B {}/{} C {}/{} D {}/{} E {:?} F {} G {}/{} unexpected {:?}; door opened {:?}, {} asks",
        report.final_hash,
        report.fresh_equal,
        report.total,
        report.reversed_equal,
        report.ticks,
        report.loaded_equal,
        report.loaded_total,
        report.replay.as_ref().map(|hash| hash.short()),
        report.other_seed_differs,
        report.rollbacks_matched,
        report.rollbacks,
        report.rollbacks_unexpected,
        ground.door_opened_at,
        ground.asks,
    );
    assert!(report.fresh_same(), "B: a fresh world went another way");
    assert!(report.reversed_same(), "C: the seat order changed a step");
    assert!(report.loaded_same(), "D: the loaded save went another way");
    assert!(report.replay_same(), "E: the replay went another way");
    assert!(
        report.other_seed_differs,
        "F: the hash does not see the seed"
    );
    assert!(
        report.rollbacks_unexpected.is_empty(),
        "G: rollbacks from ticks {:?} did not match as expected",
        report.rollbacks_unexpected
    );
    assert!(
        ground.door_opened_at.is_some(),
        "the pilot never opened the proving ground's door: the run no longer exercises door states"
    );
    if !left.is_empty() {
        println!("the pin is left to the merge: {}", left.join(", "));
        return;
    }
    assert_eq!(
        report.final_hash.to_string(),
        pinned,
        "the final hash is not the pinned one: a determinism bug, or a change on purpose, said in a bump file (bumps/README.md)"
    );
}

/// The synthetic collision probes, and, when `.env` names a KOTOR install,
/// the module probes on `end_m01aa` (`kjk collision probes k1 end_m01aa`).
#[test]
fn collision_probes() {
    let synthetic = collision::probes::synthetic();
    for probe in &synthetic.0 {
        println!(
            "{} {}",
            if probe.pass { "ok  " } else { "FAIL" },
            probe.name
        );
    }
    assert!(!synthetic.0.is_empty());
    assert_eq!(synthetic.failed(), 0, "synthetic probes failed");

    let module = "end_m01aa";
    let source = match module_source(&discovery(), vfs::Game::Kotor1, module) {
        Ok(source) => source,
        Err(why) => {
            println!("module probes skipped: {why}");
            return;
        }
    };
    let clip = collision_odyssey::built_in_clip("k1", module).expect("the built-in clip parses");
    let mut built = collision_odyssey::build_module(
        &source,
        module,
        clip.as_ref(),
        &collision_odyssey::BuildOptions::default(),
    )
    .expect("end_m01aa builds");
    let area = &built.module.areas[built.area];
    let (probes, facts) = collision_odyssey::probes::module(
        &built.module,
        area,
        &built.surfaces,
        &mut built.collision,
    );
    for probe in &probes.0 {
        println!(
            "{} k1 {module} {}",
            if probe.pass { "ok  " } else { "FAIL" },
            probe.name
        );
    }
    println!(
        "k1 {module}: {} traces, {:.2} µs each, answers digest {}",
        facts.traces, facts.trace_us, facts.traces_digest
    );
    assert_eq!(probes.0.len(), 4);
    assert_eq!(probes.failed(), 0, "module probes failed");
}

/// `surfacemat.2da`'s grass column read per material, and the made-up
/// walls, footprints and navigation meshes beside it (no installs needed;
/// proposed 2026-10-08; the funnel past a gate between two blocks' corners
/// 2026-10-28).
#[test]
fn surface_probes() {
    probed("surfaces", &collision_odyssey::probes::probes(), 8);
}

fn near(what: &str, measured: f32, expected: f32, tolerance: f32) {
    assert!(
        (measured - expected).abs() <= tolerance,
        "{what}: measured {measured}, JA's {expected} (± {tolerance})"
    );
}

/// `ragdoll::probes` on bodies made there (docs/RAGDOLLS.md).
#[test]
fn ragdoll_physics() {
    let probes = ragdoll::probes::all();
    for probe in &probes {
        println!(
            "{} {}: {} ({})",
            if probe.pass { "ok  " } else { "FAIL" },
            probe.name,
            probe.expect,
            probe.measured
        );
    }
    assert_eq!(probes.len(), 8);
    assert!(
        probes.iter().all(|probe| probe.pass),
        "ragdoll probes failed"
    );
}

/// `kjk move measure` with the stand-in animation lengths: JA's numbers
/// (docs/MOVEMENT.md).
#[test]
fn movement_measures() {
    let m = sim::measure::measure(&sim::motion::AnimTable::stand_in());

    // Speeds, units/s: the mean over the last second held. Walking, JA's
    // client sends 64 of 127.
    let speed = |name: &str| {
        m.speeds
            .iter()
            .find(|s| s.name == name)
            .unwrap_or_else(|| panic!("no {name} speed"))
    };
    for (name, expected) in [
        ("run", 250.0),
        ("walk", 250.0 * 64.0 / 127.0),
        ("backward", 187.5),
        ("strafe", 250.0),
        ("diagonal", 250.0),
        ("crouched", 125.0),
        ("backward walk", 250.0 * 64.0 / 127.0),
    ] {
        let s = speed(name);
        println!(
            "{name}: mean {:.2}, top {:.2}, 95% at {} ms",
            s.mean, s.top, s.rise_ms
        );
        near(name, s.mean, expected, 0.05);
    }
    near(
        "run: 95% of top speed (ms)",
        speed("run").rise_ms as f32,
        144.0,
        0.0,
    );

    // Jumps: JA's steps written out, and every jump the same.
    println!("JA's jump: apex {:.3} units, {} ms", m.ja_apex, m.ja_air_ms);
    near("JA's apex (units)", m.ja_apex, 33.68, 0.005);
    assert_eq!(m.ja_air_ms, 600, "JA's time in the air");
    let jumps: Vec<&str> = m.jumps.iter().map(|jump| jump.name).collect();
    assert_eq!(jumps, ["standing", "running", "held"], "one flight each");
    for jump in &m.jumps {
        near(jump.name, jump.apex, m.ja_apex, 0.01);
        assert_eq!(jump.air_ms, 600, "{}: ms in the air", jump.name);
    }
    near(
        "running jump (units along)",
        m.jumps[1].distance,
        150.0,
        0.5,
    );

    // Steps: 18 units climbed, 18.2 blocked.
    for step in &m.steps {
        let climbs = step.rise <= 18.0;
        assert_eq!(step.walked_up, climbs, "a step of {} units", step.rise);
        assert_eq!(
            step.step_event, climbs,
            "the step event at {} units",
            step.rise
        );
    }

    // Slopes: WALKABLE_NORMAL_Z 0.7 is 45.57°; 44° is walked, 45.6° slides.
    for slope in &m.slopes {
        let walks = slope.degrees <= 44.0;
        assert_eq!(slope.reached_top, walks, "a ramp of {}°", slope.degrees);
    }

    // A roll at a run: one, as long as BOTH_ROLL_F holds it (to the tick).
    assert_eq!(m.rolls.len(), 1, "one roll");
    let roll = m.rolls[0];
    println!(
        "roll {}..{} ms (BOTH_ROLL_F {} ms), {:.1} units",
        roll.from, roll.to, m.roll_anim_ms, roll.distance
    );
    near(
        "roll (ms)",
        (roll.to - roll.from) as f32,
        m.roll_anim_ms as f32,
        8.0,
    );

    // Drops: a hard landing (the fall event) from 63 units; from 65 the
    // player is in the air long enough to play the in-air animation.
    for drop in &m.drops {
        assert_eq!(
            drop.fall_event,
            drop.height >= 63.0,
            "the fall event off {} units ({})",
            drop.height,
            drop.keys
        );
        let in_air = drop.legs.iter().any(|(_, name)| *name == "BOTH_INAIR1");
        assert_eq!(
            in_air,
            drop.height >= 65.0,
            "in the air off {} units",
            drop.height
        );
    }

    // The 43-unit tunnel: crouched all the way through, standing after.
    assert!(m.tunnel.stays_crouched_inside, "stood up in the tunnel");
    assert!(m.tunnel.standing_at_end, "still crouched after the tunnel");
}

/// Every rig timeline's sharpest jolt stays under its ceiling. Needs JA
/// and KOTOR installs and `--features rig` (kjk's library, Bevy).
#[test]
fn timeline_jolt_ceilings() {
    #[cfg(not(feature = "rig"))]
    println!("timeline_jolt_ceilings skipped: run with --features rig (builds kjk's library)");
    #[cfg(feature = "rig")]
    rig::jolt_ceilings();
}

/// Who holds which weapon model as a play scene's world changes
/// (`kjk_game::holdings::probes`): a seat armed again retires what it held,
/// and no character takes a retired weapon when the world starts over in
/// the same scene (a joined player's swap). Needs `--features rig` (the
/// game's library, Bevy); no installs.
#[test]
fn weapon_holdings() {
    #[cfg(not(feature = "rig"))]
    println!("weapon_holdings skipped: run with --features rig (builds the game's library)");
    #[cfg(feature = "rig")]
    {
        let checks = kjk_game::holdings::probes();
        for check in &checks {
            let mark = if check.ok { "ok  " } else { "FAIL" };
            println!("{mark} {}: {}", check.name, check.detail);
        }
        let failed: Vec<&str> = checks.iter().filter(|c| !c.ok).map(|c| c.name).collect();
        assert!(checks.len() >= 6, "probes went missing");
        assert!(failed.is_empty(), "holdings probes failed: {failed:?}");
    }
}

/// Watching a pazaak match on screen (`kjk::probes::pazaak_watch`, R9a):
/// the host watching the match a guest's companion plays sees its table,
/// and whatever it clicks or keys there sends no move; its Leave and the
/// Leave key stop watching (`Action::Watch` with no player), the player's
/// End Turn is its move, and the host's click on the companion's pazaak
/// icon watches its match; a watcher is told a result in the third person
/// ("Carth won the set."), and a watched player's name not yet known is
/// asked for again. Needs `--features rig` (kjk's library, Bevy); no
/// installs.
/// (proposed 2026-10-15)
#[test]
fn pazaak_watch_view() {
    #[cfg(not(feature = "rig"))]
    println!("pazaak_watch_view skipped: run with --features rig (builds kjk's library)");
    #[cfg(feature = "rig")]
    probed("pazaak view", &kjk::probes::pazaak_watch(), 6);
}

/// The grass a K1 module's scene is drawn with (`kjk::probes::grass`):
/// Dantooine's courtyard (`danm14aa`) has patches with tufts, placed the
/// same on a second build; the Endar Spire (`end_m01aa`) has none; an area
/// naming a texture the install lacks keeps its grass but no patches. The
/// grass option (`display.grass`, on by default) reads back, and turning it
/// off hides the grass entity and on again shows it, the same entity both
/// times. Needs `--features rig` (kjk's library, Bevy); the scene checks
/// also need a K1 install.
/// (proposed 2026-10-07)
#[test]
fn grass_in_scenes() {
    #[cfg(not(feature = "rig"))]
    println!("grass_in_scenes skipped: run with --features rig (builds kjk's library)");
    #[cfg(feature = "rig")]
    probed("grass in scenes", &kjk::probes::grass(), 3);
}

/// TSL's rain and snow held round the camera (`kjk::probes::weather`): the
/// six copies stand on a hexagon of 4.5 m round the camera's ground point,
/// one ahead, one behind and four at the sides 5 m higher, turning with the
/// camera; a particle is folded into the 10 m square round the camera; with
/// the weather systems over a rainy area of a wet and a dry room the copies
/// are out in the wet one, gone in the dry one and back outside, and after
/// the rain stops still out 5.9 s on and gone at 6 s; and TSL's Dxun landing
/// rains (with dry and wet rooms), Telos's polar area snows, each with its
/// model's emitters all moved by the wind, and K1's Dantooine courtyard has
/// no weather. The lightning: a storm at chance 100 strikes every 5 to 10 s
/// over 600 s (from a flash's end), both flashes (clips 1 to 3, within 9.9 m
/// aside and 10 m up) and distant thunder, and never at chance 0; with the
/// storm's systems the flash's light stands 20 m round the player + its
/// offset in the colour its clip keys, held in the shared light list, gone after,
/// each flash counted with close thunder and the rest with distant; indoors
/// nothing; and TSL's `fx_lightning01` (radius 20, multiplier 100, three
/// keyed clips) and its four thunder sounds load, K1 playing no thunder.
/// Needs `--features rig` (kjk's library, Bevy); the area checks skip a game
/// that is not installed.
/// (proposed 2026-10-08)
#[test]
fn weather_in_scenes() {
    #[cfg(not(feature = "rig"))]
    println!("weather_in_scenes skipped: run with --features rig (builds kjk's library)");
    #[cfg(feature = "rig")]
    probed("weather in scenes", &kjk::probes::weather(), 10);
}

/// How many particles an emitter keeps (`kjk::probes::particle_limits`): the
/// count its stream holds (birth rate plus random birth rate, times life,
/// and the one being born), 1051 for a downpour of 150 a second living 7,
/// 1 with no births, the 4096 frame guard for particles that never die, and
/// no more than that guard; an emitter's own limit is the peak over its rest
/// values and clips (a fountain a clip keys on), and an explosion keeps at
/// least its whole burst (birth rate 1 when none is set), never past the
/// guard. The same probes check how emitters draw and fire: a world-z quad
/// stands on world z turned to the camera's side (upside down for an
/// upside-down camera), a particle-direction quad is long along its
/// particle's heading, and a `detonate` at a loop's last frame is passed
/// once as the clip wraps, not as it first begins. And as the executable
/// moves them: a bouncing particle is turned back off what it meets, slowed
/// by its bounce coefficient, and lies there once slow; one is born within
/// half its emitter's spread, as fast as its velocity up or down by its
/// random one; a way into a body's capsule meets it at its skin. Needs
/// `--features rig` (kjk's library, Bevy).
/// (proposed 2026-10-08; birth and bounce 2026-10-23)
#[test]
fn particle_limits() {
    #[cfg(not(feature = "rig"))]
    println!("particle_limits skipped: run with --features rig (builds kjk's library)");
    #[cfg(feature = "rig")]
    probed("particle limit", &kjk::probes::particle_limits(), 27);
}

/// A room clip a script plays (`PlayRoomAnimation`) in a module's scene
/// outside minigames (`kjk::probes::script_room_clips`): TSL's Peragus `103per` with
/// the sim playing `scriptloop01` on `103pert` since 0 ms, stepped 2 s,
/// plays that clip on the room and its puffs' emitters have particles
/// (their birth rate keyed by the clip alone); no other room plays a
/// script's clip. Needs `--features rig`
/// (kjk's library, Bevy); skips without a TSL install.
/// (proposed 2026-10-07)
#[test]
fn script_room_clips() {
    #[cfg(not(feature = "rig"))]
    println!("script_room_clips skipped: run with --features rig (builds kjk's library)");
    #[cfg(feature = "rig")]
    probed("script room clips", &kjk::probes::script_room_clips(), 1);
}

/// Every fight of the playthrough, a `fight` goal's and any goal's fight
/// back, by one rule (`kjk::probes::chase`, `kjk sim probes --only "harness
/// chase"`): on the arena, a blade swings at a foe within the sim's swing,
/// stepping in beyond its creatures' reach and standing within it; a foe out
/// of the swing that no way leads to, or that the walk to it arrived short
/// of, is gone at straight, never swung at from where the walk ended (the
/// Dantooine ruins' guard droids); a gun fires at a foe it sees within its
/// reach, and from where it stands at one no way leads to; a walk left from
/// a chase is planned again for the target a script then has the player
/// strike (`walk_for`: Peragus's shift console). Needs `--features
/// rig` (kjk's library); no installs.
/// (proposed 2026-10-13)
#[test]
fn harness_chase() {
    #[cfg(not(feature = "rig"))]
    println!("harness_chase skipped: run with --features rig (builds kjk's library)");
    #[cfg(feature = "rig")]
    probed("harness chase", &kjk::probes::chase(), 17);
}

#[cfg(feature = "rig")]
mod rig {
    use super::discovery;

    /// The ceiling per timeline (m/s²): the top jolt measured on 2026-09-30
    /// (in the comment: node, seconds) plus about a tenth. Peaks that are
    /// JA's own data are allowed where they are, with a small margin: the
    /// staff's `BOTH_A7_KICK_B` (the foot snaps to full extension in one
    /// frame, as Ghoul2 plays it) and the melee kick's like it.
    const CEILINGS: &[(&str, f32)] = &[
        ("showcase", 2_900.0),   // 2 635 rfoot_g 6.885
        ("acrobatics", 5_400.0), // 4 840 lfoot_g 19.340, a cartwheel landing
        ("specials", 2_300.0),   // 2 040 rhand_g 5.285
        ("defense", 3_900.0),    // 3 510 rhand_g 7.300
        ("force", 2_200.0),      // 1 952 rhand_g 8.650
        ("staff", 7_700.0),      // 7 574 rfoot_g 27.500, BOTH_A7_KICK_B
        ("dual", 4_800.0),       // 4 293 lhand_g 6.300
        ("reactions", 2_400.0),  // 2 167 lhand_g 4.100
        ("melee", 7_700.0),      // 7 571 rfoot_g 8.300, the kick
        ("styles", 4_000.0),     // 3 611 rhand_g 10.270
        ("gestures", 1_600.0),   // 1 415 head_g 15.500
        ("look", 2_600.0),       // 2 326 rfoot_g 16.400
    ];

    pub fn jolt_ceilings() {
        let discovery = discovery();
        for game in [vfs::Game::JediAcademy, vfs::Game::Kotor1] {
            if discovery.get(game).is_none() {
                println!(
                    "timeline_jolt_ceilings skipped: {} not found (set {} in .env)",
                    game.key(),
                    game.env_var()
                );
                return;
            }
        }
        let mut over = Vec::new();
        for timeline in kjk::rig_measure::timelines() {
            let notes = kjk::rig_measure::notes(&discovery, timeline)
                .unwrap_or_else(|error| panic!("{timeline}: {error}"));
            let jolts = kjk::rig_measure::jolts(&notes);
            assert!(!jolts.is_empty(), "{timeline}: no jolts measured");
            let (node, peak, at) = jolts
                .iter()
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .cloned()
                .expect("checked above");
            let ceiling = CEILINGS
                .iter()
                .find(|(name, _)| name == timeline)
                .map(|(_, ceiling)| *ceiling);
            let named = ceiling.map_or("none: a new timeline needs one".to_string(), |c| {
                format!("{c:.0}")
            });
            println!("{timeline}: top jolt {peak:.0} m/s² ({node} at {at:.3} s), ceiling {named}");
            if ceiling.is_none_or(|ceiling| peak > ceiling) {
                over.push(format!(
                    "{timeline} {peak:.0} ({node} at {at:.3} s), ceiling {named}"
                ));
            }
        }
        assert!(over.is_empty(), "jolts over their ceilings: {over:?}");
    }
}

/// The script VM's probes (`kjk script probes`): programs written here,
/// each instruction family, routine arguments and defaults, deferred
/// code, faults. With a KOTOR install, every compiled script in every
/// layer decodes, writes back byte for byte and calls only routines its
/// `nwscript.nss` declares (`kjk script check`).
#[test]
fn script_vm() {
    passed("script", &script_ncs::probes::all(), 30);

    let discovery = discovery();
    for game in [vfs::Game::Kotor1, vfs::Game::Kotor2] {
        let Some(install) = discovery.get(game) else {
            println!("{}: no install, skipped", game.key());
            continue;
        };
        let index = vfs::DirIndex::build(&install.data_root).expect("the install indexes");
        let base = vfs::resolver::Resolver::base(game, &index, vfs::resolver::TextureQuality::High)
            .expect("the install opens");
        let nss = fmt_odyssey::ResType::NSS;
        let ncs = fmt_odyssey::ResType::NCS;
        let nwscript = base
            .find("nwscript", nss)
            .and_then(|found| found.location.read().ok())
            .expect("nwscript.nss reads");
        let text: String = nwscript.iter().map(|&byte| char::from(byte)).collect();
        let files: Vec<(String, Vec<u8>)> = base
            .every_layer(&index)
            .iter()
            .flat_map(|layer| {
                layer
                    .resources
                    .iter()
                    .filter(|((_, restype), _)| *restype == ncs)
                    .map(move |((name, _), location)| {
                        let bytes = location.read().expect("a script reads");
                        (format!("{}/{name}", layer.name), bytes)
                    })
            })
            .collect();
        let report = script_ncs::probes::install(
            files
                .iter()
                .map(|(name, bytes)| (name.clone(), bytes.as_slice())),
            &text,
        );
        println!(
            "{}: {} scripts, {} routines, {} failures",
            game.key(),
            report.scripts,
            report.routines,
            report.failures.len()
        );
        assert!(
            report.scripts > 1000,
            "{}: too few scripts found",
            game.key()
        );
        assert!(
            report.failures.is_empty(),
            "{}: {:?}",
            game.key(),
            &report.failures[..report.failures.len().min(10)]
        );
    }
}

/// `****` names nothing (`fmt_odyssey::is_blank`), in a 2DA cell and in a
/// layout's room line alike. A module built here from bytes made here: the
/// layout's `****` room is left out, and a placeable whose `placeables.2da`
/// row has `****` for a model gets none, while its neighbour's model reads.
#[test]
fn blank_cells_name_nothing() {
    use asset_odyssey::made::{blank_module, gff, node, resref};
    use asset_odyssey::module::{Kind, ModuleIr};
    use fmt_odyssey::gff::GffValue;
    use fmt_odyssey::twoda::TwoDa;

    for (text, named) in [
        (&b"****"[..], None),
        (b" **** ", None),
        (b"", None),
        (b"  ", None),
        (b"PLC_Chair ", Some("plc_chair")),
    ] {
        let shown = String::from_utf8_lossy(text);
        assert_eq!(fmt_odyssey::named(text).as_deref(), named, "{shown:?}");
    }

    let placeable = |appearance| {
        node(vec![
            ("TemplateResRef", resref("plc")),
            ("Appearance", GffValue::Dword(appearance)),
        ])
    };
    let row = |cells: [&str; 2]| cells.map(|cell| cell.as_bytes().to_vec()).to_vec();
    let placeables = TwoDa {
        columns: vec![b"label".to_vec(), b"modelname".to_vec()],
        row_labels: vec![b"0".to_vec(), b"1".to_vec()],
        cells: vec![row(["chair", "PLC_Chair"]), row(["nothing", "****"])],
        column_separator: b'\t',
        row_separator: b'\t',
    };
    let mut files = blank_module(Vec::new(), &["****", "Room_A"]);
    files.0.insert(
        "area.git".into(),
        gff(
            b"GIT ",
            vec![(
                "Placeable List",
                GffValue::List(vec![placeable(0), placeable(1)]),
            )],
        ),
    );
    files.0.insert("placeables.2da".into(), placeables.write());
    let module = ModuleIr::build("blank", &files).expect("the module builds");
    let area = &module.areas[0];
    let rooms: Vec<&str> = area.rooms.iter().map(|room| room.model.as_str()).collect();
    assert_eq!(rooms, ["room_a"], "the layout's rooms");
    let models: Vec<Option<&str>> = area
        .instances
        .iter()
        .filter(|instance| instance.kind == Kind::Placeable)
        .map(|instance| instance.model.as_deref())
        .collect();
    assert_eq!(models, [Some("plc_chair"), None], "the placeables' models");
}

/// Areas' grass and weather from the ARE (`docs/GRASS-WEATHER.md`):
/// a module made here reads its grass texture, density, size and variant
/// chances, none without a texture, its weather chances and wind, and the
/// rooms it keeps dry. No installs (proposed 2026-10-07).
#[test]
fn area_grass_and_weather() {
    probed(
        "area grass and weather",
        &asset_odyssey::module::probes(),
        5,
    );
}

/// A room's grass faces and clusters (`docs/GRASS-WEATHER.md`): from
/// a room model made here, the faces of its AABB mesh whose material is grassy
/// (none without the AABB or a grassy material), at the node's offset; on each
/// `trunc(area × density)` clusters, none on a thin face, the same every time
/// and different when the room moves, each with its turn, quarter and size in
/// range; and the C library's `rand` giving 41, 18467, 6334 from seed 1. No
/// installs (proposed 2026-10-08).
#[test]
fn grass_placement() {
    probed("grass placement", &asset_odyssey::grass::probes(), 10);
}

/// An area's weather from its chances (`docs/GRASS-WEATHER.md`): rain
/// over snow over lightning, running while a roll of 0 to 99 is under the
/// chance used, so a Dxun-like area has rain with lightning, a polar one snow,
/// a lightning-only one (Malachor) lightning without a fall, a dry one nothing,
/// and a half-chance area rain below 50 and none from 50, with its wind
/// whatever the roll; and the roll of each 300 s the same for a resref in any
/// case and in range, and over 1000 epochs at chance 50 a fair coin: under 50
/// for 40 to 60 % of them, the weather changing at over a quarter of the
/// steps. No installs (proposed 2026-10-08).
#[test]
fn weather_rolls() {
    probed("weather rolls", &asset_odyssey::weather::probes(), 7);
}

/// The same facts in the games' own areas: TSL's Dxun landing has rain and
/// lightning with some rooms dry and some wet, its Telos area snow, and K1's
/// Dantooine area no rain, snow or lightning and the grass `lda_grass2`. Skips
/// a game that is not installed (proposed 2026-10-07).
#[test]
fn area_grass_and_weather_in_the_games() {
    let discovery = discovery();
    for (game, module) in [
        (vfs::Game::Kotor2, "401dxn"),
        (vfs::Game::Kotor2, "261tel"),
        (vfs::Game::Kotor1, "danm14aa"),
    ] {
        if discovery.get(game).is_none() {
            println!(
                "{module} skipped: no KOTOR install (set {} in .env)",
                game.env_var()
            );
            continue;
        }
        let source = module_source(&discovery, game, module).expect("it opens");
        probed(
            module,
            &asset_odyssey::module::install_probes(module, &source),
            1,
        );
    }
}

/// Prints `checks` and fails on any that did not pass, or on fewer than
/// `at_least` (probes gone missing).
fn passed(what: &str, checks: &[script_ncs::probes::Check], at_least: usize) {
    for check in checks {
        let mark = if check.ok { "ok  " } else { "FAIL" };
        println!("{mark} {}: {}", check.name, check.detail);
    }
    let failed: Vec<&str> = checks.iter().filter(|c| !c.ok).map(|c| c.name).collect();
    assert!(checks.len() >= at_least, "{what} probes went missing");
    assert!(failed.is_empty(), "{what} probes failed: {failed:?}");
}

/// `passed` for checks named with a result each.
fn probed<N: AsRef<str>>(what: &str, checks: &[(N, Result<String, String>)], at_least: usize) {
    for (name, result) in checks {
        let name = name.as_ref();
        match result {
            Ok(detail) => println!("ok   {name}: {detail}"),
            Err(detail) => println!("FAIL {name}: {detail}"),
        }
    }
    let failed: Vec<&str> = checks
        .iter()
        .filter(|(_, result)| result.is_err())
        .map(|(name, _)| name.as_ref())
        .collect();
    assert!(checks.len() >= at_least, "{what} probes went missing");
    assert!(failed.is_empty(), "{what} probes failed: {failed:?}");
}

/// The formats conversations read, from bytes made here: a DLG written as
/// a GFF and read back, a LIP's shapes held and blended, a PCM WAV's and
/// KOTOR's MP3-behind-a-RIFF voice file's lengths.
#[test]
fn dialogue_formats() {
    passed(
        "dialogue format",
        &sim::script::dialogue_probes::formats(),
        4,
    );
}

/// What conversations keep: answer bytes (a count that moves, so a repeated
/// command answers once), and the scripts' state and the snapshot's view
/// of conversations through their encodings.
#[test]
fn dialogue_state() {
    passed("dialogue state", &sim::script::dialogue_probes::kept(), 4);
}

/// K1's Endar Spire, headless, answering through seat 0's command as
/// `kjk dialogue run` does: Trask walks up and his opening plays to
/// "Okay." with the first reply each time (the class checks choose the
/// scoundrel's lines for the dev soldier); Carth's comm call plays on
/// entering the Starboard Section. Needs a K1 install, else skipped.
#[test]
fn dialogue_endar_spire() {
    let discovery = discovery();
    if discovery.get(vfs::Game::Kotor1).is_none() {
        println!("skipped: no KOTOR install (set KJK_KOTOR1 in .env)");
        return;
    }
    let trask = conversation(&discovery, "end_m01aa", 70);
    println!("end_m01aa: {trask:?}");
    assert_eq!(
        trask,
        [
            "end_trask01 E54",
            "end_trask01 E55",
            "end_trask01 E57",
            "end_trask01 E58",
            "end_trask01 E61",
            "end_trask01 E62",
            "end_trask01 E63",
            "end_trask01 E64",
            "end_trask01 E65",
            "end_trask01 E73",
            "end_trask01 ends",
        ]
        .map(String::from)
    );
    let carth = conversation(&discovery, "end_m01ab", 20);
    println!("end_m01ab: {carth:?}");
    assert_eq!(
        carth,
        ["end_carth001 E6", "end_carth001 E7", "end_carth001 ends"].map(String::from)
    );
}

/// `module` of K1 stepped `seconds` for one player who answers each offer
/// with its first reply once the line has played: the lines shown and the
/// ends, in order (conversations that end before saying anything left
/// out).
fn conversation(discovery: &vfs::Discovery, module: &str, seconds: u64) -> Vec<String> {
    use sim::script::dialogue::{Line, Pick, answer};
    let source = module_source(discovery, vfs::Game::Kotor1, module).expect("the module opens");
    let request = session::Request::new("k1", module, None, 1);
    let anims = std::sync::Arc::new(sim::motion::AnimTable::stand_in());
    let plan = session::plan(&source, &request, anims).expect("the module plans");
    let config = sim::WorldConfig {
        seed: 1,
        seats: 1,
        wanderers: 0,
        spawn: [0.0; 3],
        yaw: 0.0,
        sheet: sim::combat::Sheet::dev_soldier(),
        guests: sim::Guests::Avatars,
    };
    let mut world = sim::SimWorld::new(config, sim::Level::arena()).expect("a world");
    plan.commit(&mut world).expect("the module installs");
    let rate = sim::TickRate::default();
    let (mut reply, mut answered) = (0, std::collections::BTreeSet::new());
    for tick in 1..=seconds * u64::from(sim::TICK_HZ) {
        let mut input = sim::TickInput::default();
        let command = sim::Command {
            reply,
            ..sim::Command::default()
        };
        input.insert(sim::Seat(0), command);
        let snapshot = sim::step(
            &mut world,
            tick,
            &input,
            rate.msec(tick),
            sim::StepReason::Authority,
        );
        let waiting = snapshot.talk.conversations.iter().any(|c| {
            c.until_ms == u64::MAX && !c.replies.is_empty() && answered.insert((c.id, c.shown_ms))
        });
        if waiting {
            reply = answer(reply, Pick::Reply(0));
        }
    }
    let talk = &world.script_report().talk;
    let spoke: std::collections::BTreeSet<u32> = talk
        .iter()
        .filter_map(|line| match line {
            Line::Entry { id, .. } => Some(*id),
            _ => None,
        })
        .collect();
    talk.iter()
        .filter_map(|line| match line {
            Line::Entry { dialog, entry, .. } => Some(format!("{dialog} E{entry}")),
            Line::Ended { id, dialog, .. } if spoke.contains(id) => Some(format!("{dialog} ends")),
            _ => None,
        })
        .collect()
}

/// KOTOR's layouts and fonts (`kjk gui probes`): layouts, fonts and
/// screens built here, each drawn and checked. With KOTOR installs, every
/// `.gui` of every layer parses and paints with no unknown control kind
/// and no missing font, and K1's with no missing texture (`kjk gui
/// sweep`).
#[test]
fn gui_layouts() {
    let checks = ui::probes::all();
    for check in &checks {
        let mark = if check.ok { "ok  " } else { "FAIL" };
        println!("{mark} {}: {}", check.name, check.detail);
    }
    let failed: Vec<&str> = checks.iter().filter(|c| !c.ok).map(|c| c.name).collect();
    assert!(checks.len() >= 75, "probes went missing");
    assert!(failed.is_empty(), "gui probes failed: {failed:?}");

    let discovery = discovery();
    for game in [vfs::Game::Kotor1, vfs::Game::Kotor2] {
        if discovery.get(game).is_none() {
            println!("{}: no install, skipped", game.key());
            continue;
        }
        let sweep = ui::sweep::sweep_install(&discovery, game).expect("the install opens");
        let textures = sweep.refusals.get("texture").map_or(0, Vec::len);
        println!(
            "{}: {} layouts, {} missing textures",
            game.key(),
            sweep.layouts,
            textures
        );
        assert!(sweep.layouts > 80, "{}: too few layouts", game.key());
        // K1's textures all resolve but those the game fills at run time
        // (`ui::sweep::PLACEHOLDERS`).
        assert!(
            sweep.passes(game),
            "{}: parse {:?}, kinds {:?}, refusals {:?}",
            game.key(),
            sweep.failed,
            sweep.unknown_kinds,
            sweep.refusals
        );
    }
}

/// Containers (I21's loot): two seats at one container and one out of
/// reach (`sim::containers::probes`): the contested first item goes to
/// the lower seat and the other is told it is gone, taking everything left
/// works once, out of reach and closed are refused, what a script gave a
/// container is shown with its own items while it is held open, a save
/// loads with no container held open, and two runs end the same. The sim
/// alone, no window, no install.
#[test]
fn ui_loot() {
    probed("loot", &sim::containers::probes(), 5);
}

/// TSL's party rules (`sim::tsl_probes`): natural healing and its hold,
/// influence, puppets, forfeits, the puppet's follow, Bao-Dur met and the
/// lines it names, alignment following influence, and a companion spawned
/// twice in one script standing once (`spawned_twice_is_one`, Bao-Dur
/// joining on Telos), and the player restrict mode ending with its module
/// (`restrict_mode_ends_with_module`: the asteroid walk's had held the
/// AI idle on Telos), and a minigame's area taking no companions
/// (`minigame_takes_no_companions`: Peragus's hangar turret), and the
/// party selection screen's exit script told the party was accepted
/// (`party_screen_accepted`: the Ebon Hawk's ramp on Dantooine), and the
/// area's iteration passing over the dead (`area_iteration_skips_the_dead`:
/// the battle of Khoonda's end), and a companion spawned again while its
/// creature is here being that creature (`spawned_again_is_the_same`:
/// Atton's warning on Nar Shaddaa), and `k_align_movie` run as the status
/// summary closes (`summary_closed_runs_align_movie`: Visas Marr's
/// arrival). The sim alone, on the proving ground.
#[test]
fn tsl_party_rules() {
    probed("tsl", &sim::tsl_probes::probes(), 20);
}

/// The dead's remains (KOTOR's body bags; `sim::remains::probes`), owner
/// 2026-10-05: on the proving ground a creature holding droppable items and
/// gold is killed, its body taken away after its appearance's delay and
/// fade (a script's destroy mid-fade leaving the fade to run: approved
/// 2026-10-10), and a placeable stands where it lay holding those items and the
/// gold, the target block finding it there (approved 2026-10-10); a save keeps it, the world loaded from it takes all and closes
/// it, and it goes; one a script keeps (`SetIsDestroyable(FALSE)`) leaves
/// no remains; the player running through where one lies does not shove its
/// body (a corpse passes through bodies, JA's dead mask); two runs end the
/// same. The sim alone.
#[test]
fn corpse_remains() {
    probed("remains", &sim::remains::probes(), 9);
}

/// `DestroyObject` as K1's engine takes it (`sim::destroys::probes`),
/// approved 2026-10-10: on the proving ground a destroyed thing leaves the
/// world at its delay and is drawn departing after: `bNoFade` takes a
/// creature away at once, unfaded; the defaults fade one out over a
/// second; a delay with `fDelayUntilFade` keeps it in the world to its
/// delay and draws it whole until the fade begins; a placeable fades too;
/// of two destroys in one step the sooner is kept; a fading one destroyed
/// again never comes back; a destroyed container does not open; a
/// destroyed beast strikes no more and is no one's target; a destroyed
/// creature does not block the way; a mid-fade snapshot round-trips and
/// loads; each run twice ends the same. The sim alone.
#[test]
fn destroy_fades() {
    probed("destroys", &sim::destroys::probes(), 13);
}

/// Using an object (`docs/UI.md`): a seat's
/// `Action::Use` round-trips on the wire; on the proving ground the host
/// opens and closes the door within reach and not from afar, drops a
/// stale verb, opens a container, fails a locked one (its script and the
/// "locked" line), uses a placeable, talks to a creature at conversation
/// range, and one behind a wall within it (KOTOR's conversation needs
/// no sight: Dantooine's Administrator across her desk), one on a ledge
/// above within it across the floor (Dxun's Kumus), refuses what a
/// wall hides and a door closed on someone; a K1
/// container's use runs its `OnUsed`, locked (no "locked" line) or opening;
/// scripts see who did it; two runs end the same. The sim alone.
#[test]
fn use_actions() {
    probed("use", &sim::uses::probes(), 37);
}

/// Bashing (`docs/COMBAT.md`): on the proving ground
/// the player's blow on the locked gate takes off what it is worth less the
/// gate's hardness and raises its `OnDamaged` with the player as damager;
/// a creature outside the party hurts nothing; nor does a weapon on what is
/// not locked, is plot, or out-hards it, though the one it out-hards hears
/// it was attacked (`OnMeleeAttacked`, not `OnDamaged`); a script's damage
/// hurts what is not plot; Min1HP holds at 1; the gate's hit points gone,
/// it breaks open, unlocked, its `OnDeath` raised, and neither closes nor
/// takes more; a grenade blast's hit breaks a locked safe; a creature whose
/// heartbeat asks `DoDoorAction(…, DOOR_ACTION_BASH)` breaks a locked box
/// beside it; the player holding attack at the locked gate breaks it with
/// its blaster, empty-handed with its fists, and with a stun baton (the
/// sim's own shots and punches); a baton with Door Cutting (TSL's plasma
/// torch) breaks a gate of hardness 100 a plain baton leaves whole; the
/// state round-trips; two runs end the same. The sim alone.
#[test]
fn bash_doors() {
    probed("bash", &sim::objects::probes(), 19);
}

/// Placeables' looks and the visuals on objects (proposed 2026-10-06, the
/// Star Forge's captive Jedi): on the proving ground a captive placed on
/// is used, and its `OnUsed` leaves it off (`PlayAnimation`) and puts a
/// drain beam on it from a placeable beside it, with the drain's flash; the
/// snapshot carries both and round-trips; the flash goes after a second,
/// the beam after its time; two runs end the same. The sim alone.
#[test]
fn placeable_looks() {
    probed("placeable looks", &sim::visuals::probes(), 7);
}

/// The area's map in the sim (`docs/UI.md`): on
/// the proving ground a script's `SetMapPinEnabled` hides a waypoint's
/// note shown as placed and shows one hidden as placed; standing explores
/// the map's cells within reach and not the far ones, standing across the
/// room more; what is explored survives leaving the module and coming
/// back, and a save and load; `RevealMap` explores round a point,
/// `ExploreAreaForPlayer` the whole; the state round-trips; two runs end
/// the same. The sim alone.
#[test]
fn map_fog() {
    probed("map", &sim::charting::probes(), 11);
}

/// The games' scripts using objects (`docs/UI.md`):
/// on the proving ground a creature's `ActionUseObject` walks it to a panel
/// and the panel's `OnUsed` sees it as the user; another's
/// `ActionInteractObject` walks it to the shut door, which opens and runs
/// its `OnOpen` once with it as the opener; a lasting visual a spawn script
/// puts on is kept and gone once the heartbeat clears effects; a held use
/// runs a panel's `OnUsed` once, as KOTOR's click (again only once let go,
/// a click with it the same use); what the use key would act on
/// (`SimWorld::key_use`), aimed at the door, at a panel beside it and away,
/// is what a press then opens or uses, and nothing else; `GetHasSpell` is
/// a power known and paid for (K1's exe), not known alone; a silent shout
/// is heard within a creature's hearing, not across the area; a scripted run
/// given a creature thrown into the air is judged once it lands, and taken;
/// two runs end the same. The sim alone.
#[test]
fn use_scripts() {
    probed("script use", &sim::uses::script_probes(), 3);
}

/// Walking to a target (`docs/UI.md`): on the
/// proving ground the player, steered by `sim::target::Walk` in place of
/// its keys, walks from the entry to a panel out of reach and uses it on
/// arriving; its own forward key on the way cancels the walk and nothing is
/// used; two runs end the same. The sim alone.
#[test]
fn use_walk() {
    probed("walk", &sim::uses::walk_probes(), 3);
}

/// Security in one press (owner's playtest, 2026-10-25: "Security
/// sometimes needs two clicks to open a door"): on the proving ground the
/// player chooses Security once on a locked door from near, straight on,
/// at an angle, from afar and already running, with the door's scripts and
/// without; walked there where out of reach, it unlocks and opens the door
/// every time. The sim alone.
#[test]
fn security_walk() {
    probed("security walk", &sim::uses::security_walk_probes(), 7);
}

/// Traps and mines (`docs/COMBAT.md`), owner
/// 2026-10-04: KOTOR's detect, disarm, recover and lay rules at fixed
/// rolls, and on the proving ground a mine detected, sprung on a hostile
/// (its script run), spared by its own faction, disarmed and recovered,
/// one laid and sprung, the trap routines, a laid mine kept through a
/// snapshot; the sim alone.
#[test]
fn traps_lifecycle() {
    probed("traps", &sim::traps::probes(), 20);
}

/// Stealth and Awareness (`docs/COMBAT.md`),
/// owner 2026-10-04: item skill bonuses worn, the spot and listen scores
/// at fixed rolls, stealth turned on and refused, ended by attacking,
/// unseen and seen on the proving ground at KOTOR's cadence, the sneak
/// opener, stealth XP; the sim alone.
#[test]
fn stealth_spot() {
    probed("stealth", &sim::stealth::probes(), 20);
}

/// A beast's attack (docs/COMBAT.md, "Beasts"): on the proving ground a
/// creature whose model has no humanoid swing, its script setting it on a
/// dummy, lands blows of its creature weapon at its attack's hit moment, a
/// round apart; its rounds run (`ScriptEndRound`) and the dummy hears it was
/// attacked (`ScriptAttacked`); a humanoid with empty hands punches with
/// its fists instead; two runs end the same. The sim alone.
#[test]
fn beast_strikes() {
    probed("strike", &sim::strikes::probes(), 8);
}

/// A fight through a hold (docs/EVENTS.md, "End of round"): on the proving
/// ground a creature its script sets on a dummy, its rounds attacking again
/// as KOTOR's AI's do, is held for 30 s with the dummy in its reach; its
/// rounds run on through the hold and it lands blows again once the hold
/// is over (the Endar Spire's two Jedi, their scene holding them apart,
/// once stood idle for good); a creature its script sets on a dummy behind
/// the shut door keeps it as its target every step, unseen, past four
/// rounds (KOTOR's attack walks to its target until it reaches it); two
/// runs end the same. The sim alone.
#[test]
fn fight_through_a_hold() {
    probed("hold", &sim::strikes::hold_probes(), 3);
}

/// Fists (docs/COMBAT.md, "Fists"): on the proving ground a player with
/// empty hands, holding attack, punches a bare-handed foe to death, the
/// left and right punches in turn a round apart, each blow halfway through
/// its punch; the foe closes in and punches the player back, its punches in
/// the snapshot; two runs end the same. The sim alone.
#[test]
fn fists_brawl() {
    probed("fists", &sim::strikes::fist_probes(), 7);
}

/// Weapons out and away (docs/COMBAT.md, "Weapons out and away"): on the
/// proving ground a saber a click lit (its draw flourish played) goes out
/// 8 s after the click (its put-away played); the
/// holster key puts it out at once, though its 8 s still run, and mid-swing
/// as the swing ends; an attack lights it again; a weapon taken out is out;
/// a saber a script holds lit stays lit; a foe's saber is lit as it closes
/// in; a gun away is raised before it fires; the action reads back; two runs end the same. The sim alone.
/// (approved 2026-10-11)
#[test]
fn holster_weapon() {
    probed("sheath", &sim::strikes::sheath_probes(), 12);
}

/// A pistol in each hand (KOTOR's two pistols, `combat::pair`): the guns
/// take turns, right then left, each from its own muzzle with its own
/// two-weapon penalty; the hand keys take neither down; a trigger let go
/// banks no shot; a pair broken mid-fight fires its one gun from the right;
/// Rapid Shot fires main, off, main, Power Blast both guns for one cost,
/// Sniper Shot the main gun alone; a hostile creature with a pair fires
/// both, oftener than with one; the pair's state comes back whole from a
/// snapshot mid-burst; two runs end the same. The sim alone.
/// (approved 2026-10-25)
#[test]
fn dual_pistols() {
    probed("dual pistols", &sim::pair_probes::probes(), 11);
}

/// Saving and loading (I19, the `saves` crate): a world saved into a saves
/// folder made here, read back and made again goes on as the saved one
/// does, the same hash every tick for 500; the folder lists the latest first
/// and refuses an id that would leave it. A save is refused with the player
/// down or the party in a fight. With a K1 install (else that part is
/// skipped), the same on `end_m01aa` saved during Trask's conversation
/// (which refuses a player's save) and loaded into the module planned again.
#[test]
fn saves_round_trip() {
    let root = std::env::temp_dir().join(format!("kjk-saves-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);

    let level = sim::determinism::proving_ground();
    let mut world = sim::SimWorld::new(Setup::pinned().world, level.clone()).expect("a world");
    walk(&mut world, 300);
    let first = save(&root, &mut world, None);
    let loaded = load(&root, &first, &level);
    assert_eq!(first, "0001", "the first save's number");
    goes_on_alike(world, loaded, "the proving ground");

    let mut quick = sim::SimWorld::new(Setup::pinned().world, level.clone()).expect("a world");
    save(&root, &mut quick, Some(saves::QUICK));
    let listing = saves::list(&root);
    let ids: Vec<&str> = listing.saves.iter().map(|(id, _)| id.as_str()).collect();
    println!("listed {ids:?}, unread {:?}", listing.unread);
    assert_eq!(ids, ["quick", "0001"], "the latest first");
    assert!(
        saves::read(&root, "../0001").is_err(),
        "an id outside the folder"
    );

    refusals(&quick.snapshot(), quick.level());

    let discovery = discovery();
    if discovery.get(vfs::Game::Kotor1).is_none() {
        println!("end_m01aa skipped: no KOTOR install (set KJK_KOTOR1 in .env)");
        let _ = std::fs::remove_dir_all(&root);
        return;
    }
    let source = module_source(&discovery, vfs::Game::Kotor1, "end_m01aa").expect("it opens");
    let request = session::Request::new("k1", "end_m01aa", None, 1);
    let anims = std::sync::Arc::new(sim::motion::AnimTable::stand_in());
    let plan = || session::plan(&source, &request, anims.clone()).expect("it plans");
    let config = sim::WorldConfig {
        seats: 1,
        wanderers: 0,
        ..Setup::pinned().world
    };
    let mut world = sim::SimWorld::new(config, sim::Level::arena()).expect("a world");
    plan().commit(&mut world).expect("it installs");
    let mut talking = None;
    for _ in 0..70 * sim::TICK_HZ {
        walk(&mut world, 1);
        let snapshot = world.snapshot();
        if !snapshot.talk.conversations.is_empty() {
            talking = Some(snapshot);
            break;
        }
    }
    let talking = talking.expect("Trask talks within 70 s");
    assert_eq!(
        saves::refusal(&talking, sim::Seat(0), world.level()).as_deref(),
        Some("You cannot save during a conversation.")
    );
    let id = save(&root, &mut world, None);
    let loaded = load(&root, &id, &plan().level);
    goes_on_alike(world, loaded, "end_m01aa in Trask's conversation");
    let _ = std::fs::remove_dir_all(&root);
}

/// `world` stepped `ticks` on, seat 0 walking about and using what it
/// faces now and then.
fn walk(world: &mut sim::SimWorld, ticks: u64) {
    let rate = sim::TickRate::default();
    let from = world.snapshot().tick;
    for tick in from + 1..=from + ticks {
        let mut input = sim::TickInput::default();
        input.insert(
            sim::Seat(0),
            sim::Command {
                angles: sim::moves::view_angles(0.0, (tick % 2000) as f32 * 0.18),
                forward: if tick % 400 < 300 { 127 } else { 0 },
                buttons: if tick % 97 < 5 { sim::buttons::USE } else { 0 },
                ..sim::Command::default()
            },
        );
        sim::step(
            world,
            tick,
            &input,
            rate.msec(tick),
            sim::StepReason::Authority,
        );
    }
}

/// `world` saved into `root` as `slot` (a new number without one): its id.
fn save(root: &std::path::Path, world: &mut sim::SimWorld, slot: Option<&str>) -> String {
    let snapshot = world.snapshot();
    let meta = saves::Meta {
        name: "approved".into(),
        module: world.level().id().name,
        area: "here".into(),
        saved: saves::now(),
        seats: snapshot.seats(),
        ..saves::Meta::default()
    };
    let save = sim::Save {
        hz: sim::TICK_HZ,
        level: world.level().id(),
        script: world.script_state().clone(),
        snapshot,
    };
    saves::write(root, slot, &save, &meta).expect("the save is written")
}

/// The save `id` made again in `level`, as a game loads it.
fn load(root: &std::path::Path, id: &str, level: &sim::Level) -> sim::SimWorld {
    let (save, _) = saves::read(root, id).expect("the save reads");
    save.level.check(level).expect("the save's level");
    let mut world = sim::SimWorld::from_snapshot(&save.snapshot, level.clone()).expect("a world");
    world.set_script_state(save.script);
    world
}

/// Both worlds walked 500 ticks on stay the same, tick by tick.
fn goes_on_alike(mut saved: sim::SimWorld, mut loaded: sim::SimWorld, what: &str) {
    for n in 1..=500 {
        walk(&mut saved, 1);
        walk(&mut loaded, 1);
        let (a, b) = (saved.snapshot().hash(), loaded.snapshot().hash());
        assert_eq!(
            a, b,
            "{what}: the loaded world went another way {n} ticks on"
        );
    }
    println!("{what}: saved and loaded, the same for 500 ticks");
}

/// A quiet moment saves; seat 0 in a fight or down does not.
fn refusals(quiet: &sim::Snapshot, level: &sim::Level) {
    let refusal = |snapshot: &sim::Snapshot| saves::refusal(snapshot, sim::Seat(0), level);
    assert_eq!(refusal(quiet), None, "a quiet moment");
    let mut fighting = quiet.clone();
    fighting.in_combat = true;
    assert_eq!(
        refusal(&fighting).as_deref(),
        Some("You cannot save during combat.")
    );
    use sim::combat::fighter::Life;
    for life in [Life::Downed, Life::Dead] {
        let mut snapshot = quiet.clone();
        let me = snapshot
            .entities
            .iter_mut()
            .find(|entity| entity.kind == sim::Kind::Avatar(sim::Seat(0)))
            .and_then(|entity| entity.fighter.as_mut())
            .expect("seat 0 fights");
        me.life = life;
        assert_eq!(
            refusal(&snapshot).as_deref(),
            Some("You cannot save while you are down."),
            "{life:?}"
        );
    }
}

/// Guests drive companions (I31): `sim::drivers::probes`, the sim alone.
#[test]
fn guests_drive_companions() {
    probed("drivers", &sim::drivers::probes(), 30);
}

/// Conversations in co-op (I31): `sim::coop_talk::probes`, every phase's
/// cases and the whole scenario run twice ending alike; the sim alone.
#[test]
fn dialogue_coop() {
    probed("coop talk", &sim::coop_talk::probes(), 135);
}

/// Scene mode (`docs/DIALOGUE.md`): the scene probe group,
/// `sim::scene_probes::all` (`kjk sim probes scene`), each case on the
/// proving ground with conversations and scripts written there; the sim
/// alone. Approved by the owner 2026-10-17 (do what you think is best).
#[test]
fn scene_mode() {
    probed("scene", &sim::scene_probes::all(), 69);
}

/// The party's calls (I31): `sim::party::probes`, among them in combat,
/// a break's freeze and the ready check, and what enters a trigger; the sim
/// alone.
#[test]
fn party_calls() {
    probed("party", &sim::party::probes(), 20);
}

/// Falling and being revived (I31): `sim::falls::probes`, each on the
/// proving ground; the sim alone.
#[test]
fn downed_and_revive() {
    probed("falls", &sim::falls::probes(), 18);
}

/// Items and levels in co-op (I31): `sim::coop_items::probes`, the
/// scenario's checks and two runs ending alike; the sim alone.
#[test]
fn coop_items_and_levels() {
    probed("coop items", &sim::coop_items::probes(), 21);
}

/// Items used and upgraded (the owner's bugs of 2026-10-04):
/// `sim::item_uses::probes`, a use taken from the goods, the HUD's kinds
/// as they have them (those for the user's race), Use Item doing what the
/// item's button does or refused when nothing would, a droid's worn items,
/// a lightsaber's crystals fitted and taken out; the sim alone.
#[test]
fn item_uses() {
    probed("item uses", &sim::item_uses::probes(), 24);
}

/// Item properties beyond combat (`sim::inventory::probes`): an item's
/// best bonus of a kind with its penalties, typed extras and Keen; two
/// items' scores, skills, saves, dodge defence, defence against a race and
/// a bonus feat on a sheet, held by the caps, gone when taken off; use
/// limits (a side's, K1's and TSL's "All", Proficiency All, a fitted
/// crystal's light-side limit, a race's, an ability's); immunity to
/// criticals; on-hit stun, a mind-immune target, death; regeneration every
/// 6 s; a worn light lasting; TSL's upgrade slots; the encodings. The sim
/// alone, no install.
#[test]
fn item_properties() {
    probed("item properties", &sim::inventory::probes::probes(), 26);
}

/// `SwitchPlayerCharacter` (K1's Leviathan, TSL's many): `sim::switch::probes`,
/// a script switching the first seat to a companion made from its
/// template, the seat walking it, scripts seeing it, a switch back, saves,
/// snapshots and prediction, a standing creature taken, a guest giving way;
/// the sim alone.
#[test]
fn switch_player_character() {
    probed("switch", &sim::switch::probes(), 16);
}

/// Encounters count the drivers (I31): `sim::scaling::probes`, and
/// `session::encounters::probes` (the count a plan is for reaches its
/// module); no install.
#[test]
fn encounters_count_drivers() {
    probed("scaling", &sim::scaling::probes(), 12);
    probed("encounter count", &session::encounters::probes(), 3);
}

/// Minigames in co-op (`docs/MINIGAMES.md`):
/// `sim::minigame::probes`, on courses of our own: a race's run per seat
/// with its own hooks, globals and finish (the module held for the others,
/// 20 s at most), the turret's gunner per seat with its own aim and bolts,
/// fighters scaled per gunner, a gunner's own compass through its lens, a
/// course with runs and gunners encoded and stepped twice alike; no
/// install.
#[test]
fn minigame_coop() {
    probed("minigame", &sim::minigame::probes::probes(), 13);
}

/// The galaxy map and the game's end: `sim::galaxy::probes`, on the
/// proving ground with two seats and scripts written here: the map a script
/// shows reaches both with the planets made available and selectable; the
/// guest's travel and the host's to a planet not selectable run nothing,
/// the host's runs the travel script, which hears the planet;
/// `EndGame(TRUE)` and K1's argument-less `EndGame()` lose the game,
/// `EndGame(FALSE)` shows both the end, after the credits when
/// `StartCreditSequence` started them; the sim alone.
#[test]
fn galaxy_and_end() {
    probed("galaxy and end", &sim::galaxy::probes(), 6);
}

/// Pazaak (`docs/PAZAAK.md`): `sim::pazaak::probes`, on decks of our own:
/// the cards as `pazaakdecks.2da` and the card items write them, the wager
/// and side deck checked, the rules on tables set by hand (a bust and its
/// rescue, 20 and the ninth card standing, a tie replayed and broken, the
/// third set winning), the opponent's play, a match and its moves through
/// their bytes, and a match a script starts played to its end script (the
/// wager held while it is played, the credits settled, the result read),
/// the party's credits spent meanwhile not sparing the loss, twice alike;
/// two players' matches at once (each end script its own result, each
/// wager held on its own, in seat order), a paused match resuming where it
/// stood, a module change and an exit quitting one, and a decided one
/// settling as decided; no install.
#[test]
fn pazaak() {
    probed("pazaak", &sim::pazaak::probes::probes(), 21);
}

/// Passive feats and feat prerequisites (`combat::passive`, `levels`):
/// `sim::feats::probes`, on sheets and classes made here: two-weapon
/// fighting's penalties by tier and a balanced off hand, a double-bladed
/// weapon, Dueling's to hit and defence, the proficiency penalty and
/// Proficiency All, the equip gates, Conditioning's saves, Uncanny Dodge
/// against grenades, the skill feats, Improved and Wookiee Toughness's
/// damage reduction (K1's and TSL's), levels with Toughness (picked, its
/// earlier levels), Wookiee Toughness, Force Sensitive and an ability
/// minimum; and on the proving ground, a companion refused armour and an
/// implant with the game's line, granted feats by `GrantFeat` (Toughness
/// for every level), then wearing the armour and taking a level whose
/// class grants Master Toughness, twice alike; no install.
#[test]
fn feats() {
    probed("feats", &sim::feats::probes(), 16);
}

/// A host's save kept by a player (I31): `saves::copies::probes` (packed,
/// kept byte for byte, refused when foreign, replaced by a later copy, the
/// player's own saves untouched, two hosts of one name apart, a host's
/// copies capped, no number left refused, ids inside the folder) and
/// `saves::refusal_probes` (what refuses a save); no install.
#[test]
fn save_copy_round_trip() {
    probed("save copies", &saves::copies::probes(), 12);
    probed("save refusals", &saves::refusal_probes(), 14);
}

/// The saves' safety net (`saves::safety_probes`): the meta's new fields
/// and an older meta, a damaged save listed and refused, one without a hash
/// trusted, autosaves rotating, the backup before a new release, a failed
/// write leaving nothing, a refusal naming the build that made the save;
/// no install.
#[test]
fn save_safety() {
    probed("save safety", &saves::safety_probes(), 11);
}

/// The flight recorder's file (`sim::flight::probes`): the pinned run kept
/// by a recorder and the flight side by side replays alike, a file cut
/// anywhere in its last chunk or bent there keeps the whole chunks, an
/// empty one is refused, and a replay may skip the data check; no install.
#[test]
fn flight_recorder() {
    probed("flight", &sim::flight::probes(), 6);
}

/// A save copy in parts on the wire (I30 P17):
/// `net::session::probes::copy_probes`, split, coded, decoded and joined
/// again, a cut copy and a part out of turn dropped, malformed parts
/// refused, the host's caps.
#[test]
fn save_copy_parts() {
    probed(
        "save copy parts",
        &net_checks(net::session::probes::copy_probes()),
        5,
    );
}

/// A check named `name` as `probed` takes it: what was `seen`, as passing
/// or not.
fn judged(name: String, seen: String, pass: bool) -> (String, Result<String, String>) {
    (name, if pass { Ok(seen) } else { Err(seen) })
}

/// `net::probes`' checks as `probed` takes them.
fn net_checks(probes: net::probes::Probes) -> Vec<(String, Result<String, String>)> {
    probes
        .list
        .into_iter()
        .map(|probe| {
            let seen = format!("{} ({})", probe.expect, probe.detail);
            judged(probe.name, seen, probe.pass)
        })
        .collect()
}

/// The network core (`kjk net probes`): `net::probes::groups` all pass,
/// in virtual time over a `SimNet` from fixed seeds: varints and every
/// packet kind; reliable delivery in order exactly once through loss,
/// duplication and reordering; the RTT estimate; the handshake's accept
/// and refusals; netem's conditions; the link's limits and hostile
/// peers; loopback; the same seed the same bytes; a million corrupted
/// packets with no panic.
#[test]
fn net_core() {
    for (name, run) in net::probes::groups() {
        probed(name, &net_checks(run()), 1);
    }
}

/// The snapshot codec (`kjk net codec`, no install): known answers on
/// made-up parts; the pinned determinism run with 1, 2 and 4 seats, every
/// snapshot rebuilt exactly from a keyframe and from each baseline up to
/// the client ring's depth back, a host's and a client's rings over a link
/// losing 5% and 6 snapshots each way rebuilding nothing wrong; and
/// corrupted frames of the 4-seat run never panicking or rebuilding
/// another snapshot. (`kjk net codec` adds 8 seats and KOTOR's walk.)
#[test]
fn net_codec() {
    use net::codec::{CLIENT_RING, probes};
    let synthetic = probes::synthetic();
    let checks: Vec<(String, Result<String, String>)> = synthetic
        .0
        .into_iter()
        .map(|probe| {
            let seen = format!("{} ({})", probe.found, probe.expect);
            judged(probe.name, seen, probe.pass)
        })
        .collect();
    probed("codec synthetic", &checks, 5);
    let runs: Vec<probes::Recorded> = std::thread::scope(|scope| {
        [1, 2, 4]
            .map(|seats| scope.spawn(move || probes::determinism_run(seats, PINNED_TICKS)))
            .into_iter()
            .map(|run| run.join().expect("a determinism run finishes"))
            .collect()
    });
    // Each run's rebuilds and session on a thread, the corruptions on one
    // of their own: they are the long parts.
    let checks: Vec<(String, Result<String, String>)> = std::thread::scope(|scope| {
        let mut handles: Vec<_> = runs
            .iter()
            .map(|run| {
                scope.spawn(move || {
                    let exact = probes::exactness(run, CLIENT_RING);
                    let session = probes::session(run, 5, 6, 1);
                    vec![
                        judged(
                            format!("{} rebuilt exactly", run.name),
                            format!("{} of {}, {:?}", exact.exact, exact.total, exact.failures),
                            exact.exact == exact.total && exact.total > 0,
                        ),
                        judged(
                            format!("{} session", run.name),
                            format!(
                                "{} sent, {} rebuilt, {} refused, wrong {:?}",
                                session.sent, session.rebuilt, session.refused, session.wrong
                            ),
                            session.wrong.is_empty() && session.rebuilt > 0,
                        ),
                    ]
                })
            })
            .collect();
        let four = &runs[2];
        handles.push(scope.spawn(move || {
            let mutations = probes::mutations(four, 200_000, 4);
            vec![judged(
                format!("{} corrupt frames", four.name),
                format!(
                    "{} tried, {} panics, {} accepted not the original",
                    mutations.tried, mutations.panics, mutations.accepted_wrong
                ),
                mutations.tried > 0 && mutations.panics == 0 && mutations.accepted_wrong == 0,
            )]
        }));
        handles
            .into_iter()
            .flat_map(|handle| handle.join().expect("a codec check finishes"))
            .collect()
    });
    probed("codec runs", &checks, 7);
}

/// Whole sessions (`kjk net session`, no install): the determinism world's
/// `lan`, `wan`, a one-minute `soak`, `hostile`, `reconnect`, `restart`
/// and `save_copy` (`hold` and `reload` are their own tests) played over a
/// `SimNet`, every binding check passing; and a million corrupted session
/// encodings through their decoders, none panicking.
#[test]
fn net_sessions() {
    use net::session::sessions;
    let played: Vec<sessions::Session> = sessions::determinism(1)
        .into_iter()
        .filter(|session| !["hold", "reload"].contains(&session.setup.name.as_str()))
        .collect();
    assert_eq!(played.len(), 7, "sessions went missing");
    std::thread::scope(|scope| {
        let handles: Vec<_> = played
            .into_iter()
            .map(|session| scope.spawn(move || session_checks(session)))
            .collect();
        for handle in handles {
            handle.join().expect("a session passes");
        }
    });
    let mutations = net::session::probes::decoder_mutations(1_000_000, 5);
    assert_eq!(mutations.tried, 1_000_000, "corrupted encodings tried");
    assert_eq!(mutations.panics, 0, "corrupted encodings that panicked");
}

/// `session` played over a `SimNet`; every binding check of it passes
/// (`net::session::sessions::checks`). Its report and checks' names.
fn session_checks(
    mut session: net::session::sessions::Session,
) -> (net::session::probes::Report, Vec<String>) {
    let name = session.setup.name.clone();
    let report = session.run();
    let checks = net::session::sessions::checks(&report, session.expect);
    for check in &checks {
        let mark = match (check.pass, check.binding) {
            (true, _) => "ok  ",
            (false, true) => "FAIL",
            (false, false) => "note",
        };
        println!("{mark} {name} {}: {}", check.name, check.detail);
    }
    let failed: Vec<&str> = checks
        .iter()
        .filter(|check| check.failed())
        .map(|check| check.name.as_str())
        .collect();
    assert!(failed.is_empty(), "{name} session: failed {failed:?}");
    (report, checks.into_iter().map(|check| check.name).collect())
}

/// The determinism world's session `name` (`kjk net session --only
/// name`), `session_checks`.
fn session_passes(name: &str) -> (net::session::probes::Report, Vec<String>) {
    let session = net::session::sessions::determinism(0)
        .into_iter()
        .find(|session| session.setup.name == name)
        .unwrap_or_else(|| panic!("no {name} session"));
    session_checks(session)
}

/// The party's fall reloads in place (I30 P13): the lan session, the host
/// keeping its world at 4 s and reloading it at 7 s; every client takes it
/// whole and plays on, exact and predicted, its clocks settled.
#[test]
fn reload_in_place() {
    let (report, names) = session_passes("reload");
    assert_eq!(report.reloads, 1, "one reload");
    for index in 0..report.clients.len() {
        let installs = format!("client {index} installs");
        assert!(names.contains(&installs), "no check: {installs}");
    }
}

/// The host holds its world (a movie, a break) 6 s, past the link's
/// timeout, and resumes: every client is held, stays connected and starts
/// again, exact and predicted, its clocks settled.
#[test]
fn hold_and_resume() {
    let (report, names) = session_passes("hold");
    for index in 0..report.clients.len() {
        let held = format!("client {index} held and started again");
        assert!(names.contains(&held), "no check: {held}");
    }
}

/// Character creation (`sim::creation`): scores bought with 30 points from 8
/// (a raise costing the score's modifier, 1 to 3, up to 18); the first
/// level a level of the class taken from nothing (the hit die and CON, the
/// class's rows, the feat it grants, the feat picked or the game's, the
/// ranks bought, four times a level's points), nothing worn or held,
/// neutral; refused past the points, the ranks' points or cap, a feat's
/// need or one pick too many; TSL's player feats granted besides; a build's
/// planned level-up picks before the class's; a seat's player made so (its
/// sheet, full hit points, its career in the snapshot, its persona);
/// `sim::player_probes`, no install.
#[test]
fn player_creation() {
    probed("creation", &sim::player_probes::creation(), 9);
}

/// TSL's player's level as the scripts read it (`G_PC_LEVEL`, set by the
/// XP the party gains) and `AddMultiClass` on a player (the class taken up
/// unlevelled, the party given its next level's XP); `sim::player_probes`,
/// no install.
#[test]
fn player_levels_told() {
    probed("levels told", &sim::player_probes::levels_told(), 1);
}

/// The scripts read a player's persona of its own character (`GetGender`,
/// `GetName`, `GetAppearanceType` of `GetFirstPC()`): a woman's and a man's,
/// and a player with none (male, no name); a snapshot and a world made from
/// it keep the persona. `sim::player_probes`, no install.
#[test]
fn player_persona_scripts() {
    probed("persona", &sim::player_probes::persona_scripts(), 4);
}

/// A player's body is dressed by the body armour it wears
/// (`sim::persona::dressed`): its underclothes, a robe put on, taken off
/// (`sim::player_probes`). With a K1 install (else that part is skipped),
/// `appearance.2da` dresses K1's player bodies so: a man's scout build
/// (row 137) `pmbam` in `pmbama01` bare and `pmbbm` in `pmbbm01` in the
/// Endar Spire's clothes (`g_a_clothes01`), a woman's (row 92) `pfbam`
/// and `pfbbm`.
#[test]
fn body_dressed_by_armour() {
    probed("dressing", &sim::player_probes::dressing(), 4);
    let discovery = discovery();
    if discovery.get(vfs::Game::Kotor1).is_none() {
        println!("K1's bodies skipped: no KOTOR install (set KJK_KOTOR1 in .env)");
        return;
    }
    let source = module_source(&discovery, vfs::Game::Kotor1, "end_m01aa").expect("it opens");
    let mut tables = asset_odyssey::tables::Tables::default();
    let mut look = |appearance: u32, armour: Option<&str>| {
        let look = asset_odyssey::creature::resolve_appearance(
            &source,
            appearance,
            armour.map(str::to_string),
            &mut tables,
        )
        .expect("the appearance resolves");
        (look.body, look.body_texture)
    };
    let seen = [
        look(137, None),
        look(137, Some("g_a_clothes01")),
        look(92, None),
        look(92, Some("g_a_clothes01")),
    ];
    println!("K1's bodies: {seen:?}");
    let named = |body: &str, texture: &str| (body.to_string(), Some(texture.to_string()));
    assert_eq!(
        seen,
        [
            named("pmbam", "pmbama01"),
            named("pmbbm", "pmbbm01"),
            named("pfbam", "pfbama01"),
            named("pfbbm", "pfbbm01"),
        ]
    );
}

/// The rulebook (`data/rules/`, proposed 2026-10-24): `docs/RULEBOOK.md`
/// is the page `cargo xtask rulebook` writes from it; every rule is read by
/// the code; every literal constant in `crates/sim` is a rule's alias or
/// listed in `data/rules/not-rules.txt` (`rules::audit`); a table change
/// changes its cell (`vfs::resolver::change_cell`) and, for each game
/// installed, names a row and a column its table has.
#[test]
fn rulebook() {
    use fmt_odyssey::twoda::TwoDa;
    let change = rules::TableChange {
        game: "k1",
        table: "made",
        row: "Second",
        column: "numdice",
        value: "2",
        why: "a test",
    };
    let mut made = TwoDa {
        columns: vec![b"label".to_vec(), b"numdice".to_vec()],
        row_labels: vec![b"0".to_vec(), b"1".to_vec()],
        cells: vec![
            vec![b"First".to_vec(), b"1".to_vec()],
            vec![b"second".to_vec(), b"1".to_vec()],
        ],
        column_separator: b'\t',
        row_separator: b'\t',
    };
    vfs::resolver::change_cell(&mut made, &change).expect("the row by its label");
    assert_eq!(
        made.int(1, "numdice"),
        Some(2),
        "the labelled row's cell changed"
    );
    assert_eq!(made.int(0, "numdice"), Some(1), "the other row's cell kept");
    let by_number = rules::TableChange { row: "0", ..change };
    vfs::resolver::change_cell(&mut made, &by_number).expect("the row by its number");
    assert_eq!(
        made.int(0, "numdice"),
        Some(2),
        "the numbered row's cell changed"
    );
    let discovery = discovery();
    for game in [vfs::Game::Kotor1, vfs::Game::Kotor2] {
        let Ok(source) = module_source(
            &discovery,
            game,
            if game == vfs::Game::Kotor1 {
                "end_m01aa"
            } else {
                "001ebo"
            },
        ) else {
            continue;
        };
        let problems: Vec<&str> = source
            .0
            .problems
            .iter()
            .filter(|problem| problem.source == vfs::resolver::RULEBOOK)
            .map(|problem| problem.problem.as_str())
            .collect();
        assert!(
            problems.is_empty(),
            "{}'s table changes: {problems:?}",
            game.key()
        );
    }
    let root = approved_tests::workspace();
    let page =
        std::fs::read_to_string(root.join("docs/RULEBOOK.md")).unwrap_or_else(|_| String::new());
    assert!(
        page == rules::page::markdown(),
        "docs/RULEBOOK.md is stale: run `cargo xtask rulebook`"
    );
    let problems = rules::audit::problems(&root);
    assert!(
        problems.is_empty(),
        "the rulebook ({} problems):\n  {}",
        problems.len(),
        problems.join("\n  ")
    );
}

/// Every fixture in `crates/approved_tests/saves` (a code-built save from each
/// pin that moved a stored format, `sim::fixtures`) loads on this build:
/// decoded, re-encoded and decoded the same, and made a world on its level;
/// the kitchen sink leaves no field at its start but those it names; and
/// fixtures from before a `breaking` bump are refused by their version
/// (docs/superpowers/specs/2026-10-28-save-migrations-design.md, 2a).
#[test]
fn old_saves_load() {
    let dir = approved_tests::workspace().join(sim::fixtures::DIR);
    let (level, sink) = sim::fixtures::kitchen_sink();
    let set = |fields: &[&'static str]| {
        fields
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
    };
    assert_eq!(
        set(&sim::fixtures::left_at_start(&sink)),
        set(sim::fixtures::LEFT_AT_START),
        "the kitchen sink's fields at their start: give a new field a value in `sim::fixtures`, or name it in LEFT_AT_START with why"
    );
    let mut seen = 0;
    for entry in std::fs::read_dir(&dir).expect("the fixtures folder") {
        let path = entry.expect("a fixture").path();
        if path.extension().is_none_or(|extension| extension != "kjkv") {
            continue;
        }
        let bytes = std::fs::read(&path).expect("a fixture reads");
        let name = path
            .file_name()
            .expect("a fixture's name")
            .to_string_lossy();
        let versions = sim::fixtures::versions_of(&name)
            .unwrap_or_else(|| panic!("{name}: not a fixture's name"));
        // Older than a pending `breaking` bump: its pin retires it.
        if sim::fixtures::refused(versions) {
            let error = sim::Save::decode(&bytes)
                .map(|_| ())
                .expect_err("a fixture before a breaking bump is refused");
            assert!(error.to_string().contains("version"), "{name}: {error}");
            seen += 1;
            continue;
        }
        let mut save =
            sim::Save::decode(&bytes).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let again = sim::Save::decode(&save.encode()).expect("a re-encoded save decodes");
        assert_eq!(again, save, "{}: re-encoding changes it", path.display());
        sim::fixtures::restamp(&mut save, &level);
        save.world(&level, 2, None)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        seen += 1;
    }
    assert!(seen > 0, "no fixtures in {}", dir.display());
    let newest = dir.join(sim::fixtures::file_name(sim::fixtures::current()));
    assert!(
        newest.exists() || approved_tests::stored_bumps_pending(),
        "no fixture at this build's versions: `cargo run --release -p approved_tests --bin save_fixture`"
    );
    // The kitchen sink as this build writes it, damaged three ways: each
    // refused, none a panic.
    let first = sink.encode();
    let mut newer = first.clone();
    newer[4..8].copy_from_slice(&(sim::fixtures::current().save + 1).to_le_bytes());
    let mut companion = sink.clone();
    let npc = companion
        .script
        .party
        .available
        .values_mut()
        .find(|npc| npc.template_data.is_some())
        .expect("the kitchen sink keeps a companion's template");
    npc.template_data = Some(vec![0xff; 3].into());
    let damaged = [
        ("a newer build's", newer, "version"),
        ("half of one", first[..first.len() / 2].to_vec(), "save"),
        (
            "a broken companion's",
            companion.encode(),
            "a kept companion",
        ),
    ];
    for (what, bytes, says) in damaged {
        let error = sim::Save::decode(&bytes)
            .map(|_| ())
            .expect_err(what)
            .to_string();
        assert!(error.contains(says), "{what}: {error}");
    }
    // The network's snapshots stay at this build's version alone
    // (`Snapshot::decode`): checked once a stored snapshot is older than
    // this build's, which no fixture is yet.
    for entry in std::fs::read_dir(dir.join("retired")).into_iter().flatten() {
        let path = entry.expect("a retired fixture").path();
        let bytes = std::fs::read(&path).expect("a retired fixture reads");
        let error = sim::Save::decode(&bytes)
            .map(|_| ())
            .expect_err("a retired fixture is refused");
        assert!(
            error.to_string().contains("version"),
            "{}: {error}",
            path.display()
        );
    }
}

/// Bump files say how old saves are read (bumps/README.md): a format saves
/// hold, bumped without `[migrations]` or `breaking`, is refused, as is a
/// migration of a format not bumped; and pending bumps of one format get
/// ordered named versions (`sim::since`).
#[test]
fn bump_files() {
    let cases: &[(&str, &str, Result<(), &str>)] = &[
        (
            "ok.toml",
            "[formats]\nsnapshot = \"x\"\n[migrations]\nsnapshot = \"old: none\"\n",
            Ok(()),
        ),
        (
            "breaks.toml",
            "breaking = \"why\"\n[formats]\nsave = \"x\"\n",
            Ok(()),
        ),
        ("net.toml", "[formats]\ninstall = \"x\"\n", Ok(())),
        (
            "bare.toml",
            "[formats]\npack = \"x\"\n",
            Err("pack: say how old saves are read"),
        ),
        (
            "stray.toml",
            "[formats]\nsave = \"x\"\n[migrations]\nsave = \"y\"\nsnapshot = \"z\"\n",
            Err("snapshot is not bumped"),
        ),
        (
            "2026-fix.toml",
            "[formats]\nsave = \"x\"\n[migrations]\nsave = \"y\"\n",
            Err("not a name"),
        ),
        (
            "net-migrated.toml",
            "[formats]\ninstall = \"x\"\n[migrations]\ninstall = \"y\"\n",
            Err("install is not stored in saves"),
        ),
    ];
    for (name, text, expected) in cases {
        let got = bumps::parse_file(name, text).map(|_| ());
        match expected {
            Ok(()) => assert!(got.is_ok(), "{name}: {got:?}"),
            Err(part) => assert!(
                got.as_ref().is_err_and(|error| error.contains(part)),
                "{name}: {got:?}"
            ),
        }
    }
    let migrated = "[formats]\nsave = \"x\"\n[migrations]\nsave = \"y\"\n";
    let mut two = vec![
        bumps::parse_file("b-later.toml", migrated).expect("a migrated bump"),
        bumps::parse_file("a-first.toml", migrated).expect("a migrated bump"),
    ];
    two.sort_by(|a, b| a.path.cmp(&b.path));
    assert_eq!(
        bumps::pending_since(&two, "save"),
        vec![("A_FIRST".to_string(), 1), ("B_LATER".to_string(), 2)]
    );
    // A name a pinned bump took, or two files of one name, is refused by
    // the file: its `since` constant would be another's.
    let pinned = "pub mod save {\n    pub const A_FIRST: crate::Since = crate::Since(crate::Stored::Save, 60);\n}\n";
    let reused = bumps::clashes(&two, pinned).expect_err("a pinned name reused");
    assert!(reused.contains("a-first.toml"), "{reused}");
    two.push(bumps::parse_file("a_first.toml", migrated).expect("a migrated bump"));
    let twice = bumps::clashes(&two, "").expect_err("two files of one name");
    assert!(twice.contains("a_first.toml"), "{twice}");
}
