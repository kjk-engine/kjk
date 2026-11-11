//! The owner-approved scenarios (2026-09-30; `script_vm` and
//! `weapon_holdings` 2026-10-01; the `dialogue_…` three 2026-10-02;
//! `blank_cells_name_nothing` proposed 2026-10-02; `saves_round_trip`
//! 2026-10-02; the I31 co-op ten 2026-10-03; `scene_mode` 2026-10-17;
//! `old_saves_load` and `bump_files` proposed 2026-10-28; `stale_stamps_load`
//! proposed 2026-10-30), by the names approved.
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
    probed("surfaces", &collision_odyssey::probes::probes(), 9);
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

/// The fight's grunts (`kjk_game::grunts::probes`): the odds, the lines,
/// and each character's grunts kept `combat.grunts.gap_ms` apart, a gap
/// started only by a grunt heard (#265). Needs `--features rig`; no installs.
#[test]
fn grunts() {
    #[cfg(not(feature = "rig"))]
    println!("grunts skipped: run with --features rig (builds the game's library)");
    #[cfg(feature = "rig")]
    probed("grunts", &kjk_game::grunts::probes(), 7);
}

/// Object and item names as the screens show them
/// (`kjk_game::dialogue_view::name_probes`). The player's tokens are filled
/// in, so TSL's "<FullName>'s Kolto Tank" on Peragus and its
/// "<FullName>'s Starport Visa" show the player's name. A gender's pair
/// takes the player's word, and braced notes and an item's sort key are
/// left out. A token in another case ("<fullname>") is not one, as in both
/// games' executables. Without a player the tokens stand as they are.
/// Needs `--features rig`; no installs (approved 2026-11-11).
#[test]
fn shown_names() {
    #[cfg(not(feature = "rig"))]
    println!("shown_names skipped: run with --features rig (builds the game's library)");
    #[cfg(feature = "rig")]
    probed("shown names", &kjk_game::dialogue_view::name_probes(), 7);
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

/// A danglymesh's spring and the area's wind (`kjk::probes::dangly`), as
/// the executables run them: a still area has no wind, a windy one's noise
/// cells stay within 0 to 0.963 and its gusts turn it within its limits; a
/// still mesh stays at rest and in a wind leans its tip downwind, its root
/// pinned; a walk leaves its tip behind while its
/// pinned root (constraint 0) stays, a turn swings the tip no further than
/// its box (`displacement × (1 − constraint/255)` per axis), and a jump of
/// more than 5 m snaps it back to rest. Needs `--features rig`.
/// (proposed 2026-11-03)
#[test]
fn dangly_spring() {
    #[cfg(not(feature = "rig"))]
    println!("dangly_spring skipped: run with --features rig (builds kjk's library)");
    #[cfg(feature = "rig")]
    probed("dangly spring", &kjk::probes::dangly(), 7);
}

/// A head held level over a leaning spine with a weapon out
/// (`kjk::probes::straightened`), on a skeleton made here: a node beside
/// the head that no standing clip keys (a woman's lower neck), which the
/// pose turns afresh each frame but never moves, posed alike 2000 times at
/// 400 fps, is set alike every frame and stays on the chest (each frame's
/// straightening once built on the last, a shoulder drawn metres out at a
/// high frame rate: issue 238), and with the weapon away it stands at rest.
/// Needs `--features rig`; no installs.
/// (proposed 2026-11-09)
#[test]
fn held_node_straightened() {
    #[cfg(not(feature = "rig"))]
    println!("held_node_straightened skipped: run with --features rig (builds kjk's library)");
    #[cfg(feature = "rig")]
    probed("held node straightened", &kjk::probes::straightened(), 3);
}

/// A companion whose template no module of the session ships, drawn from
/// what the party table keeps of it (`kjk::probes::kept_companion`, issue
/// 309). A creature with no `appearance.2da` row keeps nothing. Dark side
/// Bastila's template ships only in the Temple Summit. Placed on the Star
/// Forge from her kept appearance and worn items, she is drawn with the
/// same models as the Temple Summit draws her with. Needs `--features
/// rig`; the second check needs a K1 install, else it is skipped.
/// (proposed 2026-11-11)
#[test]
fn kept_companion_drawn() {
    #[cfg(not(feature = "rig"))]
    println!("kept_companion_drawn skipped: run with --features rig (builds kjk's library)");
    #[cfg(feature = "rig")]
    probed("kept companion drawn", &kjk::probes::kept_companion(), 2);
}

/// The rooms a fight's light reaches (`kjk::probes::fight_rooms`), on rooms
/// made in the test. A VIS link one way counts both ways, and a room that
/// is only seen is linked with every room. A light's mask keeps it out of
/// the rooms not linked with its own, and rooms past the 128th are always
/// reached. Over stacked floors a light is in the room whose floor is under
/// it. Over a pit it is in the smallest room whose bounds hold it, not in a
/// room far below. Outside every room it is in none. Needs `--features
/// rig`; no installs. (proposed 2026-11-11)
#[test]
fn fight_light_rooms() {
    #[cfg(not(feature = "rig"))]
    println!("fight_light_rooms skipped: run with --features rig (builds kjk's library)");
    #[cfg(feature = "rig")]
    probed("fight light rooms", &kjk::probes::fight_rooms(), 6);
}

/// What the capsule shadows send the shader (`kjk::probes::capsule_shadows`),
/// on capsules and lights made in the test. A droid's box casts as a
/// capsule along its longest side. The sphere sent for a character holds
/// all its capsules. A light that only brightens the ambient casts no
/// shadow, and neither does a lamp out of reach. A fight's light casts only
/// when fight lights cast, and a bolt's line counts toward its reach. A
/// crate gets a box, but a small canister, a large hull, a body lying
/// down and a junk pile that fills little of its box do not. A block fills
/// all of its box, and a cylinder standing up about 78% of it. A box is
/// placed and turned by its placeable's root. The sphere a point must lie
/// in to trace any character holds every character's reach. A light at a
/// body's skin, such as a welding torch's sparks, does not cast that
/// body's shadow. Needs `--features rig`; no installs. (proposed
/// 2026-11-11; the junk pile and the fill added for #328; approved
/// 2026-11-12)
#[test]
fn capsule_shadows() {
    #[cfg(not(feature = "rig"))]
    println!("capsule_shadows skipped: run with --features rig (builds kjk's library)");
    #[cfg(feature = "rig")]
    probed("capsule shadows", &kjk::probes::capsule_shadows(), 8);
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
/// particle's heading, a quad's picture stands upright (its v climbs from
/// foot to head), and a `detonate` at a loop's last frame is passed
/// once as the clip wraps, not as it first begins. And as the executable
/// moves them: a bouncing particle is turned back off what it meets, slowed
/// by its bounce coefficient, and lies there once slow; one is born within
/// half its emitter's spread, as fast as its velocity up or down by its
/// random one; a way into a body's capsule meets it at its skin. Needs
/// `--features rig` (kjk's library, Bevy).
/// (proposed 2026-10-08; birth and bounce 2026-10-23; upright 2026-11-02;
/// approved 2026-11-12)
#[test]
fn particle_limits() {
    #[cfg(not(feature = "rig"))]
    println!("particle_limits skipped: run with --features rig (builds kjk's library)");
    #[cfg(feature = "rig")]
    probed("particle limit", &kjk::probes::particle_limits(), 28);
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

/// The saber animation audit's checks (`kjk::anim_audit::checks`) on frames
/// made here. A hilt held 0.10 m from its hand in every frame passes. The
/// same hilt slipping to 0.15 m in one frame gives a Grip of 0.05 m. A state
/// that holds it 0.10 m off, in a case that held it 0.08 m off elsewhere,
/// gives 0.02 m. A blade 0.05 m deep in the torso gives a self-cut of 0.05 m
/// with the reference blade clear, and 0 with the reference blade as deep. A
/// blade through the head counts from each head's own bone. A blade turning
/// 360 degrees in 0.25 s gives a large spin. A growing blade is not
/// measured. A drawn blade 0.2 m from the sim's gives a gap of 0.2 m in
/// contact and none outside it. A staff's off hand 0.2 m from its hilt line
/// gives a two-hands value of 0.2 m. A hand jolted 0.25 m for one frame gives
/// 800 m/s2. A hand moving 0.4 m/s after a fast first 0.3 s gives a
/// stillness of 0.4 m/s. In a held block, the stillness is the gripping
/// hand's speed in the chest's own frame. A hand that walks and turns with
/// the chest gives 0. Two frames at the same moment give no NaN. The reference is scaled by
/// the arms' length, not the torso's.
///
/// The audit's script (`kjk::anim_audit::cases`) gets a lead-in wherever its
/// blade would be put away. A case's run is cut into its states without the
/// lead-ins. A state without a lit blade is not measured, except the
/// holster. MDA rows are judged against the JA floor
/// (`kjk::anim_audit::report::floor`). A JA-rules value that moves by 0.1%
/// of its limit or less is run-timing noise. Failing rows under MDA's rules
/// are listed before those under JA's rules. A state that falls back to
/// JA's sequence, or loses a check, counts as worse against an earlier run.
/// The state `attack_stand` stands for 2 s and then attacks for 1.2 s, so it
/// needs no lead-in. A swing begins at a frame whose sim torso plays a start
/// or an attack, after a frame that plays none of a start, an attack and a
/// transition. A blade that turns 30 degrees and moves its tip 0.5 m in each
/// of the 4 steps after a swing's first frame, and then 90 degrees and 2 m in
/// each step, gives a swing-start-turn of 120 degrees and a swing-start-travel
/// of 2 m, when the Movie Duels reference's blade stays still. A reference
/// that turns 20 degrees in each step leaves 40 degrees, and one that turns
/// 40 degrees in each step leaves 0. The same frames give 0 when no swing
/// begins in them or when the blade is growing, and a swing that begins 50 ms
/// before the state's end sums 2 steps. A swing that begins on the state's
/// first frame is counted when the frame before the state is given. A blade
/// that turns 20 degrees in each of the 4 steps after the drawn body starts
/// to play another sequence than the sim's gives a stance-entry-turn of 80
/// degrees. A move that starts to cut while the drawn blade is 0.2 m from the
/// sim's gives a cut-start-gap of 0.2 m, while contact-gap is 0.5 m for the
/// same frames. The start checks are floored by their JA-rules values as
/// contact-gap is, and a JA-rules value of 0 leaves the design limit.
/// Two runs that begin differently but draw the same stepped frames measure
/// the same. Needs `--features rig` (kjk's library); no installs.
#[test]
fn anim_audit_checks() {
    #[cfg(not(feature = "rig"))]
    println!("anim_audit_checks skipped: run with --features rig (builds kjk's library)");
    #[cfg(feature = "rig")]
    {
        audit::checks();
        audit::scale();
        audit::states();
        audit::floor();
        audit::swings();
        audit::stepped();
    }
}

#[cfg(feature = "rig")]
mod audit {
    use kjk::anim_audit::cases::{Body, Case, HOLSTER, Rules, SCRIPT, Weapon, label, timeline};
    use kjk::anim_audit::checks::{self, Check, Still, Worst, measure};
    use kjk::anim_audit::report::{self, Record, StateRecord};
    use kjk::anim_audit::{Blade, Frame};

    /// The script's run and its states. A fight lasts 8 s after an attack,
    /// so a 2 s lead-in (an attack, then the stance) comes before `stand`,
    /// before `run` (which would end 8 s after the first) and before
    /// `block_walk_l` (8 s after the attack held in `block_stand_aim`). A
    /// frame in a lead-in is in no state. The blade is lit on every frame
    /// but those of `walk` and `holster`, so `walk` is unmeasured and
    /// `holster` is measured. A NaN worst survives `cases.json` and fails.
    pub fn states() {
        let (moves, spans) = timeline(SCRIPT);
        assert_eq!(moves.len(), SCRIPT.len() + 3, "three lead-ins: {moves:?}");
        let labelled = [
            (1000.0, None),
            (2500.0, Some("stand")),
            (5999.0, Some("walk")),
            (7000.0, None),
            (8500.0, Some("run")),
            (19000.0, None),
            (20500.0, Some("block_walk_l")),
            (30500.0, Some("attack_stand")),
            (33199.0, Some("attack_stand")),
            (33200.0, Some(HOLSTER)),
            (36699.0, Some("ignite")),
            (36700.0, None),
        ];
        for (ms, expected) in labelled {
            let found = label(SCRIPT, ms);
            println!("{ms} ms: {found:?}");
            assert_eq!(found, expected, "at {ms} ms");
        }

        // `attack_stand` stands 2 s, then attacks for 1.2 s. It attacks, so
        // it gets no lead-in, and neither does `holster` after it.
        let stand_attack = spans
            .iter()
            .find(|span| span.label == "attack_stand")
            .expect("an attack_stand state");
        assert_eq!(stand_attack.to - stand_attack.from, 3200);
        assert_eq!(stand_attack.from, 30000);

        let unlit = |ms: f64| matches!(label(SCRIPT, ms), Some("walk" | HOLSTER));
        let drawn = frames(1500, |i| {
            if unlit(i as f64 * 25.0) {
                frame(i, false, Vec::new())
            } else {
                held(i)
            }
        });
        let case = case(Rules::Ja, Body::Male);
        let states = case.measured(drawn);
        assert_eq!(states.len(), spans.len());
        for (span, state) in spans.iter().zip(&states) {
            assert_eq!(state.state, span.label);
            assert_eq!(
                state.frames as u64,
                (span.to - span.from) / 25,
                "{}",
                span.label
            );
        }
        // The script's own 30.7 s at 25 ms a frame, the lead-ins' left out.
        let counted: usize = states.iter().map(|state| state.frames).sum();
        assert_eq!(counted, 1228);
        let of = |name: &str| {
            let found = states.iter().find(|state| state.state == name);
            found.unwrap_or_else(|| panic!("no state {name}"))
        };
        let (stand, walk, holster) = (of("stand"), of("walk"), of(HOLSTER));
        println!(
            "stand {} lit, walk {} lit (no lit blade: {}), holster {} lit with {} checks",
            stand.lit_frames,
            walk.lit_frames,
            walk.no_lit_blade,
            holster.lit_frames,
            holster.worst.len()
        );
        assert_eq!(stand.lit_frames, stand.frames);
        assert!(stand.worst.iter().all(|worst| worst.value == 0.0));
        assert!(walk.no_lit_blade && walk.lit_frames == 0 && walk.worst.is_empty());
        assert!(!walk.fallback);
        assert!(!holster.no_lit_blade && holster.lit_frames == 0 && !holster.worst.is_empty());

        // A broken measure's NaN is written to `cases.json`, read back as
        // NaN, and listed in `report.md` as failing.
        let mut states = states;
        states[0].worst[0].value = f32::NAN;
        let record = Record {
            case,
            name: "nan case".to_string(),
            args: Vec::new(),
            env: Default::default(),
            failed: None,
            states,
        };
        let (read, markdown) = written("nan", &[record], report::read);
        let value = read[0].states[0].worst[0].value;
        println!("NaN read back as {value}");
        assert!(value.is_nan());
        assert!(
            markdown.contains("| nan case | stand | grip | NaN |"),
            "{markdown}"
        );
    }

    /// The JA floor (`report::floor`). An MDA row fails only when its value
    /// is over its check's design limit and also over the JA-rules value of
    /// the same check, weapon, body and state plus 5% of the design limit.
    /// A JA-rules row, and an MDA row without a JA-rules value, are judged
    /// by the design limit alone. Grip's design limit is 0.04 m and
    /// ContactGap's is 0.10 m. `report.md` shows each failing row's limit,
    /// design limit and JA value.
    pub fn floor() {
        let record = |name: &str, rules, body, values: &[(Check, f32)]| Record {
            case: case(rules, body),
            name: name.to_string(),
            args: Vec::new(),
            env: Default::default(),
            failed: None,
            states: vec![StateRecord {
                state: "attack".to_string(),
                frames: 1,
                lit_frames: 1,
                no_lit_blade: false,
                fallback: false,
                worst: values
                    .iter()
                    .map(|&(check, value)| {
                        Worst {
                            check,
                            value,
                            ms: 0.0,
                        }
                        .into()
                    })
                    .collect(),
            }],
        };
        let (grip, gap) = (Check::Grip, Check::ContactGap);
        let mut records = vec![
            record("ja", Rules::Ja, Body::Male, &[(grip, 0.03), (gap, 0.40)]),
            record(
                "under",
                Rules::Mda,
                Body::Male,
                &[(grip, 0.04), (gap, 0.40)],
            ),
            record("over", Rules::Mda, Body::Male, &[(grip, 0.05), (gap, 0.41)]),
            record("no ja", Rules::Mda, Body::Female, &[(gap, 0.40)]),
        ];
        report::floor(&mut records);
        let mut failing: Vec<(String, String, f32)> = [Rules::Mda, Rules::Ja]
            .into_iter()
            .flat_map(|rules| report::failing(&records, rules))
            .map(|row| {
                (
                    row.record.name.clone(),
                    row.worst.check.clone(),
                    row.worst.limit,
                )
            })
            .collect();
        failing.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
        println!("failing rows with their limits: {failing:?}");
        let expected = [
            ("ja", "contact-gap", 0.10),
            ("no ja", "contact-gap", 0.10),
            ("over", "contact-gap", 0.405),
            ("over", "grip", 0.04),
        ];
        assert_eq!(failing.len(), expected.len(), "{failing:?}");
        for ((name, check, limit), (want_name, want_check, want_limit)) in
            failing.iter().zip(expected)
        {
            assert_eq!((name.as_str(), check.as_str()), (want_name, want_check));
            near(&format!("{name} {check} limit"), *limit, want_limit);
        }

        // The failing rows under MDA's rules come first in `report.md`, and
        // the rows under JA's rules have a section of their own after them.
        let under_mda: Vec<_> = report::failing(&records, Rules::Mda)
            .iter()
            .map(|row| row.record.name.clone())
            .collect();
        assert!(!under_mda.contains(&"ja".to_string()), "{under_mda:?}");
        let (_, markdown) = written("floor", &records, report::read);
        let at = |heading: &str| {
            markdown
                .find(heading)
                .unwrap_or_else(|| panic!("{heading}"))
        };
        let (mda_at, ja_at) = (
            at("## Failing rows under MDA's rules"),
            at("## Failing rows under JA's rules"),
        );
        assert!(mda_at < ja_at, "{markdown}");
        assert!(
            markdown[mda_at..ja_at].contains("| over | attack | contact-gap |")
                && !markdown[mda_at..ja_at].contains("| ja | attack |"),
            "{markdown}"
        );
        assert!(
            markdown[ja_at..].contains("| ja | attack | contact-gap | 0.400 |"),
            "{markdown}"
        );
        assert!(
            markdown.contains("| over | attack | contact-gap | 0.410 | 0.405 | 0.100 | 0.400 |"),
            "{markdown}"
        );
        assert!(
            markdown.contains("| no ja | attack | contact-gap | 0.400 | 0.100 | 0.100 |  | 0 |"),
            "{markdown}"
        );

        // An earlier run's file, written when Grip's limit was 0.03 m, holds
        // a failing Grip of 0.039 m. Read by today's limit of 0.04 m
        // (`report::earlier`), that Grip passed. A Grip of 0.0405 m today
        // fails, so it is reported as failing now, though it rose by less
        // than 5% of the limit.
        let mut old = vec![record("grip", Rules::Mda, Body::Female, &[(grip, 0.039)])];
        old[0].states[0].worst[0].limit = 0.03;
        let (old, _) = written("against", &old, report::earlier);
        near("earlier grip limit", old[0].states[0].worst[0].limit, 0.04);
        let mut new = vec![record("grip", Rules::Mda, Body::Female, &[(grip, 0.0405)])];
        report::floor(&mut new);
        let worse = report::worse(&old, &new);
        println!("worse against the earlier run: {worse:?}");
        assert!(
            worse.len() == 1 && worse[0].ends_with("fails now"),
            "{worse:?}"
        );

        // Under JA's rules a value counts as changed only when it moves by
        // more than 0.1% of its check's design limit. Grip's limit of 0.04 m
        // allows 0.04 mm of run-timing noise.
        let ja_grip = |value: f32| vec![record("ja grip", Rules::Ja, Body::Male, &[(grip, value)])];
        let (noise, moved) = (
            report::worse(&ja_grip(0.154), &ja_grip(0.15403)),
            report::worse(&ja_grip(0.154), &ja_grip(0.1541)),
        );
        println!("JA-rules grip 0.154 to 0.15403: {noise:?}; to 0.1541: {moved:?}");
        assert!(noise.is_empty(), "{noise:?}");
        assert!(
            moved.len() == 1 && moved[0].ends_with("changed under JA's rules"),
            "{moved:?}"
        );

        // A state that now falls back to JA's sequence, and did not before,
        // is worse. So is a check that the earlier run had and this one lacks.
        let swap = |fallback: bool, values: &[(Check, f32)]| {
            let mut records = vec![record("swap", Rules::Mda, Body::Male, values)];
            records[0].states[0].fallback = fallback;
            records
        };
        let (lost_swap, lost_check) = (
            report::worse(&swap(false, &[(grip, 0.01)]), &swap(true, &[(grip, 0.01)])),
            report::worse(
                &swap(false, &[(grip, 0.01), (gap, 0.0)]),
                &swap(false, &[(grip, 0.01)]),
            ),
        );
        println!("lost swap: {lost_swap:?}; lost check: {lost_check:?}");
        assert!(
            lost_swap.len() == 1 && lost_swap[0].ends_with("falls back to JA's sequence now"),
            "{lost_swap:?}"
        );
        assert!(
            lost_check.len() == 1 && lost_check[0].ends_with("contact-gap: no value now"),
            "{lost_check:?}"
        );
        let unchanged = report::worse(&swap(true, &[(grip, 0.01)]), &swap(true, &[(grip, 0.01)]));
        assert!(unchanged.is_empty(), "{unchanged:?}");

        // The swing-start, stance-entry and cut-start checks are floored as
        // ContactGap is, by the higher of the design limit and the JA value
        // plus 5% of the design limit. The design limits are 25 degrees and
        // 0.35 m over the Movie Duels reference, and 0.45 m for CutStartGap.
        // A JA value of 0 leaves the design limit.
        for (check, mda, ja, limit, fails) in [
            (Check::SwingStartTurn, 30.0, Some(10.0), 25.0, true),
            (Check::SwingStartTurn, 24.0, None, 25.0, false),
            (Check::SwingStartTurn, 30.0, Some(28.0), 29.25, true),
            (Check::SwingStartTurn, 29.0, Some(28.0), 29.25, false),
            (Check::SwingStartTravel, 0.4, Some(0.0), 0.35, true),
            (Check::StanceEntryTurn, 26.0, Some(25.0), 26.25, false),
            (Check::StanceEntryTravel, 0.36, None, 0.35, true),
            (Check::CutStartGap, 0.31, Some(0.30), 0.45, false),
            (Check::CutStartGap, 0.47, Some(0.46), 0.4825, false),
            (Check::CutStartGap, 0.5, Some(0.46), 0.4825, true),
        ] {
            let what = format!("{} {mda} against {ja:?}", check.name());
            let mut records = vec![record("mda", Rules::Mda, Body::Male, &[(check, mda)])];
            if let Some(ja) = ja {
                records.push(record("ja", Rules::Ja, Body::Male, &[(check, ja)]));
            }
            report::floor(&mut records);
            near(&what, records[0].states[0].worst[0].limit, limit);
            let failing = report::failing(&records, Rules::Mda).len();
            assert_eq!(failing, usize::from(fails), "{what}");
        }
    }

    /// The fixed-window checks and CutStartGap. `steps` are the turn
    /// (degrees) and the tip's travel (metres) of the blade into each frame
    /// after the first. The blade is 1 m long, and its tip moves along x.
    /// `tweak` changes frame `i` as the case needs.
    fn swinging(steps: &[(f32, f32)], tweak: impl Fn(usize, &mut Frame)) -> Vec<Frame> {
        let steps_into = |i: usize| steps[..i].iter();
        frames(steps.len() + 1, |i| {
            let (angle, x) = steps_into(i)
                .fold((0.0_f32, 0.0_f32), |(angle, x), &(turn, travel)| {
                    (angle + turn, x + travel)
                });
            let (sin, cos) = angle.to_radians().sin_cos();
            let tip = [x, 0.0, 1.0];
            let hilt = [x - cos, -sin, 1.0];
            let mut frame = frame(i, false, vec![blade(hilt, tip, AHEAD, 0.0)]);
            tweak(i, &mut frame);
            frame
        })
    }

    /// The steps of a swing or a stance entry whose first frame is frame 1.
    /// The step into frame 1 is large, to show it is not counted. Then come 4
    /// steps of `window`, the turn and the travel inside the window, and 2
    /// large steps.
    fn steps_after_start(window: (f32, f32)) -> Vec<(f32, f32)> {
        let large = (90.0, 2.0);
        [vec![large], vec![window; 4], vec![large; 2]].concat()
    }

    /// The checks of the start of a swing, of the stance entry and of a cut.
    pub fn swings() {
        let swing = |before: &'static str, after: &'static str| {
            let steps = steps_after_start((30.0, 0.5));
            swinging(&steps, move |i, frame| {
                frame.sim[1] = (if i == 0 { before } else { after }).to_string();
            })
        };
        for (before, after, begins) in [
            ("BOTH_STAND2", "BOTH_S2_S1_T_", true),
            ("BOTH_STAND2", "BOTH_A2_T__BR_", true),
            ("BOTH_R2_T__S1", "BOTH_S2_S1_T_", true),
            ("BOTH_T2_T__R_", "BOTH_A2_R__L_", false),
        ] {
            let frames = swing(before, after);
            let (turn, travel) = if begins { (120.0, 2.0) } else { (0.0, 0.0) };
            let what = format!("{before} to {after}");
            near(
                &format!("swing-start-turn, {what}"),
                worst(&frames, Check::SwingStartTurn),
                turn,
            );
            near(
                &format!("swing-start-travel, {what}"),
                worst(&frames, Check::SwingStartTravel),
                travel,
            );
        }

        // The blade growing is not measured. A swing that begins 50 ms
        // before the last frame sums the 2 steps it has.
        let mut growing = swing("BOTH_STAND2", "BOTH_S2_S1_T_");
        growing
            .iter_mut()
            .for_each(|frame| frame.blades[0].growing = true);
        let short = swing("BOTH_STAND2", "BOTH_S2_S1_T_");
        for (what, frames, turn, travel) in [
            ("growing", &growing[..], 0.0, 0.0),
            ("cut short", &short[..4], 60.0, 1.0),
        ] {
            let (found_turn, found_travel) = (
                worst(frames, Check::SwingStartTurn),
                worst(frames, Check::SwingStartTravel),
            );
            near(&format!("swing-start-turn, {what}"), found_turn, turn);
            near(&format!("swing-start-travel, {what}"), found_travel, travel);
        }

        // The checks count only what the drawn blade adds over the Movie
        // Duels reference. Its blade turns about its tip, so the tip does
        // not move. A reference that turns 20 degrees in each step leaves
        // 40 of the drawn 120 degrees. One that turns 40 degrees in each
        // step is faster than the drawn blade, which then adds nothing.
        for (md_step, turn) in [(20.0, 40.0), (40.0, 0.0)] {
            let md = swinging(&steps_after_start((md_step, 0.0)), |_, _| {});
            let mut frames = swing("BOTH_STAND2", "BOTH_S2_S1_T_");
            for (frame, md) in frames.iter_mut().zip(&md) {
                frame.blades[0].md = Some(md.blades[0].drawn);
            }
            let what = format!("against a reference turning {md_step} degrees a step");
            near(
                &format!("swing-start-turn, {what}"),
                worst(&frames, Check::SwingStartTurn),
                turn,
            );
            near(
                &format!("swing-start-travel, {what}"),
                worst(&frames, Check::SwingStartTravel),
                2.0,
            );
        }

        // A swing that begins on the state's first frame is counted when the
        // frame before the state is given, and not otherwise.
        let whole = swing("BOTH_STAND2", "BOTH_S2_S1_T_");
        for (lead, turn) in [(Some(&whole[0]), 120.0), (None, 0.0)] {
            let worst = measure(
                (lead, &whole[1..]),
                &checks::holds(&whole),
                true,
                Still::Hands,
            );
            let found = worst.iter().find(|w| w.check == Check::SwingStartTurn);
            let what = format!(
                "swing-start-turn from the state's first frame, lead {}",
                lead.is_some()
            );
            near(&what, found.expect("swing-start-turn").value, turn);
        }

        // The drawn body starts to play another sequence than the sim's at
        // frame 1, and its blade turns 20 degrees and moves 0.25 m in each of
        // the next 4 steps.
        let swapped = swinging(&steps_after_start((20.0, 0.25)), |i, frame| {
            frame.swapped = i >= 1;
        });
        near(
            "stance-entry-turn",
            worst(&swapped, Check::StanceEntryTurn),
            80.0,
        );
        near(
            "stance-entry-travel",
            worst(&swapped, Check::StanceEntryTravel),
            1.0,
        );

        // A move starts to cut at frame 2, with the drawn blade 0.2 m from
        // the sim's, and the blades are 0.5 m apart at frame 4.
        let cutting = frames(6, |i| {
            let gap = if i == 4 { 0.5 } else { 0.2 };
            let mut frame = frame(i, i >= 2, vec![blade(AHEAD[0], AHEAD[1], AHEAD, gap)]);
            frame.cuts = i >= 2;
            frame
        });
        near("cut-start-gap", worst(&cutting, Check::CutStartGap), 0.2);
        near(
            "contact-gap, same frames",
            worst(&cutting, Check::ContactGap),
            0.5,
        );
    }

    /// Two runs of one case that draw the same stepped frames measure the
    /// same. The runs begin differently: one draws free frames at 6.8 ms
    /// and 16 ms and its first stepped frame at 25 ms, the other free
    /// frames at 48 ms and 56 ms and its first stepped frame at 75 ms. The
    /// second run draws each stepped moment three times, and every fourth
    /// one 1 ms early. Only the frames on the 25 ms grid are measured, one
    /// for each stepped moment, at that moment. The left hand drifts 2 mm
    /// every 25 ms, so a frame measured at another moment changes the
    /// speeds.
    pub fn stepped() {
        let drifting = |i: usize| moved(held(i), "lhand_g", [0.002 * i as f32, 0.0, 0.0]);
        let at = |frame: Frame, ms: f64| Frame { ms, ..frame };
        let free = |moments: [f64; 2]| moments.map(|ms| at(drifting(0), ms));
        let calm: Vec<Frame> = free([6.8, 16.0])
            .into_iter()
            .chain((1..1400).map(drifting))
            .collect();
        let loaded: Vec<Frame> = free([48.0, 56.0])
            .into_iter()
            .chain((3..1400).flat_map(|i| {
                let early = if i % 4 == 1 { 1.0 } else { 0.0 };
                let frame = at(drifting(i), i as f64 * 25.0 - early);
                [frame.clone(), frame.clone(), frame]
            }))
            .collect();
        let case = case(Rules::Ja, Body::Male);
        let (calm_states, loaded_states) = (case.measured(calm), case.measured(loaded.clone()));
        for (calm, loaded) in calm_states.iter().zip(&loaded_states) {
            assert_eq!(format!("{calm:?}"), format!("{loaded:?}"), "{}", calm.state);
        }
        let kept = kjk::anim_audit::stepped(loaded.clone());
        println!(
            "the loaded run keeps {} of {} frames, from {} ms",
            kept.len(),
            loaded.len(),
            kept[0].ms
        );
        assert_eq!(kept.len(), 1397);
        assert!(kept.iter().all(|frame| frame.ms % 25.0 == 0.0));
        assert_eq!(kept[0].ms, 75.0);
    }

    /// The body every frame stands in, by its KOTOR node, its JA bone and
    /// its place. The reference stands where the drawn body does (scale 1).
    const BODY: [(&str, &str, [f32; 3]); 15] = [
        ("pelvis_g", "pelvis", [0.0, 0.0, 1.0]),
        ("torsoupr_g", "thoracic", [0.0, 0.0, 1.5]),
        ("lthigh_g", "lfemurYZ", [-0.1, 0.0, 0.95]),
        ("rthigh_g", "rfemurYZ", [0.1, 0.0, 0.95]),
        ("lshin_g", "ltibia", [-0.1, 0.0, 0.5]),
        ("rshin_g", "rtibia", [0.1, 0.0, 0.5]),
        ("lfoot_g", "ltalus", [-0.1, 0.0, 0.1]),
        ("rfoot_g", "rtalus", [0.1, 0.0, 0.1]),
        ("lbicep_g", "lhumerus", LEFT_ARM[0]),
        ("lforearm_g", "lradius", LEFT_ARM[1]),
        ("lhand_g", "lhand", LEFT_ARM[2]),
        ("rbicep_g", "rhumerus", RIGHT_ARM[0]),
        ("rforearm_g", "rradius", RIGHT_ARM[1]),
        ("rhand_g", "rhand", HAND),
        // Each head is centred 0.12 m above its bone, `headhook` or
        // `cranium`, which sit at the same height.
        ("headhook", "cranium", [0.0, 0.0, 1.6]),
    ];
    const HAND: [f32; 3] = [0.3, 0.3, 1.0];
    /// Each arm's shoulder, elbow and wrist.
    const LEFT_ARM: [[f32; 3]; 3] = [[-0.2, 0.0, 1.45], [-0.25, 0.15, 1.2], [-0.3, 0.3, 1.0]];
    const RIGHT_ARM: [[f32; 3]; 3] = [[0.2, 0.0, 1.45], [0.25, 0.15, 1.2], HAND];

    /// A blade held out ahead, clear of the body, its hilt in the hand.
    const AHEAD: [[f32; 3]; 2] = [HAND, [0.3, 1.3, 1.0]];

    /// Frame `i` of the standing body, holding the blade out ahead.
    fn held(i: usize) -> Frame {
        frame(i, false, vec![blade(AHEAD[0], AHEAD[1], AHEAD, 0.0)])
    }

    /// The case of the stances off, with a single blade, of `body` under
    /// `rules`.
    fn case(rules: Rules, body: Body) -> Case {
        Case {
            stance: "ja".to_string(),
            weapon: Weapon::Single,
            body,
            rules,
        }
    }

    /// `records` written as `cases.json` and `report.md` into a scratch
    /// folder named by `tag`, then read back by `read`, with the report's
    /// text. The folder is removed.
    fn written(
        tag: &str,
        records: &[Record],
        read: fn(&std::path::Path) -> Result<Vec<Record>, String>,
    ) -> (Vec<Record>, String) {
        let out = std::env::temp_dir().join(format!("kjk-audit-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&out).expect("a scratch folder");
        report::write(&out, records).expect("the report writes");
        let read = read(&out.join(report::CASES)).expect("cases.json reads back");
        let markdown = std::fs::read_to_string(out.join(report::REPORT)).expect("report.md");
        let _ = std::fs::remove_dir_all(&out);
        (read, markdown)
    }

    /// `n` frames, 25 ms apart, each made by `make` from its index.
    fn frames(n: usize, make: impl Fn(usize) -> Frame) -> Vec<Frame> {
        (0..n).map(make).collect()
    }

    /// Frame `i` of the standing body, holding `blades` in its right hand.
    fn frame(i: usize, contact: bool, blades: Vec<Blade>) -> Frame {
        Frame {
            ms: i as f64 * 25.0,
            contact,
            nodes: BODY
                .iter()
                .map(|(node, _, at)| (node.to_string(), *at))
                .collect(),
            ref_nodes: BODY
                .iter()
                .map(|(_, bone, at)| (bone.to_string(), *at))
                .collect(),
            blades,
            ..Frame::default()
        }
    }

    /// A blade drawn from `hilt` to `tip`, JA's from `reference`, and the
    /// sim's `gap` away from the drawn one along x.
    fn blade(hilt: [f32; 3], tip: [f32; 3], reference: [[f32; 3]; 2], gap: f32) -> Blade {
        let shift = |p: [f32; 3]| [p[0] + gap, p[1], p[2]];
        Blade {
            drawn: [hilt, tip],
            reference,
            sim: [shift(hilt), shift(tip)],
            hand: HAND,
            ..Blade::default()
        }
    }

    /// Frame `frame` with its drawn `node` moved `by`.
    fn moved(mut frame: Frame, node: &str, by: [f32; 3]) -> Frame {
        let at = frame.nodes.get_mut(node).expect("a node of the body");
        *at = [at[0] + by[0], at[1] + by[1], at[2] + by[2]];
        frame
    }

    /// The worst of `check` over `frames`, every check applying and the
    /// hands judged against the reference's.
    fn worst(frames: &[Frame], check: Check) -> f32 {
        worst_judged(frames, check, Still::Hands)
    }

    /// The worst of `check` over `frames`, every check applying and Stillness
    /// judged as `still`.
    fn worst_judged(frames: &[Frame], check: Check, still: Still) -> f32 {
        let worst = measure((None, frames), &checks::holds(frames), true, still);
        let found = worst.iter().find(|w| w.check == check);
        found.unwrap_or_else(|| panic!("no {}", check.name())).value
    }

    fn near(what: &str, measured: f32, expected: f32) {
        println!("{what}: {measured:.4} (expected {expected})");
        let tolerance = 0.001 * expected.abs().max(1.0);
        assert!(
            (measured - expected).abs() <= tolerance,
            "{what}: {measured}"
        );
    }

    /// The reference's scale (`checks::scale`) is the drawn arms' length
    /// over the reference's, each arm from the upper arm through the
    /// forearm to the hand. The drawn torso's length plays no part, as
    /// KOTOR's `pelvis_g` sits about 0.3 m above JA's pelvis bone. Drawn
    /// arms half as long again give 1.5. A body without arms gives 1.
    pub fn scale() {
        near("scale, same body", checks::scale(&frames(4, held)), 1.0);
        let high_pelvis = frames(4, |i| moved(held(i), "pelvis_g", [0.0, 0.0, 0.35]));
        near(
            "scale, higher drawn pelvis",
            checks::scale(&high_pelvis),
            1.0,
        );
        let longer = frames(4, |i| {
            let mut frame = held(i);
            for (side, [shoulder, elbow, wrist]) in [("l", LEFT_ARM), ("r", RIGHT_ARM)] {
                let half = |to: [f32; 3]| std::array::from_fn(|k| (to[k] - shoulder[k]) / 2.0);
                frame = moved(frame, &format!("{side}forearm_g"), half(elbow));
                frame = moved(frame, &format!("{side}hand_g"), half(wrist));
            }
            frame
        });
        near(
            "scale, drawn arms 1.5 times as long",
            checks::scale(&longer),
            1.5,
        );
        let armless = frames(4, |i| {
            let mut frame = held(i);
            frame.nodes.retain(|node, _| !node.contains("bicep"));
            frame
        });
        near("scale, no arms", checks::scale(&armless), 1.0);
    }

    pub fn checks() {
        // Grip counts how much farther the drawn hilt is from the hand than
        // in the case's steadiest hold. The hilt held 0.10 m from the hand in
        // every frame passes, wherever the reference holds its own.
        let along_x = |x: f32| {
            [
                [HAND[0] + x, HAND[1], HAND[2]],
                [HAND[0] + x, HAND[1] + 1.0, HAND[2]],
            ]
        };
        let gripped = |x: &dyn Fn(usize) -> f32| {
            frames(4, |i| {
                let [hilt, tip] = along_x(x(i));
                frame(i, false, vec![blade(hilt, tip, AHEAD, 0.0)])
            })
        };
        near(
            "grip, held steady",
            worst(&gripped(&|_| 0.10), Check::Grip),
            0.0,
        );
        // The hilt slips to 0.15 m from the hand in one frame. That is 0.05 m
        // farther than the steadiest hold.
        let slipping = gripped(&|i| if i == 2 { 0.15 } else { 0.10 });
        near("grip, slipping", worst(&slipping, Check::Grip), 0.05);
        // The steadiest hold is the whole case's. A state that holds the hilt
        // 0.10 m off, in a case that held it 0.08 m off elsewhere, counts
        // 0.02 m.
        let case = gripped(&|i| if i == 0 { 0.08 } else { 0.10 });
        let state = &case[1..];
        let in_state = measure(
            (case.first(), state),
            &checks::holds(&case),
            true,
            Still::Hands,
        );
        let grip = in_state
            .iter()
            .find(|w| w.check == Check::Grip)
            .expect("grip");
        near("grip, steadiest hold of the case", grip.value, 0.02);

        // The blade runs through the torso's capsule, which has a radius of
        // 0.16 m about x = y = 0. It is 0.11 m from the axis, so it is 0.05 m
        // deep.
        let deep = [[-0.6, 0.11, 1.25], [0.6, 0.11, 1.25]];
        let cut = |reference| {
            frames(4, |i| {
                frame(i, false, vec![blade(deep[0], deep[1], reference, 0.0)])
            })
        };
        near(
            "self-cut, reference clear",
            worst(&cut(AHEAD), Check::SelfCut),
            0.05,
        );
        near(
            "self-cut, reference as deep",
            worst(&cut(deep), Check::SelfCut),
            0.0,
        );
        // Each head (radius 0.11) is centred 0.12 m above its own bone. The
        // drawn blade runs 0.05 m above the drawn head's centre (z = 1.72),
        // 0.06 m deep. The reference's `cranium` is raised by `raised`, and
        // its blade runs at `height`. The self-cut is what the drawn blade
        // adds over the reference's, so it follows each side's own bone.
        let across = |z: f32| [[-0.6, 0.0, z], [0.6, 0.0, z]];
        let heads = [
            ("bones level, blades level", 0.0, 1.77, 0.0),
            ("cranium 0.2 m higher, its blade as high", 0.2, 1.97, 0.0),
            (
                "cranium 0.2 m higher, its blade at the drawn one's",
                0.2,
                1.77,
                0.06,
            ),
        ];
        for (what, raised, height, expected) in heads {
            let through_head = frames(4, |i| {
                let [hilt, tip] = across(1.77);
                let mut frame = frame(i, false, vec![blade(hilt, tip, across(height), 0.0)]);
                let cranium = frame.ref_nodes.get_mut("cranium").expect("a cranium");
                cranium[2] += raised;
                frame
            });
            near(
                &format!("self-cut, {what}"),
                worst(&through_head, Check::SelfCut),
                expected,
            );
        }

        // A full turn in 0.25 s, ten steps of 36°, the reference still.
        let spun = frames(11, |i| {
            let angle = (i as f32 * 36.0).to_radians();
            let tip = [HAND[0] + angle.cos(), HAND[1] + angle.sin(), HAND[2]];
            frame(i, false, vec![blade(HAND, tip, AHEAD, 0.0)])
        });
        let spin = worst(&spun, Check::Spin);
        println!("spin: {spin:.0} °/s");
        assert!(spin >= 1400.0, "spin {spin}");

        let growing = frames(4, |i| {
            let far = [HAND[0] + 1.0, HAND[1], HAND[2]];
            let blade = Blade {
                growing: true,
                ..blade(far, [far[0], far[1] + 1.0, far[2]], AHEAD, 0.0)
            };
            frame(i, false, vec![blade])
        });
        near("growing grip", worst(&growing, Check::Grip), 0.0);

        let gapped = |contact| {
            frames(4, |i| {
                frame(i, contact, vec![blade(AHEAD[0], AHEAD[1], AHEAD, 0.2)])
            })
        };
        near(
            "gap outside contact",
            worst(&gapped(false), Check::ContactGap),
            0.0,
        );
        near(
            "gap in contact",
            worst(&gapped(true), Check::ContactGap),
            0.2,
        );
        // A staff along x, 0.2 m from the off hand at (-0.3, 0.3, 1.0),
        // while the reference's passes through that hand.
        let staff = frames(4, |i| {
            let reference = [[0.3, 0.3, 1.0], [-1.3, 0.3, 1.0]];
            frame(
                i,
                false,
                vec![blade([0.3, 0.5, 1.0], [-1.3, 0.5, 1.0], reference, 0.0)],
            )
        });
        near("two hands", worst(&staff, Check::TwoHands), 0.2);

        // The right hand jolted 0.25 m for one frame of 25 ms: 2 × 0.25 m
        // / (0.025 s)² at that frame.
        let jolted = frames(5, |i| {
            let by = if i == 2 { 0.25 } else { 0.0 };
            moved(held(i), "rhand_g", [by, 0.0, 0.0])
        });
        near("pops", worst(&jolted, Check::Pops), 800.0);

        // The left hand rushes 0.2 m every 25 ms (8 m/s) through the state's
        // first 0.3 s, as a pose's entry does, and then moves 0.01 m every
        // 25 ms. The reference's hand is still. Stillness counts only the
        // 0.4 m/s after the first 0.3 s.
        let drifting = frames(20, |i| {
            let by = 0.2 * i.min(12) as f32 + 0.01 * i.saturating_sub(12) as f32;
            moved(held(i), "lhand_g", [by, 0.0, 0.0])
        });
        near("stillness", worst(&drifting, Check::Stillness), 0.4);

        // A held block's gripping hand is judged in the drawn chest's own
        // frame, its place and its turn. In each case the body walks 0.2 m
        // every 25 ms (8 m/s). The chest turns about the upright by the
        // case's angle every 25 ms, and the hand turns with it. The hand
        // also slips by the case's distance every 25 ms against the chest,
        // from the first moment. Only the slip counts. A turn of 0.025 rad
        // every 25 ms is 1 rad/s, and the hand is 0.42 m out from the
        // chest's upright, so a measure in world space would read 0.42 m/s.
        // The reference's hand is still, and it is not used.
        for (name, turn, slip, expected) in [
            ("walks and slips", 0.0, 0.01, 0.4),
            ("walks", 0.0, 0.0, 0.0),
            ("walks and turns", 0.025, 0.0, 0.0),
            ("walks, turns and slips", 0.025, 0.01, 0.4),
        ] {
            let walking = frames(20, |i| {
                let mut frame = moved(held(i), "torsoupr_g", [0.2 * i as f32, 0.0, 0.0]);
                let chest = frame.nodes["torsoupr_g"];
                let angle = turn * i as f32;
                let (sin, cos) = angle.sin_cos();
                let half = (angle / 2.0).sin_cos();
                // The hand's place against the chest before the turn.
                let out: [f32; 3] = std::array::from_fn(|k| {
                    HAND[k] - BODY[1].2[k] + [slip * i as f32, 0.0, 0.0][k]
                });
                let hand = [
                    chest[0] + cos * out[0] - sin * out[1],
                    chest[1] + sin * out[0] + cos * out[1],
                    chest[2] + out[2],
                ];
                frame.chest = Some([0.0, 0.0, half.0, half.1]);
                for blade in &mut frame.blades {
                    blade.hand = hand;
                }
                frame
            });
            near(
                &format!("stillness, held block that {name}"),
                worst_judged(&walking, Check::Stillness, Still::Chest),
                expected,
            );
        }

        // Two frames at the same moment, then a blade of no length, with
        // the blade spinning and the left hand moving. The checks keep a NaN
        // as their worst, so no check may give one here.
        let mut repeated: Vec<Frame> = spun
            .iter()
            .zip(&drifting)
            .map(|(spun, drifting)| Frame {
                nodes: drifting.nodes.clone(),
                ..spun.clone()
            })
            .collect();
        repeated[3].ms = repeated[2].ms;
        repeated[4].blades = vec![blade(HAND, HAND, [HAND, HAND], 0.0)];
        for w in measure(
            (None, &repeated),
            &checks::holds(&repeated),
            true,
            Still::Hands,
        ) {
            println!("repeated moment {}: {}", w.check.name(), w.value);
            assert!(w.value.is_finite(), "{}: {}", w.check.name(), w.value);
        }

        // The held blade alone is clean on every check.
        let steady = frames(4, held);
        for w in measure((None, &steady), &checks::holds(&steady), true, Still::Hands) {
            near(w.check.name(), w.value, 0.0);
        }
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
        let index = vfs::DirIndex::of_install(install).expect("the install indexes");
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

/// A 2DA's stored cell-data size is not read, as in the games and reone:
/// a mod's table storing 0 there (High Quality Blasters' baseitems.2da) or
/// any other wrong size reads as written. No installs (approved
/// 2026-11-05).
#[test]
fn mod_2da_size_ignored() {
    use fmt_odyssey::twoda::TwoDa;

    let row = |cells: [&str; 2]| cells.map(|cell| cell.as_bytes().to_vec()).to_vec();
    let table = TwoDa {
        columns: vec![b"label".to_vec(), b"defaultmodel".to_vec()],
        row_labels: vec![b"0".to_vec(), b"1".to_vec()],
        cells: vec![
            row(["Blaster_Pistol", "w_BlstrPstl_001"]),
            row(["Bowcaster", "w_BowCstr_001"]),
        ],
        column_separator: b'\t',
        row_separator: b'\t',
    };
    let written = table.write();
    let data_len = table
        .cells
        .iter()
        .flatten()
        .map(|cell| cell.len() + 1)
        .sum::<usize>();
    let size_at = written.len() - data_len - 2;
    for stored in [0u16, 1, u16::MAX] {
        let mut bytes = written.clone();
        bytes[size_at..size_at + 2].copy_from_slice(&stored.to_le_bytes());
        let read = TwoDa::parse(&bytes).unwrap_or_else(|e| panic!("size {stored}: {e}"));
        assert_eq!(read, table, "size {stored}");
    }
    let past_end = written.len() - 1;
    assert!(
        TwoDa::parse(&written[..past_end]).is_err(),
        "a cell cut short"
    );
}

/// An area's music waits the given delay after the area is entered (the
/// game passes `world.area_music.wait_ms`, issue 291), plays its track
/// through, waits again, and plays anew. The last area's track does not
/// carry over, and a script's start plays it at once. No installs
/// (approved 2026-11-11).
#[test]
fn area_music_waits_its_delay() {
    use asset_odyssey::sound::Song;

    const TRACK: &str = "area/mus_area_ahto";
    const DELAY: f64 = 120_000.0;
    // Each moment: the time (ms), whether the track is still heard, whether a
    // script started the music, and whether the track plays then.
    let entered: &[(f64, bool, bool, bool)] = &[
        // The last area's track is still heard as this one is entered.
        (0.0, true, false, false),
        (60_000.0, false, false, false),
        (119_999.0, false, false, false),
        (120_000.0, false, false, true),
        (150_000.0, true, false, true),
        // The track ends: the next waits the delay from then.
        (210_000.0, false, false, false),
        (329_999.0, false, false, false),
        (330_000.0, false, false, true),
        (400_000.0, false, false, false),
        // A script's MusicBackgroundPlay in the wait plays it at once.
        (410_000.0, false, true, true),
    ];
    let mut song = Some(Song::Entered);
    for &(now, heard, started, plays) in entered {
        let (turn, played) = Song::turn(song.take(), TRACK, (heard, started), now, DELAY);
        assert_eq!(played, plays, "at {now} ms: {turn:?}");
        song = Some(turn);
    }
    // An area entered with a script's start already made plays at once, and
    // a new track (a script's MusicBackgroundChangeDay) replaces the old.
    let (_, played) = Song::turn(Some(Song::Entered), TRACK, (false, true), 0.0, DELAY);
    assert!(played, "a script's start on entering");
    let playing = Some(Song::Playing("area/mus_area_valsith".into()));
    let (turn, played) = Song::turn(playing, TRACK, (true, false), 0.0, DELAY);
    assert!(played, "a new track");
    assert_eq!(turn, Song::Playing(TRACK.into()));
}

/// A weapon's texture with alpha and no TXI is drawn solid
/// (`kjk_game::material::weapon_surface`, `weapons_in_hand.look.solid`; a
/// kjk choice, issue 171: High Quality Blasters' guns showed the world
/// through them). A mod's 32-bit TGA takes its facts from a loose TXI
/// alone; with none its alpha blends on anything but a weapon, and a TXI
/// naming an env map or additive blending (a saber's blade) keeps its own
/// look on a weapon too. Needs `--features rig` (the game's library); no
/// installs (proposed 2026-11-07, not yet approved).
#[test]
fn weapon_alpha_without_txi_solid() {
    #[cfg(not(feature = "rig"))]
    println!("weapon_alpha_without_txi_solid skipped: run with --features rig");
    #[cfg(feature = "rig")]
    {
        use asset_odyssey::texture::TextureIr;
        use fmt_odyssey::texture::Texture;
        use fmt_odyssey::txi::Txi;
        use kjk_game::material::{Surface, is_weapon_model, surface, weapon_surface};

        // A 2×2 uncompressed 32-bit TGA, alpha 8 bits, its texels' alpha low.
        let mut tga = vec![0u8, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 2, 0, 32, 8];
        for _ in 0..4 {
            tga.extend_from_slice(&[40, 40, 40, 128]);
        }
        let texture = Texture::parse_tga(&tga).expect("the TGA parses");
        for (txi, model, expected) in [
            (None, "w_hvyblstr_001", Surface::Opaque),
            (None, "W_BlstrRfl_001", Surface::Opaque),
            (None, "p_bastilabb", Surface::OpaqueAlpha),
            (None, "plc_chair1", Surface::OpaqueAlpha),
            (
                Some("envmaptexture CM_Baremetal"),
                "w_hvyblstr_001",
                Surface::OpaqueEnvMasked,
            ),
            (
                Some("blending additive"),
                "w_lghtsbr_001",
                Surface::Additive,
            ),
            (Some("decal 1"), "w_hvyblstr_001", Surface::Blend),
        ] {
            let txi = txi.map(|line| Txi::parse(format!("{line}\r\n").as_bytes()));
            let facts = TextureIr::from_texture(&texture, txi.as_ref())
                .expect("the TGA decodes")
                .facts;
            let drawn = weapon_surface(
                surface((0, false), 1.0, (Some(&facts), &[&facts])),
                &[&facts],
                is_weapon_model(model),
            );
            assert_eq!(drawn, expected, "{model} with TXI {txi:?}");
        }
    }
}

/// Every TGA layout a mod's texture may come in reads, with its rows bottom
/// to top as a TPC's (`fmt_odyssey::texture::Texture::parse_tga_layers`):
/// true colour at 32, 24 and 16 bits, raw or run-length encoded, grey with
/// and without alpha, colour-mapped, an unused colour map skipped, from any
/// corner; a TXI's `cube` stacks six faces in it, and a TXI flip-book's
/// sheet is cut into its frames (`parse_tga_frames`). A texture that did not
/// read was drawn white (issue 276). No installs (proposed 2026-11-10, the
/// flip-book sheet 2026-11-12; approved 2026-11-12).
#[test]
fn tga_layouts_read() {
    use fmt_odyssey::texture::Texture;

    // How a layout reads the image's colours (grey, or without alpha).
    type Reads = fn([u8; 4]) -> [u8; 4];
    // A 2×2 image, top row red and green, bottom row blue and clear white,
    // as RGBA top to bottom.
    const IMAGE: [[u8; 4]; 4] = [
        [255, 0, 0, 255],
        [0, 255, 0, 255],
        [0, 0, 255, 255],
        [255, 255, 255, 0],
    ];
    let header = |kind: u8, depth: u8, descriptor: u8, map: &[u8], map_depth: u8| {
        let entries = (map.len() / usize::from(map_depth).div_ceil(8).max(1)) as u8;
        let mut tga = vec![
            0,
            u8::from(!map.is_empty()),
            kind,
            0,
            0,
            entries,
            0,
            map_depth,
        ];
        tga.extend_from_slice(&[0, 0, 0, 0, 2, 0, 2, 0, depth, descriptor]);
        tga.extend_from_slice(map);
        tga
    };
    // The image's texels in a file's order: bottom row first unless the
    // file starts at the top; each row right to left when it starts there.
    let texels = |descriptor: u8| -> Vec<[u8; 4]> {
        let rows = if descriptor & 0x20 != 0 {
            [0, 1]
        } else {
            [1, 0]
        };
        rows.iter()
            .flat_map(|row| {
                let mut row = IMAGE[row * 2..row * 2 + 2].to_vec();
                if descriptor & 0x10 != 0 {
                    row.reverse();
                }
                row
            })
            .collect()
    };
    let bgra = |[r, g, b, a]: [u8; 4]| vec![b, g, r, a];
    let bgr = |[r, g, b, _]: [u8; 4]| vec![b, g, r];
    let argb1555 = |[r, g, b, a]: [u8; 4]| {
        let bits = (u16::from(a > 127) << 15)
            | (u16::from(r >> 3) << 10)
            | (u16::from(g >> 3) << 5)
            | u16::from(b >> 3);
        bits.to_le_bytes().to_vec()
    };
    let opaque = |rgba: [u8; 4]| [rgba[0], rgba[1], rgba[2], 255];
    let grey = |[r, ..]: [u8; 4]| [r, r, r, 255];
    let file = |kind: u8, depth: u8, descriptor: u8, texel: &dyn Fn([u8; 4]) -> Vec<u8>| {
        let mut tga = header(kind, depth, descriptor, &[], 0);
        tga.extend(texels(descriptor).into_iter().flat_map(texel));
        tga
    };
    // Run-length encoded: one raw packet of all four texels.
    let rle = |depth: u8, texel: &dyn Fn([u8; 4]) -> Vec<u8>| {
        let mut tga = header(10, depth, 8, &[], 0);
        tga.push(3);
        tga.extend(texels(8).into_iter().flat_map(texel));
        tga
    };
    // Colour-mapped: a palette of the four colours, indexed in order.
    let indexed = |kind: u8| {
        let palette: Vec<u8> = IMAGE.into_iter().flat_map(bgra).collect();
        let mut tga = header(kind, 8, 8, &palette, 32);
        let index = |texel: [u8; 4]| IMAGE.iter().position(|c| *c == texel).unwrap() as u8;
        tga.extend(texels(8).into_iter().map(index));
        tga
    };
    let mut unused_map = header(2, 24, 0, &[1, 2, 3, 4, 5, 6], 24);
    unused_map.extend(texels(0).into_iter().flat_map(bgr));
    let cases: [(&str, Vec<u8>, Reads); 11] = [
        ("32-bit", file(2, 32, 8, &bgra), |c| c),
        ("32-bit, from the top", file(2, 32, 0x28, &bgra), |c| c),
        ("32-bit, right to left", file(2, 32, 0x18, &bgra), |c| c),
        ("24-bit", file(2, 24, 0, &bgr), opaque),
        ("24-bit, run-length", rle(24, &bgr), opaque),
        ("32-bit, run-length", rle(32, &bgra), |c| c),
        ("16-bit with alpha", file(2, 16, 1, &argb1555), |c| c),
        ("8-bit grey", file(3, 8, 0, &|[r, ..]| vec![r]), grey),
        (
            "grey with alpha",
            file(3, 16, 8, &|[r, _, _, a]| vec![r, a]),
            |c| [c[0], c[0], c[0], c[3]],
        ),
        ("colour-mapped", indexed(1), |c| c),
        ("an unused colour map", unused_map, opaque),
    ];
    for (layout, tga, expected) in cases {
        let texture = Texture::parse_tga(&tga).unwrap_or_else(|error| panic!("{layout}: {error}"));
        let read: Vec<[u8; 4]> =
            (texture.rgba(0, 0).expect("it decodes").as_chunks::<4>().0).to_vec();
        assert_eq!(read, IMAGE.map(expected), "{layout}");
    }
    // A cube map: six 2×2 faces in one 2×12 image, kept in stored order.
    let mut cube = header(2, 24, 0, &[], 0);
    cube[14] = 12;
    cube.extend((0..24u8).flat_map(|texel| [texel, texel, texel]));
    let texture = Texture::parse_tga_layers(&cube, 6).expect("the cube parses");
    assert!(texture.cube && texture.layers.len() == 6);
    let faces: Vec<u8> = (0..6).map(|face| texture.layers[face][0].data[0]).collect();
    assert_eq!(faces, [0, 4, 8, 12, 16, 20]);
    // A flip-book sheet: a 4×2 image of 2×2 frames, each 2×1, stored bottom
    // row first, each texel its stored place. The frames come out left to
    // right from the top row, each its own layer (a mod's glowing blade
    // edge drew its whole sheet across the blade).
    let mut sheet = header(2, 24, 0, &[], 0);
    sheet[12] = 4;
    sheet.extend((0..8u8).flat_map(|texel| [texel, texel, texel]));
    let texture = Texture::parse_tga_frames(&sheet, 2, 2).expect("the sheet parses");
    let frames: Vec<Vec<u8>> = (texture.layers.iter())
        .map(|frame| frame[0].data.iter().step_by(3).copied().collect())
        .collect();
    assert_eq!(frames, [[4, 5], [6, 7], [0, 1], [2, 3]]);
    assert!(Texture::parse_tga_frames(&sheet, 3, 2).is_err());
}

/// A texture saved with an alpha channel it leaves opaque throughout has no
/// alpha to draw by (`fmt_odyssey::texture::Texture::has_alpha`), so its
/// mesh is drawn opaque, first and whatever its order among see-through
/// meshes, as its blend comes to the same: a modded head's 32-bit face drawn
/// as a blended surface came after its eyelashes from some angles, and the
/// lashes' soft edges showed the room through the face. One texel less than
/// opaque keeps the alpha, in a 32-bit image or a DXT5 block (whose index 6
/// is clear where its first alpha is not above its second). No installs;
/// the surfaces need `--features rig` (proposed 2026-11-10, not yet
/// approved).
#[test]
fn opaque_alpha_channel_is_no_alpha() {
    use fmt_odyssey::texture::{Mip, PixelFormat, Texture};

    let image = |format: PixelFormat, data: Vec<u8>| Texture {
        format,
        layers: vec![vec![Mip {
            width: 4,
            height: 4,
            data,
        }]],
        cube: false,
        txi: Default::default(),
        bottom_up: true,
        missing_mips: 0,
        declared_size: None,
    };
    let rgba = |last: u8| {
        let mut data = [200, 200, 200, 255].repeat(16);
        data[63] = last;
        image(PixelFormat::Rgba8, data)
    };
    // A DXT5 block: its two alphas, every texel's 3-bit index (0 for the
    // first alpha), then any colour.
    let dxt5 = |a0: u8, a1: u8, last_index: u64| {
        let mut data = vec![a0, a1];
        data.extend_from_slice(&(last_index << 45).to_le_bytes()[..6]);
        data.extend_from_slice(&[0xff; 8]);
        image(PixelFormat::Dxt5, data)
    };
    for (what, texture, alpha) in [
        ("32-bit, opaque", rgba(255), false),
        ("32-bit, a texel short of opaque", rgba(254), true),
        ("DXT5, both alphas opaque", dxt5(255, 255, 7), false),
        ("DXT5, its clear index used", dxt5(255, 255, 6), true),
        ("DXT5, first alpha only", dxt5(255, 0, 0), false),
        ("DXT5, second alpha used", dxt5(255, 0, 1), true),
    ] {
        assert_eq!(texture.has_alpha(), alpha, "{what}");
    }
    #[cfg(not(feature = "rig"))]
    println!("opaque_alpha_channel_is_no_alpha's surfaces skipped: run with --features rig");
    #[cfg(feature = "rig")]
    {
        use asset_odyssey::texture::TextureIr;
        use kjk_game::material::{Surface, surface};
        for (texture, drawn) in [
            (rgba(255), Surface::Opaque),
            (rgba(254), Surface::OpaqueAlpha),
        ] {
            let facts = TextureIr::from_texture(&texture, None)
                .expect("it decodes")
                .facts;
            assert_eq!(surface((0, true), 1.0, (Some(&facts), &[&facts])), drawn);
        }
    }
}

/// A lightmapped mesh is cut out by its diffuse's alpha, whatever its
/// lightmap's (`kjk_game::material::surface`): High Quality Skyboxes II's
/// Taris building cards (`m02aa_10b`, DXT5 over a lightmap with none) drew
/// their hidden black field and the author's mark. A lightmap's own alpha
/// counts only when no diffuse loaded, as before. Needs `--features rig`;
/// no installs (proposed 2026-11-08, not yet approved).
#[test]
fn lightmapped_cut_out_by_diffuse() {
    #[cfg(not(feature = "rig"))]
    println!("lightmapped_cut_out_by_diffuse skipped: run with --features rig");
    #[cfg(feature = "rig")]
    {
        use asset_odyssey::texture::TextureFacts;
        use kjk_game::material::{Surface, surface};

        let with_alpha = TextureFacts {
            alpha: true,
            ..TextureFacts::default()
        };
        let without = TextureFacts::default();
        for (diffuse, lightmap, expected) in [
            (Some(&with_alpha), Some(&without), Surface::OpaqueAlpha),
            (Some(&without), Some(&with_alpha), Surface::Opaque),
            (Some(&with_alpha), None, Surface::OpaqueAlpha),
            (None, Some(&with_alpha), Surface::OpaqueAlpha),
            (None, Some(&without), Surface::Opaque),
        ] {
            let loaded: Vec<_> = [diffuse, lightmap].into_iter().flatten().collect();
            assert_eq!(
                surface((0, false), 1.0, (diffuse, &loaded)),
                expected,
                "diffuse {diffuse:?}, lightmap {lightmap:?}"
            );
        }
    }
}

/// Which copy of a resource the games read (`vfs::probes`, journal
/// 2026-11-05-override-precedence): `override/` over a module's own, a
/// `.mod` standing for its RIMs and `_dlg.erf`, `patch.erf` over a module,
/// TSL's override subfolders one level down, co-op's check covering just
/// the override files read. On installs written there; no
/// installs needed (approved 2026-11-05).
#[test]
fn mod_precedence() {
    probed("precedence", &vfs::probes::probes(), 5);
}

/// The games' data as co-op compares it (`vfs::fingerprint`): what a play
/// leaves in JA's `base/` (saves, screenshots, demos, settings, logs, the
/// game's libraries, folder metadata) is not game data, a model's
/// `animation.cfg` is; a patched archive is named by itself; twelve mod
/// archives stay within a handshake's groups; a file browser's metadata in
/// KOTOR's folders is not game data either. No installs (approved
/// 2026-10-30).
#[test]
fn coop_data_check() {
    use vfs::fingerprint::{Fingerprint, MAX_WORLD_GROUPS, fingerprint};
    use vfs::{DirIndex, Game};

    // A data root holding `files` (path under `folder`, contents).
    let print = |game: Game, name: &str, folder: &str, files: &[(&str, &str)]| -> Fingerprint {
        let root = std::env::temp_dir().join(format!(
            "kjk-coop-data-{}-{}-{name}",
            std::process::id(),
            game.key()
        ));
        let _ = std::fs::remove_dir_all(&root);
        for (path, contents) in files {
            let path = root.join(folder).join(path);
            std::fs::create_dir_all(path.parent().expect("under the root")).expect("a folder");
            std::fs::write(path, contents).expect("a file");
        }
        let index = DirIndex::build(&root).expect("the index");
        let print = fingerprint(game, &index, None).expect("the fingerprint");
        std::fs::remove_dir_all(&root).expect("cleaned up");
        print
    };
    let ja = |name: &str, files: &[(&str, &str)]| print(Game::JediAcademy, name, "base", files);
    let stock = [
        ("assets0.pk3", "zero"),
        ("assets1.pk3", "one"),
        ("models/players/kyle/model.glm", "kyle"),
        (
            "models/players/_humanoid/animation.cfg",
            "BOTH_DEATH1 0 21 -1 20",
        ),
    ];
    let played = [
        ("saves/auto.sav", "save"),
        ("screenshots/shot0001.jpg", "shot"),
        ("demos/duel.dm_26", "demo"),
        ("jaconfig.cfg", "seta r_mode 4"),
        ("qconsole.log", "log"),
        ("jampgamex86.dll", "dll"),
        ("cgame.dylib", "dylib"),
        ("jampgame.bundle/contents/macos/jampgame", "bundle"),
        (".ds_store", "finder"),
    ];

    let fresh = ja("fresh", &stock);
    let after_a_play = ja("played", &[&stock[..], &played[..]].concat());
    assert_eq!(fresh.world(), after_a_play.world(), "a played-in base/");
    assert_eq!(after_a_play.files, stock.len(), "the files covered");

    let patched = stock.map(|(path, contents)| {
        (
            path,
            if path == "assets1.pk3" {
                "patched"
            } else {
                contents
            },
        )
    });
    let (host, other) = (fresh.world(), ja("patched", &patched).world());
    let differ: Vec<&str> = host
        .groups
        .iter()
        .filter(|group| !other.groups.contains(group))
        .map(|(name, _)| name.as_str())
        .collect();
    assert_eq!(differ, ["base/assets1.pk3"], "the groups that differ");

    let names: Vec<String> = (0..12).map(|n| format!("mod{n:02}.pk3")).collect();
    let mods: Vec<(&str, &str)> = names.iter().map(|name| (name.as_str(), "mod")).collect();
    let world = ja("mods", &mods).world();
    let groups: Vec<&str> = world.groups.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(groups.len(), MAX_WORLD_GROUPS, "twelve archives' groups");
    assert_eq!(groups[0], "base", "the archives past the seventh");
    assert_eq!(
        groups[MAX_WORLD_GROUPS - 1],
        "base/mod06.pk3",
        "the seventh"
    );

    let kotor = [
        ("chitin.key", "key"),
        ("dialog.tlk", "tlk"),
        ("override/appearance.2da", "2DA V2.b"),
        ("modules/end_m01aa.rim", "rim"),
    ];
    let browsed = [
        ("override/.ds_store", "finder"),
        ("override/thumbs.db", "explorer"),
        ("modules/desktop.ini", "explorer"),
    ];
    let fresh = print(Game::Kotor1, "fresh", "", &kotor);
    let browsed = print(
        Game::Kotor1,
        "browsed",
        "",
        &[&kotor[..], &browsed[..]].concat(),
    );
    assert_eq!(fresh.world(), browsed.world(), "KOTOR's browsed folders");
    assert_eq!(browsed.files, kotor.len(), "KOTOR's files covered");
}

/// The store's cap (`store::Store::cap`), which holds a game's area bakes
/// to a size: the least recently used go first, one read again counting as
/// new, down to the size; the most recent stays even alone over it; other
/// kinds are untouched. No installs (proposed 2026-11-03).
#[test]
fn store_cap() {
    let root = std::env::temp_dir().join(format!("kjk-store-cap-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let store = store::Store::at(&root);
    let kind = "bake/k1/area-v1";
    let keys = [b"x", b"y", b"z"].map(|input| store::Key::of("cap-v1", &[input]));
    let now = std::time::SystemTime::now();
    // Made three, two and one hours ago, ten bytes each.
    for (hours, key) in (1..=3).rev().zip(&keys) {
        let path = store.put(kind, key, &[0; 10]).expect("put");
        std::fs::File::options()
            .append(true)
            .open(&path)
            .and_then(|file| file.set_modified(now - std::time::Duration::from_secs(hours * 3600)))
            .expect("aged");
    }
    let other = store
        .put("bake/k1/library-v1", &keys[0], &[0; 10])
        .expect("put");
    assert!(store.get(kind, &keys[0]).is_some(), "the oldest read again");
    // Looked at by path: a read would mark them used.
    let kept = || keys.each_ref().map(|key| store.path(kind, key).exists());

    let removed = store.cap(kind, 25).expect("capped");
    assert_eq!((removed.files, removed.bytes), (1, 10));
    assert_eq!(kept(), [true, false, true], "the least recently used went");

    let removed = store.cap(kind, 0).expect("capped");
    assert_eq!(removed.files, 1);
    assert_eq!(kept(), [true, false, false], "the most recent stays");
    assert!(other.exists(), "another kind untouched");
    let _ = std::fs::remove_dir_all(&root);
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

/// The talk-table lines a level's digest holds: those its scripts may read,
/// as they read them, so tables differing only in lines no script names
/// (Aspyr's Mac K1 table and the Windows one: an options screen's text)
/// make the same levels, and a named line changed moves the digest. No
/// installs (approved 2026-11-05).
#[test]
fn talk_lines_digest() {
    passed("talk lines", &sim::script::dialogue_probes::talk(), 1);
}

/// What conversations keep: answer bytes (a count that moves, so a repeated
/// command answers once), and the scripts' state and the snapshot's view
/// of conversations through their encodings.
#[test]
fn dialogue_state() {
    passed("dialogue state", &sim::script::dialogue_probes::kept(), 4);
}

/// A save against a module whose placements changed: where each old number
/// stands in the new list, the roster's encoding, and saves loaded and
/// modules left entered again on the proving ground after a placement was
/// inserted, removed or added (the save migrations design, 3c).
#[test]
fn reconcile_numbers() {
    passed("reconcile", &sim::reconcile_probes::checks(), 11);
    passed("reconcile at load", &sim::reconcile_probes::loads(), 17);
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

/// Issue 165: an item whose template only a module's own archives hold
/// (Tatooine's star map datapad, the Sand People's chieftain's gift, Brejik's
/// prizes) shows its name and description in every other module, as KOTOR's
/// save keeps it whole; it had shown its resref. Read on the Ebon Hawk
/// (`ui::assets::GuiAssets::item`, its template found in its module's own
/// archives).
#[test]
fn carried_story_items_named() {
    const CARRIED: [&str; 16] = [
        "tat_starpad",
        "tat17_racebond",
        "tat20aa_westmap",
        "tat20_gaffichief",
        "tat17_huntpermit",
        "tat17_wraidplate",
        "tat17_datapad",
        "tat18_wraidplate",
        "dan_starpad",
        "dan16_key001",
        "g_mandat1",
        "dan_map1",
        "dan_map2",
        "tar03_brejikband",
        "tar03_brejikglov",
        "tar03_brejikbelt",
    ];
    let discovery = discovery();
    if discovery.get(vfs::Game::Kotor1).is_none() {
        println!("k1: no install, skipped");
        return;
    }
    let mut assets =
        ui::assets::GuiAssets::open(&discovery, vfs::Game::Kotor1).expect("the install opens");
    assets.enter_module("ebo_m12aa");
    let unnamed: Vec<&str> = CARRIED
        .into_iter()
        .filter(|template| {
            assets.item(template).is_none_or(|look| {
                println!("{template}: {}", String::from_utf8_lossy(&look.name));
                // The hunter's licence has no description in K1's data.
                look.name == template.as_bytes()
                    || (look.description.is_empty() && *template != "tat17_huntpermit")
            })
        })
        .collect();
    assert!(
        unnamed.is_empty(),
        "shown by resref or undescribed: {unnamed:?}"
    );
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
/// AI idle on Telos), and the restrict mode holding the player back
/// (`restrict_mode_holds_the_player`: no jump, no attack, the weapon put
/// away and a weapon change refused in the games' suit sections; approved
/// 2026-11-12), and a
/// minigame's area taking no companions
/// (`minigame_takes_no_companions`: Peragus's hangar turret), and the
/// party selection screen's exit script told the party was accepted
/// (`party_screen_accepted`: the Ebon Hawk's ramp on Dantooine) or that the
/// player backed out (`party_screen_cancelled`: the ramp stays aboard),
/// and the party selection screen opened in play changing the party in
/// TSL but not in an unescapable area nor in K1 (`party_changed_in_play`:
/// Luxa's arms deal on Telos; both issue 317, approved 2026-11-12), and the
/// area's iteration passing over the dead (`area_iteration_skips_the_dead`:
/// the battle of Khoonda's end), and a companion spawned again while its
/// creature is here being that creature (`spawned_again_is_the_same`:
/// Atton's warning on Nar Shaddaa), and `k_align_movie` run as the status
/// summary closes (`summary_closed_runs_align_movie`: Visas Marr's
/// arrival), and each game's local boolean slots (K1's 96, TSL's 160:
/// `local_booleans_k1`, `local_booleans_tsl`), and the player's own
/// character kept out of `ChangeToStandardFaction` in TSL
/// (`pc_faction_tsl`: Dxun's battle circle). The sim alone, on the proving
/// ground.
#[test]
fn tsl_party_rules() {
    probed("tsl", &sim::tsl_probes::probes(), 31);
}

/// The routines override mods call that the games' own scripts rarely do
/// (`sim::routine_probes`): the maths routines' bits as both executables'
/// handler gives them (degrees, `f32` steps, zeros out of a domain), the
/// string routines as `CExoString` and `atof` work them, and in a world of
/// three creatures the faction walk, leader and change, the sheet getters,
/// an item's worth, floating text, six effects, `ActionUseFeat` and
/// `ActionEquipMostEffectiveArmor`; TSLRCM's last ones (a forced follow,
/// the module's player death and `GetLastPlayerDied`, the autosave and
/// swoop screen asked), a pick-up and a put-down, and invisibility, damage
/// reduction and damage shields; and K1's own scripts' last unbound ones
/// (inventories and their disturbance, item costs changed, damage by type,
/// a visual at a place, a party slot's creature, the music's volume, the
/// in-game page's script), and every minigame routine answered counted bound
/// (`kjk script check`). The sim alone, on the proving ground. At least 26
/// checks (raised from 18, approved by the owner 2026-11-06).
#[test]
fn mod_routines() {
    probed("mod routines", &sim::routine_probes::probes(), 26);
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

/// Items a module gives a resref of its own, and what a creature joining
/// brings (`sim::own_item_probes`), approved 2026-11-08: two modules'
/// `g_i_datapad002` (Calo Nord's journal, the Sith base's log) are two
/// items, each read as its own module's, never one stack; two of one
/// module's still stack; a companion joining puts what it carries in the
/// party's inventory (Mission's vibroblade) and keeps what it wears. The
/// script state alone.
#[test]
fn own_items() {
    probed("own items", &sim::own_item_probes::probes(), 4);
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
/// loads; each run twice ends the same. Approved 2026-11-09: the player's
/// own character, destroyed by a script, stays (K1's Bek base night
/// destroys it while a companion leads). The sim alone.
#[test]
fn destroy_fades() {
    probed("destroys", &sim::destroys::probes(), 14);
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
/// scripts see who did it; Close is offered on an open door the player
/// could open again (QUIRKS.md Q-0330), not on one only scripts open, one
/// leading out or one broken open; a companion, led or driven by a player,
/// is a target, picked only clear ahead of the player, and never the target
/// of the player it is the body of; a placeable hung up a wall (Yavin
/// Station's door panels) is picked from the floor beneath it; a console
/// on a bench, or in its own solid body, is picked (Peragus); two runs
/// end the same. The sim alone.
#[test]
fn use_actions() {
    probed("use", &sim::uses::probes(), 44);
}

/// The crosshair on a placeable used from its action point (proposed
/// 2026-11-10, Lonna Vash's body in its force cage on Korriban): on the
/// proving ground a container behind the wall, its action point before the
/// wall, is picked looking straight at it and with soft aim; the same
/// container without an action point, or with its action point behind the
/// wall too, is not. The sim alone.
#[test]
fn crosshair_action_points() {
    probed(
        "crosshair action point",
        &sim::target::action_point_probes(),
        3,
    );
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
/// used; two runs end the same; from beside the wall the player walks to
/// the gate's action point on its side (KOTOR's use range from a door's
/// walkmesh use position), off the wall's face, opens the gate, then closes
/// it with Close, its `OnClosed` heard; Dialog on a creature behind a
/// counter no walk comes round starts where the walk comes no nearer,
/// within a talk's reach, and not past it (issue 204). The sim alone.
#[test]
fn use_walk() {
    probed("walk", &sim::uses::walk_probes(), 7);
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

/// Disarming a mine in one choice (owner's playtest, 2026-10-30: "fiddly
/// and sometimes leads to you detonating it"): on the proving ground the
/// player chooses Disarm on a K1 mine (its 1.5 m octagon, hostile) straight
/// on, at an angle, from the side, already running and from beside it;
/// walked there, it stops outside the mine's circle, never sets it off,
/// and disarms it. The sim alone. [awaiting the owner's approval]
#[test]
fn mine_walk() {
    probed("mine walk", &sim::traps::walk_probes(), 5);
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
/// its fists instead; set on a dummy of its own faction its attack lands
/// all the same; two runs end the same. The sim alone.
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
/// Rapid Shot alternates (right, left, right, left; Multi Shot a shot more),
/// Power Blast and Sniper Shot fire both guns at once for one cost
/// (owner's ask, 2026-11-02), and a gunner without the feats only fires plain
/// shots with the alternate; a hostile creature with a pair fires
/// both, oftener than with one; the pair's state comes back whole from a
/// snapshot mid-burst; two runs end the same. The sim alone.
/// (approved 2026-10-25)
#[test]
fn dual_pistols() {
    probed("dual pistols", &sim::pair_probes::probes(), 12);
}

/// A feat attack chosen with the next-feat key (`combat::picks`): a gun
/// goes round the lines known, each type keeping its own; two blades and a
/// double-bladed weapon carry each melee line known in turn, then none, and
/// a blow carries the chosen feat, Flurry's with its extra pace; a single
/// blade has no choice; a script's talent chooses too; the choices come
/// back from their bytes. The sim alone. [awaiting the owner's approval]
#[test]
fn feat_picks() {
    probed("feat picks", &sim::picks_probes::probes(), 8);
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
        stances: world.stance_ids(),
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
    probed("coop talk", &sim::coop_talk::probes(), 143);
}

/// Scene mode (`docs/DIALOGUE.md`): the scene probe group,
/// `sim::scene_probes::all` (`kjk sim probes scene`), each case on the
/// proving ground with conversations and scripts written there; the sim
/// alone. Approved by the owner 2026-10-17 (do what you think is best).
/// The two saber-rules cases (a scene plays alike under JA, MD and MDA)
/// were approved 2026-11-09. The check that a loaded game plays the
/// area's music as placed (`load_plays_music_as_placed`) was approved
/// 2026-11-12.
#[test]
fn scene_mode() {
    probed("scene", &sim::scene_probes::all(), 74);
}

/// The party's calls (I31): `sim::party::probes`, among them in combat,
/// a break's freeze and the ready check, and what enters a trigger; the sim
/// alone.
#[test]
fn party_calls() {
    probed("party", &sim::party::probes(), 20);
}

/// Falling and being revived (I31): `sim::falls::probes`, each on the
/// proving ground; the sim alone. A poison that downed a body is gone
/// when it gets up (the owner's report of 2026-10-30).
#[test]
fn downed_and_revive() {
    probed("falls", &sim::falls::probes(), 19);
}

/// A creature put at a chair, its box buried in it where KOTOR's walker
/// stands free on the floor, stands there rather than being lifted onto
/// it: `sim::tucked_probes` (TSL's Jedi Council seated, #277).
#[test]
fn tucked_at_furniture() {
    probed("tucked", &sim::tucked_probes::probes(), 4);
}

/// A door's or placeable's facing, as scripts read and set it, is the way
/// its model's +Y points: its bearing turned a quarter left. A creature
/// made at a placeable's location faces that way, and a placeable made at a
/// location turns its +Y to the location's facing: `sim::facing_probes`
/// (TSL's Exile lying across the Ebon Hawk's medical bed).
#[test]
fn placeable_facing_is_its_front() {
    probed("facing", &sim::facing_probes::probes(), 3);
}

/// A playthrough's character up on a raised floor its walk's goal is not
/// on steps down where it can walk straight over to, not into a wall on
/// the goal's side: `sim::step_down_probes` (Dantooine's Enclave sublevel,
/// the ledge round the dung pit).
#[test]
fn steps_down_where_it_can_walk() {
    probed("step down", &sim::step_down_probes::probes(), 1);
}

/// A route through a doorway whose walkable floor runs on under the wall
/// beside it keeps every leg one a standing body walks straight along: the
/// legs are moved sideways off the jamb: `sim::doorway_probes` (Nar
/// Shaddaa's refugee quad, its upper level's doors).
#[test]
fn doorway_legs_pass_the_jamb() {
    probed("doorway", &sim::doorway_probes::probes(), 1);
}

/// The player and the companions are solid to each other, as every other
/// character is, and a character that keeps running into another for a
/// second walks through it until the two have parted: `sim::jam_probes`
/// (issue 280).
#[test]
fn characters_solid_and_unstuck() {
    probed("jam", &sim::jam_probes::probes(), 9);
}

/// The party members nobody plays follow and fight beside the body played:
/// `sim::party_fights_probes`, owner's request.
#[test]
fn unmanned_party_fights() {
    probed("party fights", &sim::party_fights_probes::probes(), 7);
}

/// Control passing on from a fallen leader to the next party member
/// standing, as Tab would pass it, past a guest's companion:
/// `sim::party_fights_probes::passing`, the owner's report of 2026-10-30.
#[test]
fn downed_leader_passes_control() {
    probed("downed switch", &sim::party_fights_probes::passing(), 3);
}

/// Items and levels in co-op (I31): `sim::coop_items::probes`, the
/// scenario's checks and two runs ending alike; the sim alone.
#[test]
fn coop_items_and_levels() {
    probed("coop items", &sim::coop_items::probes(), 22);
}

/// Items used and upgraded (the owner's bugs of 2026-10-04):
/// `sim::item_uses::probes`, a use taken from the goods, the HUD's kinds
/// as they have them (those for the user's race), Use Item doing what the
/// item's button does or refused when nothing would, a droid's worn items,
/// a lightsaber's crystals fitted and taken out; the sim alone.
#[test]
fn item_uses() {
    probed("item uses", &sim::item_uses::probes(), 30);
}

/// Item properties beyond combat (`sim::inventory::probes`): an item's
/// best bonus of a kind with its penalties, typed extras and Keen; two
/// items' scores, skills, saves, dodge defence, defence against a race and
/// a bonus feat on a sheet, held by the caps, gone when taken off; use
/// limits (a side's, K1's and TSL's "All", Proficiency All, a fitted
/// crystal's light-side limit, a race's, an ability's); a body's locked
/// slots (TSL's one-handed Kreia: both off hands neither filled nor emptied
/// by her player, her main hand as anyone's); immunity to criticals; on-hit
/// stun, a mind-immune target, death; regeneration every 6 s; a worn light
/// lasting; TSL's upgrade slots; the encodings. The sim alone, no install.
#[test]
fn item_properties() {
    probed("item properties", &sim::inventory::probes::probes(), 27);
}

/// `SwitchPlayerCharacter` (K1's Leviathan, TSL's many): `sim::switch::probes`,
/// a script switching the first seat to a companion made from its
/// template, or as the party table kept it (its body too: TSL's one-handed
/// Kreia), the seat walking it, scripts seeing it, a switch back, saves,
/// snapshots and prediction, a standing creature taken, a guest giving way;
/// a companion's body changed in the run that makes it (107PER's Kreia); a
/// companion standing in a module left, not in the party, kept by the table
/// as it stands, body and all; the sim alone.
#[test]
fn switch_player_character() {
    probed("switch", &sim::switch::probes(), 19);
}

/// Encounters count the drivers (I31): `sim::scaling::probes`, and
/// `session::encounters::probes` (the count a plan is for reaches its
/// module); no install.
#[test]
fn encounters_count_drivers() {
    probed("scaling", &sim::scaling::probes(), 12);
    probed("encounter count", &session::encounters::probes(), 3);
}

/// A death's XP read from either game's `xptable.2da` heading, and TSL's
/// death XP as its executable works it out (`session::journal::probes`); no
/// install.
#[test]
fn kill_xp_tables() {
    probed("kill xp tables", &session::journal::probes(), 3);
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
/// shows reaches both with the planets made available and selectable, K1's
/// Yavin Station among them as its live content check makes it (TSL none),
/// and on no planet where the script asks for -1 (leaving Yavin); the
/// guest's travel and the host's to a planet not selectable run nothing,
/// the host's runs the travel script, which hears the planet;
/// `EndGame(TRUE)` and K1's argument-less `EndGame()` lose the game,
/// `EndGame(FALSE)` shows both the end, after the credits when
/// `StartCreditSequence` started them or TSL's ending movie rolled them;
/// the sim alone.
#[test]
fn galaxy_and_end() {
    probed("galaxy and end", &sim::galaxy::probes(), 9);
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
/// minimum; KOTOR's feat and power charts (Force Wave needing Push and
/// Whirlwind); and on the proving ground, a companion refused armour and an
/// implant with the game's line, granted feats by `GrantFeat` (Toughness
/// for every level), then wearing the armour and taking a level whose
/// class grants Master Toughness, twice alike; no install.
#[test]
fn feats() {
    probed("feats", &sim::feats::probes(), 17);
}

/// A host's save kept by a player (I31): `saves::copies::probes` (packed,
/// kept byte for byte, refused when foreign, replaced by a later copy, the
/// player's own saves untouched, two hosts of one name apart, a host's
/// copies capped, no number left refused, ids inside the folder) and
/// `saves::refusal_probes` (what refuses a save) and `saves::story_probes` (a save's story by name, its roster module, a JSON round trip, a slot write with `story.json`, a missing one read as none, a companion who joined and left keeping her items in the story, one who never joined none, an item the module lacks restored from the install's lookup, else lost, the story restored and written again equal but for what each case names as lost, spec 4c, and placements alike told apart by the story's roster, here and in a module left, when a placement is inserted before them); no install.
#[test]
fn save_copy_round_trip() {
    probed("save copies", &saves::copies::probes(), 12);
    probed("save refusals", &saves::refusal_probes(), 14);
    probed("save story", &saves::story_probes(), 15);
}

/// The save editor's ground (`edit::probes::all`, approved 2026-11-04) and
/// the app's model on it (`save_editor::probes::all`): what docs/TESTS.md's
/// row lists. Only with `--features editor` (editor.yml), so the engine's
/// own runs neither build nor run it.
#[cfg(feature = "editor")]
#[test]
fn save_edits() {
    probed("save edits", &edit::probes::all(), 26);
    probed("save editor", &save_editor::probes::all(), 0);
}

/// The saves' safety net (`saves::safety_probes`): the meta's new fields
/// and an older meta, a damaged save listed and refused, one without a hash
/// trusted, autosaves rotating, the backup before a new release, a failed
/// write leaving nothing, a refusal naming the build that made the save;
/// and the release gate's corpus (`saves::corpus_probes`): a slot folder read
/// back whole and a damaged one refused, the corpus's slots found by release,
/// a current save passing, one older than a fake `breaking` entry expected
/// (and failing without the entry or when its story restore lost something), the table printed, a slot written only under a plain name, a panicking save a FAIL row among the others; no install.
#[test]
fn save_safety() {
    probed("save safety", &saves::safety_probes(), 11);
    probed("save corpus", &saves::corpus_probes(), 7);
}

/// What a start reads from the saves' folder (`saves::startup_probes`,
/// issue 279). A note of what was played that was cut short, or holds a
/// value of the wrong type, reads as nothing and is kept aside as
/// `played.json.bad`. The note and the release stamp are written without
/// leaving a temporary file. A folder with a cut meta, a cut story and a
/// damaged release stamp still lists its good save and takes its backup.
/// No install is needed.
#[test]
fn startup_files() {
    probed("startup files", &saves::startup_probes(), 4);
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

/// The sessions with tests of their own (`net_sessions` plays the rest).
const OWN_SESSION_TESTS: [&str; 7] = [
    "hold",
    "reload",
    "lan_every_snapshot",
    "wan_rewinds_on_a_thread",
    "lan_rewinds_on_a_thread",
    "hold_rewinds_on_a_thread",
    "lan_rewind_threads_die",
];

/// Whole sessions (`kjk net session`, no install): the determinism world's
/// `lan`, `wan`, a one-minute `soak`, `hostile`, `reconnect`, `restart`
/// and `save_copy` (`hold`, `reload` and the rewinds' are their own tests)
/// played over a `SimNet`, every binding check passing; and a million
/// corrupted session encodings through their decoders, none panicking.
#[test]
fn net_sessions() {
    use net::session::sessions;
    let played: Vec<sessions::Session> = sessions::determinism(1)
        .into_iter()
        .filter(|session| !OWN_SESSION_TESTS.contains(&session.setup.name.as_str()))
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

/// A joined player's rewinds (issue 229, added 2026-11-09): the sessions
/// that make them on a thread beside the frames, as the game does (on a
/// clean link, over `wan`'s, through a 6 s hold, and with the threads
/// dying mid-session, going on in the caller), and from every snapshot,
/// as before;
/// every binding check passing, each client rewinding and none failing
/// to restore, and no own-avatar mismatch of an own command on the clean
/// links (`prediction`). The rewinds on a thread land whenever the thread
/// is done in wall time, so those sessions are not the same twice; their
/// checks are.
#[test]
fn net_rewinds() {
    use net::session::sessions;
    let played: Vec<sessions::Session> = sessions::determinism(1)
        .into_iter()
        .filter(|session| OWN_SESSION_TESTS[2..].contains(&session.setup.name.as_str()))
        .collect();
    assert_eq!(played.len(), 5, "sessions went missing");
    for session in played {
        let (report, checks) = session_checks(session);
        for client in &report.clients {
            assert!(
                client.stats.rewinds_beside > 0 && client.stats.rewinds_failed == 0,
                "every client rewinds while its frames go on, none failing: {} beside, {} failed",
                client.stats.rewinds_beside,
                client.stats.rewinds_failed
            );
        }
        assert!(
            checks.iter().any(|name| name.ends_with("rewinds")),
            "the rewinds are checked"
        );
    }
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
    probed("creation", &sim::player_probes::creation(), 11);
}

/// TSL's player's level as the scripts read it (`G_PC_LEVEL`, set by the
/// XP the party gains) and `AddMultiClass` on a player (the class taken up
/// unlevelled, the party given its next level's XP); `sim::player_probes`,
/// no install.
#[test]
fn player_levels_told() {
    probed("levels told", &sim::player_probes::levels_told(), 1);
    probed(
        "multiclass outside the party",
        &sim::player_probes::multiclass_outside_party(),
        2,
    );
}

/// A class `AddMultiClass` gives is a script's second class at once, before
/// its first level (both games' `GetClassByPosition`), and what a script
/// delays after it shows the upgrade bench waits until the bench closes, as
/// both games pause under it; `sim::player_probes`, no install (approved
/// 2026-11-12).
#[test]
fn multiclass_and_bench_scripts() {
    probed("class at once", &sim::player_probes::class_at_once(), 1);
    probed("bench holds", &sim::player_probes::bench_holds(), 1);
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

/// A creature's body and head take `appearance.2da`'s `envmap` as KOTOR's
/// executable applies it (`asset_odyssey::creature::EnvMap`): a cube map
/// by name, else the area's ARE `DefaultEnvMap` (`DEFAULT`, or an empty
/// cell), none where that is empty; the scene then draws their textures as
/// it draws one whose TXI names that env map, its alpha the env map's mask
/// rather than see-through (a mod's armour showed through itself). With a
/// K1 install (else skipped): a Sith soldier (row 28) and a war droid (66)
/// name `CM_BareMetal`, the red spider droid (437) `Gunmetal`, a player's
/// body (137) the area's, and the Endar Spire's areas have none (proposed
/// 2026-11-10, not yet approved).
#[test]
fn creature_env_maps() {
    use asset_odyssey::creature::EnvMap;
    let named = |name: &str| EnvMap::Named(name.to_string());
    for (cell, area, map) in [
        (Some("cm_baremetal"), None, Some("cm_baremetal")),
        (Some("cm_baremetal"), Some("cm_area"), Some("cm_baremetal")),
        (Some("default"), Some("cm_area"), Some("cm_area")),
        (None, Some("cm_area"), Some("cm_area")),
        (Some("DEFAULT"), None, None),
        (None, Some(""), None),
    ] {
        let envmap = EnvMap::of_cell(cell.map(str::to_string));
        assert_eq!(
            envmap.resolve(area),
            map.map(str::to_string),
            "{cell:?} in {area:?}"
        );
    }
    let discovery = discovery();
    if discovery.get(vfs::Game::Kotor1).is_none() {
        println!("K1's env maps skipped: no KOTOR install (set KJK_KOTOR1 in .env)");
        return;
    }
    let source = module_source(&discovery, vfs::Game::Kotor1, "end_m01aa").expect("it opens");
    let mut tables = asset_odyssey::tables::Tables::default();
    let seen = [28, 66, 437, 137].map(|appearance| {
        asset_odyssey::creature::resolve_appearance(&source, appearance, None, &mut tables)
            .expect("the appearance resolves")
            .envmap
    });
    assert_eq!(
        seen,
        [
            named("cm_baremetal"),
            named("cm_baremetal"),
            named("gunmetal"),
            EnvMap::AreaDefault,
        ]
    );
    let module = asset_odyssey::module::ModuleIr::build("end_m01aa", &source).expect("it builds");
    assert!(
        module
            .areas
            .iter()
            .all(|area| area.default_envmap.is_none())
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
        let save =
            sim::Save::decode(&bytes).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let again = sim::Save::decode(&save.encode()).expect("a re-encoded save decodes");
        assert_eq!(again, save, "{}: re-encoding changes it", path.display());
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

/// Every format fixture this build reads, stamped with other data than its
/// levels' (its own level, its place and the module it left), loads
/// re-stamped with the level's digest and walks back into the module it
/// left, re-stamped as it enters; another level, or the module left planned
/// for another player count, is still refused
/// (docs/superpowers/specs/2026-10-28-save-migrations-design.md, 3b).
#[test]
fn stale_stamps_load() {
    let dir = approved_tests::workspace().join(sim::fixtures::DIR);
    let (level, left) = (sim::fixtures::kitchen_sink().0, sim::fixtures::left());
    let stale = |digest: &mut [u8; 32]| digest.iter_mut().for_each(|byte| *byte = !*byte);
    let mut seen = 0;
    for entry in std::fs::read_dir(&dir).expect("the fixtures folder") {
        let path = entry.expect("a fixture").path();
        let name = path.display().to_string();
        let versions = path
            .file_name()
            .and_then(|name| sim::fixtures::versions_of(&name.to_string_lossy()));
        if versions.is_none_or(sim::fixtures::refused) {
            continue;
        }
        let mut save = sim::Save::decode(&std::fs::read(&path).expect("a fixture reads"))
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        stale(&mut save.level.digest);
        if let Some(place) = &mut save.snapshot.place {
            stale(&mut place.digest);
        }
        let mut modules = save.snapshot.stash.to_vec();
        modules
            .iter_mut()
            .for_each(|stashed| stale(&mut stashed.place.digest));
        save.snapshot.stash = sim::Stash::new(modules);

        let restamp = save
            .restamp(&level)
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(
            restamp.map(|restamp| (restamp.from, restamp.to)),
            Some((save.level.digest, level.digest())),
            "{name}: its own level re-stamped"
        );
        let load = || {
            save.world(&level, 2, None)
                .unwrap_or_else(|error| panic!("{name}: {error}"))
        };
        let mut world = load();
        let place = world.snapshot().place.expect("the save is in a module");
        assert_eq!(place.digest, level.digest(), "{name}: its place re-stamped");
        assert!(
            world.stale_stamp(&left).is_some(),
            "{name}: the module left is stamped with other data"
        );
        world
            .install(left.clone(), None)
            .unwrap_or_else(|error| panic!("{name}: back into the module left: {error}"));

        let mut recounted = (**left.module().expect("a module")).clone();
        recounted.players += 1;
        let error = load()
            .install(left.clone().with_module(recounted), None)
            .expect_err("the module left, planned for other players");
        assert!(
            matches!(error, sim::InstallError::StashChanged { .. }),
            "{name}: {error}"
        );

        let error = save
            .world(&left, 2, None)
            .map(|_| ())
            .expect_err("a save in another level");
        assert!(error.contains("another level"), "{name}: {error}");
        seen += 1;
    }
    assert!(seen > 0, "no fixture this build reads in {}", dir.display());
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

/// Pending bumps are numbered in the order they reached `main`, not by
/// name: a file merged later that sorts first takes the next number, and
/// those `main` already numbered (in saves made by its builds) keep theirs.
/// A branch numbers `main`'s first and its own after, as its merge will.
#[test]
fn bump_files_numbered_as_they_reached_main() {
    let root = std::env::temp_dir().join(format!("kjk-bumps-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(bumps::DIR)).expect("a temp dir");
    let git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(&root)
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .expect("git runs");
        assert!(status.status.success(), "git {args:?}: {status:?}");
    };
    let write = |name: &str| {
        let text = "[formats]\nsave = \"x\"\n[migrations]\nsave = \"y\"\n";
        std::fs::write(root.join(bumps::DIR).join(name), text).expect("a bump file");
    };
    let commit = |name: &str| {
        write(name);
        git(&["add", "."]);
        git(&["commit", "-q", "-m", name]);
    };
    let numbers = || {
        let pending = bumps::pending(&root).expect("the pending bumps");
        bumps::pending_since(&pending, "save")
    };
    let named = |names: &[&str]| -> Vec<(String, u32)> {
        (1..)
            .zip(names)
            .map(|(place, name)| (name.to_string(), place))
            .collect()
    };
    git(&["init", "-q", "-b", "main"]);
    commit("zz-first.toml");
    git(&["checkout", "-q", "-b", "branch"]);
    commit("aa-branch.toml");
    git(&["checkout", "-q", "main"]);
    commit("mm-second.toml");
    let on_main = named(&["ZZ_FIRST", "MM_SECOND"]);
    assert_eq!(numbers(), on_main);

    git(&["checkout", "-q", "branch"]);
    git(&["merge", "-q", "--no-edit", "main"]);
    assert_eq!(numbers(), named(&["ZZ_FIRST", "MM_SECOND", "AA_BRANCH"]));

    // The branch squash-merged: `aa` sorts first but reached `main` last.
    git(&["checkout", "-q", "main"]);
    commit("aa-branch.toml");
    let merged = numbers();
    assert_eq!(merged[..2], on_main[..]);
    assert_eq!(merged, named(&["ZZ_FIRST", "MM_SECOND", "AA_BRANCH"]));

    // A file not yet committed comes last.
    write("ab-new.toml");
    assert_eq!(
        numbers(),
        named(&["ZZ_FIRST", "MM_SECOND", "AA_BRANCH", "AB_NEW"])
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// A folder or a file reached through a symbolic link (a junction on
/// Windows) is indexed under the link's own path, as the games open
/// files through it. A link back up the tree is walked once, not forever,
/// and a link to nothing is skipped. Issue 262: an install whose
/// `streammusic/` was a link played no music.
#[cfg(unix)]
#[test]
fn dir_index_follows_links() {
    use std::os::unix::fs::symlink;
    use vfs::DirIndex;

    let base = std::env::temp_dir().join(format!("kjk-dir-links-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("install");
    let elsewhere = base.join("elsewhere");
    for (path, contents) in [
        (root.join("chitin.key"), "key"),
        (elsewhere.join("music/mus_theme_cult.wav"), "music"),
        (
            elsewhere.join("music/deeper/al_en_undergrnd.wav"),
            "ambience",
        ),
        (elsewhere.join("dialog.tlk"), "talk"),
    ] {
        std::fs::create_dir_all(path.parent().expect("under the base")).expect("a folder");
        std::fs::write(path, contents).expect("a file");
    }
    symlink(elsewhere.join("music"), root.join("StreamMusic")).expect("a folder link");
    symlink(elsewhere.join("dialog.tlk"), root.join("dialog.tlk")).expect("a file link");
    symlink(&root, root.join("loop")).expect("a link up the tree");
    symlink(base.join("nothing"), root.join("broken")).expect("a link to nothing");

    let index = DirIndex::build(&root).expect("the index");
    let keys: Vec<&str> = index.iter().map(|(key, _)| key).collect();
    assert_eq!(
        keys,
        [
            "chitin.key",
            "dialog.tlk",
            "streammusic/deeper/al_en_undergrnd.wav",
            "streammusic/mus_theme_cult.wav",
        ]
    );
    let read = |key: &str| std::fs::read_to_string(index.get(key).expect("indexed")).expect("read");
    assert_eq!(read("streammusic/mus_theme_cult.wav"), "music");
    assert_eq!(read("dialog.tlk"), "talk");
    let _ = std::fs::remove_dir_all(&base);
}

/// The stance sets' sim rules on the made-up skeleton (`kjk sim probes --only "stance sets"`).
#[test]
fn stance_sets_sim() {
    let failed: Vec<String> = sim::trace::stance_probes::probes()
        .into_iter()
        .filter_map(|(name, found)| found.err().map(|why| format!("{name}: {why}")))
        .collect();
    assert!(failed.is_empty(), "{failed:#?}");
}

/// The stance sets' part of the co-op handshake. The host refuses a guest whose
/// Movie Duels stance sets differ only when the host names the entry, and the
/// refusal carries at most 32 sets with names of at most 24 bytes. No installs.
#[test]
fn stance_sets_coop() {
    use net::wire::{
        Credentials, GameFingerprint, MAX_STANCE_NAME, MAX_STANCE_SETS, Reader, Refusal,
        STANCE_GAME,
    };

    // A credentials value that holds the stance entry when `digest` is given.
    let credentials = |digest: Option<[u8; 32]>| Credentials {
        build: "build".to_string(),
        games: digest
            .map(|digest| GameFingerprint {
                game: STANCE_GAME.to_string(),
                digest,
                groups: Vec::new(),
            })
            .into_iter()
            .collect(),
        name: "player".to_string(),
        reconnect: None,
    };
    let refused = Err(Refusal::GameData {
        game: STANCE_GAME.to_string(),
        groups: Vec::new(),
    });
    let checks = [
        ("neither has it", None, None, Ok(())),
        ("only the guest has it", None, Some([1; 32]), Ok(())),
        ("both have the same", Some([1; 32]), Some([1; 32]), Ok(())),
        ("the guest lacks it", Some([1; 32]), None, refused.clone()),
        ("another digest", Some([1; 32]), Some([2; 32]), refused),
    ];
    for (what, host, guest, expected) in checks {
        let got = Credentials::check(&credentials(host), &credentials(guest));
        assert_eq!(got, expected, "{what}");
    }

    // 40 sets with names of 30 bytes, and one name that ends in a character of two bytes.
    let long_sets: Vec<(String, [u8; 32])> = (0..40u8)
        .map(|n| (format!("{n:02}{}", "x".repeat(28)), [n; 32]))
        .collect();
    let multibyte = vec![(format!("{}é", "a".repeat(23)), [7u8; 32])];
    let sets_of = |refusal: &Refusal| match refusal {
        Refusal::Stances { sets, more } => (sets.clone(), *more),
        other => panic!("not a stance refusal: {other:?}"),
    };
    let tables = [
        ("40 sets", &long_sets, MAX_STANCE_SETS, true),
        ("one multibyte name", &multibyte, 1, false),
    ];
    for (what, host, kept, more) in tables {
        let refusal = Refusal::stances(host);
        let (sets, got_more) = sets_of(&refusal);
        assert_eq!(sets.len(), kept, "{what}: the sets kept");
        assert_eq!(got_more, more, "{what}: the more flag");
        for ((name, digest), (host_name, host_digest)) in sets.iter().zip(host.iter()) {
            assert!(name.len() <= MAX_STANCE_NAME, "{what}: {name} is too long");
            assert!(
                host_name.starts_with(name.as_str()),
                "{what}: {name} is a cut"
            );
            assert_eq!(digest[..], host_digest[..4], "{what}: the digest's start");
        }
        let mut bytes = Vec::new();
        refusal.encode(&mut bytes);
        // The 40-set row is the largest refusal, at 931 bytes.
        assert!(bytes.len() <= 1000, "{what}: {} bytes", bytes.len());
        let mut reader = Reader::new(&bytes);
        assert_eq!(
            Refusal::decode(&mut reader),
            Ok(refusal),
            "{what}: round trip"
        );
        assert_eq!(reader.finish(), Ok(()), "{what}: nothing left over");
    }
    assert_eq!(
        sets_of(&Refusal::stances(&multibyte)).0[0].0,
        "a".repeat(23),
        "a cut never splits a character"
    );

    // What a client tells its player. The host's names arrive cut to 24 bytes.
    let long = "_humanoid_kotor+BastilaShan";
    let cut = &long[..MAX_STANCE_NAME];
    let host_sets = |names: &[(&str, u8)]| -> Vec<(String, [u8; 4])> {
        names
            .iter()
            .map(|(name, digest)| (name.to_string(), [*digest; 4]))
            .collect()
    };
    let here_sets = |names: &[(&str, u8)]| -> Vec<(String, [u8; 32])> {
        names
            .iter()
            .map(|(name, digest)| (name.to_string(), [*digest; 32]))
            .collect()
    };
    let differs = "The host's Movie Duels stance sets differ from this machine's";
    let texts = [
        (
            "no sets here",
            host_sets(&[("alpha", 1)]),
            false,
            here_sets(&[]),
            "The host plays Movie Duels Advanced with Movie Duels' stances. This machine has no Movie Duels, so it cannot join.".to_string(),
        ),
        (
            "one set differs",
            host_sets(&[("alpha", 1), ("beta", 2)]),
            false,
            here_sets(&[("alpha", 9), ("beta", 2)]),
            format!("{differs}: alpha."),
        ),
        (
            "more than fit",
            host_sets(&[("alpha", 1), ("beta", 2)]),
            true,
            here_sets(&[("alpha", 9), ("beta", 2)]),
            format!("{differs}: alpha. There are more."),
        ),
        (
            "a long name with the same digest",
            host_sets(&[(cut, 3)]),
            false,
            here_sets(&[(long, 3), ("beta", 2)]),
            format!("{differs}."),
        ),
        (
            "a long name with another digest",
            host_sets(&[(cut, 3)]),
            false,
            here_sets(&[(long, 4)]),
            format!("{differs}: {cut}."),
        ),
        (
            "nothing named differs",
            host_sets(&[("alpha", 1)]),
            false,
            here_sets(&[("alpha", 1)]),
            format!("{differs}."),
        ),
        (
            "nothing named differs, and more",
            host_sets(&[("alpha", 1)]),
            true,
            here_sets(&[("alpha", 1)]),
            format!("{differs}. There are more."),
        ),
    ];
    for (what, host, more, here, expected) in texts {
        assert_eq!(
            Refusal::stances_text(&host, more, &here),
            expected,
            "{what}"
        );
    }
}
